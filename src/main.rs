use server::{
    config::{Selection, SupportedTransitSystem},
    types::{gtfs::StationName, traintime_packet::TraintimePacket},
};

#[tokio::main]
async fn main() {
    colog::init();

    let station_name = StationName::from("Katonah");
    let transit_system = SupportedTransitSystem::MtaMetroNorth;
    let (mut rx_traintime_packets, tx_selection) = server::start(None).await.unwrap();

    tx_selection.send_replace(Some(Selection {
        station_name: Some(station_name),
        transit_system,
    }));

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
