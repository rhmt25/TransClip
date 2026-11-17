//! Reading the clipboard and deciding whether an item may be shared.

use engine::clip::Clip;
use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;

use super::dib;
use super::formats::{self, CF_DIB, CF_DIBV5, CF_UNICODETEXT};

/// Result of looking at the clipboard.
#[derive(Debug, PartialEq, Eq)]
pub enum Read {
    /// Something to send.
    Clip(Clip),
    /// Deliberately ignored; the reason is for debug logging only.
    Skip(&'static str),
    /// Nothing we handle (files, empty, ...).
    Nothing,
}

fn available(format: u32) -> bool {
    // SAFETY: no preconditions; the call only queries the format list.
    unsafe { IsClipboardFormatAvailable(format).is_ok() }
}

/// Decode a DWORD flag value such as `CanIncludeInClipboardHistory`.
fn dword_is_zero(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 0
}

/// Decode CF_UNICODETEXT bytes (UTF-16LE, NUL terminated) into a string.
pub fn text_from_wide_bytes(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Read the current clipboard content.
pub fn read() -> Read {
    let f = formats::formats();
    // Cheap checks first, without opening the clipboard.
    if available(f.origin) {
        return Read::Skip("written by TransClip");
    }
    if available(f.exclude) || available(f.viewer_ignore) {
        return Read::Skip("marked as sensitive");
    }
    let has_image = available(f.png) || available(CF_DIBV5) || available(CF_DIB);
    let has_text = available(CF_UNICODETEXT);
    if !has_image && !has_text {
        return Read::Nothing;
    }
    let Some(guard) = formats::open(None) else {
        tracing::warn!("clipboard busy, skipping this change");
        return Read::Skip("clipboard busy");
    };
    if available(f.can_include_history)
        && formats::get_bytes(&guard, f.can_include_history).is_some_and(|b| dword_is_zero(&b))
    {
        return Read::Skip("excluded from clipboard history");
    }
    if has_image {
        if let Some(png) = formats::get_bytes(&guard, f.png) {
            if let Ok(img) = dib::decode_image(&png) {
                return Read::Clip(Clip::Image {
                    mime: "image/png".into(),
                    w: img.w,
                    h: img.h,
                    bytes: png,
                });
            }
        }
        for format in [CF_DIBV5, CF_DIB] {
            if let Some(data) = formats::get_bytes(&guard, format) {
                match dib::dib_to_png(&data) {
                    Ok((png, w, h)) => {
                        return Read::Clip(Clip::Image {
                            mime: "image/png".into(),
                            w,
                            h,
                            bytes: png,
                        });
                    }
                    Err(e) => tracing::debug!(error = %e, "bitmap not convertible"),
                }
            }
        }
    }
    if has_text {
        if let Some(bytes) = formats::get_bytes(&guard, CF_UNICODETEXT) {
            let text = text_from_wide_bytes(&bytes);
            if !text.is_empty() {
                return Read::Clip(Clip::Text(text));
            }
        }
    }
    Read::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_text_stops_at_the_terminator() {
        let mut bytes = Vec::new();
        for u in "Halo \u{1f600}".encode_utf16().chain([0, 0x41]) {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(text_from_wide_bytes(&bytes), "Halo \u{1f600}");
        assert_eq!(text_from_wide_bytes(&[]), "");
    }

    #[test]
    fn history_flag_zero_means_sensitive() {
        assert!(dword_is_zero(&[0, 0, 0, 0]));
        assert!(!dword_is_zero(&[1, 0, 0, 0]));
        assert!(!dword_is_zero(&[0]));
    }
}
