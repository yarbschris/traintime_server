use gtfs_decode::transit_realtime::{FeedEntity, FeedMessage, trip_update::StopTimeUpdate};
use reqwest::Response;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tokio::time::{self, Duration};

use crate::static_data::StaticData;

pub mod static_data;

#[tokio::main]
async fn main() {
    let active_static_data = Arc::new(Mutex::new(Some(StaticData {
        relevant_stop_ids: vec![],
        header_lookup: HashMap::new(),
    })));

    let (tx_static_data, rx_static_data) = mpsc::channel(2);

    static_data::gtfs_static_handler(tx_static_data).await;

    tokio::spawn(update_static_data_handler(
        rx_static_data,
        Arc::clone(&active_static_data),
    ));

    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(30));
    loop {
        gtfs_rt_fetch_interval.tick().await;
        let Ok(response) = fetch_gtfs_rt().await else {
            dbg!("Error Fetching GTFS-RT");
            continue;
        };
        let Ok(decoded) = decode_gtfs_rt(response).await else {
            dbg!("Error Decoding GTFS-RT");
            continue;
        };
        let guard = active_static_data.lock().await;
        let Some(static_data) = guard.as_ref() else {
            continue;
        };
        let entities = &decoded.entity;
        let packets = entities
            .iter()
            .filter_map(|entity| feed_entity_to_packet(entity, static_data))
            .collect::<Vec<TraintimePacket>>();

        for packet in &packets {
            println!("{}", packet)
        }
    }
}

async fn update_static_data_handler(
    mut rx_static_data: mpsc::Receiver<StaticData>,
    old_data: Arc<Mutex<Option<StaticData>>>,
) {
    while let Some(new_data) = rx_static_data.recv().await {
        dbg!("Recieved new static data");
        let mut old_inner = old_data.lock().await;
        old_inner.as_mut().unwrap().relevant_stop_ids = new_data.relevant_stop_ids;
        old_inner.as_mut().unwrap().header_lookup = new_data.header_lookup;
    }
}

async fn fetch_gtfs_rt() -> Result<Response, reqwest::Error> {
    println!("Fetching MTA Subway Line Data...");
    reqwest::get(static_data::TEST_ENDPOINT).await
}

async fn decode_gtfs_rt(response: Response) -> Result<FeedMessage, prost::DecodeError> {
    let response_bytes = response.bytes().await.unwrap();
    <FeedMessage as prost::Message>::decode(response_bytes)
}

fn feed_entity_to_packet(entity: &FeedEntity, static_data: &StaticData) -> Option<TraintimePacket> {
    let next_update = entity
        .trip_update
        .as_ref()?
        .stop_time_update
        .iter()
        .filter(|update| {
            static_data
                .relevant_stop_ids
                .contains(update.stop_id.as_ref().unwrap())
        })
        .collect::<Vec<&StopTimeUpdate>>()
        .into_iter()
        .next()?;
    let arrival_time = next_update
        .arrival
        .and_then(|a| a.time)
        .or_else(|| next_update.departure.and_then(|d| d.time))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mins_until = (arrival_time - now) / 60;
    if mins_until.is_negative() {
        return None;
    };
    Some(TraintimePacket {
        route_id: entity.trip_update.as_ref()?.trip.route_id.as_ref()?.clone(),
        stop_id: next_update.stop_id.as_ref()?.clone(),
        trip_headsign: static_data
            .header_lookup
            .get(entity.trip_update.as_ref()?.trip.trip_id.as_ref().unwrap())
            .unwrap_or_else(|| {
                // TODO: Figure out a better way to work around NYC MTA's trip_id convention. This
                // currently works as a backup when they drop the suffix of a trip_id
                let binding = "default".to_string();
                let key = static_data
                    .header_lookup
                    .keys()
                    .find(|key| {
                        key.contains(
                            entity
                                .trip_update
                                .as_ref()
                                .unwrap()
                                .trip
                                .trip_id
                                .clone()
                                .unwrap()
                                .as_str(),
                        )
                    })
                    .unwrap_or(&binding);
                static_data.header_lookup.get(key).unwrap()
            })
            .to_string(),
        mins_until_arrival: mins_until,
        delay: next_update.arrival?.delay,
    })
}

struct TraintimePacket {
    route_id: String,
    stop_id: String,
    trip_headsign: String,
    mins_until_arrival: i64,
    delay: Option<i32>,
}

impl std::fmt::Display for TraintimePacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Line: {} bound {} to {}\nMinutes Until Arrival: {},\nDelay: {}\n",
            // TODO: This pattern is NYC Subway Specific
            match &self.stop_id[self.stop_id.len() - 1..] {
                "S" => "Downtown",
                "N" => "Uptown",
                _ => "Unknown",
            },
            self.route_id,
            self.trip_headsign,
            self.mins_until_arrival,
            self.delay.unwrap_or(0),
        )
    }
}
