use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::thread;

use async_trait::async_trait;
use log::trace;
use tokio::sync::mpsc::error::TryRecvError;
use windows::runtime::Result as WinResult;
use windows::{Win32::Foundation::*, Win32::System::{LibraryLoader::GetModuleHandleA}, Win32::UI::WindowsAndMessaging::*, runtime::Handle as WinHandle};

use tokio::{runtime::Handle, sync::{mpsc, oneshot}};

pub type WindowThreadCallback = Box<dyn FnOnce(&mut WindowData) + Send>;

pub struct WindowsEventLoop {
    handle: HWND,
    tx_callbacks: Option<mpsc::Sender<WindowThreadCallback>>,
    rx_shutdown: Option<oneshot::Receiver<Result<WinResult<()>, Box<dyn Any + Send>>>>,
}
impl WindowsEventLoop {
    pub async fn init() -> WinResult<Self> {
        let (tx, rx) = oneshot::channel();
        let (tx_callbacks, rx_callbacks) = mpsc::channel(16);
        let handle = Handle::current();

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        trace!("Spawning thread");
        thread::spawn(move || {
            trace!("Thread starting");
            let res = std::panic::catch_unwind(AssertUnwindSafe(move || run(handle, rx_callbacks, tx)));
            match tx_shutdown.send(res) {
                Ok(()) => trace!("they received our thread result"),
                Err(Ok(Ok(()))) => trace!("no receiver it but it's no crash"),
                Err(Ok(Err(_))) => {
                    trace!("no receiver but we failed");
                    // it's not a panic either however, so can't be that serious i guess?
                    // maybe let's not kill the program in this case
                }
                Err(Err(e)) => {
                    trace!("no receiver and we had a crash, rethrowing: {:?}", e);
                    // no choice but to rethrow and bring down the application
                    std::panic::resume_unwind(e);
                }
            }
        });


        let handle = match rx.await {
            Ok(h) => h,
            Err(_) => {
                // the only way this can happen is if they crashed; let's propagate the error or panic
                match rx_shutdown.await.unwrap() {
                    Ok(Ok(())) => unreachable!(),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => std::panic::resume_unwind(e),
                }
            }
        };

        trace!("WindowsEventLoop is ready");
        Ok(WindowsEventLoop { handle, tx_callbacks: Some(tx_callbacks), rx_shutdown: Some(rx_shutdown) })
    }
    pub fn window_handle(&self) -> HWND {
        self.handle
    }
    /// For completeness' sake, note that this call will re-surface any errors or panics that happened on the clipboard thread.
    /// However, the clipboard thread is not expected to panic in general.
    pub async fn shutdown(mut self) -> WinResult<()> {
        trace!("gracefully shutting down");
        let rx_shutdown = self.rx_shutdown.take().unwrap();
        drop(self);
        match rx_shutdown.await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => std::panic::resume_unwind(e),
            Err(_) => unreachable!(),
        }
    }

    pub async fn send_callback(&self, cb: WindowThreadCallback) -> Result<(), ()> {
        self.tx_callbacks.clone().unwrap().send(cb).await.map_err(|_| ())?;
        unsafe { PostMessageA(self.handle, WM_USER, None, None) }.ok().map_err(|_| ())?;
        Ok(())
    }
}
impl Drop for WindowsEventLoop {
    fn drop(&mut self) {
        trace!("detaching to background");
        drop(self.tx_callbacks.take());
        unsafe { PostMessageA(self.handle, WM_USER, None, None) }.ok().unwrap();
    }
}


pub struct RemoveMeError;
#[async_trait(?Send)]
pub trait WindowMessageListener {
    async fn handle(&mut self, window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<Option<LRESULT>, RemoveMeError>;
}
#[async_trait(?Send)]
impl<U: Future<Output=Result<Option<LRESULT>, RemoveMeError>>, T: FnMut(HWND, u32, WPARAM, LPARAM) -> U> WindowMessageListener for T {
    async fn handle(&mut self, window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<Option<LRESULT>, RemoveMeError> {
        (self)(window, message, wparam, lparam).await
    }
}

pub struct WindowData {
    listeners: Vec<Box<dyn WindowMessageListener>>,
    callbacks: mpsc::Receiver<WindowThreadCallback>,
    runtime: Handle,
}
impl WindowData {
    pub fn register_listener(&mut self, listener: Box<dyn WindowMessageListener>) {
        self.listeners.push(listener);
    }
    unsafe fn handle_wm(&mut self, window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            WM_USER => {
                trace!("scheduling user callbacks");
                loop {
                    match self.callbacks.try_recv() {
                        Ok(cb) => (cb)(self),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            DestroyWindow(window).ok().unwrap();
                            break;
                        }
                    }
                }

                LRESULT(0)
            }
            WM_DESTROY => {
                trace!("quitting");
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                let all_handlers = futures_util::future::join_all(self.listeners.iter_mut().enumerate().rev().map(move |(i, x)| async move { (i, x.handle(window, message, wparam, lparam).await) }));
                let all_results = self.runtime.block_on(all_handlers);

                let mut result = None;
                for (i, res) in all_results {
                    match res {
                        Ok(o) => { result = result.or(o); }
                        Err(RemoveMeError) => { self.listeners.remove(i); }
                    }
                }

                result.unwrap_or_else(|| DefWindowProcA(window, message, wparam, lparam))
            }
        }
    }
}

// Problems:
// 1. There seems to be no way to asynchronously wait for the a thread's message queue.
// 2. There is no clear way how to coexist with other windows components on the same thread,
//    since someone must handle thread messages without a target window, and if that someone is us,
//    other code may not receive the messages.
// Hence, we just start our own background thread for this (and cry).
fn run(
    handle: Handle,
    callbacks: mpsc::Receiver<WindowThreadCallback>,
    tx: oneshot::Sender<HWND>) -> WinResult<()> {
    unsafe {
        let instance = GetModuleHandleA(None);
        trace!("hinstance = {:?}", instance);
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
        trace!("class atom = {:?}", atom);
        debug_assert!(atom != 0);
        let hwnd = CreateWindowExA(
            Default::default(),
            wc.lpszClassName,
            "windows-eventloop window", // never shown anywhere
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
        trace!("window created, sending hwnd: {:?}", hwnd);
        tx.send(hwnd).unwrap();

        let mut mywin = WindowData { listeners: Vec::new(), callbacks, runtime: handle };

        SetWindowLongPtrA(hwnd, GWLP_USERDATA, &mut mywin as *mut _ as isize);

        trace!("entering message pump");
        let mut message = MSG::default();
        while GetMessageA(&mut message, HWND(0), 0, 0).into() {
            DispatchMessageA(&message);
        }
        trace!("leaving message pump");

        Ok(())
    }
}

// TODO: would need to catch panics and then resurface them in run()
extern "system" fn wndproc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        let window_ptr = GetWindowLongPtrA(window, GWLP_USERDATA) as *mut WindowData;
        if window_ptr.is_null() {
            // during window creation, wndproc is called (but our WindowData does not exist yet)
            DefWindowProcA(window, message, wparam, lparam)
        } else {
            let window_data = &mut *(window_ptr);
            window_data.handle_wm(window, message, wparam, lparam)
        }
    }
}
