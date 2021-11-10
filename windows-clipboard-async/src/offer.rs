//! Offering data on the clipboard.

use std::{ffi::OsString, fmt::Debug, io::{BufRead, Read, Seek, SeekFrom}, os::windows::prelude::OsStringExt};
use windows::{Win32::{Foundation::{HANDLE, HWND}, System::SystemServices::CLIPBOARD_FORMATS}};
use thiserror::Error;

use crate::{format::{self, DebugFormats}, raw};

/// An application is offering data on the clipboard.
///
/// # Remarks
///
/// While this type may be cloned and moved arbitrarily, it is only useful as long as the
/// clipboard contents have not changed.
/// Once the clipboard contents change, it is no longer possible to receive contents through this offer.
///
/// ## Potential deadlock when receiving from yourself
///
/// When receiving delay-rendered content from an offer that your application put onto the clipboard
/// using the tokio current_thread runtime, you will run into the following deadlock:
///
/// - you call a the (synchronous) receive function
/// - windows asks the clipboard thread to render the delay-rendered content
/// - the clipboard thread asks your delay renderer to render its content
/// - your delay renderer can't run because your application thread is blocked waiting for
///   the clipboard contents to arrive
///
/// This deadlock "resolves" itself once Windows times out the clipboard request (after several seconds!).
/// Note that in this case, your delay-rendered content is removed from the clipboard by Windows.
#[derive(Clone)]
pub struct ClipboardOffer {
    sequence: u32,
    formats: Vec<CLIPBOARD_FORMATS>,
}
impl Debug for ClipboardOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardOffer")
            .field("sequence", &self.sequence)
            .field("formats", &DebugFormats(&self.formats))
            .finish()
    }
}


/// An error while receiving clipboard contents.
#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    /// The clipboard offer has expired
    #[error("The clipboard offer has expired.")]
    ClipboardOfferExpired,
    /// The requested format is not available
    #[error("The requested format is not available.")]
    FormatNotAvailable,
    /// The Windows API reported an error
    #[error(transparent)]
    Windows(#[from] windows::runtime::Error),
    /// Decoding the image data failed
    #[cfg(feature = "image")]
    #[error(transparent)]
    Image(#[from] image::ImageError),
}
type Result<T> = std::result::Result<T, Error>;

impl ClipboardOffer {
    pub(crate) fn new(sequence: u32, formats: Vec<CLIPBOARD_FORMATS>) -> Self {
        ClipboardOffer { sequence, formats }
    }

    /// Iterates over the formats currently available on the clipboard
    pub fn formats(&self) -> impl Iterator<Item=CLIPBOARD_FORMATS> + '_ {
        self.formats.iter().copied()
    }

    /// Receive any clipboard format as a `HANDLE`.
    ///
    /// The type of handle depends entirely on the format.
    pub fn receive_handle(&self, format: CLIPBOARD_FORMATS) -> Result<HANDLE> {
        if !self.formats().any(|f| f == format) {
            return Err(Error::FormatNotAvailable);
        }

        let clipboard = raw::WindowsClipboard::open(HWND(0));
        if clipboard.sequence_number() != self.sequence {
            return Err(Error::ClipboardOfferExpired);
        }

        Ok(clipboard.receive(format)?)
    }

    /// Receive any clipboard format as a memory buffer.
    ///
    /// This method only works for clipboard formats that are represented as handles
    /// to memory buffers.
    /// If the format is not a memory buffer (e.g. handle-based like `CF_HBITMAP`), it returns an error.
    pub fn receive_bytes(&self, format: CLIPBOARD_FORMATS) -> Result<Vec<u8>> {
        if !self.formats().any(|f| f == format) {
            return Err(Error::FormatNotAvailable);
        }

        let clipboard = raw::WindowsClipboard::open(HWND(0));
        if clipboard.sequence_number() != self.sequence {
            return Err(Error::ClipboardOfferExpired);
        }

        let buf = clipboard.receive_buffer(format)?;
        Ok(buf.to_owned())
    }

    /// Checks whether this clipboard offer contains image data.
    pub fn has_image(&self) -> bool {
        self.formats().any(|x| x == format::CF_DIBV5)
    }
    /// Receive a bitmap image from the clipboard.
    ///
    /// Returns the `FormatNotAvailable` error if there is no image data on the clipboard.
    #[cfg(feature = "image")]
    pub fn receive_image(&self) -> Result<image::DynamicImage> {
        use std::mem;

        use image::ImageFormat;
        use windows::Win32::Graphics::Gdi::BITMAPV5HEADER;

        let mem = self.receive_bytes(format::CF_DIBV5)?;
        // mem now contains essentially a .bmp file, but without the first part of the header
        // in a better world, we could just tell image crate to parse it anyway
        // (their bmp parser even has an internal flag to do just that)
        // sadly, that is not the word we live in

        // so instead, we quickly synthesize a bitmap header to make things work

        // std::io::Chain would solve this perfectly, if only it would implement Seek :((
        let multi = MultiCursor {
            bufs: &[b"BM", &[0; 8], &(mem::size_of::<BITMAPV5HEADER>() as u32).to_le_bytes(), &mem],
            position: 0,
        };

        Ok(image::load(multi, ImageFormat::Bmp)?)
    }

    /// Checks whether this clipboard offer contains text.
    pub fn has_string(&self) -> bool {
        self.formats().any(|x| x == format::CF_UNICODETEXT)
    }
    /// Receive a string from the clipboard.
    ///
    /// Returns the `FormatNotAvailable` error if there is no text on the clipboard.
    pub fn receive_string(&self) -> Result<String> {
        let memory = self.receive_bytes(format::CF_UNICODETEXT)?;
        unsafe {
            let (_, stringmem, _) = memory.align_to::<u16>();
            // remove null terminator
            let str = OsString::from_wide(&stringmem[..stringmem.len() - 1]);
            Ok(str.to_string_lossy().into_owned())
        }
    }
}

#[derive(Clone)]
struct MultiCursor<'a> {
    bufs: &'a [&'a [u8]],
    position: u64,
}
impl<'a> Read for MultiCursor<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut pos = self.position;
        for &b in self.bufs {
            let blen = u64::try_from(b.len()).unwrap();
            if pos < blen {
                let upos = usize::try_from(pos).unwrap();
                let readsz = std::cmp::min(buf.len(), (blen - pos).try_into().unwrap());
                buf[..readsz].copy_from_slice(&b[upos..][..readsz]);
                self.consume(readsz);
                return Ok(readsz);
            } else {
                pos -= blen;
            }
        }
        Ok(0)
    }
}
impl<'a> BufRead for MultiCursor<'a> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        let mut pos = self.position;
        for &b in self.bufs {
            let blen = u64::try_from(b.len()).unwrap();
            if pos < blen {
                let upos = usize::try_from(pos).unwrap();
                return Ok(&b[upos..]);
            } else {
                pos -= blen;
            }
        }
        Ok(&[])
    }

    fn consume(&mut self, amt: usize) {
        self.position += u64::try_from(amt).unwrap();
    }
}
impl<'a> Seek for MultiCursor<'a> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        // TODO: technically the math here needs to check for over/underflows and return errors accordingly
        self.position = match pos {
            SeekFrom::Start(i) => i,
            SeekFrom::Current(i) => ((self.position as i64) + i) as u64,
            SeekFrom::End(i) => (i64::try_from(self.bufs.iter().map(|b| b.len()).sum::<usize>()).unwrap() + i) as u64,
        };
        Ok(self.position)
    }
}
