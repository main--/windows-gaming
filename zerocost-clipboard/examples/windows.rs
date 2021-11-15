use image::GenericImageView;
use tokio::sync::oneshot;
use windows_eventloop::WindowsEventLoop;
use zerocost_clipboard::{DelayRenderedClipboardData, WindowsClipboard};
use zerocost_clipboard::send::{ClipboardContents, ClipboardFormatContent};

#[tokio::main]
async fn main() {
    env_logger::init();

    let wel = WindowsEventLoop::init().await.unwrap();
    let clipboard = WindowsClipboard::init(&wel).await.unwrap();

    let (txd, rxd) = oneshot::channel::<oneshot::Sender<String>>();
    tokio::spawn(async move {
        if let Ok(lel) = rxd.await {
            println!("delay rendering!");
            lel.send("delayed".to_owned()).unwrap();
        }
    });

    clipboard.send(ClipboardContents(vec![
        ClipboardFormatContent::DelayRendered(DelayRenderedClipboardData::Text(txd))
    ])).unwrap();

    let mut watch = clipboard.watch();
    for _ in 0..10 {
        let guard = watch.borrow_and_update();
        if let Some(offer) = guard.as_ref() {
            if offer.has_string() {
                println!("got text: {:?}", offer.receive_string());
            } else if offer.has_image() {
                let img = offer.receive_image().unwrap();
                println!("got image with {:?} pixels", img.dimensions());
            } else {
                println!("got something else: {:?}", offer);
            }
        }

        drop(guard);
        watch.changed().await.unwrap();
    }

    wel.shutdown().await.unwrap();
}
