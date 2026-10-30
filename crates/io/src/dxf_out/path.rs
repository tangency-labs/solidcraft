//! Minimal X1 bulged-polyline support; path conversion/fitting is not ported.

use solidcraft_geom::{Seg2, Vec2};

/// A polyline vertex. `bulge` describes the edge leaving `at`: `tan(sweep / 4)`.
/// The terminal vertex of an open polyline has zero bulge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfVertex {
    pub at: Vec2,
    pub bulge: f64,
}

pub(super) const MAX_PATH_ITEMS: usize = 200_000;

fn circle_bounds_finite(c: Vec2, r: f64) -> bool {
    c.is_finite() && r.is_finite() && r > 0.0 && (c - Vec2::new(r, r)).is_finite() && (c + Vec2::new(r, r)).is_finite()
}

/// Reconstruct a non-straight polyline edge with a signed sweep for bounds and validation.
/// Avoid squaring the chord length or first forming 1/bulge: those can overflow even when
/// the resulting radius is finite. A zero chord with a nonzero bulge is not an arc.
pub(super) fn bulge_arc(from: DxfVertex, to: Vec2) -> Option<Seg2> {
    let b = from.bulge;
    if !(from.at.is_finite() && to.is_finite() && b.is_finite()) || b == 0.0 || b.abs() > 1.0 {
        return None;
    }
    let chord = to - from.at;
    let length = chord.len();
    if !chord.is_finite() || !length.is_finite() || length == 0.0 {
        return None;
    }
    let scaled = (length * 0.25) / b;
    let radius = scaled.abs() * (1.0 + b * b);
    let center = from.at * 0.5 + to * 0.5 + (chord / length).perp() * (scaled * (1.0 - b * b));
    if !circle_bounds_finite(center, radius) {
        return None;
    }
    let radial = from.at - center;
    if !radial.is_finite() || radial == Vec2::ZERO {
        return None;
    }
    let start = radial.angle();
    let sweep = 4.0 * b.atan();
    (start.is_finite() && sweep.is_finite() && sweep != 0.0).then_some(Seg2::Arc { center, radius, start, sweep })
}
