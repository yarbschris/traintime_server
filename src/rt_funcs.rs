use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::FeedMessage;
use log::{error, info, warn};
use prost::bytes::Bytes;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time;

use crate::config;
use crate::error::GtfsRtError;
use crate::types::traintime_packet::TraintimePacket;
use crate::types::{static_data::StaticData, traintime_packet};

pub fn setup_gtfs_rt(
    rx_resolved_station: watch::Receiver<Option<config::ResolvedStation>>,
    rx_active_static_data: watch::Receiver<StaticData>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(gtfs_rt_handler(
        rx_resolved_station,
        rx_active_static_data,
        tx_traintime_packets,
    ))
}

async fn gtfs_rt_handler(
    mut rx_resolved_station: watch::Receiver<Option<config::ResolvedStation>>,
    rx_active_static_data: watch::Receiver<StaticData>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) {
    let fetch_interval_seconds = 30;
    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(fetch_interval_seconds));

    let http_client = reqwest::Client::new();

    loop {
        tokio::select! {
            _ = gtfs_rt_fetch_interval.tick() => {},
            r = rx_resolved_station.changed() => if r.is_err() {
                // TODO: Add an rt error variant, this will always be an internal error tho
                error!("bug: resolved station channel closed");
                break;
            },
        }

        let (relevant_stop_ids, relevant_endpoints) = {
            let Some(rx) = rx_resolved_station.borrow().clone() else {
                warn!(
                    "No resolved station, retrying on regular {}s interval or station selection change",
                    fetch_interval_seconds
                );
                continue;
            };
            (rx.stop_ids, rx.relevant_endpoints)
        };
        // Because we have to move into filter_map closure later
        let relevant_stop_ids = &relevant_stop_ids;

        let feed_messages = gtfs_rt_request_handler(relevant_endpoints, &http_client).await;
        let packets = {
            let active_static_data = &rx_active_static_data.borrow();

            feed_messages
                .iter()
                .flat_map(|message| {
                    match message.header.timestamp {
                        None => {
                            error!("Missing FeedMessage Timestamp, dropping all entities from this FeedMessage"); 
                            // TODO: Instead of dropping, maybe try to use system time?
                            None
                        }
                        Some(now) => {
                            Some(message.entity.iter().filter_map(move |entity| {
                                traintime_packet::feed_entity_to_packet(
                                    entity,
                                    active_static_data,
                                    relevant_stop_ids,
                                    &now,
                                )
                            }))
                        }
                    }

                })
                .flatten()
                .collect::<Vec<traintime_packet::TraintimePacket>>()
        };

        send_packets(packets, &tx_traintime_packets, fetch_interval_seconds).await;
    }
}

async fn send_packets(
    packets: Vec<TraintimePacket>,
    tx_traintime_packets: &mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
    fetch_interval_seconds: u64,
) {
    tx_traintime_packets.send(packets).await.inspect_err(|e| {
            error!(
                "error sending traintime packets, retrying packet refresh in {fetch_interval_seconds} seconds: {e}"
            )
        }).ok();
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
    let decoded = decode_gtfs_rt(gtfs_rt_bytes)?;
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

pub fn decode_gtfs_rt(response_bytes: Bytes) -> Result<FeedMessage, prost::DecodeError> {
    gtfs_rt_decode::decode::from_bytes(response_bytes)
}
