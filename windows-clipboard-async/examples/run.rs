use image::GenericImageView;
use tokio::sync::oneshot;
use windows_clipboard_async::send::{ClipboardContents, ClipboardFormatContent, ClipboardFormatData};

#[tokio::main]
async fn main() {
    let clipboard = windows_clipboard_async::WindowsClipboard::init().await.unwrap();

    let (txd, rxd) = oneshot::channel::<oneshot::Sender<ClipboardFormatData>>();
    tokio::spawn(async move {
        if let Ok(lel) = rxd.await {
            println!("delay rendering!");
            lel.send(ClipboardFormatData::Text("delayed".to_owned())).unwrap();
        }
    });

    clipboard.send(ClipboardContents(vec![
        (windows_clipboard_async::format::CF_UNICODETEXT, ClipboardFormatContent::DelayRendered(txd))
    ])).await.ok().unwrap();

    let mut watch = clipboard.watch();
    for _ in 0..10 {
        let guard = watch.borrow_and_update();
        let offer = guard.as_ref().unwrap();

        if offer.has_string() {
            println!("got text: {:?}", offer.receive_string());
        } else if offer.has_image() {
            let img = offer.receive_image().unwrap();
            println!("got image with {:?} pixels", img.dimensions());
        } else {
            println!("got something else: {:?}", offer);
        }
        drop(guard);
        watch.changed().await.unwrap();
    }

    clipboard.shutdown().await.unwrap();
}
