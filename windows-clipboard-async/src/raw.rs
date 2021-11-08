use std::{ffi::c_void, marker::PhantomData, ops::Deref, thread};

use windows::{runtime::Result, Win32::Foundation::*, Win32::System::DataExchange::*, Win32::System::Memory::*, Win32::System::SystemServices::*, runtime::{Error, Handle}};

use crate::offer::ClipboardOffer;

pub struct WindowsClipboard(());
impl WindowsClipboard {
    pub fn open(window: HWND) -> Self {
        loop {
            if let Some(x) = Self::try_open(window) {
                return x;
            }
            thread::yield_now();
        }
    }
    pub fn try_open(window: HWND) -> Option<Self> {
        let success = unsafe { OpenClipboard(window) }.as_bool();
        if success {
            Some(WindowsClipboard(()))
        } else {
            None
        }
    }
    pub fn enum_formats(&self) -> EnumClipboardFormats {
        EnumClipboardFormats {
            current: 0,
            marker: PhantomData
        }
    }
    pub fn receive(&self, format: CLIPBOARD_FORMATS) -> Result<HANDLE> {
        unsafe { GetClipboardData(format.0) }.ok()
    }
    pub fn clear(&self) -> Result<()> {
        unsafe { EmptyClipboard() }.ok()
    }
    pub fn sequence_number(&self) -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }
}
impl Drop for WindowsClipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() }.ok().unwrap();
    }
}

pub struct GlobalMemory {
    hglobal: HANDLE,
    ptr: *mut c_void,
    size: usize,
}
impl TryFrom<HANDLE> for GlobalMemory {
    type Error = Error;

    fn try_from(hglobal: HANDLE) -> Result<Self> {
        unsafe {
            let ptr = GlobalLock(hglobal.0);
            if ptr.is_null() {
                return Err(Error::from_win32());
            }
            let size = GlobalSize(hglobal.0);
            Ok(GlobalMemory { hglobal, ptr, size })
        }
    }
}
impl GlobalMemory {
    pub fn ptr(&self) -> *mut c_void {
        self.ptr
    }
    pub fn size(&self) -> usize {
        self.size
    }
}
// implementing deref mut is not safe because hglobals are shared
impl Deref for GlobalMemory {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        unsafe {
            std::slice::from_raw_parts(self.ptr as *const u8, self.size)
        }
    }
}
impl Drop for GlobalMemory {
    fn drop(&mut self) {
        unsafe { GlobalUnlock(self.hglobal.0) }.ok().unwrap();
    }
}

pub struct EnumClipboardFormats<'a> {
    current: u32,
    marker: PhantomData<*mut &'a WindowsClipboard>,
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

pub fn read_clipboard(window: HWND) -> Result<Option<ClipboardOffer>> {
    let clipboard = WindowsClipboard::open(window);
    let formats: Vec<_> = clipboard.enum_formats().collect::<Result<_>>()?;
    let offer = if formats.is_empty() {
        None
    } else {
        let sequence = clipboard.sequence_number();
        Some(ClipboardOffer::new(sequence, formats))
    };
    Ok(offer)
}
