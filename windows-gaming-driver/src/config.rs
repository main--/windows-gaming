use std::path::Path;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use toml;
use serde_yaml;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Copy)]
pub struct DeviceId {
    pub vendor: u16,
    pub product: u16,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Config {
    pub machine: MachineConfig,
    pub samba: Option<SambaConfig>,
    pub setup: Option<SetupConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct SetupConfig {
    // VM options
    pub cdrom: Option<String>,
    pub floppy: Option<String>,
    pub gui: bool,

    // setup state
    pub iommu_commanded: bool,
    // convention: gpu must be first for iommu group checks
    pub vfio_devs: Vec<DeviceId>,
    pub reboot_commanded: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct MachineConfig {
    pub memory: String,
    pub hugepages: Option<bool>,

    pub cores: usize,
    pub threads: Option<u32>,

    // convention: gpu is first
    pub vfio_slots: Vec<String>,
    pub network: Option<NetworkConfig>,
    pub storage: Vec<StorageDevice>,
    // convention: first element is the mouse (for mouse only setup)
    pub usb_devices: Vec<DeviceId>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct StorageDevice {
    pub path: String,
    pub cache: String,
    pub format: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct NetworkConfig {
    pub bridges: Vec<String>, // TODO: custom usernet
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SambaConfig {
    pub user: String,
    pub path: String,
}

impl From<(u16, u16)> for DeviceId {
    fn from(old: (u16, u16)) -> DeviceId {
        DeviceId {
            vendor: old.0,
            product: old.1,
        }
    }
}

impl From<DeviceId> for (u16, u16) {
    fn from(old: DeviceId) -> (u16, u16) {
        (old.vendor, old.product)
    }
}

impl Config {
    pub fn save<P: AsRef<Path>>(&self, path: P) {
        let contents = serde_yaml::to_string(self).unwrap();
        let mut file = OpenOptions::new().create(true).write(true)
            .truncate(true).open(path).expect("Failed to open config file");
        writeln!(file, "{}", contents).expect("Failed to write config file");
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Option<Config> {
        let yaml_path = path.as_ref().with_extension("yml");
        let file_path;
        let needs_upgrade = !yaml_path.exists();
        if needs_upgrade {
            file_path = yaml_path.with_extension("toml");
        } else {
            file_path = yaml_path.clone();
        }

        if !file_path.exists() {
            return None;
        }

        let mut config = String::new();
        {
            let mut config_file = File::open(file_path).expect("Failed to open config file");
            config_file.read_to_string(&mut config).expect("Failed to read config file");
        }

        Some(if needs_upgrade {
            // old-style toml config - upgrade it
            let cfg: Config = toml::from_str(&config).expect("Failed to decode old-style TOML config");
            cfg.save(yaml_path);
            cfg
        } else {
            serde_yaml::from_str(&config).expect("Failed to decode config")
        })
    }
}
