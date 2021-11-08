#[tokio::main(flavor = "current_thread")]
async fn main() {
    let x = windows_clipboard_async::WindowsClipboard::init().await.unwrap();
    let mut rxo = x.rx_offers.clone();
    loop {
        let borrow_and_update = rxo.borrow_and_update();
        let offer = borrow_and_update.as_ref().unwrap();
        println!("{:?}", offer);
        if offer.formats().any(|x| x == windows_clipboard_async::format::CF_UNICODETEXT) {
            println!("recv: {:?}", offer.receive_string());
        } else {
            let memes = offer.receive_image().unwrap();
            let memes = memes.to_owned();
            println!("recv: {:?}", memes);
        }
        drop(borrow_and_update);
        rxo.changed().await.unwrap();
    }
}
