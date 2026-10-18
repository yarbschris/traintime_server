use prost::bytes::Bytes;
use reqwest::header::HeaderValue;
use server::config::{SupportedTransitSystem, get_stop_ids_by_station_name};
use server::error::{GtfsStaticError, StationResolveError};
use server::static_funcs::should_skip_update;
use server::types::gtfs::{StationName, StopID};
use server::types::static_data::StaticData;

// Hand-written feed, see test_data/*.txt for the sources. Regenerate from test_data/ with:
// zip -X gtfs_static.zip stops.txt transfers.txt trips.txt stop_times.txt
const GTFS_STATIC_ZIP: &[u8] = include_bytes!("../test_data/gtfs_static.zip");

fn build_fixture() -> StaticData {
    StaticData::build_from_bytes(
        Bytes::from_static(GTFS_STATIC_ZIP),
        SupportedTransitSystem::NycSubway,
    )
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

fn stop_ids(
    static_data: &StaticData,
    station_name: &str,
) -> Result<Vec<StopID>, StationResolveError> {
    get_stop_ids_by_station_name(&StationName::from(station_name), static_data)
}

fn sorted_stops(static_data: &StaticData, station_name: &str) -> Vec<String> {
    let mut stops: Vec<String> = stop_ids(static_data, station_name)
        .expect("station should be in stop_lookup")
        .into_iter()
        .map(|stop| stop.0)
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

    // Parent stations (101, 201, 301, 401, 501, 601) are left out
    assert_eq!(
        stop_ids,
        [
            "101N", "101S", "201N", "201S", "301N", "401N", "501N", "601N"
        ]
    );
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
fn route_lookup_merges_stations_sharing_a_name_and_a_transfer() {
    let static_data = build_fixture();

    // Parents 501 and 601 are both named Test Stop 4 and transfers.txt links them
    assert_eq!(sorted_routes(&static_data, "Test Stop 4"), ["6", "7"]);
}

#[test]
fn route_lookup_splits_stations_sharing_a_name_without_a_transfer() {
    let static_data = build_fixture();

    // Parents 301 and 401 are both named Test Stop 3, but transfers.txt does not link them
    assert_eq!(sorted_routes(&static_data, "Test Stop 3 (4)"), ["4"]);
    assert_eq!(sorted_routes(&static_data, "Test Stop 3 (5)"), ["5"]);
    assert!(
        !static_data
            .route_lookup
            .contains_key(&StationName::from("Test Stop 3"))
    );
}

#[test]
fn station_without_a_self_transfer_is_still_built() {
    let static_data = build_fixture();

    // Parent 201 does not appear in transfers.txt at all
    assert_eq!(sorted_routes(&static_data, "Test Stop 2"), ["1", "2", "3"]);
    assert_eq!(sorted_stops(&static_data, "Test Stop 2"), ["201N", "201S"]);
}

#[test]
fn route_lookup_ignores_unknown_trips_and_stops() {
    let static_data = build_fixture();

    // test_trip_id_missing is not in trips.txt and 999N is not in stops.txt
    assert_eq!(static_data.route_lookup.len(), 5);
    assert_eq!(sorted_routes(&static_data, "Test Stop 1"), ["1"]);
}

#[test]
fn relevant_stops_are_every_platform_with_the_station_name() {
    let static_data = build_fixture();

    assert_eq!(sorted_stops(&static_data, "Test Stop 1"), ["101N", "101S"]);
    assert_eq!(sorted_stops(&static_data, "Test Stop 4"), ["501N", "601N"]);
    // Parents 301 and 401 share a name but not a transfer, so each gets its own label
    assert_eq!(sorted_stops(&static_data, "Test Stop 3 (4)"), ["301N"]);
    assert_eq!(sorted_stops(&static_data, "Test Stop 3 (5)"), ["401N"]);
    // The unqualified name belongs to neither
    assert!(matches!(
        stop_ids(&static_data, "Test Stop 3"),
        Err(StationResolveError::UnknownStation(_))
    ));
}

#[test]
fn relevant_stops_are_an_error_for_unknown_station() {
    let static_data = build_fixture();

    assert!(matches!(
        stop_ids(&static_data, "Not A Station"),
        Err(StationResolveError::UnknownStation(_))
    ));
}

#[test]
fn build_from_bytes_rejects_bytes_that_are_not_a_zip() {
    let result = StaticData::build_from_bytes(
        Bytes::from_static(b"not a zip"),
        SupportedTransitSystem::NycSubway,
    );

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
