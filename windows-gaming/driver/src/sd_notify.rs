use libc::{dlopen, dlsym, c_char, RTLD_LAZY};
use std::ffi::CString;
use std::mem::transmute;

type SdNotify = Option<extern "C" fn(i32, *const c_char) -> i32>;

lazy_static! {
    static ref SD_NOTIFY: SdNotify = {
        unsafe {
            let lib = CString::new("libsystemd.so").unwrap();
            let dll = dlopen(lib.as_ptr(), RTLD_LAZY);
            if dll.is_null() {
                None
            } else {
                let symbol = CString::new("sd_notify").unwrap();
                transmute(dlsym(dll, symbol.as_ptr()))
            }
        }
    };
}

/// Attempts to notify systemd about our status.
/// Doesn't do anything unless we're running as a systemd service.
pub fn notify_systemd(ready: bool, status: &'static str) {
    trace!("Notifying systemd (ready={} status='{}')", ready, status);
    if let Some(sd_notify) = *SD_NOTIFY {
        let state = CString::new(format!("READY={}\nSTATUS={}", if ready { "1" } else { "0" }, status)).unwrap();
        let ret = sd_notify(0, state.as_ptr());
        debug!("systemd returned {}", ret);
    } else {
        debug!("No libsystemd found.");
    }
}
