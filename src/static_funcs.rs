use crate::types::{gtfs, static_data};
use log::{error, info};
use prost::bytes::Bytes;
use reqwest::{Response, header::HeaderValue};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::watch;

use crate::config::TraintimeSystemConfig;
use crate::types::static_data::StaticData;

pub fn setup_gtfs_static(
    tx_active_static_data: watch::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    tokio::spawn(fetch_static_handler(
        tx_active_static_data,
        rx_system_config,
    ));
}

async fn fetch_static_handler(
    tx_active_static_data: watch::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(1));
    // To compare with fetched data etag to avoid uneccessary updates
    let mut last_etag: HeaderValue =
        HeaderValue::from_str("none").expect("Ensure default etag is a valid HeaderValue");

    loop {
        gtfs_static_fetch_interval.tick().await;
        match fetch_gtfs_static_data(rx_system_config.clone()).await {
            Ok(response) => {
                let new_etag = response.headers().get("etag");

                if should_skip_update(&last_etag, new_etag) {
                    info!("No new static data found, skipping update...");
                    continue;
                }

                if let Some(etag) = new_etag {
                    last_etag = etag.clone();
                }

                let static_data =
                    parse_and_filter_gtfs_static_data(response.bytes().await.unwrap());

                tx_active_static_data
                    .send(static_data)
                    .expect("Failed to send static data from static data fetch handler");
            }

            Err(e) => {
                print_static_fetch_error_message(e);
                continue;
            }
        }
    }
}

// Don't update static data if new etag exists and is same as last_etag, else we should update
fn should_skip_update(last_etag: &HeaderValue, new: Option<&HeaderValue>) -> bool {
    if let Some(new_etag) = new
        && last_etag == new_etag
    {
        return true;
    }
    false
}

/// Make a request to the endpoint which provides gtfs static data
async fn fetch_gtfs_static_data(
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) -> Result<Response, reqwest::Error> {
    info!("Fetching GTFS Static Data...");
    let gtfs_static_endpoint = rx_system_config.borrow().gtfs_static_endpoint.clone();

    match reqwest::get(&gtfs_static_endpoint).await {
        Ok(response) => {
            // TODO: More concrete HTTP Response handling
            response.error_for_status_ref()?;
            info!("Fetched GTFS Static Data!");
            Ok(response)
        }
        Err(e) => Err(e),
    }
}

fn print_static_fetch_error_message(e: reqwest::Error) {
    error!(
        "Failed to fetch gtfs static data.\nError: {}\nFetch will retry on regular fetch interval...",
        e.without_url(),
    )
}

fn parse_and_filter_gtfs_static_data(static_data_bytes: Bytes) -> StaticData {
    info!("Recieved static bytes");
    let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(static_data_bytes)).unwrap();
    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt").unwrap());

    info!("Deserializing Stop Data...");
    let stops: Vec<gtfs::Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
    drop(rdr);
    let stop_lookup = static_data::build_stop_lookup(stops);

    info!("Deserializing Trip Data...");
    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
    let trips: Vec<gtfs::Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
    drop(rdr);
    let trip_lookup = static_data::build_trips_map(trips);

    info!("Deserializing StopTime Data...");
    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt").unwrap());
    let mut raw_record = csv::StringRecord::new();
    let headers = rdr
        .headers()
        .expect("Headers Missing, Cannot Deserialize")
        .clone();

    let mut route_lookup: HashMap<gtfs::StationName, Vec<gtfs::RouteID>> = HashMap::new();

    while rdr
        .read_record(&mut raw_record)
        .expect("Error reading raw stop time record")
    {
        let record: gtfs::StopTime = raw_record
            .deserialize(Some(&headers))
            .expect("Could not deserialize stop time record into StopTime");
        let station_name = stop_lookup
            .get(&record.stop_id)
            .expect("Unabled to get station_name")
            .clone();
        let route_id = trip_lookup
            .get(&record.trip_id)
            .expect("Unable to get route_id");

        route_lookup
            .entry(station_name)
            .and_modify(|routes| {
                if !routes.contains(route_id) {
                    routes.push(route_id.clone())
                }
            })
            .or_insert(vec![route_id.clone()]);
    }

    info!("Building static data...");
    StaticData {
        stop_lookup,
        route_lookup,
    }
}
