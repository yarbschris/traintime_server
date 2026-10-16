use crate::{
    error::{ConfigError, ResolveError, StartError},
    types::{
        gtfs::{RouteID, StationName, StopID},
        static_data::StaticData,
    },
};
use log::warn;
use tokio::{sync::watch, task::JoinHandle};

const NYC_SUBWAY_CONFIG: &str = include_str!("../system_configs/nyc_subway.yml");
const BOSTON_TRANSIT_CONFIG: &str = include_str!("../system_configs/boston_transit.yml");
const MTA_METRO_NORTH_CONFIG: &str = include_str!("../system_configs/mta_metro_north.yml");

// Create watch channels for configurations, spawn resolve station task
pub fn setup_config(
    system: SupportedTransitSystem,
    rx_static_data: watch::Receiver<StaticData>,
) -> Result<
    (
        watch::Sender<TransitSystemConfig>,
        watch::Receiver<TransitSystemConfig>,
        watch::Sender<SelectedStation>,
        watch::Receiver<Option<ResolvedStation>>,
        JoinHandle<()>,
    ),
    StartError,
> {
    // Watch channel for transit system configuration.
    let (tx_system_config, rx_system_config) =
        watch::channel(TransitSystemConfig::read_config_by_system(&system)?);

    // Watch channel for station config
    let (tx_selected_station, rx_selected_station) = watch::channel(SelectedStation::new(system));

    let (tx_resolved_station, rx_resolved_station) = watch::channel(None);

    let resolve_task_join_handle = tokio::spawn(resolve_station_on_selection_change(
        rx_selected_station,
        rx_static_data,
        rx_system_config.clone(),
        tx_resolved_station,
    ));

    Ok((
        tx_system_config,
        rx_system_config,
        tx_selected_station,
        rx_resolved_station,
        resolve_task_join_handle,
    ))
}

#[derive(Debug, serde::Deserialize)]
pub struct GtfsRtEndpoint {
    pub url: String,
    pub routes: Option<Vec<RouteID>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TransitSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<GtfsRtEndpoint>,
}

impl TransitSystemConfig {
    fn parse_config(config: &str) -> Result<TransitSystemConfig, serde_norway::Error> {
        serde_norway::from_str(config)
    }

    fn validate_config(self) -> Result<TransitSystemConfig, ConfigError> {
        if self.gtfs_rt_endpoints.is_empty() {
            return Err(ConfigError::NoRealtimeEndpoints);
        }
        Ok(self)
    }

    fn from_yaml(s: &str) -> Result<Self, ConfigError> {
        Self::parse_config(s)?.validate_config()
    }

    pub fn read_config_by_system(
        system: &SupportedTransitSystem,
    ) -> Result<TransitSystemConfig, ConfigError> {
        let config_yaml = match system {
            SupportedTransitSystem::NycSubway => NYC_SUBWAY_CONFIG,
            SupportedTransitSystem::BostonTransit => BOSTON_TRANSIT_CONFIG,
            SupportedTransitSystem::MtaMetroNorth => MTA_METRO_NORTH_CONFIG,
        };

        TransitSystemConfig::from_yaml(config_yaml)
    }
}

#[derive(Debug)]
pub struct SelectedStation {
    pub transit_system: SupportedTransitSystem,
    pub station_name: StationName,
}

impl SelectedStation {
    pub fn new(transit_system: SupportedTransitSystem) -> Self {
        SelectedStation {
            transit_system,
            station_name: StationName::new(),
        }
    }
}

#[derive(Clone)]
pub struct ResolvedStation {
    pub routes: Vec<RouteID>,
    pub stop_ids: Vec<StopID>,
    pub relevant_endpoints: Vec<String>,
}

// Spawned Task: Watch for change in selected_station, resolve station.
async fn resolve_station_on_selection_change(
    mut rx_selected_station: watch::Receiver<SelectedStation>,
    mut rx_static_data: watch::Receiver<StaticData>,
    rx_system_config: watch::Receiver<TransitSystemConfig>,
    tx_resolved_station: watch::Sender<Option<ResolvedStation>>,
) {
    loop {
        tokio::select! {
            r = rx_selected_station.changed() => if r.is_err() { break },
            r = rx_static_data.changed() => if r.is_err() {break},
        }

        let selected_station = rx_selected_station.borrow_and_update();
        let static_data = rx_static_data.borrow();
        let system_config = rx_system_config.borrow();
        match resolve_station(&selected_station, &static_data, &system_config) {
            Ok(new_resolved_station) => {
                tx_resolved_station
                    .send_modify(|resolved_station| *resolved_station = Some(new_resolved_station));
            }
            Err(e) => {
                warn!("{e}");
                tx_resolved_station.send_modify(|resolved_station| *resolved_station = None);
            }
        }
    }
}

// Resolve the user's selected station into the data we actually need at runtime
fn resolve_station(
    sel: &SelectedStation,
    static_data: &StaticData,
    system: &TransitSystemConfig,
) -> Result<ResolvedStation, ResolveError> {
    let routes = get_relevant_routes(&sel.station_name, static_data)?;
    let stop_ids = get_stop_ids_by_station_name(&sel.station_name, static_data)?;
    let relevant_endpoints: Vec<String> = system
        .gtfs_rt_endpoints
        .iter()
        .filter(|endpoint| endpoint_is_relevant(endpoint, &routes))
        .map(|endpoint| endpoint.url.clone())
        .collect();

    Ok(ResolvedStation {
        routes,
        stop_ids,
        relevant_endpoints,
    })
}

// Get the routes that run through a station.
pub fn get_relevant_routes(
    station_name: &StationName,
    static_data: &StaticData,
) -> Result<Vec<RouteID>, ResolveError> {
    match static_data.route_lookup.get(station_name) {
        Some(ids) => Ok(ids.clone()),
        None => Err(ResolveError::UnknownStation(station_name.clone())),
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
) -> Result<Vec<StopID>, ResolveError> {
    let matching_entries: Vec<(&StopID, &StationName)> = static_data
        .stop_lookup
        .iter()
        .filter(|(_, name)| *name == station_name)
        .collect();

    if matching_entries.is_empty() {
        return Err(ResolveError::UnknownStation(station_name.clone()));
    }

    Ok(matching_entries
        .iter()
        .map(|(id, _)| (*id).clone())
        .collect())
}

#[derive(Debug, Clone)]
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
    fn two_feed_system() -> TransitSystemConfig {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  \
                    - url: https://example.com/ace\n    routes: [\"A\", \"C\", \"E\"]\n  \
                    - url: https://example.com/bdfm\n    routes: [\"B\", \"D\", \"F\", \"M\"]\n";
        TransitSystemConfig::from_yaml(yaml).unwrap()
    }

    // One station, Delancey St, served by F and M from two platforms
    fn delancey_static_data() -> StaticData {
        let mut static_data = StaticData::new();
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

    fn selection(station_name: &str) -> SelectedStation {
        SelectedStation {
            transit_system: SupportedTransitSystem::NycSubway,
            station_name: StationName::from(station_name),
        }
    }

    #[test]
    fn resolve_station_keeps_only_endpoints_carrying_the_station_routes() {
        let resolved = resolve_station(
            &selection("Delancey St"),
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
            &selection("Not A Station"),
            &delancey_static_data(),
            &two_feed_system(),
        );

        assert!(matches!(
            result,
            Err(ResolveError::UnknownStation(name)) if name == StationName::from("Not A Station")
        ));
    }

    #[test]
    fn resolve_station_fails_before_static_data_has_loaded() {
        // What the resolver sees at boot, before the first static download finishes
        let result = resolve_station(
            &selection("Delancey St"),
            &StaticData::new(),
            &two_feed_system(),
        );

        assert!(matches!(result, Err(ResolveError::UnknownStation(_))));
    }

    #[test]
    fn bundled_system_configs_parse() {
        for system in [
            SupportedTransitSystem::NycSubway,
            SupportedTransitSystem::BostonTransit,
            SupportedTransitSystem::MtaMetroNorth,
        ] {
            let config = TransitSystemConfig::read_config_by_system(&system).unwrap();

            assert!(!config.gtfs_static_endpoint.is_empty());
            assert!(!config.gtfs_rt_endpoints.is_empty());
        }
    }

    #[test]
    fn config_with_one_realtime_endpoint_is_accepted() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  - url: https://example.com/rt\n";

        let config = TransitSystemConfig::from_yaml(yaml).unwrap();

        assert_eq!(config.gtfs_rt_endpoints.len(), 1);
        assert_eq!(config.gtfs_rt_endpoints[0].url, "https://example.com/rt");
        assert!(config.gtfs_rt_endpoints[0].routes.is_none());
    }

    #[test]
    fn endpoint_routes_parse_as_route_ids() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  - url: https://example.com/rt\n    routes: [\"A\", \"6X\"]\n";

        let config = TransitSystemConfig::from_yaml(yaml).unwrap();

        assert_eq!(
            config.gtfs_rt_endpoints[0].routes,
            Some(routes(&["A", "6X"]))
        );
    }

    #[test]
    fn config_without_realtime_endpoints_is_rejected() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints: []\n";

        let result = TransitSystemConfig::from_yaml(yaml);

        assert!(matches!(result, Err(ConfigError::NoRealtimeEndpoints)));
    }

    #[test]
    fn config_missing_the_realtime_endpoints_key_is_a_parse_error() {
        // A missing key is malformed YAML for this struct, not an empty list
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n";

        let result = TransitSystemConfig::from_yaml(yaml);

        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }
}
