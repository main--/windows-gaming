use log::SetLoggerError;
use env_logger::{LogBuilder};
use time;
use std::env;

pub fn init() -> Result<(), SetLoggerError> {
    let mut builder = LogBuilder::new();
    builder.format(|record| {
        let now = time::now();
        let time = time::strftime("%Y-%m-%d %H:%M:%S", &now).unwrap();
        format!("[{},{:03}] {}: {}", time, now.tm_nsec / 1_000_000, record.level(), record.args())
    });
    if env::var("RUST_LOG").is_ok() {
        builder.parse(&env::var("RUST_LOG").unwrap());
    }
    builder.init()
}
