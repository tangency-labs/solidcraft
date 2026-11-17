//! Rounded enclosures for the tooth module's certificates.
//!
//! The library's `sin`, `cos`, `atan` and `hypot` (the platform's libm; IEEE does not round them
//! correctly) are taken as within two units in the last place of the magnitude of their result.
//! Every other operation is an `Ivl` operation, widened outward, or counted with `gamma`.
//!
//! An interval here is the kernel's [`Ivl`]: its `+ - * /` and `sqrt` are widened outward past
//! their rounding, so each holds the exact result for every number of its operands, and anything
//! undefined (a NaN, a division by an interval that holds 0) is the whole line, which no test
//! passes. A single number enters an interval only through [`exact`], and an interval is negated
//! only through [`neg`].
//!
//! Not upstream: SolidCraft's tooth outlines (see [`crate::teeth`]). No recursion.

use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::geom::certified::{next_down, next_up};
use crate::teeth::outline::Point;

/// The point interval `[x, x]` (`Ivl::new(x, x)`); the whole line if `x` is not finite. The only
/// way a number enters an interval in the tooth module. Counts nothing: no operation is done.
pub(crate) fn exact(x: f64) -> Ivl {
    if x.is_finite() {
        Ivl::new(x, x)
    } else {
        Ivl::whole()
    }
}

/// `-x`, exactly: `[-x.hi, -x.lo]`. The only way an interval is negated (`Ivl` has no `Neg`). A
/// change of sign is exact, so nothing is counted.
pub(crate) fn neg(x: Ivl) -> Ivl {
    Ivl::new(-x.hi, -x.lo)
}

/// `x` squared, never negative. With `a = min(|lo|, |hi|)` and `b = max(|lo|, |hi|)`: when `x`
/// holds 0 (`lo <= 0 <= hi`) the lower end is exactly 0.0; otherwise it is
/// `max(0.0, round_down(a * a))` (the absolute part of about 1e-300 in `round_down` would make
/// `round_down(0.0)`, or that of a tiny square, negative, which a square never is). The upper end
/// is `round_up(b * b)`. The whole line for a non-finite `x`. Counted: the two products, each
/// moved outward by `round_down` / `round_up`, which cover one rounding of each.
pub(crate) fn sqr(x: Ivl) -> Ivl {
    if !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi) {
        return Ivl::whole();
    }
    let (a, b) = (x.lo.abs().min(x.hi.abs()), x.lo.abs().max(x.hi.abs()));
    let lo = if x.lo <= 0.0 && x.hi >= 0.0 {
        0.0
    } else {
        round_down(a * a).max(0.0)
    };
    Ivl::new(lo, round_up(b * b))
}

/// A libm result `x` (sin, cos, atan or hypot) widened by two units in the last place of `|x|`:
/// `w = 2 (next_up(|x|) - |x|)` (exact); `[next_down(x - w), next_up(x + w)]`; the whole line if
/// `x` is not finite. Counted: the libm result as within two units in the last place (the
/// module's one assumption); the two sums, each rounded, covered by one step outward.
pub(crate) fn libm(x: f64) -> Ivl {
    if !x.is_finite() {
        return Ivl::whole();
    }
    let w = 2.0 * (next_up(x.abs()) - x.abs());
    let (lo, hi) = (next_down(x - w), next_up(x + w));
    if lo.is_finite() && hi.is_finite() {
        Ivl::new(lo, hi)
    } else {
        Ivl::whole()
    }
}

/// pi enclosed: `[PI, next_up(PI)]`. The double `PI` lies below pi, and the next double above it
/// lies above pi.
pub(crate) fn pi() -> Ivl {
    Ivl::new(std::f64::consts::PI, next_up(std::f64::consts::PI))
}

/// The middle `m = 0.5 (lo + hi)` of a stretch and a half width `r >= max(m - lo, hi - m)`,
/// rounded up (`r = next_up(max(m - lo, hi - m))`). Takes `lo <= hi`. Counted: the two
/// differences, each rounded to nearest (at most half a unit in the last place), covered by the
/// step up; the middle needs no count, as it is the centre the half width is measured from. An
/// infinite half width where a difference is not a number.
pub(crate) fn half_width(lo: f64, hi: f64) -> (f64, f64) {
    let m = 0.5 * (lo + hi);
    let below = m - lo;
    let above = hi - m;
    let r = if below.is_nan() || above.is_nan() {
        f64::INFINITY
    } else {
        next_up(below.max(above))
    };
    (m, r)
}

/// sin and cos over every angle of `angle`: with `(m, r) = half_width(angle.lo, angle.hi)`,
/// `S = libm(sin m) + libm(cos m) [-r, r] + [-q, q]`,
/// `C = libm(cos m) + libm(sin m) [-r, r] + [-q, q]`, `q = (exact(r) exact(r) exact(0.5)).hi`
/// (Taylor, `|sin''|, |cos''| <= 1`), each then cut to `[-1, 1]`. A non-finite angle gives
/// `([-1, 1], [-1, 1])`. Counted: libm sin and cos of the middle, two units in the last place
/// each; the rest as `Ivl` operations; the Taylor remainder `r^2 / 2` rounded up.
pub(crate) fn sin_cos(angle: Ivl) -> (Ivl, Ivl) {
    let any = Ivl::new(-1.0, 1.0);
    if !angle.is_finite() {
        return (any, any);
    }
    let (m, r) = half_width(angle.lo, angle.hi);
    if !r.is_finite() {
        return (any, any);
    }
    let (s, c) = m.sin_cos();
    let spread = Ivl::new(-r, r);
    let q = (exact(r) * exact(r) * exact(0.5)).hi;
    let rest = Ivl::new(-q, q);
    let sine = libm(s) + libm(c) * spread + rest;
    let cosine = libm(c) + libm(s) * spread + rest;
    (cut_to_one(sine), cut_to_one(cosine))
}

/// An enclosure of a sine or a cosine cut to `[-1, 1]`, which holds every one of them.
fn cut_to_one(x: Ivl) -> Ivl {
    let lo = if x.lo > -1.0 { x.lo } else { -1.0 };
    let hi = if x.hi < 1.0 { x.hi } else { 1.0 };
    Ivl::new(lo, hi)
}

/// atan over every number of `x` (atan increases):
/// `[libm(atan(x.lo)).lo, libm(atan(x.hi)).hi]`; the whole line for NaN. Counted: libm atan of
/// the two ends, two units in the last place each.
pub(crate) fn atan_over(x: Ivl) -> Ivl {
    if x.lo.is_nan() || x.hi.is_nan() {
        return Ivl::whole();
    }
    Ivl::new(libm(x.lo.atan()).lo, libm(x.hi.atan()).hi)
}

/// `R(angle) v` enclosed for every angle of `angle`: with `(S, C) = sin_cos(angle)`,
/// `[C v0 - S v1, S v0 + C v1]`. (Named `rotated_ivl` so that it never clashes with
/// `outline::rotated(p: Point, angle: f64)`, which involute.rs and fit.rs already import.)
/// Counted: [`sin_cos`], then four products and two sums as `Ivl` operations.
pub(crate) fn rotated_ivl(angle: Ivl, v: [Ivl; 2]) -> [Ivl; 2] {
    let (s, c) = sin_cos(angle);
    let [v0, v1] = v;
    [c * v0 - s * v1, s * v0 + c * v1]
}

/// An upper bound of `|v|` for every `v` of the box: `sqrt(mag(v0)^2 + mag(v1)^2)` in `Ivl`, its
/// upper end (infinite when not finite, or when an end of the box is not a number). Counted: the
/// squares, the sum and the root as `Ivl` operations.
pub(crate) fn magnitude(v: [Ivl; 2]) -> f64 {
    let [v0, v1] = v;
    if has_nan(v0) || has_nan(v1) {
        return f64::INFINITY;
    }
    let size = (sqr(exact(v0.mag())) + sqr(exact(v1.mag()))).sqrt().hi;
    if size.is_finite() {
        size
    } else {
        f64::INFINITY
    }
}

/// Whether an end of an interval is not a number. The `Ivl` operations never make one, but an
/// interval built from outside values can hold one, and `Ivl::mag` would then report the other
/// end's size.
fn has_nan(x: Ivl) -> bool {
    x.lo.is_nan() || x.hi.is_nan()
}

/// The second-order Taylor form over a stretch of half width `r` about its middle:
/// `value + slope [-r, r] + curvature [0, q]`, `q = (exact(r) exact(r) exact(0.5)).hi`. `value`
/// and `slope` are enclosures at the middle, `curvature` an enclosure of the second derivative
/// over the stretch. Counted: `q` rounded up, then two products and two sums as `Ivl` operations.
pub(crate) fn taylor(value: Ivl, slope: Ivl, curvature: Ivl, r: f64) -> Ivl {
    let q = (exact(r) * exact(r) * exact(0.5)).hi;
    value + slope * Ivl::new(-r, r) + curvature * Ivl::new(0.0, q)
}

/// The same form anchored at an end of a stretch of width `h` (`h` at least the width, rounded
/// up): from the start (forward) `value + slope [0, h] + curvature [0, q]`; from the end
/// (backward) `value + slope [-h, 0] + curvature [0, q]`; `q = (exact(h) exact(h) exact(0.5)).hi`.
/// Counted as [`taylor`].
pub(crate) fn taylor_from_end(
    value: Ivl,
    slope: Ivl,
    curvature: Ivl,
    h: f64,
    forward: bool,
) -> Ivl {
    let q = (exact(h) * exact(h) * exact(0.5)).hi;
    let step = if forward {
        Ivl::new(0.0, h)
    } else {
        Ivl::new(-h, 0.0)
    };
    value + slope * step + curvature * Ivl::new(0.0, q)
}

/// What a curve does over a stretch of its parameter: enclosures of its first and second derivative
/// vectors, a bound on the size of the second, and the counted rounding of one computed point.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bend {
    /// Encloses `P'(t)`, each coordinate, for every `t` of the stretch.
    pub(crate) first: [Ivl; 2],
    /// Encloses `P''(t)`, each coordinate, for every `t` of the stretch.
    pub(crate) second: [Ivl; 2],
    /// An upper bound of `|P''(t)|` over the stretch (may be tighter than `magnitude(second)`).
    pub(crate) second_size: f64,
    /// An upper bound of `|computed point - P(t)|` for every `t` of the stretch.
    pub(crate) rounding: f64,
}

impl Bend {
    /// Nothing known: whole intervals, infinite sizes. Proves nothing.
    pub(crate) fn unknown() -> Bend {
        Bend {
            first: [Ivl::whole(), Ivl::whole()],
            second: [Ivl::whole(), Ivl::whole()],
            second_size: f64::INFINITY,
            rounding: f64::INFINITY,
        }
    }
}

/// An upper bound of the Hausdorff distance (both ways, as sets) between the curve over a stretch
/// of parameter width at most `h` and the chord between its two computed end points `p0`, `p1`,
/// with the rounding of those points counted. Infinite (never NaN) where nothing is proved.
///
/// Two bounds are formed and the smaller is taken; both hold. By size: `h^2/8` times the size of
/// `P''` (`Bend::second_size`). Across the chord: `h^2/8` times the size of the part of `P''`
/// across the chord, which holds where `P'` runs along the chord, or against it, on the whole
/// stretch (`along.lo > 0.0 || along.hi < 0.0`, so a curve sampled from a larger to a smaller
/// parameter has it too). It is what keeps a curve with a large `P''` along its own direction, as
/// an involute flank near its base circle has, from being cut far finer than it needs.
///
/// Proof. With `E = bend.rounding`, `s` in `[0, 1]` and `L(s)` the chord between the exact end
/// points `P(t0)` and `P(t1)`, `|P(t0 + s h) - L(s)| <= h^2/8 max |P''|` (the Peano kernel of
/// linear interpolation, in vector form by the integral): the bound by size holds at matched
/// parameters, and so as sets. Where `P'.c` has one sign on the whole stretch (`c` the exact
/// chord; positive, or negative for a curve that runs against its chord, as when `sampled` is
/// called with `from > to`), the curve is a graph over the chord whose projection covers the chord
/// segment, so its distance from the segment is `|n.(P - P(t0))|` (`n` the unit normal of the
/// chord), a function that vanishes at both ends, bounded by `h^2/8 max |n.P''|`. The exact chord
/// lies in the box `C = (p1 - p0) + [-2E, 2E]^2`, so every interval below is taken over `C`; the
/// computed chord is within `E` of `L` everywhere. Hence the Hausdorff distance between the curve
/// over the stretch and the computed chord is at most the result. The bound across the chord does
/// NOT bound the distance at matched parameters (a point at the same parameter may lie farther
/// along the chord); nothing in the tooth module needs matched parameters from `sampled`. Where
/// the along test fails (near the involute's cusp on the base circle, where `P' = 0`) only the
/// bound by size is left.
///
/// Counted: `h^2/8` as `exact(h) exact(h) exact(0.125)` rounded outward; every other operation an
/// `Ivl` operation; the end points' rounding `E` added last and the sum rounded up.
pub(crate) fn chord_bound(h: f64, p0: Point, p1: Point, bend: &Bend) -> f64 {
    let e = bend.rounding;
    if !(e >= 0.0) {
        return f64::INFINITY;
    }
    // An end point that is not a number proves nothing, whatever the bends are (the bound by size
    // never reads the end points).
    let [x0, y0] = p0;
    let [x1, y1] = p1;
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return f64::INFINITY;
    }
    let hh = exact(h) * exact(h) * exact(0.125);
    let plain = (hh * exact(bend.second_size)).hi;
    let across = across_bound(hh, e, p0, p1, bend);
    let best = if across < plain { across } else { plain };
    let result = (exact(best) + exact(e)).hi;
    if result.is_nan() {
        f64::INFINITY
    } else {
        result
    }
}

/// The bound across the chord of [`chord_bound`], before the rounding of the end points is added:
/// `h^2/8` times the size of the component of `P''` across the chord, or infinite where `P'` does
/// not run along the chord (or against it) on the whole stretch, or where the chord may have no
/// length.
fn across_bound(hh: Ivl, e: f64, p0: Point, p1: Point, bend: &Bend) -> f64 {
    let wide = 2.0 * e;
    let slack = Ivl::new(-wide, wide);
    let ([x0, y0], [x1, y1]) = (p0, p1);
    let c0 = exact(x1) - exact(x0) + slack;
    let c1 = exact(y1) - exact(y0) + slack;
    let len = (sqr(c0) + sqr(c1)).sqrt();
    if !(len.lo > 0.0) {
        return f64::INFINITY;
    }
    let [f0, f1] = bend.first;
    let along = c0 * f0 + c1 * f1;
    if !(along.lo > 0.0 || along.hi < 0.0) {
        return f64::INFINITY;
    }
    let [s0, s1] = bend.second;
    let cross = (c0 * s1 - c1 * s0) / len;
    if has_nan(cross) {
        return f64::INFINITY;
    }
    let bound = (hh * exact(cross.mag())).hi;
    if bound.is_nan() {
        f64::INFINITY
    } else {
        bound
    }
}
