use std::path::Path;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::fmt::{Display, Formatter, Result as FmtResult};

use toml;
use serde_yaml;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Copy)]
pub struct DeviceId {
    pub vendor: u16,
    pub product: u16,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum UsbBinding {
    ById(DeviceId),
    ByPort { bus: u16, port: u16 },
}

// https://en.wikipedia.org/wiki/Host_controller_interface_(USB,_Firewire)#Open_Host_Controller_Interface_2
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UsbBus {
    Ohci,
    Uhci,
    Ehci,
    Xhci,
}

impl Display for UsbBus {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        f.write_str(match self {
            &UsbBus::Ohci => "ohci",
            &UsbBus::Uhci => "uhci",
            &UsbBus::Ehci => "ehci",
            &UsbBus::Xhci => "xhci",
        })
    }
}

impl Default for UsbBus {
    fn default() -> Self {
        UsbBus::Xhci
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct UsbDevice {
    pub binding: UsbBinding,
    #[serde(default = "default_usbdevice_permanent")]
    pub permanent: bool,
    #[serde(default = "default_usbdevice_bus")]
    pub bus: UsbBus,
}

fn default_usbdevice_permanent() -> bool {
    false
}

fn default_usbdevice_bus() -> UsbBus {
    UsbBus::Xhci
}

impl UsbDevice {
    pub fn from_binding(binding: UsbBinding) -> Self {
        UsbDevice {
            binding: binding,
            permanent: default_usbdevice_permanent(),
            bus: default_usbdevice_bus(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct HotKey {
    pub key: String,
    pub action: HotKeyAction,
}
#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum HotKeyAction {
    Exec(String),
    Action(Action),
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
pub enum Action {
    IoEntry,
    IoEntryForced,
    IoExit,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Config {
    pub machine: MachineConfig,
    pub sound: SoundConfig,
    pub samba: Option<SambaConfig>,
    pub setup: Option<SetupConfig>,
}

#[serde(default)]
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SoundConfig {
    pub timer_period: usize,
    pub input: SoundSettings,
    pub output: SoundSettings,
    pub backend: SoundBackend,
}

impl Default for SoundConfig {
    fn default() -> SoundConfig {
        SoundConfig {
            timer_period: 100,
            input: SoundSettings::default(),
            output: SoundSettings::default(),
            backend: SoundBackend::default(),
        }
    }
}

#[serde(default)]
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SoundSettings {
    pub voices: usize,
    pub use_polling: bool,
    pub fixed: Option<SoundFixedSettings>,
}

impl Default for SoundSettings {
    fn default() -> SoundSettings {
        SoundSettings {
            voices: 1,
            use_polling: true,
            fixed: None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SoundFixedSettings {
    pub frequency: usize,
    pub format: String,
    pub channels: usize,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum SoundBackend {
    None,

    Alsa {
        sink: AlsaSettings,
        source: AlsaSettings,
    },

    PulseAudio {
        buffer_samples: usize,
        server: Option<String>,
        sink_name: Option<String>,
        source_name: Option<String>,
    },
}

impl Default for SoundBackend {
    fn default() -> SoundBackend {
        SoundBackend::None
    }
}

#[serde(default)]
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct AlsaSettings {
    pub name: String,

    pub unit: AlsaUnit,
    pub buffer_size: usize,
    pub period_size: usize,
}

impl Default for AlsaSettings {
    fn default() -> AlsaSettings {
        AlsaSettings {
            name: "default".to_owned(),

            unit: AlsaUnit::Frames,
            buffer_size: 0,
            period_size: 0,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub enum AlsaUnit {
    Frames,
    MicroSeconds,
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
    pub usb_devices: Vec<UsbDevice>,
    #[serde(default = "machineconfig_hotkeys_default")]
    pub hotkeys: Vec<HotKey>,
}

fn machineconfig_hotkeys_default() -> Vec<HotKey> {
    vec![HotKey {
         key: "Ctrl+Alt+NoRepeat+Insert".to_string(),
         action: HotKeyAction::Action(Action::IoExit),
    }]
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
        let yaml_path = path.as_ref().with_extension("yml");
        let contents = serde_yaml::to_string(self).unwrap();
        let mut file = OpenOptions::new().create(true).write(true)
            .truncate(true).open(yaml_path).expect("Failed to open config file");
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
