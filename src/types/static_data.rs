use crate::gtfs;
use crate::static_funcs::{build_route_lookup, build_stop_lookup};
use std::collections::HashMap;

pub struct StaticData {
    pub stop_lookup: HashMap<gtfs::StopID, gtfs::StationName>, // stop_id -> stop_name
    pub route_lookup: HashMap<gtfs::StationName, Vec<gtfs::RouteID>>, // station_name -> route_id
}

impl StaticData {
    pub fn new() -> Self {
        StaticData {
            stop_lookup: HashMap::new(),
            route_lookup: HashMap::new(),
        }
    }

    pub fn build_from_static_data(
        stops: Vec<gtfs::Stop>,
        trips: Vec<gtfs::Trip>,
        stop_times: Vec<gtfs::StopTime>,
    ) -> Self {
        let stop_lookup = build_stop_lookup(stops);
        let route_lookup = build_route_lookup(stop_times, trips, &stop_lookup);
        StaticData {
            stop_lookup,
            route_lookup,
        }
    }

    pub fn get_relevant_stops_to_station(&self, target_stop_name: &str) -> Vec<&gtfs::StopID> {
        self.stop_lookup
            .iter()
            .filter(|(_, stop_name)| stop_name.0.as_str() == target_stop_name)
            .map(|(stop_id, _)| stop_id)
            .collect::<Vec<&gtfs::StopID>>()
    }
}

impl Default for StaticData {
    fn default() -> Self {
        Self::new()
    }
}
