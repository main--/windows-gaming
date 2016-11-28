use systemd::daemon::*;
use std::collections::HashMap;

pub fn notify_systemd(ready: bool, status: &'static str) {
    let mut info = HashMap::new();
    info.insert(STATE_READY, if ready { "1" } else { "0" });
    info.insert(STATE_STATUS, status);

    // this returns false if we're not actually running inside systemd
    // we don't care about that though
    notify(false, info).expect("sd_notify failed");
}
