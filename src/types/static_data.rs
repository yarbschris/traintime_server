use crate::config::SupportedTransitSystem;
use crate::gtfs;
use crate::{
    error::GtfsStaticError,
    types::gtfs::{RouteID, StationName, Stop, StopID, Transfer, Trip, TripID},
};
use itertools::Itertools;
use log::{error, info, warn};
use prost::bytes::Bytes;
use std::collections::HashMap;

pub struct StationGroup {
    pub station_name: StationName,
    pub route_list: Vec<RouteID>,
    pub name_is_shared_flag: bool,
}

impl StationGroup {
    pub fn new(station_name: StationName) -> Self {
        StationGroup {
            station_name,
            route_list: Vec::new(),
            name_is_shared_flag: false,
        }
    }

    pub fn display_name(&self) -> StationName {
        if !self.name_is_shared_flag {
            self.station_name.clone()
        } else {
            let routes = self.route_list.iter().sorted().join(", ");
            let new_name = format!("{} ({})", self.station_name, routes);
            StationName(new_name)
        }
    }
}

pub struct StaticData {
    pub built_from_system: SupportedTransitSystem,
    pub stop_lookup: HashMap<gtfs::StopID, gtfs::StationName>, // stop_id -> stop_name
    pub route_lookup: HashMap<gtfs::StationName, Vec<gtfs::RouteID>>, // station_name -> route_id
}

impl StaticData {
    pub fn empty(built_from_system: SupportedTransitSystem) -> Self {
        StaticData {
            built_from_system,
            stop_lookup: HashMap::new(),
            route_lookup: HashMap::new(),
        }
    }

    pub fn build_from_bytes(
        bytes: Bytes,
        built_from_system: SupportedTransitSystem,
    ) -> Result<Self, GtfsStaticError> {
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt")?);

        info!("Deserializing Stop Data...");
        let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);

        let (parent_stops, child_stops) = split_stops_into_parents_and_children(stops);

        info!("Deserializing Transfer Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("transfers.txt")?);
        let transfers: Vec<Transfer> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);

        let (mut station_groups, parent_station_group_lookup) =
            group_parents(&parent_stops, &transfers);

        let mut child_station_group_lookup =
            build_child_station_group_lookup(child_stops, &parent_station_group_lookup);

        // TODO: this is not the best solution to parent stops w/ no children, fix later :)
        // so lazy
        let parent_stations_with_no_children: HashMap<StopID, usize> = parent_station_group_lookup
            .into_iter()
            .filter(|(_, parent_index)| {
                !child_station_group_lookup
                    .values()
                    .any(|child_index| child_index == parent_index)
            })
            .map(|(parent_stop_id, parent_index)| (parent_stop_id.clone(), parent_index))
            .collect();

        child_station_group_lookup.extend(parent_stations_with_no_children);

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

        info!("Adding routes to station_groups");
        while rdr.read_record(&mut raw_record)? {
            let record: gtfs::StopTime = raw_record.deserialize(Some(&headers))?;
            let stop_id = &record.stop_id;
            if let Some(route_id) = trip_lookup.get(&record.trip_id)
                && let Some(station_group_index) = child_station_group_lookup.get(stop_id)
                && let Some(station_group) = station_groups.get_mut(*station_group_index)
                && !station_group
                    .route_list
                    .iter()
                    .any(|exisiting_route_id| exisiting_route_id == route_id)
            {
                station_group.route_list.push(route_id.clone());
            }
        }

        Ok(StaticData::build_from_station_group_and_child_group_map(
            station_groups,
            child_station_group_lookup,
            built_from_system,
        ))
    }

    fn build_from_station_group_and_child_group_map(
        station_groups: Vec<StationGroup>,
        child_station_group_lookup: HashMap<StopID, usize>,
        built_from_system: SupportedTransitSystem,
    ) -> Self {
        StaticData {
            route_lookup: build_route_lookup(&station_groups),
            stop_lookup: build_stop_lookup(station_groups, child_station_group_lookup),
            built_from_system,
        }
    }
}

// Build mapping of StopID -> Stop Display Name
fn build_stop_lookup(
    station_groups: Vec<StationGroup>,
    child_station_group_lookup: HashMap<StopID, usize>,
) -> HashMap<StopID, StationName> {
    info!("Building Stop Lookup...");
    child_station_group_lookup
        .into_iter()
        .filter_map(|(stop_id, group_index)| {
            let Some(station_group) = station_groups.get(group_index) else {
                error!("Stop {:?} has an invalid index ({}) into station group vector, this means there is an internal bug", stop_id, group_index);
                return None;
            };
            Some((stop_id, station_group.display_name()))
        })
        .collect()
}

// Build mapping of StationName -> Vec<RouteId>
fn build_route_lookup(station_groups: &[StationGroup]) -> HashMap<StationName, Vec<RouteID>> {
    info!("Building Route Lookup...");
    station_groups
        .iter()
        .map(|station_group| {
            (
                station_group.display_name(),
                station_group.route_list.clone(),
            )
        })
        .collect()
}

// Mark group names as shared if they appear more than once
fn mark_station_groups_with_shared_names(station_groups: &mut [StationGroup]) {
    let station_name_counter = build_station_name_counter(station_groups);
    station_groups.iter_mut().for_each(|station_group| {
        if let Some(count) = station_name_counter.get(&station_group.station_name)
            && *count > 1
        {
            station_group.name_is_shared_flag = true;
        }
    })
}

// Build a mapping of station_name -> num appearences in vector of StationGroup
fn build_station_name_counter(station_groups: &[StationGroup]) -> HashMap<StationName, usize> {
    let mut station_name_counter: HashMap<StationName, usize> = HashMap::new();

    station_groups.iter().for_each(|station_group| {
        station_name_counter
            .entry(station_group.station_name.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
    });
    station_name_counter
}
// Build a lookup from child_stop_id -> index into a StationGroup vector
fn build_child_station_group_lookup(
    child_stops: Vec<Stop>,
    parent_station_group_lookup: &HashMap<&StopID, usize>,
) -> HashMap<StopID, usize> {
    info!("Building Child Station Group Lookup");
    child_stops
        .into_iter()
        .filter_map(|child_stop| {
            let Some(group_position) = parent_station_group_lookup.get(&child_stop.parent_station)
            else {
                warn!(
                    "Skipping stop {:?}: parent {:?}_is not a known station",
                    child_stop.stop_id, child_stop.parent_station
                );
                return None;
            };
            Some((child_stop.stop_id, group_position.to_owned()))
        })
        .collect()
}

// In some systems (NYC), multiple stops share the same name, even if a walking transfer is not a
// feasible between the routes those shared stops (e.g. In NYC, there are multiple "86 St" stops,
// but the "86 St" stop that services the '1' route is a miles away from the "86 St" that services
// the '4', '5', '6','6X' routes. Using "transfers.txt", we identify stops which
// share a name, but are not listed in "transfers.txt" as possible transfers. Those 'ambiguous'
// stations new stop name will be displayed as "{StationName} ({route serviced})+"
//
// To this end, we separate the stations into StationGroups if they could be considered part of the
// same complex (they share a stop name, and a transfer is listed in transfers.txt)
fn group_parents<'a>(
    parent_stops: &'a Vec<Stop>,
    transfers: &[Transfer],
) -> (Vec<StationGroup>, HashMap<&'a StopID, usize>) {
    info!("Building Station Groups");
    // Transfers.txt is optional, so if it does not exist we just build one StationGroup per parent
    // station
    if transfers.is_empty() {
        return parent_stops
            .iter()
            .enumerate()
            .map(|(index, parent_stop)| {
                (
                    StationGroup::new(parent_stop.stop_name.clone()),
                    (&parent_stop.stop_id, index),
                )
            })
            .collect();
    }

    let mut station_groups: Vec<StationGroup> = Vec::new();
    let mut parent_station_group_lookup: HashMap<&StopID, usize> = HashMap::new();

    // Keep track of the current_index into the station_groups vector
    let mut current_map_index: usize = 0;

    for stop in parent_stops {
        // Skip if lookup already has an entry for this station
        if parent_station_group_lookup.contains_key(&stop.stop_id) {
            continue;
        }

        // Get stop ids that belong to the same group
        let stops_ids_in_same_group =
            find_stop_ids_that_belong_to_group(stop, parent_stops, transfers);

        // Create new station group
        let new_station_group = StationGroup::new(stop.stop_name.clone());

        {
            // Add the station group to the station group vector and add mappings from all group
            // stop_ids -> current_map_index. Increment current_map_index
            station_groups.push(new_station_group);
            stops_ids_in_same_group.into_iter().for_each(|stop_id| {
                parent_station_group_lookup.insert(stop_id, current_map_index);
            });
            current_map_index += 1;
        }
    }

    mark_station_groups_with_shared_names(&mut station_groups);

    (station_groups, parent_station_group_lookup)
}

// Given one parent stop and all parent stops, return a vector of all stop_ids that belong to the
// same StationGroup as 'target_parent_stop'
fn find_stop_ids_that_belong_to_group<'a>(
    target_parent_stop: &Stop,
    parent_stops: &'a [Stop],
    transfers: &[Transfer],
) -> Vec<&'a StopID> {
    parent_stops
        .iter()
        .filter(|other_stop| {
            stops_belong_to_same_station_group(target_parent_stop, other_stop, transfers)
        })
        .map(|stop| &stop.stop_id)
        .collect()
}

// Given a vector of stops, separate into a tuple of (Parents, Children)
fn split_stops_into_parents_and_children(stops: Vec<Stop>) -> (Vec<Stop>, Vec<Stop>) {
    stops.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut parents, mut children), stop| {
            if stop.parent_station.0.is_empty() {
                parents.push(stop)
            } else {
                children.push(stop)
            }
            (parents, children)
        },
    )
}

fn stops_belong_to_same_station_group(
    stop_1: &Stop,
    stop_2: &Stop,
    transfers: &[Transfer],
) -> bool {
    // A stop always belongs to its own group, even if transfers.txt has no self-transfer for it
    stop_1.stop_id == stop_2.stop_id
        || (stops_have_same_name(stop_1, stop_2)
            && transfer_exists_between_stops(stop_1, stop_2, transfers))
}

fn stops_have_same_name(stop_1: &Stop, stop_2: &Stop) -> bool {
    stop_1.stop_name.0 == stop_2.stop_name.0
}

fn transfer_exists_between_stops(stop_1: &Stop, stop_2: &Stop, transfers: &[Transfer]) -> bool {
    transfers.iter().any(|transfer| {
        transfer.from_stop_id == stop_1.stop_id && transfer.to_stop_id == stop_2.stop_id
    })
}

// Build HashMap to Lookup RouteID from TripID
fn build_trips_map(trips: Vec<Trip>) -> HashMap<TripID, RouteID> {
    trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect()
}
