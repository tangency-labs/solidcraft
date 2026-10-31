//! 2D drawing exchange for sketches: DXF (ASCII, read and write) and SVG (read). Readers produce
//! plain 2D geometry in millimetres; the engine turns it into sketch curves.

use solidcraft_geom::{MAX_BEZIER_DEGREE, Vec2};

/// One imported 2D entity (millimetres, y up).
#[derive(Clone, Debug, PartialEq)]
pub enum Geom2 {
    Point(Vec2),
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
    /// Counter-clockwise arc around `c` from `a` to `b`.
    Arc {
        c: Vec2,
        a: Vec2,
        b: Vec2,
    },
    /// Full ellipse: centre, end of the major axis, minor radius.
    Ellipse {
        c: Vec2,
        major: Vec2,
        minor: f64,
    },
    /// Spline through (`control == false`) or over (`control`) the points.
    Spline {
        pts: Vec<Vec2>,
        control: bool,
        degree: u8,
    },
    /// B-spline over `ctrl` with its own `knots` (`ctrl.len() + degree + 1`, not clamped and
    /// evenly spaced, or with uneven `weights`: those are [`Geom2::Spline`]), as a file gives it.
    /// Made by [`Geom2::bspline`]; the importer splits it into the exact pieces a sketch holds.
    Nurbs {
        degree: u8,
        knots: Vec<f64>,
        ctrl: Vec<Vec2>,
        weights: Option<Vec<f64>>,
    },
    /// Quadratic Bézier / conic.
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        rho: f64,
    },
    Text {
        text: String,
        at: Vec2,
        height: f64,
        angle: f64,
    },
}

impl Geom2 {
    /// The B-spline a file gives as `degree`, `ctrl`, `knots` and, when rational, `weights` (the
    /// fields of a DXF SPLINE): a [`Geom2::Spline`] over the points when the knots are clamped and
    /// evenly spaced and the weights all equal, which is exactly the sketch's own spline, else a
    /// [`Geom2::Nurbs`] that keeps them. `None` when they don't describe a curve: a degree outside
    /// 1 to 9, a knot count other than `ctrl.len() + degree + 1`, knots that aren't finite or
    /// decrease or leave no domain, or weights that aren't one positive number per point.
    pub fn bspline(degree: u8, ctrl: Vec<Vec2>, knots: Vec<f64>, weights: Option<Vec<f64>>) -> Option<Geom2> {
        let (n, p) = (ctrl.len(), usize::from(degree));
        if !(1..=MAX_BEZIER_DEGREE).contains(&p) || n < p + 1 || knots.len() != n + p + 1 {
            return None;
        }
        if !knots.iter().all(|k| k.is_finite()) || knots.windows(2).any(|w| w[1] < w[0]) || !ctrl.iter().all(|q| q.is_finite()) {
            return None;
        }
        let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
        if hi <= lo {
            return None;
        }
        if let Some(w) = &weights
            && (w.len() != n || !w.iter().all(|w| w.is_finite() && *w > 0.0))
        {
            return None;
        }
        // Weights that are all the same are no weights: only their ratios shape the curve.
        let weights = weights.filter(|w| w.first().is_some_and(|first| w.iter().any(|v| (v - first).abs() > 1e-12 * first)));
        // The sketch's spline: each end knot `degree + 1` times, the inner ones at even steps (to
        // the last digits a file writes them with).
        let spans = n - p;
        let even = knots.iter().enumerate().all(|(i, k)| {
            let at = lo + (hi - lo) * (i.saturating_sub(p).min(spans) as f64 / spans as f64);
            (k - at).abs() <= 1e-9 * (hi - lo)
        });
        Some(if even && weights.is_none() {
            Geom2::Spline { pts: ctrl, control: true, degree }
        } else {
            Geom2::Nurbs { degree, knots, ctrl, weights }
        })
    }
}

/// Most entities one file may produce.
pub const MAX_ENTITIES: usize = 200_000;
/// Largest drawing file we read.
pub const MAX_DRAWING_BYTES: usize = 64 << 20;

/// Counter-clockwise arc from `p0` to `p1` with DXF bulge `b` (tan of a quarter of the
/// included angle, negative = clockwise).
pub(crate) fn bulge_arc(p0: Vec2, p1: Vec2, b: f64) -> Option<Geom2> {
    if b.abs() < 1e-12 || !b.is_finite() {
        return Some(Geom2::Line(p0, p1));
    }
    let chord = p1 - p0;
    let l = chord.len();
    if l < 1e-12 {
        return None;
    }
    let theta = 4.0 * b.atan();
    let r = l / (2.0 * (theta / 2.0).sin()).abs();
    // Centre on the perpendicular bisector, to the left for counter-clockwise bulges.
    let mid = (p0 + p1) * 0.5;
    let h = r * (theta / 2.0).cos().abs();
    let left = chord.perp() / l;
    let sign = if theta.abs() > std::f64::consts::PI { -1.0 } else { 1.0 };
    let c = if b > 0.0 { mid + left * (h * sign) } else { mid - left * (h * sign) };
    Some(if b > 0.0 { Geom2::Arc { c, a: p0, b: p1 } } else { Geom2::Arc { c, a: p1, b: p0 } })
}
