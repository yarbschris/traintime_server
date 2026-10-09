use crate::types::{gtfs, static_data::StaticData};
use gtfs_rt_decode::gtfs_rt_types::{FeedEntity, trip_descriptor, trip_update::stop_time_update};

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

    // Drop Canceled / Deleted Trips
    if matches!(
        trip_update.trip.schedule_relationship(),
        trip_descriptor::ScheduleRelationship::Canceled
            | trip_descriptor::ScheduleRelationship::Deleted
    ) {
        return None;
    }

    // Get next update where the stop_id matches a stop_id we we are looking for
    let next_update = trip_update.stop_time_update.iter().find(|update| {
        !matches!(
            update.schedule_relationship(),
            stop_time_update::ScheduleRelationship::Skipped
        ) && update.stop_id.is_some()
            && relevant_stop_ids.contains(&&gtfs::StopID(update.stop_id.clone().unwrap()))
    })?;

    let last_stop_id = trip_update
        .stop_time_update
        .iter()
        .last()?
        .clone()
        .stop_id?; // TODO: Don't want to drop packet if the last stop_id is missing (?)

    // We drop the packet if the station is the end station of the trip, we only want to show when
    // the rider can board (Grand Central <-> Times Sq. 42nd is a good example of this happening)
    if &last_stop_id == next_update.stop_id.as_ref()? {
        return None;
    }

    // Arrival time takes precedence. If there is no arrival time (for example, if we are looking at
    // an originating station), then we use the departure time. If neither of these times are
    // available, packet is dropped.
    let arrival_time = next_update
        .arrival
        .and_then(|a| a.time)
        .or_else(|| next_update.departure.and_then(|d| d.time))?;

    // Drop packets where train has already departed
    let secs_until = arrival_time - *now as i64;
    if secs_until.is_negative() {
        return None;
    };
    let mins_until = secs_until / 60;

    Some(TraintimePacket {
        route_id: gtfs::RouteID(trip_update.trip.route_id.as_ref()?.clone()),
        trip_headsign: static_data
            .stop_lookup
            .get(&last_stop_id)
            // if a trip's last top stop_id is not in stop_lookup, simply use unknown
            // TODO: Can this be more efficient (String alloc + Clone rn)
            .unwrap_or(&gtfs::StationName(
                String::from("Unknown, ID: ") + &last_stop_id,
            ))
            .clone(),
        mins_until_arrival: mins_until,
        delay: next_update
            .arrival
            .and_then(|stop_time_event| stop_time_event.delay)
            .or_else(|| {
                next_update
                    .departure
                    .and_then(|stop_time_event| stop_time_event.delay)
            }),
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

    // A stop after Test Stop 2, so the trip does not end where the rider is waiting
    fn last_stop() -> StopTimeUpdate {
        stop_time_update("301N", event(900, Some(0)), None)
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
            vec![
                stop_time_update("201N", event(659, Some(0)), None),
                last_stop(),
            ],
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
            vec![
                stop_time_update("201N", arrival, event(120, None)),
                last_stop(),
            ],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.mins_until_arrival, 2);
    }

    // First stops of a trip typically carry only a departure
    #[test]
    fn departure_time_is_used_when_there_is_no_arrival() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("201N", None, event(120, None)),
                last_stop(),
            ],
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

    // A rider cannot board a train that ends its trip at their stop
    #[test]
    fn no_packet_when_the_trip_ends_at_the_relevant_stop() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("101N", event(300, Some(0)), None),
                stop_time_update("201N", event(600, Some(0)), None),
            ],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_when_the_train_has_already_arrived() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("201N", event(-120, Some(0)), None),
                last_stop(),
            ],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_when_stop_has_no_arrival_or_departure_time() {
        let entity = entity(
            Some("F"),
            vec![stop_time_update("201N", None, None), last_stop()],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn no_packet_without_a_route_id() {
        let entity = entity(
            None,
            vec![
                stop_time_update("201N", event(600, Some(0)), None),
                last_stop(),
            ],
        );

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn headsign_falls_back_to_the_stop_id_when_the_last_stop_is_unknown() {
        let entity = entity(
            Some("F"),
            vec![
                stop_time_update("201N", event(600, Some(0)), None),
                stop_time_update("999N", event(900, Some(0)), None),
            ],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(
            packet.trip_headsign,
            gtfs::StationName::from("Unknown, ID: 999N")
        );
        assert_eq!(packet.mins_until_arrival, 10);
    }

    #[test]
    fn no_packet_for_a_canceled_or_deleted_trip() {
        for relationship in [
            trip_descriptor::ScheduleRelationship::Canceled,
            trip_descriptor::ScheduleRelationship::Deleted,
        ] {
            let mut entity = entity(
                Some("F"),
                vec![
                    stop_time_update("201N", event(600, Some(0)), None),
                    last_stop(),
                ],
            );
            entity
                .trip_update
                .as_mut()
                .expect("entity should have a trip update")
                .trip
                .set_schedule_relationship(relationship);

            assert!(
                packet_at_test_stop_2(&entity).is_none(),
                "{relationship:?} trip should not build a packet"
            );
        }
    }

    #[test]
    fn no_packet_when_the_relevant_stop_is_skipped() {
        // A skipped stop may still carry times
        let mut skipped = stop_time_update("201N", event(600, Some(0)), None);
        skipped.set_schedule_relationship(stop_time_update::ScheduleRelationship::Skipped);
        let entity = entity(Some("F"), vec![skipped, last_stop()]);

        assert!(packet_at_test_stop_2(&entity).is_none());
    }

    #[test]
    fn packet_is_built_when_only_another_stop_is_skipped() {
        let mut skipped = stop_time_update("101N", event(300, Some(0)), None);
        skipped.set_schedule_relationship(stop_time_update::ScheduleRelationship::Skipped);
        let entity = entity(
            Some("F"),
            vec![
                skipped,
                stop_time_update("201N", event(600, Some(0)), None),
                last_stop(),
            ],
        );

        let packet = packet_at_test_stop_2(&entity).expect("packet should be built");

        assert_eq!(packet.mins_until_arrival, 10);
    }
}
