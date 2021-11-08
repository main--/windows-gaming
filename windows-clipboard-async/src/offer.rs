use std::{ffi::OsString, io::{BufRead, Read, Seek, SeekFrom}, os::windows::prelude::OsStringExt};
use windows::{Win32::{Foundation::{HANDLE, HWND}, System::SystemServices::CLIPBOARD_FORMATS}};
use thiserror::Error;

use crate::{format, raw};

/// An application is offering data on the clipboard.
///
/// # Remarks
///
/// While this type may be cloned and moved arbitrarily, it is only useful as long as the
/// clipboard contents have not changed.
/// Once the clipboard contents change, it is no longer possible to receive contents through this offer.
#[derive(Debug, Clone)]
pub struct ClipboardOffer {
    sequence: u32,
    formats: Vec<CLIPBOARD_FORMATS>,
}

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    #[error("The clipboard offer has expired.")]
    ClipboardOfferExpired,
    #[error("The requested format is not available.")]
    FormatNotAvailable,
    #[error(transparent)]
    Windows(#[from] windows::runtime::Error),
    #[cfg(feature = "image")]
    #[error(transparent)]
    Image(#[from] image::ImageError),
}
type Result<T> = std::result::Result<T, Error>;

impl ClipboardOffer {
    pub(crate) fn new(sequence: u32, formats: Vec<CLIPBOARD_FORMATS>) -> Self {
        ClipboardOffer { sequence, formats }
    }
    pub fn formats(&self) -> impl Iterator<Item=CLIPBOARD_FORMATS> + '_ {
        self.formats.iter().copied()
    }
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
    pub fn receive_bytes(&self, format: CLIPBOARD_FORMATS) -> Result<raw::GlobalMemory> {
        Ok(raw::GlobalMemory::try_from(self.receive_handle(format)?)?)
    }
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
            bufs: &[b"BM", &[0; 8], &u32::try_from(mem::size_of::<BITMAPV5HEADER>()).unwrap().to_le_bytes(), &*mem],
            position: 0,
        };

        Ok(image::load(multi, ImageFormat::Bmp)?)
    }
    pub fn receive_string(&self) -> Result<String> {
        let memory = self.receive_bytes(format::CF_UNICODETEXT)?;
        unsafe {
            let (_, stringmem, _) = memory.align_to::<u16>();
            let str = OsString::from_wide(stringmem);
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
