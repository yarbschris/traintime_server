use server::{config::SupportedTransitSystem, types::gtfs::StationName};

#[tokio::main]
async fn main() {
    colog::init();

    let station_name = StationName::from("Katonah");
    let system = SupportedTransitSystem::MtaMetroNorth;
    let (mut rx_traintime_packets, _tx_selected_station) =
        server::start(station_name, system).await.unwrap();

    while let Some(mut packets) = rx_traintime_packets.recv().await {
        packets.sort_by_key(|a| a.mins_until_arrival);
        packets.reverse();
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
