use crate::rt_data;
use serde_yaml;
use std::collections::HashMap;

#[derive(Debug, serde::Deserialize)]
pub struct TraintimeSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<String>,
}

impl TraintimeSystemConfig {
    fn read_from_config_file(path: &str) -> TraintimeSystemConfig {
        let file = std::fs::File::open(path).unwrap();
        let config: TraintimeSystemConfig = serde_yaml::from_reader(file).unwrap();
        config
    }

    pub fn read_config_by_system(system: SupportedTransitSystem) -> TraintimeSystemConfig {
        let path = match system {
            SupportedTransitSystem::NycSubway => "system_configs/nyc_subway.yml",
        };

        TraintimeSystemConfig::read_from_config_file(path)
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
        let mut relevant_endpoints = Vec::new();
        for endpoint in &system_config.gtfs_rt_endpoints {
            let response = rt_data::fetch_gtfs_rt(endpoint.as_str()).await.unwrap();
            let feed = rt_data::decode_gtfs_rt(response).await.unwrap();
            for route in rt_data::accumulate_routes(feed) {
                if relevant_routes.contains(&route) {
                    relevant_endpoints.push(endpoint.clone());
                    break;
                }
            }
        }

        SelectedStationConfig {
            station_name: station_name.to_string(),
            relevant_endpoints,
        }
    }
}

pub enum SupportedTransitSystem {
    NycSubway,
}
