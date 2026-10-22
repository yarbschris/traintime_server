use log::error;
use server::types::{status::ServerStatus, traintime_packet::TraintimePacket};
use std::process::exit;
mod cli;

#[tokio::main]
async fn main() {
    colog::init();

    let selection = match cli::get_selection() {
        Ok(selection) => Some(selection),
        Err(e) => {
            error!("{e}");
            exit(1);
        }
    };

    let (mut rx_traintime_packets, tx_selection, rx_server_status) =
        server::start(None).await.unwrap();

    tokio::spawn(receive_status(rx_server_status));

    tx_selection.send_replace(selection);

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
