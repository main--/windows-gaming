use tokio::sync::{mpsc, oneshot, watch};

use super::ClipboardRequest;
use super::eventloop::WaylandEventLoop;
use super::clipboard_internal::{ClipboardOffer, WaylandClipboardInternal};


enum Command {
    Get(oneshot::Sender<Option<ClipboardOffer>>),
    Subscribe(oneshot::Sender<watch::Receiver<Option<ClipboardOffer>>>),
    Claim { mime_types: Vec<String>, sender: oneshot::Sender<mpsc::UnboundedReceiver<ClipboardRequest>> },
    ClaimString(String),
}

/// Handle to the Wayland clipboard.
///
/// Dropping this handle causes all clipboard functionality (clipboard watching and providing data) to cease.
#[derive(Clone)]
pub struct WaylandClipboard {
    sender: mpsc::Sender<Command>,
}

impl WaylandClipboard {
    /// Initialize the clipboard.
    ///
    /// Returns a `Future` as well as a `WaylandClipboard`.
    /// The `Future` is the task that takes care of actually running the clipboard.
    /// It will complete once you drop the `WaylandClipboard`.
    /// `WaylandClipboard` does nothing unless you schedule this task.
    pub async fn init() -> anyhow::Result<(impl std::future::Future<Output=anyhow::Result<()>>, Self)> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Command>(1);

        log::trace!("making job");
        let job = async move {
            log::trace!("job booting");
            let mut queue = WaylandEventLoop::new()?;
            log::trace!("have wel queue");
            let wc = WaylandClipboardInternal::init(&mut queue).await?;
            log::trace!("have wl clipboard");

            let command_handler = async move {
                log::trace!("entering commandhandler");
                while let Some(cmd) = rx.recv().await {
                    // in general for oneshot senders, we don't care if they don't want their result
                    match cmd {
                        Command::Get(sender) => {
                            let _ = sender.send(wc.current());
                        }
                        Command::Subscribe(sender) => {
                            let _ = sender.send(wc.incoming().clone());
                        }
                        Command::Claim { mime_types, sender } => {
                            let _ = sender.send(wc.claim(mime_types.into_iter()));
                        }
                        Command::ClaimString(text) => {
                            tokio::spawn(wc.claim_string(text));
                        }
                    }
                }
                // shutting down
                //
                // TODO: cleanup work maybe?
                Ok::<_, anyhow::Error>(())
            };

            tokio::select! {
                r = command_handler => r,
                r = queue.run() => r,
            }
        };

        Ok((job, WaylandClipboard { sender: tx }))
    }

    /// Obtain the current contents of the clipboard.
    pub async fn get(&self) -> Option<ClipboardOffer> {
        let (ttx, rx) = oneshot::channel();
        self.sender.send(Command::Get(ttx)).await.ok().unwrap();
        rx.await.unwrap()
    }
    /// Subscribe to clipboard changes.
    pub async fn subscribe(&self) -> watch::Receiver<Option<ClipboardOffer>> {
        let (ttx, rx) = oneshot::channel();
        self.sender.send(Command::Subscribe(ttx)).await.ok().unwrap();
        rx.await.unwrap()
    }
    /// Claim the clipboard and offer the given MIME types.
    pub async fn claim(&self, mime_types: impl IntoIterator<Item=String>) -> mpsc::UnboundedReceiver<ClipboardRequest> {
        let (ttx, rx) = oneshot::channel();
        self.sender.send(Command::Claim { mime_types: mime_types.into_iter().collect(), sender: ttx }).await.ok().unwrap();
        rx.await.unwrap()
    }
    /// Claim the clipboard and offer a fixed `String`.
    pub async fn claim_string(&self, text: String) {
        self.sender.send(Command::ClaimString(text)).await.ok().unwrap();
    }
}
