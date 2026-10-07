use crate::types::{gtfs, static_data::StaticData};
use gtfs_rt_decode::gtfs_rt_types::FeedEntity;

pub struct TraintimePacket {
    pub route_id: gtfs::RouteID,
    pub trip_headsign: gtfs::StationName,
    pub mins_until_arrival: i64,
    pub delay: Option<i32>,
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
    relevant_stop_ids: &[&gtfs::StopID],
    now: &u64,
) -> Option<TraintimePacket> {
    let trip_update = entity.trip_update.as_ref()?;

    let next_update = trip_update.stop_time_update.iter().find(|x| {
        x.stop_id.is_some()
            && relevant_stop_ids.contains(&&gtfs::StopID(x.stop_id.clone().unwrap()))
    })?;

    let arrival_time = next_update
        .arrival
        .and_then(|a| a.time)
        .or_else(|| next_update.departure.and_then(|d| d.time))?;

    let mins_until = (arrival_time - *now as i64) / 60;
    if mins_until.is_negative() {
        return None;
    };

    Some(TraintimePacket {
        route_id: gtfs::RouteID(trip_update.trip.route_id.as_ref()?.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use gtfs_rt_decode::gtfs_rt_types::{
        TripDescriptor, TripUpdate,
        trip_update::{StopTimeEvent, StopTimeUpdate},
    };

    const NOW: u64 = 1_000_000;

    fn static_data() -> StaticData {
        let mut static_data = StaticData::new();
        for (stop_id, station_name) in [
            ("101N", "Test Stop 1"),
            ("201N", "Test Stop 2"),
            ("301N", "Test Stop 3"),
        ] {
            static_data.stop_lookup.insert(
                gtfs::StopID(String::from(stop_id)),
                gtfs::StationName::from(station_name),
            );
        }
        static_data
    }

    fn event(seconds_from_now: i64, delay: Option<i32>) -> Option<StopTimeEvent> {
        Some(StopTimeEvent {
            time: Some(NOW as i64 + seconds_from_now),
            delay,
            ..Default::default()
        })
    }

    fn stop_time_update(
        stop_id: &str,
        arrival: Option<StopTimeEvent>,
        departure: Option<StopTimeEvent>,
    ) -> StopTimeUpdate {
        StopTimeUpdate {
            stop_id: Some(String::from(stop_id)),
            arrival,
            departure,
            ..Default::default()
        }
    }

    fn entity(route_id: Option<&str>, stop_time_update: Vec<StopTimeUpdate>) -> FeedEntity {
        FeedEntity {
            trip_update: Some(TripUpdate {
                trip: TripDescriptor {
                    route_id: route_id.map(String::from),
                    ..Default::default()
                },
                stop_time_update,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // Packet for a rider waiting at Test Stop 2
    fn packet_at_test_stop_2(entity: &FeedEntity) -> Option<TraintimePacket> {
        let relevant_stop_id = gtfs::StopID(String::from("201N"));
        feed_entity_to_packet(entity, &static_data(), &[&relevant_stop_id], &NOW)
    }

    #[test]
    fn packet_is_built_from_the_relevant_stop() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("101N", event(300, Some(0)), None),
                stop_time_update("201N", event(600, Some(45)), None),
                stop_time_update("301N", event(900, Some(0)), None),
            ],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.route_id, gtfs::RouteID(String::from("F")));
        assert_eq!(packet.mins_until_arrival, 10);
        assert_eq!(packet.delay, Some(45));
        // Headsign is the name of the last stop in the update
        assert_eq!(packet.trip_headsign, gtfs::StationName::from("Test Stop 3"));
    }

    #[test]
    fn minutes_until_arrival_round_down() {
        let entity = entity(
            Some("F"),
            vec![stop_time_update("201N", event(659, Some(0)), None)],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.mins_until_arrival, 10);
    }

    #[test]
    fn departure_time_is_used_when_arrival_has_no_time() {
        let arrival = Some(StopTimeEvent {
            delay: Some(0),
            ..Default::default()
        });
        let entity = entity(
            Some("F"),
            vec![stop_time_update("201N", arrival, event(120, None))],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.mins_until_arrival, 2);
    }

    // Known bug, see "A departure-only stop time is dropped" in the TODO. Remove the ignore
    // once it is fixed.
    #[test]
    #[ignore = "known bug: departure-only stop times are dropped"]
    fn departure_time_is_used_when_there_is_no_arrival() {
        let entity = entity(
            Some("F"),
            vec![stop_time_update("201N", None, event(120, None))],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.mins_until_arrival, 2);
        assert_eq!(packet.delay, None);
    }

    #[test]
    fn no_packet_without_a_trip_update() {
        assert!(packet_at_test_stop_2(&FeedEntity::default()).is_none());
    }

    #[test]
    fn no_packet_when_trip_does_not_stop_at_a_relevant_stop() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("101N", event(300, Some(0)), None),
                stop_time_update("301N", event(900, Some(0)), None),
            ],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_when_the_train_has_already_arrived() {
        let entity = entity(
            Some("F"),
            vec![stop_time_update("201N", event(-120, Some(0)), None)],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_when_stop_has_no_arrival_or_departure_time() {
        let entity = entity(Some("F"), vec![stop_time_update("201N", None, None)]);

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_without_a_route_id() {
        let entity = entity(
            None,
            vec![stop_time_update("201N", event(600, Some(0)), None)],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }
}
