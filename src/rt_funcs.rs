use futures::future::join_all;
use gtfs_rt_decode::gtfs_rt_types::FeedMessage;
use log::{debug, error, info, warn};
use prost::bytes::Bytes;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time;

use crate::config;
use crate::error::GtfsRtError;
use crate::types::traintime_packet::TraintimePacket;
use crate::types::{generation::Generation, gtfs::StopID};
use crate::types::{static_data::StaticData, traintime_packet};

pub fn setup_gtfs_rt(
    rx_resolved_station: watch::Receiver<Option<config::ResolvedStation>>,
    rx_static_data: watch::Receiver<Option<StaticData>>,
    tx_traintime_packets: mpsc::Sender<Vec<traintime_packet::TraintimePacket>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(gtfs_rt_handler(
        rx_resolved_station,
        rx_static_data,
        tx_traintime_packets,
    ))
}

async fn gtfs_rt_handler(
    mut rx_resolved_station: watch::Receiver<Option<config::ResolvedStation>>,
    rx_static_data: watch::Receiver<Option<StaticData>>,
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

        if rx_static_data.borrow().is_none() {
            debug!("No static data loaded, skipping realtime fetch");
            continue;
        }

        let (relevant_stop_ids, relevant_endpoints, resolved_station_generation) = {
            let Some(resolved_station) = rx_resolved_station.borrow().clone() else {
                debug!(
                    "No resolved station, retrying on regular {}s interval or station selection change",
                    fetch_interval_seconds
                );
                continue;
            };
            (
                resolved_station.stop_ids,
                resolved_station.relevant_endpoints,
                resolved_station.generation,
            )
        };
        let feed_messages = gtfs_rt_request_handler(relevant_endpoints, &http_client).await;
        let packets = {
            let static_data_ref = rx_static_data.borrow();
            let Some(static_data) = static_data_ref.as_ref() else {
                debug!("Static Data cleared during realtime fetch, skipping tick...");
                continue;
            };
            let Some(packets) = packets_for_tick(
                &feed_messages,
                static_data,
                resolved_station_generation,
                &relevant_stop_ids,
            ) else {
                continue;
            };
            packets
        };

        send_packets(packets, &tx_traintime_packets, fetch_interval_seconds).await;
    }
}

// Build the packets for one tick, or None to skip it because the resolved station was built
// against older static data. The resolver is already re-resolving, so the next tick is consistent
fn packets_for_tick(
    feed_messages: &[FeedMessage],
    static_data: &StaticData,
    resolved_station_generation: Generation,
    relevant_stop_ids: &[StopID],
) -> Option<Vec<TraintimePacket>> {
    if static_data.generation != resolved_station_generation {
        debug!(
            "Resolved against {:?}, static data is {:?}, skipping tick...",
            resolved_station_generation, static_data.generation
        );
        return None;
    }
    let packets = feed_messages
        .iter()
        .flat_map(|message| match message.header.timestamp {
            None => {
                error!(
                    "Missing FeedMessage Timestamp, dropping all entities from this FeedMessage"
                );
                // TODO: Instead of dropping, maybe try to use system time?
                None
            }
            Some(now) => Some(message.entity.iter().filter_map(move |entity| {
                traintime_packet::feed_entity_to_packet(
                    entity,
                    static_data,
                    relevant_stop_ids,
                    &now,
                )
            })),
        })
        .flatten()
        .collect();
    Some(packets)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SupportedTransitSystem;
    use crate::types::gtfs::StationName;
    use gtfs_rt_decode::gtfs_rt_types::{
        FeedEntity, FeedHeader, TripDescriptor, TripUpdate,
        trip_update::{StopTimeEvent, StopTimeUpdate},
    };

    const NOW: u64 = 1_000_000;

    fn static_data_at(generation: Generation) -> StaticData {
        let mut static_data = StaticData::empty(SupportedTransitSystem::NycSubway);
        for (stop_id, station_name) in [("201N", "Test Stop 2"), ("301N", "Test Stop 3")] {
            static_data.stop_lookup.insert(
                StopID(String::from(stop_id)),
                StationName::from(station_name),
            );
        }
        static_data.generation = generation;
        static_data
    }

    fn stop_time_update(stop_id: &str, seconds_from_now: i64) -> StopTimeUpdate {
        StopTimeUpdate {
            stop_id: Some(String::from(stop_id)),
            arrival: Some(StopTimeEvent {
                time: Some(NOW as i64 + seconds_from_now),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // One F train calling at Test Stop 2 in ten minutes, then ending at Test Stop 3
    fn feed_message(timestamp: Option<u64>) -> FeedMessage {
        FeedMessage {
            header: FeedHeader {
                timestamp,
                ..Default::default()
            },
            entity: vec![FeedEntity {
                trip_update: Some(TripUpdate {
                    trip: TripDescriptor {
                        route_id: Some(String::from("F")),
                        ..Default::default()
                    },
                    stop_time_update: vec![
                        stop_time_update("201N", 600),
                        stop_time_update("301N", 900),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            }],
        }
    }

    fn test_stop_2() -> Vec<StopID> {
        vec![StopID(String::from("201N"))]
    }

    #[test]
    fn packets_are_built_when_the_station_was_resolved_against_the_current_static_data() {
        let packets = packets_for_tick(
            &[feed_message(Some(NOW))],
            &static_data_at(Generation(1)),
            Generation(1),
            &test_stop_2(),
        )
        .expect("tick should not be skipped");

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].trip_headsign, StationName::from("Test Stop 3"));
    }

    #[test]
    fn tick_is_skipped_when_the_station_was_resolved_against_older_static_data() {
        // Same feed as above, so the skip comes from the generations alone
        let packets = packets_for_tick(
            &[feed_message(Some(NOW))],
            &static_data_at(Generation(2)),
            Generation(1),
            &test_stop_2(),
        );

        assert!(packets.is_none());
    }

    #[test]
    fn feed_without_a_timestamp_is_dropped_without_skipping_the_tick() {
        let packets = packets_for_tick(
            &[feed_message(None)],
            &static_data_at(Generation(1)),
            Generation(1),
            &test_stop_2(),
        )
        .expect("tick should not be skipped");

        assert!(packets.is_empty());
    }
}
