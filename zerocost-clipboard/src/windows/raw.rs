//! Direct access to the clipboard on the current thread.
//!
//! While this interface tries to paper over them, there are still some footguns
//! if you try hard enough (e.g. leaking a `WindowsClipboard`).
//! This module is used by the crate internally and probably not useful for you.
//! You have been warned.

use std::{ffi::c_void, marker::PhantomData, ops::Deref, thread};

use windows::{runtime::Result, Win32::Foundation::*, Win32::System::DataExchange::*, Win32::System::Memory::*, Win32::System::SystemServices::*, runtime::{Error, Handle}};

/// Zero-sized marker which makes sure that `OpenClipboard` and `CloseClipboard` are called correctly.
///
/// Do not hold this value for extended periods of time. You must not invoke any blocking operations while holding it.
///
/// While it might *technically* be valid to send this value across threads, this crate currently doesn't allow it.
/// Doing so would be a terrible idea regardless.
pub struct WindowsClipboard(PhantomData<*mut ()>);
impl WindowsClipboard {
    /// Open the clipboard with a given window handle (may be zero).
    ///
    /// This function invokes `try_open` until it succeeds.
    /// This is a potential deadlock if an application leaves the clipboard open forever, but
    /// in that case many things will break so it should be fine for most cases.
    pub fn open(window: HWND) -> Self {
        loop {
            if let Some(x) = Self::try_open(window) {
                return x;
            }
            thread::yield_now();
        }
    }

    /// Try to open the clipboard with a given window handle (may be zero).
    pub fn try_open(window: HWND) -> Option<Self> {
        let success = unsafe { OpenClipboard(window) }.as_bool();
        if success {
            Some(WindowsClipboard(PhantomData))
        } else {
            None
        }
    }

    /// Enumerate the formats that are available on the clipboard right now.
    pub fn enum_formats(&self) -> EnumClipboardFormats {
        EnumClipboardFormats {
            current: 0,
            marker: PhantomData
        }
    }

    /// Receive data from the clipboard in a given format as a `HANDLE`.
    pub fn receive(&self, format: CLIPBOARD_FORMATS) -> Result<HANDLE> {
        unsafe { GetClipboardData(format.0) }.ok()
    }
    /// Receive data from the clipboard in a given format as a memory buffer.
    pub fn receive_buffer<'a>(&'a self, format: CLIPBOARD_FORMATS) -> Result<GlobalMemory<'a>> {
        Ok(GlobalMemory::new(self.receive(format)?)?)
    }

    /// Clear the clipboard, taking ownership over it.
    pub fn clear<'a>(&'a mut self) -> Result<WindowsClipboardOwned<'a>> {
        unsafe {
            EmptyClipboard().ok()?;
            Ok(WindowsClipboardOwned(PhantomData))
        }
    }

    /// Query the clipboard sequence number.
    ///
    /// The clipboard sequence number changes whenever the clipboard changes.
    ///
    /// Note: technically, this function does not require the clipboard to be open.
    /// However, using it without opening the clipboard is virtually useless, as that
    /// would still leave you open to TOCTTOU race conditions.
    pub fn sequence_number(&self) -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// Query the clipboard owner.
    pub fn owner(&self) -> HWND {
        unsafe { GetClipboardOwner() }
    }
}
impl Drop for WindowsClipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() }.ok().unwrap();
    }
}
/// Zero-sized marker that represents the state where you are holding the clipboard.
///
/// Produced from `WindowsClipboard::clear`.
pub struct WindowsClipboardOwned<'a>(PhantomData<&'a WindowsClipboard>);
impl WindowsClipboardOwned<'static> {
    /// Obtain this type without having cleared the clipboard.
    ///
    /// This is mostly useful for implementing WM_RENDERFORMAT.
    pub unsafe fn assert() -> Self {
        WindowsClipboardOwned(PhantomData)
    }
}
fn success_is_good(r: Result<()>) -> Result<()> {
    match r {
        Err(e) if e.code() == S_OK => Ok(()),
        x => x,
    }
}
impl<'a> WindowsClipboardOwned<'a> {
    /// Places data on the clipboard in a specified clipboard format.
    pub fn send(&mut self, format: CLIPBOARD_FORMATS, handle: HANDLE) -> Result<()> {
        success_is_good(unsafe { SetClipboardData(format.0, handle).ok().map(|_| ()) })
    }
    /// Places delay-rendered data on the clipboard in a specified clipboard format.
    ///
    /// This is a convenience wrapper for `send(format, HANDLE(0))`.
    pub fn send_delay_rendered(&mut self, format: CLIPBOARD_FORMATS) -> Result<()> {
        self.send(format, HANDLE(0))
    }
}


/// Describes a memory buffer received from the clipboard.
pub struct GlobalMemory<'a> {
    marker: PhantomData<&'a WindowsClipboard>,
    hglobal: HANDLE,
    ptr: *mut c_void,
    size: usize,
}
impl<'a> GlobalMemory<'a> {
    fn new(hglobal: HANDLE) -> Result<Self> {
        unsafe {
            let ptr = GlobalLock(hglobal.0);
            if ptr.is_null() {
                return Err(Error::from_win32());
            }
            let size = GlobalSize(hglobal.0);
            Ok(GlobalMemory { marker: PhantomData, hglobal, ptr, size })
        }
    }

    /// Pointer to the memory buffer.
    pub fn ptr(&self) -> *mut c_void {
        self.ptr
    }
    /// Memory buffer size in bytes.
    pub fn size(&self) -> usize {
        self.size
    }
}

// implementing deref mut is not safe because hglobals are shared
impl<'a> Deref for GlobalMemory<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe {
            std::slice::from_raw_parts(self.ptr as *const u8, self.size)
        }
    }
}

impl<'a> Drop for GlobalMemory<'a> {
    fn drop(&mut self) {
        unsafe { GlobalUnlock(self.hglobal.0) };
    }
}

/// Iterator over formats currently on the clipboard.
pub struct EnumClipboardFormats<'a> {
    current: u32,
    marker: PhantomData<&'a WindowsClipboard>,
}
impl<'a> Iterator for EnumClipboardFormats<'a> {
    type Item = Result<CLIPBOARD_FORMATS>;

    fn next(&mut self) -> Option<Self::Item> {
        self.current = unsafe { EnumClipboardFormats(self.current) };
        if self.current == 0 {
            let error = Error::from_win32();
            if error.code().is_ok() {
                None
            } else {
                Some(Err(error))
            }
        } else {
            Some(Ok(CLIPBOARD_FORMATS::from(self.current)))
        }
    }
}
