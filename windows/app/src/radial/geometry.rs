//! Pure geometry of the radial menu (PLAN section 5.4): which segment is under the cursor.
//!
//! Segment 0 is centred at the top; segments run clockwise.

/// What the cursor is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The centre disc: dropping here cancels.
    Cancel,
    /// Outside the ring (or no segments).
    Outside,
    /// A segment index.
    Segment(usize),
}

/// Classify the offset (`dx`, `dy`, screen orientation: y grows downwards) from the centre.
pub fn hit_test(dx: f64, dy: f64, n: usize, inner: f64, outer: f64) -> Hit {
    if n == 0 {
        return Hit::Outside;
    }
    let d = dx.hypot(dy);
    if d < inner {
        return Hit::Cancel;
    }
    if d > outer {
        return Hit::Outside;
    }
    Hit::Segment(segment_of(dx, dy, n))
}

/// Segment index for a direction; the distance is ignored.
pub fn segment_of(dx: f64, dy: f64, n: usize) -> usize {
    let n = n.max(1);
    // Degrees clockwise from the top: atan2(dx, -dy).
    let mut deg = dx.atan2(-dy).to_degrees();
    if deg < 0.0 {
        deg += 360.0;
    }
    let width = 360.0 / n as f64;
    // Shift by half a segment so segment 0 is centred on the top.
    let shifted = (deg + width / 2.0) % 360.0;
    ((shifted / width) as usize).min(n - 1)
}

/// Centre of a segment as an angle (degrees clockwise from the top).
pub fn segment_centre_deg(i: usize, n: usize) -> f64 {
    i as f64 * 360.0 / n.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: f64 = 40.0;
    const OUT: f64 = 140.0;

    fn at(deg: f64, r: f64) -> (f64, f64) {
        let rad = deg.to_radians();
        (r * rad.sin(), -r * rad.cos())
    }

    #[test]
    fn every_segment_is_hit_at_its_own_centre_for_one_to_twelve() {
        for n in 1..=12 {
            for i in 0..n {
                let (dx, dy) = at(segment_centre_deg(i, n), 90.0);
                assert_eq!(hit_test(dx, dy, n, IN, OUT), Hit::Segment(i), "n={n} i={i}");
            }
        }
    }

    #[test]
    fn boundaries_fall_into_the_next_segment_and_wrap() {
        // n = 4: segment 0 spans -45..45 degrees.
        let (dx, dy) = at(44.9, 90.0);
        assert_eq!(hit_test(dx, dy, 4, IN, OUT), Hit::Segment(0));
        let (dx, dy) = at(45.1, 90.0);
        assert_eq!(hit_test(dx, dy, 4, IN, OUT), Hit::Segment(1));
        let (dx, dy) = at(-45.1, 90.0);
        assert_eq!(hit_test(dx, dy, 4, IN, OUT), Hit::Segment(3));
    }

    #[test]
    fn centre_cancels_and_far_is_outside() {
        assert_eq!(hit_test(0.0, 0.0, 3, IN, OUT), Hit::Cancel);
        assert_eq!(hit_test(10.0, 10.0, 3, IN, OUT), Hit::Cancel);
        assert_eq!(hit_test(300.0, 0.0, 3, IN, OUT), Hit::Outside);
        assert_eq!(hit_test(50.0, 0.0, 0, IN, OUT), Hit::Outside);
    }

    #[test]
    fn index_is_always_in_range() {
        for n in 1..=12 {
            for deg in 0..720 {
                let (dx, dy) = at(f64::from(deg) * 0.5, 100.0);
                assert!(segment_of(dx, dy, n) < n);
            }
        }
    }
}
