use crate::rt_data;
use futures::future::join_all;
use serde_norway;
use std::collections::HashMap;

const NYC_SUBWAY_CONFIG_PATH: &str = include_str!("../system_configs/nyc_subway.yml");

#[derive(Debug, serde::Deserialize)]
pub struct TraintimeSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<String>,
}

impl TraintimeSystemConfig {
    fn parse_config(config: &str) -> TraintimeSystemConfig {
        let parsed_config: TraintimeSystemConfig = serde_norway::from_str(config).unwrap();
        parsed_config
    }

    pub fn read_config_by_system(system: SupportedTransitSystem) -> TraintimeSystemConfig {
        let config = match system {
            SupportedTransitSystem::NycSubway => NYC_SUBWAY_CONFIG_PATH,
        };

        TraintimeSystemConfig::parse_config(config)
    }
}

#[derive(Debug)]
pub struct SelectedStationConfig {
    pub station_name: String,
    pub relevant_endpoints: Vec<String>,
}

impl SelectedStationConfig {
    pub async fn build(
        station_name: &str,
        system_config: &TraintimeSystemConfig,
        route_lookup: &HashMap<String, Vec<String>>,
    ) -> SelectedStationConfig {
        let relevant_routes = route_lookup.get(station_name).unwrap();

        let futures = system_config
            .gtfs_rt_endpoints
            .iter()
            .map(|endpoint| determine_endpoint_routes(endpoint, relevant_routes));

        let relevant_endpoints = join_all(futures)
            .await
            .drain(..)
            .flatten()
            .collect::<Vec<String>>();

        SelectedStationConfig {
            station_name: station_name.to_string(),
            relevant_endpoints,
        }
    }
}

async fn determine_endpoint_routes(
    endpoint: &String,
    relevant_routes: &[String],
) -> Option<String> {
    let decoded_entities = rt_data::fetch_and_decode_gtfs_rt(endpoint.clone()).await;
    if rt_data::accumulate_entities_routes(decoded_entities)
        .iter()
        .any(|x| relevant_routes.contains(x))
    {
        Some(endpoint.to_owned())
    } else {
        None
    }
}

pub enum SupportedTransitSystem {
    NycSubway,
}
