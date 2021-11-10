//! Sending data to the clipboard

use std::{ffi::OsStr, os::windows::prelude::OsStrExt, slice};

use tokio::sync::oneshot;
use windows::Win32::{Foundation::HANDLE, System::{Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}, SystemServices::CLIPBOARD_FORMATS}};

use crate::raw::WindowsClipboardOwned;

/// A set of data in different formats to be put onto the clipboard.
#[derive(Debug)]
pub struct ClipboardContents(pub Vec<(CLIPBOARD_FORMATS, ClipboardFormatContent)>);

/// Data to be put onto the clipboard, which may be delay-rendered.
#[derive(Debug)]
pub enum ClipboardFormatContent {
    /// Delay-rendered data.
    ///
    /// You must provide a `Sender`. One the format is requested, you will receive
    /// a `Sender` which you must then use to send the delay-rendered data.
    /// Requesting data from the clipboard is a blocking operation in Windows,
    /// so you should make sure to respond as quickly as possible.
    DelayRendered(oneshot::Sender<oneshot::Sender<ClipboardFormatData>>),
    /// Data that is available immediately.
    Immediate(ClipboardFormatData),
}

/// Data to be put onto the clipboard.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ClipboardFormatData {
    Text(String),
    Bytes(Vec<u8>),
    #[cfg(feature = "image")]
    Image(image::DynamicImage),
}

impl ClipboardFormatData {
    /// Render this data to a clipboard.
    pub fn render(&self, clipboard: &mut WindowsClipboardOwned<'_>, format: CLIPBOARD_FORMATS) -> windows::runtime::Result<()> {
        let mut buf = Vec::new();

        let buf = match self {
            ClipboardFormatData::Text(s) => {
                buf.extend(OsStr::new(&s).encode_wide().chain([0u16]).flat_map(|c| c.to_le_bytes()));
                &buf
            }
            ClipboardFormatData::Bytes(b) => b.as_slice(),
            #[cfg(feature = "image")]
            ClipboardFormatData::Image(i) => {
                i.write_to(&mut buf, image::ImageOutputFormat::Bmp)
                    .expect("writing image data to a memory buffer failed (should not be possible)");
                &buf[14..]
            }
        };
        let handle = alloc_hglobal(buf);

        clipboard.send(format, handle)
    }
}

fn alloc_hglobal(b: &[u8]) -> HANDLE {
    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, b.len());
        let ptr = GlobalLock(hglobal);
        slice::from_raw_parts_mut(ptr as *mut u8, b.len()).copy_from_slice(b);
        GlobalUnlock(hglobal);
        HANDLE(hglobal)
    }
}
