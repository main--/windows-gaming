use std::{collections::HashMap, sync::{Arc, Mutex}};

use windows::{Win32::Foundation::*, Win32::System::DataExchange::*, Win32::System::{LibraryLoader::GetModuleHandleA, SystemServices::CLIPBOARD_FORMATS}, Win32::UI::WindowsAndMessaging::*, runtime::Handle as RtHandle};

use tokio::{runtime::{Handle, Runtime}, sync::{mpsc, oneshot, watch}};

use crate::send::DelayRenderedClipboardData;
use crate::{offer::ClipboardOffer, raw::{WindowsClipboard, WindowsClipboardOwned}, send::{ClipboardContents, ClipboardFormatContent}};

struct WindowData {
    upd: watch::Sender<Option<ClipboardOffer>>,
    delay_renderers: Arc<Mutex<HashMap<u32, DelayRenderedClipboardData>>>,
    runtime: Runtime,
}

// Problems:
// 1. There seems to be no way to asynchronously wait for the a thread's message queue.
// 2. There is no clear way how to coexist with other windows components on the same thread,
//    since someone must handle thread messages without a target window, and if that someone is us,
//    other code may not receive the messages.
// Hence, we just start our own background thread for this (and cry).
pub fn run(
    handle: Handle,
    tx_offers: watch::Sender<Option<ClipboardOffer>>,
    mut rx_contents: mpsc::Receiver<ClipboardContents>,
    tx: oneshot::Sender<HWND>) -> windows::runtime::Result<()> {
    unsafe {
        let instance = GetModuleHandleA(None);
        debug_assert!(instance.0 != 0);
        let wc = WNDCLASSA {
            hCursor: LoadCursorW(None, IDC_ARROW),
            hInstance: instance,
            lpszClassName: PSTR(b"window\0".as_ptr() as _),

            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            ..Default::default()
        };
        let atom = RegisterClassA(&wc);
        debug_assert!(atom != 0);
        let hwnd = CreateWindowExA(
            Default::default(),
            wc.lpszClassName, //None, //window_class,
            "clipboard window", // never shown anywhere
            Default::default(),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            std::ptr::null_mut(),
        ).ok()?;

        let mut mywin = WindowData { upd: tx_offers, delay_renderers: Default::default(), runtime: tokio::runtime::Builder::new_current_thread().build().unwrap() };
        let delay_renderers = mywin.delay_renderers.clone();
        let clipboard_updater = handle.spawn(async move {
            while let Some(x) = rx_contents.recv().await {
                let mut clipboard = WindowsClipboard::open(hwnd);
                let mut clipboard = clipboard.clear()?;
                let mut delay_renderers = delay_renderers.lock().unwrap();
                delay_renderers.clear();
                for content in x.0 {
                    match content {
                        ClipboardFormatContent::DelayRendered(renderer) => {
                            let format = renderer.format();
                            delay_renderers.insert(format.0, renderer);
                            clipboard.send_delay_rendered(format)?;
                        }
                        ClipboardFormatContent::Immediate(val) => {
                            val.render(&mut clipboard)?;
                        }
                    }
                }
            }
            Ok::<(), windows::runtime::Error>(())
        });

        SetWindowLongPtrA(hwnd, GWLP_USERDATA, &mut mywin as *mut _ as isize);
        tx.send(hwnd).unwrap();
        assert!(AddClipboardFormatListener(hwnd).as_bool());
        let mut message = MSG::default();
        while GetMessageA(&mut message, HWND(0), 0, 0).into() {
            DispatchMessageA(&message);
        }

        // wait for updater task to complete
        // Note: for this to be correct, we must be able to ensure that the window loop
        // can only be terminated once all senders for rx_contents have been dropped
        mywin.runtime.block_on(clipboard_updater).unwrap()?;

        Ok(())
    }
}


extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let window_data = || &mut *(GetWindowLongPtrA(window, GWLP_USERDATA) as *mut WindowData);

        match message {
            WM_USER => {
                DestroyWindow(window);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_CLIPBOARDUPDATE => {
                let offer = crate::read_clipboard(window).unwrap_or(None);
                let _ = window_data().upd.send(offer);
                // we don't care if there's nobody to receive
                LRESULT(0)
            }
            WM_RENDERFORMAT => {
                let data = window_data();
                let mut delay_renderers = data.delay_renderers.lock().unwrap();
                let fmt = CLIPBOARD_FORMATS(wparam.0 as u32);
                if let Some(dr) = delay_renderers.remove(&fmt.0) {
                    if let Some(cfd) = data.runtime.block_on(dr.delay_render()) {
                        let _ = cfd.render(&mut WindowsClipboardOwned::assert());
                        // if windows refuses to render this there is nothing we can do
                    }
                    // else: if our delay renderer us not responding there is nothing we can do
                }
                // else: if our delay renderer is already gone there is - you guessed it - nothing we can do

                // TODO: maybe add debug logging for all of these cases

                LRESULT(0)
            }
            WM_RENDERALLFORMATS => {
                let data = window_data();
                let mut delay_renderers = data.delay_renderers.lock().unwrap();

                let clipboard_open = WindowsClipboard::open(window);
                let mut clipboard = WindowsClipboardOwned::assert(); // must not clear existing data here
                if GetClipboardOwner() == window { // someone else could have the clipboard by now
                    let joined = futures_util::future::join_all(delay_renderers.drain().map(|(_, renderer)| async move {
                        renderer.delay_render().await
                    }));
                    let joined = data.runtime.block_on(joined);

                    for cfd in joined.into_iter().flatten() {
                        let _ = cfd.render(&mut clipboard);
                        // if windows refuses to render this there is nothing we can do
                    }
                }
                drop(clipboard_open);

                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}
