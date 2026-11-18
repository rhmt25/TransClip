//! Radial drop menu (PLAN section 5.4): hold the left mouse button, drag, press the trigger key
//! (default Right Ctrl) and a ring of devices appears under the cursor; drop files on a segment
//! to send them there.
//!
//! Deviation from the plan: the trigger is detected by polling `GetAsyncKeyState` from a timer
//! on the UI thread instead of low-level hooks. It has the same effect, cannot slow down input
//! for the whole desktop, and needs no hook callbacks (DECISIONS D-014).

mod drop_target;
pub mod geometry;
pub mod keys;
pub mod render;

use std::sync::atomic::{AtomicI32, AtomicIsize, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_VCENTER, HDC, TRANSPARENT,
};
use windows::Win32::System::Ole::{OleInitialize, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics,
    RegisterClassExW, SetWindowPos, ShowWindow, UpdateLayeredWindow, HWND_TOPMOST, SM_CXDRAG,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_APP,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::bridge::ToEngine;
use drop_target::DropTarget;
use geometry::segment_centre_deg;
use proto::ids::DeviceId;
use render::Layout;

/// The hovered segment changed: redraw.
pub const WM_RADIAL_HOVER: u32 = WM_APP + 2;
/// A drop finished: hide the overlay.
pub const WM_RADIAL_END: u32 = WM_APP + 3;
/// Timer that polls the mouse and trigger key.
pub const RADIAL_TIMER: usize = 0x7261;
/// Poll interval in milliseconds.
pub const POLL_MS: u32 = 40;

/// One segment of the ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// "Kirim ke Semua".
    All,
    /// One online device.
    Peer {
        /// Device id.
        id: DeviceId,
        /// Display name.
        name: String,
    },
}

impl Target {
    fn label(&self) -> String {
        match self {
            Target::All => "Semua".into(),
            Target::Peer { name, .. } => label_for(name),
        }
    }
}

/// Names are cut to 14 characters (PLAN section 5.4).
pub fn label_for(name: &str) -> String {
    let mut chars = name.chars();
    let head: String = chars.by_ref().take(14).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

/// What the overlay currently shows.
pub struct Active {
    pub layout: Layout,
    pub centre_screen: (i32, i32),
    pub targets: Vec<Target>,
}

/// State shared with the COM drop target.
pub struct Shared {
    pub layout: Mutex<Option<Active>>,
    /// -2 nothing, -1 cancel disc, otherwise the segment index.
    pub hover: AtomicI32,
    pub tx: UnboundedSender<ToEngine>,
    pub ui_hwnd: AtomicIsize,
    /// Number of dragged items, learned when the drag enters the overlay.
    pub items: AtomicUsize,
}

/// Controller owned by the UI thread.
pub struct Radial {
    shared: Arc<Shared>,
    overlay: HWND,
    peers: Vec<(DeviceId, String)>,
    trigger_vks: Vec<i32>,
    radius: u32,
    press: Option<POINT>,
    shown: bool,
    idle_ticks: u32,
    last_hover: i32,
    ready: bool,
}

#[allow(clippy::unnecessary_wraps)]
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn key_down(vk: i32) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

impl Radial {
    /// Create the controller; call [`Radial::init`] on the UI thread afterwards.
    pub fn new(tx: UnboundedSender<ToEngine>) -> Self {
        Self {
            shared: Arc::new(Shared {
                layout: Mutex::new(None),
                hover: AtomicI32::new(-2),
                tx,
                ui_hwnd: AtomicIsize::new(0),
                items: AtomicUsize::new(0),
            }),
            overlay: HWND::default(),
            peers: Vec::new(),
            trigger_vks: keys::vks_or_default(keys::DEFAULT),
            radius: 140,
            press: None,
            shown: false,
            idle_ticks: 0,
            last_hover: -2,
            ready: false,
        }
    }

    /// Create the hidden overlay window and register it as a drop target.
    pub fn init(&mut self, ui_hwnd: HWND) {
        self.shared
            .ui_hwnd
            .store(ui_hwnd.0 as isize, Ordering::Relaxed);
        // SAFETY: plain Win32/OLE initialisation on the UI thread; every failure disables the
        // feature instead of continuing with a half-built window.
        unsafe {
            if OleInitialize(None).is_err() {
                tracing::warn!("radial menu disabled: OLE could not be initialised");
                return;
            }
            let Ok(hinst) = windows::Win32::System::LibraryLoader::GetModuleHandleW(None) else {
                return;
            };
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: hinst.into(),
                lpszClassName: w!("TransClipRadial"),
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                w!("TransClipRadial"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinst.into()),
                None,
            ) else {
                return;
            };
            let target: windows::Win32::System::Ole::IDropTarget =
                DropTarget::new(Arc::clone(&self.shared)).into();
            if RegisterDragDrop(hwnd, &target).is_err() {
                tracing::warn!("radial menu disabled: cannot register the drop target");
                let _ = DestroyWindow(hwnd);
                return;
            }
            self.overlay = hwnd;
            self.ready = true;
        }
    }

    /// Release the overlay.
    pub fn shutdown(&mut self) {
        if !self.ready {
            return;
        }
        // SAFETY: the window was created by `init` on this thread.
        unsafe {
            let _ = RevokeDragDrop(self.overlay);
            let _ = DestroyWindow(self.overlay);
        }
        self.ready = false;
    }

    /// Online devices that become segments.
    pub fn set_peers(&mut self, peers: Vec<(DeviceId, String)>) {
        self.peers = peers;
    }

    /// Trigger key name and ring radius from the settings.
    pub fn set_config(&mut self, trigger: &str, radius: u32) {
        self.trigger_vks = keys::vks_or_default(trigger);
        self.radius = radius.clamp(60, 400);
    }

    fn targets(&self) -> Vec<Target> {
        if self.peers.is_empty() {
            return Vec::new();
        }
        let mut t: Vec<Target> = self
            .peers
            .iter()
            .map(|(id, name)| Target::Peer {
                id: *id,
                name: name.clone(),
            })
            .collect();
        t.push(Target::All);
        t
    }

    /// Called by the timer every [`POLL_MS`].
    pub fn tick(&mut self) {
        if !self.ready {
            return;
        }
        let left = key_down(i32::from(VK_LBUTTON.0));
        let mut cursor = POINT::default();
        // SAFETY: `cursor` is a valid out pointer.
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        if self.shown {
            if key_down(i32::from(VK_ESCAPE.0)) {
                self.hide();
                return;
            }
            // After the button is released the source application delivers the drop; give it a
            // moment before the overlay disappears.
            if left {
                self.idle_ticks = 0;
            } else {
                self.idle_ticks += 1;
                if self.idle_ticks * POLL_MS > 300 {
                    self.hide();
                }
            }
            return;
        }
        if !left {
            self.press = None;
            return;
        }
        let Some(start) = self.press else {
            self.press = Some(cursor);
            return;
        };
        // SAFETY: GetSystemMetrics has no preconditions.
        let slop = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
        let moved = (cursor.x - start.x).abs().max((cursor.y - start.y).abs()) > slop;
        if moved && self.trigger_vks.iter().any(|vk| key_down(*vk)) {
            self.show(cursor);
        }
    }

    fn show(&mut self, cursor: POINT) {
        let targets = self.targets();
        if targets.is_empty() {
            return;
        }
        // SAFETY: GetDpiForSystem has no preconditions.
        let scale = f64::from(unsafe { GetDpiForSystem() }) / 96.0;
        let layout = Layout::new(f64::from(self.radius) * scale, scale);
        let half = layout.size as i32 / 2;
        *self
            .shared
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Active {
            layout,
            centre_screen: (cursor.x, cursor.y),
            targets,
        });
        self.shared.hover.store(-2, Ordering::Relaxed);
        self.shared.items.store(0, Ordering::Relaxed);
        self.last_hover = -2;
        self.idle_ticks = 0;
        self.draw(POINT {
            x: cursor.x - half,
            y: cursor.y - half,
        });
        // SAFETY: `overlay` is our window. Showing without activation keeps the drag source focused.
        unsafe {
            let _ = ShowWindow(self.overlay, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                self.overlay,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        self.shown = true;
    }

    /// Hide the overlay.
    pub fn hide(&mut self) {
        if self.shown {
            // SAFETY: `overlay` is our window.
            unsafe {
                let _ = ShowWindow(self.overlay, SW_HIDE);
            }
        }
        self.shown = false;
        self.press = None;
        *self
            .shared
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.shared.hover.store(-2, Ordering::Relaxed);
    }

    /// The hovered segment changed.
    pub fn on_hover(&mut self) {
        let h = self.shared.hover.load(Ordering::Relaxed);
        if self.shown && h != self.last_hover {
            self.last_hover = h;
            let origin = {
                let g = self
                    .shared
                    .layout
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                g.as_ref().map(|a| {
                    let half = a.layout.size as i32 / 2;
                    POINT {
                        x: a.centre_screen.0 - half,
                        y: a.centre_screen.1 - half,
                    }
                })
            };
            if let Some(p) = origin {
                self.draw(p);
            }
        }
    }

    /// Render the ring, draw the labels with GDI and push the bitmap to the layered window.
    fn draw(&self, top_left: POINT) {
        let guard = self
            .shared
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(active) = guard.as_ref() else { return };
        let layout = active.layout;
        let n = active.targets.len();
        let hover = self.shared.hover.load(Ordering::Relaxed);
        let highlight = usize::try_from(hover).ok().filter(|i| *i < n);
        let pixels = render::render(&layout, n, highlight, hover == -1);
        let size = layout.size as i32;

        // SAFETY: standard GDI sequence. Every object created here is deselected and deleted
        // before returning; `bits` points into the DIB section and is only written while the
        // bitmap lives, within `size * size * 4` bytes.
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return;
            };
            if bits.is_null() {
                let _ = DeleteObject(bmp.into());
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return;
            }
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
            let old = SelectObject(mem, bmp.into());

            draw_labels(
                mem,
                &layout,
                &active.targets,
                hover,
                self.shared.items.load(Ordering::Relaxed),
            );

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let size_s = SIZE { cx: size, cy: size };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                self.overlay,
                Some(screen),
                Some(&top_left),
                Some(&size_s),
                Some(mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
        }
    }
}

/// Draw the segment names and the centre text with GDI onto the premultiplied bitmap.
///
/// # Safety
/// `mem` must be a memory DC with the ring bitmap selected into it.
unsafe fn draw_labels(mem: HDC, layout: &Layout, targets: &[Target], hover: i32, items: usize) {
    let scale = layout.corner / 10.0;
    let font = |px: f64, weight: i32| {
        CreateFontW(
            (px * scale).round() as i32,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            Default::default(),
            Default::default(),
            Default::default(),
            0,
            w!("Segoe UI"),
        )
    };
    let draw =
        |text: &str, rect: &mut RECT, flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT| {
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            DrawTextW(mem, &mut wide, rect, flags | DT_NOPREFIX);
        };
    let name_font = font(15.5, 600);
    let old_font = SelectObject(mem, name_font.into());
    SetBkMode(mem, TRANSPARENT);
    let n = targets.len();
    let r_mid = layout.label_radius();
    let half_w = ((layout.outer - layout.inner) * 0.9).max(64.0 * scale);
    for (i, target) in targets.iter().enumerate() {
        // Premultiplied alpha: no channel may exceed the shape's alpha (>= 224). The highlighted
        // segment is sky blue, so its text is dark.
        let colour = if usize::try_from(hover).ok() == Some(i) {
            COLORREF(0x001A_110B)
        } else {
            COLORREF(0x00DE_DAD6)
        };
        SetTextColor(mem, colour);
        let rad = segment_centre_deg(i, n).to_radians();
        let cx = layout.centre + r_mid * rad.sin();
        let cy = layout.centre - r_mid * rad.cos();
        let mut rect = RECT {
            left: (cx - half_w) as i32,
            top: (cy - 14.0 * scale) as i32,
            right: (cx + half_w) as i32,
            bottom: (cy + 14.0 * scale) as i32,
        };
        draw(
            &target.label(),
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }

    // The centre disc is the cancel drop zone: how many items are being dragged, and the hint.
    let hot = hover == -1;
    let title = match items {
        0 => "Lepas file".to_owned(),
        k => format!("{k} item"),
    };
    SetTextColor(mem, COLORREF(0x00F9_F5F1));
    let big = font(20.0, 700);
    SelectObject(mem, big.into());
    let c = layout.centre;
    let w2 = layout.inner * 0.92;
    let mut title_rect = RECT {
        left: (c - w2) as i32,
        top: (c - layout.inner * 0.62) as i32,
        right: (c + w2) as i32,
        bottom: (c - layout.inner * 0.08) as i32,
    };
    draw(
        &title,
        &mut title_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let _ = DeleteObject(big.into());
    let small = font(12.0, 400);
    SelectObject(mem, small.into());
    SetTextColor(
        mem,
        if hot {
            COLORREF(0x0071_71F8)
        } else {
            COLORREF(0x00B8_A094)
        },
    );
    let mut hint_rect = RECT {
        left: (c - w2) as i32,
        top: (c - layout.inner * 0.04) as i32,
        right: (c + w2) as i32,
        bottom: (c + layout.inner * 0.62) as i32,
    };
    draw(
        "Lepas di sini
untuk batal",
        &mut hint_rect,
        DT_CENTER | DT_TOP,
    );
    SelectObject(mem, old_font);
    let _ = DeleteObject(small.into());
    let _ = DeleteObject(name_font.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_cut_at_fourteen_characters() {
        assert_eq!(label_for("Pixel"), "Pixel");
        assert_eq!(label_for("12345678901234"), "12345678901234");
        assert_eq!(label_for("123456789012345"), "12345678901234…");
        assert_eq!(
            label_for("日本語日本語日本語日本語日本語"),
            "日本語日本語日本語日本語日本…"
        );
    }
}
