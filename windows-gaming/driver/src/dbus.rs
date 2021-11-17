use std::collections::VecDeque;
use std::ops::DerefMut;
use std::os::unix::prelude::RawFd;

use futures03::Stream;
use libdbus::{Connection, ConnectionItem, Watch, WatchEvent};
use mio::Ready;
use mio::unix::UnixReady;
use futures::{Async};

use tokio1::io::unix::AsyncFd;

pub struct DBusItems<'a> {
    conn: &'a Connection,
    fds: Vec<Fd>,
    pending_items: VecDeque<ConnectionItem>,
}

struct Fd {
    io: AsyncFd<RawFd>,
    interest: Ready,
}

impl<'a> DBusItems<'a> {
    pub fn new(bus: &'a Connection) -> DBusItems<'a> {
        DBusItems {
            fds: bus.watch_fds().iter().map(|x| {
                Fd {
                    io: AsyncFd::new(x.fd()).unwrap(),
                    interest: to_ready(&x),
                }
            }).collect(),
            conn: bus,
            pending_items: VecDeque::new(),
        }
    }
}

impl Fd {
    fn poll(&self, cx: &mut std::task::Context<'_>) -> Async<Ready> {
        let mut ready = Ready::empty();
        if self.interest.contains(Ready::readable()) {
            if self.io.poll_read_ready(cx).is_ready() {
                ready |= Ready::readable();
            }
        }
        if self.interest.contains(Ready::writable()) {
            if self.io.poll_write_ready(cx).is_ready() {
                ready |= Ready::writable();
            }
        }

        if ready.is_empty() {
            Async::NotReady
        } else {
            Async::Ready(ready)
        }
    }
}

impl<'a> Stream for DBusItems<'a> {
    type Item = Result<ConnectionItem, ()>;

    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        // if we still got data, don't touch the fds
        if let Some(x) = self.pending_items.pop_front() {
            return std::task::Poll::Ready(Some(Ok(x)));
        }

        // query fds
        let results: Vec<_> = self.fds.iter().flat_map(|fd| match fd.poll(cx) {
            Async::Ready(x) => Some((*fd.io.get_ref(), x)),
            Async::NotReady => None,
        }).collect();

        // borrow things to keep borrowck happy
        let myself = self.deref_mut();
        let fds = &mut myself.fds;
        let conn = &myself.conn;
        // make dbus process fds and collect the results
        myself.pending_items.extend(
            results.into_iter().flat_map(|(fd, r)| conn.watch_handle(fd, from_ready(r)))
                .flat_map(|ci| match ci {
                    // eat WatchFd events
                    ConnectionItem::WatchFd(w) => {
                        let ready = to_ready(&w);
                        let pos = fds.iter().position(|x| *x.io.get_ref() == w.fd());
                        if ready.is_empty() {
                            // removed
                            if let Some(i) = pos {
                                fds.remove(i);
                            }
                        } else {
                            // add or update
                            if let Some(i) = pos {
                                fds[i].interest = ready;
                            } else {
                                fds.push(Fd {
                                    io: AsyncFd::new(w.fd()).unwrap(),
                                    interest: ready,
                                });
                            }
                        }
                        None
                    }
                    _ => Some(ci),
                }));

        // finally, return results (if any)
        if let Some(x) = self.pending_items.pop_front() {
            std::task::Poll::Ready(Some(Ok(x)))
        } else {
            std::task::Poll::Pending
        }
    }
}

// yuck
fn from_ready(r: Ready) -> u32 {
    let mut ret = 0;
    if r.is_readable() {
        ret |= WatchEvent::Readable as u32;
    }
    if r.is_writable() {
        ret |= WatchEvent::Writable as u32;
    }
    if UnixReady::from(r).is_hup() {
        ret |= WatchEvent::Hangup as u32;
    }
    if UnixReady::from(r).is_error() {
        ret |= WatchEvent::Error as u32;
    }
    ret
}

fn to_ready(w: &Watch) -> Ready {
    let mut r = Ready::empty();
    if w.readable() {
        r = r | Ready::readable();
    }
    if w.writable() {
        r = r | Ready::writable();
    }
    r
}
