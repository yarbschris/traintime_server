use crate::static_data::StaticData;
use gtfs_rt_decode::gtfs_rt_types::FeedEntity;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TraintimePacket {
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
            "Line: {} to {}\nMinutes Until Arrival: {},\nDelay: {}\n",
            self.route_id,
            self.trip_headsign,
            self.mins_until_arrival,
            self.delay.unwrap_or(0),
        )
    }
}

pub fn feed_entity_to_packet(
    entity: &FeedEntity,
    static_data: &StaticData,
    relevant_stop_ids: &[&String],
) -> Option<TraintimePacket> {
    let trip_update = entity.trip_update.as_ref()?;

    let next_update = trip_update.stop_time_update.iter().find(|x| {
        x.stop_id.is_some() && relevant_stop_ids.contains(&x.stop_id.as_ref().unwrap())
    })?;

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
        route_id: trip_update.trip.route_id.as_ref()?.clone(),
        stop_id: next_update.stop_id.as_ref()?.clone(),
        trip_headsign: static_data
            .stop_lookup
            .get(
                &trip_update
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
