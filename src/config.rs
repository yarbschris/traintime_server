use crate::{rt_data, static_data::StaticData};
use futures::future::join_all;
use log::info;
use tokio::sync::watch;

const NYC_SUBWAY_CONFIG: &str = include_str!("../system_configs/nyc_subway.yml");

#[derive(Debug, serde::Deserialize)]
pub struct TraintimeSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<String>,
}

impl TraintimeSystemConfig {
    fn parse_config(config: &str) -> TraintimeSystemConfig {
        serde_norway::from_str(config).unwrap()
    }

    pub fn read_config_by_system(system: SupportedTransitSystem) -> TraintimeSystemConfig {
        let config = match system {
            SupportedTransitSystem::NycSubway => NYC_SUBWAY_CONFIG,
        };

        TraintimeSystemConfig::parse_config(config)
    }
}

#[derive(Debug)]
pub struct SelectedStationConfig {
    pub station_name: String,
    pub relevant_endpoints: Vec<String>,
}

// Update the station config to reflect new relevant endpoint when static data is updated
pub async fn update_endpoints_on_static_data_update(
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
    mut rx_active_static_data: watch::Receiver<StaticData>,
    tx_station_config: watch::Sender<SelectedStationConfig>,
) {
    while rx_active_static_data.changed().await.is_ok() {
        info!("Updating relevant endpoints");
        let relevant_routes = {
            let station_config = tx_station_config.borrow();
            let static_data = rx_active_static_data.borrow_and_update();
            static_data
                .route_lookup
                .get(&station_config.station_name)
                .unwrap()
                .clone()
        };

        let gtfs_rt_endpoints = {
            let system_config = rx_system_config.borrow();
            system_config.gtfs_rt_endpoints.clone()
        };

        let futures = gtfs_rt_endpoints
            .iter()
            .map(|endpoint| determine_endpoint_routes(endpoint.to_owned(), &relevant_routes));

        let relevant_endpoints = join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<String>>();

        tx_station_config.send_modify(|x| x.relevant_endpoints = relevant_endpoints);
        info!("Updated relevant endpoints");
    }
}

impl SelectedStationConfig {
    pub fn new() -> Self {
        SelectedStationConfig {
            station_name: String::new(),
            relevant_endpoints: Vec::new(),
        }
    }
}

impl Default for SelectedStationConfig {
    fn default() -> Self {
        Self::new()
    }
}

async fn determine_endpoint_routes(endpoint: String, relevant_routes: &[String]) -> Option<String> {
    let decoded_entities = rt_data::fetch_and_decode_gtfs_rt(endpoint.clone()).await;
    if rt_data::accumulate_entities_routes(decoded_entities)
        .iter()
        .any(|x| relevant_routes.contains(x))
    {
        Some(endpoint)
    } else {
        None
    }
}

pub enum SupportedTransitSystem {
    NycSubway,
}
