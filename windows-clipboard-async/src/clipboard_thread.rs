use windows::{
    Win32::Foundation::*,
    Win32::System::LibraryLoader::GetModuleHandleA, Win32::UI::WindowsAndMessaging::*,
    Win32::System::DataExchange::*,
};

use tokio::sync::{oneshot, watch};

use crate::{offer::ClipboardOffer, raw};

struct WindowData {
    upd: watch::Sender<Option<ClipboardOffer>>,
}

// Problems:
// 1. There seems to be no way to asynchronously wait for the a thread's message queue.
// 2. There is no clear way how to coexist with other windows components on the same thread,
//    since someone must handle thread messages without a target window, and if that someone is us,
//    other code may not receive the messages.
// Hence, we just start our own background thread for this (and cry).
pub fn run(tx_offers: watch::Sender<Option<ClipboardOffer>>, tx: oneshot::Sender<HWND>) {
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
        );
        println!("{:?}", GetLastError());
        assert!(hwnd.0 != 0);
        let mut mywin = WindowData { upd: tx_offers };
        SetWindowLongPtrA(hwnd, GWLP_USERDATA, &mut mywin as *mut _ as isize);
        tx.send(hwnd).unwrap();
        assert!(AddClipboardFormatListener(hwnd).as_bool());
        let mut message = MSG::default();
        while GetMessageA(&mut message, HWND(0), 0, 0).into() {
            DispatchMessageA(&message);
        }
    }
}

extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match message {
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_CLIPBOARDUPDATE => {
                let mywin = &mut *(GetWindowLongPtrA(window, GWLP_USERDATA) as *mut WindowData);

                let offer = crate::read_clipboard(window).unwrap_or(None);
                let _ = mywin.upd.send(offer);
                // we don't care if there's nobody to receive
                LRESULT(0)
            }
            _ => DefWindowProcA(window, message, wparam, lparam),
        }
    }
}
