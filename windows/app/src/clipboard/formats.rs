//! Registered clipboard formats and the open/close guard.

use std::sync::OnceLock;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

/// Standard format ids used by the daemon.
pub const CF_UNICODETEXT: u32 = 13;
/// Packed DIB with the v5 header (supports alpha).
pub const CF_DIBV5: u32 = 17;
/// Packed DIB with the v3 header.
pub const CF_DIB: u32 = 8;

/// Registered (named) formats.
#[derive(Debug, Clone, Copy)]
pub struct Formats {
    /// `PNG`
    pub png: u32,
    /// `ExcludeClipboardContentFromMonitorProcessing`
    pub exclude: u32,
    /// `CanIncludeInClipboardHistory`
    pub can_include_history: u32,
    /// `CanUploadToCloudClipboard`
    pub can_upload: u32,
    /// `Clipboard Viewer Ignore`
    pub viewer_ignore: u32,
    /// `TransClipOrigin`, written by us so the listener can recognise its own writes.
    pub origin: u32,
}

fn register(name: PCWSTR) -> u32 {
    // SAFETY: `name` is a valid NUL-terminated wide string.
    unsafe { RegisterClipboardFormatW(name) }
}

/// The registered formats (registered once).
pub fn formats() -> &'static Formats {
    static FORMATS: OnceLock<Formats> = OnceLock::new();
    FORMATS.get_or_init(|| Formats {
        png: register(w!("PNG")),
        exclude: register(w!("ExcludeClipboardContentFromMonitorProcessing")),
        can_include_history: register(w!("CanIncludeInClipboardHistory")),
        can_upload: register(w!("CanUploadToCloudClipboard")),
        viewer_ignore: register(w!("Clipboard Viewer Ignore")),
        origin: register(w!("TransClipOrigin")),
    })
}

/// The clipboard is open while this value lives.
pub struct OpenClipboardGuard(());

/// Open the clipboard, retrying 10 times with 20 ms pauses (another program may hold it).
pub fn open(owner: Option<HWND>) -> Option<OpenClipboardGuard> {
    for attempt in 0..10 {
        // SAFETY: OpenClipboard has no memory-safety preconditions; the window handle, if any,
        // is only used by the system as the clipboard owner.
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return Some(OpenClipboardGuard(()));
        }
        if attempt < 9 {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    None
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: the guard exists only while this thread has the clipboard open.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Copy the bytes of a clipboard format. Requires the clipboard to be open (`_guard`).
pub fn get_bytes(_guard: &OpenClipboardGuard, format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open. The handle returned for a global-memory format is an
    // HGLOBAL owned by the clipboard; we lock it, copy `GlobalSize` bytes and unlock it.
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let hglobal = HGLOBAL(handle.0);
        let size = GlobalSize(hglobal);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(hglobal);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(hglobal);
        Some(bytes)
    }
}

/// Allocate movable global memory holding `data`. The caller passes it to the clipboard or frees it.
pub fn alloc_global(data: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: the allocation has exactly `data.len()` bytes and is written only while locked.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)).ok()?;
        let ptr = GlobalLock(h);
        if ptr.is_null() {
            let _ = GlobalFree(Some(h));
            return None;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.cast::<u8>(), data.len());
        let _ = GlobalUnlock(h);
        Some(h)
    }
}

/// Hand `h` to the clipboard as `format`. On failure the memory is freed.
pub fn set_global(_guard: &OpenClipboardGuard, format: u32, h: HGLOBAL) -> bool {
    use windows::Win32::System::DataExchange::SetClipboardData;
    // SAFETY: the clipboard is open and `h` is a valid HGLOBAL. On success the clipboard owns
    // it; on failure we still own it and free it.
    unsafe {
        if SetClipboardData(format, Some(HANDLE(h.0))).is_ok() {
            true
        } else {
            let _ = GlobalFree(Some(h));
            false
        }
    }
}
