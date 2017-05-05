use std::path::Path;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use toml::{self, Parser, Decoder, Value};
use rustc_serialize::Decodable;

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct Config {
    pub machine: MachineConfig,
    pub samba: Option<SambaConfig>,
    pub setup: Option<SetupConfig>,
}

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct SetupConfig {
    // VM options
    pub cdrom: Option<String>,
    pub floppy: Option<String>,
    pub gui: bool,

    // setup state
    pub iommu_commanded: bool,
    pub vfio_devs: Vec<(u16, u16)>,
    pub reboot_commanded: bool,
}

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct MachineConfig {
    pub memory: String,
    pub hugepages: Option<bool>,

    pub cores: usize,
    pub threads: Option<u32>,

    pub network: Option<NetworkConfig>,
    pub storage: Vec<StorageDevice>,
    pub usb_devices: Vec<(u16, u16)>, // convention: first element is the mouse (for mouse only setup)
}

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct StorageDevice {
    pub path: String,
    pub cache: String,
    pub format: String,
}

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct NetworkConfig {
    pub bridges: Vec<String>, // TODO: custom usernet
}

#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct SambaConfig {
    pub user: String,
    pub path: String,
}



impl Config {
    pub fn save<P: AsRef<Path>>(&self, path: P) {
        let contents = toml::encode_str(self);
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

        let mut parser = Parser::new(&config);

        let parsed = match parser.parse() {
            Some(x) => x,
            None => {
                for e in parser.errors {
                    println!("{}", e);
                }
                panic!("Failed to parse config");
            }
        };

        match Decodable::decode(&mut Decoder::new(Value::Table(parsed))) {
            Ok(x) => x,
            Err(e) => panic!("Failed to decode config: {}", e),
        }
    }
}
