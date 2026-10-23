use log::error;
use server::types::{status::ServerStatus, traintime_packet::TraintimePacket};
use std::collections::BTreeSet;
use std::process::exit;
mod cli;

#[tokio::main]
async fn main() {
    colog::init();

    let selection = match cli::get_selection() {
        Ok(selection) => selection,
        Err(e) => {
            error!("{e}");
            exit(1);
        }
    };

    let (mut rx_traintime_packets, tx_selection, rx_server_status, mut rx_static_data) =
        server::start(None).await.unwrap();

    tokio::spawn(receive_status(rx_server_status));

    // Sent before listing stations too: the server only fetches static data once it has a system
    let list_stations = selection.station_name.is_none();
    tx_selection.send_replace(Some(selection));

    // TODO: Temporary list stations solution. All clients are planned to
    // be interactive. For now, since cli is not interactive, interpret
    // `None` in `station_name` to mean "list all the stations"
    if list_stations {
        if rx_static_data.changed().await.is_err() {
            std::process::exit(1);
        };
        let static_data = rx_static_data.borrow();
        match static_data.as_ref() {
            Some(static_data) => {
                // Collect into a BTreeSet to deduplicate station names (multiple IDs -> same name
                // otherwise)
                let names: BTreeSet<_> = static_data.stop_lookup.values().collect();
                for name in &names {
                    println!("{}", name);
                }
            }
            None => {
                error!("No station names found.");
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    // -- Begin Actual Client

    loop {
        if rx_traintime_packets.changed().await.is_err() {
            break;
        }
        let packets = rx_traintime_packets.borrow_and_update().clone();
        draw(&packets);
    }
}

// Packets arrive soonest first; print them reversed so the soonest sits just above the prompt
fn draw(packets: &[TraintimePacket]) {
    for packet in packets.iter().rev() {
        println!("{}", packet);
    }
}

async fn receive_status(mut rx_server_status: tokio::sync::watch::Receiver<ServerStatus>) {
    println!("{}", rx_server_status.borrow().clone());
    loop {
        if rx_server_status.changed().await.is_err() {
            break;
        }
        println!("{}", rx_server_status.borrow().clone());
    }
}
