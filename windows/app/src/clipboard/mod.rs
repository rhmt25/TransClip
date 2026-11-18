//! Windows clipboard integration (PLAN section 5.3).
//!
//! `WM_CLIPBOARDUPDATE` arrives on the UI thread; changes are debounced by 150 ms and only
//! processed when the clipboard sequence number really changed.

pub mod dib;
pub mod formats;
pub mod read;
pub mod write;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber, RemoveClipboardFormatListener,
};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use engine::clip::Clip;

use std::sync::atomic::{AtomicBool, Ordering};

static ALLOW_TEXT: AtomicBool = AtomicBool::new(true);
static ALLOW_IMAGES: AtomicBool = AtomicBool::new(true);

/// Tell the clipboard layer which kinds the user lets through (the sync toggles).
pub fn set_allowed(text: bool, images: bool) {
    ALLOW_TEXT.store(text, Ordering::Relaxed);
    ALLOW_IMAGES.store(images, Ordering::Relaxed);
}

/// Whether items of the given kind may be shared or applied.
pub fn is_allowed(is_text: bool) -> bool {
    if is_text {
        ALLOW_TEXT.load(Ordering::Relaxed)
    } else {
        ALLOW_IMAGES.load(Ordering::Relaxed)
    }
}

/// Timer id used for the debounce.
pub const DEBOUNCE_TIMER: usize = 0x7C11;
const DEBOUNCE_MS: u32 = 150;

/// Listens for clipboard changes.
#[derive(Default)]
pub struct Watcher {
    last_seq: u32,
}

impl Watcher {
    /// Register for `WM_CLIPBOARDUPDATE`.
    pub fn start(&mut self, hwnd: HWND) -> bool {
        // SAFETY: `hwnd` is our live window.
        let ok = unsafe { AddClipboardFormatListener(hwnd) }.is_ok();
        // SAFETY: no preconditions.
        self.last_seq = unsafe { GetClipboardSequenceNumber() };
        ok
    }

    /// Unregister.
    pub fn stop(&mut self, hwnd: HWND) {
        // SAFETY: `hwnd` is our window; failure (not registered) is harmless.
        unsafe {
            let _ = RemoveClipboardFormatListener(hwnd);
        }
    }

    /// `WM_CLIPBOARDUPDATE` received: (re)start the debounce timer.
    pub fn on_update(&mut self, hwnd: HWND) {
        // SAFETY: `hwnd` is our window; SetTimer with the same id restarts the timer.
        unsafe {
            SetTimer(Some(hwnd), DEBOUNCE_TIMER, DEBOUNCE_MS, None);
        }
    }

    /// The debounce timer fired: read the clipboard if it changed. Returns the item to share.
    pub fn on_timer(&mut self, hwnd: HWND) -> Option<Clip> {
        // SAFETY: `hwnd` is our window and DEBOUNCE_TIMER was created by `on_update`.
        unsafe {
            let _ = KillTimer(Some(hwnd), DEBOUNCE_TIMER);
        }
        // SAFETY: no preconditions.
        let seq = unsafe { GetClipboardSequenceNumber() };
        if seq == self.last_seq {
            return None;
        }
        self.last_seq = seq;
        match read::read_filtered(
            ALLOW_TEXT.load(Ordering::Relaxed),
            ALLOW_IMAGES.load(Ordering::Relaxed),
        ) {
            read::Read::Clip(c) => Some(c),
            read::Read::Skip(why) => {
                tracing::debug!(reason = why, "clipboard change not shared");
                None
            }
            read::Read::Nothing => None,
        }
    }
}
