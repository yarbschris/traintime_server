use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::FeedMessage;
use log::{info, warn};
use prost::bytes::Bytes;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time;

use crate::config;
use crate::error::GtfsRtError;
use crate::types::{static_data::StaticData, traintime_packet};

pub fn setup_gtfs_rt(
    rx_station_config: watch::Receiver<config::SelectedStationConfig>,
    rx_active_static_data: watch::Receiver<StaticData>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(gtfs_rt_handler(
        rx_station_config,
        rx_active_static_data,
        tx_traintime_packets,
    ))
}

async fn gtfs_rt_handler(
    rx_station_config: watch::Receiver<config::SelectedStationConfig>,
    rx_active_static_data: watch::Receiver<StaticData>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) {
    let fetch_interval_seconds = 30;
    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(fetch_interval_seconds));

    let http_client = reqwest::Client::new();

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

        let feed_messages = gtfs_rt_request_handler(relevant_endpoints, &http_client).await;
        let packets = {
            let active_static_data = &rx_active_static_data.borrow();

            let relevant_stop_ids =
                &active_static_data.get_relevant_stops_to_station(target_station_name.0.as_str());

            feed_messages
                .iter()
                .flat_map(|message| {
                    let now = message
                        .header
                        .timestamp
                        .expect("Timestamp missing from feed message, should never happen.");

                    message.entity.iter().filter_map(move |entity| {
                        traintime_packet::feed_entity_to_packet(
                            entity,
                            active_static_data,
                            relevant_stop_ids,
                            &now,
                        )
                    })
                })
                .collect::<Vec<traintime_packet::TraintimePacket>>()
        };

        tx_traintime_packets.send(packets).await.unwrap();
    }
}

// For each endpoint, make a request. Put feed messages together, and return
async fn gtfs_rt_request_handler(
    endpoints: Vec<String>,
    http_client: &reqwest::Client,
) -> Vec<FeedMessage> {
    let futures = endpoints
        .iter()
        .map(|endpoint| fetch_and_decode_gtfs_rt(endpoint, http_client));
    join_all(futures)
        .await
        .into_iter()
        .zip(&endpoints)
        .filter_map(|(result, endpoint)| {
            result
                .inspect_err(|e| warn!("Skipping {endpoint}: {e}"))
                .ok()
        })
        .collect()
}

pub async fn fetch_and_decode_gtfs_rt(
    endpoint: &str,
    http_client: &reqwest::Client,
) -> Result<FeedMessage, GtfsRtError> {
    info!("Fetching and decoding gtfs-rt data");
    let gtfs_rt_bytes = fetch_gtfs_rt(endpoint, http_client).await?;

    info!("Fetched gtfs-rt data");
    let decoded = decode_gtfs_rt(gtfs_rt_bytes).await?;
    info!("Decoded gtfs-rt data");

    Ok(decoded)
}

async fn fetch_gtfs_rt(
    gtfs_rt_endpoint: &str,
    http_client: &reqwest::Client,
) -> Result<Bytes, reqwest::Error> {
    http_client
        .get(gtfs_rt_endpoint)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await
}

async fn decode_gtfs_rt(response_bytes: Bytes) -> Result<FeedMessage, prost::DecodeError> {
    gtfs_rt_decode::decode::from_bytes(response_bytes)
}
