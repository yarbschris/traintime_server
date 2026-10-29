use std::fmt::Display;

#[derive(Debug, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct Stop {
    pub stop_id: StopID,
    pub stop_name: StationName,
    pub parent_station: Option<StopID>,
    pub location_type: LocationType,
}

#[derive(Debug, Clone, serde::Deserialize, Hash, PartialEq, PartialOrd, Eq, Ord)]
pub struct StationName(pub String);
impl StationName {
    pub fn from(name: &str) -> Self {
        StationName(String::from(name))
    }
}

impl Display for StationName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Hash)]
#[serde(try_from = "Option<u8>")]
pub enum LocationType {
    Stop,         // A location where passengers board a transit vehicle.
    Station,      // A physical structure or area that contains one or more platform
    EntranceExit, // A location where passengers enter / exit a station
    GenericNode,  // A location within a station not matching any other, not necessary for us
    BoardingArea, // A specific location on a platform, also unnecessary
}

impl TryFrom<Option<u8>> for LocationType {
    type Error = String;

    fn try_from(value: Option<u8>) -> Result<Self, Self::Error> {
        match value {
            None | Some(0) => Ok(LocationType::Stop),
            Some(1) => Ok(LocationType::Station),
            Some(2) => Ok(LocationType::EntranceExit),
            Some(3) => Ok(LocationType::GenericNode),
            Some(4) => Ok(LocationType::BoardingArea),
            Some(n) => Err(format!("Invalid location_type {n}")),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, Hash, PartialEq, std::cmp::Eq, PartialOrd, Ord)]
pub struct RouteID(pub String);

impl Display for RouteID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, serde::Deserialize, Hash, PartialEq, std::cmp::Eq)]
pub struct TripID(pub String);

#[derive(Debug, serde::Deserialize, Hash, PartialEq, std::cmp::Eq, Clone)]
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
pub struct Transfer {
    pub from_stop_id: StopID,
    pub to_stop_id: StopID,
    pub transfer_type: TransferType,
}

#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "Option<u8>")]
pub enum TransferType {
    Recommended,
    Timed,
    TimedMinimum,
    NotPossible,
    SameVehicle,
    SameVehicleNoInSeat,
}

impl TryFrom<Option<u8>> for TransferType {
    type Error = String;

    fn try_from(value: Option<u8>) -> Result<Self, Self::Error> {
        match value {
            None | Some(0) => Ok(TransferType::Recommended),
            Some(1) => Ok(TransferType::Timed),
            Some(2) => Ok(TransferType::TimedMinimum),
            Some(3) => Ok(TransferType::NotPossible),
            Some(4) => Ok(TransferType::SameVehicle),
            Some(5) => Ok(TransferType::SameVehicleNoInSeat),
            Some(n) => Err(format!("Invalid transfer_type {n}")),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct StopTime {
    pub trip_id: TripID,
    pub stop_id: StopID,
}

// Unused, but planned for future use
#[allow(unused)]
#[derive(Debug, serde::Deserialize)]
struct Route {
    route_id: RouteID,
    route_short_name: Option<String>,
    route_long_name: Option<String>,
    route_type: RouteType,
    route_color: Option<String>,
    route_text_color: Option<String>,
    route_sort_order: Option<String>,
}

#[allow(unused)]
#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "u8")]
enum RouteType {
    LightRailOrStreetLevel,
    Subway,
    Rail,
    Bus,
    Ferry,
    CableTram,
    AerialLift,
    Funicular,
    Trolleybus,
    Monorail,
}

impl TryFrom<u8> for RouteType {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(RouteType::LightRailOrStreetLevel),
            1 => Ok(RouteType::Subway),
            2 => Ok(RouteType::Rail),
            3 => Ok(RouteType::Bus),
            4 => Ok(RouteType::Ferry),
            5 => Ok(RouteType::CableTram),
            6 => Ok(RouteType::AerialLift),
            7 => Ok(RouteType::Funicular),
            11 => Ok(RouteType::Trolleybus),
            12 => Ok(RouteType::Monorail),
            _ => Err(format!("Invalid route_type: {value}")),
        }
    }
}
