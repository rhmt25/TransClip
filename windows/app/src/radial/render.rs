//! Software rendering of the radial menu into a premultiplied BGRA bitmap (PLAN section 5.4,
//! pedoman_desain). Segments are separate rounded wedges with a gap between them, around a
//! centre disc that is the cancel drop zone. Pixels inside a shape have alpha >= 1, so they
//! receive the drop; everything else is transparent.

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
    /// Gap between neighbouring shapes.
    pub gap: f64,
    /// Corner radius of the segments.
    pub corner: f64,
}

impl Layout {
    /// Layout for a ring of the given outer radius; `scale` is the DPI scale (1.0 = 96 dpi).
    pub fn new(outer_px: f64, scale: f64) -> Self {
        let outer = outer_px.max(80.0);
        let size = ((outer + 8.0) * 2.0).ceil() as u32;
        Self {
            size,
            centre: f64::from(size) / 2.0,
            inner: outer * 0.34,
            outer,
            gap: 5.0 * scale,
            corner: 10.0 * scale,
        }
    }

    /// Radius where the labels sit.
    pub fn label_radius(&self) -> f64 {
        (self.inner + self.gap + self.outer) / 2.0
    }
}

type Rgba = (u8, u8, u8, u8);

/// Colours from pedoman_desain: soft charcoal glass with a sky-blue edge and highlight.
pub const FILL: Rgba = (18, 21, 26, 224);
pub const CENTRE: Rgba = (26, 30, 38, 235);
pub const EDGE: Rgba = (56, 189, 248, 96);
pub const HOT: Rgba = (56, 189, 248, 238);
pub const HOT_EDGE: Rgba = (125, 211, 252, 255);
pub const CANCEL_FILL: Rgba = (78, 30, 36, 240);
pub const CANCEL_EDGE: Rgba = (248, 113, 113, 200);

fn premultiply(c: Rgba, coverage: f64) -> [f64; 4] {
    let a = f64::from(c.3) * coverage.clamp(0.0, 1.0);
    [
        f64::from(c.2) * a / 255.0,
        f64::from(c.1) * a / 255.0,
        f64::from(c.0) * a / 255.0,
        a,
    ]
}

/// Paint `top` (premultiplied) over `bottom` (premultiplied).
fn over(top: [f64; 4], bottom: [f64; 4]) -> [f64; 4] {
    let k = 1.0 - top[3] / 255.0;
    [
        top[0] + bottom[0] * k,
        top[1] + bottom[1] * k,
        top[2] + bottom[2] * k,
        top[3] + bottom[3] * k,
    ]
}

/// Inside-distance (positive inside, in pixels) of a rounded wedge. `a1` is the distance to the
/// nearest radial edge, `a2` to the nearest side line.
fn rounded(a1: f64, a2: f64, corner: f64) -> f64 {
    if a1 < corner && a2 < corner {
        corner - (corner - a1).hypot(corner - a2)
    } else {
        a1.min(a2)
    }
}

/// Paint one shape with a soft 1 px edge: fill inside, `edge` colour on the outermost pixel.
fn paint(s: f64, fill: Rgba, edge: Rgba) -> [f64; 4] {
    let cover = (s + 0.5).clamp(0.0, 1.0);
    if cover <= 0.0 {
        return [0.0; 4];
    }
    let body = premultiply(fill, cover);
    let edge_w = (1.5 - s).clamp(0.0, 1.0);
    over(premultiply(edge, cover * edge_w), body)
}

/// Render `n` segments. `highlight` is the hovered segment; `cancel_hot` marks the centre disc
/// as hovered. Row 0 is the top row.
pub fn render(layout: &Layout, n: usize, highlight: Option<usize>, cancel_hot: bool) -> Vec<u8> {
    let size = layout.size as usize;
    let mut px = vec![0u8; size * size * 4];
    let width = std::f64::consts::TAU / n.max(1) as f64;
    let r_in = layout.inner + layout.gap;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 + 0.5 - layout.centre;
            let dy = y as f64 + 0.5 - layout.centre;
            let d = dx.hypot(dy);
            let out = if d <= layout.inner + 1.0 {
                let (fill, edge) = if cancel_hot {
                    (CANCEL_FILL, CANCEL_EDGE)
                } else {
                    (CENTRE, EDGE)
                };
                paint(layout.inner - d, fill, edge)
            } else if n > 0 && d >= r_in - 1.0 && d <= layout.outer + 1.0 {
                let seg = segment_of(dx, dy, n);
                let centre_angle = seg as f64 * width;
                let mut a = dx.atan2(-dy);
                if a < 0.0 {
                    a += std::f64::consts::TAU;
                }
                let mut diff = (a - centre_angle).abs();
                if diff > std::f64::consts::PI {
                    diff = std::f64::consts::TAU - diff;
                }
                let radial = (d - r_in).min(layout.outer - d);
                let side = if n > 1 {
                    d * (width / 2.0 - diff).max(0.0).sin() - layout.gap / 2.0
                } else {
                    f64::MAX
                };
                let (fill, edge) = if highlight == Some(seg) {
                    (HOT, HOT_EDGE)
                } else {
                    (FILL, EDGE)
                };
                paint(rounded(radial, side, layout.corner), fill, edge)
            } else {
                [0.0; 4]
            };
            let i = (y * size + x) * 4;
            let alpha = out[3].round().clamp(0.0, 255.0) as u8;
            let cap = f64::from(alpha);
            for (k, v) in out.iter().take(3).enumerate() {
                px[i + k] = v.round().clamp(0.0, cap) as u8;
            }
            px[i + 3] = alpha;
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(px: &[u8], l: &Layout, x: f64, y: f64) -> u8 {
        px[(y as usize * l.size as usize + x as usize) * 4 + 3]
    }

    #[test]
    fn shapes_are_opaque_enough_and_the_outside_is_transparent() {
        let l = Layout::new(140.0, 1.0);
        let px = render(&l, 4, None, false);
        assert_eq!(px.len(), (l.size * l.size * 4) as usize);
        assert!(alpha_at(&px, &l, l.centre + l.label_radius(), l.centre) >= 1);
        assert!(alpha_at(&px, &l, l.centre, l.centre) >= 1);
        assert_eq!(alpha_at(&px, &l, 1.0, 1.0), 0);
    }

    #[test]
    fn there_is_a_gap_between_the_centre_and_the_ring() {
        let l = Layout::new(140.0, 1.0);
        let px = render(&l, 4, None, false);
        assert_eq!(
            alpha_at(&px, &l, l.centre + l.inner + l.gap / 2.0, l.centre),
            0
        );
    }

    #[test]
    fn neighbouring_segments_are_separated_by_a_gap() {
        let l = Layout::new(140.0, 1.0);
        let px = render(&l, 4, None, false);
        // The boundary between segment 0 and 1 lies 45 degrees clockwise from the top.
        let r = l.label_radius();
        let (bx, by) = (
            l.centre + r * 45f64.to_radians().sin(),
            l.centre - r * 45f64.to_radians().cos(),
        );
        assert_eq!(alpha_at(&px, &l, bx, by), 0);
    }

    #[test]
    fn highlighted_segment_uses_the_accent_colour() {
        let l = Layout::new(140.0, 1.0);
        let off = render(&l, 4, None, false);
        let on = render(&l, 4, Some(0), false);
        let i = ((l.centre - l.label_radius()) as usize * l.size as usize + l.centre as usize) * 4;
        assert_ne!(off[i..i + 4], on[i..i + 4]);
        // BGRA: blue is the strongest channel of the accent.
        assert!(on[i] > on[i + 2]);
    }

    #[test]
    fn cancel_hover_turns_the_centre_red() {
        let l = Layout::new(140.0, 1.0);
        let off = render(&l, 3, None, false);
        let on = render(&l, 3, None, true);
        let i = (l.centre as usize * l.size as usize + l.centre as usize) * 4;
        // BGRA: red is the strongest channel when hovered.
        assert!(on[i + 2] > on[i]);
        assert!(off[i] >= off[i + 2]);
    }

    #[test]
    fn premultiplied_channels_never_exceed_alpha() {
        let l = Layout::new(100.0, 1.0);
        let px = render(&l, 5, Some(2), true);
        for p in px.as_chunks::<4>().0 {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
        }
    }

    #[test]
    fn a_single_segment_is_a_full_ring() {
        let l = Layout::new(120.0, 1.0);
        let px = render(&l, 1, None, false);
        assert!(alpha_at(&px, &l, l.centre - l.label_radius(), l.centre) >= 1);
        assert!(alpha_at(&px, &l, l.centre + l.label_radius(), l.centre) >= 1);
    }
}
