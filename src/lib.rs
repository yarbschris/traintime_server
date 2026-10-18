#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod config;
pub mod error;
pub mod rt_funcs;
pub mod static_funcs;
pub mod types;

use crate::config::SelectedStation;
use crate::types::{gtfs, traintime_packet::TraintimePacket};
use error::StartError;
use log::info;
use tokio::sync::{mpsc, watch};

/// Wires up the static-data and realtime pipelines, and returns the channel
/// that decoded packets arrive on. Spawns background tasks onto the current
/// tokio runtime; the caller decides what to do with the packets.
pub async fn start(
    fallback_station: SelectedStation,
) -> Result<
    (
        mpsc::Receiver<Vec<TraintimePacket>>,
        watch::Sender<SelectedStation>,
    ),
    StartError,
> {
    info!("Starting Server");

    let (tx_static_data, rx_static_data) = watch::channel(None);

    let (
        tx_selected_station,
        rx_resolved_system_config,
        rx_resolved_station,
        _resolve_task_join_handle,
    ) = config::setup_config(fallback_station, rx_static_data.clone())?;

    // MPSC for Traintime Packets
    let (tx_traintime_packets, rx_traintime_packets) = mpsc::channel(2);

    let _static_join_handle =
        static_funcs::setup_gtfs_static(tx_static_data, rx_resolved_system_config.clone());

    let _rt_join_handle =
        rt_funcs::setup_gtfs_rt(rx_resolved_station, rx_static_data, tx_traintime_packets);

    Ok((rx_traintime_packets, tx_selected_station))
}
