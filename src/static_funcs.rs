use crate::types::gtfs;
use log::{error, info};
use prost::bytes::Bytes;
use reqwest::{Response, header::HeaderValue};
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
                if let Some(new_etag) = response.headers().get("etag") {
                    if last_etag == new_etag {
                        info!("No new static data found, skipping update...");
                        continue;
                    }
                    last_etag = new_etag.clone();
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

    info!("Deserializing Trip Data...");
    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
    let trips: Vec<gtfs::Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
    drop(rdr);

    info!("Deserializing StopTime Data...");
    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt").unwrap());
    let stop_times: Vec<gtfs::StopTime> = rdr.deserialize().collect::<Result<_, _>>().unwrap();

    info!("Building static data...");
    StaticData::build_from_static_data(stops, trips, stop_times)
}
