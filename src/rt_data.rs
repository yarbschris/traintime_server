use gtfs_decode::transit_realtime::FeedMessage;
use reqwest::Response;

// TODO: Make only one request, wrap in function that requests multiple itmes
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
