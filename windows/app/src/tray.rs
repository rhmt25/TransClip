//! Notification-area icon and its context menu (PLAN section 5.5).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, HICON, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::bridge::Status;
use crate::strings;

/// Callback message of the tray icon.
pub const WM_TRAY: u32 = WM_APP;
const ICON_ID: u32 = 1;

/// Menu command ids.
pub const CMD_SEND: usize = 101;
pub const CMD_PAUSE: usize = 102;
pub const CMD_SETTINGS: usize = 103;
pub const CMD_PAIR: usize = 104;
pub const CMD_DOWNLOADS: usize = 105;
pub const CMD_QUIT: usize = 106;

const RES_APP: usize = 1;
const RES_PAUSED: usize = 2;
const RES_ERROR: usize = 3;

/// Which resource icon a status maps to.
pub fn icon_for(status: &Status) -> usize {
    if status.error {
        RES_ERROR
    } else if status.paused {
        RES_PAUSED
    } else {
        RES_APP
    }
}

fn load_icon(resource: usize) -> HICON {
    // SAFETY: the module handle is our own exe; the id refers to an ICON resource embedded by
    // build.rs. A missing resource returns an error and we fall back to a null icon.
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(Some(hinst.into()), PCWSTR(resource as *const u16)).unwrap_or_default()
    }
}

fn copy_wide(dst: &mut [u16], text: &str) {
    let mut n = 0;
    for unit in text.encode_utf16().take(dst.len().saturating_sub(1)) {
        dst[n] = unit;
        n += 1;
    }
    dst[n] = 0;
}

/// The icon in the notification area.
pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    /// Create a tray object for `hwnd`; call [`Tray::add`] to show the icon.
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd }
    }

    fn data(&self, status: &Status) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: load_icon(icon_for(status)),
            ..Default::default()
        };
        let tip = format!(
            "{} \u{2014} {}",
            strings::APP_NAME,
            strings::status_line(status.online, status.paired, status.paused)
        );
        copy_wide(&mut nid.szTip, &tip);
        nid
    }

    /// Add the icon. Also used again after Explorer restarts (`TaskbarCreated`).
    pub fn add(&self, status: &Status) -> bool {
        let nid = self.data(status);
        // SAFETY: `nid` is fully initialised and lives for both calls.
        unsafe {
            let mut nid = nid;
            let ok = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
            ok
        }
    }

    /// Update icon and tooltip.
    pub fn update(&self, status: &Status) {
        let nid = self.data(status);
        // SAFETY: `nid` is fully initialised.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Show a balloon notification (a toast on Windows 11).
    pub fn notify(&self, status: &Status, title: &str, text: &str) {
        let mut nid = self.data(status);
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut nid.szInfoTitle, title);
        copy_wide(&mut nid.szInfo, text);
        // SAFETY: `nid` is fully initialised.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Remove the icon.
    pub fn remove(&self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        // SAFETY: `nid` identifies our icon and is fully initialised.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    /// Show the context menu at the cursor.
    pub fn show_menu(&self, status: &Status) {
        // SAFETY: all handles come from the calls below and are used on the UI thread only; the
        // menu is destroyed before returning. The strings passed to AppendMenuW are copied by
        // the system.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let line = windows::core::HSTRING::from(strings::status_line(
                status.online,
                status.paired,
                status.paused,
            ));
            let _ = AppendMenuW(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, &line);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let item = |id: usize, text: &str, flags| {
                let _ = AppendMenuW(
                    menu,
                    MF_STRING | flags,
                    id,
                    &windows::core::HSTRING::from(text),
                );
            };
            let none = windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS(0);
            let send_flags = if status.paired { none } else { MF_GRAYED };
            item(CMD_SEND, strings::MENU_SEND_FILE, send_flags);
            item(
                CMD_PAUSE,
                strings::MENU_PAUSE,
                if status.paused { MF_CHECKED } else { none },
            );
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            item(CMD_SETTINGS, strings::MENU_SETTINGS, none);
            item(CMD_PAIR, strings::MENU_PAIR, none);
            item(CMD_DOWNLOADS, strings::MENU_OPEN_DOWNLOADS, none);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            item(CMD_QUIT, strings::MENU_QUIT, none);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // The window must be foreground or the menu will not close when clicking elsewhere.
            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                Some(0),
                self.hwnd,
                None,
            );
            let _ = PostMessageW(
                Some(self.hwnd),
                WM_NULL,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
            let _ = DestroyMenu(menu);
        }
        let _ = w!("");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_follows_status() {
        let ok = Status::default();
        assert_eq!(icon_for(&ok), RES_APP);
        assert_eq!(icon_for(&Status { paused: true, ..ok }), RES_PAUSED);
        assert_eq!(
            icon_for(&Status {
                paused: true,
                error: true,
                ..ok
            }),
            RES_ERROR
        );
    }

    #[test]
    fn wide_copy_truncates_and_terminates() {
        let mut buf = [1u16; 4];
        copy_wide(&mut buf, "abcdef");
        assert_eq!(buf, [97, 98, 99, 0]);
    }
}
