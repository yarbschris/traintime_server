use prost::bytes::Bytes;
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

static MTA_GTFS_STATIC_SUPPLEMENTED_DOWNLOAD_ENDPOINT: &str =
    "https://rrgtfsfeeds.s3.amazonaws.com/gtfs_supplemented.zip";

static MTA_1_TO_7_ENDPOINT: &str =
    "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/nyct%2Fgtfs";
// static MTA_BDFM_ENDPOINT: &str =
// "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/nyct%2Fgtfs-bdfm";
// static HUDSON_YARDS_STOP_NAME: &str = "34 St-Hudson Yards";
static TIMES_SQUARE_STOP_NAME: &str = "Times Sq-42 St";
// static EAST_BROADWAY_STOP_NAME: &str = "East Broadway";

pub static TEST_ENDPOINT: &str = MTA_1_TO_7_ENDPOINT;
pub static TEST_STATIC_ENDPOINT: &str = MTA_GTFS_STATIC_SUPPLEMENTED_DOWNLOAD_ENDPOINT;
pub static TEST_STOP_NAME: &str = TIMES_SQUARE_STOP_NAME;

pub struct StaticData {
    pub relevant_stop_ids: Vec<String>,
    pub stop_lookup: HashMap<String, String>, // stop_id -> stop_name
}

pub async fn gtfs_static_handler(tx_static_data: mpsc::Sender<StaticData>) {
    let (tx_static_bytes, rx_static_bytes) = mpsc::channel(2);
    tokio::spawn(fetch_gtfs_static_data(tx_static_bytes));
    tokio::spawn(parse_and_filter_gtfs_static_data(
        tx_static_data,
        rx_static_bytes,
        TEST_STOP_NAME,
    ));
}

/// Make a request to the endpoint which provides gtfs static data
async fn fetch_gtfs_static_data(tx: mpsc::Sender<Bytes>) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(1));
    loop {
        gtfs_static_fetch_interval.tick().await;
        dbg!("Fetching GTFS Static Data...");
        let response = reqwest::get(TEST_STATIC_ENDPOINT)
            .await
            .expect("Failed to fetch static data");
        dbg!("Fetched GTFS Static Data!");
        tx.send(response.bytes().await.unwrap()).await.unwrap();
        dbg!("Sent static bytes");
    }
}

async fn parse_and_filter_gtfs_static_data(
    tx_static_data: mpsc::Sender<StaticData>,
    mut rx_static_bytes: mpsc::Receiver<Bytes>,
    chosen_stop: &str,
) {
    while let Some(bytes) = rx_static_bytes.recv().await {
        dbg!("Recieved static bytes");
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt").unwrap());
        dbg!("Deserializing Stop Data...");
        let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        dbg!("Getting Stop IDs...");
        let relevant_stop_ids = get_child_stop_ids_by_station_name(&stops, chosen_stop);
        dbg!("Building lookup...");

        let stop_lookup = stops.iter().fold(HashMap::new(), |mut acc, stop| {
            acc.insert(stop.stop_id.clone(), stop.stop_name.clone());
            acc
        });
        dbg!("Sending Static Data");
        tx_static_data
            .send(StaticData {
                relevant_stop_ids,
                stop_lookup,
            })
            .await
            .unwrap()
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Stop {
    stop_id: String,
    pub stop_name: String,
    parent_station: String,
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
// TODO: This works for NYC Subway specifically because child subway stop ids
// indicate the direction in which the train is moving. I have no clue whether relevent
// stations in other systems follow the same pattern
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
