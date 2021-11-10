use std::os::unix::prelude::{AsRawFd, RawFd};
use std::sync::{Arc, RwLock};

use tokio::io::unix::AsyncFd;
use tokio::sync::Notify;
use wayland_client::{Attached, Display, EventQueue};
use wayland_client::protocol::wl_display;

use super::cancel::make_cancelable;

struct EventQueueWrapper {
    queue: RwLock<EventQueue>,
    fd: RawFd,
}
impl From<EventQueue> for EventQueueWrapper {
    fn from(queue: EventQueue) -> Self {
        EventQueueWrapper {
            fd: queue.display().get_connection_fd(),
            queue: RwLock::new(queue),
        }
    }
}
impl AsRawFd for EventQueueWrapper {
    fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
        self.fd
    }
}

pub struct WaylandEventLoop {
    queue: AsyncFd<EventQueueWrapper>,
    display: Attached<wl_display::WlDisplay>,
    write_notify: Arc<Notify>,
}

impl WaylandEventLoop {
    pub fn new() -> anyhow::Result<Self> {
        let display = Display::connect_to_env()?;
        let queue = display.create_event_queue();
        let display = display.attach(queue.token());
        let write_notify = Default::default();
        Ok(WaylandEventLoop { queue: AsyncFd::new(EventQueueWrapper::from(queue))?, display, write_notify })
    }
    pub fn write_notify(&self) -> &Arc<Notify> {
        &self.write_notify
    }
    pub fn display(&self) -> &Attached<wl_display::WlDisplay> {
        &self.display
    }
    pub async fn roundtrip(&mut self) -> anyhow::Result<()> {
        let display = self.display.clone();

        let (runtask, cancel) = make_cancelable(self.run());
        let cb = display.sync();

        let mut cancel_holder = Some(cancel);
        cb.quick_assign(move |_, _, _| {
            cancel_holder.take();
        });

        runtask.await.unwrap_or(Ok(()))
    }
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // while not done

        let write_task = async {
            loop {
                let mut guard = self.queue.writable().await?;
                match guard.try_io(|fd| fd.get_ref().queue.read().unwrap().display().flush()) {
                    Ok(r) => r?,
                    Err(_) => continue,
                }

                // if we're done writing, wait for a notification to continue working
                self.write_notify.notified().await;
            }
            #[allow(unreachable_code)] // infinite loop but need to use this trick to annotate types
            Ok::<(), anyhow::Error>(())
        };
        let read_task = async {
            loop {
                let mut guard = self.queue.readable().await?;
                let prepared_read = guard.get_inner().queue.read().unwrap().prepare_read();
                if let Some(res) = prepared_read {
                    match guard.try_io(|_| res.read_events()) {
                        Ok(r) => r?,
                        Err(_) => continue,
                    }
                }
                guard.get_inner().queue.write().unwrap().dispatch_pending(&mut (), |_, _, _| {})?;
            }
            #[allow(unreachable_code)] // infinite loop but need to use this trick to annotate types
            Ok::<(), anyhow::Error>(())
        };

        tokio::try_join!(write_task, read_task)?;

        Ok(())
    }
}