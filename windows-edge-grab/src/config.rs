use std::path::Path;
use std::fs::File;
use std::io::Read;

use serde_yaml;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub monitors: Vec<Monitor>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub bounds: Rectangle,
    //you need to set this, if both monitors use the same x and y coordiantes
    pub vbounds: Rectangle,
    pub is_windows: bool,
    /// whether this monitor is still connected after switching inputs to Windows
    #[serde(default)]
    pub connected: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Rectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl From<(i32, i32)> for Point {
    fn from(t: (i32, i32)) -> Point {
        Point { x: t.0, y: t.1 }
    }
}

impl Rectangle {
    pub fn contains<T: Into<Point>>(&self, p: T) -> bool {
        let p = p.into();
        p.x >= self.x && p.x < self.x + self.width
        && p.y >= self.y && p.y < self.y + self.height
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Option<Config> {
        let path = path.as_ref();
        if !path.exists() {
            return None;
        }

        let mut config = String::new();
        {
            let mut config_file = File::open(path).expect("Failed to open config file");
            config_file.read_to_string(&mut config).expect("Failed to read config file");
        }

        let de = serde_yaml::from_str(&config).expect("Failed to decode config");
        Some(de)
    }
}
