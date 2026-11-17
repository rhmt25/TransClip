//! The UI thread: a hidden top-level window, the tray icon and the Win32 message loop.
//!
//! A top-level (not message-only) window is required so the window receives the
//! `TaskbarCreated` broadcast after Explorer restarts.

use std::sync::mpsc;

use anyhow::{anyhow, Result};
use tokio::sync::mpsc::UnboundedSender;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW,
    SetWindowLongPtrW, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_COMMAND, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY,
    WM_LBUTTONDBLCLK, WM_NCCREATE, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
};

use crate::bridge::{Status, ToEngine, ToUi, UiHandle, WM_BRIDGE};
use crate::tray::{
    Tray, CMD_DOWNLOADS, CMD_PAIR, CMD_PAUSE, CMD_QUIT, CMD_SEND, CMD_SETTINGS, WM_TRAY,
};

struct State {
    tray: Tray,
    engine: UnboundedSender<ToEngine>,
    inbox: mpsc::Receiver<ToUi>,
    status: Status,
    taskbar_created: u32,
    clip: crate::clipboard::Watcher,
}

impl State {
    fn drain(&mut self, hwnd: HWND) {
        while let Ok(msg) = self.inbox.try_recv() {
            match msg {
                ToUi::Status(s) => {
                    self.status = s;
                    self.tray.update(&s);
                }
                ToUi::Notify { title, text } => self.tray.notify(&self.status, &title, &text),
                ToUi::Quit => {
                    // SAFETY: `hwnd` is our own window, destroyed on the thread that created it.
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
        }
    }

    fn command(&mut self, hwnd: HWND, id: usize) {
        let send = |m| {
            let _ = self.engine.send(m);
        };
        match id {
            CMD_SEND => send(ToEngine::SendFiles),
            CMD_PAUSE => {
                self.status.paused = !self.status.paused;
                self.tray.update(&self.status);
                send(ToEngine::SetPaused(self.status.paused));
            }
            CMD_SETTINGS => send(ToEngine::ShowSettings),
            CMD_PAIR => send(ToEngine::Pair),
            CMD_DOWNLOADS => send(ToEngine::OpenDownloads),
            CMD_QUIT => {
                send(ToEngine::Quit);
                // SAFETY: as in `drain`.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            _ => {}
        }
    }
}

/// # Safety
/// Called by the system with a valid window handle on the thread that created the window.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for WM_NCCREATE lparam points to a CREATESTRUCTW whose lpCreateParams is the
        // State pointer passed to CreateWindowExW.
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if state.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    // SAFETY: the pointer was stored by WM_NCCREATE and points to a State that outlives the
    // window (it is owned by `run`). Only this thread touches it.
    let state = &mut *state;
    match msg {
        WM_CREATE => LRESULT(0),
        WM_BRIDGE => {
            state.drain(hwnd);
            LRESULT(0)
        }
        WM_TRAY => {
            // With NOTIFYICON_VERSION_4 the event is in the low word of lparam.
            match (lparam.0 & 0xffff) as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => state.tray.show_menu(&state.status),
                WM_LBUTTONDBLCLK => {
                    let _ = state.engine.send(ToEngine::ShowSettings);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            state.command(hwnd, wparam.0 & 0xffff);
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            state.clip.on_update(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == crate::clipboard::DEBOUNCE_TIMER => {
            if let Some(clip) = state.clip.on_timer(hwnd) {
                let _ = state.engine.send(ToEngine::LocalClip(clip));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            state.clip.stop(hwnd);
            state.tray.remove();
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m == state.taskbar_created => {
            state.tray.add(&state.status);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Create the window and tray icon, hand the engine side a [`UiHandle`] through `ready`, and
/// run the message loop until the window is destroyed.
pub fn run(
    engine: UnboundedSender<ToEngine>,
    initial: Status,
    ready: impl FnOnce(UiHandle),
) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    // The pointer given to CreateWindowExW must stay valid until the loop ends.
    let state = Box::new(State {
        tray: Tray::new(HWND::default()),
        engine,
        inbox: rx,
        status: initial,
        // SAFETY: the string literal is a valid wide string.
        taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        clip: crate::clipboard::Watcher::default(),
    });
    let state_ptr = Box::into_raw(state);

    // SAFETY: standard window class registration and creation. `state_ptr` stays alive until it
    // is reclaimed with Box::from_raw below, after the loop has ended and the window is gone.
    let hwnd = unsafe {
        let hinst = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst.into(),
            lpszClassName: w!("TransClipUiWindow"),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            drop(Box::from_raw(state_ptr));
            return Err(anyhow!("cannot register window class"));
        }
        match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("TransClipUiWindow"),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            Some(state_ptr.cast()),
        ) {
            Ok(h) => h,
            Err(e) => {
                drop(Box::from_raw(state_ptr));
                return Err(e.into());
            }
        }
    };

    // SAFETY: `state_ptr` is valid and only used on this thread.
    unsafe {
        (*state_ptr).tray = Tray::new(hwnd);
        if !(*state_ptr).clip.start(hwnd) {
            tracing::warn!("could not register the clipboard listener");
        }
        if !(*state_ptr).tray.add(&(*state_ptr).status) {
            tracing::warn!("could not add the tray icon (Explorer not ready?)");
        }
    }
    ready(UiHandle::new(hwnd, tx));

    let mut msg = MSG::default();
    // SAFETY: standard message loop on the thread that owns the window.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        drop(Box::from_raw(state_ptr));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn quit_message_ends_the_loop() {
        let (etx, _erx) = tokio::sync::mpsc::unbounded_channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || {
            run(etx, Status::default(), |h| {
                h.send(ToUi::Status(Status {
                    online: 2,
                    paired: true,
                    ..Status::default()
                }));
                h.send(ToUi::Notify {
                    title: "t".into(),
                    text: "x".into(),
                });
                h.send(ToUi::Quit);
            })
            .unwrap();
            let _ = done_tx.send(());
        });
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).is_ok());
        t.join().unwrap();
    }
}
