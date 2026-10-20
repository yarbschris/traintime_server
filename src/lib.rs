#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod config;
pub mod error;
pub mod rt_funcs;
pub mod static_funcs;
pub mod types;

use std::time::Duration;

use crate::config::Selection;
use crate::error::StartError;
use crate::types::{gtfs, traintime_packet::TraintimePacket};
use log::info;
use tokio::sync::{mpsc, watch};

/// Wires up the static-data and realtime pipelines, and returns the channel
/// that decoded packets arrive on. Spawns background tasks onto the current
/// tokio runtime; the caller decides what to do with the packets.
pub async fn start(
    selection: Option<Selection>,
) -> Result<
    (
        mpsc::Receiver<Vec<TraintimePacket>>,
        watch::Sender<Option<Selection>>,
    ),
    StartError,
> {
    info!("Starting Server");

    let (tx_static_data, rx_static_data) = watch::channel(None);
    let (resolver_channels, _resolve_task_join_handle) =
        config::setup_config(selection, rx_static_data.clone());

    // MPSC for Traintime Packets
    let (tx_traintime_packets, rx_traintime_packets) = mpsc::channel(2);

    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()?;

    let _static_join_handle = static_funcs::setup_gtfs_static(
        tx_static_data,
        resolver_channels.rx_resolved_system_config.clone(),
        http_client.clone(),
    );

    let _rt_join_handle = rt_funcs::setup_gtfs_rt(
        resolver_channels.rx_resolved_station,
        rx_static_data,
        tx_traintime_packets,
        http_client,
    );

    Ok((rx_traintime_packets, resolver_channels.tx_selection))
}
