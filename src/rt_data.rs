use gtfs_decode::transit_realtime::{FeedEntity, FeedMessage};
use reqwest::Response;

pub async fn fetch_gtfs_rt(gtfs_rt_endpoint: &str) -> Result<Response, reqwest::Error> {
    println!("Fetching MTA Subway Line Data...");
    reqwest::get(gtfs_rt_endpoint).await
}

pub async fn decode_gtfs_rt(response: Response) -> Result<FeedMessage, prost::DecodeError> {
    let response_bytes = response.bytes().await.unwrap();
    <FeedMessage as prost::Message>::decode(response_bytes)
}

pub fn accumulate_routes(mut message: FeedMessage) -> Vec<String> {
    message
        .entity
        .drain(..)
        .fold(Vec::new(), |mut acc, entity| {
            if let Some(update) = entity.trip_update
                && let Some(route) = update.trip.route_id
                && !acc.contains(&route)
            {
                acc.push(route);
            }

            acc
        })
}

// For each endpoint, make a request. Put feed messages together, and return
pub async fn gtfs_rt_handler(endpoints: &Vec<String>) -> Vec<FeedEntity> {
    let mut messages = Vec::new();
    for endpoint in endpoints {
        let Ok(response) = fetch_gtfs_rt(endpoint).await else {
            dbg!("Error Fetching GTFS-RT");
            continue;
        };
        let Ok(decoded) = decode_gtfs_rt(response).await else {
            dbg!("Error Decoding GTFS-RT");
            continue;
        };
        messages.extend(decoded.entity);
    }
    messages
}
