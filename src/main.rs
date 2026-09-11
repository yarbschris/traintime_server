use gtfs_rt_decode::gtfs_rt_types::{FeedEntity, trip_update::StopTimeUpdate};
use log::info;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tokio::time::{self, Duration};

use crate::config::SupportedTransitSystem;
use crate::static_data::StaticData;

pub mod config;
pub mod rt_data;
pub mod static_data;

#[tokio::main]
async fn main() {
    colog::init();
    let system_config =
        config::TraintimeSystemConfig::read_config_by_system(SupportedTransitSystem::NycSubway);

    let (tx_active_static_data, mut rx_active_static_data) = watch::channel(StaticData::new());

    static_data::setup_gtfs_static(
        tx_active_static_data,
        system_config.gtfs_static_endpoint.clone(),
    )
    .await;

    info!("Waiting for new gtfs static data");
    let fresh_static = rx_active_static_data
        .wait_for(|x| x.stop_lookup.len() > 1)
        .await
        .unwrap();
    info!("Got new gtfs static data");
    let selected_station_config = config::SelectedStationConfig::build(
        static_data::TEST_STOP_NAME,
        &system_config,
        &fresh_static.route_lookup,
    )
    .await;
    drop(fresh_static);

    let mut gtfs_rt_fetch_interval = time::interval(Duration::from_secs(30));
    loop {
        gtfs_rt_fetch_interval.tick().await;
        let entities = rt_data::gtfs_rt_handler(&selected_station_config.relevant_endpoints).await;
        let active_static_data = rx_active_static_data.borrow();
        let relevant_stop_ids =
            active_static_data.get_relevant_stops_to_station(static_data::TEST_STOP_NAME);

        let packets = entities
            .iter()
            .filter_map(|entity| {
                feed_entity_to_packet(entity, &active_static_data, &relevant_stop_ids)
            })
            .collect::<Vec<TraintimePacket>>();

        info!("main loop: dropped mutex");

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
