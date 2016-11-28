use std::path::Path;
use std::fs::File;
use std::io::Read;

#[derive(RustcDecodable, Debug)]
pub struct Config {
    pub datadir: Option<String>,
    pub workdir: Option<String>,
    pub machine: MachineConfig,
    pub samba: Option<SambaConfig>,
}

#[derive(RustcDecodable, Debug)]
pub struct MachineConfig {
    pub memory: String,
    pub hugepages: Option<bool>,

    pub cores: u32,
    pub threads: Option<u32>,

    pub network: Option<NetworkConfig>,
    pub storage: Vec<StorageDevice>,
}

#[derive(RustcDecodable, Debug)]
pub struct StorageDevice {
    pub path: String,
    pub cache: String,
    pub format: String,
}

#[derive(RustcDecodable, Debug)]
pub struct NetworkConfig {
    pub bridges: Vec<String>, // TODO: custom usernet
}

#[derive(RustcDecodable, Debug)]
pub struct SambaConfig {
    pub user: String,
    pub path: String,
}



impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Config {
        use ::toml::{Parser, Decoder, Value};
        use ::rustc_serialize::Decodable;

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
