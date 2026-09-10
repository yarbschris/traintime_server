use serde_yaml;

#[derive(Debug, serde::Deserialize)]
pub struct TraintimeSystemConfig {
    pub gtfs_static_endpoint: String,
    pub gtfs_rt_endpoints: Vec<String>,
}

impl TraintimeSystemConfig {
    fn read_from_config_file(path: &str) -> TraintimeSystemConfig {
        let file = std::fs::File::open(path).unwrap();
        let config: TraintimeSystemConfig = serde_yaml::from_reader(file).unwrap();
        config
    }

    pub fn read_config_by_system(system: SupportedTransitSystem) -> TraintimeSystemConfig {
        let path = match system {
            SupportedTransitSystem::NycSubway => "system_configs/nyc_subway.yml",
        };

        TraintimeSystemConfig::read_from_config_file(path)
    }
}

pub enum SupportedTransitSystem {
    NycSubway,
}
