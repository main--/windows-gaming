//! Sending data to the clipboard

use std::mem;
use std::{ffi::OsStr, os::windows::prelude::OsStrExt, slice};

use tokio::sync::oneshot;
use windows::Win32::System::Memory::GlobalFree;
use windows::Win32::{Foundation::HANDLE, System::{Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}, SystemServices::CLIPBOARD_FORMATS}};

use super::format;
use super::raw::WindowsClipboardOwned;

/// A set of data in different formats to be put onto the clipboard.
#[derive(Debug)]
pub struct ClipboardContents(pub Vec<ClipboardFormatContent>);

/// Data to be put onto the clipboard, which may be delay-rendered.
#[derive(Debug)]
pub enum ClipboardFormatContent {
    /// Delay-rendered data.
    DelayRendered(DelayRenderedClipboardData),
    /// Data that is available immediately.
    Immediate(ClipboardFormatData),
}

/// Data to be delay-rendered onto the clipboard.
///
/// You must provide a `Sender`. One the format is requested, you will receive
/// a `Sender` which you must then use to send the delay-rendered data.
/// Requesting data from the clipboard is a blocking operation in Windows,
/// so you should make sure to respond as quickly as possible.
///
/// If your delay-renderer hangs forever, our whole clipboard thread will hang forever.
/// It's as simple as that.
#[non_exhaustive]
#[derive(Debug)]
pub enum DelayRenderedClipboardData {
    Text(oneshot::Sender<oneshot::Sender<String>>),
    #[cfg(feature = "image")]
    Image(oneshot::Sender<oneshot::Sender<image::DynamicImage>>),
    CustomBytes(CLIPBOARD_FORMATS, oneshot::Sender<oneshot::Sender<Vec<u8>>>),
    CustomHandle(CLIPBOARD_FORMATS, oneshot::Sender<oneshot::Sender<DestructibleHandle>>),
}
impl DelayRenderedClipboardData {
    /// Returns a matching clipboard format for this data.
    pub fn format(&self) -> CLIPBOARD_FORMATS {
        match self {
            DelayRenderedClipboardData::Text(_) => format::CF_UNICODETEXT,
            #[cfg(feature = "image")]
            DelayRenderedClipboardData::Image(_) => format::CF_DIB,
            &DelayRenderedClipboardData::CustomBytes(f, _)
            | &DelayRenderedClipboardData::CustomHandle(f, _) => f,
        }
    }
    /// Execute the delay renderer in order to obtain the clipboard data.
    ///
    /// Returns `None` if the delay renderer does not respond.
    pub async fn delay_render(self) -> Option<ClipboardFormatData> {
        match self {
            DelayRenderedClipboardData::Text(s) => send_recv_oneshot(s).await.map(ClipboardFormatData::from),
            #[cfg(feature = "image")]
            DelayRenderedClipboardData::Image(s) => send_recv_oneshot(s).await.map(ClipboardFormatData::from),
            DelayRenderedClipboardData::CustomBytes(f, s) => send_recv_oneshot(s).await.map(move |s| ClipboardFormatData::from((f, s))),
            DelayRenderedClipboardData::CustomHandle(f, s) => send_recv_oneshot(s).await.map(move |s| ClipboardFormatData::from((f, s))),
        }
    }
}
async fn send_recv_oneshot<T>(s: oneshot::Sender<oneshot::Sender<T>>) -> Option<T> {
    let (tx, rx) = oneshot::channel();
    match s.send(tx) {
        Ok(()) => rx.await.ok(),
        Err(_) => None,
    }
}

/// Represents a `HANDLE` alongside with a function to destroy it in case we failed to post it to the clipboard.
#[derive(Debug)]
pub struct DestructibleHandle {
    pub handle: HANDLE,
    pub destructor: fn(HANDLE),
}
impl Drop for DestructibleHandle {
    fn drop(&mut self) {
        (self.destructor)(self.handle);
    }
}

/// Data to be put onto the clipboard.
#[non_exhaustive]
#[derive(Debug)]
pub enum ClipboardFormatData {
    Text(String),
    #[cfg(feature = "image")]
    Image(image::DynamicImage),
    CustomBytes(CLIPBOARD_FORMATS, Vec<u8>),
    CustomHandle(CLIPBOARD_FORMATS, DestructibleHandle),
}
impl From<String> for ClipboardFormatData { fn from(x: String) -> Self { ClipboardFormatData::Text(x) } }
#[cfg(feature = "image")]
impl From<image::DynamicImage> for ClipboardFormatData { fn from(x: image::DynamicImage) -> Self { ClipboardFormatData::Image(x) } }
impl From<(CLIPBOARD_FORMATS, Vec<u8>)> for ClipboardFormatData { fn from((x, y): (CLIPBOARD_FORMATS, Vec<u8>)) -> Self { ClipboardFormatData::CustomBytes(x, y) } }
impl From<(CLIPBOARD_FORMATS, DestructibleHandle)> for ClipboardFormatData { fn from((x, y): (CLIPBOARD_FORMATS, DestructibleHandle)) -> Self { ClipboardFormatData::CustomHandle(x, y) } }


impl ClipboardFormatData {
    /// Render this data to a clipboard.
    pub fn render(self, clipboard: &mut WindowsClipboardOwned<'_>) -> windows::runtime::Result<()> {
        let (format, handle) = match self {
            ClipboardFormatData::Text(s) => {
                let buf: Vec<_> = OsStr::new(&s).encode_wide().chain([0u16]).flat_map(|c| c.to_le_bytes()).collect();
                (format::CF_UNICODETEXT, alloc_hglobal(&buf))
            }
            #[cfg(feature = "image")]
            ClipboardFormatData::Image(i) => {
                let mut buf = Vec::new();
                // we rely on the fact that image's bmp encoder always creates a DIB (BITMAPINFOHEADER) for Rgb8
                let i = image::DynamicImage::ImageRgb8(i.into_rgb8());
                i.write_to(&mut buf, image::ImageOutputFormat::Bmp)
                    .expect("writing image data to a memory buffer failed (should not be possible)");
                (format::CF_DIB, alloc_hglobal(&buf[14..]))
            }
            ClipboardFormatData::CustomBytes(format, b) => (format, alloc_hglobal(&b)),
            ClipboardFormatData::CustomHandle(format, handle) => (format, handle),
        };

        clipboard.send(format, handle.handle)?;
        mem::forget(handle); // do not close the handle if we successfully posted it to the clipboard
        Ok(())
    }
}

fn alloc_hglobal(b: &[u8]) -> DestructibleHandle {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, b.len());
        let ptr = GlobalLock(hglobal);
        slice::from_raw_parts_mut(ptr as *mut u8, b.len()).copy_from_slice(b);
        GlobalUnlock(hglobal);
        DestructibleHandle {
            handle: HANDLE(hglobal),
            destructor: |h| { GlobalFree(h.0); },
        }
    }
}
