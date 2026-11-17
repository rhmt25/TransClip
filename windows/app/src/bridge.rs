//! Two-way channel between the UI thread and the engine runtime (PLAN section 5.1).
//!
//! UI to engine: an unbounded tokio channel (sending never blocks the UI thread).
//! Engine to UI: a std channel plus `PostMessageW` to wake the message loop.

use std::ffi::c_void;
use std::sync::mpsc;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Posted to the UI window when `ToUi` messages are waiting.
pub const WM_BRIDGE: u32 = WM_APP + 1;

/// What the tray shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Status {
    /// Number of peers with a link.
    pub online: usize,
    /// A group key exists.
    pub paired: bool,
    /// Sync is paused.
    pub paused: bool,
    /// Something needs attention (icon turns red).
    pub error: bool,
}

/// Engine to UI.
#[derive(Debug)]
pub enum ToUi {
    /// New status for the tray.
    Status(Status),
    /// Show a balloon notification.
    Notify {
        /// Short title.
        title: String,
        /// Body text.
        text: String,
    },
    /// Close the UI and leave the message loop.
    Quit,
}

/// UI to engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToEngine {
    /// Pause or resume synchronisation.
    SetPaused(bool),
    /// The user wants the settings window.
    ShowSettings,
    /// The user wants the pairing screen.
    Pair,
    /// Open the receive folder.
    OpenDownloads,
    /// The user wants to pick files to send.
    SendFiles,
    /// Shut down.
    Quit,
}

/// Cloneable handle the engine side uses to talk to the UI thread.
#[derive(Clone)]
pub struct UiHandle {
    hwnd: isize,
    tx: mpsc::Sender<ToUi>,
}

impl UiHandle {
    pub(crate) fn new(hwnd: HWND, tx: mpsc::Sender<ToUi>) -> Self {
        Self {
            hwnd: hwnd.0 as isize,
            tx,
        }
    }

    /// Queue a message and wake the UI thread.
    pub fn send(&self, message: ToUi) {
        if self.tx.send(message).is_err() {
            return; // the UI is gone
        }
        // SAFETY: PostMessageW is safe to call from any thread with any window handle; a stale
        // handle only makes it fail, which is ignored because the UI is shutting down.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as *mut c_void)),
                WM_BRIDGE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}
