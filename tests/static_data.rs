use prost::bytes::Bytes;
use reqwest::header::HeaderValue;
use server::error::GtfsStaticError;
use server::static_funcs::should_skip_update;
use server::types::gtfs::{StationName, StopID};
use server::types::static_data::StaticData;

// Hand-written feed, see test_data/*.txt for the sources. Regenerate from test_data/ with:
// zip -X gtfs_static.zip stops.txt trips.txt stop_times.txt
const GTFS_STATIC_ZIP: &[u8] = include_bytes!("../test_data/gtfs_static.zip");

fn build_fixture() -> StaticData {
    StaticData::build_from_bytes(Bytes::from_static(GTFS_STATIC_ZIP))
        .expect("fixture zip should build")
}

fn sorted_routes(static_data: &StaticData, station_name: &str) -> Vec<String> {
    let mut routes: Vec<String> = static_data
        .route_lookup
        .get(&StationName::from(station_name))
        .expect("station should be in route_lookup")
        .iter()
        .map(|route| route.0.clone())
        .collect();
    routes.sort();
    routes
}

fn sorted_stops(static_data: &StaticData, station_name: &str) -> Vec<String> {
    let mut stops: Vec<String> = static_data
        .get_relevant_stops_to_station(station_name)
        .into_iter()
        .map(|stop| stop.0.clone())
        .collect();
    stops.sort();
    stops
}

#[test]
fn stop_lookup_contains_only_platforms() {
    let static_data = build_fixture();

    let mut stop_ids: Vec<&str> = static_data
        .stop_lookup
        .keys()
        .map(|stop| stop.0.as_str())
        .collect();
    stop_ids.sort();

    // Parent stations (101, 201, 301, 401) are left out
    assert_eq!(stop_ids, ["101N", "101S", "201N", "201S", "301N", "401N"]);
}

#[test]
fn stop_lookup_maps_platform_to_station_name() {
    let static_data = build_fixture();

    assert_eq!(
        static_data.stop_lookup.get(&StopID(String::from("201S"))),
        Some(&StationName::from("Test Stop 2"))
    );
}

#[test]
fn route_lookup_deduplicates_routes() {
    let static_data = build_fixture();

    // Four stop_times across two platforms, all on route 1
    assert_eq!(sorted_routes(&static_data, "Test Stop 1"), ["1"]);
}

#[test]
fn route_lookup_merges_routes_from_every_platform() {
    let static_data = build_fixture();

    // Route 1 only stops at 201N and route 3 only stops at 201S
    assert_eq!(sorted_routes(&static_data, "Test Stop 2"), ["1", "2", "3"]);
}

#[test]
fn route_lookup_merges_stations_sharing_a_name() {
    let static_data = build_fixture();

    // Parents 301 and 401 are both named Test Stop 3
    assert_eq!(sorted_routes(&static_data, "Test Stop 3"), ["4", "5"]);
}

#[test]
fn route_lookup_ignores_unknown_trips_and_stops() {
    let static_data = build_fixture();

    // test_trip_id_missing is not in trips.txt and 999N is not in stops.txt
    assert_eq!(static_data.route_lookup.len(), 3);
    assert_eq!(sorted_routes(&static_data, "Test Stop 1"), ["1"]);
}

#[test]
fn relevant_stops_are_every_platform_with_the_station_name() {
    let static_data = build_fixture();

    assert_eq!(sorted_stops(&static_data, "Test Stop 1"), ["101N", "101S"]);
    assert_eq!(sorted_stops(&static_data, "Test Stop 3"), ["301N", "401N"]);
}

#[test]
fn relevant_stops_is_empty_for_unknown_station() {
    let static_data = build_fixture();

    assert!(sorted_stops(&static_data, "Not A Station").is_empty());
}

#[test]
fn build_from_bytes_rejects_bytes_that_are_not_a_zip() {
    let result = StaticData::build_from_bytes(Bytes::from_static(b"not a zip"));

    assert!(matches!(result, Err(GtfsStaticError::Zip(_))));
}

#[test]
fn update_is_not_skipped_without_a_previous_etag() {
    let new_etag = Some(HeaderValue::from_static("a"));

    assert!(!should_skip_update(&None, &new_etag));
    assert!(!should_skip_update(&None, &None));
}

#[test]
fn update_is_skipped_when_etag_is_unchanged() {
    let etag = Some(HeaderValue::from_static("a"));

    assert!(should_skip_update(&etag, &etag.clone()));
}

#[test]
fn update_is_not_skipped_when_etag_changes() {
    let last_etag = Some(HeaderValue::from_static("a"));
    let new_etag = Some(HeaderValue::from_static("b"));

    assert!(!should_skip_update(&last_etag, &new_etag));
}

#[test]
fn update_is_skipped_when_etag_disappears() {
    let last_etag = Some(HeaderValue::from_static("a"));

    assert!(should_skip_update(&last_etag, &None));
}
