extern crate sd_notify as api;
use self::api::NotifyState;

/// Attempts to notify systemd about our status.
/// Doesn't do anything unless we're running as a systemd service.
pub fn notify_systemd(ready: bool, status: &'static str) {
    trace!("Notifying systemd (ready={} status='{}')", ready, status);
    if let Err(e) = api::notify(false, &[NotifyState::Ready, NotifyState::Status(status.to_owned())]) {
        debug!("sd_notify error: {:?}", e);
    }
}
