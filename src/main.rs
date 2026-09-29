#[tokio::main]
async fn main() {
    colog::init();

    let mut rx_traintime_packets = server::start().await;

    while let Some(packets) = rx_traintime_packets.recv().await {
        for packet in &packets {
            println!("{}", packet);
        }
    }
}
