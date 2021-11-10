use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::ops::Deref;
use std::os::unix::prelude::{AsRawFd, FromRawFd};
use std::rc::Rc;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, mpsc, watch};
use tokio_pipe::{PipeRead, PipeWrite};
use wayland_client::GlobalManager;
use wayland_client::Main;
use wayland_client::global_filter;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_protocols::wlr::unstable::data_control::v1::client::zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1};
use wayland_protocols::wlr::unstable::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols::wlr::unstable::data_control::v1::client::zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1};
use wayland_protocols::wlr::unstable::data_control::v1::client::zwlr_data_control_source_v1;

use crate::cancel::make_cancelable;
use crate::eventloop::WaylandEventLoop;

pub struct WaylandClipboardInternal {
    clipboard_manager: Main<ZwlrDataControlManagerV1>,
    device_clipboard: Main<ZwlrDataControlDeviceV1>,

    current_offer: watch::Receiver<Option<ClipboardOffer>>,
    notify_write: Arc<Notify>,
}


impl WaylandClipboardInternal {
    pub async fn init(wel: &mut WaylandEventLoop) -> anyhow::Result<Self> {
        let notify_write = wel.write_notify().clone();

        let seats = Rc::new(RefCell::new(Vec::new()));
        let seats_2 = seats.clone();
        let global_manager = GlobalManager::new_with_cb(wel.display(),
        global_filter!([WlSeat, 2, move |seat: Main<WlSeat>, _: DispatchData| {
            seats_2.borrow_mut().push(seat);
        }]));

        wel.roundtrip().await?;


        let clipboard_manager = global_manager.instantiate_exact::<ZwlrDataControlManagerV1>(1)?;

        // TODO: respect XDG_SEAT (or some other mechanism)
        let device_clipboard = clipboard_manager.get_data_device(&seats.borrow()[0]);

        let notify = notify_write.clone();
        let (current_offer_set, current_offer) = watch::channel(None);
        device_clipboard.quick_assign(move |_data_device, event, _dispatch_data| {
            match event {
                zwlr_data_control_device_v1::Event::DataOffer { id } => {
                    // as soon as a data offer appears, we must set a handler to collect mime types
                    id.as_ref().user_data().set(move || RefCell::new(HashSet::<String>::new()));
                    id.quick_assign(|offer, event, _| {
                        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
                            let mime = offer.as_ref().user_data().get::<RefCell<HashSet<String>>>().unwrap();
                            mime.borrow_mut().insert(mime_type);
                        }
                    });
                },
                zwlr_data_control_device_v1::Event::Selection { id } => {
                    let notify = notify.clone();
                    let _ = current_offer_set.send(id.map(|offer| ClipboardOffer { offer, notify }));
                }
                _ => (),
            }
        });
        // make sure that the current clipboard state has arrived
        // (so that if you check self.current() right away you don't always get None)
        wel.roundtrip().await?;

        Ok(WaylandClipboardInternal { clipboard_manager, device_clipboard, current_offer, notify_write })
    }

    pub fn incoming(&self) -> &watch::Receiver<Option<ClipboardOffer>> {
        &self.current_offer
    }
    pub fn current(&self) -> Option<ClipboardOffer> {
        self.current_offer.borrow().clone()
    }


    pub fn claim(&self, mime_types: impl Iterator<Item=String>) -> mpsc::UnboundedReceiver<ClipboardRequest> {
        let (tx, rx) = mpsc::unbounded_channel();
        let ds = self.clipboard_manager.create_data_source();

        let device_clipboard = self.device_clipboard.detach();
        let tx2 = tx.clone();
        let notify_write = self.notify_write.clone();
        let (undo_copy_task, undo_guard) = make_cancelable(async move {
            // make it so that dropping the receiver clears our selection out of the clipboard
            // BUT only if we haven't been cancelled yet (or else we would yoink someone else from the clipboard)
            tx2.closed().await;
            device_clipboard.set_selection(None);
            notify_write.notify_one();
        });
        tokio::spawn(undo_copy_task);

        let undo_token = undo_guard.disarm();
        let mut sender = Some(tx);
        ds.quick_assign(move |_data_source, event, _dispatch_data| {
            match event {
                zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                    let pipe = unsafe { PipeWrite::from_raw_fd(fd) };
                    if let Some(sender) = sender.as_ref() {
                        let _ = sender.send(ClipboardRequest { mime_type, target: pipe });
                    }
                },
                zwlr_data_control_source_v1::Event::Cancelled => {
                    undo_token.cancel();
                    sender.take();
                }
                _ => (),
            }
        });

        for mime in mime_types {
            ds.offer(mime);
        }

        self.device_clipboard.set_selection(Some(&ds));
        self.notify_write.notify_one();

        rx
    }
    pub fn claim_string(&self, text: String) -> impl Future<Output=()> + Send {
        let mut receiver = self.claim(PLAINTEXT_MIME_TYPES.iter().copied().map(str::to_owned));
        async move {
            while let Some(req) = receiver.recv().await {
                let mut target = req.into_target();
                // errors when writing to copy targets should be ignored (not our problem if they close the pipe early)
                let _ = target.write_all(text.as_bytes()).await;
            }
        }
    }
}

/// You should offer all of these MIME types when offering UTF-8 text in order to ensure compatibility with other applications.
pub const PLAINTEXT_MIME_TYPES: &[&str] = &[
    "text/plain",
    "text/plain;charset=utf-8",
    "STRING",
    "UTF8_STRING",
    "TEXT",
];

/// An application is requesting clipboard data from you.
///
/// Use `target` or `into_target` to deliver the response.
pub struct ClipboardRequest {
    mime_type: String,
    target: PipeWrite,
}
impl ClipboardRequest {
    /// The MIME type they asked for.
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }
    /// A reference to write the data into.
    pub fn target(&mut self) -> &mut (impl AsyncWrite + AsRawFd) {
        &mut self.target
    }
    /// A value to write the data into.
    pub fn into_target(self) -> impl AsyncWrite + AsRawFd {
        self.target
    }
}

/// An application is offering data on the clipboard.
#[derive(Clone)]
pub struct ClipboardOffer {
    offer: ZwlrDataControlOfferV1,
    notify: Arc<Notify>,
}
/*
TODO: is this even needed?
If yes, need to track who is the last clone so we don't UAF

impl Drop for ClipboardOffer {
    fn drop(&mut self) {
        self.offer.destroy()
    }
}
*/
impl ClipboardOffer {
    /// Set of MIME types they are offering.
    pub fn mime_types(&self) -> impl Deref<Target=HashSet<String>> + '_ {
        let mime = self.offer.as_ref().user_data().get::<RefCell<HashSet<String>>>().unwrap();
        mime.borrow()
    }
    /// Receive clipboard contents as `String`.
    ///
    /// Convenience wrapper around `receive_bytes`.
    pub async fn receive_string(&self) -> anyhow::Result<String> {
        let v = self.receive_bytes("text/plain;charset=utf-8".to_owned()).await?;
        Ok(String::from_utf8(v)?)
    }
    /// Receive clipboard contents of a given MIME type as a reader.
    pub async fn receive_reader(&self, mime: impl Into<String>) -> anyhow::Result<PipeRead> {
        let mime = mime.into();
        if !self.mime_types().contains(&mime) {
            anyhow::bail!("The requested MIME type is not available");
        }
        let (r, w) = tokio_pipe::pipe()?;
        self.offer.receive(mime, w.as_raw_fd());
        // this is dangerous because we're closing the fd before the eventloop has a chance
        // to send the message containing it.
        // however, wayland-client dups the file descriptor, presumably to avoid exactly this issue
        drop(w);
        self.notify.notify_one();
        Ok(r)
    }
    /// Receive clipboard contents of a given MIME type as `Vec<u8>`.
    ///
    /// Convenience wrapper around `receive_reader`.
    pub async fn receive_bytes(&self, mime: impl Into<String>) -> anyhow::Result<Vec<u8>> {
        let mut r = self.receive_reader(mime).await?;
        let mut v = Vec::new();
        r.read_to_end(&mut v).await?;
        Ok(v)
    }
}
