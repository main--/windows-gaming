use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_WIN, VK_DELETE};
use windows_eventloop::WindowsEventLoop;
use windows_keybinds::HotKeyManager;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();

    let wel = WindowsEventLoop::init().await.unwrap();
    let mut kb = HotKeyManager::new(Box::new(wel)).await;
    let mut reg = kb.register_hotkey(MOD_CONTROL | MOD_WIN, VK_DELETE.0.into()).await;
    for _ in 0..3 {
        reg.recv().await.unwrap();
        println!("got one hk");
    }

    kb.into_inner().shutdown().await.unwrap();
}
