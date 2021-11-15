use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::prelude::OsStrExt;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicI32, Ordering};

use log::{debug, warn};
use tokio::sync::mpsc;
use windows::Win32::Foundation::{LRESULT, PWSTR, WPARAM};
use windows::Win32::System::DataExchange::{GlobalAddAtomW, GlobalDeleteAtom};
use windows::Win32::UI::Input::KeyboardAndMouse::{HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::WM_HOTKEY;
use windows_eventloop::WindowsEventLoop;


pub struct HotKeyManager<T> {
    wel: T,
    queues: Arc<Mutex<HashMap<u16, mpsc::UnboundedSender<()>>>>,
}

// used to assign unique hotkey ids
static HK_COUNTER: AtomicI32 = AtomicI32::new(0);

impl<T: AsRef<WindowsEventLoop>> HotKeyManager<T> {
    pub async fn new(wel: T) -> Self {
        let queues: Arc<Mutex<HashMap<u16, mpsc::UnboundedSender<()>>>> = Default::default();
        let queues2 = queues.clone();
        wel.as_ref().send_callback(Box::new(|wd| {
            wd.register_listener(Box::new(move |hwnd, msg, hotkey_id: WPARAM, _| {
                let queues = queues2.clone();
                async move {
                    match msg {
                        WM_HOTKEY => {
                            let hotkey_id = hotkey_id.0 as u16;
                            let mut queues = queues.lock().unwrap();

                            if queues.get_mut(&hotkey_id).and_then(|queue| queue.send(()).ok()).is_none() {
                                // if sending failed, remove this binding
                                unsafe {
                                    debug!("Unregistering hotkey #{}", hotkey_id);
                                    if UnregisterHotKey(hwnd, hotkey_id.into()).as_bool() {
                                        GlobalDeleteAtom(hotkey_id);
                                    }
                                }
                            }

                            Ok(Some(LRESULT(0)))
                        }
                        _ => Ok(None),
                    }
                }
            }));
        })).await.unwrap();
        HotKeyManager { wel, queues }
    }

    /// Register a hotkey.
    ///
    /// Note: There is a footgun where if you register the same modifier+vk combination multiple times
    /// the docs state that only the first call should succeed. Subsequent registrations will fail silently
    /// under the current implementation. Turn on logging to debug this.
    pub async fn register_hotkey(&self, modifiers: HOT_KEY_MODIFIERS, vk: u32) -> mpsc::UnboundedReceiver<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        let hwnd = self.wel.as_ref().window_handle();
        let sname = OsString::from(format!("rust-windows-keybinds#{}", HK_COUNTER.fetch_add(1, Ordering::Relaxed)));
        let mut name: Vec<_> = sname.encode_wide().chain([0]).collect();
        let id = unsafe { GlobalAddAtomW(PWSTR(name.as_mut_ptr())) };

        debug!("Registering hotkey for mod={:?} vk={:?} as {:?}={}", modifiers.0, vk, sname, id);

        assert!(self.queues.lock().unwrap().insert(id, tx).is_none());

        self.wel.as_ref().send_callback(Box::new(move |_wd| {
            if let Err(e) = unsafe { RegisterHotKey(hwnd, id.into(), modifiers, vk) }.ok() {
                warn!("Error registering hotkey #{}: {:?}", id, e);
            }
            // TODO: if send_callback had a oneshot to funnel return values back to the
            // caller (i.e. us), we could propagate the error back to our caller
        })).await.unwrap();

        rx
    }

    /// Destroy this `HotKeyManager`. Mostly useful for shutting down the `WindowsEventLoop`.
    pub fn into_inner(self) -> T {
        self.wel
    }
}
