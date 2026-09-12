use itertools::Itertools;
use log::info;
use prost::bytes::Bytes;
use std::{collections::HashMap, time::Duration};
use tokio::sync::{mpsc, watch};

static TIMES_SQUARE_STOP_NAME: &str = "Times Sq-42 St";
pub static TEST_STOP_NAME: &str = TIMES_SQUARE_STOP_NAME;

pub struct StaticData {
    pub stop_lookup: HashMap<String, String>, // stop_id -> stop_name
    pub route_lookup: HashMap<String, Vec<String>>,
}

impl StaticData {
    pub fn new() -> Self {
        StaticData {
            stop_lookup: HashMap::new(),
            route_lookup: HashMap::new(),
        }
    }

    fn build_from_static_data(
        stops: Vec<Stop>,
        trips: Vec<Trip>,
        stop_times: Vec<StopTime>,
    ) -> Self {
        let stop_lookup = build_stop_lookup(stops);
        let route_lookup = build_route_lookup(stop_times, trips, &stop_lookup);
        StaticData {
            stop_lookup,
            route_lookup,
        }
    }

    pub fn get_relevant_stops_to_station(&self, target_stop_name: &str) -> Vec<&String> {
        self.stop_lookup
            .iter()
            .filter_map(|(stop_id, stop_name)| {
                if stop_name == target_stop_name {
                    Some(stop_id)
                } else {
                    None
                }
            })
            .collect::<Vec<&String>>()
    }
}

impl Default for StaticData {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn setup_gtfs_static(
    tx_active_static_data: watch::Sender<StaticData>,
    gtfs_static_endpoint: String,
) {
    let (tx_new_static_data, rx_new_static_data) = mpsc::channel(1);

    fetch_static_handler(tx_new_static_data, gtfs_static_endpoint).await;

    tokio::spawn(update_static_data_handler(
        rx_new_static_data,
        tx_active_static_data,
    ));
}

pub async fn fetch_static_handler(
    tx_static_data: mpsc::Sender<StaticData>,
    gtfs_static_endpoint: String,
) {
    let (tx_static_bytes, rx_static_bytes) = mpsc::channel(2);

    tokio::spawn(fetch_gtfs_static_data(
        tx_static_bytes,
        gtfs_static_endpoint,
    ));

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
async fn fetch_gtfs_static_data(tx: mpsc::Sender<Bytes>, gtfs_static_endpoint: String) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(2));
    loop {
        gtfs_static_fetch_interval.tick().await;
        info!("Fetching GTFS Static Data...");
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
        let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);

        info!("Deserializing Trip Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
        let trips: Vec<Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);

        info!("Deserializing StopTime Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt").unwrap());
        let stop_times: Vec<StopTime> = rdr.deserialize().collect::<Result<_, _>>().unwrap();

        info!("Building static data...");
        let data_to_send = StaticData::build_from_static_data(stops, trips, stop_times);

        info!("Sending Static Data");
        tx_static_data.send(data_to_send).await.unwrap()
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Stop {
    stop_id: String,
    pub stop_name: String,
    parent_station: String,
}

fn build_stop_lookup(mut stops: Vec<Stop>) -> HashMap<String, String> {
    info!("Building Stop Lookup");
    stops.drain(..).fold(HashMap::new(), |mut acc, stop| {
        if stop.parent_station.is_empty() {
            return acc;
        }
        acc.insert(stop.stop_id, stop.stop_name);
        acc
    })
}

pub fn get_unique_station_names(stops: &[Stop]) -> Vec<&String> {
    stops.iter().fold(Vec::new(), |mut acc, stop| {
        if acc.contains(&&stop.stop_name) {
            acc
        } else {
            acc.push(&stop.stop_name);
            acc
        }
    })
}

// Given a station name, get all stop ids where parent field is not none (a child station)
pub fn get_child_stop_ids_by_station_name(stops: &[Stop], stop_name: &str) -> Vec<String> {
    stops.iter().fold(Vec::new(), |mut acc, stop| {
        if stop.stop_name == stop_name && !stop.parent_station.is_empty() {
            acc.push(stop.stop_id.clone());
            acc
        } else {
            acc
        }
    })
}

#[derive(Debug, serde::Deserialize)]
struct Trip {
    route_id: String,
    trip_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct StopTime {
    trip_id: String,
    stop_id: String,
}

fn build_route_lookup(
    mut stop_times: Vec<StopTime>,
    mut trips: Vec<Trip>,
    stop_lookup: &HashMap<String, String>,
) -> HashMap<String, Vec<String>> {
    // For stop_times, first build a mapping of stop_id -> vector of trip_ids
    info!("Building StopTime Map");
    let mut stops_map: HashMap<String, Vec<String>> =
        stop_times.drain(..).fold(HashMap::new(), |mut acc, trip| {
            if let Some(vec) = acc.get_mut(&trip.stop_id) {
                vec.push(trip.trip_id);
                acc
            } else {
                acc.insert(trip.stop_id, vec![trip.trip_id]);
                acc
            }
        });

    // For trips, build a mapping of trip_id -> route_id
    info!("Building Trip Map");
    let trips_map: HashMap<String, String> =
        trips.drain(..).fold(HashMap::new(), |mut acc, stop_time| {
            acc.insert(stop_time.trip_id, stop_time.route_id);
            acc
        });

    // Finally, build a lookup table of station_name -> vec of route_id
    info!("Combining StopTime Map and Trip Map into Route Lookup");
    stops_map.drain().fold(
        HashMap::new(),
        |mut acc: HashMap<String, Vec<String>>, (stop_id, trip_ids)| {
            let key = stop_lookup.get(&stop_id).unwrap();
            let values = trip_ids
                .iter()
                .filter_map(|trip_id| {
                    let x = trips_map.get(trip_id)?;
                    Some(x.clone())
                })
                .unique()
                .collect::<Vec<String>>();
            if let Some(vec) = acc.get_mut(key) {
                for value in &values {
                    if vec.contains(value) {
                        return acc;
                    }
                }
                vec.extend(values);
            } else {
                acc.insert(key.clone(), values);
            }
            acc
        },
    )
}

#[allow(unused)]
#[derive(Debug, serde::Deserialize)]
struct Route {
    route_id: String,
    agency_id: String,
    route_short_name: String,
    route_long_name: String,
    route_desc: String,
    route_type: String,
    route_url: String,
    route_color: String,
    route_text_color: String,
    route_sort_order: String,
}
