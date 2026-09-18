use crate::{
    rt_funcs,
    types::{
        gtfs::{self, StationName},
        static_data::StaticData,
    },
};
use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::FeedEntity;
use itertools::Itertools;
use log::info;
use tokio::sync::watch;

const NYC_SUBWAY_CONFIG: &str = include_str!("../system_configs/nyc_subway.yml");
const BOSTON_TRANSIT_CONFIG: &str = include_str!("../system_configs/boston_transit.yml");

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
            SupportedTransitSystem::BostonTransit => BOSTON_TRANSIT_CONFIG,
        };

        TraintimeSystemConfig::parse_config(config)
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
    while rx_active_static_data.changed().await.is_ok() {
        let station_name = { tx_station_config.borrow().station_name.clone() };
        let new_relevant_endpoints = SelectedStationConfig::determine_relevant_endpoints(
            &station_name,
            rx_system_config.clone(),
            rx_active_static_data.clone(),
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
    ) -> Vec<gtfs::RouteID> {
        let static_data = rx_active_static_data.borrow_and_update();
        static_data
            .route_lookup
            .get(station_name)
            .expect("No relevant routes found, please check config")
            .clone()
    }

    // Get endpoints relevant to the station name. This should be done when 1) New static data is fetched,
    // and 2) The station_name is updated.
    pub async fn determine_relevant_endpoints(
        station_name: &StationName,
        rx_system_config: watch::Receiver<TraintimeSystemConfig>,
        rx_active_static_data: watch::Receiver<StaticData>,
    ) -> Vec<String> {
        info!("Updating relevant endpoints");
        let relevant_routes = {
            SelectedStationConfig::get_relevant_routes(station_name, rx_active_static_data.clone())
        };

        let gtfs_rt_endpoints = {
            let system_config = rx_system_config.borrow();
            system_config.gtfs_rt_endpoints.clone()
        };

        let futures = gtfs_rt_endpoints.iter().map(|endpoint| {
            determine_if_endpoint_is_relevant(endpoint.to_owned(), &relevant_routes)
        });

        join_all(futures)
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<String>>()
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
) -> Option<String> {
    let decoded_entities = rt_funcs::fetch_and_decode_gtfs_rt(endpoint.as_str()).await;
    if accumulate_entities_routes(decoded_entities)
        .iter()
        .any(|x| relevant_routes.contains(x))
    {
        Some(endpoint)
    } else {
        None
    }
}

// Given a vector of feed entities, return a vector of all the RouteIDs contained in those feeds
pub fn accumulate_entities_routes(entities: Vec<FeedEntity>) -> Vec<gtfs::RouteID> {
    entities.into_iter().fold(Vec::new(), |mut acc, entity| {
        if let Some(update) = entity.trip_update
            && let Some(route) = update.trip.route_id
        {
            acc.push(gtfs::RouteID(route));
        }
        acc.into_iter().unique().collect()
    })
}

#[allow(unused)]
pub enum SupportedTransitSystem {
    NycSubway,
    BostonTransit,
}
