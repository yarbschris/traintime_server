use crate::types::static_data;
use log::{debug, error, info, warn};
use reqwest::{Response, header::HeaderValue};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::watch;

use crate::config::ResolvedSystemConfig;
use crate::error::GtfsStaticError;
use crate::types::{gtfs::RouteID, static_data::StaticData};

pub fn setup_gtfs_static(
    tx_static_data: watch::Sender<Option<StaticData>>,
    rx_resolved_system_config: watch::Receiver<Option<ResolvedSystemConfig>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(fetch_static_handler(
        tx_static_data,
        rx_resolved_system_config,
    ))
}

async fn fetch_static_handler(
    tx_static_data: watch::Sender<Option<StaticData>>,
    mut rx_resolved_system_config: watch::Receiver<Option<ResolvedSystemConfig>>,
) {
    let mut gtfs_static_fetch_interval = tokio::time::interval(Duration::from_hours(1));
    // To compare with fetched data etag to avoid uneccessary updates
    let mut last_etag: Option<HeaderValue> = None;

    let http_client = reqwest::Client::new();

    loop {
        tokio::select! {
            _ = gtfs_static_fetch_interval.tick() => {},
            r = rx_resolved_system_config.changed() => {
                if r.is_err() {
                    break;
                }
                // Need to reset last etag if we change systems
                last_etag = None;
                // Also clear static data for old system
                clear_static_data(&tx_static_data);
            },
        }

        if let Err(e) = refresh_static_data(
            &http_client,
            &rx_resolved_system_config,
            &tx_static_data,
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
    rx_resolved_system_config: &watch::Receiver<Option<ResolvedSystemConfig>>,
    tx_static_data: &watch::Sender<Option<StaticData>>,
    last_etag: &mut Option<HeaderValue>,
) -> Result<(), GtfsStaticError> {
    // We clone the system config out of the watch channel to avoid holding the watch lock over .await calls
    let resolved_system_config_option = { rx_resolved_system_config.borrow().as_ref().cloned() };

    if let Some(resolved_system_config) = resolved_system_config_option {
        let endpoint = resolved_system_config.gtfs_static_endpoint();
        let response = fetch_gtfs_static_data(endpoint, http_client).await?;
        let new_etag = response.headers().get("etag").cloned();
        if should_skip_update(last_etag, &new_etag) {
            info!("No new static data found, skipping update...");
            return Ok(());
        }

        info!("Building retained static data structure...");
        let static_data = static_data::StaticData::build_from_bytes(
            response.bytes().await?,
            *resolved_system_config.supported_system(),
        )?;

        // System could have been changed during the build. Make sure that we built for the correct
        // system, send None if we did not
        let is_current = rx_resolved_system_config
            .borrow()
            .as_ref()
            .is_some_and(|c| c.supported_system() == &static_data.built_from_system);
        if !is_current {
            warn!("System changed during static data build, cannot send new static data");
            clear_static_data(tx_static_data);
            return Ok(());
        }

        warn_unaccounted_for_routes(&static_data, &resolved_system_config);

        info!("Sending retained static data structure...");
        tx_static_data.send_replace(Some(static_data));
        *last_etag = new_etag;
        Ok(())
    } else {
        debug!("No system config loaded, clearing static data");
        clear_static_data(tx_static_data);
        Ok(())
    }
}

// Set static_data to `None`, waking receivers only if it was `Some`
fn clear_static_data(tx_static_data: &watch::Sender<Option<StaticData>>) {
    tx_static_data.send_if_modified(|static_data| static_data.take().is_some());
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
    gtfs_static_endpoint: &str,
    http_client: &reqwest::Client,
) -> Result<Response, reqwest::Error> {
    info!("Fetching GTFS Static Data...");

    let response = http_client
        .get(gtfs_static_endpoint)
        .send()
        .await?
        .error_for_status()?;
    info!("Fetched GTFS Static Data!");
    Ok(response)
}

// Warn about routes in the static data which are not present in any endpoint's route list
// That means that the system has added a new route, and the system's yaml config
// has not yet been updated. Skip if only 1 endpoint bc route list is not required in
// this case
pub fn warn_unaccounted_for_routes(static_data: &StaticData, system: &ResolvedSystemConfig) {
    let endpoints = system.gtfs_rt_endpoints();
    if endpoints.len() <= 1 {
        return;
    }

    let all_endpoint_routes = collect_endpoint_routes(system);
    let all_static_data_routes = collect_static_data_routes(static_data);

    let routes_not_present_in_endpoint_routes: Vec<&&RouteID> = all_static_data_routes
        .difference(&all_endpoint_routes)
        .collect();

    for route in routes_not_present_in_endpoint_routes {
        warn!(
            "route {route} is present in gtfs static feeds, but no endpoint's route list in system yaml config contains it, please update firmware or raise an issue on github"
        )
    }
}

// Get all unique routes found in static data as a HashSet
fn collect_static_data_routes(static_data: &StaticData) -> HashSet<&RouteID> {
    HashSet::from_iter(static_data.route_lookup.values().flatten())
}

// Routes are stored in an Vec<Option<Vec<RouteID>>>, extract to HashSet<&RouteID>
fn collect_endpoint_routes(system: &ResolvedSystemConfig) -> HashSet<&RouteID> {
    HashSet::from_iter(
        system
            .gtfs_rt_endpoints()
            .iter()
            // Filter Map is okay because we validate that each endpoint has Some(Routes) at config parsing
            .filter_map(|endpoint| endpoint.routes.as_ref())
            .flatten(),
    )
}
