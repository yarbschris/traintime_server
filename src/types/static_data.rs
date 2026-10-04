use crate::gtfs;
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

    pub fn build_from_bytes(bytes: Bytes) -> Self {
        let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt").unwrap());

        info!("Deserializing Stop Data...");
        let stops: Vec<gtfs::Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);
        let stop_lookup = build_stop_lookup(stops);

        info!("Deserializing Trip Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
        let trips: Vec<gtfs::Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
        drop(rdr);
        let trip_lookup = build_trips_map(trips);

        info!("Deserializing StopTime Data...");
        let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stop_times.txt").unwrap());
        let mut raw_record = csv::StringRecord::new();
        let headers = rdr
            .headers()
            .expect("Headers Missing, Cannot Deserialize")
            .clone();

        let mut route_lookup: HashMap<gtfs::StationName, Vec<gtfs::RouteID>> = HashMap::new();

        while rdr
            .read_record(&mut raw_record)
            .expect("Error reading raw stop time record")
        {
            let record: gtfs::StopTime = raw_record
                .deserialize(Some(&headers))
                .expect("Could not deserialize stop time record into StopTime");
            let station_name = stop_lookup
                .get(&record.stop_id)
                .expect("Unabled to get station_name")
                .clone();
            let route_id = trip_lookup
                .get(&record.trip_id)
                .expect("Unable to get route_id");

            route_lookup
                .entry(station_name)
                .and_modify(|routes| {
                    if !routes.contains(route_id) {
                        routes.push(route_id.clone())
                    }
                })
                .or_insert(vec![route_id.clone()]);
        }

        info!("Building static data...");
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

// Build HashMap to Lookup Station Name using Stop ID as key
fn build_stop_lookup(stops: Vec<gtfs::Stop>) -> HashMap<gtfs::StopID, gtfs::StationName> {
    info!("Building Stop Lookup");
    stops
        .into_iter()
        .filter(|stop| !stop.parent_station.is_empty())
        .map(|stop| (stop.stop_id, stop.stop_name))
        .collect()
}

// Build HashMap to Lookup RouteID from TripID
fn build_trips_map(trips: Vec<gtfs::Trip>) -> HashMap<gtfs::TripID, gtfs::RouteID> {
    trips
        .into_iter()
        .map(|trip| (trip.trip_id, trip.route_id))
        .collect()
}
