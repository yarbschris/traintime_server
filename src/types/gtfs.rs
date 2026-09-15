use std::fmt::Display;

#[derive(Debug, serde::Deserialize)]
pub struct Stop {
    pub stop_id: StopID,
    pub stop_name: StationName,
    pub parent_station: String,
}

#[derive(Debug, Clone, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]
pub struct StationName(pub String);
impl StationName {
    pub fn new() -> Self {
        StationName(String::new())
    }

    pub fn from(name: &str) -> Self {
        StationName(String::from(name))
    }
}

impl Display for StationName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]
pub struct RouteID(pub String);

impl Display for RouteID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
