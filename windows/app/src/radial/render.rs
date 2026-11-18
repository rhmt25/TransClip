//! Software rendering of the radial ring into a premultiplied BGRA bitmap (PLAN section 5.4).
//!
//! The ring pixels are opaque enough (alpha >= 1) for hit-testing; everything outside is fully
//! transparent. Labels are drawn afterwards with GDI.

use super::geometry::segment_of;

/// Size and radii of one rendering, in device pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// Bitmap width and height.
    pub size: u32,
    /// Centre coordinate (same for x and y).
    pub centre: f64,
    /// Radius of the cancel disc.
    pub inner: f64,
    /// Outer radius of the ring.
    pub outer: f64,
}

impl Layout {
    /// Layout for a ring of the given outer radius; the cancel disc is 40 px at 100 % scaling.
    pub fn new(outer_px: f64, scale: f64) -> Self {
        let outer = outer_px.max(60.0);
        let size = ((outer + 6.0) * 2.0).ceil() as u32;
        Self {
            size,
            centre: f64::from(size) / 2.0,
            inner: 40.0 * scale,
            outer,
        }
    }
}

const BASE: (u8, u8, u8, u8) = (38, 42, 50, 225);
const HOT: (u8, u8, u8, u8) = (0, 120, 215, 240);
const CANCEL: (u8, u8, u8, u8) = (70, 70, 76, 200);
const CANCEL_HOT: (u8, u8, u8, u8) = (196, 54, 54, 245);
const LINE: (u8, u8, u8, u8) = (10, 10, 12, 255);

fn premultiply(c: (u8, u8, u8, u8), coverage: f64) -> [u8; 4] {
    let a = f64::from(c.3) * coverage.clamp(0.0, 1.0);
    let p = |v: u8| (f64::from(v) * a / 255.0).round() as u8;
    [p(c.2), p(c.1), p(c.0), a.round() as u8] // BGRA
}

/// Render `n` segments with `highlight` drawn in the accent colour. Row 0 is the top row.
pub fn render(layout: &Layout, n: usize, highlight: Option<usize>, cancel_hot: bool) -> Vec<u8> {
    let size = layout.size as usize;
    let mut px = vec![0u8; size * size * 4];
    let width_rad = std::f64::consts::TAU / n.max(1) as f64;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - layout.centre;
            let dy = y as f64 + 0.5 - layout.centre;
            let d = dx.hypot(dy);
            let out = if d < layout.inner - 0.5 {
                let c = if cancel_hot { CANCEL_HOT } else { CANCEL };
                premultiply(c, 1.0)
            } else if d <= layout.outer + 0.5 && n > 0 {
                // Soft edges: one pixel of coverage falloff on both radii.
                let cov = (layout.outer + 0.5 - d)
                    .min(d - (layout.inner - 0.5))
                    .clamp(0.0, 1.0);
                let seg = segment_of(dx, dy, n);
                let base = if highlight == Some(seg) { HOT } else { BASE };
                // Distance to the nearest boundary between two segments, in pixels.
                let mut a = dx.atan2(-dy);
                if a < 0.0 {
                    a += std::f64::consts::TAU;
                }
                let rel = (a + width_rad / 2.0) % width_rad;
                let edge = rel.min(width_rad - rel) * d;
                if n > 1 && edge < 1.0 {
                    premultiply(LINE, cov)
                } else {
                    premultiply(base, cov)
                }
            } else {
                [0, 0, 0, 0]
            };
            let i = (y * size + x) * 4;
            px[i..i + 4].copy_from_slice(&out);
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(px: &[u8], l: &Layout, x: f64, y: f64) -> u8 {
        let i = (y as usize * l.size as usize + x as usize) * 4;
        px[i + 3]
    }

    #[test]
    fn ring_is_opaque_enough_and_outside_is_transparent() {
        let l = Layout::new(140.0, 1.0);
        let px = render(&l, 4, None, false);
        assert_eq!(px.len(), (l.size * l.size * 4) as usize);
        // A point on the ring (90 px right of the centre) has alpha >= 1.
        assert!(alpha_at(&px, &l, l.centre + 90.0, l.centre) >= 1);
        // The corner is outside the ring.
        assert_eq!(alpha_at(&px, &l, 1.0, 1.0), 0);
    }

    #[test]
    fn highlighted_segment_uses_the_accent_colour() {
        let l = Layout::new(140.0, 1.0);
        let off = render(&l, 4, None, false);
        let on = render(&l, 4, Some(0), false);
        // Segment 0 is at the top: sample 90 px above the centre.
        let i = ((l.centre - 90.0) as usize * l.size as usize + l.centre as usize) * 4;
        assert_ne!(off[i..i + 4], on[i..i + 4]);
        // BGRA: blue is the strongest channel of the accent.
        assert!(on[i] > on[i + 2]);
    }

    #[test]
    fn premultiplied_channels_never_exceed_alpha() {
        let l = Layout::new(100.0, 1.0);
        let px = render(&l, 5, Some(2), true);
        for p in px.as_chunks::<4>().0 {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
        }
    }
}
