use crate::types::gtfs::{RouteID, StationName};

#[derive(Debug)]
pub enum StartError {
    Client(reqwest::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Client(e) => write!(f, "error initializing http client: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<reqwest::Error> for StartError {
    fn from(e: reqwest::Error) -> Self {
        StartError::Client(e)
    }
}

#[derive(Debug)]
pub enum SystemConfigError {
    Parse(serde_norway::Error),
    NoRealtimeEndpoints,
    NoRoutesList(String),
}

impl std::fmt::Display for SystemConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{e}"),
            Self::NoRealtimeEndpoints => write!(f, "no gtfs_rt_endpoints provided"),
            Self::NoRoutesList(url) => write!(
                f,
                "no routes list found for {url}. field is required when multiple endpoints are provided"
            ),
        }
    }
}

impl std::error::Error for SystemConfigError {}

impl From<serde_norway::Error> for SystemConfigError {
    fn from(e: serde_norway::Error) -> Self {
        Self::Parse(e)
    }
}

#[derive(Debug)]
pub enum StationResolveError {
    UnknownStation(StationName),
    EndpointsDoNotCoverAnyRoutes(Vec<RouteID>),
}

impl std::fmt::Display for StationResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownStation(station_name) => {
                write!(f, "cannot resolve station name {}", station_name)
            }
            Self::EndpointsDoNotCoverAnyRoutes(routes) => {
                write!(
                    f,
                    "no detected station routes found in system yaml config. detected routes: {routes:?}"
                )
            }
        }
    }
}

#[derive(Debug)]
pub enum GtfsRtError {
    Fetch(reqwest::Error),
    Decode(prost::DecodeError),
}

impl std::fmt::Display for GtfsRtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch(e) => write!(f, "error fetching GTFS-RF: {e}"),
            Self::Decode(e) => write!(f, "error decoding GTFS-RF: {e}"),
        }
    }
}

impl std::error::Error for GtfsRtError {}

impl From<reqwest::Error> for GtfsRtError {
    fn from(e: reqwest::Error) -> Self {
        Self::Fetch(e)
    }
}

impl From<prost::DecodeError> for GtfsRtError {
    fn from(e: prost::DecodeError) -> Self {
        Self::Decode(e)
    }
}

#[derive(Debug)]
pub enum GtfsStaticError {
    Zip(zip::result::ZipError),
    Csv(csv::Error),
    Fetch(reqwest::Error),
}

impl std::fmt::Display for GtfsStaticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zip(e) => write!(f, "error interacting with gtfs-static zip file: {e}"),
            Self::Csv(e) => write!(
                f,
                "error interacting with gtfs-static csv representation: {e}"
            ),
            Self::Fetch(e) => write!(f, "error fetching gtfs-static: {e}"),
        }
    }
}
impl std::error::Error for GtfsStaticError {}

impl From<zip::result::ZipError> for GtfsStaticError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::Zip(e)
    }
}

impl From<csv::Error> for GtfsStaticError {
    fn from(e: csv::Error) -> Self {
        Self::Csv(e)
    }
}

impl From<reqwest::Error> for GtfsStaticError {
    fn from(e: reqwest::Error) -> Self {
        Self::Fetch(e)
    }
}
