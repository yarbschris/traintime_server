use server::{
    config::{Selection, SupportedTransitSystem},
    types::gtfs::StationName,
};

#[tokio::main]
async fn main() {
    colog::init();

    let station_name = StationName::from("Katonah");
    let transit_system = SupportedTransitSystem::MtaMetroNorth;
    let (mut rx_traintime_packets, tx_selection) = server::start(None).await;

    tx_selection.send_replace(Some(Selection {
        station_name: Some(station_name),
        transit_system,
    }));

    while let Some(mut packets) = rx_traintime_packets.recv().await {
        packets.sort_by_key(|a| a.mins_until_arrival);
        packets.reverse();
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
