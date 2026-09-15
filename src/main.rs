use log::info;
use tokio::sync::{mpsc, watch};

use crate::config::SupportedTransitSystem;
use crate::static_data::StaticData;

pub mod config;
pub mod rt_data;
pub mod static_data;
pub mod traintime_packet;

#[tokio::main]
async fn main() {
    colog::init();

    // Watch channel for transit system configuration. In the future we want to be able to change
    // the target transit system (Unused _tx_system_config)
    let (_tx_system_config, rx_system_config) = watch::channel(
        config::TraintimeSystemConfig::read_config_by_system(SupportedTransitSystem::NycSubway),
    );

    // Watch channel for station config
    let (tx_station_config, mut rx_station_config) =
        watch::channel(config::SelectedStationConfig::new());

    // Watch channel for static data
    let (tx_active_static_data, rx_active_static_data) = watch::channel(StaticData::new());

    // MPSC for Traintime Packets
    let (tx_traintime_packets, mut rx_traintime_packets) = mpsc::channel(2);

    // TODO: We want to dynamically change station name, rn we just set it manually
    tx_station_config.send_modify(|x| x.station_name = String::from("East Broadway"));

    static_data::setup_gtfs_static(tx_active_static_data, rx_system_config.clone());

    tokio::spawn(config::update_endpoints_on_static_data_update(
        rx_system_config,
        rx_active_static_data.clone(),
        tx_station_config.clone(),
    ));

    // We need a relevant endpoints before we can fetch targeted data, so we wait until we have relevant endpoints
    info!("Waiting for station config to be built...");
    rx_station_config
        .wait_for(|station_config| !station_config.relevant_endpoints.is_empty())
        .await
        .unwrap();

    tokio::spawn(rt_data::gtfs_rt_handler(
        rx_station_config,
        rx_active_static_data,
        tx_traintime_packets,
    ));

    while let Some(packets) = rx_traintime_packets.recv().await {
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
