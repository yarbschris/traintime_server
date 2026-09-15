use crate::types::gtfs::{self, StopID};
use itertools::Itertools;
use log::info;
use prost::bytes::Bytes;
use std::{collections::HashMap, time::Duration};
use tokio::sync::{mpsc, watch};

use crate::config::TraintimeSystemConfig;

pub struct StaticData {
    pub stop_lookup: HashMap<gtfs::StopID, String>, // stop_id -> stop_name
    pub route_lookup: HashMap<String, Vec<gtfs::RouteID>>, // station_name -> route_id
}

impl StaticData {
    pub fn new() -> Self {
        StaticData {
            stop_lookup: HashMap::new(),
            route_lookup: HashMap::new(),
        }
    }

    fn build_from_static_data(
        stops: Vec<gtfs::Stop>,
        trips: Vec<gtfs::Trip>,
        stop_times: Vec<gtfs::StopTime>,
    ) -> Self {
        let stop_lookup = build_stop_lookup(stops);
        let route_lookup = build_route_lookup(stop_times, trips, &stop_lookup);
        StaticData {
            stop_lookup,
            route_lookup,
        }
    }

    pub fn get_relevant_stops_to_station(&self, target_stop_name: &str) -> Vec<&StopID> {
        self.stop_lookup
            .iter()
            .filter(|(_, stop_name)| stop_name.as_str() == target_stop_name)
            .map(|(stop_id, _)| stop_id)
            .collect::<Vec<&StopID>>()
    }
}

impl Default for StaticData {
    fn default() -> Self {
        Self::new()
    }
}

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
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(2));
    loop {
        gtfs_static_fetch_interval.tick().await;
        info!("Fetching GTFS Static Data...");
        let gtfs_static_endpoint = rx_system_config.borrow().gtfs_static_endpoint.clone();
        let response = reqwest::get(&gtfs_static_endpoint)
            .await
            .expect("Failed to fetch static data");
        info!("Fetched GTFS Static Data!");
        tx.send(response.bytes().await.unwrap()).await.unwrap();
        info!("Sent static bytes");
    }
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

fn build_stop_lookup(stops: Vec<gtfs::Stop>) -> HashMap<gtfs::StopID, String> {
    info!("Building Stop Lookup");
    stops
        .into_iter()
        .filter(|stop| !stop.parent_station.is_empty())
        .map(|stop| (stop.stop_id, stop.stop_name))
        .collect()
}

fn build_route_lookup(
    stop_times: Vec<gtfs::StopTime>,
    trips: Vec<gtfs::Trip>,
    stop_lookup: &HashMap<gtfs::StopID, String>,
) -> HashMap<String, Vec<gtfs::RouteID>> {
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
    let x: HashMap<String, Vec<gtfs::RouteID>> = stops_map
        .into_iter()
        .map(|(stop_id, trip_ids)| {
            (
                stop_lookup.get(&stop_id.0).unwrap().clone(),
                trip_ids
                    .into_iter()
                    .filter_map(|trip_id| trips_map.get(&trip_id))
                    .unique()
                    .cloned()
                    .collect(),
            )
        })
        .collect();

    x
}
