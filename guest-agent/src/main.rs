#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsStr;
use std::os::windows::prelude::OsStrExt;
use std::{mem, ptr};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::{FramedRead, FramedWrite};
use windows::Win32::Foundation::{BOOLEAN, HANDLE, LUID, PWSTR};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{EWX_POWEROFF, EXIT_WINDOWS_FLAGS, ExitWindowsEx, SHTDN_REASON_FLAG_PLANNED};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{EWX_FORCEIFHUNG, GetMessageExtraInfo};
use windows::runtime::Handle;
use windows_eventloop::WindowsEventLoop;
use windows_keybinds::HotKeyManager;
use zerocost_clipboard::{ClipboardContents, ClipboardFormatContent, DelayRenderedClipboardData, WindowsClipboard};

mod clientpipe_codec;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    env_logger::init();

    log::info!("requesting shutdown privileges ...");
    request_shutdown_privileges().unwrap();
    log::info!("success");

    let (rx, tx) = TcpStream::connect(("10.0.2.1", 31337)).await?.into_split();
    let incoming = FramedRead::new(rx, clientpipe_codec::Codec);
    let outgoing = FramedWrite::new(tx,clientpipe_codec::Codec);

    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(ReceiverStream::new(rx).inspect(|v| log::trace!("Sending {:?}", v)).map(Ok).forward(outgoing));

    let wel = WindowsEventLoop::init().await?;
    let wel_hwnd = wel.window_handle();

    let clipboard = Arc::new(WindowsClipboard::init(&wel).await?);

    let mut clip_watch = clipboard.watch();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        while let Ok(()) = clip_watch.changed().await {
            let grab = match clip_watch.borrow_and_update().as_ref() {
                Some(offer) if offer.owner() != wel_hwnd => true,
                _ => false,
            };

            if grab {
                tx2.send(clientpipe_codec::ClipboardMessage::GrabClipboard(()).into()).await.unwrap();
            }
        }
    });

    tx.send(clientpipe_codec::GaCmdIn::ReportBoot(())).await.unwrap();


    let outstanding_clipboard_request = Arc::new(Mutex::<Option<oneshot::Sender<String>>>::new(None));
    let hkm = Arc::new(HotKeyManager::new(Box::new(wel)).await);
    let a = incoming.for_each(move |msg| {
        let tx = tx.clone();
        let hkm = hkm.clone();
        let clipboard = clipboard.clone();
        let outstanding_clipboard_request = outstanding_clipboard_request.clone();
        async move {
            match msg.unwrap() {
                clientpipe_codec::GaCmdOut::Ping(()) => tx.send(clientpipe_codec::GaCmdIn::Pong(())).await.unwrap(),
                clientpipe_codec::GaCmdOut::RegisterHotKey(x) => {
                    log::info!("registering hotkey {:?}", x);
                    let mut hk_pipe = hkm.register_hotkey(From::from(x.modifiers), x.key).await;
                    tokio::spawn(async move {
                        while let Some(()) = hk_pipe.recv().await {
                            tx.send(clientpipe_codec::GaCmdIn::HotKey(x.id)).await.unwrap();
                        }
                    });
                },
                clientpipe_codec::GaCmdOut::ReleaseModifiers(()) => {
                    release_modifiers();
                }
                clientpipe_codec::GaCmdOut::Suspend(()) => {
                    let res = unsafe { SetSuspendState(BOOLEAN(0), BOOLEAN(0), BOOLEAN(0)) }.ok();
                    if let Err(e) = res {
                        log::error!("Failed to suspend: {:?}", e);
                    }
                }
                clientpipe_codec::GaCmdOut::Shutdown(()) => {
                    let res = unsafe { ExitWindowsEx(EWX_POWEROFF | EXIT_WINDOWS_FLAGS(EWX_FORCEIFHUNG), SHTDN_REASON_FLAG_PLANNED.0) }.ok();
                    if let Err(e) = res {
                        log::error!("Failed to shut down: {:?}", e);
                    }
                }
                clientpipe_codec::GaCmdOut::Clipboard(c) => {
                    log::trace!("handling clipboard {:?}", c);
                    match c.message {
                        Some(clientpipe_codec::ClipboardMessage::GrabClipboard(())) => {
                            let (ttx, rx) = oneshot::channel();
                            clipboard.send(ClipboardContents(vec![ClipboardFormatContent::DelayRendered(DelayRenderedClipboardData::Text(ttx))])).unwrap();
                            tokio::spawn(async move {
                                if let Ok(s) = rx.await {
                                    *outstanding_clipboard_request.lock().unwrap() = Some(s);
                                    tx.send(clientpipe_codec::ClipboardMessage::RequestClipboardContents(clientpipe_codec::ClipboardType::Text.into()).into()).await.unwrap();
                                }
                            });
                        }
                        Some(clientpipe_codec::ClipboardMessage::RequestClipboardContents(types)) => {
                            if types == clientpipe_codec::ClipboardType::None.into() {
                                tx.send(clientpipe_codec::ClipboardMessage::ContentTypes(clientpipe_codec::ClipboardTypes { types: vec![clientpipe_codec::ClipboardType::Text.into()] }).into()).await.unwrap();
                            } else {
                                let s = match clipboard.current() {
                                    Some(offer) if offer.has_string() => offer.receive_string().unwrap(),
                                    _ => "failed".to_owned(),
                                };
                                tx.send(clientpipe_codec::ClipboardMessage::ClipboardContents(s.into_bytes()).into()).await.unwrap();
                            }
                        }
                        Some(clientpipe_codec::ClipboardMessage::ContentTypes(_types)) => (), // ignored, we assume string only
                        Some(clientpipe_codec::ClipboardMessage::ClipboardContents(m)) => {
                            let mut ocr = outstanding_clipboard_request.lock().unwrap();
                            if let Some(s) = ocr.take() {
                                s.send(String::from_utf8(m).unwrap()).unwrap();
                            }
                        }
                        None => unreachable!(),
                    }
                }
                clientpipe_codec::GaCmdOut::SetMousePosition(_point) => (), // unimplemented
            }
        }
    });
    a.await;

    Ok(())
}

fn request_shutdown_privileges() -> windows::runtime::Result<()> {
    use windows::Win32::Security::*;
    unsafe {
        let mut token: HANDLE = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).ok()?;
        let mut privname: Vec<_> = OsStr::new("SeShutdownPrivilege").encode_wide().chain([0]).collect();
        let mut luid = LUID::default();
        LookupPrivilegeValueW(None, PWSTR(privname.as_mut_ptr()), &mut luid).ok()?;
        let privs = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        AdjustTokenPrivileges(token, false, &privs, 0, ptr::null_mut(), ptr::null_mut()).ok()?;
    }
    Ok(())
}

fn release_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    unsafe {
        unsafe fn i(vk: VIRTUAL_KEY) -> INPUT {
            INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: GetMessageExtraInfo().0 as usize } } }
        }
        let inputs = [
            i(VK_SHIFT), i(VK_LSHIFT), i(VK_RSHIFT),
            i(VK_CONTROL), i(VK_LCONTROL), i(VK_RCONTROL),
            i(VK_MENU), i(VK_LMENU), i(VK_RMENU),
            i(VK_LWIN), i(VK_RWIN),
        ];
        SendInput(inputs.len() as u32, inputs.as_ptr(), mem::size_of_val(&inputs[0]) as i32);
    }
}
