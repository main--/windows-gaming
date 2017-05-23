use std::path::Path;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use toml;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Copy)]
pub struct DeviceId {
    pub vendor: u16,
    pub product: u16,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Config {
    pub machine: MachineConfig,
    pub samba: Option<SambaConfig>,
    pub setup: Option<SetupConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
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

#[derive(Deserialize, Serialize, Debug, Clone)]
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
        let contents = toml::to_string(self).unwrap();
        let mut file = OpenOptions::new().create(true).write(true)
            .truncate(true).open(path).expect("Failed to open config file");
        write!(file, "{}", contents).expect("Failed to write config file");
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Config {
        let mut config = String::new();
        {
            let mut config_file = File::open(path).expect("Failed to open config file");
            config_file.read_to_string(&mut config).expect("Failed to read config file");
        }

        toml::from_str(&config).expect("Failed to decode config")
    }
}
