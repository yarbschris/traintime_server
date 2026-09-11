use gtfs_decode::transit_realtime::{FeedEntity, trip_update::StopTimeUpdate};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tokio::time::{self, Duration};

use crate::config::SupportedTransitSystem;
use crate::static_data::StaticData;

pub mod config;
pub mod rt_data;
pub mod static_data;

#[tokio::main]
async fn main() {
    let system_config =
        config::TraintimeSystemConfig::read_config_by_system(SupportedTransitSystem::NycSubway);

    let active_static_data = Arc::new(Mutex::new(Some(StaticData::new())));

    let (tx_static_data, rx_static_data) = mpsc::channel(1);

    static_data::gtfs_static_handler(tx_static_data, system_config.gtfs_static_endpoint.clone())
        .await;

    let (tx_new_static_data, mut rx_new_static_data) = mpsc::channel(1);

    tokio::spawn(static_data::update_static_data_handler(
        rx_static_data,
        tx_new_static_data,
        Arc::clone(&active_static_data),
    ));

    dbg!("Waiting to recieve static data");
    // Wait for first round of static data before entering loop
    rx_new_static_data.recv().await;

    let guard = active_static_data.lock().await;
    let fresh_static = guard.as_ref().unwrap();
    let selected_station_config = config::SelectedStationConfig::build(
        static_data::TEST_STOP_NAME,
        &system_config,
        &fresh_static.route_lookup,
    )
    .await;
    drop(guard);

    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(30));
    loop {
        gtfs_rt_fetch_interval.tick().await;
        let entities = rt_data::gtfs_rt_handler(&selected_station_config.relevant_endpoints).await;
        let guard = active_static_data.lock().await;
        let Some(static_data) = guard.as_ref() else {
            continue;
        };
        let relevant_stop_ids =
            static_data.get_relevant_stops_to_station(static_data::TEST_STOP_NAME);

        let packets = entities
            .iter()
            .filter_map(|entity| feed_entity_to_packet(entity, static_data, &relevant_stop_ids))
            .collect::<Vec<TraintimePacket>>();

        for packet in &packets {
            println!("{}", packet)
        }
    }
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
