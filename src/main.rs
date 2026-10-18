use server::{
    config::{SelectedStation, SupportedTransitSystem},
    types::gtfs::StationName,
};

#[tokio::main]
async fn main() {
    colog::init();

    let station_name = StationName::from("Katonah");
    let transit_system = SupportedTransitSystem::MtaMetroNorth;
    let fallback_system = SelectedStation {
        station_name,
        transit_system,
    };
    let (mut rx_traintime_packets, _tx_selected_station) =
        server::start(fallback_system).await.unwrap();

    while let Some(mut packets) = rx_traintime_packets.recv().await {
        packets.sort_by_key(|a| a.mins_until_arrival);
        packets.reverse();
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
