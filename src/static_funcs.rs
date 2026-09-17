use crate::types::gtfs;
use itertools::Itertools;
use log::{error, info};
use prost::bytes::Bytes;
use reqwest::header::HeaderValue;
use std::{collections::HashMap, time::Duration};
use tokio::sync::{mpsc, watch};

use crate::config::TraintimeSystemConfig;
use crate::types::static_data::StaticData;

pub fn setup_gtfs_static(
    tx_active_static_data: watch::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    let (tx_new_static_data, rx_new_static_data) = mpsc::channel(1);

    fetch_static_handler(tx_new_static_data, rx_system_config);

    tokio::spawn(update_static_data_handler(
        rx_new_static_data,
        tx_active_static_data,
    ));
}

pub fn fetch_static_handler(
    tx_static_data: mpsc::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    let (tx_static_bytes, rx_static_bytes) = mpsc::channel(2);

    tokio::spawn(fetch_gtfs_static_data(tx_static_bytes, rx_system_config));

    tokio::spawn(parse_and_filter_gtfs_static_data(
        tx_static_data,
        rx_static_bytes,
    ));
}

pub async fn update_static_data_handler(
    mut rx_new_static_data: mpsc::Receiver<StaticData>,
    tx_active_static_data: watch::Sender<StaticData>,
) {
    while let Some(new_data) = rx_new_static_data.recv().await {
        info!("Recieved new static data");
        tx_active_static_data.send(new_data).unwrap();
        info!("Sent new static data")
    }
}

/// Make a request to the endpoint which provides gtfs static data
async fn fetch_gtfs_static_data(
    tx: mpsc::Sender<Bytes>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(1));
    let mut last_etag: HeaderValue =
        HeaderValue::from_str("none").expect("Ensure default etag is a valid HeaderValue");
    loop {
        gtfs_static_fetch_interval.tick().await;
        info!("Fetching GTFS Static Data...");
        let gtfs_static_endpoint = rx_system_config.borrow().gtfs_static_endpoint.clone();

        match reqwest::get(&gtfs_static_endpoint).await {
            Ok(response) => {
                if let Err(e) = response.error_for_status_ref() {
                    print_static_fetch_error_message(e);
                    continue;
                }

                if let Some(new_etag) = response.headers().get("etag") {
                    if last_etag == new_etag {
                        info!("No new static data detected, skipping update...");
                        continue;
                    }
                    last_etag = new_etag.clone();
                }

                info!("Fetched GTFS Static Data!");
                tx.send(response.bytes().await.unwrap()).await.unwrap();
                info!("Sent static bytes");
            }
            Err(e) => {
                print_static_fetch_error_message(e);
            }
        }
    }
}

fn print_static_fetch_error_message(e: reqwest::Error) {
    error!(
        "Failed to fetch gtfs static data.\nError: {}\nFetch will retry on regular fetch interval...",
        e.without_url(),
    )
}

async fn parse_and_filter_gtfs_static_data(
    tx_static_data: mpsc::Sender<StaticData>,
    mut rx_static_bytes: mpsc::Receiver<Bytes>,
) {
    while let Some(bytes) = rx_static_bytes.recv().await {
        info!("Recieved static bytes");
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt").unwrap());

        info!("Deserializing Stop Data...");
        let stops: Vec<gtfs::Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);

        info!("Deserializing Trip Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
        let trips: Vec<gtfs::Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);

        info!("Deserializing StopTime Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt").unwrap());
        let stop_times: Vec<gtfs::StopTime> = rdr.deserialize().collect::<Result<_, _>>().unwrap();

        info!("Building static data...");
        let data_to_send = StaticData::build_from_static_data(stops, trips, stop_times);

        info!("Sending Static Data");
        tx_static_data.send(data_to_send).await.unwrap()
    }
}

pub fn build_stop_lookup(stops: Vec<gtfs::Stop>) -> HashMap<gtfs::StopID, gtfs::StationName> {
    info!("Building Stop Lookup");
    stops
        .into_iter()
        .filter(|stop| !stop.parent_station.is_empty())
        .map(|stop| (stop.stop_id, stop.stop_name))
        .collect()
}

pub fn build_route_lookup(
    stop_times: Vec<gtfs::StopTime>,
    trips: Vec<gtfs::Trip>,
    stop_lookup: &HashMap<gtfs::StopID, gtfs::StationName>,
) -> HashMap<gtfs::StationName, Vec<gtfs::RouteID>> {
    // For stop_times, first build a mapping of stop_id -> vector of trip_ids
    info!("Building StopTime Map");
    let mut stops_map: HashMap<gtfs::StopID, Vec<gtfs::TripID>> = HashMap::new();
    for stop_time in stop_times {
        stops_map
            .entry(stop_time.stop_id)
            .or_default()
            .push(stop_time.trip_id);
    }

    // For trips, build a mapping of trip_id -> route_id
    info!("Building Trip Map");
    let trips_map: HashMap<gtfs::TripID, gtfs::RouteID> = trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect();

    // Finally, build a lookup table of station_name -> vec of route_id
    info!("Combining StopTime Map and Trip Map into Route Lookup");
    let x: HashMap<gtfs::StationName, Vec<gtfs::RouteID>> = stops_map
        .into_iter()
        .filter_map(|(stop_id, trip_ids)| {
            let stop_id = stop_lookup.get(&stop_id.0)?.clone();
            Some((
                stop_id,
                trip_ids
                    .into_iter()
                    .filter_map(|trip_id| trips_map.get(&trip_id))
                    .unique()
                    .cloned()
                    .collect(),
            ))
        })
        .collect();

    x
}
