#[derive(Debug, serde::Deserialize)]
pub struct Stop {
    pub stop_id: StopID,
    pub stop_name: String,
    pub parent_station: String,
}

#[derive(Debug, Clone, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]

pub struct RouteID(pub String);

#[derive(Debug, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]
pub struct TripID(pub String);

#[derive(Debug, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]
pub struct StopID(pub String);

impl std::borrow::Borrow<String> for StopID {
    fn borrow(&self) -> &String {
        &self.0
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Trip {
    pub route_id: RouteID,
    pub trip_id: TripID,
}

#[derive(Debug, serde::Deserialize)]
pub struct StopTime {
    pub trip_id: TripID,
    pub stop_id: StopID,
}

#[allow(unused)]
#[derive(Debug, serde::Deserialize)]
struct Route {
    route_id: String,
    agency_id: String,
    route_short_name: String,
    route_long_name: String,
    route_desc: String,
    route_type: String,
    route_url: String,
    route_color: String,
    route_text_color: String,
    route_sort_order: String,
}
