use prost::bytes::Bytes;
use server::config::SupportedTransitSystem;
use server::error::GtfsStaticError;
use server::types::generation::Generation;
use server::types::gtfs::{LocationType, TransferType};
use server::types::static_data::StaticData;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

// Feeds are built in memory so each test shows the rows it depends on. One trip per route:
// trip_N runs on route N.
const TRIPS: &str = "\
route_id,trip_id
1,trip_1
2,trip_2
3,trip_3
4,trip_4
5,trip_5
6,trip_6
";

// The existing fixture's sources, for the tests that only change transfers.txt
const FIXTURE_STOPS: &str = include_str!("../test_data/stops.txt");
const FIXTURE_TRIPS: &str = include_str!("../test_data/trips.txt");
const FIXTURE_STOP_TIMES: &str = include_str!("../test_data/stop_times.txt");
const FIXTURE_TRANSFERS: &str = include_str!("../test_data/transfers.txt");

fn zip_files(files: &[(&str, &str)]) -> Bytes {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in files {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .expect("zip entry should start");
        writer
            .write_all(contents.as_bytes())
            .expect("zip entry should be written");
    }
    Bytes::from(writer.finish().expect("zip should finish").into_inner())
}

fn build(files: &[(&str, &str)]) -> Result<StaticData, GtfsStaticError> {
    StaticData::build_from_bytes(
        zip_files(files),
        SupportedTransitSystem::NycSubway,
        Generation(0),
    )
}

// A feed with the shared TRIPS, and transfers.txt only if given
fn build_feed(
    stops: &str,
    transfers: Option<&str>,
    stop_times: &str,
) -> Result<StaticData, GtfsStaticError> {
    let mut files = vec![
        ("stops.txt", stops),
        ("trips.txt", TRIPS),
        ("stop_times.txt", stop_times),
    ];
    if let Some(transfers) = transfers {
        files.push(("transfers.txt", transfers));
    }
    build(&files)
}

// The existing fixture, with its transfers.txt replaced (or left out)
fn build_fixture_with_transfers(transfers: Option<&str>) -> Result<StaticData, GtfsStaticError> {
    let mut files = vec![
        ("stops.txt", FIXTURE_STOPS),
        ("trips.txt", FIXTURE_TRIPS),
        ("stop_times.txt", FIXTURE_STOP_TIMES),
    ];
    if let Some(transfers) = transfers {
        files.push(("transfers.txt", transfers));
    }
    build(&files)
}

// Every stop in the lookup with its label, sorted, so a test can compare the whole result
fn labels(static_data: &StaticData) -> BTreeMap<&str, &str> {
    static_data
        .stop_lookup
        .iter()
        .map(|(stop_id, name)| (stop_id.0.as_str(), name.0.as_str()))
        .collect()
}

fn sorted_routes(static_data: &StaticData, label: &str) -> Vec<String> {
    let mut routes: Vec<String> = static_data
        .route_lookup
        .iter()
        .find(|(name, _)| name.0 == label)
        .expect("label should be in route_lookup")
        .1
        .iter()
        .map(|route| route.0.clone())
        .collect();
    routes.sort();
    routes
}

// transfers.txt

#[test]
fn missing_transfers_file_gives_one_group_per_station() {
    let static_data = build_fixture_with_transfers(None).expect("fixture should build");

    // Without transfers nothing merges, so both same-name pairs get route qualifiers, and every
    // platform still points at its own station's group
    assert_eq!(
        labels(&static_data),
        BTreeMap::from([
            ("101N", "Test Stop 1"),
            ("101S", "Test Stop 1"),
            ("201N", "Test Stop 2"),
            ("201S", "Test Stop 2"),
            ("301N", "Test Stop 3 (4)"),
            ("401N", "Test Stop 3 (5)"),
            ("501N", "Test Stop 4 (6)"),
            ("601N", "Test Stop 4 (7)"),
        ])
    );
    assert_eq!(sorted_routes(&static_data, "Test Stop 2"), ["1", "2", "3"]);
}

#[test]
fn header_only_transfers_file_is_the_same_as_a_missing_one() {
    let missing = build_fixture_with_transfers(None).expect("fixture should build");
    let header_only = build_fixture_with_transfers(Some("from_stop_id,to_stop_id,transfer_type\n"))
        .expect("fixture should build");

    assert_eq!(labels(&header_only), labels(&missing));
    assert_eq!(header_only.route_lookup, missing.route_lookup);
}

#[test]
fn transfers_file_with_rows_still_merges() {
    let with_transfers =
        build_fixture_with_transfers(Some(FIXTURE_TRANSFERS)).expect("fixture should build");

    // Pins the direction of the empty check at load: if rows were discarded, Test Stop 4 would
    // be split as in the missing-file case
    assert_eq!(labels(&with_transfers)["501N"], "Test Stop 4");
    assert_eq!(labels(&with_transfers)["601N"], "Test Stop 4");
    assert_eq!(sorted_routes(&with_transfers, "Test Stop 4"), ["6", "7"]);
}

// transfer_type

#[test]
fn same_name_stations_merge_unless_the_transfer_is_not_possible() {
    // Four same-name pairs, each linked both ways with a different transfer_type. NYC's
    // transfers are all type 2; a draft read 2 as "not possible" and split every complex.
    let stops = "\
stop_id,stop_name,location_type,parent_station
11,Empty Type,1,
11N,Empty Type,,11
12,Empty Type,1,
12N,Empty Type,,12
21,Timed,1,
21N,Timed,,21
22,Timed,1,
22N,Timed,,22
31,Min Time,1,
31N,Min Time,,31
32,Min Time,1,
32N,Min Time,,32
41,Not Possible,1,
41N,Not Possible,,41
42,Not Possible,1,
42N,Not Possible,,42
";
    let transfers = "\
from_stop_id,to_stop_id,transfer_type
11,12,
12,11,
21,22,1
22,21,1
31,32,2
32,31,2
41,42,3
42,41,3
";
    let stop_times = "\
trip_id,stop_id
trip_1,11N
trip_2,12N
trip_1,21N
trip_2,22N
trip_1,31N
trip_2,32N
trip_1,41N
trip_2,42N
";
    let static_data = build_feed(stops, Some(transfers), stop_times).expect("feed should build");

    assert_eq!(
        labels(&static_data),
        BTreeMap::from([
            ("11N", "Empty Type"),
            ("12N", "Empty Type"),
            ("21N", "Timed"),
            ("22N", "Timed"),
            ("31N", "Min Time"),
            ("32N", "Min Time"),
            ("41N", "Not Possible (1)"),
            ("42N", "Not Possible (2)"),
        ])
    );
}

// Standalone stops

#[test]
fn standalone_stop_is_its_own_station() {
    // Metro North's shape: a boarding stop with no parent and no transfers. location_type is
    // left out entirely, which the spec allows and which reads as 0.
    let stops = "\
stop_id,stop_name,parent_station
1,Katonah,
2,Bedford Hills,
";
    let stop_times = "\
trip_id,stop_id
trip_1,1
trip_1,2
trip_2,2
";
    let static_data = build_feed(stops, None, stop_times).expect("feed should build");

    assert_eq!(
        labels(&static_data),
        BTreeMap::from([("1", "Katonah"), ("2", "Bedford Hills")])
    );
    assert_eq!(sorted_routes(&static_data, "Bedford Hills"), ["1", "2"]);
}

#[test]
fn standalone_stop_joins_a_same_name_station_it_has_a_transfer_to() {
    // Station 71 has a platform; 72 is a standalone stop with the same name. Both IDs must
    // reach the lookup: platforms through the station, the standalone stop by its own ID.
    let stops = "\
stop_id,stop_name,location_type,parent_station
71,Junction,1,
71N,Junction,0,71
72,Junction,0,
";
    let transfers = "\
from_stop_id,to_stop_id,transfer_type
71,72,0
72,71,0
";
    let stop_times = "\
trip_id,stop_id
trip_1,71N
trip_2,72
";
    let static_data = build_feed(stops, Some(transfers), stop_times).expect("feed should build");

    assert_eq!(
        labels(&static_data),
        BTreeMap::from([("71N", "Junction"), ("72", "Junction")])
    );
    assert_eq!(sorted_routes(&static_data, "Junction"), ["1", "2"]);
}

// location_type 2-4

#[test]
fn entrances_nodes_and_boarding_areas_are_left_out() {
    // Boston's shape. 81E, 81G and 81B hang off the station or its platform. 82 is an entrance
    // with no parent and the station's name: if it became a group, the shared name would put a
    // qualifier on the real station.
    let stops = "\
stop_id,stop_name,location_type,parent_station
81,Park Street,1,
81N,Park Street,0,81
81E,Park Street,2,81
81G,Park Street,3,81
81B,Park Street,4,81N
82,Park Street,2,
";
    let stop_times = "\
trip_id,stop_id
trip_1,81N
";
    // Run both grouping paths: they decide separately what becomes a group
    let transfers = "\
from_stop_id,to_stop_id,transfer_type
81,81,0
";
    for transfers in [None, Some(transfers)] {
        let static_data = build_feed(stops, transfers, stop_times).expect("feed should build");

        assert_eq!(
            labels(&static_data),
            BTreeMap::from([("81N", "Park Street")]),
            "transfers: {transfers:?}"
        );
        assert_eq!(
            static_data.route_lookup.len(),
            1,
            "transfers: {transfers:?}"
        );
    }
}

// Parsing

#[test]
fn location_type_reads_empty_as_a_stop_and_rejects_out_of_range() {
    assert_eq!(LocationType::try_from(None), Ok(LocationType::Stop));
    assert_eq!(LocationType::try_from(Some(0)), Ok(LocationType::Stop));
    assert_eq!(LocationType::try_from(Some(1)), Ok(LocationType::Station));
    assert_eq!(
        LocationType::try_from(Some(4)),
        Ok(LocationType::BoardingArea)
    );
    assert!(LocationType::try_from(Some(5)).is_err());
}

#[test]
fn transfer_type_follows_the_gtfs_numbering_and_rejects_out_of_range() {
    assert!(matches!(
        TransferType::try_from(None),
        Ok(TransferType::Recommended)
    ));
    assert!(matches!(
        TransferType::try_from(Some(2)),
        Ok(TransferType::TimedMinimum)
    ));
    assert!(matches!(
        TransferType::try_from(Some(3)),
        Ok(TransferType::NotPossible)
    ));
    assert!(matches!(
        TransferType::try_from(Some(5)),
        Ok(TransferType::SameVehicleNoInSeat)
    ));
    assert!(TransferType::try_from(Some(6)).is_err());
}

#[test]
fn out_of_range_location_type_fails_the_build() {
    let stops = "\
stop_id,stop_name,location_type,parent_station
1,Katonah,7,
";
    let result = build_feed(stops, None, "trip_id,stop_id\n");

    assert!(matches!(result, Err(GtfsStaticError::Csv(_))));
}

#[test]
fn out_of_range_transfer_type_fails_the_build() {
    // A bad row must not be read as "no transfers": that would build, with complexes split
    let result =
        build_fixture_with_transfers(Some("from_stop_id,to_stop_id,transfer_type\n501,601,7\n"));

    assert!(matches!(result, Err(GtfsStaticError::Csv(_))));
}
