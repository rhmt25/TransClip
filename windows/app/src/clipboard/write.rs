//! Writing received items to the clipboard.

use engine::clip::{Clip, ClipboardSink, RemoteClip};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::EmptyClipboard;

use super::dib;
use super::formats::{self, CF_DIBV5, CF_UNICODETEXT};

/// Encode text as CF_UNICODETEXT (UTF-16LE with terminator).
pub fn wide_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Writes items from other devices into the OS clipboard.
pub struct WindowsSink {
    /// The UI window; the clipboard owner must be a real window or `SetClipboardData` fails.
    hwnd: isize,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl WindowsSink {
    /// Create a sink that refuses items while `paused` is set.
    pub fn new(hwnd: isize, paused: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self { hwnd, paused }
    }
}

impl ClipboardSink for WindowsSink {
    fn apply(&self, item: RemoteClip) -> Result<(), String> {
        if self.paused.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("paused".into());
        }
        let owner = HWND(self.hwnd as *mut std::ffi::c_void);
        // Convert before taking the clipboard so it is held as briefly as possible.
        let payload = match &item.clip {
            Clip::Text(t) => Payload::Text(wide_bytes(t)),
            Clip::Image { mime, bytes, .. } => {
                let rgba = dib::decode_image(bytes).map_err(|e| e.to_string())?;
                let png = if mime == "image/png" {
                    bytes.clone()
                } else {
                    dib::rgba_to_png(&rgba).map_err(|e| e.to_string())?
                };
                Payload::Image {
                    png,
                    dibv5: dib::rgba_to_dibv5(&rgba),
                }
            }
        };
        let f = formats::formats();
        let guard = formats::open(Some(owner)).ok_or("clipboard busy")?;
        // SAFETY: the clipboard is open and owned by our window.
        unsafe {
            EmptyClipboard().map_err(|e| e.to_string())?;
        }
        let mut ok = match payload {
            Payload::Text(w) => set(&guard, CF_UNICODETEXT, &w),
            Payload::Image { png, dibv5 } => {
                set(&guard, f.png, &png) & set(&guard, CF_DIBV5, &dibv5)
            }
        };
        // Keep the item out of the cloud clipboard but let it into Win+V history.
        ok &= set(&guard, f.can_upload, &0u32.to_le_bytes());
        ok &= set(&guard, f.origin, item.meta.id.as_bytes());
        if ok {
            Ok(())
        } else {
            Err("could not write clipboard data".into())
        }
    }
}

enum Payload {
    Text(Vec<u8>),
    Image { png: Vec<u8>, dibv5: Vec<u8> },
}

fn set(guard: &formats::OpenClipboardGuard, format: u32, data: &[u8]) -> bool {
    formats::alloc_global(data).is_some_and(|h| formats::set_global(guard, format, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_window() -> isize {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, WINDOW_EX_STYLE, WS_POPUP};
        // SAFETY: creates a hidden window of the built-in STATIC class for the test.
        let h = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        };
        h.unwrap().0 as isize
    }

    fn item(clip: Clip) -> RemoteClip {
        RemoteClip {
            meta: engine::clip::ItemMeta {
                id: "00000000-0000-4000-8000-000000000001".into(),
                origin: proto::ids::DeviceId([9; 16]),
                ts: 1,
                hash: clip.hash(),
            },
            clip,
        }
    }

    /// Overwrites the real clipboard, so it only runs on request:
    /// `cargo test -p transclip -- --ignored real_clipboard`.
    #[test]
    #[ignore = "modifies the user's clipboard"]
    fn real_clipboard_round_trip_with_echo_marker() {
        let sink = WindowsSink::new(test_window(), Default::default());
        sink.apply(item(Clip::Text("Halo \u{1f600} dunia".into())))
            .unwrap();
        // Our own write must be recognised and skipped by the listener.
        assert_eq!(
            super::super::read::read(),
            super::super::read::Read::Skip("written by TransClip")
        );
        let g = formats::open(None).unwrap();
        let bytes = formats::get_bytes(&g, CF_UNICODETEXT).unwrap();
        assert_eq!(
            super::super::read::text_from_wide_bytes(&bytes),
            "Halo \u{1f600} dunia"
        );
        drop(g);

        let img = dib::Rgba {
            w: 3,
            h: 2,
            pixels: (0..24u8)
                .map(|i| i.wrapping_mul(41).wrapping_add(7))
                .collect(),
        };
        let png = dib::rgba_to_png(&img).unwrap();
        sink.apply(item(Clip::Image {
            mime: "image/png".into(),
            w: 3,
            h: 2,
            bytes: png.clone(),
        }))
        .unwrap();
        let g = formats::open(None).unwrap();
        assert_eq!(formats::get_bytes(&g, formats::formats().png).unwrap(), png);
        let v5 = formats::get_bytes(&g, CF_DIBV5).unwrap();
        assert_eq!(dib::dib_to_rgba(&v5).unwrap(), img);
    }

    #[test]
    fn paused_sink_refuses() {
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let sink = WindowsSink::new(0, paused);
        assert_eq!(
            sink.apply(item(Clip::Text("x".into()))),
            Err("paused".into())
        );
    }

    #[test]
    fn text_is_utf16_with_terminator() {
        assert_eq!(wide_bytes("A"), [0x41, 0, 0, 0]);
        assert_eq!(wide_bytes(""), [0, 0]);
    }
}
