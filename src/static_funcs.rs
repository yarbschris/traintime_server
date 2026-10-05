use crate::types::static_data;
use log::{error, info};
use reqwest::{Response, header::HeaderValue};
use std::time::Duration;
use tokio::sync::watch;

use crate::config::TraintimeSystemConfig;
use crate::error::GtfsStaticError;
use crate::types::static_data::StaticData;

pub fn setup_gtfs_static(
    tx_active_static_data: watch::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(fetch_static_handler(
        tx_active_static_data,
        rx_system_config,
    ))
}

async fn fetch_static_handler(
    tx_active_static_data: watch::Sender<StaticData>,
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(1));
    // To compare with fetched data etag to avoid uneccessary updates
    let mut last_etag: Option<HeaderValue> = None;

    let http_client = reqwest::Client::new();

    loop {
        gtfs_static_fetch_interval.tick().await;
        if let Err(e) = refresh_static_data(
            &http_client,
            &rx_system_config,
            &tx_active_static_data,
            &mut last_etag,
        )
        .await
        {
            error!("Error refreshing static data, will retry on regular interval: {e}");
        }
    }
}

// Fetch -> Check if update is needed -> Build retained structure -> Send -> Update etag
async fn refresh_static_data(
    http_client: &reqwest::Client,
    rx_system_config: &watch::Receiver<TraintimeSystemConfig>,
    tx_active_static_data: &watch::Sender<StaticData>,
    last_etag: &mut Option<HeaderValue>,
) -> Result<(), GtfsStaticError> {
    let response = fetch_gtfs_static_data(rx_system_config.clone(), http_client).await?;
    let new_etag = response.headers().get("etag").cloned();
    if should_skip_update(last_etag, &new_etag) {
        info!("No new static data found, skipping update...");
        return Ok(());
    }

    info!("Building retained static data strucutre");
    let static_data = static_data::StaticData::build_from_bytes(response.bytes().await?)?;
    info!("Sending retained static data");
    if let Err(e) = tx_active_static_data.send(static_data) {
        return Err(GtfsStaticError::Send(e));
    }
    *last_etag = new_etag;
    Ok(())
}

// Don't update static data if new etag exists and is same as last_etag, else we should update
pub fn should_skip_update(last: &Option<HeaderValue>, new: &Option<HeaderValue>) -> bool {
    match (last, new) {
        (Some(last_etag), Some(new_etag)) => last_etag == new_etag,
        // Always attempt rebuild if we don't have a previous etag
        (None, _) => false,
        // Do not attempt rebuild if there is no new etag
        (Some(_), None) => true,
    }
}

// Make a request to the endpoint which provides gtfs static data
async fn fetch_gtfs_static_data(
    rx_system_config: watch::Receiver<TraintimeSystemConfig>,
    http_client: &reqwest::Client,
) -> Result<Response, reqwest::Error> {
    info!("Fetching GTFS Static Data...");
    let gtfs_static_endpoint = rx_system_config.borrow().gtfs_static_endpoint.clone();

    let response = http_client
        .get(&gtfs_static_endpoint)
        .send()
        .await?
        .error_for_status()?;
    info!("Fetched GTFS Static Data!");
    Ok(response)
}
