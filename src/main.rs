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
    let (mut rx_traintime_packets, _tx_selected_station) = server::start(fallback_system).await;

    let mut i = 0;
    while let Some(mut packets) = rx_traintime_packets.recv().await {
        packets.sort_by_key(|a| a.mins_until_arrival);
        packets.reverse();
        for packet in &packets {
            println!("{}", packet);
        }
        if i == 5 {
            _tx_selected_station.send_modify(|selected_station| {
                selected_station.transit_system = SupportedTransitSystem::NycSubway;
                selected_station.station_name = StationName::from("East Broadway");
            });
        }
        i += 1;
    }
}
