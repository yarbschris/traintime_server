use server::{config::SupportedTransitSystem, types::gtfs::StationName};

#[tokio::main]
async fn main() {
    colog::init();

    let station_name = StationName::from("East Broadway");
    let system = SupportedTransitSystem::NycSubway;
    let (mut rx_traintime_packets, _tx_selected_station) =
        server::start(station_name, system).await.unwrap();

    while let Some(packets) = rx_traintime_packets.recv().await {
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
