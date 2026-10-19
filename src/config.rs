use crate::{
    error::{StationResolveError, SystemConfigError},
    types::{
        generation::Generation,
        gtfs::{RouteID, StationName, StopID},
        static_data::StaticData,
    },
};
use log::{debug, warn};
use serde::Deserialize;
use tokio::{sync::watch, task::JoinHandle};

const NYC_SUBWAY_CONFIG: &str = include_str!("../system_configs/nyc_subway.yml");
const BOSTON_TRANSIT_CONFIG: &str = include_str!("../system_configs/boston_transit.yml");
const MTA_METRO_NORTH_CONFIG: &str = include_str!("../system_configs/mta_metro_north.yml");

pub struct ResolverChannels {
    pub tx_selection: watch::Sender<Option<Selection>>,
    pub rx_resolved_system_config: watch::Receiver<Option<ResolvedSystemConfig>>,
    pub rx_resolved_station: watch::Receiver<Option<ResolvedStation>>,
}

// Create watch channels for configurations, spawn resolve station task
pub fn setup_config(
    initial_selection: Option<Selection>,
    rx_static_data: watch::Receiver<Option<StaticData>>,
) -> (ResolverChannels, JoinHandle<()>) {
    // Watch channel for station config
    let (tx_selection, rx_selection) = watch::channel(initial_selection);

    // Watch channel for transit system configuration.
    let (tx_resolved_system_config, rx_resolved_system_config) = watch::channel(None);

    let (tx_resolved_station, rx_resolved_station) = watch::channel(None);

    let resolve_task_join_handle = tokio::spawn(resolve_station_on_selection_change(
        rx_selection,
        rx_static_data,
        tx_resolved_system_config.clone(),
        tx_resolved_station,
    ));

    let resolver_channels = ResolverChannels {
        tx_selection,
        rx_resolved_system_config,
        rx_resolved_station,
    };

    (resolver_channels, resolve_task_join_handle)
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct GtfsRtEndpoint {
    pub url: String,
    pub routes: Option<Vec<RouteID>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedSystemConfig {
    supported_system: SupportedTransitSystem,
    gtfs_static_endpoint: String,
    gtfs_rt_endpoints: Vec<GtfsRtEndpoint>,
}

impl ResolvedSystemConfig {
    pub fn from_system(system: &SupportedTransitSystem) -> Result<Self, SystemConfigError> {
        Self::from_yaml(system, bundled_config_yaml(system))
    }

    fn from_yaml(system: &SupportedTransitSystem, yaml: &str) -> Result<Self, SystemConfigError> {
        let raw = RawSystemConfig::parse_config(yaml)?;
        ResolvedSystemConfig {
            supported_system: *system,
            gtfs_static_endpoint: raw.gtfs_static_endpoint,
            gtfs_rt_endpoints: raw.gtfs_rt_endpoints,
        }
        .validate_config()
    }

    fn validate_config(self) -> Result<ResolvedSystemConfig, SystemConfigError> {
        if self.gtfs_rt_endpoints.is_empty() {
            return Err(SystemConfigError::NoRealtimeEndpoints);
        }
        if self.gtfs_rt_endpoints.len() > 1 {
            for endpoint in &self.gtfs_rt_endpoints {
                if endpoint.routes.is_none() {
                    return Err(SystemConfigError::NoRoutesList(endpoint.url.clone()));
                }
            }
        }
        Ok(self)
    }

    pub fn supported_system(&self) -> &SupportedTransitSystem {
        &self.supported_system
    }

    pub fn gtfs_static_endpoint(&self) -> &str {
        &self.gtfs_static_endpoint
    }

    pub fn gtfs_rt_endpoints(&self) -> &[GtfsRtEndpoint] {
        &self.gtfs_rt_endpoints
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawSystemConfig {
    gtfs_static_endpoint: String,
    gtfs_rt_endpoints: Vec<GtfsRtEndpoint>,
}

impl RawSystemConfig {
    fn parse_config(config: &str) -> Result<RawSystemConfig, serde_norway::Error> {
        serde_norway::from_str(config)
    }
}

fn bundled_config_yaml(system: &SupportedTransitSystem) -> &'static str {
    match system {
        SupportedTransitSystem::NycSubway => NYC_SUBWAY_CONFIG,
        SupportedTransitSystem::BostonTransit => BOSTON_TRANSIT_CONFIG,
        SupportedTransitSystem::MtaMetroNorth => MTA_METRO_NORTH_CONFIG,
    }
}

#[derive(Debug)]
pub struct Selection {
    pub transit_system: SupportedTransitSystem,
    pub station_name: Option<StationName>,
}

impl Selection {
    pub fn new(transit_system: SupportedTransitSystem) -> Self {
        Selection {
            transit_system,
            station_name: None,
        }
    }
}

#[derive(Clone)]
pub struct ResolvedStation {
    pub routes: Vec<RouteID>,
    pub stop_ids: Vec<StopID>,
    pub relevant_endpoints: Vec<String>,
    // The generation of the `StaticData` this station was resolved against. If it differs from
    // the current static data's generation, the static data was rebuilt since this resolve
    pub generation: Generation,
}

// Spawned Task: Watch for change in selected_station, resolve system and station.
async fn resolve_station_on_selection_change(
    mut rx_selection: watch::Receiver<Option<Selection>>,
    mut rx_static_data: watch::Receiver<Option<StaticData>>,
    tx_resolved_system_config: watch::Sender<Option<ResolvedSystemConfig>>,
    tx_resolved_station: watch::Sender<Option<ResolvedStation>>,
) {
    // On startup right now, we set an initial value, we want to resolve immediately so that
    // static refresh task and resolve task do not hang waiting for each other
    rx_selection.mark_changed();

    loop {
        tokio::select! {
            r = rx_selection.changed() => if r.is_err() { break },
            r = rx_static_data.changed() => if r.is_err() { break },
        }

        let selection_option = rx_selection.borrow_and_update();
        let Some(selection) = selection_option.as_ref() else {
            debug!("No selection");
            // Clear both resolved channels for consistency, only notify receivers if channel held
            // `Some`
            tx_resolved_system_config.send_if_modified(|c| c.take().is_some());
            tx_resolved_station.send_if_modified(|c| c.take().is_some());
            continue;
        };
        if resolve_needs_system_config_refresh(selection, &tx_resolved_system_config) {
            match resolve_system(selection) {
                Ok(resolved_system_config) => {
                    tx_resolved_system_config.send_replace(Some(resolved_system_config));
                }
                Err(e) => {
                    warn!("{e}");
                    tx_resolved_system_config.send_replace(None);
                }
            }
        }

        let resolved = try_resolve_station(&rx_static_data, &tx_resolved_system_config, selection);
        tx_resolved_station.send_if_modified(|current| {
            if current.is_none() && resolved.is_none() {
                false
            } else {
                *current = resolved;
                true
            }
        });
    }
}

// We must update the resolved system config during refresh if the resolved system config watch
// channel currently holds None or the resolved system config's system is different from user selection
fn resolve_needs_system_config_refresh(
    selection: &Selection,
    tx_resolved_system_config: &watch::Sender<Option<ResolvedSystemConfig>>,
) -> bool {
    tx_resolved_system_config
        .borrow()
        .as_ref()
        .is_none_or(|resolved_system_config| {
            resolved_system_config.supported_system != selection.transit_system
        })
}

// Validate the necessary conditions for resolving station, return None otherwise
fn try_resolve_station(
    rx_static_data: &watch::Receiver<Option<StaticData>>,
    tx_resolved_system_config: &watch::Sender<Option<ResolvedSystemConfig>>,
    selection: &Selection,
) -> Option<ResolvedStation> {
    let Some(station_name) = selection.station_name.as_ref() else {
        debug!("Cannot resolve station: no station selected");
        return None;
    };
    let static_data_ref = rx_static_data.borrow();
    let Some(static_data) = static_data_ref.as_ref() else {
        debug!("Cannot resolve station: no static data loaded");
        return None;
    };
    let resolved_system_config_ref = tx_resolved_system_config.borrow();
    let Some(resolved_system_config) = resolved_system_config_ref.as_ref() else {
        debug!("Cannot resolve station: no system config loaded");
        return None;
    };
    if static_data.built_from_system != resolved_system_config.supported_system {
        debug!("Cannot resolve station: static data is for a different system");
        return None;
    }

    match resolve_station(station_name, static_data, resolved_system_config) {
        Ok(new_resolved_station) => Some(new_resolved_station),
        Err(e) => {
            warn!("{e}");
            None
        }
    }
}

fn resolve_system(sel: &Selection) -> Result<ResolvedSystemConfig, SystemConfigError> {
    ResolvedSystemConfig::from_system(&sel.transit_system)
}

// Resolve the user's selected station into the data we actually need at runtime
fn resolve_station(
    station_name: &StationName,
    static_data: &StaticData,
    system: &ResolvedSystemConfig,
) -> Result<ResolvedStation, StationResolveError> {
    let routes = get_relevant_routes(station_name, static_data)?;
    let stop_ids = get_stop_ids_by_station_name(station_name, static_data)?;
    let relevant_endpoints: Vec<String> = system
        .gtfs_rt_endpoints
        .iter()
        .filter(|endpoint| endpoint_is_relevant(endpoint, &routes))
        .map(|endpoint| endpoint.url.clone())
        .collect();

    if relevant_endpoints.is_empty() {
        return Err(StationResolveError::EndpointsDoNotCoverAnyRoutes(routes));
    }

    Ok(ResolvedStation {
        routes,
        stop_ids,
        relevant_endpoints,
        generation: static_data.generation, // Derive generation from StaticData
    })
}

// Get the routes that run through a station.
pub fn get_relevant_routes(
    station_name: &StationName,
    static_data: &StaticData,
) -> Result<Vec<RouteID>, StationResolveError> {
    match static_data.route_lookup.get(station_name) {
        Some(ids) => Ok(ids.clone()),
        None => Err(StationResolveError::UnknownStation(station_name.clone())),
    }
}

// Given a gtfs-rt endpoint, return true if that endpoint points to any relevant routes
fn endpoint_is_relevant(endpoint: &GtfsRtEndpoint, relevant_routes: &[RouteID]) -> bool {
    match &endpoint.routes {
        None => true,
        Some(endpoint_routes) => relevant_routes.iter().any(|relevant_route| {
            endpoint_routes
                .iter()
                .any(|endpoint_route| endpoint_route == relevant_route)
        }),
    }
}

// Given station name, search static_data's stop_lookup for matching entires and return a vector of
// those stop_id's
pub fn get_stop_ids_by_station_name(
    station_name: &StationName,
    static_data: &StaticData,
) -> Result<Vec<StopID>, StationResolveError> {
    let matching_entries: Vec<(&StopID, &StationName)> = static_data
        .stop_lookup
        .iter()
        .filter(|(_, name)| *name == station_name)
        .collect();

    if matching_entries.is_empty() {
        return Err(StationResolveError::UnknownStation(station_name.clone()));
    }

    Ok(matching_entries
        .iter()
        .map(|(id, _)| (*id).clone())
        .collect())
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum SupportedTransitSystem {
    NycSubway,
    BostonTransit,
    MtaMetroNorth,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routes(route_ids: &[&str]) -> Vec<RouteID> {
        route_ids
            .iter()
            .map(|route_id| RouteID(String::from(*route_id)))
            .collect()
    }

    fn endpoint(routes: Option<Vec<RouteID>>) -> GtfsRtEndpoint {
        GtfsRtEndpoint {
            url: String::from("https://example.com/rt"),
            routes,
        }
    }

    #[test]
    fn endpoint_is_relevant_when_it_carries_one_of_the_station_routes() {
        let endpoint = endpoint(Some(routes(&["A", "C", "E"])));

        assert!(endpoint_is_relevant(&endpoint, &routes(&["F", "E"])));
    }

    #[test]
    fn endpoint_is_not_relevant_when_it_carries_none_of_the_station_routes() {
        let endpoint = endpoint(Some(routes(&["A", "C", "E"])));

        assert!(!endpoint_is_relevant(&endpoint, &routes(&["F", "M"])));
        assert!(!endpoint_is_relevant(&endpoint, &[]));
    }

    #[test]
    fn endpoint_without_a_route_list_carries_everything() {
        // Single-feed systems (Metro North, Boston) don't list routes
        let endpoint = endpoint(None);

        assert!(endpoint_is_relevant(&endpoint, &routes(&["F"])));
    }

    // Two feeds split by route, like NYC's
    fn two_feed_system() -> ResolvedSystemConfig {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  \
                    - url: https://example.com/ace\n    routes: [\"A\", \"C\", \"E\"]\n  \
                    - url: https://example.com/bdfm\n    routes: [\"B\", \"D\", \"F\", \"M\"]\n";
        ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml).unwrap()
    }

    // One station, Delancey St, served by F and M from two platforms
    fn delancey_static_data() -> StaticData {
        let mut static_data = StaticData::empty(SupportedTransitSystem::NycSubway);
        for stop_id in ["F15N", "F15S"] {
            static_data.stop_lookup.insert(
                StopID(String::from(stop_id)),
                StationName::from("Delancey St"),
            );
        }
        static_data
            .route_lookup
            .insert(StationName::from("Delancey St"), routes(&["F", "M"]));
        static_data
    }

    fn selection(station_name: &str) -> Selection {
        Selection {
            transit_system: SupportedTransitSystem::NycSubway,
            station_name: Some(StationName::from(station_name)),
        }
    }

    #[test]
    fn resolve_station_keeps_only_endpoints_carrying_the_station_routes() {
        let resolved = resolve_station(
            &StationName::from("Delancey St"),
            &delancey_static_data(),
            &two_feed_system(),
        )
        .unwrap();

        let mut stop_ids: Vec<&str> = resolved.stop_ids.iter().map(|id| id.0.as_str()).collect();
        stop_ids.sort();

        assert_eq!(resolved.relevant_endpoints, ["https://example.com/bdfm"]);
        assert_eq!(resolved.routes, routes(&["F", "M"]));
        assert_eq!(stop_ids, ["F15N", "F15S"]);
    }

    #[test]
    fn resolve_station_fails_for_an_unknown_station() {
        let result = resolve_station(
            &StationName::from("Not A Station"),
            &delancey_static_data(),
            &two_feed_system(),
        );

        assert!(matches!(
            result,
            Err(StationResolveError::UnknownStation(name)) if name == StationName::from("Not A Station")
        ));
    }

    // The channel contents the resolver task passes to try_resolve_station
    fn resolver_inputs(
        static_data: Option<StaticData>,
        system_config: Option<ResolvedSystemConfig>,
    ) -> (
        watch::Receiver<Option<StaticData>>,
        watch::Sender<Option<ResolvedSystemConfig>>,
    ) {
        let (_, rx_static_data) = watch::channel(static_data);
        let (tx_system_config, _) = watch::channel(system_config);
        (rx_static_data, tx_system_config)
    }

    #[test]
    fn try_resolve_station_resolves_when_static_data_matches_the_system() {
        let (rx_static_data, tx_system_config) =
            resolver_inputs(Some(delancey_static_data()), Some(two_feed_system()));

        let resolved = try_resolve_station(
            &rx_static_data,
            &tx_system_config,
            &selection("Delancey St"),
        );

        assert!(resolved.is_some());
    }

    #[test]
    fn try_resolve_station_waits_for_a_station_when_only_a_system_is_selected() {
        // A system without a station is a normal state (the CLI selects one to load the station
        // list), so it resolves to None even when everything else is ready
        let (rx_static_data, tx_system_config) =
            resolver_inputs(Some(delancey_static_data()), Some(two_feed_system()));

        let resolved = try_resolve_station(
            &rx_static_data,
            &tx_system_config,
            &Selection::new(SupportedTransitSystem::NycSubway),
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_station_waits_for_static_data() {
        // What the resolver sees at boot, before the first static download finishes
        let (rx_static_data, tx_system_config) = resolver_inputs(None, Some(two_feed_system()));

        let resolved = try_resolve_station(
            &rx_static_data,
            &tx_system_config,
            &selection("Delancey St"),
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_station_waits_for_a_system_config() {
        let (rx_static_data, tx_system_config) =
            resolver_inputs(Some(delancey_static_data()), None);

        let resolved = try_resolve_station(
            &rx_static_data,
            &tx_system_config,
            &selection("Delancey St"),
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn try_resolve_station_refuses_static_data_from_another_system() {
        // Right after a system switch, before the new system's static data lands. The station
        // exists in the old data, so only the system check stops it resolving there
        let mut static_data = delancey_static_data();
        static_data.built_from_system = SupportedTransitSystem::MtaMetroNorth;
        let (rx_static_data, tx_system_config) =
            resolver_inputs(Some(static_data), Some(two_feed_system()));

        let resolved = try_resolve_station(
            &rx_static_data,
            &tx_system_config,
            &selection("Delancey St"),
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn bundled_system_configs_parse() {
        for system in [
            SupportedTransitSystem::NycSubway,
            SupportedTransitSystem::BostonTransit,
            SupportedTransitSystem::MtaMetroNorth,
        ] {
            let config = ResolvedSystemConfig::from_system(&system).unwrap();

            assert!(!config.gtfs_static_endpoint.is_empty());
            assert!(!config.gtfs_rt_endpoints.is_empty());
        }
    }

    #[test]
    fn config_with_one_realtime_endpoint_is_accepted() {
        // A single endpoint may leave out routes, since it carries everything
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  - url: https://example.com/rt\n";

        let config =
            ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml).unwrap();

        assert_eq!(config.gtfs_rt_endpoints.len(), 1);
        assert_eq!(config.gtfs_rt_endpoints[0].url, "https://example.com/rt");
        assert!(config.gtfs_rt_endpoints[0].routes.is_none());
    }

    #[test]
    fn config_with_several_endpoints_rejects_one_without_routes() {
        // The second endpoint is the one missing routes, so a check that only looked at the first
        // would pass this config
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  \
                    - url: https://example.com/ace\n    routes: [\"A\", \"C\", \"E\"]\n  \
                    - url: https://example.com/bdfm\n";

        let result = ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml);

        assert!(matches!(
            result,
            Err(SystemConfigError::NoRoutesList(url)) if url == "https://example.com/bdfm"
        ));
    }

    #[test]
    fn config_with_several_endpoints_all_with_routes_is_accepted() {
        let config = two_feed_system();

        assert_eq!(config.gtfs_rt_endpoints.len(), 2);
        assert!(
            config
                .gtfs_rt_endpoints
                .iter()
                .all(|endpoint| endpoint.routes.is_some())
        );
    }

    #[test]
    fn endpoint_routes_parse_as_route_ids() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  - url: https://example.com/rt\n    routes: [\"A\", \"6X\"]\n";

        let config =
            ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml).unwrap();

        assert_eq!(
            config.gtfs_rt_endpoints[0].routes,
            Some(routes(&["A", "6X"]))
        );
    }

    #[test]
    fn config_without_realtime_endpoints_is_rejected() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints: []\n";

        let result = ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml);

        assert!(matches!(
            result,
            Err(SystemConfigError::NoRealtimeEndpoints)
        ));
    }

    #[test]
    fn config_missing_the_realtime_endpoints_key_is_a_parse_error() {
        // A missing key is malformed YAML for this struct, not an empty list
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n";

        let result = ResolvedSystemConfig::from_yaml(&SupportedTransitSystem::NycSubway, yaml);

        assert!(matches!(result, Err(SystemConfigError::Parse(_))));
    }
}
