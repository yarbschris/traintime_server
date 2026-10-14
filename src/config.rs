use crate::{
    error::ConfigError,
    rt_funcs,
    types::{
        gtfs::{self, StationName},
        static_data::StaticData,
    },
};
use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::FeedEntity;
use log::{info, warn};
use tokio::sync::watch;

const NYC_SUBWAY_CONFIG: &str = include_str!("../system_configs/nyc_subway.yml");
const BOSTON_TRANSIT_CONFIG: &str = include_str!("../system_configs/boston_transit.yml");
const MTA_METRO_NORTH_CONFIG: &str = include_str!("../system_configs/mta_metro_north.yml");

#[derive(Debug, serde::Deserialize)]
pub struct TraintimeSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<String>,
}

impl TraintimeSystemConfig {
    fn parse_config(config: &str) -> Result<TraintimeSystemConfig, serde_norway::Error> {
        serde_norway::from_str(config)
    }

    fn validate_config(self) -> Result<TraintimeSystemConfig, ConfigError> {
        if self.gtfs_rt_endpoints.is_empty() {
            return Err(ConfigError::NoRealtimeEndpoints);
        }
        Ok(self)
    }

    fn from_yaml(s: &str) -> Result<Self, ConfigError> {
        Self::parse_config(s)?.validate_config()
    }

    pub fn read_config_by_system(
        system: SupportedTransitSystem,
    ) -> Result<TraintimeSystemConfig, ConfigError> {
        let config_yaml = match system {
            SupportedTransitSystem::NycSubway => NYC_SUBWAY_CONFIG,
            SupportedTransitSystem::BostonTransit => BOSTON_TRANSIT_CONFIG,
            SupportedTransitSystem::MtaMetroNorth => MTA_METRO_NORTH_CONFIG,
        };

        TraintimeSystemConfig::from_yaml(config_yaml)
    }
}

#[derive(Debug)]
pub struct SelectedStationConfig {
    pub station_name: gtfs::StationName,
    pub relevant_endpoints: Vec<String>,
}

// Update the station config to reflect new relevant endpoint when static data is updated
pub async fn update_endpoints_on_static_data_update(
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
    mut rx_active_static_data: watch::Receiver<StaticData>,
    tx_station_config: watch::Sender<SelectedStationConfig>,
) {
    let http_client = reqwest::Client::new();

    while rx_active_static_data.changed().await.is_ok() {
        let station_name = { tx_station_config.borrow().station_name.clone() };
        let new_relevant_endpoints = SelectedStationConfig::determine_relevant_endpoints(
            &station_name,
            rx_system_config.clone(),
            rx_active_static_data.clone(),
            &http_client,
        )
        .await;

        tx_station_config.send_modify(|station_config: &mut SelectedStationConfig| {
            station_config.relevant_endpoints = new_relevant_endpoints
        });
    }
}

impl SelectedStationConfig {
    pub fn new() -> Self {
        SelectedStationConfig {
            station_name: gtfs::StationName::new(),
            relevant_endpoints: Vec::new(),
        }
    }

    // Get the routes that run through a station.
    pub fn get_relevant_routes(
        station_name: &StationName,
        mut rx_active_static_data: watch::Receiver<StaticData>,
    ) -> Option<Vec<gtfs::RouteID>> {
        let static_data = rx_active_static_data.borrow_and_update();
        static_data.route_lookup.get(station_name).cloned()
    }

    // Get endpoints relevant to the station name. This should be done when 1) New static data is fetched,
    // and 2) The station_name is updated.
    pub async fn determine_relevant_endpoints(
        station_name: &StationName,
        rx_system_config: watch::Receiver<TraintimeSystemConfig>,
        rx_active_static_data: watch::Receiver<StaticData>,
        http_client: &reqwest::Client,
    ) -> Vec<String> {
        info!("Updating relevant endpoints");
        let gtfs_rt_endpoints = { rx_system_config.borrow().gtfs_rt_endpoints.clone() };
        let num_rt_endpoints = gtfs_rt_endpoints.len();
        // No endpoints is caught at config loading, so we do not need to check here

        if num_rt_endpoints == 1 {
            return gtfs_rt_endpoints;
        }
        match SelectedStationConfig::get_relevant_routes(
            station_name,
            rx_active_static_data.clone(),
        ) {
            Some(relevant_routes) => {
                let futures = gtfs_rt_endpoints.iter().map(|endpoint| {
                    determine_if_endpoint_is_relevant(
                        endpoint.to_owned(),
                        &relevant_routes,
                        http_client,
                    )
                });

                join_all(futures)
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<Vec<String>>()
            }
            None => {
                warn!(
                    "No relevant routes found for this station, please check data source. Using all provided endpoints."
                );
                gtfs_rt_endpoints
            }
        }
    }
}

impl Default for SelectedStationConfig {
    fn default() -> Self {
        Self::new()
    }
}

// Given a gtfs-rt endpoint, return Some(endpoint) if that endpoint returns entities that pertain to
// any relevant routes, else None
async fn determine_if_endpoint_is_relevant(
    endpoint: String,
    relevant_routes: &[gtfs::RouteID],
    http_client: &reqwest::Client,
) -> Option<String> {
    let decoded_entities = rt_funcs::fetch_and_decode_gtfs_rt(endpoint.as_str(), http_client)
        .await
        .inspect_err(|e| warn!("Fetch / Decode Error in determine_if_endpoint_is_relevant: {e}"))
        .ok()?
        .entity;

    if endpoint_matches(&decoded_entities, relevant_routes) {
        Some(endpoint)
    } else {
        None
    }
}

// See if a collection of feed entities contains an entity that belongs to a route in relevant_routes
pub fn endpoint_matches(entities: &[FeedEntity], relevant_routes: &[gtfs::RouteID]) -> bool {
    entities
        .iter()
        .filter_map(|entity| entity.trip_update.as_ref()?.trip.route_id.as_deref())
        .any(|route| {
            relevant_routes
                .iter()
                .any(|relevant_route| relevant_route.0 == route)
        })
}

#[allow(unused)]
pub enum SupportedTransitSystem {
    NycSubway,
    BostonTransit,
    MtaMetroNorth,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtfs_rt_decode::gtfs_rt_types::{TripDescriptor, TripUpdate};

    fn trip_update_entity(route_id: Option<&str>) -> FeedEntity {
        FeedEntity {
            trip_update: Some(TripUpdate {
                trip: TripDescriptor {
                    route_id: route_id.map(String::from),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn routes(route_ids: &[&str]) -> Vec<gtfs::RouteID> {
        route_ids
            .iter()
            .map(|route_id| gtfs::RouteID(String::from(*route_id)))
            .collect()
    }

    #[test]
    fn endpoint_matches_when_an_entity_is_on_a_relevant_route() {
        let entities = [trip_update_entity(Some("A")), trip_update_entity(Some("F"))];

        assert!(endpoint_matches(&entities, &routes(&["F", "M"])));
    }

    #[test]
    fn endpoint_does_not_match_when_no_entity_is_on_a_relevant_route() {
        let entities = [trip_update_entity(Some("A")), trip_update_entity(Some("C"))];

        assert!(!endpoint_matches(&entities, &routes(&["F", "M"])));
    }

    #[test]
    fn endpoint_does_not_match_entities_without_a_route() {
        // An entity with no trip update (vehicle position, alert) and one with no route_id
        let entities = [FeedEntity::default(), trip_update_entity(None)];

        assert!(!endpoint_matches(&entities, &routes(&["F"])));
    }

    #[test]
    fn endpoint_does_not_match_without_entities_or_relevant_routes() {
        assert!(!endpoint_matches(&[], &routes(&["F"])));
        assert!(!endpoint_matches(&[trip_update_entity(Some("F"))], &[]));
    }

    #[test]
    fn bundled_system_configs_parse() {
        for system in [
            SupportedTransitSystem::NycSubway,
            SupportedTransitSystem::BostonTransit,
            SupportedTransitSystem::MtaMetroNorth,
        ] {
            let config = TraintimeSystemConfig::read_config_by_system(system).unwrap();

            assert!(!config.gtfs_static_endpoint.is_empty());
            assert!(!config.gtfs_rt_endpoints.is_empty());
        }
    }

    #[test]
    fn config_with_one_realtime_endpoint_is_accepted() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints:\n  - https://example.com/rt\n";

        let config = TraintimeSystemConfig::from_yaml(yaml).unwrap();

        assert_eq!(config.gtfs_rt_endpoints, vec!["https://example.com/rt"]);
    }

    #[test]
    fn config_without_realtime_endpoints_is_rejected() {
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n\
                    gtfs_rt_endpoints: []\n";

        let result = TraintimeSystemConfig::from_yaml(yaml);

        assert!(matches!(result, Err(ConfigError::NoRealtimeEndpoints)));
    }

    #[test]
    fn config_missing_the_realtime_endpoints_key_is_a_parse_error() {
        // A missing key is malformed YAML for this struct, not an empty list
        let yaml = "gtfs_static_endpoint: https://example.com/static.zip\n";

        let result = TraintimeSystemConfig::from_yaml(yaml);

        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }
}
