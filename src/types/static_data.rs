use crate::gtfs;
use itertools::Itertools;
use log::info;
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

pub fn build_stop_lookup(stops: Vec<gtfs::Stop>) -> HashMap<gtfs::StopID, gtfs::StationName> {
    info!("Building Stop Lookup");
    stops
        .into_iter()
        .filter(|stop| !stop.parent_station.is_empty())
        .map(|stop| (stop.stop_id, stop.stop_name))
        .collect()
}

pub fn build_route_lookup(
    stop_times: Vec<gtfs::StopTime>,
    trips: Vec<gtfs::Trip>,
    stop_lookup: &HashMap<gtfs::StopID, gtfs::StationName>,
) -> HashMap<gtfs::StationName, Vec<gtfs::RouteID>> {
    // For stop_times, first build a mapping of stop_id -> vector of trip_ids
    info!("Building StopTime Map");
    let mut stops_map: HashMap<gtfs::StopID, Vec<gtfs::TripID>> = HashMap::new();
    for stop_time in stop_times {
        stops_map
            .entry(stop_time.stop_id)
            .or_default()
            .push(stop_time.trip_id);
    }

    // For trips, build a mapping of trip_id -> route_id
    info!("Building Trip Map");
    let trips_map: HashMap<gtfs::TripID, gtfs::RouteID> = trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect();

    // Finally, build a lookup table of station_name -> vec of route_id
    info!("Combining StopTime Map and Trip Map into Route Lookup");
    let x: HashMap<gtfs::StationName, Vec<gtfs::RouteID>> = stops_map
        .into_iter()
        .filter_map(|(stop_id, trip_ids)| {
            let stop_id = stop_lookup.get(&stop_id.0)?.clone();
            Some((
                stop_id,
                trip_ids
                    .into_iter()
                    .filter_map(|trip_id| trips_map.get(&trip_id))
                    .unique()
                    .cloned()
                    .collect(),
            ))
        })
        .collect();

    x
}
