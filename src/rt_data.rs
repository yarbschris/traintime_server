use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::{FeedEntity, FeedMessage};
use itertools::Itertools;
use log::{info, warn};
use reqwest::Response;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time;

use crate::{config, static_data, traintime_packet};

pub async fn gtfs_rt_handler(
    rx_station_config: watch::Receiver<config::SelectedStationConfig>,
    rx_active_static_data: watch::Receiver<static_data::StaticData>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) {
    let fetch_interval_seconds = 30;
    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(fetch_interval_seconds));

    loop {
        gtfs_rt_fetch_interval.tick().await;

        let (target_station_name, relevant_endpoints) = {
            let station_config = rx_station_config.borrow();
            (
                station_config.station_name.clone(),
                station_config.relevant_endpoints.clone(),
            )
        };
        if relevant_endpoints.is_empty() {
            warn!(
                "No relevant endpoints found. Retrying in {} seconds...",
                fetch_interval_seconds
            );
            continue;
        }

        let entities = gtfs_rt_request_handler(relevant_endpoints).await;
        let packets = {
            let active_static_data = rx_active_static_data.borrow();
            let relevant_stop_ids =
                active_static_data.get_relevant_stops_to_station(&target_station_name);

            entities
                .iter()
                .filter_map(|entity| {
                    traintime_packet::feed_entity_to_packet(
                        entity,
                        &active_static_data,
                        &relevant_stop_ids,
                    )
                })
                .collect::<Vec<traintime_packet::TraintimePacket>>()
        };

        tx_traintime_packets.send(packets).await.unwrap();
    }
}

// For each endpoint, make a request. Put feed messages together, and return
pub async fn gtfs_rt_request_handler(endpoints: Vec<String>) -> Vec<FeedEntity> {
    let futures = endpoints
        .iter()
        .map(|endpoint| fetch_and_decode_gtfs_rt(endpoint));
    join_all(futures).await.concat()
}

pub async fn fetch_and_decode_gtfs_rt(endpoint: &str) -> Vec<FeedEntity> {
    info!("Fetching and decoding gtfs-rt data");
    let Ok(response) = fetch_gtfs_rt(endpoint).await else {
        panic!("Error Fetching GTFS-RT");
    };
    info!("Fetched gtfs-rt data");
    let Ok(decoded) = decode_gtfs_rt(response).await else {
        panic!("Error Decoding GTFS-RT");
    };
    info!("Decoded gtfs-rt data");
    decoded.entity
}

async fn fetch_gtfs_rt(gtfs_rt_endpoint: &str) -> Result<Response, reqwest::Error> {
    reqwest::get(gtfs_rt_endpoint).await
}

async fn decode_gtfs_rt(response: Response) -> Result<FeedMessage, prost::DecodeError> {
    let response_bytes = response.bytes().await.unwrap();
    gtfs_rt_decode::decode::from_bytes(response_bytes)
}

pub fn accumulate_entities_routes(entities: Vec<FeedEntity>) -> Vec<String> {
    entities.into_iter().fold(Vec::new(), |mut acc, entity| {
        if let Some(update) = entity.trip_update
            && let Some(route) = update.trip.route_id
        {
            acc.push(route);
        }
        acc.into_iter().unique().collect()
    })
}
