use gtfs_rt_decode::gtfs_rt_types::{FeedEntity, FeedMessage};
use prost::bytes::Bytes;
use server::config::endpoint_matches;
use server::rt_funcs::decode_gtfs_rt;
use server::types::gtfs::{RouteID, StationName, StopID};
use server::types::static_data::StaticData;
use server::types::traintime_packet::{TraintimePacket, feed_entity_to_packet};
use std::collections::{BTreeMap, BTreeSet};

// Recorded from the MTA 1/2/3/4/5/6/7/S endpoint on 2026-10-07. Frozen, do not regenerate: every
// count and arrival time below is specific to this recording.
const GTFS_RT: &[u8] = include_bytes!("../test_data/test_1_7_S_gtfs.pb");

// Header timestamp of the recording, which is what the server uses as "now"
const FEED_TIMESTAMP: u64 = 1_791_411_185;

const GRAND_CENTRAL: &str = "Grand Central-42 St";

fn decode_fixture() -> FeedMessage {
    decode_gtfs_rt(Bytes::from_static(GTFS_RT)).expect("fixture feed should decode")
}

// Subset of the real stops.txt: the Grand Central platforms, and the last stop of every trip in
// the recording that calls at one of them
fn static_data() -> StaticData {
    static_data_from(&[
        ("631N", GRAND_CENTRAL),
        ("631S", GRAND_CENTRAL),
        ("723N", GRAND_CENTRAL),
        ("723S", GRAND_CENTRAL),
        ("901N", GRAND_CENTRAL),
        ("901S", GRAND_CENTRAL),
        ("204N", "Nereid Av"),
        ("208N", "Gun Hill Rd"),
        ("247S", "Flatbush Av-Brooklyn College"),
        ("250S", "Crown Hts-Utica Av"),
        ("401N", "Woodlawn"),
        ("405N", "Bedford Park Blvd-Lehman College"),
        ("415N", "149 St-Hostos"),
        ("501N", "Eastchester-Dyre Av"),
        ("601N", "Pelham Bay Park"),
        ("608N", "Parkchester"),
        ("640S", "Brooklyn Bridge-City Hall"),
        ("701N", "Flushing-Main St"),
        ("726S", "34 St-Hudson Yards"),
        ("902N", "Times Sq-42 St"),
    ])
}

fn static_data_from(stops: &[(&str, &str)]) -> StaticData {
    let mut static_data = StaticData::new();
    for (stop_id, station_name) in stops {
        static_data.stop_lookup.insert(
            StopID(String::from(*stop_id)),
            StationName::from(station_name),
        );
    }
    static_data
}

// Same steps as gtfs_rt_handler: relevant stops from the station name, "now" from the header
fn packets_at(
    message: &FeedMessage,
    static_data: &StaticData,
    station_name: &str,
) -> Vec<TraintimePacket> {
    let relevant_stop_ids = static_data.get_relevant_stops_to_station(station_name);
    let now = message
        .header
        .timestamp
        .expect("fixture header should have a timestamp");

    message
        .entity
        .iter()
        .filter_map(|entity| feed_entity_to_packet(entity, static_data, &relevant_stop_ids, &now))
        .collect()
}

// (route, headsign) -> sorted minutes until arrival
fn arrivals_by_route_and_headsign(
    packets: &[TraintimePacket],
) -> BTreeMap<(String, String), Vec<i64>> {
    let mut arrivals: BTreeMap<(String, String), Vec<i64>> = BTreeMap::new();
    for packet in packets {
        arrivals
            .entry((packet.route_id.0.clone(), packet.trip_headsign.0.clone()))
            .or_default()
            .push(packet.mins_until_arrival);
    }
    for minutes in arrivals.values_mut() {
        minutes.sort();
    }
    arrivals
}

fn entity_by_id<'a>(message: &'a FeedMessage, id: &str) -> &'a FeedEntity {
    message
        .entity
        .iter()
        .find(|entity| entity.id == id)
        .expect("entity should be in the fixture")
}

fn routes(route_ids: &[&str]) -> Vec<RouteID> {
    route_ids
        .iter()
        .map(|route_id| RouteID(String::from(*route_id)))
        .collect()
}

#[test]
fn fixture_decodes_with_header() {
    let message = decode_fixture();

    assert_eq!(message.header.gtfs_realtime_version, "1.0");
    assert_eq!(message.header.timestamp, Some(FEED_TIMESTAMP));
}

#[test]
fn fixture_decodes_every_entity() {
    let message = decode_fixture();

    let trip_updates = message
        .entity
        .iter()
        .filter(|entity| entity.trip_update.is_some())
        .count();
    let vehicles = message
        .entity
        .iter()
        .filter(|entity| entity.vehicle.is_some())
        .count();
    let alerts = message
        .entity
        .iter()
        .filter(|entity| entity.alert.is_some())
        .count();

    assert_eq!(message.entity.len(), 530);
    assert_eq!(trip_updates, 324);
    assert_eq!(vehicles, 205);
    assert_eq!(alerts, 1);
}

#[test]
fn fixture_decodes_trip_update_fields() {
    let message = decode_fixture();

    let trip_update = entity_by_id(&message, "000001")
        .trip_update
        .as_ref()
        .expect("first entity should be a trip update");
    let first_stop = &trip_update.stop_time_update[0];

    assert_eq!(trip_update.trip.trip_id.as_deref(), Some("100550_5..N"));
    assert_eq!(trip_update.trip.route_id.as_deref(), Some("5"));
    assert_eq!(trip_update.trip.start_date.as_deref(), Some("20261007"));
    assert_eq!(first_stop.stop_id.as_deref(), Some("213N"));
    assert_eq!(
        first_stop.arrival.and_then(|arrival| arrival.time),
        Some(1_791_411_098)
    );
    assert_eq!(
        first_stop.departure.and_then(|departure| departure.time),
        Some(1_791_411_128)
    );
}

#[test]
fn fixture_carries_the_routes_of_its_endpoint() {
    let message = decode_fixture();

    let route_ids: BTreeSet<&str> = message
        .entity
        .iter()
        .filter_map(|entity| entity.trip_update.as_ref()?.trip.route_id.as_deref())
        .collect();

    // Express variants and the 42 St shuttle have their own route ids (the shuttle is GS, not S)
    assert_eq!(
        route_ids,
        BTreeSet::from(["1", "2", "3", "4", "5", "6", "6X", "7", "7X", "GS"])
    );
}

#[test]
fn decode_rejects_a_truncated_feed() {
    let result = decode_gtfs_rt(Bytes::from_static(&GTFS_RT[..GTFS_RT.len() / 2]));

    assert!(result.is_err());
}

#[test]
fn decode_rejects_bytes_that_are_not_a_feed() {
    let result = decode_gtfs_rt(Bytes::from_static(b"not a feed"));

    assert!(result.is_err());
}

#[test]
fn endpoint_matches_routes_in_the_fixture() {
    let message = decode_fixture();

    assert!(endpoint_matches(&message.entity, &routes(&["A", "6"])));
    assert!(endpoint_matches(&message.entity, &routes(&["GS"])));
}

#[test]
fn endpoint_does_not_match_routes_missing_from_the_fixture() {
    let message = decode_fixture();

    // S is the shuttle's name, but not its route id
    assert!(!endpoint_matches(
        &message.entity,
        &routes(&["A", "C", "S"])
    ));
}

#[test]
fn packets_are_built_for_every_platform_of_the_station() {
    let message = decode_fixture();

    let packets = packets_at(&message, &static_data(), GRAND_CENTRAL);
    let arrivals = arrivals_by_route_and_headsign(&packets);

    let expected: [(&str, &str, &[i64]); 16] = [
        // 631N / 631S
        ("4", "149 St-Hostos", &[21]),
        (
            "4",
            "Crown Hts-Utica Av",
            &[0, 2, 14, 22, 23, 26, 34, 43, 47, 56, 62, 71],
        ),
        ("4", "Woodlawn", &[7, 11, 16, 18, 29, 37, 44, 51, 56, 61]),
        (
            "5",
            "Eastchester-Dyre Av",
            &[9, 19, 27, 31, 41, 48, 54, 59, 63, 71],
        ),
        (
            "5",
            "Flatbush Av-Brooklyn College",
            &[14, 21, 32, 42, 49, 60, 69, 76],
        ),
        ("5", "Gun Hill Rd", &[3]),
        ("5", "Nereid Av", &[1, 13, 25, 37]),
        (
            "6",
            "Brooklyn Bridge-City Hall",
            &[
                6, 9, 12, 16, 20, 24, 26, 29, 32, 36, 41, 46, 49, 54, 56, 60, 64, 69, 72, 82,
            ],
        ),
        ("6", "Parkchester", &[1, 13, 20, 28, 35]),
        ("6", "Pelham Bay Park", &[43]),
        ("6X", "Pelham Bay Park", &[5, 7, 16, 24, 32, 39, 47]),
        // 723N / 723S
        (
            "7",
            "34 St-Hudson Yards",
            &[
                0, 2, 5, 7, 9, 15, 17, 18, 23, 26, 28, 34, 37, 40, 43, 46, 49, 52, 55, 58, 61, 64,
            ],
        ),
        ("7", "Flushing-Main St", &[0, 9, 15, 21, 27, 34]),
        ("7X", "Flushing-Main St", &[2, 6, 12, 18, 24, 30, 37]),
        // 901S, shuttles terminating here
        ("GS", GRAND_CENTRAL, &[2, 5, 8, 11, 15, 18, 22, 25, 29, 33]),
        (
            "GS",
            "Times Sq-42 St",
            &[0, 3, 6, 10, 13, 17, 20, 24, 28, 32],
        ),
    ];
    let expected: BTreeMap<(String, String), Vec<i64>> = expected
        .into_iter()
        .map(|(route_id, headsign, minutes)| {
            (
                (String::from(route_id), String::from(headsign)),
                minutes.to_vec(),
            )
        })
        .collect();

    assert_eq!(packets.len(), 134);
    assert_eq!(arrivals, expected);
}

#[test]
fn packets_have_no_delay_when_the_feed_does_not_report_one() {
    let message = decode_fixture();

    let packets = packets_at(&message, &static_data(), GRAND_CENTRAL);

    // The MTA only sends times, never delays
    assert!(packets.iter().all(|packet| packet.delay.is_none()));
}

#[test]
fn no_packet_for_a_train_that_left_over_a_minute_ago() {
    let message = decode_fixture();
    let static_data = static_data();
    let relevant_stop_ids = static_data.get_relevant_stops_to_station(GRAND_CENTRAL);

    // 4 to Bedford Park Blvd, left 631N 73 seconds before the feed was generated
    let packet = feed_entity_to_packet(
        entity_by_id(&message, "000239"),
        &static_data,
        &relevant_stop_ids,
        &FEED_TIMESTAMP,
    );

    assert!(packet.is_none());
}

#[test]
fn train_that_left_under_a_minute_ago_is_dropped() {
    let message = decode_fixture();
    let static_data = static_data();
    let relevant_stop_ids = static_data.get_relevant_stops_to_station(GRAND_CENTRAL);

    // 6 to Brooklyn Bridge, left 631S 59 seconds before the feed was generated. Departure is
    // checked in seconds, so it is dropped rather than rounded to 0 minutes away.
    let packet = feed_entity_to_packet(
        entity_by_id(&message, "000372"),
        &static_data,
        &relevant_stop_ids,
        &FEED_TIMESTAMP,
    );

    assert!(packet.is_none());
}

#[test]
fn no_packets_from_vehicle_or_alert_entities() {
    let message = decode_fixture();
    let static_data = static_data();
    let relevant_stop_ids = static_data.get_relevant_stops_to_station(GRAND_CENTRAL);

    // Vehicle positions carry a stop_id too (often a relevant one), but no trip update
    let packets = message
        .entity
        .iter()
        .filter(|entity| entity.trip_update.is_none())
        .filter_map(|entity| {
            feed_entity_to_packet(entity, &static_data, &relevant_stop_ids, &FEED_TIMESTAMP)
        })
        .count();

    assert_eq!(packets, 0);
}

#[test]
fn no_packets_for_a_station_the_feed_does_not_serve() {
    let message = decode_fixture();
    let static_data = static_data_from(&[("F15N", "Delancey St-Essex St")]);

    assert!(packets_at(&message, &static_data, "Delancey St-Essex St").is_empty());
}

#[test]
fn no_packets_for_an_unknown_station() {
    let message = decode_fixture();

    assert!(packets_at(&message, &static_data(), "Not A Station").is_empty());
}

// A trip whose last stop is not in stop_lookup still builds a packet, with the stop id standing
// in for the headsign
#[test]
fn headsign_falls_back_to_the_stop_id_when_the_last_stop_is_not_in_stop_lookup() {
    let message = decode_fixture();
    let static_data = static_data_from(&[
        ("631N", GRAND_CENTRAL),
        ("631S", GRAND_CENTRAL),
        ("723N", GRAND_CENTRAL),
        ("723S", GRAND_CENTRAL),
        ("901N", GRAND_CENTRAL),
        ("901S", GRAND_CENTRAL),
    ]);

    let packets = packets_at(&message, &static_data, GRAND_CENTRAL);
    let (known, unknown): (Vec<&TraintimePacket>, Vec<&TraintimePacket>) = packets
        .iter()
        .partition(|packet| packet.trip_headsign.0 == GRAND_CENTRAL);

    // Same packets as with the full stop_lookup, none dropped
    assert_eq!(packets.len(), 134);
    // Only the shuttles terminating at Grand Central have a headsign that can be looked up
    assert_eq!(known.len(), 10);
    assert!(known.iter().all(|packet| packet.route_id.0 == "GS"));
    assert_eq!(unknown.len(), 124);
    assert!(
        unknown
            .iter()
            .all(|packet| packet.trip_headsign.0.starts_with("Unknown, ID: "))
    );
    // The ten shuttles leaving for Times Sq
    assert_eq!(
        unknown
            .iter()
            .filter(|packet| packet.trip_headsign.0 == "Unknown, ID: 902N")
            .count(),
        10
    );
}

// Make sure we do not drop packets for stations at which a trip is originating
#[test]
fn packets_are_built_for_trains_originating_at_the_station() {
    let message = decode_fixture();

    let packets = packets_at(&message, &static_data(), GRAND_CENTRAL);
    let arrivals = arrivals_by_route_and_headsign(&packets);

    // 134 after dropping trips that have already departed
    assert_eq!(packets.len(), 134);
    assert_eq!(
        arrivals.get(&(String::from("GS"), String::from("Times Sq-42 St"))),
        Some(&vec![0, 3, 6, 10, 13, 17, 20, 24, 28, 32])
    );
}
