use server::config::{Selection, SupportedTransitSystem};
use server::types::gtfs::StationName;

const HELP: &str = "\
Traintime CLI

USAGE:
  traintime_cli [OPTIONS]

FLAGS:
  -h, --help                    Prints help information
  -l, --list-systems            Lists valid systems

OPTIONS:
  -t, --system TRANSIT_SYSTEM           Sets transit system
  -s, --station STATION                 Sets station
  -L, --list-stations TRANSIT_SYSTEM    Lists valid stations within system and exits
";

const SYSTEMS: &str = "\
Valid Systems:
    MtaMetroNorth,
    NycSubway,
    BostonTransit
";

#[derive(Debug)]
struct Args {
    /// Selected Transit System
    transit_system: String,
    station: Option<String>,
}

pub fn get_selection() -> Result<Selection, pico_args::Error> {
    let mut pargs = pico_args::Arguments::from_env();
    if pargs.contains(["-h", "--help"]) {
        print!("{}", HELP);
        std::process::exit(0);
    }
    if pargs.contains(["-l", "--list-systems"]) {
        print!("{}", SYSTEMS);
        std::process::exit(0);
    }

    // In reality, all this flag does is make the parser ignore the
    // system/station args and instead parse the system given to
    // --list-stations, returning None for the station_name field.
    // For now, main will interpret None in station_name as list_stations.
    // For this basic of a CLI it is okay because we don't have the ability
    // to send another station after starting the program.
    let list_stations: Option<String> = pargs.opt_value_from_str(["-L", "--list-stations"])?;
    let args = match list_stations {
        Some(transit_system) => Args {
            transit_system,
            station: None,
        },
        None => Args {
            transit_system: pargs.value_from_str(["-t", "--system"])?,
            station: Some(pargs.value_from_str(["-s", "--station"])?),
        },
    };

    let transit_system = if args.transit_system == "MtaMetroNorth" {
        SupportedTransitSystem::MtaMetroNorth
    } else if args.transit_system == "NycSubway" {
        SupportedTransitSystem::NycSubway
    } else if args.transit_system == "BostonTransit" {
        SupportedTransitSystem::BostonTransit
    } else {
        let cause = format!(
            "{} is not a valid system.\n{}",
            args.transit_system, SYSTEMS,
        );
        return Err(pico_args::Error::ArgumentParsingFailed { cause });
    };

    Ok(Selection {
        station_name: args.station.map(|station| StationName::from(&station)),
        transit_system,
    })
}
