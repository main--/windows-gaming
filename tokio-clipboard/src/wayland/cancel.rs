use std::future::Future;

use tokio_util::sync::{CancellationToken, DropGuard};


pub fn make_cancelable<T>(f: impl Future<Output=T>) -> (impl Future<Output=Option<T>>, DropGuard) {
    let token = CancellationToken::new();
    let guard = token.clone().drop_guard();
    let task = async move {
        tokio::select! {
            res = f => Some(res),
            _ = token.cancelled() => None,
        }
    };
    (task, guard)
}
