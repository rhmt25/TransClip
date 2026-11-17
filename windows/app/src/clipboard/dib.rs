//! Device-independent bitmap (CF_DIB / CF_DIBV5) to and from RGBA and PNG (PLAN section 5.3).
//!
//! Everything here is safe code working on byte slices, so it is covered by plain unit tests.

use image::{ImageEncoder, ImageFormat};

/// Largest accepted side length.
const MAX_SIDE: u32 = 16_384;
/// Largest accepted pixel buffer (RGBA bytes).
const MAX_PIXEL_BYTES: u64 = 256 * 1024 * 1024;

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const LCS_SRGB: u32 = 0x7352_4742;
const LCS_GM_IMAGES: u32 = 4;

/// Why a bitmap could not be converted.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DibError {
    /// Header or pixel data is too short or inconsistent.
    #[error("malformed bitmap")]
    Malformed,
    /// Valid but not handled (palette, RLE, JPEG, ...).
    #[error("unsupported bitmap format")]
    Unsupported,
    /// Dimensions exceed the limits.
    #[error("bitmap too large")]
    TooLarge,
    /// PNG or image codec failure.
    #[error("image codec: {0}")]
    Codec(String),
}

/// 8-bit RGBA, top-down, straight (not premultiplied) alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    /// `w * h * 4` bytes.
    pub pixels: Vec<u8>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

fn i32_at(d: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// A channel mask: where the bits are and how to widen them to 8 bits.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    max: u32,
}

impl Channel {
    fn new(mask: u32) -> Self {
        if mask == 0 {
            return Self {
                mask: 0,
                shift: 0,
                max: 0,
            };
        }
        let shift = mask.trailing_zeros();
        Self {
            mask,
            shift,
            max: mask >> shift,
        }
    }

    fn get(self, px: u32) -> u8 {
        if self.mask == 0 {
            return 0;
        }
        let v = (px & self.mask) >> self.shift;
        // Widen with rounding; max is at most 0xFFFF_FFFF so use u64.
        u8::try_from((u64::from(v) * 255 + u64::from(self.max) / 2) / u64::from(self.max))
            .unwrap_or(255)
    }
}

/// Convert a packed DIB (header, optional masks/palette, pixels) to RGBA.
pub fn dib_to_rgba(dib: &[u8]) -> Result<Rgba, DibError> {
    let header_size = u32_at(dib, 0).ok_or(DibError::Malformed)? as usize;
    if !matches!(header_size, 40 | 52 | 56 | 108 | 124) {
        return Err(DibError::Unsupported);
    }
    let width = i32_at(dib, 4).ok_or(DibError::Malformed)?;
    let height = i32_at(dib, 8).ok_or(DibError::Malformed)?;
    let planes = u16_at(dib, 12).ok_or(DibError::Malformed)?;
    let bpp = u32::from(u16_at(dib, 14).ok_or(DibError::Malformed)?);
    let compression = u32_at(dib, 16).ok_or(DibError::Malformed)?;
    let colors_used = u32_at(dib, 32).ok_or(DibError::Malformed)?;
    if planes != 1 || width <= 0 || height == 0 {
        return Err(DibError::Malformed);
    }
    let top_down = height < 0;
    let w = width.unsigned_abs();
    let h = height.unsigned_abs();
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) * 4 > MAX_PIXEL_BYTES {
        return Err(DibError::TooLarge);
    }
    if !matches!(bpp, 16 | 24 | 32) || !matches!(compression, BI_RGB | BI_BITFIELDS) {
        return Err(DibError::Unsupported);
    }

    // Channel masks.
    let mut masks_len = 0usize;
    let (r, g, b, a) = if compression == BI_BITFIELDS {
        let at = if header_size >= 56 {
            40
        } else {
            masks_len = 12;
            header_size
        };
        let rm = u32_at(dib, at).ok_or(DibError::Malformed)?;
        let gm = u32_at(dib, at + 4).ok_or(DibError::Malformed)?;
        let bm = u32_at(dib, at + 8).ok_or(DibError::Malformed)?;
        let am = if header_size >= 56 {
            u32_at(dib, at + 12).ok_or(DibError::Malformed)?
        } else {
            0
        };
        (rm, gm, bm, am)
    } else {
        match bpp {
            16 => (0x7c00, 0x03e0, 0x001f, 0),
            _ => (0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000),
        }
    };
    let (rc, gc, bc, ac) = (
        Channel::new(r),
        Channel::new(g),
        Channel::new(b),
        Channel::new(a),
    );

    let palette = if bpp <= 8 {
        colors_used as usize * 4
    } else {
        0
    };
    let data_start = header_size + masks_len + palette;
    let stride = (u64::from(w) * u64::from(bpp)).div_ceil(32) * 4;
    let needed = stride * u64::from(h);
    let pixels_in = dib.get(data_start..).ok_or(DibError::Malformed)?;
    if (pixels_in.len() as u64) < needed {
        return Err(DibError::Malformed);
    }
    let stride = stride as usize;
    let bytes_pp = (bpp / 8) as usize;
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];
    let mut any_alpha = false;
    for y in 0..h as usize {
        let src_y = if top_down { y } else { h as usize - 1 - y };
        let row = &pixels_in[src_y * stride..src_y * stride + w as usize * bytes_pp];
        for x in 0..w as usize {
            let px = &row[x * bytes_pp..(x + 1) * bytes_pp];
            let (red, green, blue, alpha) = if bytes_pp == 3 {
                (px[2], px[1], px[0], 255)
            } else {
                let v = if bytes_pp == 2 {
                    u32::from(u16::from_le_bytes([px[0], px[1]]))
                } else {
                    u32::from_le_bytes([px[0], px[1], px[2], px[3]])
                };
                (rc.get(v), gc.get(v), bc.get(v), ac.get(v))
            };
            any_alpha |= alpha != 0;
            let o = (y * w as usize + x) * 4;
            out[o..o + 4].copy_from_slice(&[red, green, blue, alpha]);
        }
    }
    // A bitmap whose alpha channel is entirely zero has no usable alpha: treat it as opaque.
    if !any_alpha || ac.mask == 0 {
        for px in out.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    Ok(Rgba { w, h, pixels: out })
}

/// Encode as PNG (lossless).
pub fn rgba_to_png(img: &Rgba) -> Result<Vec<u8>, DibError> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&img.pixels, img.w, img.h, image::ExtendedColorType::Rgba8)
        .map_err(|e| DibError::Codec(e.to_string()))?;
    Ok(out)
}

/// Decode PNG, JPEG, BMP, ... to RGBA.
pub fn decode_image(bytes: &[u8]) -> Result<Rgba, DibError> {
    let format = image::guess_format(bytes).map_err(|e| DibError::Codec(e.to_string()))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp
    ) {
        return Err(DibError::Unsupported);
    }
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|e| DibError::Codec(e.to_string()))?
        .to_rgba8();
    let (w, h) = decoded.dimensions();
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(DibError::TooLarge);
    }
    Ok(Rgba {
        w,
        h,
        pixels: decoded.into_raw(),
    })
}

/// DIB straight to PNG bytes, returning the dimensions as well.
pub fn dib_to_png(dib: &[u8]) -> Result<(Vec<u8>, u32, u32), DibError> {
    let img = dib_to_rgba(dib)?;
    Ok((rgba_to_png(&img)?, img.w, img.h))
}

/// Build a CF_DIBV5 payload (124-byte header, 32 bpp BGRA with alpha, bottom-up).
pub fn rgba_to_dibv5(img: &Rgba) -> Vec<u8> {
    let (w, h) = (img.w as usize, img.h as usize);
    let size_image = w * h * 4;
    let mut out = Vec::with_capacity(124 + size_image);
    let put32 = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    put32(&mut out, 124); // bV5Size
    put32(&mut out, img.w);
    put32(&mut out, img.h); // positive: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bit count
    put32(&mut out, BI_BITFIELDS);
    put32(&mut out, u32::try_from(size_image).unwrap_or(0));
    put32(&mut out, 0); // x ppm
    put32(&mut out, 0); // y ppm
    put32(&mut out, 0); // colors used
    put32(&mut out, 0); // colors important
    put32(&mut out, 0x00ff_0000); // red mask
    put32(&mut out, 0x0000_ff00); // green mask
    put32(&mut out, 0x0000_00ff); // blue mask
    put32(&mut out, 0xff00_0000); // alpha mask
    put32(&mut out, LCS_SRGB);
    out.extend_from_slice(&[0u8; 36]); // endpoints
    put32(&mut out, 0); // gamma red
    put32(&mut out, 0); // gamma green
    put32(&mut out, 0); // gamma blue
    put32(&mut out, LCS_GM_IMAGES);
    put32(&mut out, 0); // profile data
    put32(&mut out, 0); // profile size
    put32(&mut out, 0); // reserved
    debug_assert_eq!(out.len(), 124);
    for y in (0..h).rev() {
        for px in img.pixels[y * w * 4..(y + 1) * w * 4].as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected 2x2 picture, top-down: red, green / blue, white.
    fn expected(alpha: [u8; 4]) -> Vec<u8> {
        vec![
            255, 0, 0, alpha[0], //
            0, 255, 0, alpha[1], //
            0, 0, 255, alpha[2], //
            255, 255, 255, alpha[3],
        ]
    }

    fn header40(w: i32, h: i32, bpp: u16, compression: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&bpp.to_le_bytes());
        v.extend_from_slice(&compression.to_le_bytes());
        v.extend_from_slice(&[0u8; 20]);
        v
    }

    fn png_pixels(dib: &[u8]) -> Rgba {
        let (png, w, h) = dib_to_png(dib).unwrap();
        let back = decode_image(&png).unwrap();
        assert_eq!((back.w, back.h), (w, h));
        back
    }

    #[test]
    fn bgr24_with_row_padding() {
        // 2 pixels * 3 bytes = 6, padded to 8 per row. Bottom-up: blue/white first.
        let mut d = header40(2, 2, 24, BI_RGB);
        d.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]); // blue, white, pad
        d.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // red, green, pad
        let img = png_pixels(&d);
        assert_eq!(img.pixels, expected([255; 4]));
    }

    #[test]
    fn bgrx32_without_alpha_is_opaque() {
        let mut d = header40(2, 2, 32, BI_RGB);
        d.extend_from_slice(&[255, 0, 0, 0, 255, 255, 255, 0]); // blue, white (alpha byte 0)
        d.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]); // red, green
        assert_eq!(png_pixels(&d).pixels, expected([255; 4]));
    }

    #[test]
    fn bgra32_keeps_real_alpha() {
        let mut d = header40(2, 2, 32, BI_RGB);
        d.extend_from_slice(&[255, 0, 0, 40, 255, 255, 255, 255]);
        d.extend_from_slice(&[0, 0, 255, 128, 0, 255, 0, 200]);
        assert_eq!(png_pixels(&d).pixels, expected([128, 200, 40, 255]));
    }

    #[test]
    fn bitfields_with_masks_after_header() {
        let mut d = header40(2, 2, 32, BI_BITFIELDS);
        // RGBA byte order in memory: masks describe a 0xAABBGGRR layout.
        for m in [0x0000_00ffu32, 0x0000_ff00, 0x00ff_0000] {
            d.extend_from_slice(&m.to_le_bytes());
        }
        // Without a v4 alpha mask, alpha is treated as opaque.
        d.extend_from_slice(&[0, 0, 255, 0, 255, 255, 255, 0]); // blue (b=255 at bits16..), white
        d.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0]); // red, green
        assert_eq!(png_pixels(&d).pixels, expected([255; 4]));
    }

    #[test]
    fn rgb565_bitfields() {
        let mut d = header40(2, 1, 16, BI_BITFIELDS);
        for m in [0xf800u32, 0x07e0, 0x001f] {
            d.extend_from_slice(&m.to_le_bytes());
        }
        d.extend_from_slice(&0xf800u16.to_le_bytes()); // red
        d.extend_from_slice(&0x001fu16.to_le_bytes()); // blue
        let img = dib_to_rgba(&d).unwrap();
        assert_eq!(img.pixels, [255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn top_down_negative_height() {
        let mut d = header40(2, -2, 24, BI_RGB);
        d.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
        d.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]);
        assert_eq!(png_pixels(&d).pixels, expected([255; 4]));
    }

    #[test]
    fn v5_round_trip_is_pixel_identical() {
        let img = Rgba {
            w: 3,
            h: 2,
            pixels: (0..24u8)
                .map(|i| i.wrapping_mul(37).wrapping_add(11))
                .collect(),
        };
        let v5 = rgba_to_dibv5(&img);
        assert_eq!(dib_to_rgba(&v5).unwrap(), img);
        let png = rgba_to_png(&img).unwrap();
        assert_eq!(decode_image(&png).unwrap(), img);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(dib_to_rgba(&[]), Err(DibError::Malformed));
        assert_eq!(dib_to_rgba(&[0; 8]), Err(DibError::Unsupported));
        let mut short = header40(4, 4, 24, BI_RGB);
        short.extend_from_slice(&[0; 10]);
        assert_eq!(dib_to_rgba(&short), Err(DibError::Malformed));
        let huge = header40(100_000, 100_000, 24, BI_RGB);
        assert_eq!(dib_to_rgba(&huge), Err(DibError::TooLarge));
        let rle = header40(2, 2, 24, 1);
        assert_eq!(dib_to_rgba(&rle), Err(DibError::Unsupported));
        let palette = header40(2, 2, 8, BI_RGB);
        assert_eq!(dib_to_rgba(&palette), Err(DibError::Unsupported));
    }
}
