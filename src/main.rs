use gtfs_decode::transit_realtime::{FeedEntity, FeedMessage, trip_update::StopTimeUpdate};
use reqwest::Response;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{self, Duration};

use crate::static_data::{StaticData, TEST_STOP_NAME};

pub mod static_data;

#[tokio::main]
async fn main() {
    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(30));
    let static_data_bytes = static_data::fetch_gtfs_static_data().await.unwrap();
    let static_data =
        static_data::parse_and_filter_gtfs_static_data(static_data_bytes, TEST_STOP_NAME);
    loop {
        gtfs_rt_fetch_interval.tick().await;
        if let Ok(response) = fetch_gtfs_rt().await {
            if let Ok(decoded) = decode_gtfs_rt(response).await {
                let entities = &decoded.entity;
                let packets = entities
                    .iter()
                    .filter_map(|entity| feed_entity_to_packet(entity, &static_data))
                    .collect::<Vec<TraintimePacket>>();

                for packet in &packets {
                    println!("{}", packet)
                }
            } else {
                println!(
                    "Error Decoding gtfs-rt message. Please verify endpoint. Retrying in 30s..."
                )
            }
        } else {
            println!(
                "Error trying to fetch gtfs-rt data. Please verify endpoint. Retrying in 30s..."
            )
        }
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
                .contains(&&update.stop_id.as_ref().unwrap())
        })
        .collect::<Vec<&StopTimeUpdate>>()
        .into_iter()
        .next()?;
    let arrival_time = next_update.arrival?.time?;
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
            .unwrap_or(&std::rc::Rc::new(
                entity
                    .trip_update
                    .as_ref()
                    .unwrap()
                    .trip
                    .trip_id()
                    .to_string(),
            ))
            .to_string(),
        mins_until_arrival: mins_until,
        trip_id: entity.trip_update.as_ref()?.trip.trip_id.as_ref()?.clone(),
        delay: next_update.arrival?.delay,
    })
}

fn post_filtered_data() {
    todo!();
}

struct TraintimePacket {
    route_id: String,
    stop_id: String,
    trip_id: String,
    trip_headsign: String,
    mins_until_arrival: i64,
    delay: Option<i32>,
}

impl std::fmt::Display for TraintimePacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Line: {} bound {} to {}\nMinutes Until Arrival: {},\nDelay: {}\nTrip ID: {}\n",
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
            self.trip_id,
        )
    }
}
