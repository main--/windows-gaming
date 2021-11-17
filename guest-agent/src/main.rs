#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsStr;
use std::io::Cursor;
use std::os::windows::prelude::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::{mem, ptr};
use std::sync::{Arc, Mutex};

use clientpipe_proto::{ClipboardType, ClipboardTypes};
use futures_util::StreamExt;
use image::DynamicImage;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::codec::{FramedRead, FramedWrite};
use windows::Win32::Foundation::{BOOLEAN, HANDLE, LUID, PWSTR};
use windows::Win32::System::Console::AllocConsole;
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{EWX_POWEROFF, EXIT_WINDOWS_FLAGS, ExitWindowsEx, SHTDN_REASON_FLAG_PLANNED};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{EWX_FORCE, GetMessageExtraInfo};
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
            let types = match clip_watch.borrow_and_update().as_ref() {
                Some(offer) if offer.owner() != wel_hwnd => {
                    let mut types = ClipboardTypes::default();
                    if offer.has_string() {
                        types.push_types(ClipboardType::Text);
                    }
                    if offer.has_image() {
                        types.push_types(ClipboardType::Image);
                    }
                    types
                }
                _ => continue,
            };

            tx2.send(clientpipe_codec::ClipboardMessage::GrabClipboard(types).into()).await.unwrap();
        }
    });

    tx.send(clientpipe_codec::GaCmdIn::ReportBoot(())).await.unwrap();


    let outstanding_clipboard_request = Arc::new(Mutex::<Option<oneshot::Sender<Vec<u8>>>>::new(None));
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
                    let res = unsafe { ExitWindowsEx(EWX_POWEROFF | EXIT_WINDOWS_FLAGS(EWX_FORCE), SHTDN_REASON_FLAG_PLANNED.0) }.ok();
                    if let Err(e) = res {
                        log::error!("Failed to shut down: {:?}", e);
                    }
                }
                clientpipe_codec::GaCmdOut::Clipboard(c) => {
                    log::trace!("handling clipboard {:?}", c);
                    match c.message {
                        Some(clientpipe_codec::ClipboardMessage::GrabClipboard(types)) => {
                            let spawn_delay_renderer_for_type = |t: ClipboardType| {
                                let (ttx, rx) = oneshot::channel();
                                let outstanding_clipboard_request = outstanding_clipboard_request.clone();
                                let tx = tx.clone();
                                tokio::spawn(async move {
                                    if let Ok(s) = rx.await {
                                        *outstanding_clipboard_request.lock().unwrap() = Some(s);
                                        tx.send(clientpipe_codec::ClipboardMessage::RequestClipboardContents(t.into()).into()).await.unwrap();
                                    }
                                });



                                let drcd = match t {
                                    ClipboardType::Invalid => unreachable!(),
                                    ClipboardType::Text => {
                                        let (tx2, rx2) = oneshot::channel::<oneshot::Sender<String>>();
                                        tokio::spawn(async move {
                                            if let Ok(s) = rx2.await {
                                                let (a, b) = oneshot::channel();
                                                ttx.send(a).unwrap();

                                                let bytes = b.await.unwrap_or(Vec::new());
                                                let _ = s.send(String::from_utf8_lossy(&bytes).into_owned());
                                            }
                                        });
                                        DelayRenderedClipboardData::Text(tx2)
                                    }
                                    ClipboardType::Image => {
                                        let (tx2, rx2) = oneshot::channel::<oneshot::Sender<DynamicImage>>();
                                        tokio::spawn(async move {
                                            if let Ok(s) = rx2.await {
                                                let (a, b) = oneshot::channel();
                                                ttx.send(a).unwrap();

                                                let bytes = b.await.unwrap_or(Vec::new());
                                                let img = image::io::Reader::with_format(Cursor::new(bytes), image::ImageFormat::Png).decode().unwrap_or_else(|e| {
                                                    log::warn!("making an empty image for pasting because the image failed to decode: {:?}", e);
                                                    let empty_img = image::GrayImage::new(0, 0);
                                                    DynamicImage::ImageLuma8(empty_img)
                                                });
                                                let _ = s.send(img);
                                            }
                                        });
                                        DelayRenderedClipboardData::Image(tx2)
                                    }
                                };
                                ClipboardFormatContent::DelayRendered(drcd)
                            };

                            clipboard.send(ClipboardContents(types.types().map(spawn_delay_renderer_for_type).collect())).unwrap();
                        }
                        Some(clientpipe_codec::ClipboardMessage::RequestClipboardContents(typ)) => {
                            let data = match clipboard.current() {
                                None => None,
                                Some(offer) => match ClipboardType::from_i32(typ).unwrap() {
                                    ClipboardType::Invalid => None,
                                    ClipboardType::Text => offer.receive_string().ok().map(String::into_bytes),
                                    ClipboardType::Image => {
                                        offer.receive_image().ok().and_then(|i| {
                                            let mut bytes = Vec::new();
                                            i.write_to(&mut bytes, image::ImageOutputFormat::Png).ok().map(|_| bytes)
                                        })
                                    }
                                },
                            };

                            tx.send(clientpipe_codec::ClipboardMessage::ClipboardContents(data.unwrap_or(Vec::new())).into()).await.unwrap();
                        }
                        Some(clientpipe_codec::ClipboardMessage::ClipboardContents(m)) => {
                            let mut ocr = outstanding_clipboard_request.lock().unwrap();
                            if let Some(s) = ocr.take() {
                                s.send(m).unwrap();
                            }
                        }
                        None => unreachable!(),
                    }
                }
                clientpipe_codec::GaCmdOut::SetMousePosition(_point) => (), // unimplemented
                clientpipe_codec::GaCmdOut::EnableDebugConsole(rust_log) => {
                    if !has_output() {
                        unsafe {
                            AllocConsole().ok().unwrap();
                            assert!(has_output()); // technically redundant, since otherwise the env_logger initialization just panics
                            env_logger::init_from_env(env_logger::Env::new().default_filter_or(rust_log));
                        }
                    } // else ignore, console is already open
                }
            }
        }
    });
    a.await;

    Ok(())
}

fn has_output() -> bool {
    !std::io::stdout().as_raw_handle().is_null()
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
