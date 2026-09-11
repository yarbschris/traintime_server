use std::collections::HashMap;
use std::rc::Rc;

use csv::Reader;

static MTA_GTFS_STATIC_SUPPLEMENTED_DOWNLOAD_ENDPOINT: &str =
    "https://rrgtfsfeeds.s3.amazonaws.com/gtfs_supplemented.zip";

static MTA_1_TO_7_ENDPOINT: &str =
    "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/nyct%2Fgtfs";
static MTA_BDFM_ENDPOINT: &str =
    "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/nyct%2Fgtfs-bdfm";
static HUDSON_YARDS_STOP_NAME: &str = "34 St-Hudson Yards";
static TIMES_SQUARE_STOP_NAME: &str = "Times Sq-42 St";
static EAST_BROADWAY_STOP_NAME: &str = "East Broadway";

pub static TEST_ENDPOINT: &str = MTA_BDFM_ENDPOINT;
pub static TEST_STATIC_ENDPOINT: &str = MTA_GTFS_STATIC_SUPPLEMENTED_DOWNLOAD_ENDPOINT;
pub static TEST_STOP_NAME: &str = EAST_BROADWAY_STOP_NAME;

/// Make a request to the endpoint which provides gtfs static data, download the zip file
/// data/nyc/subway/
pub async fn fetch_gtfs_static_data() -> Result<prost::bytes::Bytes, reqwest::Error> {
    let response = reqwest::get(TEST_STATIC_ENDPOINT).await?;
    response.bytes().await
}

pub struct StaticData {
    pub relevant_stop_ids: Vec<String>,
    pub header_lookup: HashMap<String, Rc<String>>,
}

pub fn parse_and_filter_gtfs_static_data(
    bytes: prost::bytes::Bytes,
    chosen_stop: &str,
) -> StaticData {
    let mut zip_reader = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("stops.txt").unwrap());
    let stops: Vec<Stop> = rdr.deserialize().collect::<Result<_, _>>().unwrap();
    let relevant_stop_ids = get_child_stop_ids_by_station_name(&stops, chosen_stop);

    drop(rdr);

    let mut rdr = csv::Reader::from_reader(zip_reader.by_name("trips.txt").unwrap());
    let trips: Vec<Trip> = rdr.deserialize().collect::<Result<_, _>>().unwrap();

    let trip_headsigns: Vec<Rc<String>> = trips.iter().fold(Vec::new(), |mut acc, trip| {
        if !acc
            .iter()
            .filter(|x| x.as_str() == trip.trip_headsign)
            .collect::<Vec<&Rc<String>>>()
            .is_empty()
        {
            acc
        } else {
            acc.push(Rc::new(trip.trip_headsign.to_string()));
            acc
        }
    });
    for x in &trip_headsigns {
        println!("{}", x.as_str())
    }

    // TODO: Right now, some trips in a direction with only one endpoint do not specify the last
    // three chars of trip_id, so a direct match doesn't always work (WTF WHY)
    let header_lookup: HashMap<String, Rc<String>> =
        trips.iter().fold(HashMap::new(), |mut acc, trip| {
            let headsign_reference: &Rc<String> = trip_headsigns
                .iter()
                .find(|x| x.as_str() == &trip.trip_headsign)
                .unwrap();
            acc.insert(
                trip.trip_id.split_once('_').unwrap().1.to_string(),
                Rc::clone(headsign_reference),
            );
            acc
        });

    StaticData {
        relevant_stop_ids,
        header_lookup,
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Stop {
    stop_id: String,
    pub stop_name: String,
    parent_station: String,
}

pub fn get_unique_station_names(stops: &[Stop]) -> Vec<&String> {
    stops.iter().fold(Vec::new(), |mut acc, stop| {
        if acc.contains(&&stop.stop_name) {
            acc
        } else {
            acc.push(&stop.stop_name);
            acc
        }
    })
}

// Given a station name, get all stop ids where parent field is not none (a child station)
// TODO: This works for NYC Subway specifically because child subway stop ids
// indicate the direction in which the train is moving. I have no clue whether relevent
// stations in other systems follow the same pattern
pub fn get_child_stop_ids_by_station_name(stops: &[Stop], stop_name: &str) -> Vec<String> {
    stops.iter().fold(Vec::new(), |mut acc, stop| {
        if stop.stop_name == stop_name && !stop.parent_station.is_empty() {
            acc.push(stop.stop_id.clone());
            acc
        } else {
            acc
        }
    })
}

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

#[derive(Debug, serde::Deserialize)]
pub struct Trip {
    trip_id: String,
    trip_headsign: String,
}
