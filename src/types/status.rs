use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Starting,
    Ready,
    NoSelection,
    NoStationSelected,
    LoadingStaticData,
    Resolving,
    UnknownStation(String),
    NoEndpointsForStation,
    FetchFailed,
}

impl std::fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starting => write!(f, "Starting"),
            Self::Ready => write!(f, "Ready"),
            Self::NoSelection => write!(f, "No System Selected"),
            Self::NoStationSelected => write!(f, "No Station Selected"),
            Self::LoadingStaticData => write!(f, "Loading Static Data"),
            Self::Resolving => write!(f, "Resolving Selections"),
            Self::UnknownStation(station_identifier) => {
                write!(f, "Unknown Station ({})", station_identifier)
            }
            Self::NoEndpointsForStation => write!(f, "No Endpoints for Station"),
            Self::FetchFailed => write!(f, "Fetch Failed"),
        }
    }
}

pub fn update_server_status(
    tx_server_status: &watch::Sender<ServerStatus>,
    new_status: ServerStatus,
) {
    tx_server_status.send_if_modified(|old_status| {
        if *old_status == new_status {
            false
        } else {
            *old_status = new_status;
            true
        }
    });
}
