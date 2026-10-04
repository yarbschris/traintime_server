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
