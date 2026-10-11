use crate::gtfs;
use crate::{
    error::GtfsStaticError,
    types::gtfs::{RouteID, StationName, Stop, StopID, Transfer, Trip, TripID},
};
use itertools::Itertools;
use log::info;
use prost::bytes::Bytes;
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

    pub fn build_from_bytes(bytes: Bytes) -> Result<Self, GtfsStaticError> {
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt")?);

        info!("Deserializing Stop Data...");
        let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);

        info!("Deserializing Transfer Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("transfers.txt")?);
        let transfers: Vec<Transfer> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);

        let ambiguous_parent_stop_ids = get_ambiguous_parent_stop_ids(&stops, &transfers);
        let parent_stop_id_lookup = build_parent_lookup(&stops);

        let stop_lookup = build_stop_lookup(stops);

        info!("Deserializing Trip Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt")?);
        let trips: Vec<Trip> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);
        let trip_lookup = build_trips_map(trips);

        info!("Deserializing StopTime Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt")?);

        // Temporary Raw Record which we decode into to save space
        let mut raw_record = csv::StringRecord::new();
        let headers = rdr.headers()?.clone();

        let mut route_lookup: HashMap<StationName, Vec<RouteID>> = HashMap::new();

        while rdr.read_record(&mut raw_record)? {
            let record: gtfs::StopTime = raw_record.deserialize(Some(&headers))?;
            if let Some(station_name) = stop_lookup.get(&record.stop_id)
                && let Some(route_id) = trip_lookup.get(&record.trip_id)
            {
                route_lookup
                    .entry(station_name.clone())
                    .and_modify(|routes| {
                        if !routes.contains(route_id) {
                            routes.push(route_id.clone())
                        }
                    })
                    .or_insert(vec![route_id.clone()]);
            }
        }

        info!("Building static data...");
        let static_data = StaticData {
            stop_lookup,
            route_lookup,
        };
        Ok(static_data)
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

fn get_ambiguous_parent_stop_ids<'a>(
    stops: &'a [Stop],
    transfers: &[Transfer],
) -> Vec<&'a gtfs::StopID> {
    info!("Getting ambiguous parent stop ids...");
    // Get an iterator over all parent stops (where parent_station is empty)
    let parent_stops = stops
        .iter()
        .filter(|stop| stop.parent_station.0.is_empty())
        .collect_vec();

    // Filter out stops that are not ambiguous
    parent_stops
        .iter()
        .filter(|stop_1| {
            parent_stops
                .iter()
                .any(|stop_2| stops_are_ambiguous(stop_1, stop_2, transfers))
        })
        .map(|stop| &stop.stop_id)
        .collect()
}

// In some systems (NYC), multiple stops share the same name, even if a walking transfer is not a
// feasible between the routes those shared stops (e.g. In NYC, there are multiple "86 St" stops,
// but the "86 St" stop that services the '1' route is a miles away from the "86 St" that services
// the '4', '5', '6','6X' routes. Using "transfers.txt", we identify stops which
// share a name, but are not listed in "transfers.txt" as possible transfers. Those 'ambiguous'
// stations new stop name will be displayed as "{StationName} ({route serviced})+"
fn stops_are_ambiguous(stop_1: &Stop, stop_2: &Stop, transfers: &[Transfer]) -> bool {
    !stops_are_equivalent(stop_1, stop_2)
        && stops_have_same_name(stop_1, stop_2)
        && !transfer_exists_between_stops(stop_1, stop_2, transfers)
}

fn stops_are_equivalent(stop_1: &Stop, stop_2: &Stop) -> bool {
    stop_1.stop_id == stop_2.stop_id
}
fn stops_have_same_name(stop_1: &Stop, stop_2: &Stop) -> bool {
    stop_1.stop_name.0 == stop_2.stop_name.0
}
fn transfer_exists_between_stops(stop_1: &Stop, stop_2: &Stop, transfers: &[Transfer]) -> bool {
    transfers.iter().any(|transfer| {
        transfer.from_stop_id == stop_1.stop_id && transfer.to_stop_id == stop_2.stop_id
    })
}

// Build HashMap to Lookup Station Name using Stop ID as key
fn build_stop_lookup(stops: Vec<Stop>) -> HashMap<StopID, StationName> {
    info!("Building stop lookup");
    stops
        .into_iter()
        .filter(|stop| !stop.parent_station.0.is_empty())
        .map(|stop| (stop.stop_id, stop.stop_name))
        .collect()
}

// TODO: Combine with above funciton (build_stop_lookup)?
// Build HashMap to lookup parent stop_id using child stop_id as key
fn build_parent_lookup(stops: &[Stop]) -> HashMap<&StopID, &StopID> {
    info!("Building parent lookup");
    stops
        .iter()
        .filter(|stop| !stop.parent_station.0.is_empty())
        .map(|stop| (&stop.stop_id, &stop.parent_station))
        .collect()
}

// Build HashMap to Lookup RouteID from TripID
fn build_trips_map(trips: Vec<Trip>) -> HashMap<TripID, RouteID> {
    trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect()
}
