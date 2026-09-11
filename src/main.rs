use gtfs_decode::transit_realtime::{FeedEntity, FeedMessage, trip_update::StopTimeUpdate};
use reqwest::Response;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tokio::time::{self, Duration};

use crate::config::SupportedTransitSystem;
use crate::static_data::StaticData;

pub mod config;
pub mod static_data;

#[tokio::main]
async fn main() {
    let config =
        config::TraintimeSystemConfig::read_config_by_system(SupportedTransitSystem::NycSubway);

    let active_static_data = Arc::new(Mutex::new(Some(StaticData::new())));

    let (tx_static_data, rx_static_data) = mpsc::channel(2);

    static_data::gtfs_static_handler(tx_static_data, config.gtfs_static_endpoint).await;

    let (tx_new_static_data, mut rx_new_static_data) = mpsc::channel(1);

    tokio::spawn(update_static_data_handler(
        rx_static_data,
        tx_new_static_data,
        Arc::clone(&active_static_data),
    ));

    dbg!("Waiting to recieve static data");
    // Wait for first round of static data before entering loop
    // TODO: Later on, we will use this to signal new static data when stop preference changes
    rx_new_static_data.recv().await;

    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(30));
    loop {
        gtfs_rt_fetch_interval.tick().await;
        let Ok(response) = fetch_gtfs_rt(&config.gtfs_rt_endpoints).await else {
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
        let relevant_stop_ids =
            static_data.get_relevant_stops_to_station(static_data::TEST_STOP_NAME);

        let packets = entities
            .iter()
            .filter_map(|entity| feed_entity_to_packet(entity, static_data, &relevant_stop_ids))
            .collect::<Vec<TraintimePacket>>();

        for packet in &packets {
            println!("{}", packet)
        }
        for lookup in &static_data.route_lookup {
            dbg!(lookup);
        }
    }
}

async fn update_static_data_handler(
    mut rx_static_data: mpsc::Receiver<StaticData>,
    tx_new_static_data: mpsc::Sender<u8>,
    old_data: Arc<Mutex<Option<StaticData>>>,
) {
    while let Some(new_data) = rx_static_data.recv().await {
        dbg!("Recieved new static data");
        let mut old_inner = old_data.lock().await;
        old_inner.as_mut().unwrap().stop_lookup = new_data.stop_lookup;
        old_inner.as_mut().unwrap().route_lookup = new_data.route_lookup;
        tx_new_static_data.send(0).await.unwrap();
    }
}

async fn fetch_gtfs_rt(gtfs_rt_endpoints: &[String]) -> Result<Response, reqwest::Error> {
    println!("Fetching MTA Subway Line Data...");
    reqwest::get(gtfs_rt_endpoints.get(0).unwrap()).await
}

async fn decode_gtfs_rt(response: Response) -> Result<FeedMessage, prost::DecodeError> {
    let response_bytes = response.bytes().await.unwrap();
    <FeedMessage as prost::Message>::decode(response_bytes)
}

fn feed_entity_to_packet(
    entity: &FeedEntity,
    static_data: &StaticData,
    relevant_stop_ids: &Vec<&String>,
) -> Option<TraintimePacket> {
    let next_update = entity
        .trip_update
        .as_ref()?
        .stop_time_update
        .iter()
        .filter(|update| relevant_stop_ids.contains(&update.stop_id.as_ref().unwrap()))
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
            .stop_lookup
            .get(
                &entity
                    .trip_update
                    .as_ref()?
                    .stop_time_update
                    .iter()
                    .last()?
                    .clone()
                    .stop_id?,
            )?
            .clone(),
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
