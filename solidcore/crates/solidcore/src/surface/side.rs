//! The sides of a face's parameter box, and the certified queries on them that extending a
//! sheet past one side needs: whether a side collapses to a point
//! ([`Surface::side_diameter`]), how far a surface runs across a side
//! ([`Surface::length_across_side`]).
//!
//! Not upstream: the operations SolidCraft composed over the kernel, moved into it so the
//! modelling layer computes no geometry (docs/layers.md §1). Each query returns its value with
//! a certified enclosure and decides nothing itself (owner rule 13564): the caller compares
//! the enclosure with its tolerance.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::borrow::Cow;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::{BSplineSurface, IsoCurveOptions, SurfParamType};
use crate::nurbs::math::{gamma, CPoint, NOW};
use crate::nurbs::srf_basic::Surface as Net;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::surface::{Surface, SurfaceKind};

const OP_DIAMETER: &str = "surface::side::Surface::side_diameter";
const OP_LENGTH: &str = "surface::side::Surface::length_across_side";

/// The most leaves [`Surface::length_across_side`] subdivides into.
const MAX_LEAVES: usize = 1 << 20;

/// One side of a parameter box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxSide {
    /// `u = umin`.
    UMin,
    /// `u = umax`.
    UMax,
    /// `v = vmin`.
    VMin,
    /// `v = vmax`.
    VMax,
}

impl BoxSide {
    /// Every side, in the order `UMin, UMax, VMin, VMax`.
    pub const ALL: [Self; 4] = [Self::UMin, Self::UMax, Self::VMin, Self::VMax];

    /// The side as the kernel's extension direction.
    #[must_use]
    pub const fn param(self) -> SurfParamType {
        match self {
            Self::UMin => SurfParamType::UMin,
            Self::UMax => SurfParamType::UMax,
            Self::VMin => SurfParamType::VMin,
            Self::VMax => SurfParamType::VMax,
        }
    }

    /// Whether the side is a `u = const` line (the box grows across it in u).
    #[must_use]
    pub const fn is_u(self) -> bool {
        matches!(self, Self::UMin | Self::UMax)
    }

    /// Whether the side is at the box's high end.
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, Self::UMax | Self::VMax)
    }

    /// The point of the side at `t` in `[0, 1]` along it.
    #[must_use]
    pub fn point(self, dom: &Extent2d, t: f64) -> Point2 {
        let (lo, hi) = (dom.min(), dom.max());
        let across = |a: f64, b: f64| a + t * (b - a);
        match self {
            Self::UMin => Point2::new(lo.x, across(lo.y, hi.y)),
            Self::UMax => Point2::new(hi.x, across(lo.y, hi.y)),
            Self::VMin => Point2::new(across(lo.x, hi.x), lo.y),
            Self::VMax => Point2::new(across(lo.x, hi.x), hi.y),
        }
    }

    /// The side's fixed parameter on `dom`.
    #[must_use]
    pub fn at(self, dom: &Extent2d) -> f64 {
        match self {
            Self::UMin => dom.min().x,
            Self::UMax => dom.max().x,
            Self::VMin => dom.min().y,
            Self::VMax => dom.max().y,
        }
    }
}

/// A length across a side ([`Surface::length_across_side`]): its certified enclosure, and
/// the enclosure's width split into its parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Across {
    /// The certified enclosure of the length.
    pub enclosure: Bracket,
    /// The truncation part: the leaves' control polygons less their chords, at most `rel` of
    /// the length when every leaf converged.
    pub truncation: f64,
    /// The rounding part: the sums' rounding and the leaves' points' (`gamma` of the chords
    /// and polygons, `2 (1 + p) N d`).
    pub rounding: f64,
    /// Whether every leaf met `rel`; a leaf too short to cut again before it did leaves the
    /// length not measured to `rel`.
    pub converged: bool,
    /// The stored net's own resolution of the length: how far the length can move when each
    /// stored coordinate of the points it is made of moves by one ulp (`8 p sqrt3 u M`, `M`
    /// their largest Euclidean coordinate: a point within `2 u |H|` and its weight within `2
    /// u w` moves by at most `4 sqrt3 u |E|`, the length by `2 p` times that). No net of
    /// floats places its length nearer than this to a length asked for; 0 for a closed form.
    /// Not part of the enclosure, which is the stored net's own length. It is a stop
    /// allowance only: stopping a length solve tighter than one ulp of the stored net can
    /// move the length is not resolvable, so the solve's stop is at least this wide; the
    /// certificate (the enclosure, `extend`'s `exact()`, the `1e-6 d`
    /// rounding criterion) is unchanged.
    pub resolution: f64,
}

/// A certified enclosure of a computed quantity: the exact value of the quantity, for the
/// surfaces as defined, lies in `[lower, upper]` (rounding included).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bracket {
    /// A proved lower bound.
    pub lower: f64,
    /// A proved upper bound.
    pub upper: f64,
}

impl Bracket {
    /// The midpoint, the value to report.
    #[must_use]
    pub fn mid(self) -> f64 {
        0.5 * (self.lower + self.upper)
    }

    /// `value` with a rounding bound `bound` either side.
    #[must_use]
    pub fn about(value: f64, bound: f64) -> Self {
        Self {
            lower: value - bound,
            upper: value + bound,
        }
    }
}

/// The column of the direction measured along (knots `along`, degree `dp`) that the iso
/// curve's point at `from` is, when `from` is a knot of multiplicity `dp` or more: the iso's
/// control point there is its point (the basis function of that column is 1 at `from`, every
/// other 0), taken on `to`'s side of a knot of multiplicity `dp + 1`. `None` otherwise.
fn joint_column(along: &[f64], dp: usize, from: f64, to: f64) -> Option<usize> {
    let mult = along.iter().filter(|&&x| x == from).count();
    if dp == 0 || mult < dp {
        return None;
    }
    if to > from {
        // `u_(s+1) = .. = u_(s+dp) = from`, the last copy `s + dp`
        along.iter().rposition(|&x| x == from)?.checked_sub(dp)
    } else {
        // the first copy `s + 1`
        along.iter().position(|&x| x == from)?.checked_sub(1)
    }
}

/// The net's points the iso curve's pieces over `[from, to]` at `fixed` are made of
/// (`[i along u][j along v]`), as [`Rounding::local`] reads them: the iso's extraction at
/// `fixed` is column by column (each across row's points combined at `fixed` alone), so
/// across only the rows whose basis function's support holds `fixed`; along, those whose
/// support meets `[from, to]` (closed: a support ending at an end is kept). Past a joint
/// ([`joint_column`], `s`) only the columns on `to`'s side of it, `s` itself included: the
/// Bezier decomposition starts the segment at a knot of multiplicity `p` or more from the
/// points `s .. s + p` as they are (no insertion there, `r = p - mult <= 0`), and the segment
/// ending at it needs no insertion at its end, so no piece on one side of the joint is
/// combined from a point on the other.
struct Window {
    along_u: bool,
    along: std::ops::RangeInclusive<usize>,
    held: std::ops::RangeInclusive<usize>,
    joint: Option<usize>,
    up: bool,
}

impl Window {
    fn of(
        nurb: &crate::nurbs::srf_basic::Surface,
        along_u: bool,
        fixed: f64,
        from: f64,
        to: f64,
    ) -> Self {
        let (p, q) = nurb.degrees();
        let (held, along, dp, dq) = if along_u {
            (nurb.v_knots(), nurb.u_knots(), p, q)
        } else {
            (nurb.u_knots(), nurb.v_knots(), q, p)
        };
        let (a, b) = (from.min(to), from.max(to));
        Self {
            along_u,
            along: support_meets(along, dp, a, b),
            held: support_meets(held, dq, fixed, fixed),
            joint: joint_column(along, dp, from, to),
            up: to > from,
        }
    }

    /// Whether the point `[i along u][j along v]` enters the pieces.
    fn keeps(&self, i: usize, j: usize) -> bool {
        let (ia, ih) = if self.along_u { (i, j) } else { (j, i) };
        let side = match self.joint {
            Some(s) if self.up => ia >= s,
            Some(s) => ia <= s,
            None => true,
        };
        self.along.contains(&ia) && side && self.held.contains(&ih)
    }
}

/// The column along the measured direction, among those `window` keeps, with the largest
/// weight in any kept row (the first of them; the window's first for a polynomial net).
fn heaviest_column(nurb: &crate::nurbs::srf_basic::Surface, window: &Window) -> usize {
    let mut best = (*window.along.start(), f64::NEG_INFINITY);
    for (i, row) in nurb.rows().enumerate() {
        for (j, c) in row.iter().enumerate() {
            if !window.keeps(i, j) {
                continue;
            }
            let w = if c.w == NOW { 1.0 } else { c.w };
            let column = if window.along_u { i } else { j };
            if w > best.1 || (w == best.1 && column < best.0) {
                best = (column, w);
            }
        }
    }
    best.0
}

/// [`Across::resolution`] of the points `window` keeps: `8 p sqrt3 u M` (`p` the measured
/// direction's degree, `M` the points' largest Euclidean coordinate), a few ulps wider for its
/// own rounding.
///
/// # Errors
///
/// `InvalidInput` for a weight not positive or a resolution not finite.
fn resolution_of(
    nurb: &crate::nurbs::srf_basic::Surface,
    window: &Window,
    op: &'static str,
) -> Result<f64> {
    let (p, q) = nurb.degrees();
    let dp = if window.along_u { p } else { q };
    let mut m = 0.0_f64;
    for (i, row) in nurb.rows().enumerate() {
        for (j, c) in row.iter().enumerate() {
            if !window.keeps(i, j) {
                continue;
            }
            let w = if c.w == NOW { 1.0 } else { c.w };
            if w.is_nan() || w <= 0.0 {
                return Err(SolidError::invalid_input(op, "a weight is not positive"));
            }
            m = m.max(c.x.abs().max(c.y.abs()).max(c.z.abs()) / w);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let res = 8.0 * dp as f64 * 3f64.sqrt() * crate::nurbs::math::UNIT * m * (1.0 + gamma(8));
    if res.is_finite() {
        Ok(res)
    } else {
        Err(SolidError::invalid_input(
            op,
            "the net's resolution is not bounded",
        ))
    }
}

/// `form`'s net in difference form about the column `s` along the measured direction: each
/// homogeneous point `(H, w)` (`H = w X`) made `(H - H_s - C (w - w_s), w)`, `H_s, w_s` the
/// point of column `s` in the same across row and `C` a fixed point; the weights kept. The
/// extraction and decomposition are linear in each coordinate and the B-spline basis sums
/// to 1, so the iso curve of the result at `fixed` is, in homogeneous form,
/// `H(t) - C w(t) - r` with `r = H_s(fixed) - C w_s(fixed)` (`H_s(fixed)` the iso's control
/// point of column `s`): the iso translated by `C`, less the constant `(r, 0)`. Its points'
/// magnitudes are the net's differences from column `s` (past a joint, the extension's own
/// size), not the part's size or position.
///
/// The rounding of making it, on each coordinate the points `keep` names enter with
/// (`fl(fl(H - H_s) - fl(C fl(w - w_s)))`, each operation exact to a relative `u` of its
/// result): at most `gamma_1 (|D~| + |dH~| + 2 |C| |dw~|)` from the computed difference
/// `D~`, homogeneous difference `dH~` and weight difference `dw~`. A polynomial net's
/// weights are all 1 (`dw = 0` exactly): the difference of its points alone.
///
/// # Errors
///
/// `SM_ERR` for a form without a NURBS net; those of the net.
fn differenced(
    form: &BSplineSurface,
    window: &Window,
    s: usize,
    c: Vec3,
    op: &'static str,
) -> Result<(BSplineSurface, f64)> {
    let nurb = form
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(op, "the B-spline surface has no NURB"))?;
    let cpts = nurb.cpts();
    let size = c.x.abs().max(c.y.abs()).max(c.z.abs());
    let g = gamma(2);
    let mut shift = 0.0_f64;
    let mut net: Vec<Vec<CPoint>> = Vec::new();
    for (i, row) in cpts.iter().enumerate() {
        let mut out = Vec::new();
        for (j, p) in row.iter().enumerate() {
            let (ri, rj) = if window.along_u { (s, j) } else { (i, s) };
            let r = cpts
                .get(ri)
                .and_then(|row| row.get(rj))
                .ok_or_else(|| SolidError::port_invariant(op, "a column past the net"))?;
            let (d, dh, dw) = if p.w == NOW || r.w == NOW {
                if p.w != r.w {
                    return Err(SolidError::invalid_input(
                        op,
                        "the net mixes rational and polynomial points",
                    ));
                }
                let dh = [p.x - r.x, p.y - r.y, p.z - r.z];
                (dh, dh, 0.0)
            } else {
                let dw = p.w - r.w;
                let dh = [p.x - r.x, p.y - r.y, p.z - r.z];
                (
                    [dh[0] - c.x * dw, dh[1] - c.y * dw, dh[2] - c.z * dw],
                    dh,
                    dw,
                )
            };
            if window.keeps(i, j) {
                for (x, y) in d.iter().zip(dh) {
                    shift = shift.max(g * (x.abs() + y.abs() + 2.0 * size * dw.abs()));
                }
            }
            out.push(CPoint::new(d[0], d[1], d[2], p.w));
        }
        net.push(out);
    }
    let (p, q) = nurb.degrees();
    let out = Net::new(net, p, q, nurb.u_knots().to_vec(), nurb.v_knots().to_vec())?;
    if !shift.is_finite() {
        return Err(SolidError::invalid_input(
            op,
            "the net's differences are not bounded",
        ));
    }
    Ok((BSplineSurface::from_gw_nurb(&out), shift))
}

/// A rational Bezier piece in homogeneous coordinates `(w x, w y, w z, w)`.
type Piece = Vec<[f64; 4]>;

/// The B-spline form a query on `s` measures, and its certified distance from `s` as
/// defined: 0 for a B-spline, a ball patch's net and an analytic kind's NURBS form (the
/// exact class, proved by [`AnalyticSupport::check`] first: a form off its label is
/// refused); a procedural blend's stored bound for its net.
fn form_of<'a>(
    k: &Kernel,
    s: &'a Surface,
    op: &'static str,
) -> Result<(Cow<'a, BSplineSurface>, f64)> {
    let bound = match s.kind() {
        SurfaceKind::ProceduralBlend(pb) => pb.def.bound,
        _ => 0.0,
    };
    // an analytic kind is measured on its NURBS form only once the form is proved on it
    if AnalyticSupport::of(s).is_some() {
        AnalyticSupport::check(k, s)?;
    }
    let form = crate::blend::fillet_section::station::as_b_spline(k, s)?
        .ok_or_else(|| SolidError::invalid_input(op, "the surface has no NURBS form"))?;
    Ok((form, bound))
}

/// How far a computed homogeneous point of a piece can be from the exact one, and from it
/// the Euclidean point's error: each de Casteljau or knot-insertion level is a combination
/// `(1 - a) x + a y` whose coefficient rounds relative to the knots it is made of
/// (`|da| <= gamma_3 (|t| + |u_i| + |u_j|) / (u_j - u_i)`) and whose result rounds by
/// `gamma_3`, so a level moves a coordinate by at most `c = gamma_3 (1 + 6 K / S) . 2 M`,
/// `K` the largest knot, `S` the least span, `M` the net's largest coordinate. `L` levels
/// move it by `L c`; the Euclidean point `H / w` then by
/// `sqrt3 (2 L c' + u) M_H w_max / w_min^2` (`c'` the relative part of `c`). The weight
/// coordinate is a combination of weights alone, so its move is the same with `M` the
/// largest weight ([`Rounding::weight`]): sizing it by the coordinates (lengths) would make
/// the Euclidean error `(dP - E dw) / w` grow as the square of the part's size.
#[derive(Debug, Clone, Copy)]
struct Rounding {
    /// The relative move of one level.
    level: f64,
    /// `M_H w_max / w_min^2`.
    scale: f64,
    /// `M_H`, the net's largest homogeneous point coordinate (`x, y, z`: a combination's
    /// coordinate is made of the same coordinate of its points alone, the weight's error is
    /// [`Rounding::weight`]'s).
    homogeneous: f64,
    /// `w_max`, the net's largest weight.
    weights: f64,
}

/// The largest knot magnitude and the least nonzero span of the knots of `kv` (the full
/// vector) from `margin` indices under the last knot below `a` to `margin` over the first
/// above `b`: what the knot insertions and the Bezier decomposition that make the pieces
/// over `[a, b]` read of `kv` (a segment's coefficients `(t - u_i) / (u_(i+p) - u_i)` take
/// `i` from its start group's last index, the one before it for its first points, to `p`
/// past it; the iso's extraction the knots within `q` of its span).
fn knot_window(kv: &[f64], a: f64, b: f64, margin: usize) -> (f64, f64) {
    let lo = kv
        .iter()
        .position(|&x| x > a)
        .unwrap_or(kv.len())
        .saturating_sub(margin);
    let hi = kv
        .iter()
        .rposition(|&x| x < b)
        .unwrap_or(0)
        .saturating_add(margin);
    let (mut kmax, mut smin) = (0.0_f64, f64::INFINITY);
    let window: Vec<f64> = kv
        .iter()
        .enumerate()
        .filter(|&(i, _)| i >= lo && i <= hi)
        .map(|(_, &x)| x)
        .collect();
    for (&x, &y) in window.iter().zip(window.iter().skip(1)) {
        kmax = kmax.max(x.abs()).max(y.abs());
        if y > x {
            smin = smin.min(y - x);
        }
    }
    (kmax, smin)
}

/// The largest knot magnitude and the least denominator of the iso's extraction at `t` of a
/// direction of degree `q` (full knot vector `kv`), as `extract_iso_crv` forms them: `t` in
/// its span `(u_s, u_(s+1)]` (`Side::Left`, of multiplicity `m`), each coefficient
/// `(t - u_(a+j)) / (u_(s+j+1) - u_(a+j))` with `a + j <= s - m` and `s + j + 1 >= s + 1`.
/// So the knots read are `u_(s-q+1) .. u_(s+q)` and every denominator spans `t`'s own span:
/// it is at least `u_(s+1) - u_s`, whatever shorter spans lie in the window (an extension's
/// sliver at the net's end is no denominator of an extraction in the middle). A `t` with no
/// span left of it is the net's start, extracted as its row (no coefficient: `INFINITY`).
fn extraction_window(kv: &[f64], t: f64, q: usize) -> (f64, f64) {
    let Some(s) = kv.iter().rposition(|&x| x < t) else {
        return (t.abs(), f64::INFINITY);
    };
    let (lo, hi) = (s.saturating_add(1).saturating_sub(q), s.saturating_add(q));
    let mut kmax = t.abs();
    for (i, &x) in kv.iter().enumerate() {
        if i >= lo && i <= hi {
            kmax = kmax.max(x.abs());
        }
    }
    let span = match (kv.get(s), kv.get(s.saturating_add(1))) {
        (Some(&a), Some(&b)) if b > a => b - a,
        // `t` past the last knot: the net's end, extracted as its row
        _ => f64::INFINITY,
    };
    (kmax, span)
}

/// [`knot_window`] for the Bezier decomposition along the pieces over `[a, b]` of a curve of
/// degree `p` (full knot vector `kv`): the decomposition forms a coefficient only where it
/// inserts, at an interior knot of multiplicity below `p`, and each of its denominators
/// `u_(i+j) - u_i` there is at least the length of the segment that knot ends. So the least
/// span read is the least such segment's in the window; a window whose interior knots are all
/// of multiplicity `p` or more (Bezier joints already) forms none (`INFINITY`).
fn decomposed_window(kv: &[f64], a: f64, b: f64, p: usize) -> (f64, f64) {
    let margin = p.saturating_add(2);
    let (kmax, _) = knot_window(kv, a, b, margin);
    let lo = kv
        .iter()
        .position(|&x| x > a)
        .unwrap_or(kv.len())
        .saturating_sub(margin);
    let hi = kv
        .iter()
        .rposition(|&x| x < b)
        .unwrap_or(0)
        .saturating_add(margin);
    // the distinct knots in the window with their multiplicities in the whole vector
    let mut groups: Vec<(f64, usize)> = Vec::new();
    for (i, &x) in kv.iter().enumerate() {
        match groups.last_mut() {
            Some((y, m)) if *y == x => *m = m.saturating_add(1),
            _ if i >= lo && i <= hi => groups.push((x, 1)),
            _ => {}
        }
    }
    let (first, last) = (kv.first().copied(), kv.last().copied());
    let mut smin = f64::INFINITY;
    for (&(x, _), &(y, my)) in groups.iter().zip(groups.iter().skip(1)) {
        let interior = Some(y) != first && Some(y) != last;
        if interior && my < p && y > x {
            smin = smin.min(y - x);
        }
    }
    (kmax, smin)
}

/// The control indices of degree `deg` on the knots `k` whose basis function's support
/// `[k_i, k_(i + deg + 1)]` meets `[lo, hi]` (closed: a support ending at `lo` is kept).
fn support_meets(k: &[f64], deg: usize, lo: f64, hi: f64) -> std::ops::RangeInclusive<usize> {
    let count = k.len().saturating_sub(deg + 1);
    let meets = |i: usize| matches!((k.get(i), k.get(i + deg + 1)), (Some(&a), Some(&b)) if a <= hi && b >= lo);
    let first = (0..count).find(|&i| meets(i));
    let last = (0..count).rev().find(|&i| meets(i));
    match (first, last) {
        (Some(f), Some(l)) => f..=l,
        // none: every point, as the whole net's bound
        _ => 0..=count.saturating_sub(1),
    }
}

impl Rounding {
    /// [`Rounding::of`] for the pieces of the iso at `fixed` (constant v for `along_u`)
    /// over `[from, to]` alone: the knots read are only those near them. Knot insertion and
    /// the Bezier decomposition are local: a segment's points are combinations whose
    /// coefficients take the knots from its start's segment's start to `p` past its end
    /// (`(t - u_i) / (u_(i+p) - u_i)`, i from the segment's start index), and the iso's
    /// extraction at `fixed` the knots within `q` of it. A span far from the pieces (a sliver
    /// at the net's other end) does not touch them.
    fn local(
        form: &BSplineSurface,
        window: &Window,
        fixed: f64,
        from: f64,
        to: f64,
        op: &'static str,
    ) -> Result<Self> {
        let nurb = form
            .gw_nurb()
            .ok_or_else(|| SolidError::surface_failed(op, "the B-spline surface has no NURB"))?;
        let (p, q) = nurb.degrees();
        let (held, along, dp, dq) = if window.along_u {
            (nurb.v_knots(), nurb.u_knots(), p, q)
        } else {
            (nurb.u_knots(), nurb.v_knots(), q, p)
        };
        let (a, b) = (from.min(to), from.max(to));
        // each direction's coefficients read its own knots and parameters
        let (k1, s1) = decomposed_window(along, a, b, dp);
        let (k2, s2) = extraction_window(held, fixed, dq);
        let ratio = (k1.max(from.abs()).max(to.abs()) / s1).max(k2 / s2);
        // and only the control points the pieces are made of ([`Window`]): a far row's
        // size, the other end of a long spine, or the source's past a joint, enters no piece
        Self::with_ratio_in(nurb, ratio, op, &|i, j| window.keeps(i, j))
    }

    fn of(form: &BSplineSurface, extra: &[f64], op: &'static str) -> Result<Self> {
        let nurb = form
            .gw_nurb()
            .ok_or_else(|| SolidError::surface_failed(op, "the B-spline surface has no NURB"))?;
        let (mut kmax, mut smin) = (0.0_f64, f64::INFINITY);
        for kv in [nurb.u_knots(), nurb.v_knots()] {
            for (&a, &b) in kv.iter().zip(kv.iter().skip(1)) {
                kmax = kmax.max(a.abs()).max(b.abs());
                if b > a {
                    smin = smin.min(b - a);
                }
            }
        }
        for &x in extra {
            kmax = kmax.max(x.abs());
        }
        Self::with_ratio(nurb, kmax / smin, op)
    }

    /// The rounding of a net whose coefficients' knots and parameters are at most `ratio`
    /// times the least span they divide by (`K / S`).
    fn with_ratio(
        nurb: &crate::nurbs::srf_basic::Surface,
        ratio: f64,
        op: &'static str,
    ) -> Result<Self> {
        Self::with_ratio_in(nurb, ratio, op, &|_, _| true)
    }

    /// [`Rounding::with_ratio`] over the control points `keep` names (`[i along u][j along
    /// v]`): the pieces' points are combinations of those alone.
    fn with_ratio_in(
        nurb: &crate::nurbs::srf_basic::Surface,
        ratio: f64,
        op: &'static str,
        keep: &dyn Fn(usize, usize) -> bool,
    ) -> Result<Self> {
        let (mut h, mut wmin, mut wmax) = (0.0_f64, f64::INFINITY, 0.0_f64);
        for (i, row) in nurb.rows().enumerate() {
            for c in row
                .iter()
                .enumerate()
                .filter(|&(j, _)| keep(i, j))
                .map(|(_, c)| c)
            {
                let w = if c.w == NOW { 1.0 } else { c.w };
                if w.is_nan() || w <= 0.0 {
                    return Err(SolidError::invalid_input(op, "a weight is not positive"));
                }
                h = h.max(c.x.abs()).max(c.y.abs()).max(c.z.abs());
                wmin = wmin.min(w);
                wmax = wmax.max(w);
            }
        }
        let level = gamma(3) * (1.0 + 6.0 * ratio) * 2.0;
        let scale = h * wmax / (wmin * wmin);
        if level.is_finite() && scale.is_finite() {
            Ok(Self {
                level,
                scale,
                homogeneous: h,
                weights: wmax,
            })
        } else {
            Err(SolidError::invalid_input(
                op,
                "the net's rounding is not bounded",
            ))
        }
    }

    /// The error of a homogeneous coordinate after `levels` levels.
    #[allow(clippy::cast_precision_loss)]
    fn homogeneous(self, levels: usize) -> f64 {
        (1.0 + 1e-12) * (2.0 * levels as f64 * self.level + f64::EPSILON) * self.homogeneous
    }

    /// The error of a weight after `levels` levels.
    #[allow(clippy::cast_precision_loss)]
    fn weight(self, levels: usize) -> f64 {
        (1.0 + 1e-12) * (2.0 * levels as f64 * self.level + f64::EPSILON) * self.weights
    }

    /// The Euclidean error of a point after `levels` levels.
    #[allow(clippy::cast_precision_loss)]
    fn point(self, levels: usize) -> f64 {
        3f64.sqrt() * (1.0 + 1e-12) * (2.0 * levels as f64 * self.level + f64::EPSILON) * self.scale
    }
}

fn euclid(h: [f64; 4]) -> Vec3 {
    let [x, y, z, w] = h;
    Vec3::new(x / w, y / w, z / w)
}

/// The rounding a rational Bezier piece's length carries, from its homogeneous points as
/// computed, each of `x, y, z` within `made` and the weight within `made_w` of the exact
/// piece's. A Euclidean point `E = P / w` moves by `(dP - E dw) / w`, so a point error
/// `e` and a weight error `e_w` move it by at most `(sqrt3 e + M e_w) / w_min` (`M` the
/// largest Euclidean point, `w_min` the least weight).
///
/// - **The leaves' cuts** (`cut`): `2 p` de Casteljau levels on the piece's points for
///   `t` in `[0, 1]`, each a convex combination rounding by `gamma_3` of the sum of the
///   magnitudes (at most `2 M_h` for a point's coordinate, `2 w_max` for a weight), so a
///   leaf's point is within `4 p gamma_4 M_h` and its weight within `4 p gamma_4 w_max` of
///   the computed piece's.
/// - **The piece moved** by `made`: the length of the computed piece is within
///   `int |C~' - C'|` of the exact one's. With `C' = (P' - C w') / w`,
///   `C~' - C' = (dP' - C~ dw' - dC w') / w~ - C' dw / w~`, integrated over `[0, 1]`:
///   `int |dP'| <= 2 p sqrt3 made` and `int |dw'| <= 2 p made_w` (the derivative's control
///   points are `p` times the points' differences, and each Bernstein basis function of
///   degree `p - 1` integrates to `1 / p`), `|dC| <= (sqrt3 made + M made_w) / w_min`,
///   `int |w'| <= sum |w_i+1 - w_i|` (variation diminishing; widened by `2 made_w` a
///   difference), `|dw| <= made_w`, and `int |C'|` is the exact piece's length `L`, at
///   most the computed piece's `L~` plus the move itself. So the move is at most
///   `(F + B L~) / (1 - B)`, `B = made_w / w_min` ([`PieceBounds::moved`]): the pointwise
///   rational-derivative bound `p (w_max / w_min)^2 max |E_i+1 - E_i|` is not needed.
#[derive(Debug, Clone, Copy)]
struct PieceBounds {
    /// The degree.
    p: f64,
    /// The Euclidean error of a leaf's point from its cut.
    cut: f64,
    /// The part `F` of the move not proportional to the piece's length.
    fixed: f64,
    /// The move per unit of the exact piece's length, `B`.
    per_length: f64,
    /// A bound on `int |w'| / w^2`, the variation of `1 / w`, over the exact piece: the least
    /// of its weights' variation (widened by `2 made_w` a difference) over the square of
    /// their least `w_lo` (less `made_w` and the cut's), and `p / w_lo`: `w'` is a polynomial
    /// of degree `p - 1`, so `w` has at most `p` monotone runs, each moving `1 / w` by less
    /// than `1 / w_lo`.
    inverse_weight: f64,
}

impl PieceBounds {
    /// `None` when a weight is not proved positive or a bound is not finite.
    fn of(piece: &[[f64; 4]], made: f64, made_w: f64) -> Option<Self> {
        let n = piece.len().checked_sub(1)?;
        #[allow(clippy::cast_precision_loss)]
        let p = n as f64;
        let (mut mh, mut m, mut wmin, mut wmax) = (0.0_f64, 0.0_f64, f64::INFINITY, 0.0_f64);
        for h in piece {
            let [x, y, z, w] = *h;
            mh = mh.max(x.abs()).max(y.abs()).max(z.abs());
            m = m.max(euclid(*h).length());
            wmin = wmin.min(w);
            wmax = wmax.max(w.abs());
        }
        let mut variation = 0.0_f64;
        for (a, b) in piece.iter().zip(piece.iter().skip(1)) {
            variation += (b[3] - a[3]).abs();
        }
        // a few ulps on each bound computed here
        let slack = 1.0 + gamma(16);
        let sqrt3 = 3f64.sqrt() * slack;
        let cut_h = 4.0 * p * gamma(4) * mh;
        let cut_w = 4.0 * p * gamma(4) * wmax;
        let low = wmin - made_w - cut_w;
        if low.is_nan() || low <= 0.0 {
            return None;
        }
        let cut = (sqrt3 * cut_h + m * cut_w) / low * slack;
        let point = (sqrt3 * made + m * made_w) / low * slack;
        let w1 = (variation + 2.0 * p * made_w) * slack;
        let fixed =
            (2.0 * p * sqrt3 * made + (m + point) * 2.0 * p * made_w + point * w1) / low * slack;
        let per_length = made_w / low * slack;
        let inverse_weight = (w1 / (low * low)).min(p / low) * slack;
        (cut.is_finite() && fixed.is_finite() && inverse_weight.is_finite() && per_length < 0.5)
            .then_some(Self {
                p,
                cut,
                fixed,
                per_length,
                inverse_weight,
            })
    }

    /// The bound on the length's move from the piece's rounding, `length` an upper bound
    /// of the computed piece's length: `(F + B length) / (1 - B)`.
    fn moved(self, length: f64) -> f64 {
        (self.fixed + self.per_length * length) / (1.0 - self.per_length) * (1.0 + gamma(4))
    }
}

/// The piece `[lo, hi]` of `[0, 1]` of a Bezier piece (de Casteljau at `hi`, then the left
/// part's right at `lo / hi`): `2 p` levels.
fn cut(piece: &[[f64; 4]], lo: f64, hi: f64) -> Piece {
    let split = |c: &[[f64; 4]], t: f64, left: bool| -> Piece {
        let mut level = c.to_vec();
        let (mut l, mut r) = (Vec::new(), Vec::new());
        while let (Some(&f), Some(&e)) = (level.first(), level.last()) {
            l.push(f);
            r.push(e);
            level = level
                .iter()
                .zip(level.iter().skip(1))
                .map(|(x, y)| {
                    let mut c = [0.0; 4];
                    for (ci, (xi, yi)) in c.iter_mut().zip(x.iter().zip(y)) {
                        *ci = xi * (1.0 - t) + yi * t;
                    }
                    c
                })
                .collect();
        }
        r.reverse();
        if left {
            l
        } else {
            r
        }
    };
    let mut c = if hi < 1.0 {
        split(piece, hi, true)
    } else {
        piece.to_vec()
    };
    if lo > 0.0 {
        c = split(&c, lo / hi, false);
    }
    c
}

/// The Bezier pieces of the iso curve of `form` at `fixed` (constant v when `along_u`, else
/// constant u) between the parameters `from` and `to`, in order from `from`, and how many
/// rounding levels made them.
fn iso_pieces(
    k: &Kernel,
    form: &BSplineSurface,
    along_u: bool,
    fixed: f64,
    from: f64,
    to: f64,
) -> Result<(Vec<Piece>, usize)> {
    let param = if along_u {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    // within the form's knots, exactly: the iso extraction clamps a parameter outside them
    // (by up to 1e-6) and the pieces would be cut to them, neither said
    let nurb = form
        .gw_nurb()
        .ok_or_else(|| SolidError::surface_failed(OP_LENGTH, "the B-spline surface has no NURB"))?;
    let (held, along) = if along_u {
        (nurb.v_knots(), nurb.u_knots())
    } else {
        (nurb.u_knots(), nurb.v_knots())
    };
    let inside = |x: f64, k: &[f64]| matches!((k.first(), k.last()), (Some(&a), Some(&b)) if x >= a && x <= b);
    if !(inside(fixed, held) && inside(from, along) && inside(to, along)) {
        return Err(SolidError::invalid_input(
            OP_LENGTH,
            "a parameter is outside the surface's knots",
        ));
    }
    let opts = IsoCurveOptions {
        domain: None,
        uv_iso_curve: false,
    };
    let iso = form
        .create_iso_parametric_curve(k, param, fixed, opts)?
        .curve;
    let nurb = iso
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::surface_failed(OP_LENGTH, "the iso curve has no NURB"))?;
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let mut out = Vec::new();
    let mut p = 0;
    for seg in nurb.decompose_bez()? {
        p = seg.degree();
        let (Some(&s0), Some(&s1)) = (seg.knots().first(), seg.knots().last()) else {
            continue;
        };
        let (lo, hi) = (a.max(s0), b.min(s1));
        if lo.partial_cmp(&hi) != Some(core::cmp::Ordering::Less) {
            continue;
        }
        let h: Piece = seg
            .cpts()
            .iter()
            .map(|c| {
                if c.w == NOW {
                    [c.x, c.y, c.z, 1.0]
                } else {
                    [c.x, c.y, c.z, c.w]
                }
            })
            .collect();
        out.push(cut(&h, (lo - s0) / (s1 - s0), (hi - s0) / (s1 - s0)));
    }
    if from > to {
        out.reverse();
        for piece in &mut out {
            piece.reverse();
        }
    }
    // the iso extraction (at most the fixed direction's degree + 1 levels), the
    // decomposition (p) and the cut (2 p)
    let q = form.degree(param);
    Ok((out, q + 1 + 3 * p))
}

impl Surface {
    /// The diameter of side `side` of the box `dom` (in the surface's NURBS parameters), as
    /// an enclosure: the side collapses to a point (a pole) within `tol` when `upper <= tol`,
    /// and does not when `lower > tol`.
    ///
    /// - **Upper:** the diameter of the control points of the side's iso curve (its Bezier
    ///   pieces cut to the side): a rational curve with positive weights lies in their hull.
    ///   At a pole the row collapses, so the bound is at rounding level.
    /// - **Lower:** the largest distance between the curve's points at the pieces' ends and
    ///   at the side's middle (SolidCraft's three points are among them).
    ///
    /// Both are widened by the rounding of making the pieces ([`Rounding`]) and, for a
    /// procedural blend measured on its net, twice its bound.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a surface without a NURBS form or with a weight not positive;
    /// those of the iso extraction and the decomposition.
    pub fn side_diameter(&self, k: &Kernel, dom: &Extent2d, side: BoxSide) -> Result<Bracket> {
        let (form, bound) = form_of(k, self, OP_DIAMETER)?;
        let (lo, hi) = (dom.min(), dom.max());
        let (from, to) = if side.is_u() {
            (lo.y, hi.y)
        } else {
            (lo.x, hi.x)
        };
        let mid = 0.5 * (from + to);
        let fixed = side.at(dom);
        let rounding = Rounding::of(&form, &[fixed, from, to], OP_DIAMETER)?;
        let mut pieces = Vec::new();
        let mut levels = 0;
        for (a, b) in [(from, mid), (mid, to)] {
            let (p, l) = iso_pieces(k, &form, !side.is_u(), fixed, a, b)?;
            pieces.extend(p);
            levels = levels.max(l);
        }
        let ends: Vec<Vec3> = pieces
            .iter()
            .flat_map(|p| [p.first(), p.last()])
            .flatten()
            .map(|h| euclid(*h))
            .collect();
        let hull: Vec<Vec3> = pieces.iter().flatten().map(|h| euclid(*h)).collect();
        if hull.is_empty() {
            return Err(SolidError::invalid_input(
                OP_DIAMETER,
                "the side has no extent",
            ));
        }
        let diameter = |pts: &[Vec3]| {
            let mut d = 0.0_f64;
            for (i, a) in pts.iter().enumerate() {
                for b in pts.iter().skip(i + 1) {
                    d = d.max(a.distance(*b));
                }
            }
            d
        };
        let e = 2.0 * rounding.point(levels) + 2.0 * bound;
        let (dl, du) = (diameter(&ends), diameter(&hull));
        Ok(Bracket {
            lower: (dl * (1.0 - gamma(6)) - e).max(0.0),
            upper: du * (1.0 + gamma(6)) + e,
        })
    }

    /// The length of the iso curve through the middle of side `side` of the box `dom` (in
    /// the surface's NURBS parameters), from the side across it to the end of the
    /// surface's natural domain: how far an extended surface runs past the side its face
    /// had. As an enclosure whose width is about `rel` times the length, plus rounding.
    ///
    /// **Method.** The curve's Bezier pieces are subdivided (each leaf cut from its piece
    /// directly, so the cuts' rounding does not accumulate with depth) until on every leaf
    /// the control polygon's length `q` and the chord `c` differ by at most `rel c`. The
    /// leaf's arc length `s` satisfies `c <= s <= q`: the chord is the shortest path, and a
    /// rational Bezier curve with positive weights crosses any plane no more often than its
    /// control polygon (variation diminishing), so by the Crofton formula it is no longer
    /// than the polygon.
    ///
    /// **Convergence.** On a leaf of parameter length `h` the gap `q - c` is `O(h^2)` times
    /// the leaf's length (for a circular arc of angle `t`, about `t^2 / 8` of it), so
    /// halving a leaf divides it by about 4: a quarter circle needs about `0.6 / sqrt(rel)`
    /// leaves. More than [`MAX_LEAVES`] is refused.
    ///
    /// **Rounding**, in two parts. The leaves enclose the length of the pieces as computed:
    /// each leaf's points are within its cut's rounding of that piece's (`2 p` de Casteljau
    /// levels, sized on the piece's own points), so its chord is within twice that and its
    /// polygon within `2 p` times it, and each distance rounds by `gamma_6`; over the leaves,
    /// `gamma_(N + 6 + p)` of the sums. The pieces' points are within the iso extraction's and
    /// decomposition's rounding of the exact curve's ([`Rounding`]), and the length moves by
    /// at most the integral of the derivative's change, bounded per piece in closed form
    /// ([`PieceBounds`]): a cost per piece, not per leaf, so a finer `rel` costs only the
    /// leaves' cuts.
    ///
    /// **Sized on the length, not the part.** The curve is measured on the net in difference
    /// form about one column ([`differenced`]: the joint's when the side is a knot of
    /// multiplicity `p`, an extension's joint; else the heaviest the pieces use), so every
    /// magnitude the rounding is sized by is the measured span's own: past a joint only the
    /// columns on its far side enter the pieces ([`Window`]), each `w_i (P_i - P_s)`. The
    /// level's coefficient bound reads, across, the span holding the iso's parameter (every
    /// extraction denominator spans it), not the window's least, and a point coordinate's
    /// rounding is sized by the point coordinates alone. On a rational net the measured curve
    /// is the iso less `r / w(t)` (`|r|` bounded from the reference point's own extraction):
    /// its length moves by at most `|r| int |w'| / w^2 <= |r| min(TV(w) / w_lo^2, p / w_lo)`
    /// a piece. So for an extension of length `L` from a joint the rounding is about
    /// `c(p) u L` (measured `2e-12 L` to `7e-12 L` on sides 170 from the origin, for `L` from
    /// `1e-5` to `1e-3`; a high-precision reference length is within the enclosure, its
    /// middle `1e-19` to `1e-14` off), where it was `c(p) u` times the part's size.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `rel` not in `(0, 1)`, a procedural blend (its net's length
    /// bounds nothing of its definition's), a surface without a NURBS form or with a
    /// weight not positive; `NotConverged` past [`MAX_LEAVES`]; those of the iso
    /// extraction and the decomposition.
    pub fn length_across_side(
        &self,
        k: &Kernel,
        dom: &Extent2d,
        side: BoxSide,
        rel: f64,
    ) -> Result<Across> {
        if !(rel > 0.0 && rel < 1.0) {
            return Err(SolidError::invalid_input(OP_LENGTH, "rel is not in (0, 1)"));
        }
        if matches!(self.kind(), SurfaceKind::ProceduralBlend(_)) {
            return Err(SolidError::invalid_input(
                OP_LENGTH,
                "a blend's net bounds no length of its definition",
            ));
        }
        let (form, _) = form_of(k, self, OP_LENGTH)?;
        let natural = self.natural_uv_domain_with_nurb(k)?;
        let middle = side.point(dom, 0.5);
        let (fixed, from) = if side.is_u() {
            (middle.y, middle.x)
        } else {
            (middle.x, middle.y)
        };
        let to = match side {
            BoxSide::UMin => natural.min().x,
            BoxSide::UMax => natural.max().x,
            BoxSide::VMin => natural.min().y,
            BoxSide::VMax => natural.max().y,
        };
        if from == to {
            return Ok(Across {
                enclosure: Bracket {
                    lower: 0.0,
                    upper: 0.0,
                },
                truncation: 0.0,
                rounding: 0.0,
                converged: true,
                resolution: 0.0,
            });
        }
        let along_u = side.is_u();
        let nurb = form.gw_nurb().ok_or_else(|| {
            SolidError::surface_failed(OP_LENGTH, "the B-spline surface has no NURB")
        })?;
        let window = Window::of(nurb, along_u, fixed, from, to);
        let rational = nurb.is_rational();
        let resolution = resolution_of(nurb, &window, OP_LENGTH)?;
        // the reference column: the joint's (its iso point the curve's at `from`), else the
        // one of the largest weight among those the pieces are made of (a difference is about
        // `w_i (P_i - P_s)`, so the heaviest column's term is the one taken out)
        let s = match window.joint {
            Some(s) => s,
            None => heaviest_column(nurb, &window),
        };
        // `C`: the iso's control point of column `s` as computed (a rational net's; a
        // polynomial net's differences do not read it), and a bound on `|r|`
        // ([`differenced`])
        let (c, r) = if rational {
            let rounding = Rounding::local(&form, &window, fixed, from, to, OP_LENGTH)?;
            let param = if along_u {
                SurfParamType::V
            } else {
                SurfParamType::U
            };
            let opts = IsoCurveOptions {
                domain: None,
                uv_iso_curve: false,
            };
            let iso = form
                .create_iso_parametric_curve(k, param, fixed, opts)?
                .curve;
            let cpts = iso
                .nurb
                .as_ref()
                .ok_or_else(|| SolidError::surface_failed(OP_LENGTH, "the iso curve has no NURB"))?
                .cpts();
            let columns = nurb.cpts().len();
            let columns = if along_u {
                columns
            } else {
                nurb.cpts().first().map_or(0, Vec::len)
            };
            if cpts.len() != columns {
                return Err(SolidError::port_invariant(
                    OP_LENGTH,
                    "the iso curve is not one point a column",
                ));
            }
            let h = cpts.get(s).ok_or_else(|| {
                SolidError::port_invariant(OP_LENGTH, "the reference column past the iso")
            })?;
            let w = if h.w == NOW { 1.0 } else { h.w };
            let c = Vec3::new(h.x / w, h.y / w, h.z / w);
            // `r = H_s - C w_s` (the exact control point, homogeneous): with the computed
            // `H~ = H + dH`, `w~ = w + dw` and `C = (H~ / w~)(1 + theta)` a coordinate,
            // `r = -theta H~ - dH + C dw`, so each coordinate is within `u |H~| + |dH| + |C|
            // |dw|`: the extraction's levels (at most the across degree) on the points and
            // on the weights
            let levels = form.degree(param) + 1;
            let hmax = h.x.abs().max(h.y.abs()).max(h.z.abs());
            let cmax = c.x.abs().max(c.y.abs()).max(c.z.abs());
            let r = 3f64.sqrt()
                * (crate::nurbs::math::UNIT * hmax
                    + rounding.homogeneous(levels)
                    + cmax * rounding.weight(levels))
                * (1.0 + gamma(4));
            (c, r)
        } else {
            (Vec3::new(0.0, 0.0, 0.0), 0.0)
        };
        if !(r.is_finite() && c.x.is_finite() && c.y.is_finite() && c.z.is_finite()) {
            return Err(SolidError::invalid_input(
                OP_LENGTH,
                "the iso's reference point is not bounded",
            ));
        }
        let (form, shift) = differenced(&form, &window, s, c, OP_LENGTH)?;
        let rounding = Rounding::local(&form, &window, fixed, from, to, OP_LENGTH)?;
        let (pieces, levels) = iso_pieces(k, &form, along_u, fixed, from, to)?;
        // the pieces' homogeneous points as computed, within this of the exact ones (of the
        // net differenced exactly), the differences' rounding carried through the levels'
        // convex combinations unchanged
        let made = rounding.homogeneous(levels) + shift;
        // the weights are not differenced: their own levels' rounding alone
        let made_w = rounding.weight(levels);
        let (mut chords, mut polygons, mut leaves) = (0.0_f64, 0.0_f64, 0_usize);
        let (mut chord_cuts, mut polygon_cuts, mut moved) = (0.0_f64, 0.0_f64, 0.0_f64);
        let mut offset = 0.0_f64;
        let mut converged = true;
        for piece in &pieces {
            // a weight not proved positive past the rounding: no measurement
            let Some(bounds) = PieceBounds::of(piece, made, made_w) else {
                return Ok(Across {
                    enclosure: Bracket {
                        lower: 0.0,
                        upper: f64::INFINITY,
                    },
                    truncation: f64::INFINITY,
                    rounding: f64::INFINITY,
                    converged: false,
                    resolution: f64::INFINITY,
                });
            };
            let mut n = 0_usize;
            let mut piece_polygons = 0.0_f64;
            let mut work = vec![(0.0_f64, 1.0_f64)];
            while let Some((a, b)) = work.pop() {
                let leaf = cut(piece, a, b);
                let pts: Vec<Vec3> = leaf.iter().map(|h| euclid(*h)).collect();
                let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
                    continue;
                };
                let c = first.distance(*last);
                let q: f64 = pts
                    .iter()
                    .zip(pts.iter().skip(1))
                    .map(|(a, b)| a.distance(*b))
                    .sum();
                if q - c > rel * c {
                    if b - a > f64::EPSILON {
                        let m = 0.5 * (a + b);
                        work.push((m, b));
                        work.push((a, m));
                        continue;
                    }
                    // a leaf too short to cut again before it met `rel`
                    converged = false;
                }
                leaves += 1;
                n += 1;
                if leaves > MAX_LEAVES {
                    return Err(SolidError::NotConverged {
                        operation: OP_LENGTH,
                        iterations: u32::try_from(MAX_LEAVES).unwrap_or(u32::MAX),
                    });
                }
                chords += c;
                polygons += q;
                piece_polygons += q;
            }
            let g_piece = gamma(n + 6 + piece.len());
            #[allow(clippy::cast_precision_loss)]
            let n = n as f64;
            // each leaf's chord within 2 of its cut's point error, its polygon within 2 p
            chord_cuts += n * 2.0 * bounds.cut;
            polygon_cuts += n * 2.0 * bounds.p * bounds.cut;
            // the computed piece's length is at most its leaves' polygons, with their
            // rounding and cuts
            moved +=
                bounds.moved(piece_polygons * (1.0 + g_piece) + n * 2.0 * bounds.p * bounds.cut);
            // the measured curve is the iso translated by `C` less `r / w(t)`
            // ([`differenced`]): their lengths differ by at most `|r| int |w'| / w^2` (none
            // on a polynomial net, `w = 1`)
            if rational {
                offset += r * bounds.inverse_weight;
            }
        }
        let g = gamma(leaves + 6 + pieces.iter().map(Vec::len).max().unwrap_or(0));
        // the leaves enclose the length of the pieces as computed; the exact curve's is within
        // `moved` of it (the pieces' points' rounding, through their derivatives)
        let moved = moved + offset * (1.0 + gamma(4));
        let rounding = g * (chords + polygons) + chord_cuts + polygon_cuts + 2.0 * moved;
        Ok(Across {
            enclosure: Bracket {
                lower: (chords * (1.0 - g) - chord_cuts - moved).max(0.0),
                upper: polygons * (1.0 + g) + polygon_cuts + moved,
            },
            truncation: polygons - chords,
            rounding,
            converged,
            resolution,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::cone::Cone;
    use crate::surface::sphere::{Sphere, SphereOptions};
    use std::f64::consts::{PI, SQRT_2};

    fn k() -> Kernel {
        Kernel::default()
    }

    fn sphere(r: f64, lon: [f64; 2], lat: [f64; 2]) -> Surface {
        let opts = SphereOptions {
            swap_uv: false,
            inside_out: false,
            circle_nurb: None,
        };
        let anal = Extent2d::from_bounds(lon[0], lat[0], lon[1], lat[1]);
        Surface::from(
            Sphere::new(
                &k(),
                Vec3::new(0.5, 1.0, -2.0),
                Vec3::X,
                Vec3::Y,
                anal,
                r,
                opts,
            )
            .unwrap(),
        )
    }

    /// A cylinder of radius `r` about z through (1, 2, 3), over the STEP box.
    fn cylinder(r: f64, a: [f64; 2], h: [f64; 2]) -> Cone {
        let k = k();
        let pl = Axis2Placement::new(Vec3::new(1.0, 2.0, 3.0), Vec3::X, Vec3::Y);
        let mut c = Cone::create_canonical(&k, &pl, r, r, 1.0, false, false).unwrap();
        c.adjust_step_uv_domain(&k, &Extent2d::from_bounds(a[0], h[0], a[1], h[1]))
            .unwrap();
        c
    }

    /// The NURBS box of the STEP box `[a0, a1] x [h0, h1]` on `c`.
    fn nurbs_box(c: &Cone, a: [f64; 2], h: [f64; 2]) -> Extent2d {
        let k = k();
        let p = c
            .convert_uv_from_step_to_nurbs(&k, Point2::new(a[0], h[0]))
            .unwrap();
        let q = c
            .convert_uv_from_step_to_nurbs(&k, Point2::new(a[1], h[1]))
            .unwrap();
        Extent2d::from_bounds(p.x.min(q.x), p.y.min(q.y), p.x.max(q.x), p.y.max(q.y))
    }

    /// The side of `dom` whose STEP angle (or height, `in_height`) is `value` along it.
    fn side_at(c: &Cone, dom: &Extent2d, in_height: bool, value: f64) -> BoxSide {
        let k = k();
        BoxSide::ALL
            .into_iter()
            .find(|s| {
                [0.0, 1.0].iter().all(|&t| {
                    let p = c
                        .convert_uv_from_nurbs_to_step(&k, s.point(dom, t))
                        .unwrap();
                    ((if in_height { p.y } else { p.x }) - value).abs() < 1e-9
                })
            })
            .unwrap()
    }

    fn holds(b: Bracket, x: f64) {
        assert!(b.lower <= x && x <= b.upper, "{b:?} does not hold {x}");
    }

    #[test]
    fn a_pole_is_a_point_and_the_equator_its_chord() {
        let s = sphere(2.0, [0.0, 90.0], [0.0, 90.0]);
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let d: Vec<Bracket> = BoxSide::ALL
            .iter()
            .map(|&side| s.side_diameter(&k(), &dom, side).unwrap())
            .collect();
        // one side is the pole: proved a point at rounding level
        assert_eq!(d.iter().filter(|b| b.upper < 1e-10).count(), 1);
        // the equator's quarter: its chord r sqrt2, which is its hull's diameter too
        let eq = d
            .iter()
            .filter(|b| (b.mid() - 2.0 * SQRT_2).abs() < 1e-9)
            .count();
        assert!(eq >= 1, "{d:?}");
        for b in &d {
            assert!(b.upper - b.lower < 1e-10, "{b:?}");
        }
    }

    #[test]
    fn a_half_circle_is_enclosed_between_its_chord_and_its_hull() {
        // the equator over a half turn: diameter 2r; the hull of its two quarter arcs is
        // sqrt5 r across
        let s = sphere(1.5, [0.0, 180.0], [0.0, 45.0]);
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let b = BoxSide::ALL
            .iter()
            .map(|&side| s.side_diameter(&k(), &dom, side).unwrap())
            .find(|b| (b.lower - 3.0).abs() < 1e-9)
            .unwrap();
        holds(b, 3.0);
        assert!((b.upper - 1.5 * 5f64.sqrt()).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn across_a_cylinder_round_and_along_its_axis() {
        let c = cylinder(2.0, [0.0, 180.0], [0.0, 5.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 3.0]);
        let s = Surface::from(c.clone());
        // round: from angle 90 to 180, a quarter of radius 2
        let side = side_at(&c, &dom, false, 90.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-8)
            .unwrap()
            .enclosure;
        holds(b, PI);
        // the leaves' share and the rounding's at part scale (coordinates about 3)
        assert!(b.upper - b.lower < 1e-6, "{b:?}");
        // along: from height 3 to 5
        let side = side_at(&c, &dom, true, 3.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-10)
            .unwrap()
            .enclosure;
        holds(b, 2.0);
        assert!(b.upper - b.lower < 1e-10, "{b:?}");
        // the low height side is the domain's own end: nothing across
        let side = side_at(&c, &dom, true, 0.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-10)
            .unwrap()
            .enclosure;
        assert_eq!((b.lower, b.upper), (0.0, 0.0));
    }

    #[test]
    fn a_coarser_ask_gives_a_wider_enclosure() {
        let c = cylinder(1.0, [0.0, 270.0], [0.0, 1.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 1.0]);
        let s = Surface::from(c.clone());
        let side = side_at(&c, &dom, false, 90.0);
        let fine = s
            .length_across_side(&k(), &dom, side, 1e-8)
            .unwrap()
            .enclosure;
        let coarse = s
            .length_across_side(&k(), &dom, side, 1e-4)
            .unwrap()
            .enclosure;
        holds(fine, PI);
        holds(coarse, PI);
        assert!(coarse.upper - coarse.lower > fine.upper - fine.lower);
        assert!(s.length_across_side(&k(), &dom, side, 0.0).is_err());
    }

    #[test]
    fn a_box_past_the_knots_is_refused_not_clamped() {
        let c = cylinder(2.0, [0.0, 90.0], [0.0, 3.0]);
        let s = Surface::from(c.clone());
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let past = Extent2d::from_bounds(lo.x, lo.y, hi.x + 1e-7, hi.y);
        assert!(s.side_diameter(&k(), &past, BoxSide::UMax).is_err());
        assert!(s.side_diameter(&k(), &past, BoxSide::VMin).is_err());
        assert!(s
            .length_across_side(&k(), &past, BoxSide::UMax, 1e-8)
            .is_err());
        assert!(s.side_diameter(&k(), &dom, BoxSide::UMax).is_ok());
    }

    #[test]
    fn a_form_moved_off_its_label_is_refused() {
        let mut c = cylinder(2.0, [0.0, 90.0], [0.0, 3.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 3.0]);
        let shift = Axis2Placement::new(Vec3::new(1e-6, 0.0, 0.0), Vec3::X, Vec3::Y);
        c.rev.base.transform(&shift, None).unwrap();
        let s = Surface::from(c);
        assert!(s.side_diameter(&k(), &dom, BoxSide::UMin).is_err());
        assert!(s
            .length_across_side(&k(), &dom, BoxSide::UMin, 1e-9)
            .is_err());
    }

    /// A rational quadratic `(0, 0)`, `(1, 1)`, `(2, 0)` with the middle weight `w`, swept
    /// 1 along z, all scaled by `scale`.
    fn conic(w: f64, scale: f64) -> Surface {
        use crate::geom::vector3d::Point3;
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        let mut pts = Vec::new();
        for p in [[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]] {
            for z in [0.0, 1.0] {
                pts.push(Point3::new(p[0] * scale, p[1] * scale, z * scale));
            }
        }
        let weights = [1.0, 1.0, w, w, 1.0, 1.0];
        Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 2,
                v_degree: 1,
                control_points: &pts,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[3, 3],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&weights),
                uv_domain: None,
            })
            .unwrap(),
        )
    }

    /// The length's rounding grows with the part's size, not its square (the weights'
    /// rounding is sized by the weights, not by the coordinates), and not with the square
    /// of the weight ratio (the derivative's move is integrated as the piece's own length):
    /// the same conic at 1000 times the size rounds 1000 times as much, and its enclosure
    /// is the small one's scaled.
    #[test]
    fn the_rounding_scales_with_the_part_not_its_square() {
        let dom = Extent2d::from_bounds(0.0, 0.0, 0.25, 1.0);
        for w in [1.0, 10.0, 100.0] {
            let small = conic(w, 1.0)
                .length_across_side(&k(), &dom, BoxSide::UMax, 1e-9)
                .unwrap();
            let large = conic(w, 1000.0)
                .length_across_side(&k(), &dom, BoxSide::UMax, 1e-9)
                .unwrap();
            assert!(
                large.rounding <= 1001.0 * small.rounding,
                "w {w}: {} at 1000 against {} at 1",
                large.rounding,
                small.rounding
            );
            // relative to the length, at part scale
            assert!(
                small.rounding <= 1e-7 * small.enclosure.lower,
                "w {w}: {small:?}"
            );
            let (a, b) = (small.enclosure, large.enclosure);
            assert!(
                b.lower <= 1000.0 * a.upper && 1000.0 * a.lower <= b.upper,
                "w {w}: {a:?} {b:?}"
            );
        }
    }
}
