use futures::future::join_all;
use gtfs_decode::transit_realtime::{FeedEntity, FeedMessage};
use reqwest::Response;

// For each endpoint, make a request. Put feed messages together, and return
pub async fn gtfs_rt_handler(endpoints: &Vec<String>) -> Vec<FeedEntity> {
    let mut futures = Vec::new();
    for endpoint in endpoints {
        futures.push(fetch_and_decode_gtfs_rt(endpoint.clone()))
    }
    join_all(futures).await.concat()
}

pub async fn fetch_and_decode_gtfs_rt(endpoint: String) -> Vec<FeedEntity> {
    let Ok(response) = fetch_gtfs_rt(&endpoint).await else {
        panic!("Error Fetching GTFS-RT");
    };
    dbg!("Fetched");
    let Ok(decoded) = decode_gtfs_rt(response).await else {
        panic!("Error Decoding GTFS-RT");
    };
    decoded.entity
}

async fn fetch_gtfs_rt(gtfs_rt_endpoint: &str) -> Result<Response, reqwest::Error> {
    println!("Fetching MTA Subway Line Data...");
    reqwest::get(gtfs_rt_endpoint).await
}

async fn decode_gtfs_rt(response: Response) -> Result<FeedMessage, prost::DecodeError> {
    let response_bytes = response.bytes().await.unwrap();
    <FeedMessage as prost::Message>::decode(response_bytes)
}

pub fn accumulate_entities_routes(mut entities: Vec<FeedEntity>) -> Vec<String> {
    entities.drain(..).fold(Vec::new(), |mut acc, entity| {
        if let Some(update) = entity.trip_update
            && let Some(route) = update.trip.route_id
            && !acc.contains(&route)
        {
            acc.push(route);
        }
        acc
    })
}
