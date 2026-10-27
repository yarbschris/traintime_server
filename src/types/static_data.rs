use crate::config::SupportedTransitSystem;
use crate::gtfs;
use crate::types::gtfs::{LocationType, TransferType};
use crate::{
    error::GtfsStaticError,
    types::{
        generation::Generation,
        gtfs::{RouteID, StationName, Stop, StopID, Transfer, Trip, TripID},
    },
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
    pub generation: Generation, // Used to ensure StaticData and ResolvedStation are synced
}

impl StaticData {
    /// FOR TEST USE ONLY
    pub fn empty(built_from_system: SupportedTransitSystem) -> Self {
        StaticData {
            built_from_system,
            stop_lookup: HashMap::new(),
            route_lookup: HashMap::new(),
            generation: Generation(0),
        }
    }

    pub fn build_from_bytes(
        bytes: Bytes,
        built_from_system: SupportedTransitSystem,
        generation: Generation,
    ) -> Result<Self, GtfsStaticError> {
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt")?);

        info!("Deserializing Stop Data...");
        let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>()?;
        drop(rdr);

        info!("Deserializing Transfer Data...");

        let transfers = match zip_reader.by_name("transfers.txt") {
            Ok(file) => {
                let mut rdr = csv::Reader::from_reader(file);
                let transfers_vec: Vec<Transfer> = rdr.deserialize().collect::<Result<_, _>>()?;
                Some(transfers_vec).filter(|vec| !vec.is_empty())
            }
            Err(zip::result::ZipError::FileNotFound) => {
                info!("transfers.txt not present in static data");
                None
            }
            Err(e) => return Err(e.into()),
        };

        // We'll group stations and standalones into groups, then, because platforms
        // have a parent stop, we can add platforms to their groups by parent's stop_id
        let (mut station_groups, stop_group_lookup) = group_stations(stops, transfers.as_deref());

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
                && let Some(station_group_index) = stop_group_lookup.get(stop_id)
                && let Some(station_group) = station_groups.get_mut(*station_group_index)
                && !station_group
                    .route_list
                    .iter()
                    .any(|exisiting_route_id| exisiting_route_id == route_id)
            {
                station_group.route_list.push(route_id.clone());
            }
        }

        Ok(StaticData {
            route_lookup: build_route_lookup(&station_groups),
            stop_lookup: build_stop_lookup(station_groups, stop_group_lookup),
            built_from_system,
            generation,
        })
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

fn group_stations(
    stops: Vec<Stop>,
    transfers: Option<&[Transfer]>,
) -> (Vec<StationGroup>, HashMap<StopID, usize>) {
    // First, we split by child and parent stop. Standalone stops are put into parents
    // because transfers are possible between parents and standalones, so we create groups first,
    // then add the child stops to the groups
    let (parent_stops, child_stops) = split_stops_into_parents_and_children(stops);
    let (station_groups, station_group_lookup, standalone_group_lookup) =
        group_parents(parent_stops, transfers);

    let mut child_station_group_lookup =
        build_child_station_group_lookup(child_stops, &station_group_lookup);

    child_station_group_lookup.extend(standalone_group_lookup);

    (station_groups, child_station_group_lookup)
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
fn group_parents(
    parent_stops: Vec<Stop>,
    transfers: Option<&[Transfer]>,
) -> (
    Vec<StationGroup>,
    HashMap<StopID, usize>,
    HashMap<StopID, usize>,
) {
    info!("Building Station Groups");
    // transfers.txt is optional, so if it does not exist or is empty we just build one StationGroup per parent
    // station
    let mut station_group_lookup = HashMap::new();
    let mut standalone_group_lookup = HashMap::new();
    let mut station_groups = Vec::new();

    let Some(transfers) = transfers else {
        for stop in parent_stops.into_iter() {
            let new_index = station_groups.len();
            match stop.location_type {
                LocationType::Stop => {
                    station_groups.push(StationGroup::new(stop.stop_name.clone()));
                    standalone_group_lookup.insert(stop.stop_id, new_index);
                }
                LocationType::Station => {
                    station_groups.push(StationGroup::new(stop.stop_name.clone()));
                    station_group_lookup.insert(stop.stop_id, new_index);
                }
                // Already filtered out by split
                _ => {}
            }
        }
        mark_station_groups_with_shared_names(&mut station_groups);
        return (
            station_groups,
            station_group_lookup,
            standalone_group_lookup,
        );
    };

    // Keep track of the current_index into the station_groups vector
    let mut current_map_index: usize = 0;

    for stop in &parent_stops {
        // Skip if either lookup already has an entry for this station
        if standalone_group_lookup.contains_key(&stop.stop_id)
            || station_group_lookup.contains_key(&stop.stop_id)
        {
            continue;
        }

        // Get stop ids that belong to the same group
        let stops_in_same_group = find_stops_that_belong_to_group(stop, &parent_stops, transfers);

        // Create new station group
        let new_station_group = StationGroup::new(stop.stop_name.clone());

        {
            // Add the station group to the station group vector and add mappings from all group
            // stop_ids -> current_map_index. Increment current_map_index
            station_groups.push(new_station_group);
            for group_stop in stops_in_same_group {
                match group_stop.location_type {
                    LocationType::Stop => {
                        standalone_group_lookup
                            .insert(group_stop.stop_id.clone(), current_map_index);
                    }
                    LocationType::Station => {
                        station_group_lookup.insert(group_stop.stop_id.clone(), current_map_index);
                    }
                    // Already filtered out by split
                    _ => {}
                }
            }
            current_map_index += 1;
        }
    }

    mark_station_groups_with_shared_names(&mut station_groups);

    (
        station_groups,
        station_group_lookup,
        standalone_group_lookup,
    )
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

// Given one parent stop and all parent stops, return a vector of all stop_ids that belong to the
// same StationGroup as 'target_parent_stop'
fn find_stops_that_belong_to_group<'a>(
    target_parent_stop: &Stop,
    parent_stops: &'a [Stop],
    transfers: &[Transfer],
) -> Vec<&'a Stop> {
    parent_stops
        .iter()
        .filter(|other_stop| {
            stops_belong_to_same_station_group(target_parent_stop, other_stop, transfers)
        })
        .collect()
}

// Build a lookup from child_stop_id -> index into a StationGroup vector
fn build_child_station_group_lookup(
    child_stops: Vec<Stop>,
    parent_station_group_lookup: &HashMap<StopID, usize>,
) -> HashMap<StopID, usize> {
    info!("Building Child Station Group Lookup");
    child_stops
        .into_iter()
        .filter_map(|child_stop| {
            let parent_stop_id = child_stop.parent_station?;
            let Some(group_position) = parent_station_group_lookup.get(&parent_stop_id) else {
                warn!(
                    "Skipping stop {:?}: parent {:?} is not a known station",
                    child_stop.stop_id, parent_stop_id
                );
                return None;
            };
            Some((child_stop.stop_id, group_position.to_owned()))
        })
        .collect()
}

// Given a vector of stops, separate into a tuple of (Parents, Children)
fn split_stops_into_parents_and_children(stops: Vec<Stop>) -> (Vec<Stop>, Vec<Stop>) {
    stops.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut parents, mut children), stop| {
            if stop.parent_station.is_none()
                && matches!(
                    stop.location_type,
                    LocationType::Stop | LocationType::Station
                )
            {
                parents.push(stop)
            } else if matches!(stop.location_type, LocationType::Stop) {
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
        transfer.from_stop_id == stop_1.stop_id
            && transfer.to_stop_id == stop_2.stop_id
            && !matches!(transfer.transfer_type, TransferType::NotPossible)
    })
}

// Build HashMap to Lookup RouteID from TripID
fn build_trips_map(trips: Vec<Trip>) -> HashMap<TripID, RouteID> {
    trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect()
}
