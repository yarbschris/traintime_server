use server::config::{Selection, SupportedTransitSystem};
use server::types::gtfs::StationName;

const HELP: &str = "\
Traintime CLI

USAGE:
  traintime_cli [OPTIONS]

FLAGS:
  -h, --help            Prints help information
  -l, --list-systems    Lists valid systems

OPTIONS:
  --system SYSTEM       Sets transit system
  --station STATION     Sets station
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
    station: String,
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

    let args = Args {
        transit_system: pargs.value_from_str("--system")?,
        station: pargs.value_from_str("--station")?,
    };

    let transit_system = if args.transit_system == "MetroNorth" {
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
    let station_name = StationName::from(&args.station);

    Ok(Selection {
        station_name: Some(station_name),
        transit_system,
    })
}
