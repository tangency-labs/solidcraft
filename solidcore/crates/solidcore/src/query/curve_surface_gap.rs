//! A proved bound on how far a curve lies from a face's surface, over the whole curve (not
//! upstream, tangency-labs/solidcraft#16508): what a construction that fits a surface to its
//! input curves records as its edges' tolerance, so the B-rep states its real construction
//! error (a sampled distance is a lower estimate of the largest one,
//! `docs/validity-and-tolerances.md` section 8).
//!
//! The curve `C(t)` is a B-spline, the surface `S` a NURBS net, and `W(t)` a B-spline of
//! parameters `(u, v, 0)` on that net, a correspondence chosen only to be close to the nearest
//! point (it is built through the nearest points of the curve's own samples). Whatever `W` is,
//! `dist(C(t), S) <= |g(t)|` for `g = C - S(W)`, because `S(W(t))` is a point of `S`. On a
//! piece `[a, b]` of the curve, `g` is within `h^2 / 8 * sup |g''|` of the chord between its two
//! end values (`h = b - a`), so
//!
//! `max over [a, b] of dist(C, S) <= max(|g(a)|, |g(b)|) + h^2 / 8 * sup |g''|`.
//!
//! The end values and `sup |g''|` are enclosed in outward-rounded interval arithmetic: the
//! curve's and `W`'s derivatives over the piece ([`curve_derivs_over`]), the surface's partials
//! over the box `W` sweeps there (`NetSurface::partials`, which cuts the box at the knot lines
//! it crosses, each part on its own polynomial piece, and refuses one crossed where the surface
//! is not continuous enough for the order asked). Because
//! `C` and `S(W)` nearly coincide, `g''` is small and the second term shrinks with the fourth
//! power of the piece, so the pieces are bisected until it no longer matters. A piece whose box
//! crosses a knot line the surface is only C^0 across (a patch corner the curve ends on) has no
//! second derivative to bound; there `g` is Lipschitz, and the piece is bounded by its larger
//! end value plus `h * sup |g'|` over the one-sided partials, bisected the same way.
//!
//! A surface that is not a NURBS net (or an analytic surface's exact net), a net whose
//! parameters are not the surface's, a box past the net's end where the surface does not
//! continue, and a piece the bisection cannot close all give `None`: the gap is then not
//! enclosed.

use core::cell::{Cell, RefCell};
use std::collections::{btree_map::Entry, BTreeMap, VecDeque};
use std::sync::Arc;

use crate::blend::fillet_section::certificate::{
    below_continuity, curve_derivs_over, curve_derivs_over_about, knot_cuts,
    nurbs_partials_over_piece, partials_hull, past_one_low_line, EndContinuation, NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::offset_box::offset_and_base_partials_over_piece;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{
    as_b_spline, knot, nurbs_partials, span, Nurbs, Partials,
};
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;
use crate::{Result, SolidError};

mod cone_residual;
pub(crate) mod polynomial_pieces;

/// A direction's knot samples (parameter, spacing, span index) and its slack.
/// A direction's pieces of a box, its overshoot, and the part past a knot line taken as an
/// overshoot with the span the surface truly has there (`NetSurface::partials`).
type KnotSamples = (Vec<(f64, f64, usize)>, f64, Option<(f64, f64, usize)>);
/// A side of a control net: whether it runs across u, whether at the max, and its points'
/// indices.
type Edge = (bool, bool, Vec<(usize, usize)>);

const OP: &str = "query::curve_surface_gap";

/// The most pieces the bisection may make before the gap is called not enclosed.
pub(crate) const MAX_PIECES: usize = 1 << 15;
/// The deepest bisection of one piece.
const MAX_DEPTH: u32 = 30;
/// A piece is closed once its second-order term is at most this fraction of its end values.
pub(crate) const CLOSE_RELATIVE: f64 = 1.0e-3;
/// ... or at most this absolute amount, whatever the caller's floor: below any tolerance a
/// model carries.
const CLOSE_ABSOLUTE: f64 = 1.0e-13;
/// Points of the curve the correspondence `W` passes through (their nearest points on the
/// surface): only how close `W` stays to the nearest point, so how tight the bound is, depends
/// on it, never whether it holds.
const CORRESPONDENCE_POINTS: usize = 257;
/// Pieces the bisection starts from.
const FIRST_PIECES: usize = 32;
/// The most pieces the identity-correspondence fast path in [`bound_trim_pair`] may spend
/// before falling back to [`reparametrisation`]: two full halvings of [`FIRST_PIECES`]
/// (`32 + 64 + 128 = 224`) fit within `256`.
const IDENTITY_FAST_PIECES: usize = 256;
/// The most trim bounds a [`TrimBoundMemo`] keeps; the oldest goes first.
const TRIM_MEMO_CAPACITY: usize = 4096;
/// The largest trim pair (stored `f64` values across curve, trim and net) kept in a
/// [`TrimBoundMemo`].
const TRIM_MEMO_MAX_FLOATS: usize = 8192;
/// The most retained key bytes a [`TrimBoundMemo`] holds across all entries (16 MiB);
/// the oldest entry is evicted first whenever an insert would pass this budget.
const TRIM_MEMO_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Outcome of a certified trim-image gap bound ([`MovedTrimPair::bound_before`],
/// [`MovedTrimPair::bound_after`]): distinguishes a budget-stopped bisection whose evaluated
/// endpoints stayed within `enough` from a gap proved past `over` or structurally unenclosed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TrimBoundOutcome {
    /// A proved finite upper bound at most `over`.
    Proved(f64),
    /// Proved to exceed `over`.
    ExceedsOver,
    /// Stopped by the caller's piece `budget` while every evaluated endpoint stayed within
    /// `enough`: the true gap may still lie within `over`.
    BudgetStopped,
    /// Not enclosed (not due to the piece `budget`).
    Unproved,
}

/// Exact bit-for-bit identity of a [`bound_trim_pair`] invocation over `range`,
/// preceded by a deterministic FNV-1a pre-hash so ordered-map lookups compare a single
/// `u64` first while equality stays exact bit for bit. Stored once behind an [`Arc`] in
/// [`TrimBoundMemo`].
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TrimBoundKey {
    hash: u64,
    bits: Vec<u64>,
}

impl TrimBoundKey {
    /// Approximate heap and inline bytes retained by this key.
    fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.bits.capacity().saturating_mul(size_of::<u64>()))
    }
}

/// Not upstream (#22467): exact-input memo of [`bound_trim_pair`] outcomes, keyed by the exact
/// content of the pair's data and bound parameters ([`TrimBoundKey`]). Each key is stored once
/// behind an [`Arc`] and retained memory is bounded by both [`TRIM_MEMO_CAPACITY`] and
/// [`TRIM_MEMO_MAX_BYTES`]. Clearing it changes no result, only speed.
#[derive(Debug, Default)]
pub(crate) struct TrimBoundMemo {
    bounds: BTreeMap<Arc<TrimBoundKey>, TrimBoundOutcome>,
    order: VecDeque<Arc<TrimBoundKey>>,
    retained_bytes: usize,
}

impl TrimBoundMemo {
    /// The exact outcome previously computed for `key`, if any.
    pub(crate) fn get(&self, key: &TrimBoundKey) -> Option<TrimBoundOutcome> {
        self.bounds.get(key).copied()
    }

    /// Number of cached trim outcomes (tests).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.bounds.len()
    }

    /// Total key bytes retained by the memo (tests).
    #[cfg(test)]
    pub(crate) const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Records `outcome` for `key`, storing the key once and evicting the oldest entries past
    /// [`TRIM_MEMO_CAPACITY`] or [`TRIM_MEMO_MAX_BYTES`].
    pub(crate) fn insert(&mut self, key: TrimBoundKey, outcome: TrimBoundOutcome) {
        if let Some(existing) = self.bounds.get_mut(&key) {
            *existing = outcome;
            return;
        }
        let entry_bytes = key.retained_bytes();
        if entry_bytes > TRIM_MEMO_MAX_BYTES {
            return;
        }
        while self.order.len() >= TRIM_MEMO_CAPACITY
            || self.retained_bytes.saturating_add(entry_bytes) > TRIM_MEMO_MAX_BYTES
        {
            match self.order.pop_front() {
                Some(old) => {
                    if self.bounds.remove(&old).is_some() {
                        self.retained_bytes =
                            self.retained_bytes.saturating_sub(old.retained_bytes());
                    }
                }
                None => break,
            }
        }
        let key = Arc::new(key);
        self.retained_bytes = self.retained_bytes.saturating_add(entry_bytes);
        self.order.push_back(Arc::clone(&key));
        self.bounds.insert(key, outcome);
    }
}

/// Returns packed evaluation flags when `(surface, curve, uv)` all evaluate directly from the
/// exact NURBS data carried in [`TrimPair`], so [`bound_trim_pair`] depends on nothing outside
/// [`TrimBoundKey`].
fn pure_nurbs_flags(surface: &Surface, curve: &Curve, uv: &Curve) -> Option<u64> {
    use crate::curve::curve::CurveKind;
    use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
    use crate::surface::surface::SurfaceKind;

    let s_base = match surface.kind() {
        SurfaceKind::BSpline(s) => s,
        SurfaceKind::BallPatch(s) => s.net(),
        SurfaceKind::Sphere(s) => s.base().base(),
        SurfaceKind::Torus(s) => s.base().base(),
        SurfaceKind::SurfOfRevolution(s) => s.base(),
        SurfaceKind::SurfOfExtrusion(s) => s.base(),
        _ => return None,
    };
    s_base.gw_nurb()?;
    let curve_oob = |c: &Curve| match c.kind() {
        CurveKind::BSpline(b) => Some(b.is_out_of_bounds_enabled()),
        CurveKind::Ellipse(e) => Some(e.base().is_out_of_bounds_enabled()),
        CurveKind::Hyperbola(h) => Some(h.base().is_out_of_bounds_enabled()),
        CurveKind::Parabola(p) => Some(p.base().is_out_of_bounds_enabled()),
        _ => None,
    };
    let c_oob = curve_oob(curve)?;
    let w_oob = curve_oob(uv)?;
    let s_poly = s_base.form() == BSplineSurfaceForm::Polynomial;
    Some(
        u64::from(s_base.is_out_of_bounds_enabled())
            | (u64::from(c_oob) << 1)
            | (u64::from(w_oob) << 2)
            | (u64::from(s_poly) << 3),
    )
}

/// Exact content key of a [`bound_trim_pair`] call over `range` with net-to-surface residual
/// `residual`; `None` for a pair past [`TRIM_MEMO_MAX_FLOATS`], a net carrying a cone apex, or
/// a failed fallible reservation.
fn trim_bound_key(
    pair: &TrimPair,
    [r0, r1]: [f64; 2],
    residual: f64,
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
    collapse_unproved: bool,
    any_within_enough: bool,
    eval_flags: u64,
) -> Option<TrimBoundKey> {
    if pair.net.ends.apex.is_some() {
        return None;
    }
    let cn = &pair.cn;
    let wn = &pair.wn;
    let net = &pair.net.net;
    let floats =
        cn.u.len()
            .checked_add(cn.pw.len())?
            .checked_add(wn.u.len())?
            .checked_add(wn.pw.len())?
            .checked_add(net.u.len())?
            .checked_add(net.v.len())?
            .checked_add(net.pw.len())?;
    if floats > TRIM_MEMO_MAX_FLOATS {
        return None;
    }
    let header = if pair.net.offset.is_some() { 26 } else { 24 };
    let cap = floats.checked_add(header)?;
    let mut bits = Vec::new();
    bits.try_reserve_exact(cap).ok()?;
    let flags = u64::from(pair.net.ends.u)
        | (u64::from(pair.net.ends.v) << 1)
        | (u64::from(pair.net.offset.is_some()) << 2)
        | (u64::from(budget.is_some()) << 3)
        | (u64::from(collapse_unproved) << 4)
        | (u64::from(any_within_enough) << 5)
        | (eval_flags << 6);
    bits.extend([
        r0.to_bits(),
        r1.to_bits(),
        pair.c_slack.to_bits(),
        residual.to_bits(),
        floor.to_bits(),
        enough.to_bits(),
        over.to_bits(),
        u64::try_from(budget.unwrap_or(0)).ok()?,
        flags,
    ]);
    if let Some(off) = pair.net.offset {
        bits.extend([off.lo.to_bits(), off.hi.to_bits()]);
    }
    let push_curve = |bits: &mut Vec<u64>, c: &NurbsCurve| -> Option<()> {
        bits.extend([
            u64::try_from(c.p).ok()?,
            u64::try_from(c.n).ok()?,
            u64::try_from(c.u.len()).ok()?,
            u64::try_from(c.pw.len()).ok()?,
        ]);
        bits.extend(c.u.iter().copied().map(f64::to_bits));
        bits.extend(c.pw.iter().copied().map(f64::to_bits));
        Some(())
    };
    push_curve(&mut bits, cn)?;
    push_curve(&mut bits, wn)?;
    bits.extend([
        u64::try_from(net.p).ok()?,
        u64::try_from(net.q).ok()?,
        u64::try_from(net.nu).ok()?,
        u64::try_from(net.nv).ok()?,
        u64::try_from(net.u.len()).ok()?,
        u64::try_from(net.v.len()).ok()?,
        u64::try_from(net.pw.len()).ok()?,
    ]);
    bits.extend(net.u.iter().copied().map(f64::to_bits));
    bits.extend(net.v.iter().copied().map(f64::to_bits));
    bits.extend(net.pw.iter().copied().map(f64::to_bits));
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &word in &bits {
        hash ^= word;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(TrimBoundKey { hash, bits })
}

/// The identity reparametrisation `ψ(s) = s` over `[r0, r1]` (degree 1, two control points,
/// no interior knots).
fn identity_psi([r0, r1]: [f64; 2]) -> NurbsCurve {
    NurbsCurve {
        p: 1,
        n: 2,
        u: vec![r0, r0, r1, r1],
        pw: vec![r0, 0.0, 0.0, 1.0, r1, 0.0, 0.0, 1.0],
    }
}

/// A surface prepared for enclosure: its net and the directions it continues past the net.
///
/// With `offset` the surface is the offset `Q = S + d (S_u x S_v) / |S_u x S_v|` of the net
/// `S` by `d` ([`NetSurface::of_exact`]): its partials are enclosed from the net's one order
/// higher ([`offset_and_base_partials_over_piece`]).
pub(crate) struct NetSurface {
    net: Nurbs,
    ends: EndContinuation,
    offset: Option<Ivl>,
}

impl NetSurface {
    /// The net of `surface`, or `None` when it has none (an offset, a step surface, a surface
    /// in a volume, a curve-bounded one) or its net's parameters are not the surface's: the
    /// net and the surface must give one point at the parameters `probe`.
    ///
    /// The bounds built on it take the net as the surface. That holds by construction for
    /// the kinds [`as_b_spline`] returns whose evaluation is their own net (a B-spline
    /// surface, and the analytic kinds through their exact base B-spline); the probe checks
    /// the agreement only at `probe` (to `1e-9` relative), so it catches a parameterisation
    /// mismatch but is no proof of agreement between the probes. A procedural blend's net
    /// only approximates its definition, by which it evaluates: not exact by construction
    /// (`query::seam_bound` refuses it). A cone (or cylinder) evaluates by its formula, not
    /// its net: the plane, structured-trim and bisection bounds count the distance between
    /// the two over the net's box ([`net_to_surface_bound`]), so its net is not continued past
    /// its ends, and so do [`curve_face_gap_bound`] and the kept-trim cover's
    /// `trim_distance_bound` ([`cone_net_offset`]); [`curve_uv_gap_bound`],
    /// [`curve_uv_gap_lower_bound`] and [`curve_piece_gap_bound`] do not yet
    /// (tangency-labs/solidcraft#21600).
    pub(crate) fn of(k: &Kernel, surface: &Surface, probe: &[Point2]) -> Result<Option<Self>> {
        let Some(bs) = as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let Ok(net) = Nurbs::of(&bs) else {
            return Ok(None);
        };
        // A cone's (or cylinder's) net is bounded against the cone only over its own box
        // (`cone_residual`): a box run past its end is refused, not continued.
        let ends = EndContinuation::of(surface);
        let cone = matches!(
            surface.kind(),
            crate::surface::surface::SurfaceKind::Cone(_)
        );
        let ends = if cone || ends.apex.is_some() {
            EndContinuation::default()
        } else {
            ends
        };
        Self {
            net,
            ends,
            offset: None,
        }
        .probed(k, surface, probe)
    }

    /// Not upstream (#18219): [`Self::of`] for a surface with an exact net
    /// ([`exact_nurbs_surface`]), and for an offset surface of such a surface by a distance
    /// `d`: the net of its base with the offset `Q = S + d (S_u x S_v) / |S_u x S_v|`, as the
    /// offset surface evaluates (`EvaluatePlainOrSimple`), its partials enclosed from the
    /// base's ([`offset_and_base_partials_over_piece`]). `None` for any other surface, and for
    /// an offset that may evaluate otherwise than by that formula: one whose base is an offset
    /// itself or a surface without an exact net, one that extends its base past the base's
    /// domain or holds an extension of it (there the surface is the extension's offset, not
    /// the net's continued), or one whose net and evaluation disagree at `probe`. Where the
    /// base's first partials are too small for the evaluation to take them as they are (a
    /// pole it steps off), [`Self::partials`] refuses the box.
    ///
    /// The offset continues past its net's ends in no direction: a box past them is bounded
    /// with the slack of the offset's first partials ([`Self::partials`]).
    pub(crate) fn of_exact(
        k: &Kernel,
        surface: &Surface,
        probe: &[Point2],
    ) -> Result<Option<Self>> {
        use crate::surface::surface::exact_pieces::exact_nurbs_surface;
        use crate::surface::surface::SurfaceKind;

        let SurfaceKind::OffsetSurface(off) = surface.kind() else {
            if exact_nurbs_surface(k, surface)?.is_none() {
                return Ok(None);
            }
            return Self::of(k, surface, probe);
        };
        let base = off.base_surface();
        if matches!(base.kind(), SurfaceKind::OffsetSurface(_))
            || off.allow_extension()
            || off.with_extended_base_surface(|e| e.is_some())
        {
            return Ok(None);
        }
        let d = off.offset_distance();
        if !d.is_finite() {
            return Ok(None);
        }
        let Some(bs) = exact_nurbs_surface(k, base)? else {
            return Ok(None);
        };
        let Ok(net) = Nurbs::of(&bs) else {
            return Ok(None);
        };
        Self {
            net,
            ends: EndContinuation::default(),
            offset: Some(Ivl::from(d)),
        }
        .probed(k, surface, probe)
    }

    /// `self` where its point and `surface`'s agree at every parameter of `probe` (to `1e-9`
    /// relative), else `None` ([`Self::of`]).
    fn probed(self, k: &Kernel, surface: &Surface, probe: &[Point2]) -> Result<Option<Self>> {
        let me = self;
        for &uv in probe {
            let Some(on_net) = me.point(uv) else {
                return Ok(None);
            };
            let flags = crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags {
                u_side: crate::nurbs::knots::Side::Left,
                v_side: crate::nurbs::knots::Side::Left,
                only_upper_half: true,
                non_zero_tangents: true,
            };
            let Some(on_surface) = crate::error::ignored(surface.evaluate(k, uv, 0, 0, flags))?
                .and_then(|d| d.into_iter().next().and_then(|row| row.into_iter().next()))
            else {
                return Ok(None);
            };
            let scale = 1.0 + on_surface.x.abs() + on_surface.y.abs() + on_surface.z.abs();
            if on_net.distance(on_surface) > 1.0e-9 * scale {
                return Ok(None);
            }
        }
        Ok(Some(me))
    }

    /// Not upstream (#18166): the net moved by `-t` ([`translated_pw`]), with the slack no
    /// moved control point is farther than from the exact moved one. The continuations are
    /// directions only ([`Self::of`] keeps no apex), so they move unchanged. `None` for an
    /// offset (#18219): the slack bounds how far each moved point of the net is from its exact
    /// place, not how far the moved net's normals turn, which the offset's points move with.
    fn translated(&self, t: [f64; 3]) -> Option<(Self, f64)> {
        if self.offset.is_some() {
            return None;
        }
        let (pw, slack) = translated_pw(&self.net.pw, t)?;
        let net = Nurbs {
            pw,
            ..self.net.clone()
        };
        Some((
            Self {
                net,
                ends: self.ends,
                offset: None,
            },
            slack,
        ))
    }

    /// The knot lines strictly inside `u x v` across which the surface is not C^1 (a knot of
    /// the net of multiplicity at least the degree; for an offset, which is one order less
    /// continuous than its net, at least the degree less one, and at least one): the `u`
    /// values, then the `v` values, each sorted and once. A box cut at them has each part's
    /// second partials enclosed by [`Self::partials`].
    pub(crate) fn low_lines(&self, u: Ivl, v: Ivl) -> (Vec<f64>, Vec<f64>) {
        let n = &self.net;
        let lowered = self.offset.is_some();
        let lines = |knots: &[f64], p: usize, lo: f64, hi: f64| {
            let p = if lowered {
                p.saturating_sub(1).max(1)
            } else {
                p
            };
            let mut out: Vec<f64> = Vec::new();
            for &x in knots {
                if x > lo && x < hi && out.last() != Some(&x) {
                    let m = knots.iter().filter(|&&y| y == x).count();
                    if m >= p {
                        out.push(x);
                    }
                }
            }
            out.sort_by(f64::total_cmp);
            out.dedup();
            out
        };
        (lines(&n.u, n.p, u.lo, u.hi), lines(&n.v, n.q, v.lo, v.hi))
    }

    /// The net's domain: `([u_lo, u_hi], [v_lo, v_hi])`.
    pub(crate) fn domain(&self) -> (Ivl, Ivl) {
        let n = &self.net;
        (
            Ivl::new(knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize)),
            Ivl::new(knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize)),
        )
    }

    /// The net's sides collapsed to a point: those whose row of control points (homogeneous
    /// coordinates, weight included) is one point exactly, so the surface is that point all
    /// along the side. Each is `(across_u, at_max, point)`: `across_u` for a side at
    /// constant u (the first or last column), `at_max` for the last row or column, and the
    /// point (`None` from a net with a zero weight there). Only an end whose knots are
    /// clamped (the first or last `degree + 1` equal) is a side: the surface's edge there
    /// is that row; an unclamped end's row is not on the surface, and gives no side. None for
    /// an offset: its normal is not defined at its net's collapsed side.
    pub(crate) fn collapsed_sides(&self) -> Vec<(bool, bool, V3<f64>)> {
        if self.offset.is_some() {
            return Vec::new();
        }
        let n = &self.net;
        // the end of a knot vector (degree `p`, `count` control points) is clamped
        let clamped = |kn: &[f64], p: usize, count: usize, at_max: bool| -> bool {
            if at_max {
                let end = knot(kn, count as isize);
                (1..=p).all(|i| knot(kn, (count + i) as isize) == end)
            } else {
                let end = knot(kn, p as isize);
                (0..p).all(|i| knot(kn, i as isize) == end)
            }
        };
        let at = |i: usize, j: usize| -> Option<[f64; 4]> {
            let k = (i.checked_mul(n.nv)?.checked_add(j)?).checked_mul(4)?;
            let s = n.pw.get(k..k.checked_add(4)?)?;
            Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
        };
        let mut out = Vec::new();
        let (Some(lu), Some(lv)) = (n.nu.checked_sub(1), n.nv.checked_sub(1)) else {
            return out;
        };
        // (across u, at the max, the side's control points)
        let sides: [Edge; 4] = [
            (true, false, (0..n.nv).map(|j| (0, j)).collect()),
            (true, true, (0..n.nv).map(|j| (lu, j)).collect()),
            (false, false, (0..n.nu).map(|i| (i, 0)).collect()),
            (false, true, (0..n.nu).map(|i| (i, lv)).collect()),
        ];
        for (across_u, at_max, row) in sides {
            let ends_clamped = if across_u {
                clamped(&n.u, n.p, n.nu, at_max)
            } else {
                clamped(&n.v, n.q, n.nv, at_max)
            };
            if !ends_clamped {
                continue;
            }
            let pts: Option<Vec<[f64; 4]>> = row.iter().map(|&(i, j)| at(i, j)).collect();
            let Some(pts) = pts else {
                continue;
            };
            let Some(first) = pts.first().copied() else {
                continue;
            };
            if pts.len() < 2 || pts.iter().any(|q| *q != first) {
                continue;
            }
            let [x, y, z, w] = first;
            if !(w.is_finite() && w > 0.0 && x.is_finite() && y.is_finite() && z.is_finite()) {
                continue;
            }
            out.push((
                across_u,
                at_max,
                V3 {
                    x: x / w,
                    y: y / w,
                    z: z / w,
                },
            ));
        }
        out
    }

    /// The net's point at `uv` (double arithmetic, a sanity probe only); an offset's, the
    /// middle of its enclosure there.
    fn point(&self, uv: Point2) -> Option<crate::math::Vec3> {
        let n = &self.net;
        let ku = span(&n.u, n.p, n.nu, uv.x);
        let kv = span(&n.v, n.q, n.nv, uv.y);
        let v = match self.offset {
            None => nurbs_partials(n, ku, kv, uv.x, uv.y, 0).at(0, 0),
            Some(d) => {
                let (q, _) =
                    offset_and_base_partials_over_piece(n, d, ku, kv, uv.x, uv.x, uv.y, uv.y, 0)
                        .ok()?;
                let q = q.at(0, 0);
                V3 {
                    x: 0.5 * (q.x.lo + q.x.hi),
                    y: 0.5 * (q.y.lo + q.y.hi),
                    z: 0.5 * (q.z.lo + q.z.hi),
                }
            }
        };
        let v = crate::math::Vec3::new(v.x, v.y, v.z);
        (v.x.is_finite() && v.y.is_finite() && v.z.is_finite()).then_some(v)
    }

    /// The partials up to order `lk` of the surface on the polynomial piece `(ku, kv)` of the
    /// net over `[a0, a1] x [b0, b1]`: the net's, or an offset's from the net's
    /// ([`offset_and_base_partials_over_piece`]). An offset's box is refused where a first
    /// partial of the net is not proved larger than a thousand times the size below which
    /// the offset surface's evaluation steps off it as a pole (`10 EFF_ZERO (1 + |S|)`,
    /// `EvaluatePlainOrSimple`) and evaluates otherwise than by the offset's formula: the
    /// margin covers that evaluation's own rounding of the partial, which is relative to the
    /// net's size, many orders below it.
    #[allow(clippy::too_many_arguments)]
    fn piece_partials(
        &self,
        ku: usize,
        kv: usize,
        a0: f64,
        a1: f64,
        b0: f64,
        b1: f64,
        lk: usize,
    ) -> Result<Partials<Ivl>> {
        let n = &self.net;
        // (an offset of exactly zero is its base, and evaluates as it, with no step-off)
        let Some(d) = self.offset.filter(|d| !(d.lo == 0.0 && d.hi == 0.0)) else {
            return nurbs_partials_over_piece(n, ku, kv, a0, a1, b0, b1, lk);
        };
        let (q, s) = offset_and_base_partials_over_piece(n, d, ku, kv, a0, a1, b0, b1, lk)?;
        let p = s.at(0, 0);
        let size = Ivl::from(p.x.mag().max(p.y.mag()).max(p.z.mag()));
        let pole = (Ivl::from(1.0) + size) * Ivl::from(10.0 * crate::math::EFF_ZERO * 1.0e3);
        let clear = |v: V3<Ivl>| norm_lower(v).is_some_and(|l| l > pole.hi);
        // (the evaluation also steps off where `|S_u x S_v| < EFF_ZERO`)
        let (su, sv) = (s.at(1, 0), s.at(0, 1));
        let normal = V3 {
            x: su.y * sv.z - su.z * sv.y,
            y: su.z * sv.x - su.x * sv.z,
            z: su.x * sv.y - su.y * sv.x,
        };
        let flat = Ivl::from(crate::math::EFF_ZERO * 1.0e3);
        let crossing = norm_lower(normal).is_some_and(|l| l > flat.hi);
        if !(clear(su) && clear(sv) && crossing) {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "an offset's box where its base's first partials may be a pole's",
            });
        }
        Ok(q)
    }

    /// The surface's partials up to order `lk` over the box `w[0]` sweeps, with the slack the
    /// box's overshoot past the net needs (zero for a box inside it).
    ///
    /// The surface in the box is the net's polynomial pieces, the end span's polynomial
    /// continued past an end of the net: a surface that is a line in that direction continues
    /// (a plane, a cylinder's generator), and it is then the surface itself. Elsewhere only
    /// rounding overshoots a net's end (a seam on the face's boundary, its parameters a ulp
    /// out): the continued polynomial `S~` differs from the surface at the clamped point by at
    /// most the first partials' size times the overshoot, which is the slack, added to the
    /// bound: `dist(C, S) <= |C - S~(W)| + |S~(W) - S(clamp W)|`.
    pub(crate) fn partials(
        &self,
        w: &[V3<Ivl>],
        lk: usize,
        continuity: usize,
    ) -> Result<(Partials<Ivl>, f64)> {
        let refused = |reason| SolidError::Unsupported {
            operation: OP,
            reason,
        };
        let Some(w0) = w.first() else {
            return Err(refused("no parameter box"));
        };
        let n = &self.net;
        // (an offset is one order less continuous than its net: its partials up to an order
        // need the net's one higher. Point values (`lk` 0) need none: the hull of each
        // piece's values encloses the surface's however its pieces meet)
        let lift = usize::from(self.offset.is_some() && lk >= 1);
        let dir = |knots: &[f64],
                   p: usize,
                   count: usize,
                   lo: f64,
                   hi: f64,
                   continues: bool|
         -> Result<KnotSamples> {
            let (d0, d1) = (knot(knots, p as isize), knot(knots, count as isize));
            if !(lo <= hi && lo.is_finite() && hi.is_finite()) {
                return Err(refused("an unordered parameter box"));
            }
            let slack_range = 0.05 * (d1 - d0).abs();
            let over = (d0 - lo).max(hi - d1).max(0.0);
            let continues = continues && p == 1;
            if over > 0.0 && !continues && over > slack_range {
                return Err(refused("a box past the net's end"));
            }
            let mut out = Vec::new();
            // (a part past an end runs on to the end, so the way back to the clamped point is
            // inside the box the first partials are enclosed over)
            if lo < d0 {
                out.push((lo, d0, span(knots, p, count, d0)));
            }
            let (a, b) = (lo.max(d0), hi.min(d1));
            let mut line_over = 0.0_f64;
            let mut past = None;
            let need = continuity + lift;
            if a <= b {
                match knot_cuts(knots, p, count, a, b, need) {
                    Ok(cuts) => out.extend(cuts),
                    // (offsets only: their nets' joins below the continuity asked for are
                    // the extension's, which a trim along the face's boundary crosses by
                    // rounding; a plain net's falls back to a lower order as before)
                    Err(e) if lift > 0 => {
                        let (cuts, sliver, lower) =
                            past_one_low_line(knots, p, count, a, b, need, slack_range).ok_or(e)?;
                        // (a part past the net's end next to the short part would continue
                        // the short side's polynomial while the short part continues the
                        // kept side's: the continued surface would jump at the end)
                        if (lower && lo < d0) || (!lower && hi > d1) {
                            return Err(refused(
                                "a part past a knot line next to a part past the net's end",
                            ));
                        }
                        // the short part, on the span the surface truly has there
                        let short = if lower { cuts.first() } else { cuts.last() }
                            .map(|c| (c.0, c.1, span(knots, p, count, c.0)));
                        past = Some(short.ok_or_else(|| refused("no part past a knot line"))?);
                        out.extend(cuts);
                        line_over = sliver;
                    }
                    Err(e) => return Err(e),
                }
            }
            // An offset's box ending on an inner knot line (`knot_cuts` cuts only at lines
            // strictly inside it): the surface's value at that end is the next span's (the
            // evaluator takes the span a knot starts), which differs from the last part's
            // where the base is not C^1 there (#20544 review). Point values (`lk` 0) take the
            // end on the next span into the hull. With partials, the box is the last part's
            // polynomial `S~` up to the end, and the end itself a part past the line of width
            // zero: its jump `|S - S~|` there is added to the slack as for one
            // (`past_one_low_line`), where the line is below the continuity asked for.
            // (A box's lower end on a knot line is already the next span's: its part starts
            // there.)
            if self.offset.is_some() && a < b && b == hi && hi < d1 && knots.contains(&hi) {
                let next = span(knots, p, count, hi);
                if let Some(&(_, _, last)) = out.last().filter(|c| c.2 != next) {
                    if lift == 0 {
                        out.push((hi, hi, next));
                    } else if below_continuity(knots, p, hi, need) {
                        if past.is_some() {
                            return Err(refused(
                                "a box past a knot line and ending on another below its continuity",
                            ));
                        }
                        out.push((hi, hi, last));
                        past = Some((hi, hi, next));
                    }
                }
            }
            if hi > d1 {
                out.push((d1, hi, span(knots, p, count, d1)));
            }
            Ok((
                out,
                (if continues { 0.0 } else { over }).max(line_over),
                past,
            ))
        };
        let (us, over_u, past_u) = dir(&n.u, n.p, n.nu, w0.x.lo, w0.x.hi, self.ends.u)?;
        let (vs, over_v, past_v) = dir(&n.v, n.q, n.nv, w0.y.lo, w0.y.hi, self.ends.v)?;
        if past_u.is_some() && past_v.is_some() {
            return Err(refused("a box past knot lines in both directions"));
        }
        let mut out: Option<Partials<Ivl>> = None;
        for &(a0, a1, ku) in &us {
            for &(b0, b1, kv) in &vs {
                let p = self.piece_partials(ku, kv, a0, a1, b0, b1, lk)?;
                out = Some(match out {
                    None => p,
                    Some(h) => partials_hull(h, &p, lk),
                });
            }
        }
        let Some(out) = out else {
            return Err(refused("no piece encloses the box"));
        };
        // (the point values of an end are of the continued polynomial itself; only a span's
        // bound needs the slack, and it has the first partials)
        let mut slack = 0.0_f64;
        if lk >= 1 && (over_u > 0.0 || over_v > 0.0) {
            slack = Self::slack(&out, over_u, over_v)?;
        }
        // A part past a knot line (`past_one_low_line`) is enclosed on the kept side's
        // polynomial `S~` continued, while point values (`lk` 0, the pieces' ends) are the
        // surface's own `S`. Along a piece, with `J = sup |S - S~|` over that part (for an
        // offset across a join where its base is not C^1, the offset's jump there),
        // `g = C - S(W)` is `g~ - e`, `g~ = C - S~(W)` and `e = S(W) - S~(W)`, `|e| <= J`
        // (0 off the part). The term bounds `g~` against its ends' chord `ℓ~`, and
        // `ℓ~ = ℓ + ℓe` (the chords of `g` and of `e`), so
        // `|g(t)| <= |ℓ(t)| + |g~(t) - ℓ~(t)| + |e(t) - ℓe(t)| <= ends + term + 2 J`,
        // `|ℓ(t)|` at most the ends' gap and `|e - ℓe|` at most `J + J` (the Lipschitz and
        // corner paths alike: `|g(t)| <= |g(a)| + |g~(t) - g~(a)| + |e(t)| + |e(a)|`). That
        // is the gap to the image point `S(W(t))` itself, on the face's own sheet, as
        // `trim_image_gap_bound` promises, so `2 J` is added (#18219 review; one `J` bounds
        // only the distance to the surface's closure, which may be the kept side's sheet).
        if lk >= 1 {
            let mut jump = 0.0_f64;
            // (a mean-value form about the part's middle `m`: `S - S~` over the part lies in
            // `(S - S~)(m) + (S_u - S~_u)(u - m_u) + (S_v - S~_v)(v - m_v)`, each side one
            // polynomial piece over the part, so the jump itself is not widened by the
            // part's width in the other direction)
            let mut gap = |ku: usize, kv: usize, kt: usize, across_u: bool, a: [f64; 4]| {
                // (#23139: from the net's control points where its pieces are polynomials,
                // which does not grow with the part's length along the line)
                if let Some(from_net) = self.net_jump([ku, kv], kt, across_u, a)? {
                    jump = jump.max(from_net);
                    return Ok(());
                }
                let (tu, tv) = if across_u { (kt, kv) } else { (ku, kt) };
                let mid = |lo: f64, hi: f64| (lo + 0.5 * (hi - lo)).max(lo).min(hi);
                let (mu, mv) = (mid(a[0], a[1]), mid(a[2], a[3]));
                let own_m = self.piece_partials(tu, tv, mu, mu, mv, mv, 0)?.at(0, 0);
                let cont_m = self.piece_partials(ku, kv, mu, mu, mv, mv, 0)?.at(0, 0);
                let own = self.piece_partials(tu, tv, a[0], a[1], a[2], a[3], 1)?;
                let cont = self.piece_partials(ku, kv, a[0], a[1], a[2], a[3], 1)?;
                let du = Ivl::new(a[0], a[1]) - Ivl::from(mu);
                let dv = Ivl::new(a[2], a[3]) - Ivl::from(mv);
                let (ou, ov, cu, cv) = (own.at(1, 0), own.at(0, 1), cont.at(1, 0), cont.at(0, 1));
                let d = V3 {
                    x: own_m.x - cont_m.x + (ou.x - cu.x) * du + (ov.x - cv.x) * dv,
                    y: own_m.y - cont_m.y + (ou.y - cu.y) * du + (ov.y - cv.y) * dv,
                    z: own_m.z - cont_m.z + (ou.z - cu.z) * du + (ov.z - cv.z) * dv,
                };
                let Some(g) = norm_upper(d) else {
                    return Err(refused("a part past a knot line is not enclosed"));
                };
                jump = jump.max(g);
                Ok(())
            };
            if let Some((s0, s1, kt)) = past_u {
                for &(a0, a1, ku) in us.iter().filter(|c| c.0 == s0 && c.1 == s1) {
                    for &(b0, b1, kv) in &vs {
                        gap(ku, kv, kt, true, [a0, a1, b0, b1])?;
                    }
                }
            }
            if let Some((s0, s1, kt)) = past_v {
                for &(b0, b1, kv) in vs.iter().filter(|c| c.0 == s0 && c.1 == s1) {
                    for &(a0, a1, ku) in &us {
                        gap(ku, kv, kt, false, [a0, a1, b0, b1])?;
                    }
                }
            }
            // (only with a part past a knot line: the rounded sum would make a slack of
            // exactly 0, which callers take as none, a subnormal)
            if jump > 0.0 {
                slack = (Ivl::from(slack) + Ivl::from(2.0) * Ivl::from(jump)).hi;
            }
        }
        Ok((out, slack))
    }

    /// Not upstream (#23139): `sup |S - S~|` over the part `[a0, a1] x [b0, b1]` past a knot
    /// line, from the net's control points: `S` the surface on the part's own span (`kt`
    /// across the line, the other direction's span as `kept`'s) and `S~` the polynomial of
    /// the spans `kept` continued over it. `None` where this bound does not apply (the
    /// caller then encloses the two sides apart): a weight other than 1 among the control
    /// points of either piece, a line where the net itself is not continuous, a part that
    /// does not touch the line or runs past its span in the other direction, or a normal
    /// not proved away from zero.
    ///
    /// With `t` the parameter across the line `t = x`, `r` the one along it, and `P`, `P~`
    /// the net's two polynomials, `D = P - P~` is zero on the line (a knot of multiplicity at
    /// most the degree: both pieces end on the same row of control points there). Its
    /// derivative across the line there is the jump of the net's own, `E(r) = P_t - P~_t`: a
    /// B-spline in `r` on the net's basis whose control points are
    /// `q (R[j+1] - R[j]) / (x[j+q+1] - x[j+1]) - q (R[j] - R[j-1]) / (x[j+q] - x[j])`, `R[j]`
    /// the row on the line, so `|E| <= e`, the largest of those over the columns of the
    /// part's span (the basis is non-negative and sums to 1 there); `e = 0` below full
    /// multiplicity, where the net is `C^1`. Over a part of width `s`:
    /// `|D_t| <= e + s sup |D_tt|`, `|D_r| <= s sup |D_rt|` and `|D| <= s sup |D_t|`, the
    /// second partials each at most the two pieces' own bounds added. That is the net's jump.
    /// An offset's is `D + d (n - n~)`, and for the normals `N = P_r x P_t`,
    /// `N - N~ = D_r x P_t + P~_r x D_t` and `|n - n~| <= 2 |N - N~| / |N~|`
    /// (`a/|a| - b/|b| = (a - b)/|b| + a (|b| - |a|) / (|a| |b|)`, each term at most
    /// `|a - b| / |b|`).
    ///
    /// A net that is smooth across the line (a loft through sections of Bézier pieces: a
    /// knot of full multiplicity at every joint of a section that is `C^2` there) has an
    /// `e` of the size of its control points' rounding, whatever the part's length along
    /// the line. The two sides enclosed apart differ by their enclosures' widths, which
    /// grow with that length, and held a trim along such a line open at every depth
    /// (#23139: 45,000 pieces a trim, each bisection ending at [`MAX_PIECES`]).
    fn net_jump(
        &self,
        kept: [usize; 2],
        kt: usize,
        across_u: bool,
        a: [f64; 4],
    ) -> Result<Option<f64>> {
        let n = &self.net;
        let [ku, kv] = kept;
        // (`t` across the line, `r` along it)
        let (knots_t, q, count_t, k_kept, knots_r, p, count_r, k_r) = if across_u {
            (&n.u, n.p, n.nu, ku, &n.v, n.q, n.nv, kv)
        } else {
            (&n.v, n.q, n.nv, kv, &n.u, n.p, n.nu, ku)
        };
        let ([r0, r1], [t0, t1]) = if across_u {
            ([a[2], a[3]], [a[0], a[1]])
        } else {
            ([a[0], a[1]], [a[2], a[3]])
        };
        if kt == k_kept || k_r < p || k_r >= count_r || kt < q || kt >= count_t {
            return Ok(None);
        }
        // the line: the own span's end towards the kept one
        let x = if kt > k_kept {
            knot(knots_t, kt as isize)
        } else {
            knot(knots_t, kt as isize + 1)
        };
        if !(t0 <= x && x <= t1 && t0.is_finite() && t1.is_finite()) {
            return Ok(None);
        }
        // (the columns of the part's span carry it only inside that span)
        if !(knot(knots_r, k_r as isize) <= r0 && r1 <= knot(knots_r, k_r as isize + 1)) {
            return Ok(None);
        }
        let Some(first) = knots_t.iter().position(|&y| y == x) else {
            return Ok(None);
        };
        let m = knots_t.iter().filter(|&&y| y == x).count();
        if m > q || first == 0 {
            return Ok(None);
        }
        // the control point of column `i` (along the line) and row `j` (across it)
        let point = |i: usize, j: usize| -> Option<V3<Ivl>> {
            let at = if across_u {
                j.checked_mul(n.nv)?.checked_add(i)?
            } else {
                i.checked_mul(n.nv)?.checked_add(j)?
            };
            let c = n.pw.get(at.checked_mul(4)?..)?;
            let (&x, &y, &z, &w) = (c.first()?, c.get(1)?, c.get(2)?, c.get(3)?);
            (w == 1.0).then_some(V3 {
                x: Ivl::from(x),
                y: Ivl::from(y),
                z: Ivl::from(z),
            })
        };
        // (every control point of either piece is a plain point: the pieces are polynomials)
        let (rows_lo, rows_hi) = (kt.min(k_kept) - q, kt.max(k_kept));
        for i in (k_r - p)..=k_r {
            for j in rows_lo..=rows_hi {
                if point(i, j).is_none() {
                    return Ok(None);
                }
            }
        }
        let mut e = 0.0_f64;
        if m == q {
            // (the row on the line: the knots `first .. first + q` are the line's)
            let j = first - 1;
            if j == 0 || j + 1 >= count_t {
                return Ok(None);
            }
            let above = Ivl::from(knot(knots_t, (j + q + 1) as isize))
                - Ivl::from(knot(knots_t, (j + 1) as isize));
            let below =
                Ivl::from(knot(knots_t, (j + q) as isize)) - Ivl::from(knot(knots_t, j as isize));
            if !(above.lo > 0.0 && below.lo > 0.0) {
                return Ok(None);
            }
            let deg = Ivl::from(q as f64);
            for i in (k_r - p)..=k_r {
                let (Some(lower), Some(on), Some(upper)) =
                    (point(i, j - 1), point(i, j), point(i, j + 1))
                else {
                    return Ok(None);
                };
                let d = minus(
                    scaled(minus(upper, on), deg / above),
                    scaled(minus(on, lower), deg / below),
                );
                let Some(size) = norm_upper(d) else {
                    return Ok(None);
                };
                e = e.max(size);
            }
        }
        let (tu, tv) = if across_u { (kt, kv) } else { (ku, kt) };
        // (an offset's own side is its formula's only away from a pole: refused there as
        // the two sides enclosed apart are)
        self.piece_partials(tu, tv, a[0], a[1], a[2], a[3], 0)?;
        let own = nurbs_partials_over_piece(n, tu, tv, a[0], a[1], a[2], a[3], 2)?;
        let cont = nurbs_partials_over_piece(n, ku, kv, a[0], a[1], a[2], a[3], 2)?;
        // (a partial of order `i` along the line and `j` across it)
        let at = |part: &Partials<Ivl>, i: usize, j: usize| {
            if across_u {
                part.at(j, i)
            } else {
                part.at(i, j)
            }
        };
        let size = |v: V3<Ivl>| norm_upper(v).map(Ivl::from);
        let (Some(own_tt), Some(cont_tt), Some(own_rt), Some(cont_rt), Some(own_t), Some(cont_r)) = (
            size(at(&own, 0, 2)),
            size(at(&cont, 0, 2)),
            size(at(&own, 1, 1)),
            size(at(&cont, 1, 1)),
            size(at(&own, 0, 1)),
            size(at(&cont, 1, 0)),
        ) else {
            return Ok(None);
        };
        let width = (Ivl::from(t1) - Ivl::from(x))
            .mag()
            .max((Ivl::from(x) - Ivl::from(t0)).mag());
        let s = Ivl::from(width);
        let d_t = Ivl::from(e) + s * (own_tt + cont_tt);
        let d_r = s * (own_rt + cont_rt);
        let mut jump = s * d_t;
        if let Some(d) = self.offset.filter(|d| !(d.lo == 0.0 && d.hi == 0.0)) {
            let Some(normal) = norm_lower(cross(at(&cont, 1, 0), at(&cont, 0, 1))) else {
                return Ok(None);
            };
            // (`norm_lower` gives a finite value)
            if normal <= 0.0 {
                return Ok(None);
            }
            let turned = d_r * own_t + cont_r * d_t;
            jump = jump + Ivl::from(d.mag()) * Ivl::from(2.0) * turned / Ivl::from(normal);
        }
        Ok(jump.hi.is_finite().then_some(jump.hi))
    }

    fn slack(p: &Partials<Ivl>, over_u: f64, over_v: f64) -> Result<f64> {
        let (Some(su), Some(sv)) = (norm_upper(p.at(1, 0)), norm_upper(p.at(0, 1))) else {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "the surface's first partials are not enclosed",
            });
        };
        Ok((Ivl::from(su) * Ivl::from(over_u) + Ivl::from(sv) * Ivl::from(over_v)).hi)
    }
}

/// `sqrt(x^2 + y^2 + z^2)` of an enclosure, an upper bound; `None` unless finite.
fn norm_upper(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = Ivl::from(a.mag());
        m * m
    };
    let s = sq(v.x) + sq(v.y) + sq(v.z);
    let hi = s.sqrt().hi;
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && hi.is_finite()).then_some(hi)
}

/// `sqrt(x^2 + y^2 + z^2)` of an enclosure, a lower bound (each component's least
/// magnitude); `None` unless finite.
fn norm_lower(v: V3<Ivl>) -> Option<f64> {
    let sq = |a: Ivl| {
        let m = if a.lo > 0.0 {
            a.lo
        } else if a.hi < 0.0 {
            -a.hi
        } else {
            0.0
        };
        let m = Ivl::from(m);
        m * m
    };
    let s = sq(v.x) + sq(v.y) + sq(v.z);
    let lo = s.sqrt().lo.max(0.0);
    (v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && lo.is_finite()).then_some(lo)
}

fn scaled(v: V3<Ivl>, a: Ivl) -> V3<Ivl> {
    V3 {
        x: v.x * a,
        y: v.y * a,
        z: v.z * a,
    }
}

fn plus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn minus(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// A cut that keeps at least this share of its piece's slack has stopped shrinking it: the
/// slack is the overshoot of the piece's UV box past its net's end, which a shorter piece
/// halves while the box is wider than the trim's own overshoot, and keeps once it is not
/// ([`PieceBound::closes`]).
const SLACK_KEPT: f64 = 0.75;

/// The bound of one piece of a bisection: its end values, its second-order term, the
/// surface's slack past its net, and the term with the slack added (`h² / 8 sup |g''| +
/// slack`, rounded up), the piece's bound being the end values plus the last.
#[derive(Clone, Copy)]
struct PieceBound {
    ends: f64,
    term: f64,
    slack: f64,
    with_slack: f64,
}

impl PieceBound {
    /// A piece of no slack (`with_slack` the term itself).
    fn exact(ends: f64, term: f64) -> Self {
        Self {
            ends,
            term,
            slack: 0.0,
            with_slack: term,
        }
    }

    /// `term` (an enclosure, rounded up here) with `slack` added.
    fn with(ends: f64, term: Ivl, slack: f64) -> Option<Self> {
        let b = Self {
            ends,
            term: term.hi,
            slack,
            with_slack: (term + Ivl::from(slack)).hi,
        };
        (b.term.is_finite() && b.with_slack.is_finite()).then_some(b)
    }

    /// Whether the piece closes: its second term with the slack is at most a `relative` of
    /// its end values or `floor`; or its second term alone is and the cut that made it kept
    /// at least [`SLACK_KEPT`] of its parent's slack `parent` (infinite for a first piece):
    /// cutting it further would leave the slack as it is, so a piece held open by it would be
    /// cut down to [`MAX_DEPTH`] all along its stretch and spend the [`MAX_PIECES`] the other
    /// pieces need (#18166, `single_face_fillet1019_8`: a trim running past its surface's
    /// net, the slack 1.3e-6, held the bound at 0.098 for a gap of 2.2e-4). Closing decides
    /// only how tight the bound is, never whether it holds.
    fn closes(&self, parent: f64, relative: f64, floor: f64) -> bool {
        let at_most = (relative * self.ends).max(floor).max(CLOSE_ABSOLUTE);
        self.with_slack <= at_most
            || (self.term <= at_most && self.slack > 0.0 && self.slack >= SLACK_KEPT * parent)
    }
}

/// The bound of one piece as `(end values, second-order term with the slack)`: the gap over
/// `[a, b]` is at most their sum ([`piece`]).
///
/// # Errors
///
/// Those of [`piece`].
pub(crate) fn piece_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<(f64, f64)>> {
    Ok(piece(cn, wn, s, a, b)?.map(|p| (p.ends, p.with_slack)))
}

/// The bound of one piece ([`second_term`]).
fn piece(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    s: &NetSurface,
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let c = curve_derivs_over(cn, at, 0, None)?;
        let w = curve_derivs_over(wn, at, 0, None)?;
        let (Some(c0), Some(_)) = (c.first().copied(), w.first()) else {
            return Ok(None);
        };
        let surf = s.partials(&w, 0, 0)?.0.at(0, 0);
        let Some(g) = norm_upper(minus(c0, surf)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let wd = uv_derivs_over(wn, span, 2)?;
    let (Some(c1), Some(c2)) = (cd.get(1), cd.get(2)) else {
        return Ok(None);
    };
    let h = Ivl::from(b) - Ivl::from(a);
    let term = second_term(s, &wd, *c1, *c2, h)?;
    Ok(term.and_then(|(term, slack)| PieceBound::with(ends, term, slack)))
}

/// `ψ`'s value and derivatives up to `ld` over `t`, its value clamped to `range`: `ψ` is a
/// non-rational B-spline whose control values lie in `range` ([`reparametrisation`]), so by the
/// convex hull property its values do as well. `None` when they are not enclosed.
fn psi_over(psi: &NurbsCurve, t: Ivl, ld: usize, [lo, hi]: [f64; 2]) -> Result<Option<Vec<Ivl>>> {
    let mut out: Vec<Ivl> = curve_derivs_over(psi, t, ld, None)?
        .iter()
        .map(|d| d.x)
        .collect();
    let Some(r) = out.first_mut() else {
        return Ok(None);
    };
    let (a, b) = (r.lo.max(lo), r.hi.min(hi));
    if !(a <= b && out.iter().all(|d| d.is_finite())) {
        return Ok(None);
    }
    if let Some(r) = out.first_mut() {
        *r = Ivl::new(a, b);
    }
    Ok(Some(out))
}

/// Not upstream (#22037): a UV curve's value and derivatives up to `ld` over `s`, each
/// enclosed both as [`curve_derivs_over`] gives it and about the curve's point at the middle
/// of `s` ([`curve_derivs_over_about`], that point added back to the value), and the two
/// intersected: each holds the curve's, so their intersection does.
///
/// The value is the box the surface's partials are enclosed over, and its overshoot past the
/// net's end is the slack the bisection adds ([`NetSurface::partials`]). Enclosed from the
/// origin, a rational curve's quotient by its weight widens the box by the weights' change
/// times the curve's distance from the origin, to first order in the piece: a circle trim
/// tangent to its patch's side runs past it by about `0.76 h` at `(0, -10)` (`cylinder.smb`'s
/// cap, a piece `h = 1.9e-3` long past by `1.4e-3`), so the slack shrank only as fast as the
/// pieces, and the bisection ran out of pieces about each tangency with the bound at `0.097`.
/// About the middle the quotient carries the curve's distance from that point instead, of the
/// order of the piece: the box is within a second-order term of the curve's range, and its
/// overshoot past a side the curve is tangent to is of the order of `h^2` (#22037).
fn uv_derivs_over(wn: &NurbsCurve, s: Ivl, ld: usize) -> Result<Vec<V3<Ivl>>> {
    let plain = curve_derivs_over(wn, s, ld, None)?;
    if s.lo >= s.hi {
        return Ok(plain);
    }
    let middle = curve_derivs_over(wn, Ivl::from(s.mid()), 0, None)?;
    let Some(c) = middle.first().map(|p| [p.x.mid(), p.y.mid(), p.z.mid()]) else {
        return Ok(plain);
    };
    if !c.iter().all(|x| x.is_finite()) {
        return Ok(plain);
    }
    let about = curve_derivs_over_about(wn, s, ld, None, c)?;
    let meet = |a: Ivl, b: Ivl| {
        let m = Ivl::new(a.lo.max(b.lo), a.hi.min(b.hi));
        // (each holds the curve's: they meet, but where either is not finite, or not an
        // interval, the one from the origin stands)
        if a.is_finite() && b.is_finite() && m.lo <= m.hi {
            m
        } else {
            a
        }
    };
    let [cx, cy, cz] = c;
    Ok(plain
        .iter()
        .zip(&about)
        .enumerate()
        .map(|(d, (p, q))| {
            let q = if d == 0 {
                V3 {
                    x: q.x + Ivl::from(cx),
                    y: q.y + Ivl::from(cy),
                    z: q.z + Ivl::from(cz),
                }
            } else {
                *q
            };
            V3 {
                x: meet(p.x, q.x),
                y: meet(p.y, q.y),
                z: meet(p.z, q.z),
            }
        })
        .collect())
}

/// The curve, the trim and the reparametrisation [`trim_image_gap_bound`] compares, with
/// the knots each may have a corner at.
struct Sides<'a> {
    curve: &'a NurbsCurve,
    trim: &'a NurbsCurve,
    psi: &'a NurbsCurve,
    curve_kinks: Vec<f64>,
    trim_kinks: Vec<f64>,
    range: [f64; 2],
}

/// The enclosure of the gap at one trim parameter.
fn reparametrised_endpoint(sides: &Sides<'_>, s: &NetSurface, t: f64) -> Result<Option<f64>> {
    let at = Ivl::from(t);
    let Some(r) = psi_over(sides.psi, at, 0, sides.range)? else {
        return Ok(None);
    };
    let Some(&r0) = r.first() else {
        return Ok(None);
    };
    let c = curve_derivs_over(sides.curve, r0, 0, None)?;
    let w = curve_derivs_over(sides.trim, at, 0, None)?;
    let (Some(c0), Some(_)) = (c.first().copied(), w.first()) else {
        return Ok(None);
    };
    let surf = s.partials(&w, 0, 0)?.0.at(0, 0);
    Ok(norm_upper(minus(c0, surf)))
}

/// A split adds one new endpoint. The original endpoints are also used by its children, and
/// neighbouring pieces share an endpoint. The rounded enclosure depends only on the parameter
/// bits and the geometry and correspondence fixed for this cache.
fn cached_trim_endpoint(
    bounds: &mut BTreeMap<u64, f64>,
    t: f64,
    evaluate: impl FnOnce() -> Result<Option<f64>>,
) -> Result<Option<f64>> {
    match bounds.entry(t.to_bits()) {
        Entry::Occupied(entry) => Ok(Some(*entry.get())),
        Entry::Vacant(entry) => {
            let bound = evaluate()?;
            if let Some(value) = bound {
                entry.insert(value);
            }
            Ok(bound)
        }
    }
}

/// The bound of one piece `[a, b]` of the trim's parameter `s` of `g = C(ψ(s)) - S(W(s))`:
/// `(end values, second-order term)`, with `(C o ψ)' = C'(ψ) ψ'` and
/// `(C o ψ)'' = C''(ψ) ψ'^2 + C'(ψ) ψ''`, `C`'s derivatives enclosed over the range `ψ` sweeps.
/// Where the trim has a knot inside the piece, or `ψ` sweeps over one of the curve's, that it
/// may not be C^1 across, `g` is only Lipschitz there: the term is `h sup |g'|`, with the
/// one-sided derivatives hulled (as for a C^0 knot line of the surface).
fn piece_reparametrised(
    sides: &Sides<'_>,
    s: &NetSurface,
    a: f64,
    b: f64,
    mut endpoint: impl FnMut(f64) -> Result<Option<f64>>,
) -> Result<Option<PieceBound>> {
    let Sides {
        curve: cn,
        trim: wn,
        psi,
        range,
        ..
    } = *sides;
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let Some(g) = endpoint(t)? else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let Some(r) = psi_over(psi, span, 2, range)? else {
        return Ok(None);
    };
    let (Some(&r0), Some(&p1), Some(&p2)) = (r.first(), r.get(1), r.get(2)) else {
        return Ok(None);
    };
    let cd = curve_derivs_over(cn, r0, 2, None)?;
    let wd = uv_derivs_over(wn, span, 2)?;
    let (Some(c1), Some(c2), Some(w1)) = (cd.get(1), cd.get(2), wd.get(1)) else {
        return Ok(None);
    };
    let cp = scaled(*c1, p1);
    let h = Ivl::from(b) - Ivl::from(a);
    let corner = sides.trim_kinks.iter().any(|&x| a < x && x < b)
        || sides.curve_kinks.iter().any(|&x| r0.lo < x && x < r0.hi);
    if corner {
        let (p, slack) = s.partials(&wd, 1, 0)?;
        let sw = plus(scaled(p.at(1, 0), w1.x), scaled(p.at(0, 1), w1.y));
        let Some(m) = norm_upper(minus(cp, sw)) else {
            return Ok(None);
        };
        return Ok(PieceBound::with(ends, h * Ivl::from(m), slack));
    }
    let cpp = plus(scaled(*c2, p1 * p1), scaled(*c1, p2));
    let term = second_term(s, &wd, cp, cpp, h)?;
    Ok(term.and_then(|(term, slack)| PieceBound::with(ends, term, slack)))
}

/// The second-order term of a piece of length `h` of `g = C~ - S(W)`, where `wd` are `W`'s
/// derivatives over the piece and `c1`, `c2` the first two derivatives of the curve side `C~`
/// there: `h^2 / 8 * sup |g''|`, or `h * sup |g'|` across a knot line the surface is only C^0
/// across, enclosed; and apart from it the surface's slack past its net (the box's overshoot
/// past the net's end times the first partials, [`NetSurface::partials`]). `None` when it is
/// not enclosed.
fn second_term(
    s: &NetSurface,
    wd: &[V3<Ivl>],
    c1: V3<Ivl>,
    c2: V3<Ivl>,
    h: Ivl,
) -> Result<Option<(Ivl, f64)>> {
    let (Some(w1), Some(w2)) = (wd.get(1), wd.get(2)) else {
        return Ok(None);
    };
    let (u1, v1) = (w1.x, w1.y);
    let (u2, v2) = (w2.x, w2.y);
    // The second order, where the surface is C^1 across every knot line the box crosses:
    // (S o W)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v''
    let second = s.partials(wd, 2, 1).map(|(p, slack)| {
        let mut swpp = scaled(p.at(2, 0), u1 * u1);
        swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
        swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
        swpp = plus(swpp, scaled(p.at(1, 0), u2));
        swpp = plus(swpp, scaled(p.at(0, 1), v2));
        (minus(c2, swpp), slack)
    });
    let term = match second {
        Ok((gpp, slack)) => {
            let Some(m) = norm_upper(gpp) else {
                return Ok(None);
            };
            (h * h * Ivl::from(m) * Ivl::from(0.125), slack)
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        // a knot line the surface is only C^0 across (a patch corner the curve ends on):
        // g is Lipschitz there, |g(t) - g(a)| <= h sup |g'| with the one-sided partials hulled
        Err(_) => {
            let (p, slack) = s.partials(wd, 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m), slack)
        }
    };
    Ok(Some(term))
}

/// A proved upper bound on the distance from the curve `curve` to the surface `surface`
/// over its parameters `[lo, hi]` (within its domain), where `uv` is a B-spline of parameters `(u, v, 0)` (the correspondence
/// `W`, on the curve's own parameter). A piece closes once its second term is at most a
/// thousandth of its end values or `floor` (the caller's scale: a hundredth of the gap it
/// expects, below which an excess in the bound changes nothing; a nearest point is found only
/// to some 1e-8 of a parameter, which the bound carries as well). A piece that cannot be cut
/// further is bounded as it stands, so the result is a bound however the pieces ended; `None`
/// when some piece has no bound at all.
///
/// The first pieces are also cut at every [`kinks`] of `C` and of `W` inside `[lo, hi]`:
/// the second-order term bounds `|g − ℓ|` (`ℓ` the chord of the end values) by `h² / 8 sup
/// |g''|` only where `g` is C¹, and the spans' second derivatives, hulled over a piece,
/// know nothing of a jump in the first (a corner of `C` between two pieces' ends would be
/// missed). No piece then has a corner of either inside it: it ends on the corner instead.
///
/// # Errors
///
/// Cancellation (polled at every piece), and the unported statuses of the evaluations.
fn certified_gap(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &NetSurface,
    [lo, hi]: [f64; 2],
    first_pieces: usize,
    floor: f64,
) -> Result<Option<f64>> {
    let (Ok(cn), Ok(wn)) = (NurbsCurve::of(curve), NurbsCurve::of(uv)) else {
        return Ok(None);
    };
    let (Some(&d0), Some(&d1)) = (cn.u.get(cn.p), cn.u.get(cn.n)) else {
        return Ok(None);
    };
    if !(lo.is_finite() && hi.is_finite() && d0 <= lo && lo < hi && hi <= d1) {
        return Ok(None);
    }
    let mut corners = kinks(&cn, lo, hi);
    corners.extend(kinks(&wn, lo, hi));
    corners.sort_by(f64::total_cmp);
    corners.dedup();
    bisect(
        [lo, hi],
        first_pieces,
        &corners,
        CLOSE_RELATIVE,
        floor,
        [f64::NEG_INFINITY, f64::INFINITY],
        None,
        |a, b| {
            k.check_cancelled(OP)?;
            piece(&cn, &wn, surface, a, b)
        },
    )
}

/// The bisection of `[lo, hi]` that bounds a gap `max |g|` from `piece`'s [`PieceBound`] of
/// each piece (the piece's bound its end values plus its second term with the slack): a piece
/// closes as [`PieceBound::closes`] says (its second term small against its end values or
/// `floor`, and its slack too unless the cut that made it no longer shrank it), and one that
/// cannot be cut further is bounded as it stands. `None` when some piece has no bound at all. Once a closed piece's bound is above
/// `over`, so is the whole bound, and the bisection stops there with an infinite bound (an
/// `over` of `f64::INFINITY` never stops it): the pieces before it are cut exactly as without
/// it, so the comparison of the bound with `over` is the same.
///
/// A piece whose bound is at most `enough` closes as it stands (an `enough` of
/// `f64::NEG_INFINITY` never closes one): for a caller that only asks whether the bound is at
/// most `enough`, the pieces cut further would only tighten what already answers it.
///
/// The first pieces are also cut at every point of `corners` (sorted, distinct) inside
/// `(lo, hi)`, so no first piece has one inside it ([`certified_gap`] passes the curves'
/// [`kinks`]).
///
/// `budget` is the most pieces the bisection may make while it may still show the bound at most
/// `enough`, [`MAX_PIECES`] when `None`. Without one a piece left open when the pieces run out
/// is bounded as it stands, as one cut to the deepest; with one, the gap is then not enclosed
/// (`None`): a caller that only acts on a bound at most a few times its tolerance can afford
/// neither the whole budget nor the looser bound for a gap just within `enough`, which only
/// fine pieces close below it.
///
/// With a budget, the pieces already proved are not cut further. Every end value a piece
/// evaluates is part of the bound of a piece that closes, and no cut lowers it (a cut piece's
/// halves evaluate the same ends), so the bound is at least the largest end value `seen` so far:
/// - a piece whose bound is at most `seen` closes as it stands, since cutting it cannot lower
///   the bound, and closing it keeps the bound's value;
/// - once `seen` is above `over`, so is the bound: it is infinite at once;
/// - once `seen` is above `enough`, the bound is not at most `enough` whatever the cuts, and
///   the pieces still open bound a gap the caller acts on: they get the whole [`MAX_PIECES`],
///   not the budget, which only stops the search for a bound at most `enough`. The pieces the
///   budget stopped before (whatever the order the pieces were reached in) are set aside, and
///   cut on once the other pieces are done if `seen` is then above `enough`; if it is not,
///   the gap is not enclosed.
fn bisect(
    range: [f64; 2],
    first_pieces: usize,
    corners: &[f64],
    relative: f64,
    floor: f64,
    limits: [f64; 2],
    budget: Option<usize>,
    piece: impl FnMut(f64, f64) -> Result<Option<PieceBound>>,
) -> Result<Option<f64>> {
    bisect_with_ends(
        range,
        first_pieces,
        corners,
        relative,
        floor,
        limits,
        budget,
        piece,
        None,
    )
}

/// Outcome of [`bisect_core`], distinguishing a budget-stopped run whose evaluated endpoints
/// stayed within `enough` from an unenclosed gap or one proved past `over`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BisectOutcome {
    /// A proved finite bound at most `over`.
    Proved(f64),
    /// Proved to exceed `over`.
    ExceedsOver,
    /// Stopped by the piece `budget` while every evaluated endpoint stayed at most `enough`.
    BudgetStopped,
    /// Not enclosed (not due to the piece `budget`).
    Unproved,
}

/// Not upstream (#19889): [`bisect`], with `ends` (where given) the end value at a parameter
/// that `piece` takes as the end value there: `Some(g)` where `piece` evaluates `g` there
/// (its piece's end values are then the largest of its two and `0`), `None` (or an error
/// other than a cancellation) where it bounds no piece ending there.
///
/// With `ends`, a budget, `over` infinite and `enough` at least `0`, the bisection stops at
/// once with no bound (`None`) where [`bisect`] is sure to: once the budget stops a piece
/// while the bound may still be at most `enough`, no piece is cut further until the largest
/// end value `seen` is above `enough` (each piece still on the stack is bounded or set aside
/// as it stands); where no end value of those pieces is above `enough`, `seen` stays at most
/// `enough`, the pieces set aside stay so, and the gap is not enclosed whatever the pieces'
/// bounds (`over` infinite: no bound stops it first). Only the end values are evaluated for
/// that, not the pieces' second terms: a trim the budget stops on pieces cut at every knot of
/// its reparametrisation (thousands of first pieces) evaluates one piece, not all (the
/// `crashes/2992` union bounds 510 such cut passes, 666k pieces, 50 s of its 58 s). Where an
/// end value is above `enough`, the bisection goes on as [`bisect`]'s.
///
/// # Errors
///
/// Those of `piece`, and a cancellation from `ends`.
fn bisect_with_ends(
    range: [f64; 2],
    first_pieces: usize,
    corners: &[f64],
    relative: f64,
    floor: f64,
    limits: [f64; 2],
    budget: Option<usize>,
    piece: impl FnMut(f64, f64) -> Result<Option<PieceBound>>,
    ends: Option<&mut dyn FnMut(f64) -> Result<Option<f64>>>,
) -> Result<Option<f64>> {
    let (outcome, _) = bisect_core(
        range,
        first_pieces,
        corners,
        relative,
        floor,
        limits,
        budget,
        false,
        piece,
        ends,
    )?;
    Ok(match outcome {
        BisectOutcome::Proved(bound) => Some(bound),
        BisectOutcome::ExceedsOver => Some(f64::INFINITY),
        BisectOutcome::BudgetStopped | BisectOutcome::Unproved => None,
    })
}

/// [`bisect_with_ends`], with `collapse_unproved` allowing the endpoint look-ahead
/// ([`look_ends`]) even when `over` is finite: a caller that only needs to distinguish
/// `Proved`, `ExceedsOver`, and `BudgetStopped`/`Unproved` does not need to evaluate second
/// partials on every remaining piece once the budget has parked a piece and no remaining
/// endpoint exceeds `enough`. Returns the outcome together with the number of pieces popped.
fn bisect_core(
    [lo, hi]: [f64; 2],
    first_pieces: usize,
    corners: &[f64],
    relative: f64,
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
    collapse_unproved: bool,
    mut piece: impl FnMut(f64, f64) -> Result<Option<PieceBound>>,
    mut ends: Option<&mut dyn FnMut(f64) -> Result<Option<f64>>>,
) -> Result<(BisectOutcome, u64)> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) || first_pieces == 0 {
        return Ok((BisectOutcome::Unproved, 0));
    }
    let f = |j: usize| {
        if j == first_pieces {
            hi
        } else {
            lo + (hi - lo) * (j as f64) / (first_pieces as f64)
        }
    };
    // (each piece with its parent's slack, infinite for a first piece)
    let mut stack: Vec<(f64, f64, u32, f64)> = Vec::new();
    for i in 0..first_pieces {
        let (mut a, b) = (f(i), f(i + 1));
        let start = a;
        for &x in corners.iter().filter(|&&x| start < x && x < b) {
            stack.push((a, x, 0, f64::INFINITY));
            a = x;
        }
        stack.push((a, b, 0, f64::INFINITY));
    }
    stack.reverse();
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    let mut evaluated = 0_u64;
    // (the largest end value evaluated, with a budget: the bound is at least it)
    let mut seen = f64::NEG_INFINITY;
    // (the open pieces the budget stopped, in the order they were reached)
    let mut parked: Vec<(f64, f64, u32, f64)> = Vec::new();
    // (whether the end values of the pieces left were looked at, once the budget first stopped
    // a piece: [`bisect_with_ends`])
    let mut ends_looked = ends.is_none()
        || (!collapse_unproved && over != f64::INFINITY)
        || enough.is_nan()
        || enough < 0.0;
    loop {
        while let Some((a, b, depth, parent)) = stack.pop() {
            evaluated = evaluated.saturating_add(1);
            let mid = 0.5 * (a + b);
            let bottom = depth >= MAX_DEPTH || !(a < mid && mid < b);
            // (the budget holds while the bound may still be at most `enough`; a piece that
            // cannot be cut further is bounded as it is: the bound holds whether or not the
            // second term has shrunk, closing only makes it tight)
            let out_of = |seen: f64| {
                let capped = budget.filter(|_| seen <= enough);
                (capped.is_some(), made + 2 > capped.unwrap_or(MAX_PIECES))
            };
            let mut slack = f64::INFINITY;
            match piece(a, b) {
                Ok(Some(p)) => {
                    let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack)).hi;
                    if p.ends > over {
                        return Ok((BisectOutcome::ExceedsOver, evaluated));
                    }
                    if budget.is_some() {
                        seen = seen.max(p.ends);
                    }
                    let (capped, out_of_pieces) = out_of(seen);
                    let proved = budget.is_some() && bound <= seen;
                    let closed = bound <= enough || proved || p.closes(parent, relative, floor);
                    // (an open piece the budget stopped is not bounded as it stands)
                    if !closed && capped && out_of_pieces {
                        parked.push((a, b, depth, parent));
                        if !ends_looked {
                            ends_looked = true;
                            if let Some(end) = ends.as_mut() {
                                match look_ends(&stack, &mut **end, enough, over)? {
                                    EndsLook::AllWithin => {
                                        return Ok((BisectOutcome::BudgetStopped, evaluated));
                                    }
                                    EndsLook::ExceedsOver => {
                                        return Ok((BisectOutcome::ExceedsOver, evaluated));
                                    }
                                    EndsLook::ExceedsEnough => {}
                                }
                            }
                        }
                        continue;
                    }
                    if bottom || out_of_pieces || closed {
                        if bound > over {
                            return Ok((BisectOutcome::ExceedsOver, evaluated));
                        }
                        worst = worst.max(bound);
                        continue;
                    }
                    slack = p.slack;
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                // a box past the net, on a knot line of too low a continuity: a smaller piece
                // may clear it; at the bottom the gap is not enclosed
                Ok(None) | Err(_) => {
                    let (capped, out_of_pieces) = out_of(seen);
                    if capped && out_of_pieces {
                        parked.push((a, b, depth, parent));
                        if !ends_looked {
                            ends_looked = true;
                            if let Some(end) = ends.as_mut() {
                                match look_ends(&stack, &mut **end, enough, over)? {
                                    EndsLook::AllWithin => {
                                        return Ok((BisectOutcome::BudgetStopped, evaluated));
                                    }
                                    EndsLook::ExceedsOver => {
                                        return Ok((BisectOutcome::ExceedsOver, evaluated));
                                    }
                                    EndsLook::ExceedsEnough => {}
                                }
                            }
                        }
                        continue;
                    }
                    if bottom || out_of_pieces {
                        return Ok((BisectOutcome::Unproved, evaluated));
                    }
                }
            }
            made += 2;
            stack.push((mid, b, depth + 1, slack));
            stack.push((a, mid, depth + 1, slack));
        }
        // (a gap the budget stopped while it may still be at most `enough` is not enclosed;
        // past `enough`, the stopped pieces are cut on without it)
        if parked.is_empty() {
            break;
        }
        if seen <= enough {
            return Ok((BisectOutcome::BudgetStopped, evaluated));
        }
        parked.reverse();
        stack.append(&mut parked);
    }
    let outcome = if worst.is_finite() {
        BisectOutcome::Proved(worst)
    } else {
        BisectOutcome::Unproved
    };
    Ok((outcome, evaluated))
}

/// Not upstream (#19889): `end` for the look at the end values of [`bisect_with_ends`],
/// polling the kernel's cancel token before each value, cached or not, as the pieces it
/// stands in for poll before each piece: a cancellation that arrives during the look
/// cancels the bound instead of letting it finish as "not within".
fn polled_end<'a>(
    k: &'a Kernel,
    mut end: impl FnMut(f64) -> Result<Option<f64>> + 'a,
) -> impl FnMut(f64) -> Result<Option<f64>> + 'a {
    move |t| {
        k.check_cancelled(OP)?;
        end(t)
    }
}

/// Outcome of scanning the remaining stack endpoints in [`bisect_core`].
enum EndsLook {
    /// No endpoint exceeded `enough`.
    AllWithin,
    /// At least one endpoint exceeded `enough`, none exceeded `over` yet.
    ExceedsEnough,
    /// At least one endpoint exceeded `over`.
    ExceedsOver,
}

/// Not upstream (#19889): scans the end values `end` gives at the ends of the pieces `stack`
/// against `enough` and `over` ([`bisect_core`]).
///
/// # Errors
///
/// A cancellation from `end` (any other error is no end value, as `piece` takes it).
fn look_ends<'a>(
    stack: &[(f64, f64, u32, f64)],
    end: &mut (dyn FnMut(f64) -> Result<Option<f64>> + 'a),
    enough: f64,
    over: f64,
) -> Result<EndsLook> {
    for &(a, b, ..) in stack {
        for t in [a, b] {
            match end(t) {
                Ok(Some(g)) if g > over => return Ok(EndsLook::ExceedsOver),
                Ok(Some(g)) if g > enough => return Ok(EndsLook::ExceedsEnough),
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Ok(_) | Err(_) => {}
            }
        }
    }
    Ok(EndsLook::AllWithin)
}

/// The parameters strictly inside `(lo, hi)` where the curve `cn` may not be C¹: its
/// interior knots of multiplicity at least its degree (a span's polynomial meets the next
/// only in its value there), in increasing order.
fn kinks(cn: &NurbsCurve, lo: f64, hi: f64) -> Vec<f64> {
    let Some(knots) = cn.u.get(cn.p..=cn.n) else {
        return Vec::new();
    };
    knots
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() >= cn.p.max(1))
        .filter_map(|run| run.first().copied())
        .filter(|&x| lo < x && x < hi)
        .collect()
}

/// A proved upper bound on `sup |C(t) − S(W(t))|` over `t` in `range`, for the 3D curve
/// `curve` (`C`), a B-spline `uv` of parameters `(u, v, 0)` on the same parameter (`W`: a
/// UV curve or trim of `C` on the surface), and `surface`'s net (`S`): how far the curve
/// strays from the surface's image of its UV curve, the bound [`certified_gap`] encloses
/// (not upstream, tangency-labs/solidcraft#18166). For a surface kept as the net of a
/// definition (a blend's or a sweep's), `S` is the net: the caller adds the net's own bound.
/// `floor` is the caller's scale, as for [`curve_face_gap_bound`].
///
/// `None` when the gap is not enclosed: an empty, unordered or not finite `range`, a
/// surface that is not a NURBS net or whose net's parameters are not the surface's
/// ([`NetSurface::of`], probed at `W`'s ends and middle), a `W` that cannot be evaluated,
/// or a piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn curve_uv_gap_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &Surface,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let mut probe = Vec::new();
    for t in [lo, 0.5 * (lo + hi), hi] {
        let Ok(w) = uv.evaluate_point(t) else {
            return Ok(None);
        };
        probe.push(Point2::new(w.x, w.y));
    }
    let surface = surface.clone_for_evaluation()?;
    let Some(net) = NetSurface::of(k, &surface, &probe)? else {
        return Ok(None);
    };
    certified_gap(k, curve, uv, &net, range, FIRST_PIECES, floor)
}

/// A proved lower bound on `|C(t) − S(W(t))|` at the one parameter `t`, for the curves and
/// surface of [`curve_uv_gap_bound`]: the same enclosures of `C(t)`, `W(t)` and the net's
/// point over the box `W(t)` encloses, every rounding included, and the norm's lower end.
/// It bounds the supremum [`curve_uv_gap_bound`] encloses from below (not upstream,
/// tangency-labs/solidcraft#18195, a witness for ranking two gaps).
///
/// `None` when the gap at `t` is not enclosed: `t` outside either curve's domain or not
/// finite, a surface that is not a NURBS net or whose net's parameters are not the
/// surface's (probed at `W(t)`), or a box `W(t)` too far past the net's end. A box past the
/// end by rounding has the continued polynomial's slack taken off.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn curve_uv_gap_lower_bound(
    k: &Kernel,
    curve: &BSplineCurve,
    uv: &BSplineCurve,
    surface: &Surface,
    t: f64,
) -> Result<Option<f64>> {
    // (the homogeneous points as stored, so the curves enclosed are the ones evaluated: a
    // rebuild from the Euclidean points rounds a rational point's `x w`)
    let (Some(cn), Some(wn)) = (
        polynomial_pieces::stored_nurbs(curve),
        polynomial_pieces::stored_nurbs(uv),
    ) else {
        return Ok(None);
    };
    let inside = |n: &NurbsCurve| matches!((n.u.get(n.p), n.u.get(n.n)), (Some(&d0), Some(&d1)) if d0 <= t && t <= d1);
    if !(t.is_finite() && inside(&cn) && inside(&wn)) {
        return Ok(None);
    }
    let Ok(w) = uv.evaluate_point(t) else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let Some(net) = NetSurface::of(k, &surface, &[Point2::new(w.x, w.y)])? else {
        return Ok(None);
    };
    let at = Ivl::from(t);
    let c = curve_derivs_over(&cn, at, 0, None)?;
    let w = curve_derivs_over(&wn, at, 0, None)?;
    let Some(c0) = c.first().copied() else {
        return Ok(None);
    };
    // a box past the net's end (rounding of a parameter on the boundary) is the continued
    // polynomial `S~` there: `|C - S(clamp W)| >= |C - S~(W)| - slack`, the slack of
    // [`NetSurface::partials`] bounding `|S~(W) - S(clamp W)|`
    let (surf, slack) = match net.partials(&w, 1, 0) {
        Ok((p, slack)) => (p.at(0, 0), slack),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => return Ok(None),
    };
    Ok(norm_lower(minus(c0, surf)).map(|d| (Ivl::from(d) - Ivl::from(slack)).lo.max(0.0)))
}

/// A proved upper bound on the distance from every point of `curve` over its parameters
/// `range` to the surface of face `face` over that surface's parameter box: no point of the
/// curve there is farther from the surface. `floor` is the caller's scale (a hundredth of the
/// tolerance the bound is held to, below which a tighter bound changes nothing).
///
/// `None` when the gap is not enclosed: a face without a surface or whose surface is not a
/// NURBS net ([`NetSurface::of`]), a curve point the surface cannot be dropped from, an empty
/// or unordered `range` or one past the curve's domain, or a piece of the curve the bisection
/// cannot bound.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; the kernel's unported and cancelled statuses.
pub fn curve_face_gap_bound(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let rec = brep.face(face)?;
    let Some(surface) = rec.surface.as_ref() else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    let Some((net, correspondence)) = correspondence(k, &surface, &rec.uv_domain, curve, [lo, hi])?
    else {
        return Ok(None);
    };
    // (#20643 review: the bound is on the net's image; a cone's distance off its net added)
    let Some(off) = cone_net_offset(k, &surface, &net, &correspondence)? else {
        return Ok(None);
    };
    let gap = certified_gap(k, curve, &correspondence, &net, range, FIRST_PIECES, floor)?;
    Ok(gap.and_then(|g| plus_net_offset(g, off)))
}

/// Not upstream (#20643): the distance from the net `net` of a cone (or cylinder) `surface`
/// to the point the cone evaluates ([`net_to_surface_bound`]), for a bound taken on a net
/// [`NetSurface::of`] gives to hold for the surface; `0` for any other surface (whose net is
/// its evaluation, or whose caller adds the net's own bound), and for an offset's net
/// ([`bisection_residual`] bounds those). `None` where a cone's is not enclosed.
///
/// # Errors
///
/// The kernel's cancelled status.
pub(super) fn cone_net_offset(
    k: &Kernel,
    surface: &Surface,
    net: &NetSurface,
    uv: &BSplineCurve,
) -> Result<Option<f64>> {
    use crate::surface::surface::SurfaceKind;
    match surface.kind() {
        SurfaceKind::Cone(c) if net.offset.is_none() => {
            cone_residual::cone_net_residual_within(k, c, &net.net, uv_box(uv))
        }
        _ => Ok(Some(0.0)),
    }
}

/// Not upstream (#20643): a box `[[u0, u1], [v0, v1]]` holding every point of the UV curve
/// `uv`: the hull of its control points (each point of a NURB with positive weights is a
/// convex combination of them), each coordinate's quotient by its weight enclosed outward and
/// the box padded by `2^-40` of its largest coordinate (plus `2^-1000`) for the rounding of
/// the curve's evaluation. `None` (no box: the residual is then taken over the whole net)
/// where a control point is not finite or a weight is not positive.
fn uv_box(uv: &BSplineCurve) -> Option<[[f64; 2]; 2]> {
    nurbs_uv_box(&polynomial_pieces::stored_nurbs(uv)?)
}

/// [`uv_box`] of a UV curve's homogeneous data `n` (`(x w, y w, z w, w)` per control point).
fn nurbs_uv_box(n: &NurbsCurve) -> Option<[[f64; 2]; 2]> {
    let mut b = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for c in n.pw.chunks_exact(4) {
        let (&x, &y, &w) = (c.first()?, c.get(1)?, c.get(3)?);
        if !(x.is_finite() && y.is_finite() && w.is_finite() && w > 0.0) {
            return None;
        }
        for ([lo, hi], a) in b.iter_mut().zip([x, y]) {
            let q = Ivl::from(a) / Ivl::from(w);
            *lo = lo.min(q.lo);
            *hi = hi.max(q.hi);
        }
    }
    let big = b.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
    let pad = (Ivl::from(big) * Ivl::from(2f64.powi(-40)) + Ivl::from(2f64.powi(-1000))).hi;
    for [lo, hi] in &mut b {
        *lo = (Ivl::from(*lo) - Ivl::from(pad)).lo;
        *hi = (Ivl::from(*hi) + Ivl::from(pad)).hi;
    }
    b.iter().flatten().all(|x| x.is_finite()).then_some(b)
}

/// Not upstream (#20643): `bound` plus `off` ([`cone_net_offset`]) rounded up, `bound` as it
/// is for an `off` of `0` (an interval sum rounds outward, so adding `0` would move it an
/// ulp); `None` where it is not finite.
pub(super) fn plus_net_offset(bound: f64, off: f64) -> Option<f64> {
    let b = if off > 0.0 {
        (Ivl::from(bound) + Ivl::from(off)).hi
    } else {
        bound
    };
    b.is_finite().then_some(b)
}

/// Not upstream (#18166): a proved upper bound on the distance from every point of `curve`
/// over its parameters `range` to the surface of face `face` over that surface's parameter
/// box, as [`curve_face_gap_bound`], but one that tends to the largest distance itself as the
/// pieces shrink. [`curve_face_gap_bound`] matches the curve to the surface by one fixed
/// correspondence through the nearest points of [`CORRESPONDENCE_POINTS`] samples, so its
/// bound stops at that correspondence's own error between the samples (a curve lying in a
/// plane face, 3e-14 from it, was bounded at 4.9e-5: `primitive.api_primitives` gen0083).
/// Here every piece's two ends are matched to their own nearest points on the surface, and
/// the piece to the straight map between them in the surface's parameters ([`surface_piece`]):
/// each end value is then the end's distance to the surface, a cut drops the curve's point at
/// the cut afresh, and the second-order term shrinks with the piece.
///
/// A piece whose bound is at most `enough` closes as it stands, and so does one within the
/// largest end value `seen` so far (the bound is at least that, and no cut lowers it but by
/// rounding: the halves keep their ends' nearest points). While `seen` is at most `enough` a
/// piece above it is cut until it closes below `enough`, at [`CLOSE_ABSOLUTE`] or at the
/// bottom, so a gap just within `enough` is shown within it; once `seen` is above `enough`
/// the pieces close at a thousandth of their end values or `floor`, as
/// [`curve_face_gap_bound`]'s. Once `seen` is above `over`, so is the bound: it is infinite at
/// once. An `enough` of `f64::NEG_INFINITY` and an `over` of `f64::INFINITY` give the bound to
/// the tightness `floor` asks.
///
/// Each nearest point is a witness only (`dist(C(t), S) <= |C(t) - S(w)|` for any `w` in the
/// parameter box): where the surface's point is not dropped at a cut, the straight map's
/// middle stands for it, and the bound holds either way.
///
/// On a face whose surface is a plane (its net bilinear, [`plane_gap_bound`]) the curve's
/// control points bound the gap at once (#20373): where that bound is at most `enough` (and
/// `over`) it is the answer, and no piece is cut. Otherwise the bisection runs as without it.
///
/// `None` when the gap is not enclosed: as [`curve_face_gap_bound`], and where the nearest
/// point of one of the first pieces' ends is not found.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling face; the kernel's unported and cancelled statuses.
pub fn curve_face_gap_bound_within(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    within: [f64; 2],
) -> Result<Option<f64>> {
    Ok(curve_face_gap_bound_within_counted(k, brep, face, curve, range, floor, within)?.0)
}

/// [`curve_face_gap_bound_within`] with the number of pieces its bisection bounded (none
/// where the plane bound answers).
///
/// # Errors
///
/// Those of [`curve_face_gap_bound_within`].
pub(crate) fn curve_face_gap_bound_within_counted(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
) -> Result<(Option<f64>, usize)> {
    let [lo, hi] = range;
    // (only where the bisection would bound the gap: a valid range and floor, inside the
    // curve's domain)
    let valid = lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0;
    if valid && enough.is_finite() {
        // (a cancelled run stops here, as the bisection would: no bound under a cancel)
        k.check_cancelled(OP)?;
        let rec = brep.face(face)?;
        if let (Some(surface), Ok(cn)) = (rec.surface.as_ref(), NurbsCurve::of(curve)) {
            let inside = matches!(
                (cn.u.get(cn.p), cn.u.get(cn.n)),
                (Some(&d0), Some(&d1)) if d0 <= lo && hi <= d1
            );
            if inside {
                let surface = surface.clone_for_evaluation()?;
                if let Some(g) = plane_face_gap_bound(k, &surface, &cn, range)? {
                    if g <= enough && g <= over {
                        return Ok((Some(g), 0));
                    }
                }
            }
        }
    }
    let pieces = Cell::new(0_usize);
    let gap = bisected_face_gap_bound(k, brep, face, curve, range, floor, [enough, over], &pieces)?;
    Ok((gap, pieces.get()))
}

/// Not upstream (tangency-labs/solidcraft#21215 review): [`curve_face_gap_bound_within`]
/// against a surface held by no face, `surface` with its nearest points dropped in `domain`
/// (a face's surface as extended past its boundary, which no B-rep face carries yet): the
/// same plane bound first, then the same bisection, with the same meaning of `floor`,
/// `enough` and `over`. `None` where the gap is not enclosed, as there.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn curve_surface_gap_bound_within(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    let valid = lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0;
    if !valid {
        return Ok(None);
    }
    k.check_cancelled(OP)?;
    let surface = surface.clone_for_evaluation()?;
    if enough.is_finite() {
        if let Ok(cn) = NurbsCurve::of(curve) {
            let inside = matches!(
                (cn.u.get(cn.p), cn.u.get(cn.n)),
                (Some(&d0), Some(&d1)) if d0 <= lo && hi <= d1
            );
            if inside {
                if let Some(g) = plane_face_gap_bound(k, &surface, &cn, range)? {
                    if g <= enough && g <= over {
                        return Ok(Some(g));
                    }
                }
            }
        }
    }
    let pieces = Cell::new(0_usize);
    bisected_surface_gap_bound(
        k,
        &surface,
        domain,
        curve,
        range,
        floor,
        [enough, over],
        &pieces,
    )
}

/// Not upstream (#20373): [`plane_gap_bound`] of the curve data `cn` over `range` against the
/// net of `surface`, for a plane or a B-spline surface (whose net is the surface, as
/// [`NetSurface::of`] takes it) whose net agrees with the surface at its four corners. `None`
/// for other kinds, a net that is not bilinear, and where [`plane_gap_bound`] gives none.
///
/// It bounds the distance to the surface over its whole parameter box (the net's), not over
/// the face's `uv_domain`. That is sound: it is the quantity [`curve_face_gap_bound_within`]
/// bounds, whose bisection takes each nearest point as a witness anywhere in the box
/// (`dist(C(t), S) <= |C(t) - S(w)|` for any `w` there), so a bound over the box is an upper
/// bound on that same distance; the face's extent within the surface is its trims', which the
/// trim bounds cover ([`trim_image_gap_bound_within`]).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
fn plane_face_gap_bound(
    k: &Kernel,
    surface: &Surface,
    cn: &NurbsCurve,
    range: [f64; 2],
) -> Result<Option<f64>> {
    use crate::surface::surface::SurfaceKind;
    if !matches!(
        surface.kind(),
        SurfaceKind::Plane(_) | SurfaceKind::BSpline(_)
    ) {
        return Ok(None);
    }
    let Some(net) = NetSurface::of(k, surface, &[])? else {
        return Ok(None);
    };
    let (u, v) = net.domain();
    let corners = [
        Point2::new(u.lo, v.lo),
        Point2::new(u.hi, v.lo),
        Point2::new(u.lo, v.hi),
        Point2::new(u.hi, v.hi),
    ];
    let Some(net) = NetSurface::of(k, surface, &corners)? else {
        return Ok(None);
    };
    // (the bound is on the distance to the net: the net's own distance to the surface added)
    let (Some(g), Some(off)) = (
        plane_gap_bound(cn, &net.net, range),
        net_to_surface_bound(k, surface, &net.net)?,
    ) else {
        return Ok(None);
    };
    let bound = (Ivl::from(g) + Ivl::from(off)).hi;
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream (#20373): an upper bound on the distance from each point of the net `net`
/// (the net [`NetSurface::of`] takes for `surface`) over its parameter box to `surface` at the
/// same parameters, for a bound taken on the net ([`plane_gap_bound`],
/// [`structured_trim_gap_bound`]) to hold for the surface: `0` for a surface that evaluates by
/// that net (a B-spline, a ball patch, a sphere, a torus, a surface of revolution or of
/// extrusion: [`Surface::evaluate`] and `as_b_spline` take the same net); for a plane, which
/// evaluates by its analytic formula `O + u s_u X + v s_v Y`, the largest distance at the
/// net's four corners, each held exactly and enclosed within a few ulps
/// ([`exact_corner_offset`]; in outward-rounded interval arithmetic where that gives none). The net bilinear
/// (degree 1 by 1, two by two points of one weight, clamped knots: [`bilinear_net`]), the
/// difference of net and plane is bilinear in `(u, v)` over the box, so each of its values is
/// a convex combination of its corner values and its length is at most the largest corner
/// length. A cone (and cylinder) evaluates its point by its STEP formula at the parameters
/// converted from its net's (#20643): the distance from its net to that point over the whole
/// net is enclosed in closed form ([`cone_residual::cone_net_residual`]: the net's distance to
/// the cone's axis and height against the formula's, the polar conversion's angle against the
/// net's, and the rounding of the evaluation). `None` for any other
/// surface, any other net of a plane, and a plane whose box is not inside its analytic domain
/// while it clamps its parameters there (the plane then not affine over the box).
/// `Err` only for a cancelled kernel (the cone's enclosure polls it per piece).
fn net_to_surface_bound(k: &Kernel, surface: &Surface, net: &Nurbs) -> Result<Option<f64>> {
    net_to_surface_bound_within(k, surface, net, None)
}

/// [`net_to_surface_bound`] over the net's parameters in `window` (`[[u0, u1], [v0, v1]]`,
/// the whole net where `None`): a cone's residual taken over that box only
/// ([`cone_residual::cone_net_residual_within`]), every other surface's as over the whole net
/// (#20643: a trim's image lies over the box of its UV curve, [`nurbs_uv_box`], so the seam's
/// tear of a full-turn cone counts only for a trim that reaches the seam).
fn net_to_surface_bound_within(
    k: &Kernel,
    surface: &Surface,
    net: &Nurbs,
    window: Option<[[f64; 2]; 2]>,
) -> Result<Option<f64>> {
    use crate::surface::surface::SurfaceKind;
    Ok(match surface.kind() {
        SurfaceKind::BSpline(_)
        | SurfaceKind::BallPatch(_)
        | SurfaceKind::Sphere(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_) => Some(0.0),
        SurfaceKind::Cone(c) => return cone_residual::cone_net_residual_within(k, c, net, window),
        SurfaceKind::Plane(plane) => plane_net_to_surface_bound(plane, net),
        _ => None,
    })
}

/// [`net_to_surface_bound`] of a plane: the largest distance at the net's four corners.
fn plane_net_to_surface_bound(plane: &crate::surface::plane::Plane, net: &Nurbs) -> Option<f64> {
    bilinear_net(net)?;
    let (&u0, &u1, &v0, &v1) = (net.u.get(1)?, net.u.get(2)?, net.v.get(1)?, net.v.get(2)?);
    let ivl3 = |p: crate::math::Vec3| V3 {
        x: Ivl::from(p.x),
        y: Ivl::from(p.y),
        z: Ivl::from(p.z),
    };
    let position = plane.position();
    let (o, x, y) = (
        ivl3(position.origin()),
        ivl3(position.x_axis()),
        ivl3(position.y_axis()),
    );
    let scale = plane.uv_scale();
    let mut worst = 0.0_f64;
    // (the net's control points: u-major, `P_ij` at `(i nv + j) 4`)
    for (i, u) in [u0, u1].into_iter().enumerate() {
        for (j, v) in [v0, v1].into_iter().enumerate() {
            if !plane.is_out_of_bounds_enabled()
                && !plane
                    .step_uv_domain()
                    .contains_point(Point2::new(u, v), 0.0)
            {
                return None;
            }
            let s = net.pw.get(4 * (2 * i + j)..4 * (2 * i + j) + 4)?;
            let p = [*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?];
            let exact = exact_corner_offset(p, position, [u, v], [scale.x, scale.y]);
            let corner = if let Some(d) = exact {
                d
            } else {
                let on_net = point_of(p)?;
                let on_plane = plus(
                    o,
                    plus(
                        scaled(x, Ivl::from(u) * Ivl::from(scale.x)),
                        scaled(y, Ivl::from(v) * Ivl::from(scale.y)),
                    ),
                );
                minus(on_net, on_plane)
            };
            worst = worst.max(norm_upper(corner)?);
        }
    }
    worst.is_finite().then_some(worst)
}

/// Not upstream (#20373 review r4): the difference `P - (O + (u s_u) X + (v s_v) Y)` of a net
/// corner `P` of unit weight (`p`, homogeneous) from the plane `position` at the corner's
/// parameters, each coordinate held exactly as an expansion ([`crate::geom::expansion`]: the
/// products `u s_u`, their products with the axes and the sums are error-free) and enclosed by
/// summing its components outward, least significant first, so that the enclosure is within a
/// few ulps of the exact coordinate (an interval evaluation of the formula widens by an ulp of
/// the corner's size at each step: at `|u| = 1.2e6` about `2e-9`, 12 to 21 times the distance).
/// `None` for another weight and where an expansion product is out of range (then the caller
/// encloses the formula in intervals).
fn exact_corner_offset(
    p: [f64; 4],
    position: &crate::geom::axis2_placement::Axis2Placement,
    [u, v]: [f64; 2],
    [su, sv]: [f64; 2],
) -> Option<V3<Ivl>> {
    use crate::geom::expansion::{product, scale, sub};
    let [px, py, pz, w] = p;
    if w != 1.0 || !(px.is_finite() && py.is_finite() && pz.is_finite()) {
        return None;
    }
    let (o, x, y) = (position.origin(), position.x_axis(), position.y_axis());
    let (us, vs) = (product(u, su)?, product(v, sv)?);
    let coordinate = |pk: f64, ok: f64, xk: f64, yk: f64| -> Option<Ivl> {
        let d = sub(&sub(&sub(&[pk], &[ok]), &scale(&us, xk)?), &scale(&vs, yk)?);
        let mut parts = d.iter().copied();
        let first = Ivl::from(parts.next().unwrap_or(0.0));
        let sum = parts.fold(first, |acc, c| acc + Ivl::from(c));
        sum.is_finite().then_some(sum)
    };
    Some(V3 {
        x: coordinate(px, o.x, x.x, y.x)?,
        y: coordinate(py, o.y, x.y, y.y)?,
        z: coordinate(pz, o.z, x.z, y.z)?,
    })
}

/// Not upstream (#20373 review r4): the net's own distance to the surface the bisections
/// ([`trim_image_gap_bound_within`], [`curve_face_gap_bound_within`]) add to the bound they
/// take on `net`: [`net_to_surface_bound`] of `surface`, and for an offset (whose net is its
/// base's, moved by the offset along the net's normals) `0` where the base evaluates by that
/// net; `None` where it is not enclosed (the bisection then gives no bound): a surface or
/// net [`net_to_surface_bound`] does not bound, and an offset of a base whose net is off it
/// (the offset's normals are then the base's, not the net's, which no residual of points
/// bounds). `window` limits a cone's residual to the net's parameters in that box
/// ([`net_to_surface_bound_within`]: a trim's UV box, [`nurbs_uv_box`]); the whole net where
/// `None`.
fn bisection_residual(
    k: &Kernel,
    surface: &Surface,
    net: &NetSurface,
    window: Option<[[f64; 2]; 2]>,
) -> Result<Option<f64>> {
    use crate::surface::surface::SurfaceKind;
    Ok(match (net.offset, surface.kind()) {
        (None, _) => return net_to_surface_bound_within(k, surface, &net.net, window),
        (Some(_), SurfaceKind::OffsetSurface(off)) => {
            let base_surface = off.base_surface();
            if matches!(base_surface.kind(), SurfaceKind::Cone(_)) {
                // (#20643 review) An offset of a cone evaluates its point (`point_with`) as the
                // cone's point plus the distance along the normal of the net's first
                // partials, and the cone's point is its formula's (`Cone::evaluate_rows`),
                // the partials its net's: the offset is off the net's offset by exactly the
                // base's distance off its net, at any distance. (The step to the formula's
                // normal where `|S_u × S_v|² < EFF_ZERO_SQ` is never reached on a box
                // [`NetSurface::partials`] bounds: it refuses one whose cross product may be
                // under `1e3 EFF_ZERO`.) Not counted here, as for every offset base: the
                // rounding of that step's normal and of its product with the distance.
                return net_to_surface_bound_within(k, base_surface, &net.net, window);
            }
            match net_to_surface_bound(k, base_surface, &net.net)? {
                Some(base) => (base == 0.0).then_some(0.0),
                None => None,
            }
        }
        (Some(_), _) => None,
    })
}

/// Not upstream (#20373): a proved bound on the distance from every point of the curve `cn`
/// over `range` to the net `net` over its parameter box, for a net of degree 1 by 1 with two by
/// two control points of one positive weight: the bilinear `S(a, b) = P00 + a E1 + b E2 + a b T`
/// over `a, b` in `[0, 1]` (`E1 = P10 - P00`, `E2 = P01 - P00`, `T` the twist `P11 - P10 - P01 +
/// P00`; a plane's net has `T` zero up to the rounding of its corners).
///
/// A rational curve of positive weights is at each parameter a convex combination of the
/// control points whose basis functions do not vanish there: over `range`, of those `P_i`
/// whose knot span `[u_i, u_{i+p+1}]` meets it. The orthogonal projection onto the plane
/// through `P00` spanned by `E1, E2` is affine, so the projection `F(t)` of the curve's point
/// `C(t)` has the same convex combination of the control points' projections' parameters
/// `(a_i, b_i)`, and lies in `[0, 1]^2` when each of those does. `S(a(t), b(t))` is then a
/// point of the net over its box, and
///
/// `dist(C(t), S) <= |C(t) - F(t)| + |a b| |T| <= max_i |N . (P_i - P00)| / |N| + |T|`
///
/// (`N = E1 x E2`; `|N . (C - P00)|` is the convex combination of the control points' values,
/// at most the largest). Every quantity is enclosed in outward-rounded interval arithmetic
/// from the stored homogeneous data (the points `P = (x w, y w, z w) / w`), so the bound counts
/// the rounding; the parameters `(a_i, b_i)` are `((d x E2) . N, (E1 x d) . N) / |N|^2` for
/// `d = P_i - P00`.
///
/// `None` (the caller bisects instead) for any other net, an empty, unordered or not finite
/// `range`, a weight of the curve's that is not positive or not finite, a degenerate net (`|N|^2`
/// not positive), a projected control point not inside the box, or a bound that is not finite.
pub(crate) fn plane_gap_bound(cn: &NurbsCurve, net: &Nurbs, [lo, hi]: [f64; 2]) -> Option<f64> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return None;
    }
    let ([p00, e1, e2], twist) = bilinear_net(net)?;
    let normal = cross(e1, e2);
    let nn = dot(normal, normal);
    if !nn.is_positive() {
        return None;
    }
    let length = nn.sqrt();
    let inside = |x: Ivl| x.lo >= 0.0 && x.hi <= 1.0;
    let mut worst = 0.0_f64;
    let mut any = false;
    for i in 0..cn.n {
        let (Some(&a), Some(&b)) = (cn.u.get(i), cn.u.get(i + cn.p + 1)) else {
            return None;
        };
        // (only control points whose basis function may not vanish somewhere in `range`)
        if !(a <= hi && b >= lo) {
            continue;
        }
        let s = cn.pw.get(4 * i..4 * i + 4)?;
        let q = point_of([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])?;
        let d = minus(q, p00);
        let alpha = dot(cross(d, e2), normal) / nn;
        let beta = dot(cross(e1, d), normal) / nn;
        if !(inside(alpha) && inside(beta)) {
            return None;
        }
        let height = Ivl::from(dot(normal, d).mag()) / length;
        if !height.is_finite() {
            return None;
        }
        worst = worst.max(height.hi);
        any = true;
    }
    let bound = (Ivl::from(worst) + Ivl::from(twist)).hi;
    (any && bound.is_finite()).then_some(bound)
}

/// Not upstream (#20373): a proved bound on the trim image gap of [`trim_image_gap_bound_within`]
/// (`max |C(ψ(s)) - S(W(s))|` over `range` for a continuous `ψ` of `range` onto itself with
/// its ends fixed) of the curve data `cn` against the trim data `wn` on the net `net`, where
/// the image `S(W)` is, up to a plane net's twist, a curve with the same rational structure as
/// the curve: then each of its points is the same convex combination of its control points
/// `D_i` as the curve's corresponding point is of the curve's `C_i`, and the gap is at most
/// `max_i |C_i - D_i|`. Two cases, each with every weight positive:
///
/// - **A trim on a plane with the curve's knots and weights** (bit for bit): the net bilinear
///   as [`plane_gap_bound`] takes it, `S(a, b) = F(a, b) + a b T` with `F` affine. `F(W)` is
///   the rational curve with control points `F(q_i)` and the trim's weights, the curve's, over
///   the curve's knots, so `C(t) - F(W(t)) = Σ R_i(t) (C_i - F(q_i))` with `R_i >= 0`
///   summing to 1, and `|a b T| <= |T|` where every `q_i` (the trim's points in the net's box
///   fractions) is in `[0, 1]^2`, so the trim is (convex hull). `ψ` is the identity; only
///   the control points whose span meets `range` count.
/// - **A trim of degree 1 along the whole `u` side of a net of degree 1 in `v`** (or along the
///   `v` side of a net of degree 1 in `u`, taken on the net with `u` and `v` swapped,
///   `swapped_uv`: an extrude's side as the stitch's orientation leaves it, #20373): `W(s)` runs
///   from `(u_a, v)` to `(u_b, v)` at a constant `v` (the trim's weights 1, so `u(s)` is linear
///   and the `v` exact), `[u_a, u_b]` within the net's `u` domain `[u_0, u_1]` in either
///   direction (one running from `u_1`'s end compared with the row backwards), and `range`
///   the whole domain of both the trim and the curve. Where `[u_a, u_b]` falls short of `[u_0, u_1]`
///   the bound adds the row's speed times the shortfall (`row_lipschitz`). The net's row at `v` has control points `D_i = (1 - β)
///   P_i0 + β P_i1` (`β` the fraction of `v` in the net's `v` domain), weights `w_i0 = w_i1`,
///   those of the curve (bit for bit), and the curve's degree and number of points. Both
///   knot vectors clamped, with every interior knot of multiplicity the degree, each is a
///   chain of rational Bézier segments, the `j`-th of the row's and the curve's with the same
///   weights; `ψ` takes `s` to the curve's parameter at the same fraction of the `j`-th
///   segment as `u(s)` is of the row's (continuous and increasing, its ends fixed), and each
///   segment's points are the same Bernstein combination of their control points.
/// - **A trim that is its curve taken back through the net** (#22930): the curve and the
///   trim chains of rational Bézier segments on the same breakpoints, on a net of rational
///   Bézier patches; the net's patch composed with each segment of the trim is a rational
///   polynomial, and its difference with the curve's segment is bounded by its Bernstein
///   coefficients ([`composed_trim_gap_bound`]). `ψ` is the identity, so the bound is also
///   one at equal parameters.
///
/// Every quantity is enclosed in outward-rounded interval arithmetic from the stored data, so
/// the bound counts the rounding. `None` (the caller bisects) for any other structure.
/// (On a kernel that is never cancelled, so its only error does not arise.)
#[cfg(test)]
pub(crate) fn structured_trim_gap_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    range: [f64; 2],
) -> Option<f64> {
    structured_trim_gap_bound_as(&Kernel::default(), cn, wn, net, range, false)
        .ok()
        .flatten()
}

/// [`structured_trim_gap_bound`], or with `same_parameter` (#22034) a bound on the gap at
/// equal parameters, `max |C(s) - S(W(s))|` over `range` (the gap
/// [`trim_same_parameter_gap_bound_within`] bounds): the first case's `ψ` is the identity, so
/// its bound is that gap's already; the second case adds how far its `ψ` moves the curve's
/// parameter ([`iso_row_gap_bound`]).
///
/// # Errors
///
/// The kernel's cancelled status (the third case polls it on each segment).
fn structured_trim_gap_bound_as(
    k: &Kernel,
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
    same_parameter: bool,
) -> Result<Option<f64>> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Ok(None);
    }
    if cn.pw.len() != 4 * cn.n || wn.pw.len() != 4 * wn.n {
        return Ok(None);
    }
    // (a plane net: the curve's own structure; where that does not apply, the iso case below
    // may)
    if let Some(bound) = bilinear_trim_gap_bound(cn, wn, net, [lo, hi]) {
        return Ok(Some(bound));
    }
    // (an iso line along the `u` side of a net of degree 1 in `v`, or along a linear
    // ruling of one of degree 1 in `v`, or along the `v` side / ruling of one of degree 1
    // in `u`: the same line on the net with `u` and `v` swapped; at equal parameters also
    // one along the net's first or last row or column, whatever its degree across it,
    // [`end_row`])
    if net.q == 1 || same_parameter {
        if let Some(bound) = iso_row_gap_bound(cn, wn, net, [lo, hi], same_parameter) {
            return Ok(Some(bound));
        }
        if net.q == 1 {
            if let Some(bound) = iso_ruling_gap_bound(cn, wn, net, [lo, hi]) {
                return Ok(Some(bound));
            }
        }
    }
    if net.p == 1 || same_parameter {
        if let Some((swapped, sw)) = swapped_uv(net, wn) {
            if let Some(bound) = iso_row_gap_bound(cn, &sw, &swapped, [lo, hi], same_parameter) {
                return Ok(Some(bound));
            }
            if swapped.q == 1 {
                if let Some(bound) = iso_ruling_gap_bound(cn, &sw, &swapped, [lo, hi]) {
                    return Ok(Some(bound));
                }
            }
        }
    }
    // (#22930: a trim that is its curve taken back through the net, a Bézier chain on the
    // curve's breakpoints, at equal parameters, so for either question)
    composed_trim_gap_bound(k, cn, wn, net, [lo, hi])
}

/// Not upstream (#22273): at equal parameters, a trim along a piece of a row, under a curve
/// with that piece's structure (a circle a boolean split, on its cylinder's end row):
/// [`row_piece_gap_bound`] on the net, or on the net with `u` and `v` swapped
/// ([`swapped_uv`]). `Ok(None)` where neither applies.
///
/// # Errors
///
/// The kernel's cancelled status, polled as [`row_piece_gap_bound`] works through the row.
fn row_piece_trim_gap_bound(
    k: &Kernel,
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
) -> Result<Option<f64>> {
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Ok(None);
    }
    if cn.pw.len() != 4 * cn.n || wn.pw.len() != 4 * wn.n {
        return Ok(None);
    }
    if let Some(bound) = row_piece_gap_bound(k, cn, wn, net, [lo, hi]).transpose()? {
        return Ok(Some(bound));
    }
    let Some((net, wn)) = swapped_uv(net, wn) else {
        return Ok(None);
    };
    row_piece_gap_bound(k, cn, &wn, &net, [lo, hi]).transpose()
}

/// Not upstream (#20373): an enclosure `(lo, hi)` of the exact product `a w`: the rounded
/// product twice where it is exact, the outward-rounded interval product otherwise. The
/// product `r = fl(a w)` is exact where `a` is zero, or where the fused residual
/// `fma(a, w, -r)` (the exact `a w - r` rounded once) is zero and the residual cannot have
/// rounded to zero: with `|r| >= 2^-969` the exact product's lowest bit is at least
/// `2^-1074`, the smallest subnormal, so the residual is representable and the fused one is
/// it. Below that (`|r|` in `[2^-970, 2^-969)` has a lowest bit down to `2^-1075`) a
/// residual of `2^-1075` ties to the even `0` (#20373 review r4: `a = w 2^971 = 2 - 2^-52`,
/// `r = (2 - 2^-51) 2^-970`, `a w - r = 2^-1075`, the fused residual `0`).
fn product_enclosure(a: f64, w: f64) -> (f64, f64) {
    let r = a * w;
    let exact = a == 0.0
        || (r.is_finite()
            && r.abs() >= 2.0 * f64::MIN_POSITIVE / f64::EPSILON
            && a.mul_add(w, -r) == 0.0);
    if exact {
        (r, r)
    } else {
        let enclosed = Ivl::from(a) * Ivl::from(w);
        (enclosed.lo, enclosed.hi)
    }
}

/// The first case of [`structured_trim_gap_bound`]: a trim on a bilinear net with the curve's
/// knots and weights. Every trim point `q_i = (x_i, y_i) / w_i` must lie in the net's box
/// `[u_0, u_1] x [v_0, v_1]` (the convex hull, so `|a b T| <= |T|`); that is decided from the
/// stored data, `u_0 w_i <= x_i <= u_1 w_i` (and so in `v`, `w_i > 0`): exactly for `w_i = 1`
/// and for a product that rounds to itself (a zero factor, or a zero fused residual),
/// otherwise with the product enclosed (outward-rounded, so a point is taken as inside only
/// where it is). A point on the box's edge (an extrude's side, whose trims run along its net's
/// edges: #20373) is inside; an enclosure of its fraction `(x_i / w_i - u_0) / (u_1 - u_0)`
/// rounds past `[0, 1]`, so the fractions only enclose `F(q_i)`. `None` where it does not apply.
fn bilinear_trim_gap_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
) -> Option<f64> {
    let point = |pw: &[f64], i: usize| -> Option<(V3<Ivl>, f64)> {
        let s = pw.get(4 * i..4 * i + 4)?;
        let [x, y, z, w] = [*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?];
        Some((point_of([x, y, z, w])?, w))
    };
    {
        let (corners, twist) = bilinear_net(net)?;
        if !(wn.p == cn.p && wn.n == cn.n && wn.u == cn.u) {
            return None;
        }
        let [p00, e1, e2] = corners;
        let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
            (net.u.first(), net.u.last(), net.v.first(), net.v.last())
        else {
            return None;
        };
        let (du, dv) = (Ivl::from(u1) - Ivl::from(u0), Ivl::from(v1) - Ivl::from(v0));
        if !(du.is_positive() && dv.is_positive()) {
            return None;
        }
        // (`x / w` in `[a, b]` for `w > 0`, from the stored `x` and `w`: `a w <= x <= b w`,
        // each product compared exactly where it rounds to itself, enclosed otherwise)
        let within = |x: f64, w: f64, a: f64, b: f64| {
            if !(w > 0.0 && w.is_finite()) {
                return false;
            }
            if w == 1.0 {
                return a <= x && x <= b;
            }
            product_enclosure(a, w).1 <= x && x <= product_enclosure(b, w).0
        };
        let mut worst = 0.0_f64;
        let mut any = false;
        for i in 0..cn.n {
            let (Some(&a), Some(&b)) = (cn.u.get(i), cn.u.get(i + cn.p + 1)) else {
                return None;
            };
            let (c, wc) = point(&cn.pw, i)?;
            let (q, wq) = point(&wn.pw, i)?;
            if wc.to_bits() != wq.to_bits() {
                return None;
            }
            // (every point of the trim: the hull's, so the twist term is at most `|T|`)
            let s = wn.pw.get(4 * i..4 * i + 2)?;
            if !(within(*s.first()?, wq, u0, u1) && within(*s.get(1)?, wq, v0, v1)) {
                return None;
            }
            let alpha = (q.x - Ivl::from(u0)) / du;
            let beta = (q.y - Ivl::from(v0)) / dv;
            if !(a <= hi && b >= lo) {
                continue;
            }
            let f = plus(plus(p00, scaled(e1, alpha)), scaled(e2, beta));
            worst = worst.max(norm_upper(minus(c, f))?);
            any = true;
        }
        let bound = (Ivl::from(worst) + Ivl::from(twist)).hi;
        (any && bound.is_finite()).then_some(bound)
    }
}

/// The net `net` with `u` and `v` swapped, `S'(a, b) = S(b, a)`, and the trim `wn` with its
/// two coordinates swapped, `W'(s) = (v(s), u(s))`, so that `S'(W'(s)) = S(W(s))` at every
/// `s` (the data copied, nothing computed), for [`structured_trim_gap_bound`]. `None` for
/// data whose sizes do not agree.
fn swapped_uv(net: &Nurbs, wn: &NurbsCurve) -> Option<(Nurbs, NurbsCurve)> {
    if net.pw.len() != 4 * net.nu * net.nv || wn.pw.len() != 4 * wn.n {
        return None;
    }
    let mut pw = Vec::new();
    for j in 0..net.nv {
        for i in 0..net.nu {
            let k = 4 * (i * net.nv + j);
            pw.extend_from_slice(net.pw.get(k..k + 4)?);
        }
    }
    let mut tw = Vec::new();
    for c in wn.pw.chunks_exact(4) {
        if let [x, y, z, w] = *c {
            tw.extend_from_slice(&[y, x, z, w]);
        }
    }
    let net = Nurbs {
        p: net.q,
        q: net.p,
        nu: net.nv,
        nv: net.nu,
        u: net.v.clone(),
        v: net.u.clone(),
        pw,
    };
    let wn = NurbsCurve {
        pw: tw,
        ..wn.clone()
    };
    Some((net, wn))
}

/// The second case of [`structured_trim_gap_bound`]: a trim of degree 1 along the `u` side of
/// the net `net`, of degree 1 in `v`.
///
/// With `same_parameter` (#22034), a bound at equal parameters instead:
/// `|C(s) - S(W(s))| <= |C(s) - C(ψ(s))| + |C(ψ(s)) - S(W(s))|`, the second term the bound
/// above. `τ(u(s))` (the trim's `u` mapped linearly onto the whole side, as the slip below
/// takes it) is linear in `s`, from one end of the side at `lo` to the other at `hi`, and `ψ`
/// maps each row segment's fraction linearly onto the curve segment's, so `s - ψ(s)` is linear
/// over each segment and largest in size at one of their ends: at the curve's knot `t_j`,
/// `ψ(s_j) = t_j` for the `s_j` at which `τ(u(s))` reaches the row's knot facing it
/// ([`psi_shift`]). With `L` a bound on the curve's speed ([`row_lipschitz`] of the curve's
/// own points), the first term is at most `L max_j |s_j - t_j|`.
fn iso_row_gap_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
    same_parameter: bool,
) -> Option<f64> {
    let point = |pw: &[f64], i: usize| -> Option<(V3<Ivl>, f64)> {
        let s = pw.get(4 * i..4 * i + 4)?;
        let [x, y, z, w] = [*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?];
        Some((point_of([x, y, z, w])?, w))
    };
    if cn.pw.len() != 4 * cn.n || wn.pw.len() != 4 * wn.n {
        return None;
    }
    // (at equal parameters, a trim along the net's first or last row is taken whatever the
    // net's degree in `v` ([`end_row`]); the bound of [`structured_trim_gap_bound`] keeps
    // the nets of degree 1 in `v` it always took, so the bounds it gives are unchanged)
    let two_rows = net.q == 1 && net.nv == 2;
    if !(wn.p == 1 && wn.n == 2 && net.p == cn.p && net.nu == cn.n && (two_rows || same_parameter))
    {
        return None;
    }
    if net.pw.len() != 4 * net.nu * net.nv {
        return None;
    }
    let ends =
        |k: &[f64], p: usize, n: usize| -> Option<[f64; 2]> { Some([*k.get(p)?, *k.get(n)?]) };
    if ends(&wn.u, wn.p, wn.n)? != [lo, hi] {
        return None;
    }
    // (the curve's points and knots over `[lo, hi]`: its own where its domain is `[lo, hi]`,
    // and at equal parameters a line's piece over `[lo, hi]` of a longer domain, [`line_piece`])
    let (curve, knots) = if ends(&cn.u, cn.p, cn.n)? == [lo, hi] {
        let points = (0..cn.n)
            .map(|i| point(&cn.pw, i))
            .collect::<Option<Vec<_>>>()?;
        (points, cn.u.clone())
    } else if same_parameter {
        line_piece(cn, [lo, hi])?
    } else {
        return None;
    };
    let raw = |i: usize| -> Option<[f64; 4]> {
        let s = wn.pw.get(4 * i..4 * i + 4)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
    };
    let ([u_a, v_a, _, w_a], [u_b, v_b, _, w_b]) = (raw(0)?, raw(1)?);
    let (Some(&nu0), Some(&nu1), Some(&nv0), Some(&nv1)) =
        (net.u.first(), net.u.last(), net.v.first(), net.v.last())
    else {
        return None;
    };
    // (a trim running against the side's `u`, from `u_1`'s end to `u_0`'s, runs the row
    // backwards: the curve's `i`-th point then faces the row's `n - 1 - i`-th, the row's
    // segments taken in the reverse order)
    let rev = u_a > u_b;
    let (u_lo, u_hi) = if rev { (u_b, u_a) } else { (u_a, u_b) };
    if !(w_a == 1.0 && w_b == 1.0 && v_a == v_b && nu0 <= u_lo && u_lo < u_hi && u_hi <= nu1) {
        return None;
    }
    if !(bezier_chain(&knots, cn.p, cn.n) && bezier_chain(&net.u, net.p, net.nu)) {
        return None;
    }
    let mut worst = 0.0_f64;
    let row = if two_rows {
        let clamped_v = matches!(net.v.as_slice(), [a, b, c, d] if a == b && b < c && c == d);
        if !(clamped_v && nv0 <= v_a && v_a <= nv1) {
            return None;
        }
        // (`nv0 <= v_a <= nv1` exactly, so the true `beta` is in `[0, 1]` as well as in the
        // enclosure, whose outward rounding can pass an end by an ulp: the trim at the side's
        // top, `v_a == nv1`, encloses `beta` as `[1 - ulp, 1 + ulp]`)
        let beta = (Ivl::from(v_a) - Ivl::from(nv0)) / (Ivl::from(nv1) - Ivl::from(nv0));
        let beta = Ivl::new(beta.lo.max(0.0), beta.hi.min(1.0));
        if beta.lo.is_nan() || beta.hi.is_nan() || beta.lo > beta.hi {
            return None;
        }
        let one_less = Ivl::from(1.0) - beta;
        let mut row = Vec::new();
        for j in 0..net.nu {
            let (p0, w0) = point(&net.pw, 2 * j)?;
            let (p1, w1) = point(&net.pw, 2 * j + 1)?;
            if w0.to_bits() != w1.to_bits() {
                return None;
            }
            row.push((plus(scaled(p0, one_less), scaled(p1, beta)), w0));
        }
        row
    } else {
        end_row(net, v_a)?
            .into_iter()
            .map(|i| point(&net.pw, i))
            .collect::<Option<Vec<_>>>()?
    };
    for (i, &(c, wc)) in curve.iter().enumerate() {
        let &(d, w) = row.get(if rev { cn.n.checked_sub(1 + i)? } else { i })?;
        if w.to_bits() != wc.to_bits() {
            return None;
        }
        worst = worst.max(norm_upper(minus(c, d))?);
    }
    // (a trim whose ends fall short of the side's, by `slip` in `u` at most, an ulp in the
    // extrudes of #20373: its image is the row over `[u_a, u_b]`, and the row at `u` is
    // within `L |u - τ(u)| <= L slip` of the row at `τ(u)`, `τ` the linear map of
    // `[u_a, u_b]` onto the whole side keeping its direction, which the bound above holds
    // for)
    let slip = (Ivl::from(u_lo) - Ivl::from(nu0))
        .hi
        .max((Ivl::from(nu1) - Ivl::from(u_hi)).hi);
    if slip > 0.0 {
        let lip = row_lipschitz(&row, &net.u, net.p)?;
        worst = (Ivl::from(worst) + Ivl::from(lip) * Ivl::from(slip)).hi;
    }
    if same_parameter {
        let shift = psi_shift(&knots, &net.u, [nu0, nu1], [lo, hi], rev)?;
        if shift > 0.0 {
            let lip = row_lipschitz(&curve, &knots, cn.p)?;
            worst = (Ivl::from(worst) + Ivl::from(lip) * Ivl::from(shift)).hi;
        }
    }
    worst.is_finite().then_some(worst)
}

/// An analytic bound for a degree-1 trim along a constant-`u` ruling of a net of degree 1
/// in `v` (`net.q == 1`, `net.nv == 2`, equal weights along `v`) paired with a degree-1 3D
/// curve (e.g. cylinder, cone, or linear extrusion seam/tangency edges). Because the two `v`
/// rows share weights `w(i, 0) == w(i, 1)`, the rational denominator `W(u)` is independent of
/// `v`, so `S(u_a, v)` is affine in `v`; both `C(s)` and `S(W(s))` are affine in `s` on
/// `[lo, hi]`, and by convexity of the Euclidean norm
/// `|C(s) - S(W(s))| <= max(|C(lo) - S(u_a, v(lo))|, |C(hi) - S(u_a, v(hi))|)`.
#[allow(clippy::float_cmp)] // exact bit equality of clamped knots, unit weights, constant u
fn iso_ruling_gap_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
) -> Option<f64> {
    if !(cn.p == 1 && cn.n == 2 && wn.p == 1 && wn.n == 2 && net.q == 1 && net.nv == 2) {
        return None;
    }
    if net.p == 0 || net.nu == 0 || net.u.len() != net.nu + net.p + 1 {
        return None;
    }
    if cn.pw.len() != 8 || wn.pw.len() != 8 || net.pw.len() != 4 * net.nu * 2 {
        return None;
    }
    let clamped_containing = |k: &[f64]| -> Option<[f64; 2]> {
        match k {
            [a, b, c, d] if *a == *b && *c == *d && *a <= lo && hi <= *d && *a < *d => {
                Some([*a, *d])
            }
            _ => None,
        }
    };
    let [c_lo, c_hi] = clamped_containing(&cn.u)?;
    let [w_lo, w_hi] = clamped_containing(&wn.u)?;
    let clamped_v = matches!(net.v.as_slice(), [a, b, c, d] if a == b && b < c && c == d);
    if !clamped_v {
        return None;
    }
    let (&nu0, &nu1, &nv0, &nv1) = (
        net.u.get(net.p)?,
        net.u.get(net.nu)?,
        net.v.first()?,
        net.v.last()?,
    );
    let raw = |pw: &[f64], i: usize| -> Option<[f64; 4]> {
        let s = pw.get(4 * i..4 * i + 4)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
    };
    let (c0_raw, c1_raw) = (raw(&cn.pw, 0)?, raw(&cn.pw, 1)?);
    let ([_, _, _, w_c0], [_, _, _, w_c1]) = (c0_raw, c1_raw);
    let ([u_a, v_a, _, w_a], [u_b, v_b, _, w_b]) = (raw(&wn.pw, 0)?, raw(&wn.pw, 1)?);
    if !(w_c0 == 1.0 && w_c1 == 1.0 && w_a == 1.0 && w_b == 1.0) {
        return None;
    }
    if !(u_a == u_b
        && nu0 <= u_a
        && u_a <= nu1
        && nv0 <= v_a
        && v_a <= nv1
        && nv0 <= v_b
        && v_b <= nv1)
    {
        return None;
    }
    // Equal weights along `v` guarantee `S(u_a, v)` is affine in `v`.
    let mut row0_pw = Vec::new();
    let _ = row0_pw.try_reserve(4 * net.nu);
    let mut row1_pw = Vec::new();
    let _ = row1_pw.try_reserve(4 * net.nu);
    for i in 0..net.nu {
        let p0 = net.pw.get(4 * (2 * i)..4 * (2 * i) + 4)?;
        let p1 = net.pw.get(4 * (2 * i + 1)..4 * (2 * i + 1) + 4)?;
        let (&w0, &w1) = (p0.get(3)?, p1.get(3)?);
        if !(w0.is_finite() && w0 > 0.0 && w0.to_bits() == w1.to_bits()) {
            return None;
        }
        row0_pw.extend_from_slice(p0);
        row1_pw.extend_from_slice(p1);
    }
    let row0 = NurbsCurve {
        p: net.p,
        n: net.nu,
        u: net.u.clone(),
        pw: row0_pw,
    };
    let row1 = NurbsCurve {
        p: net.p,
        n: net.nu,
        u: net.u.clone(),
        pw: row1_pw,
    };
    let at_u = Ivl::from(u_a);
    let s0 = *curve_derivs_over(&row0, at_u, 0, None).ok()?.first()?;
    let s1 = *curve_derivs_over(&row1, at_u, 0, None).ok()?.first()?;
    let dv = Ivl::from(nv1) - Ivl::from(nv0);
    if !dv.is_positive() {
        return None;
    }
    let frac = |s: f64, a: f64, b: f64| -> Option<Ivl> {
        if s == a {
            return Some(Ivl::from(0.0));
        }
        if s == b {
            return Some(Ivl::from(1.0));
        }
        let span = Ivl::from(b) - Ivl::from(a);
        if !span.is_positive() {
            return None;
        }
        let t = (Ivl::from(s) - Ivl::from(a)) / span;
        let t = Ivl::new(t.lo.max(0.0), t.hi.min(1.0));
        (!t.lo.is_nan() && !t.hi.is_nan() && t.lo <= t.hi).then_some(t)
    };
    let lerp_v3 = |p0: V3<Ivl>, p1: V3<Ivl>, t: Ivl| {
        let one_less = Ivl::from(1.0) - t;
        plus(scaled(p0, one_less), scaled(p1, t))
    };
    let lerp_ivl = |a: f64, b: f64, t: Ivl| {
        let one_less = Ivl::from(1.0) - t;
        one_less * Ivl::from(a) + t * Ivl::from(b)
    };
    let blend = |v: Ivl| -> Option<V3<Ivl>> {
        let b = (v - Ivl::from(nv0)) / dv;
        let b = Ivl::new(b.lo.max(0.0), b.hi.min(1.0));
        if b.lo.is_nan() || b.hi.is_nan() || b.lo > b.hi {
            return None;
        }
        Some(lerp_v3(s0, s1, b))
    };
    let (v_lo, v_hi) = if [w_lo, w_hi] == [lo, hi] {
        (Ivl::from(v_a), Ivl::from(v_b))
    } else {
        (
            lerp_ivl(v_a, v_b, frac(lo, w_lo, w_hi)?),
            lerp_ivl(v_a, v_b, frac(hi, w_lo, w_hi)?),
        )
    };
    let sa = blend(v_lo)?;
    let sb = blend(v_hi)?;
    let (c0_pt, c1_pt) = (point_of(c0_raw)?, point_of(c1_raw)?);
    let (ca, cb) = if [c_lo, c_hi] == [lo, hi] {
        (c0_pt, c1_pt)
    } else {
        (
            lerp_v3(c0_pt, c1_pt, frac(lo, c_lo, c_hi)?),
            lerp_v3(c0_pt, c1_pt, frac(hi, c_lo, c_hi)?),
        )
    };
    let d0 = norm_upper(minus(ca, sa))?;
    let d1 = norm_upper(minus(cb, sb))?;
    let worst = d0.max(d1);
    worst.is_finite().then_some(worst)
}

/// Not upstream (#22273): [`iso_row_gap_bound`] at equal parameters for a trim of degree 1
/// along a piece `[u_lo, u_hi]` of a row of the net `net` (its first or last row, [`end_row`],
/// or the row at the trim's `v` of a net of degree 1 in `v` with two rows), under a curve with
/// the structure of that piece rather than of the whole row: a circle a boolean split at the
/// vertices where it meets the other operand, on the end row of its cylinder, whose net row is
/// the whole circle. `W(s)` runs from `(u_a, v)` to `(u_b, v)` (weights 1), and `range` is the
/// whole domain of the trim and of the curve.
///
/// The row is the rational curve of the row's homogeneous control points (at a row of a net of
/// two rows, `(1 - β) H_i0 + β H_i1`, `β` the fraction of `v`: the net is linear in `v` in
/// homogeneous space), a chain of rational Bézier segments over the net's `u` knots. Restricted
/// to `[u_lo, u_hi]` it is the chain of the segments it meets, each cut to its part `[x, y]`
/// there: the homogeneous points of a Bézier segment of degree `p` restricted to its local
/// parameters `[τ_x, τ_y]` are its blossom `b(τ_x^(p - i), τ_y^i)`, `i = 0..p` (de Casteljau,
/// with `τ_y` at `i` of its levels), and the piece at its own local parameter `σ` is the
/// segment at `τ_x + σ (τ_y - τ_x)`, linear in `u`. Enclosed in outward-rounded interval
/// arithmetic from the exact `τ`s, so the piece's points `Q_k` and weights `b_k` enclose those
/// of the exact restriction.
///
/// The curve's segment `j` (points `P_k`, weights `a_k > 0`) is compared with the piece's `j`-th
/// (for a trim running against `u`, the `j`-th from the end, reversed) at equal local parameter
/// `σ`. With `B_k = B_k(σ) >= 0` summing to 1, `A = Σ B_k a_k`, `X = Σ B_k a_k P_k / A` and
/// `Y = Σ B_k b_k Q_k / Σ B_k b_k` (so `Σ B_k b_k (Q_k - Y) = 0`), for any real `c`,
/// `X - Y = Σ B_k a_k (P_k - Q_k) / A + Σ B_k (a_k - c b_k) (Q_k - Y) / A`, so
/// `|X - Y| <= max_k |P_k - Q_k| + max_k (|a_k - c b_k| / a_k) diam(Q)`, `Y` being in the hull
/// of the `Q_k`. `c` is taken as `a_0` over the middle of `b_0`'s enclosure, so weights equal up
/// to one factor (two homogeneous forms of one rational segment) leave only their rounding.
/// `ψ` maps each piece segment's fraction onto the curve segment's as in [`iso_row_gap_bound`],
/// and the bound adds the curve's speed times how far `ψ` slips at the knots ([`psi_shift`],
/// over the piece's knots `u_lo, .., u_hi`). `None` where it does not apply: the row or the
/// curve not a chain of Bézier segments, the two of different degrees or numbers of segments,
/// or a term not finite. The kernel's cancel token is polled before each of the row's
/// segments is cut and before each of the curve's segments is compared, and a cancellation
/// is `Some(Err(..))`, the kernel's cancelled status, never a missing bound (`None`).
fn row_piece_gap_bound(
    k: &Kernel,
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
) -> Option<Result<f64>> {
    let p = cn.p;
    if !(p > 0 && net.p == p && wn.p == 1 && wn.n == 2 && wn.pw.len() == 8) {
        return None;
    }
    if cn.pw.len() != 4 * cn.n || net.pw.len() != 4 * net.nu * net.nv {
        return None;
    }
    let ends =
        |k: &[f64], p: usize, n: usize| -> Option<[f64; 2]> { Some([*k.get(p)?, *k.get(n)?]) };
    if ends(&wn.u, wn.p, wn.n)? != [lo, hi] || ends(&cn.u, cn.p, cn.n)? != [lo, hi] {
        return None;
    }
    let raw = |pw: &[f64], i: usize| -> Option<[f64; 4]> {
        let s = pw.get(4 * i..4 * i + 4)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
    };
    let curve = (0..cn.n)
        .map(|i| {
            let s = raw(&cn.pw, i)?;
            let [_, _, _, w] = s;
            Some((point_of(s)?, w))
        })
        .collect::<Option<Vec<_>>>()?;
    let ([u_a, v_a, _, w_a], [u_b, v_b, _, w_b]) = (raw(&wn.pw, 0)?, raw(&wn.pw, 1)?);
    let (Some(&nu0), Some(&nu1), Some(&nv0), Some(&nv1)) =
        (net.u.first(), net.u.last(), net.v.first(), net.v.last())
    else {
        return None;
    };
    let rev = u_a > u_b;
    let (u_lo, u_hi) = if rev { (u_b, u_a) } else { (u_a, u_b) };
    if !(w_a == 1.0 && w_b == 1.0 && v_a == v_b && nu0 <= u_lo && u_lo < u_hi && u_hi <= nu1) {
        return None;
    }
    if !(bezier_chain(&cn.u, p, cn.n) && bezier_chain(&net.u, p, net.nu)) {
        return None;
    }
    // (the row's homogeneous control points, enclosed)
    let hom = |[x, y, z, w]: [f64; 4]| -> Option<[Ivl; 4]> {
        (x.is_finite() && y.is_finite() && z.is_finite() && w.is_finite() && w > 0.0)
            .then(|| [Ivl::from(x), Ivl::from(y), Ivl::from(z), Ivl::from(w)])
    };
    let row: Vec<[Ivl; 4]> = if net.q == 1 && net.nv == 2 {
        let clamped_v = matches!(net.v.as_slice(), [a, b, c, d] if a == b && b < c && c == d);
        if !(clamped_v && nv0 <= v_a && v_a <= nv1) {
            return None;
        }
        // (`β` in `[0, 1]` exactly, as in [`iso_row_gap_bound`])
        let beta = (Ivl::from(v_a) - Ivl::from(nv0)) / (Ivl::from(nv1) - Ivl::from(nv0));
        let beta = Ivl::new(beta.lo.max(0.0), beta.hi.min(1.0));
        if beta.lo.is_nan() || beta.hi.is_nan() || beta.lo > beta.hi {
            return None;
        }
        let one_less = Ivl::from(1.0) - beta;
        (0..net.nu)
            .map(|j| {
                let (h0, h1) = (hom(raw(&net.pw, 2 * j)?)?, hom(raw(&net.pw, 2 * j + 1)?)?);
                Some(blend4(h0, h1, one_less, beta))
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        end_row(net, v_a)?
            .into_iter()
            .map(|i| hom(raw(&net.pw, i)?))
            .collect::<Option<Vec<_>>>()?
    };
    // (a trim whose end is a rounding off one of the row's knots, as a trim along a piece
    // between two of a screw wall's stations can be, is compared as the trim between those
    // knots, and the bound adds the row's speed times how far an end moved: `W(s)` and the
    // moved trim's `W'(s)` are both linear in `s`, so `|W(s) - W'(s)|` is at most the larger
    // end move, and both lie on the row between the outermost of their ends, where
    // `|S(W) - S(W')|` is at most the row's speed there ([`row_speed`]) times that. Cut at the
    // trim's own ends, the piece would start or end with a segment a rounding wide, and its
    // segments would no longer face the curve's)
    let mut breaks = net.u.clone();
    breaks.dedup();
    let (trim_lo, trim_hi) = (u_lo, u_hi);
    let snapped = |u: f64| -> f64 {
        breaks
            .iter()
            .copied()
            .find(|&r| r != u && (r - u).abs() <= ROW_KNOT_SNAP * r.abs().max(1.0))
            .unwrap_or(u)
    };
    let (u_lo, u_hi) = (snapped(u_lo), snapped(u_hi));
    if !(nu0 <= u_lo && u_lo < u_hi && u_hi <= nu1) {
        return None;
    }
    let slip = (u_lo - trim_lo).abs().max((u_hi - trim_hi).abs());
    // (the row restricted to `[u_lo, u_hi]`: each segment it meets, cut to its part there,
    // and the piece's distinct knots)
    let mut piece: Vec<[Ivl; 4]> = Vec::new();
    let mut piece_knots = vec![u_lo];
    for (j, pair) in breaks.windows(2).enumerate() {
        if let Err(e) = k.check_cancelled(OP) {
            return Some(Err(e));
        }
        let (&[r0, r1], Some(seg)) = (pair, row.get(j * p..=j * p + p)) else {
            return None;
        };
        let (x, y) = (r0.max(u_lo), r1.min(u_hi));
        if x >= y {
            continue;
        }
        let cut: Vec<[Ivl; 4]> = if x == r0 && y == r1 {
            seg.to_vec()
        } else {
            let span = Ivl::from(r1) - Ivl::from(r0);
            if !span.is_positive() {
                return None;
            }
            // (`τ_x` exactly 0 at the segment's start, `τ_y` exactly 1 at its end)
            let tx = if x == r0 {
                Ivl::from(0.0)
            } else {
                (Ivl::from(x) - Ivl::from(r0)) / span
            };
            let ty = if y == r1 {
                Ivl::from(1.0)
            } else {
                (Ivl::from(y) - Ivl::from(r0)) / span
            };
            (0..=p)
                .map(|i| {
                    let mut pts = seg.to_vec();
                    for level in 0..p {
                        let tau = if level < i { ty } else { tx };
                        let rest = Ivl::from(1.0) - tau;
                        for m in 0..p - level {
                            let (a, b) = (*pts.get(m)?, *pts.get(m + 1)?);
                            *pts.get_mut(m)? = blend4(a, b, rest, tau);
                        }
                    }
                    pts.first().copied()
                })
                .collect::<Option<Vec<_>>>()?
        };
        // (each segment after the first starts at the last point of the one before)
        let skip = usize::from(!piece.is_empty());
        piece.extend(cut.into_iter().skip(skip));
        piece_knots.push(y);
    }
    if piece.len() != cn.n {
        return None;
    }
    // (the piece's points, enclosed, with their weights)
    let piece: Vec<(V3<Ivl>, Ivl)> = piece
        .into_iter()
        .map(|[x, y, z, w]| {
            w.is_positive().then(|| {
                (
                    V3 {
                        x: x / w,
                        y: y / w,
                        z: z / w,
                    },
                    w,
                )
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let facing = |k: usize| -> Option<(V3<Ivl>, Ivl)> {
        piece
            .get(if rev { cn.n.checked_sub(1 + k)? } else { k })
            .copied()
    };
    let mut worst = 0.0_f64;
    for j in 0..cn.n.checked_sub(1)?.checked_div(p)? {
        if let Err(e) = k.check_cancelled(OP) {
            return Some(Err(e));
        }
        let seg = curve.get(j * p..=j * p + p)?;
        let (&(_, a0), (_, b0)) = (seg.first()?, facing(j * p)?);
        let c = a0 / b0.mid();
        if !(c.is_finite() && c > 0.0) {
            return None;
        }
        let (mut points, mut weights, mut diam) = (0.0_f64, 0.0_f64, 0.0_f64);
        for (i, &(pk, ak)) in seg.iter().enumerate() {
            let (qk, bk) = facing(j * p + i)?;
            points = points.max(norm_upper(minus(pk, qk))?);
            let off = Ivl::from((Ivl::from(ak) - Ivl::from(c) * bk).mag()) / Ivl::from(ak);
            weights = weights.max(off.hi);
            for other in 0..=p {
                diam = diam.max(norm_upper(minus(qk, facing(j * p + other)?.0))?);
            }
        }
        let bound = (Ivl::from(points) + Ivl::from(weights) * Ivl::from(diam)).hi;
        worst = worst.max(bound);
    }
    let shift = psi_shift(&cn.u, &piece_knots, [u_lo, u_hi], [lo, hi], rev)?;
    if shift > 0.0 {
        let lip = row_lipschitz(&curve, &cn.u, p)?;
        worst = (Ivl::from(worst) + Ivl::from(lip) * Ivl::from(shift)).hi;
    }
    if slip > 0.0 {
        let reach = [trim_lo.min(u_lo), trim_hi.max(u_hi)];
        let speed = row_speed(&row, &breaks, p, reach)?;
        worst = (Ivl::from(worst) + Ivl::from(speed) * Ivl::from(slip)).hi;
    }
    worst.is_finite().then_some(Ok(worst))
}

/// How far, relative to the knot (and at least absolutely), an end of a trim along a row may
/// be from one of the row's knots and still be compared as the trim from that knot
/// ([`row_piece_gap_bound`]): a few roundings. The bound counts the move, so any value would be
/// sound; this only says where the move is tried.
const ROW_KNOT_SNAP: f64 = 64.0 * f64::EPSILON;

/// Not upstream: an upper bound on the speed `|r'|` of the row `row` (its homogeneous control
/// points, enclosed: a chain of rational Bézier segments of degree `p` over the distinct knots
/// `breaks`) over the segments that meet `[a, b]`, as [`row_lipschitz`] bounds a chain's:
/// `2p w_max diam / ((r_1 - r_0) w_min)` on a segment over `[r_0, r_1]`. `None` for data not
/// of that shape (a weight not positive) or a bound that is not finite.
fn row_speed(row: &[[Ivl; 4]], breaks: &[f64], p: usize, [a, b]: [f64; 2]) -> Option<f64> {
    let mut most = 0.0_f64;
    for (j, pair) in breaks.windows(2).enumerate() {
        let &[r0, r1] = pair else {
            return None;
        };
        if r1 < a || r0 > b {
            continue;
        }
        let seg = row.get(j * p..=j * p + p)?;
        let span = Ivl::from(r1) - Ivl::from(r0);
        if !span.is_positive() {
            return None;
        }
        let (mut w_max, mut w_min) = (0.0_f64, f64::INFINITY);
        let mut points = Vec::new();
        for &[x, y, z, w] in seg {
            if !w.is_positive() {
                return None;
            }
            w_max = w_max.max(w.hi);
            w_min = w_min.min(w.lo);
            points.push(V3 {
                x: x / w,
                y: y / w,
                z: z / w,
            });
        }
        if w_min.is_nan() || w_min <= 0.0 {
            return None;
        }
        let mut diam = 0.0_f64;
        for x in &points {
            for y in &points {
                diam = diam.max(norm_upper(minus(*x, *y))?);
            }
        }
        let speed = Ivl::from(2.0 * p as f64) * Ivl::from(w_max) * Ivl::from(diam)
            / (span * Ivl::from(w_min));
        most = most.max(speed.hi);
    }
    most.is_finite().then_some(most)
}

/// `s a + t b` of two homogeneous points, enclosed: one step of de Casteljau for
/// [`row_piece_gap_bound`].
fn blend4([a0, a1, a2, a3]: [Ivl; 4], [b0, b1, b2, b3]: [Ivl; 4], s: Ivl, t: Ivl) -> [Ivl; 4] {
    [
        s * a0 + t * b0,
        s * a1 + t * b1,
        s * a2 + t * b2,
        s * a3 + t * b3,
    ]
}

/// A curve's control points (enclosed) with their weights, and its knots.
type WeightedPiece = (Vec<(V3<Ivl>, f64)>, Vec<f64>);

/// Not upstream (#22034): the piece over `[lo, hi]` of the curve `cn` of degree 1 with two
/// control points of one positive weight (a line's data, whose domain may be far longer than
/// the edge's interval), as its two control points, enclosed, with that weight, and its knots
/// `[lo, lo, hi, hi]`. With equal weights the curve is affine in its parameter,
/// `C(t) = P_0 + (t - a) / (b - a) (P_1 - P_0)` over its domain `[a, b]`, so the piece is the
/// curve of degree 1 from `C(lo)` to `C(hi)` over `[lo, hi]`, at the same parameters. `None`
/// for any other curve, or `[lo, hi]` not within its domain.
fn line_piece(cn: &NurbsCurve, [lo, hi]: [f64; 2]) -> Option<WeightedPiece> {
    if !(cn.p == 1 && cn.n == 2 && cn.pw.len() == 8) {
        return None;
    }
    let (Some(&a), Some(&b)) = (cn.u.get(1), cn.u.get(2)) else {
        return None;
    };
    if !(a < b && a <= lo && lo < hi && hi <= b) {
        return None;
    }
    let at = |i: usize| -> Option<(V3<Ivl>, f64)> {
        let s = cn.pw.get(4 * i..4 * i + 4)?;
        let w = *s.get(3)?;
        Some((point_of([*s.first()?, *s.get(1)?, *s.get(2)?, w])?, w))
    };
    let ((p0, w0), (p1, w1)) = (at(0)?, at(1)?);
    if w0.to_bits() != w1.to_bits() {
        return None;
    }
    let span = Ivl::from(b) - Ivl::from(a);
    let on = |t: f64| {
        plus(
            p0,
            scaled(minus(p1, p0), (Ivl::from(t) - Ivl::from(a)) / span),
        )
    };
    Some((vec![(on(lo), w0), (on(hi), w0)], vec![lo, lo, hi, hi]))
}

/// Not upstream (#22034, #22820): the indices in `net.pw` of the net's row of control points
/// interpolated directly at `v` (for every `u` index):
/// - row `0` where `v` is the start of clamped `v` knots;
/// - row `net.nv - 1` where `v` is their end;
/// - row `first - 1` where `v` is an interior `C^0` knot of exact multiplicity `q` (`q > 0`,
///   appearing at indices `first..first + q`, as at a sphere's equator `v = 0.5`).
///
/// `None` for any other `v`, or `v` knots that are not clamped and non-decreasing. At each
/// such `v`, the basis function in `v` for that row is identically `1` from both sides and
/// every other basis function is `0`, so the net's points at that `v` are exactly the rational
/// curve of that row's points and weights, of the net's `u` knots and degree.
fn end_row(net: &Nurbs, v: f64) -> Option<Vec<usize>> {
    let q = net.q;
    if net.nv == 0 || net.v.len() != net.nv + q + 1 {
        return None;
    }
    let (Some(&v0), Some(&v1)) = (net.v.first(), net.v.last()) else {
        return None;
    };
    let (start, end) = (net.v.get(..=q)?, net.v.get(net.nv..)?);
    if !(v0 < v1 && start.iter().all(|&x| x == v0) && end.iter().all(|&x| x == v1)) {
        return None;
    }
    let k = if v == v0 {
        0
    } else if v == v1 {
        net.nv.checked_sub(1)?
    } else if q > 0 && v0 < v && v < v1 {
        if !net.v.windows(2).all(|w| matches!(w, [a, b] if a <= b)) {
            return None;
        }
        let first = net.v.partition_point(|&x| x < v);
        let after = net.v.partition_point(|&x| x <= v);
        if after.checked_sub(first)? != q || first <= q || after > net.nv {
            return None;
        }
        first.checked_sub(1)?
    } else {
        return None;
    };
    Some((0..net.nu).map(|j| j * net.nv + k).collect())
}

/// Not upstream (#22034): an upper bound on `max |s - ψ(s)|` over `[lo, hi]`, for
/// [`iso_row_gap_bound`] at equal parameters: the largest `|s_j - t_j|` over the distinct knots
/// `t_j` of `curve_knots` (a chain of Bézier segments over `[lo, hi]`), where
/// `s_j = lo + (r - u_0) (hi - lo) / (u_1 - u_0)` for the row's knot `r` facing `t_j`: the
/// `j`-th distinct knot of `row_knots` (over the side `[u_0, u_1]`), or with `rev` (the trim
/// running the row backwards) the `j`-th from the end, with `u_1 - r` for `r - u_0`. Enclosed
/// in outward-rounded interval arithmetic. `None` where the two chains have not the same
/// number of segments, or the bound is not finite.
fn psi_shift(
    curve_knots: &[f64],
    row_knots: &[f64],
    [u0, u1]: [f64; 2],
    [lo, hi]: [f64; 2],
    rev: bool,
) -> Option<f64> {
    let distinct = |k: &[f64]| {
        let mut d = k.to_vec();
        d.dedup();
        d
    };
    let (t, r) = (distinct(curve_knots), distinct(row_knots));
    if t.len() != r.len() || t.len() < 2 {
        return None;
    }
    let side = Ivl::from(u1) - Ivl::from(u0);
    if !side.is_positive() {
        return None;
    }
    let length = Ivl::from(hi) - Ivl::from(lo);
    let mut most = 0.0_f64;
    for (j, &tj) in t.iter().enumerate() {
        let x = if rev {
            Ivl::from(u1) - Ivl::from(*r.iter().rev().nth(j)?)
        } else {
            Ivl::from(*r.get(j)?) - Ivl::from(u0)
        };
        let s = Ivl::from(lo) + x * length / side;
        let d = (s - Ivl::from(tj)).mag();
        if !d.is_finite() {
            return None;
        }
        most = most.max(d);
    }
    Some(most)
}

/// An upper bound on the speed `|r'|` of the chain of rational Bézier segments with the
/// control points and weights `row` of degree `p` over the knots `k` ([`bezier_chain`]'s), for
/// [`structured_trim_gap_bound`]. On a segment over `[a, b]` with points `D_k`, weights `w_k`
/// and `W = Σ w_k B_k`, `r' = Σ (w_k B_k)' (D_k - r) / W` (as `Σ (w_k B_k)' = W'`), where
/// `Σ |B_k'| <= 2p / (b - a)` (each `B_k'` is `p / (b - a)` times a difference of two
/// Bernstein polynomials of degree `p - 1`, which sum to 1) and `|D_k - r|` is at most the
/// largest distance between two of the segment's points (`r` in their hull), so
/// `|r'| <= 2p w_max diam / ((b - a) w_min)`; the chain's speed is at most the largest
/// segment's. `None` for data not of that shape or a bound that is not finite.
fn row_lipschitz(row: &[(V3<Ivl>, f64)], k: &[f64], p: usize) -> Option<f64> {
    let spans = row.len().checked_sub(1)?;
    if p == 0 || spans < p || spans.checked_rem(p)? != 0 {
        return None;
    }
    let mut most = 0.0_f64;
    for j in 0..spans.checked_div(p)? {
        let seg = row.get(j * p..=j * p + p)?;
        let (a, b) = (*k.get(p + j * p)?, *k.get(p + (j + 1) * p)?);
        let span = Ivl::from(b) - Ivl::from(a);
        if !span.is_positive() {
            return None;
        }
        let mut diam = 0.0_f64;
        for (x, _) in seg {
            for (y, _) in seg {
                diam = diam.max(norm_upper(minus(*x, *y))?);
            }
        }
        let (mut w_max, mut w_min) = (0.0_f64, f64::INFINITY);
        for &(_, w) in seg {
            w_max = w_max.max(w);
            w_min = w_min.min(w);
        }
        if w_min.is_nan() || w_min <= 0.0 {
            return None;
        }
        let speed = Ivl::from(2.0 * p as f64) * Ivl::from(w_max) * Ivl::from(diam)
            / (span * Ivl::from(w_min));
        most = most.max(speed.hi);
    }
    most.is_finite().then_some(most)
}

/// The corner `P00`, the sides `E1 = P10 - P00`, `E2 = P01 - P00` and an upper bound on the
/// twist `|P11 - P10 - P01 + P00|` of a net of degree 1 by 1 with two by two control points of
/// one positive weight over clamped knots, enclosed; `None` for any other net.
fn bilinear_net(net: &Nurbs) -> Option<([V3<Ivl>; 3], f64)> {
    if !(net.p == 1 && net.q == 1 && net.nu == 2 && net.nv == 2 && net.pw.len() == 16) {
        return None;
    }
    let clamped = |k: &[f64]| matches!(k, [a, b, c, d] if a == b && b < c && c == d);
    if !(clamped(&net.u) && clamped(&net.v)) {
        return None;
    }
    // (the net's control points: u-major, `P_ij` at `(i nv + j) 4`)
    let corner = |i: usize| -> Option<(V3<Ivl>, f64)> {
        let s = net.pw.get(4 * i..4 * i + 4)?;
        let [x, y, z, w] = [*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?];
        Some((point_of([x, y, z, w])?, w))
    };
    let (p00, w00) = corner(0)?;
    let (p01, w01) = corner(1)?;
    let (p10, w10) = corner(2)?;
    let (p11, w11) = corner(3)?;
    if !(w00 == w01 && w00 == w10 && w00 == w11) {
        return None;
    }
    let twist = plus(minus(minus(p11, p10), p01), p00);
    Some(([p00, minus(p10, p00), minus(p01, p00)], norm_upper(twist)?))
}

/// Whether the knots `k` of a curve of degree `p` with `n` points are clamped and every
/// interior knot has multiplicity exactly `p`, the distinct knots increasing: the curve is a
/// chain of rational Bézier segments, each set by its own `p + 1` points.
fn bezier_chain(k: &[f64], p: usize, n: usize) -> bool {
    if p == 0 || k.len() != n + p + 1 {
        return false;
    }
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &x in k {
        if !x.is_finite() {
            return false;
        }
        match runs.last_mut() {
            Some((y, m)) if *y == x => *m += 1,
            Some((y, _)) if *y > x => return false,
            _ => runs.push((x, 1)),
        }
    }
    let Some((&(_, first), rest)) = runs.split_first() else {
        return false;
    };
    let Some((&(_, last), interior)) = rest.split_last() else {
        return false;
    };
    first == p + 1 && last == p + 1 && interior.iter().all(|&(_, m)| m == p)
}

/// Not upstream (#22930): the largest degree of the difference [`composed_trim_gap_bound`]
/// composes (its work grows with the square of it).
const COMPOSED_MAX_DEGREE: usize = 32;

/// Not upstream (#22930): the third case of [`structured_trim_gap_bound_as`], a bound at equal
/// parameters, `max |C(s) - S(W(s))|` over `range` (with `ψ` the identity, also one of the
/// trim image gap), for a curve and a trim that are chains of rational Bézier segments on the
/// same breakpoints ([`bezier_chain`]), on a net that is a chain of rational Bézier patches in
/// both directions, every weight positive. A screw sweep's path on its cylinder piece is one:
/// its trim is the path taken back through the cylinder's net exactly, some 1e-13 from it,
/// where the bisection bounds it under 1e-5 only after some 3000 pieces.
///
/// On the segment `[t_j, t_j+1]` of the curve and the trim (`s` its fraction), let the trim be
/// `W = (U, V) / Ω` and the patch it lies in `[u_a, u_b] x [v_a, v_b]`, of degrees `p` and
/// `q`, with control points `P_ik` and weights `w_ik`. With `A = U - u_a Ω`, `B = u_b Ω - U`,
/// `E = V - v_a Ω` and `F = v_b Ω - V` (polynomials of the trim's degree `r`), the patch's
/// basis at `W` is `b_ik / ((u_b - u_a)^p (v_b - v_a)^q Ω^(p+q))`, `b_ik = C(p, i) A^i B^(p-i)
/// C(q, k) E^k F^(q-k)`. The common factor cancels, so `S(W) = H / H_w`, `H = Σ w_ik P_ik
/// b_ik`, `H_w = Σ w_ik b_ik`: polynomials of degree `r (p + q)`, in Bernstein form from
/// products of Bernstein forms. With the curve's segment `X / X_w` of degree `d`, `C - S(W) =
/// G / D`, `G = X H_w - H X_w` and `D = X_w H_w`, of degree `d + r (p + q)`. The Bernstein
/// basis is non-negative with sum 1, so `|G| <= max_m |G_m|` and `D >= min_m D_m` over their
/// coefficients, and the gap is at most `max_m |G_m| / min_m D_m` where that minimum is
/// positive. Every coefficient is enclosed in outward-rounded interval arithmetic from the
/// stored data, every point first moved by the segment's first point of the curve (which
/// changes no difference, and keeps the sums small far from the origin).
///
/// The trim's control points must lie in one patch, so that its segment does (convex hull).
/// Where they pass the patch's knots by a rounding (the trim's ends sit on the knots), the
/// segment is bounded for the trim scaled into the patch about its middle `m`,
/// `W~ = m + λ (W - m)` with `λ < 1` (the same weights), plus
/// `|S(W) - S(W~)| <= L_u |u - u~| + L_v |v - v~|`, where `|u - u~| <= (1 - λ) max |u_i - m|`:
/// `u~` lies between `u` and `m`, so the move runs through the spans from the trim's hull to
/// the patch's; the net is continuous across its knots, and `L_u`, `L_v` bound its speed over
/// those patches ([`patch_speeds`]). Only a hull within the patch's neighbours is moved, so
/// that the speeds are taken over a few patches; a trim past the net's ends is not bounded.
///
/// `Ok(None)` (the caller bisects) for any other structure, a degree past
/// [`COMPOSED_MAX_DEGREE`], or a bound not enclosed.
///
/// # Errors
///
/// The kernel's cancelled status, polled on each segment.
fn composed_trim_gap_bound(
    k: &Kernel,
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    range: [f64; 2],
) -> Result<Option<f64>> {
    let Some((breaks, knots)) = composed_inputs(cn, wn, net, range) else {
        return Ok(None);
    };
    let [lo, hi] = range;
    let mut worst = 0.0_f64;
    let mut any = false;
    for (j, ends) in breaks.windows(2).enumerate() {
        let &[a, b] = ends else {
            return Ok(None);
        };
        // (only the segments that meet `range`)
        if !(a < hi && lo < b) {
            continue;
        }
        k.check_cancelled(OP)?;
        let Some(bound) = composed_segment_bound(cn, wn, net, &knots, j) else {
            return Ok(None);
        };
        worst = worst.max(bound);
        any = true;
    }
    Ok((any && worst.is_finite()).then_some(worst))
}

/// The structure [`composed_trim_gap_bound`] asks of its data: the curve's breakpoints and
/// the net's distinct knots in `u` and `v`, or `None` where it does not apply.
fn composed_inputs(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    [lo, hi]: [f64; 2],
) -> Option<(Vec<f64>, [Vec<f64>; 2])> {
    let degree =
        wn.p.checked_mul(net.p.checked_add(net.q)?)?
            .checked_add(cn.p)?;
    if degree > COMPOSED_MAX_DEGREE || !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return None;
    }
    if cn.pw.len() != 4 * cn.n
        || wn.pw.len() != 4 * wn.n
        || net.pw.len() != 4 * net.nu.checked_mul(net.nv)?
    {
        return None;
    }
    if !(bezier_chain(&cn.u, cn.p, cn.n)
        && bezier_chain(&wn.u, wn.p, wn.n)
        && bezier_chain(&net.u, net.p, net.nu)
        && bezier_chain(&net.v, net.q, net.nv))
    {
        return None;
    }
    let breaks = distinct_knots(&cn.u);
    if breaks != distinct_knots(&wn.u) {
        return None;
    }
    let positive = |pw: &[f64]| {
        pw.chunks_exact(4)
            .all(|c| c.iter().all(|x| x.is_finite()) && c.get(3).is_some_and(|&w| w > 0.0))
    };
    if !(positive(&cn.pw) && positive(&wn.pw) && positive(&net.pw)) {
        return None;
    }
    Some((breaks, [distinct_knots(&net.u), distinct_knots(&net.v)]))
}

/// The distinct values of the (non-decreasing) knots `k`.
fn distinct_knots(k: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for &x in k {
        if out.last() != Some(&x) {
            out.push(x);
        }
    }
    out
}

/// [`composed_trim_gap_bound`] on the `j`-th segment of the curve and the trim, `knots` the
/// net's distinct knots in `u` and `v`.
fn composed_segment_bound(
    cn: &NurbsCurve,
    wn: &NurbsCurve,
    net: &Nurbs,
    knots: &[Vec<f64>; 2],
    j: usize,
) -> Option<f64> {
    let (d, r, p, q) = (cn.p, wn.p, net.p, net.q);
    let homogeneous = |pw: &[f64], i: usize| -> Option<[f64; 4]> {
        let s = pw.get(4 * i..4 * i + 4)?;
        Some([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?])
    };
    // (the trim's segment, `(U, V, Ω)`, scaled into its patch where it passes it)
    let mut tw: Vec<[Ivl; 3]> = Vec::with_capacity(r + 1);
    for i in 0..=r {
        let [x, y, _, w] = homogeneous(&wn.pw, j.checked_mul(r)?.checked_add(i)?)?;
        tw.push([Ivl::from(x), Ivl::from(y), Ivl::from(w)]);
    }
    let [ku, kv] = knots;
    let (a, u_reach, u_shift) = fit_in_span(&mut tw, 0, ku)?;
    let (b, v_reach, v_shift) = fit_in_span(&mut tw, 1, kv)?;
    let moved = if u_shift > 0.0 || v_shift > 0.0 {
        let (lu, lv) = patch_speeds(net, knots, u_reach, v_reach)?;
        (Ivl::from(lu) * Ivl::from(u_shift) + Ivl::from(lv) * Ivl::from(v_shift)).hi
    } else {
        0.0
    };
    let (ua, ub) = (Ivl::from(*ku.get(a)?), Ivl::from(*ku.get(a + 1)?));
    let (va, vb) = (Ivl::from(*kv.get(b)?), Ivl::from(*kv.get(b + 1)?));
    // (`A`, `B`, `E` and `F` of [`composed_trim_gap_bound`])
    let (mut ea, mut eb, mut ee, mut ef) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for &[u, v, w] in &tw {
        ea.push(u - ua * w);
        eb.push(ub * w - u);
        ee.push(v - va * w);
        ef.push(vb * w - v);
    }
    let (pa, pb) = (bernstein_powers(&ea, p)?, bernstein_powers(&eb, p)?);
    let (pe, pf) = (bernstein_powers(&ee, q)?, bernstein_powers(&ef, q)?);
    // (the reference point: the curve segment's first point)
    let first = homogeneous(&cn.pw, j.checked_mul(d)?)?;
    let [rx, ry, rz, rw] = first;
    let reference = [rx / rw, ry / rw, rz / rw].map(Ivl::from);
    let moved_point = |h: [f64; 4]| -> [Ivl; 4] {
        let w = Ivl::from(h[3]);
        [
            Ivl::from(h[0]) - reference[0] * w,
            Ivl::from(h[1]) - reference[1] * w,
            Ivl::from(h[2]) - reference[2] * w,
            w,
        ]
    };
    let composed = r.checked_mul(p + q)?;
    let mut h = vec![vec![Ivl::from(0.0); composed + 1]; 4];
    for i in 0..=p {
        let bu = bernstein_product(pa.get(i)?, pb.get(p - i)?)?;
        let cu = Ivl::from(binomial(p, i)?);
        for k in 0..=q {
            let bv = bernstein_product(pe.get(k)?, pf.get(q - k)?)?;
            let c = cu * Ivl::from(binomial(q, k)?);
            let basis = bernstein_product(&bu, &bv)?;
            let at = (a * p + i).checked_mul(net.nv)?.checked_add(b * q + k)?;
            let point = moved_point(homogeneous(&net.pw, at)?);
            for (hd, &coord) in h.iter_mut().zip(point.iter()) {
                let coord = coord * c;
                for (slot, &x) in hd.iter_mut().zip(basis.iter()) {
                    *slot = *slot + coord * x;
                }
            }
        }
    }
    let mut x: Vec<Vec<Ivl>> = (0..4).map(|_| Vec::with_capacity(d + 1)).collect();
    for i in 0..=d {
        let point = moved_point(homogeneous(&cn.pw, j.checked_mul(d)?.checked_add(i)?)?);
        for (xd, &coord) in x.iter_mut().zip(point.iter()) {
            xd.push(coord);
        }
    }
    let (hw, xw) = (h.get(3)?, x.get(3)?);
    let mut g: Vec<Vec<Ivl>> = Vec::with_capacity(3);
    for dim in 0..3 {
        let left = bernstein_product(x.get(dim)?, hw)?;
        let right = bernstein_product(h.get(dim)?, xw)?;
        g.push(
            left.iter()
                .zip(right.iter())
                .map(|(&l, &r)| l - r)
                .collect(),
        );
    }
    let den = bernstein_product(xw, hw)?;
    let least = den.iter().map(|c| c.lo).fold(f64::INFINITY, f64::min);
    if !(least > 0.0 && least.is_finite()) {
        return None;
    }
    let (gx, gy, gz) = (g.first()?, g.get(1)?, g.get(2)?);
    let mut most = 0.0_f64;
    for ((&x, &y), &z) in gx.iter().zip(gy.iter()).zip(gz.iter()) {
        most = most.max(norm_upper(V3 { x, y, z })?);
    }
    let bound = (Ivl::from(most) / Ivl::from(least) + Ivl::from(moved)).hi;
    bound.is_finite().then_some(bound)
}

/// The share of a span [`fit_in_span`] keeps clear at each end: a few thousand roundings, so
/// the scaled points' enclosures lie inside, and a move of at most this share of the span
/// added to the bound.
const FIT_MARGIN: f64 = 1.0e-12;

/// For [`composed_segment_bound`]: the span of the distinct knots `k` the trim segment's
/// coordinate `c` (0 for `u`, 1 for `v`) lies in, the first and last span its control points
/// reach, and how far at most they were moved into it (0 where they lie in it). Points past it
/// are scaled into it about its middle `m`, `x~ = m + λ (x - m)` in each homogeneous point
/// (`λ` rounded down, the result checked in the span), which moves each point of the segment
/// by `(1 - λ) |x - m|`, at most `(1 - λ) max_i |x_i - m|`, through the spans between the
/// points and the span. `None` past the net's ends, where the points reach past the span's
/// neighbours (the move would run through spans whose speed is not taken), or where the
/// scaled points are not shown in the span.
fn fit_in_span(tw: &mut [[Ivl; 3]], c: usize, k: &[f64]) -> Option<(usize, [usize; 2], f64)> {
    let (&first, &last) = (k.first()?, k.last()?);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for t in tw.iter() {
        let x = coordinate(*t.get(c)?, *t.get(2)?);
        lo = lo.min(x.lo);
        hi = hi.max(x.hi);
    }
    if !(first <= lo && hi <= last) {
        return None;
    }
    let spans = k.len().checked_sub(1)?;
    let middle = 0.5 * (lo + hi);
    // (the last span that starts at or below the middle: the knots are distinct and ascending)
    let a = k
        .get(..spans)?
        .partition_point(|&x| x <= middle)
        .checked_sub(1)?;
    let (ka, kb) = (*k.get(a)?, *k.get(a + 1)?);
    let reach = [
        if lo < ka { a.checked_sub(1)? } else { a },
        if hi > kb { a + 1 } else { a },
    ];
    // (every point moves between where it is and the span, so the hull must lie in the spans
    // `reach` names, whose speeds bound the move)
    if reach[1] >= spans || lo < *k.get(reach[0])? || hi > *k.get(reach[1] + 1)? {
        return None;
    }
    if ka <= lo && hi <= kb {
        return Some((a, reach, 0.0));
    }
    let m = Ivl::from(0.5 * ka + 0.5 * kb);
    let mut far = 0.0_f64;
    for t in tw.iter() {
        far = far.max((coordinate(*t.get(c)?, *t.get(2)?) - m).mag());
    }
    // (the scaled points are aimed a little inside the span, [`FIT_MARGIN`] of its size, so
    // that their enclosures, a few roundings wide, are shown in it)
    let half = (m - Ivl::from(ka)).lo.min((Ivl::from(kb) - m).lo);
    let aim = (Ivl::from(half) - Ivl::from(FIT_MARGIN) * Ivl::from(half)).lo;
    let lambda = (Ivl::from(aim) / Ivl::from(far)).lo;
    if !(lambda > 0.0 && lambda < 1.0) {
        return None;
    }
    let lambda = Ivl::from(lambda);
    for t in tw.iter_mut() {
        let w = *t.get(2)?;
        let x = t.get_mut(c)?;
        *x = m * w + lambda * (*x - m * w);
        let at = coordinate(*x, w);
        if !(ka <= at.lo && at.hi <= kb) {
            return None;
        }
    }
    let shift = ((Ivl::from(1.0) - lambda) * Ivl::from(far)).hi;
    shift.is_finite().then_some((a, reach, shift))
}

/// The coordinate `x / w` of a homogeneous point, enclosed: `x` itself for a weight of exactly 1
/// (an interval quotient would widen it by a rounding, and a point on the net's end would seem
/// past it).
fn coordinate(x: Ivl, w: Ivl) -> Ivl {
    if w.lo == 1.0 && w.hi == 1.0 {
        x
    } else {
        x / w
    }
}

/// Upper bounds `(L_u, L_v)` on the speed of the net `net` along `u` and along `v` over the
/// patches of the spans `u_reach` by `v_reach` (first and last, of the distinct `knots`). Each
/// iso curve of a rational Bézier patch has points that are its own averaged (weights between
/// the patch's least and largest), so its speed is at most `2 p w_max diam / (h w_min)`, as
/// [`row_lipschitz`] bounds a row's; the net is continuous across its knots, so the largest
/// over the patches bounds it across them.
fn patch_speeds(
    net: &Nurbs,
    [ku, kv]: &[Vec<f64>; 2],
    u_reach: [usize; 2],
    v_reach: [usize; 2],
) -> Option<(f64, f64)> {
    let (p, q) = (net.p, net.q);
    let (mut lu, mut lv) = (0.0_f64, 0.0_f64);
    for a in u_reach[0]..=u_reach[1] {
        for b in v_reach[0]..=v_reach[1] {
            let mut points: Vec<V3<Ivl>> = Vec::new();
            let (mut w_max, mut w_min) = (0.0_f64, f64::INFINITY);
            for i in 0..=p {
                for k in 0..=q {
                    let at = 4 * (a * p + i).checked_mul(net.nv)?.checked_add(b * q + k)?;
                    let s = net.pw.get(at..at + 4)?;
                    let w = *s.get(3)?;
                    points.push(point_of([*s.first()?, *s.get(1)?, *s.get(2)?, w])?);
                    w_max = w_max.max(w);
                    w_min = w_min.min(w);
                }
            }
            if w_min.is_nan() || w_min <= 0.0 {
                return None;
            }
            let mut diam = 0.0_f64;
            for x in &points {
                for y in &points {
                    diam = diam.max(norm_upper(minus(*x, *y))?);
                }
            }
            let hu = Ivl::from(*ku.get(a + 1)?) - Ivl::from(*ku.get(a)?);
            let hv = Ivl::from(*kv.get(b + 1)?) - Ivl::from(*kv.get(b)?);
            if !(hu.is_positive() && hv.is_positive()) {
                return None;
            }
            let ratio = Ivl::from(w_max) * Ivl::from(diam) / Ivl::from(w_min);
            lu = lu.max((Ivl::from(2.0 * p as f64) * ratio / hu).hi);
            lv = lv.max((Ivl::from(2.0 * q as f64) * ratio / hv).hi);
        }
    }
    (lu.is_finite() && lv.is_finite()).then_some((lu, lv))
}

/// Pascal's triangle up to [`COMPOSED_MAX_DEGREE`] (0 past each row's end), made once. Each
/// `C(n, k)` is the product `Π_{i=1..k} (n - k + i) / i` in integers: after `i` factors it is
/// `C(n - k + i, i)`, so each division is exact, and the largest value before one,
/// `i C(n - k + i, i)`, is at most `32 C(32, 16)`, far below `2^53` (each is exact as an `f64`).
static BINOMIALS: std::sync::LazyLock<[[f64; COMPOSED_MAX_DEGREE + 1]; COMPOSED_MAX_DEGREE + 1]> =
    std::sync::LazyLock::new(|| {
        core::array::from_fn(|n| {
            core::array::from_fn(|k| {
                if k > n {
                    return 0.0;
                }
                (1..=k).fold(1_u64, |c, i| c * (n - k + i) as u64 / i as u64) as f64
            })
        })
    });

/// `C(n, k)`, exactly; `None` past [`COMPOSED_MAX_DEGREE`] or for `k > n`.
fn binomial(n: usize, k: usize) -> Option<f64> {
    if k > n {
        return None;
    }
    BINOMIALS.get(n)?.get(k).copied()
}

/// The Bernstein coefficients over `[0, 1]` of the product of two polynomials given by
/// theirs, enclosed: `c_m = Σ_{i+k=m} C(a, i) C(b, k) / C(a + b, m) x_i y_k`.
fn bernstein_product(x: &[Ivl], y: &[Ivl]) -> Option<Vec<Ivl>> {
    let (a, b) = (x.len().checked_sub(1)?, y.len().checked_sub(1)?);
    let mut out = vec![Ivl::from(0.0); a + b + 1];
    for (i, &xi) in x.iter().enumerate() {
        let ci = Ivl::from(binomial(a, i)?);
        for (k, &yk) in y.iter().enumerate() {
            let c = ci * Ivl::from(binomial(b, k)?) / Ivl::from(binomial(a + b, i + k)?);
            let slot = out.get_mut(i + k)?;
            *slot = *slot + c * xi * yk;
        }
    }
    Some(out)
}

/// The Bernstein coefficients of `x^0, ..., x^n` for the polynomial with coefficients `x`
/// (`x^0` the constant 1, of degree 0).
fn bernstein_powers(x: &[Ivl], n: usize) -> Option<Vec<Vec<Ivl>>> {
    let mut out = vec![vec![Ivl::from(1.0)]];
    for _ in 0..n {
        let next = bernstein_product(out.last()?, x)?;
        out.push(next);
    }
    Some(out)
}

/// The point `(x, y, z) / w` of homogeneous data, enclosed; `None` for a weight that is not
/// positive or data that are not finite.
fn point_of([x, y, z, w]: [f64; 4]) -> Option<V3<Ivl>> {
    if !(w.is_finite() && w > 0.0 && x.is_finite() && y.is_finite() && z.is_finite()) {
        return None;
    }
    let w = Ivl::from(w);
    Some(V3 {
        x: Ivl::from(x) / w,
        y: Ivl::from(y) / w,
        z: Ivl::from(z) / w,
    })
}

fn dot(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// The bisection of [`curve_face_gap_bound_within`], counting in `pieces` the pieces it
/// bounds.
///
/// # Errors
///
/// Those of [`curve_face_gap_bound_within`].
fn bisected_face_gap_bound(
    k: &Kernel,
    brep: &crate::topology::Brep,
    face: crate::topology::FaceId,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    pieces: &Cell<usize>,
) -> Result<Option<f64>> {
    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let rec = brep.face(face)?;
    let Some(surface) = rec.surface.as_ref() else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    bisected_surface_gap_bound(
        k,
        &surface,
        &rec.uv_domain,
        curve,
        range,
        floor,
        [enough, over],
        pieces,
    )
}

/// The bisection of [`bisected_face_gap_bound`] against `surface` (a copy for evaluation)
/// with its nearest points dropped in `domain`.
///
/// # Errors
///
/// Those of [`curve_face_gap_bound_within`] but for the face lookup.
#[allow(clippy::too_many_arguments)]
fn bisected_surface_gap_bound(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    curve: &BSplineCurve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    pieces: &Cell<usize>,
) -> Result<Option<f64>> {
    use crate::error::ignored;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;

    let [lo, hi] = range;
    if !(lo.is_finite() && hi.is_finite() && lo < hi && floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let Ok(cn) = NurbsCurve::of(curve) else {
        return Ok(None);
    };
    let (Some(&d0), Some(&d1)) = (cn.u.get(cn.p), cn.u.get(cn.n)) else {
        return Ok(None);
    };
    if !(d0 <= lo && hi <= d1) {
        return Ok(None);
    }
    let evaluated = Curve::from(curve.clone_exact());
    // (the nearest point of the curve's point at `t`, from `guess`; `None` where it is not found)
    let drop_at = |t: f64, guess: Option<Point2>| -> Result<Option<Point2>> {
        let Some(point) =
            ignored(evaluated.evaluate(k, t, 0, true, false))?.and_then(|d| d.into_iter().next())
        else {
            return Ok(None);
        };
        let opts = DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        };
        let drop = ignored(surface.drop_point::<NoObject>(k, point, domain, &opts))?;
        Ok(drop.filter(|d| d.success).map(|d| d.result_uv))
    };
    // The first pieces' ends: equally spaced, and every kink of the curve, so no first piece
    // has a corner inside it ([`certified_gap`]); their nearest points are followed from the
    // middle out to each end, each drop started from its neighbour's ([`correspondence`]).
    let mut cuts: Vec<f64> = (0..=FIRST_PIECES)
        .map(|j| {
            if j == FIRST_PIECES {
                hi
            } else {
                lo + (hi - lo) * (j as f64) / (FIRST_PIECES as f64)
            }
        })
        .collect();
    cuts.extend(kinks(&cn, lo, hi));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let n = cuts.len();
    let mid = n / 2;
    let mut feet: Vec<Option<Point2>> = vec![None; n];
    let order = (mid..n)
        .map(|i| (i, (i > mid).then(|| i - 1)))
        .chain((0..mid).rev().map(|i| (i, Some(i + 1))));
    for (i, from) in order {
        let Some(&t) = cuts.get(i) else {
            return Ok(None);
        };
        let guess = from.and_then(|j| feet.get(j).copied().flatten());
        let Some(uv) = drop_at(t, guess)? else {
            return Ok(None);
        };
        if let Some(slot) = feet.get_mut(i) {
            *slot = Some(uv);
        }
    }
    let feet: Vec<Point2> = feet.into_iter().flatten().collect();
    let Some(net) = NetSurface::of(k, surface, &feet)? else {
        return Ok(None);
    };
    // (the bound of the curve data `cn` against the net `net`, with the pieces closing at
    // `enough`; infinite past `over`)
    let run = |cn: &NurbsCurve, net: &NetSurface, enough: f64| -> Result<Option<f64>> {
        // (each piece with its ends' nearest points, its depth and its parent's slack, infinite
        // for a first piece)
        let mut stack: Vec<(f64, f64, [Point2; 2], u32, f64)> = Vec::new();
        for (ts, ws) in cuts.windows(2).zip(feet.windows(2)) {
            if let ([a, b], [wa, wb]) = (ts, ws) {
                stack.push((*a, *b, [*wa, *wb], 0, f64::INFINITY));
            }
        }
        stack.reverse();
        let mut worst = 0.0_f64;
        let mut made = stack.len();
        let mut seen = f64::NEG_INFINITY;
        while let Some((a, b, ends, depth, parent)) = stack.pop() {
            k.check_cancelled(OP)?;
            let mid = 0.5 * (a + b);
            let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
            let mut slack = f64::INFINITY;
            pieces.set(pieces.get() + 1);
            match surface_piece(cn, [a, b], net, ends, a, b) {
                Ok(Some(p)) => {
                    let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack)).hi;
                    seen = seen.max(p.ends);
                    if seen > over {
                        return Ok(Some(f64::INFINITY));
                    }
                    // (while the bound may still be at most `enough`, a piece above it is cut on)
                    let (relative, at_least) = if seen <= enough {
                        (0.0, 0.0)
                    } else {
                        (CLOSE_RELATIVE, floor)
                    };
                    let closed =
                        bound <= enough || bound <= seen || p.closes(parent, relative, at_least);
                    if last || closed {
                        if bound > over {
                            return Ok(Some(f64::INFINITY));
                        }
                        worst = worst.max(bound);
                        continue;
                    }
                    slack = p.slack;
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                // a box past the net, on a knot line of too low a continuity: a smaller piece may
                // clear it; at the bottom the gap is not enclosed
                Ok(None) | Err(_) => {
                    if last {
                        return Ok(None);
                    }
                }
            }
            let [wa, wb] = ends;
            let straight = Point2::new(0.5 * (wa.x + wb.x), 0.5 * (wa.y + wb.y));
            let wm = drop_at(mid, Some(straight))?.unwrap_or(straight);
            made += 2;
            stack.push((mid, b, [wm, wb], depth + 1, slack));
            stack.push((a, mid, [wa, wm], depth + 1, slack));
        }
        Ok(worst.is_finite().then_some(worst))
    };
    // (#20373 review r4: the pieces bound the net's image; the net's own distance to the
    // surface ([`bisection_residual`]) is taken off `enough` and added to the bound, rounded
    // up, so a bound past the net's is not taken for the surface's)
    let Some(to_surface) = bisection_residual(k, surface, &net, None)? else {
        return Ok(None);
    };
    let net_enough = if to_surface > 0.0 {
        (Ivl::from(enough) - Ivl::from(to_surface)).lo
    } else {
        enough
    };
    let first = run(&cn, &net, net_enough)?;
    let Some(bound) = recentred_retry(first, [net_enough, over], &cn, &net, run)? else {
        return Ok(None);
    };
    if to_surface == 0.0 || bound == f64::INFINITY {
        return Ok(Some(bound));
    }
    let bound = (Ivl::from(bound) + Ivl::from(to_surface)).hi;
    Ok(Some(if bound > over || !bound.is_finite() {
        f64::INFINITY
    } else {
        bound
    }))
}

/// The correspondence `W` of `curve` over `[lo, hi]` on `surface` (module docs): a cubic
/// B-spline of parameters `(u, v, 0)` on the curve's own parameter, through the nearest points
/// in `domain` of [`CORRESPONDENCE_POINTS`] equally spaced points of the curve, with the
/// surface's net. `None` where a nearest point is not found or the surface has no net whose
/// parameters are the surface's ([`NetSurface::of`]).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses; those of the curve's evaluation.
pub(crate) fn correspondence(
    k: &Kernel,
    surface: &Surface,
    domain: &crate::geom::extent2d::Extent2d,
    curve: &BSplineCurve,
    [lo, hi]: [f64; 2],
) -> Result<Option<(NetSurface, BSplineCurve)>> {
    use crate::curve::curve::Curve;
    use crate::error::ignored;
    use crate::nurbs::crv_fit::p1::ParamMethod;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;

    let evaluated = Curve::from(curve.clone_exact());
    let params: Vec<f64> = (0..CORRESPONDENCE_POINTS)
        .map(|i| {
            if i + 1 == CORRESPONDENCE_POINTS {
                hi
            } else {
                lo + (hi - lo) * (i as f64) / ((CORRESPONDENCE_POINTS - 1) as f64)
            }
        })
        .collect();
    // The nearest points are followed from the middle of the curve out to each end, each drop
    // started from its neighbour's: an end on a side of the net collapsed to one point (the apex
    // of a loft between curves that meet there) has every parameter of that side for its nearest
    // point, and a drop with nothing to start from takes any of them, so `W` would jump there
    // and run past the net. Continued from its neighbour it takes the neighbour's; the bound holds
    // for any `W`, only whether the bisection closes depends on this.
    let n = params.len();
    let mid = n / 2;
    let order: Vec<(usize, Option<usize>)> = (mid..n)
        .map(|i| (i, (i > mid).then(|| i - 1)))
        .chain((0..mid).rev().map(|i| (i, Some(i + 1))))
        .collect();
    let mut found: Vec<Option<Point2>> = vec![None; n];
    for (i, from) in order {
        let Some(&t) = params.get(i) else {
            return Ok(None);
        };
        let guess = from.and_then(|j| found.get(j).copied().flatten());
        let Some(point) =
            ignored(evaluated.evaluate(k, t, 0, true, false))?.and_then(|d| d.into_iter().next())
        else {
            return Ok(None);
        };
        let opts = DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        };
        let Some(drop) = ignored(surface.drop_point::<NoObject>(k, point, domain, &opts))? else {
            return Ok(None);
        };
        if !drop.success {
            return Ok(None);
        }
        if let Some(slot) = found.get_mut(i) {
            *slot = Some(drop.result_uv);
        }
    }
    let uvs: Vec<Point2> = found.into_iter().flatten().collect();
    let Some(net) = NetSurface::of(k, surface, &uvs)? else {
        return Ok(None);
    };
    let flat: Vec<crate::math::Vec3> = uvs
        .iter()
        .map(|uv| crate::math::Vec3::new(uv.x, uv.y, 0.0))
        .collect();
    let Ok(correspondence) = BSplineCurve::interpolate_points(
        &flat,
        Some(&params),
        3,
        None,
        None,
        false,
        ParamMethod::ChordLength,
    ) else {
        return Ok(None);
    };
    Ok(Some((net, correspondence)))
}

// ---- a curve piece against a point, a surface or a curve ------------------------------------

/// What [`curve_piece_gap_bound`] bounds a curve piece's distance to: a point, a surface
/// with the parameters of its points that correspond to the piece's two ends, or a curve
/// with the parameters of those points.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PieceTarget<'a> {
    /// A fixed point.
    Point(crate::math::Point3),
    /// A surface and its parameters at the points matching the piece's first and last end.
    Surface(&'a Surface, [Point2; 2]),
    /// A curve and its parameters at the points matching the piece's first and last end.
    Curve(&'a Curve, [f64; 2]),
}

/// The NURBS data a curve evaluates over `[lo, hi]`, with the error bound of its points
/// against the curve's own: the B-spline a B-spline, ellipse, circle, hyperbola or parabola
/// evaluates (no error: it is that B-spline), and for a line the segment between its two
/// computed end points, each within `e` per coordinate of the line's (so every point of the
/// line over `[lo, hi]`, a convex combination of the exact ends, is within `sqrt(3) e <= 2 e`
/// of the segment's point at that parameter). `None` for other kinds, a NURB that is not
/// well formed, and a range outside the curve's domain.
fn piece_nurbs(curve: &Curve, [lo, hi]: [f64; 2]) -> Option<(NurbsCurve, f64)> {
    use crate::curve::curve::p2::CurveKind;
    let (cn, err) = match curve.kind() {
        CurveKind::Line(l) => {
            let [(p0, e0), (p1, e1)] =
                l.segment_over(&crate::geom::extent1d::Extent1d::from_min_max(lo, hi))?;
            let err = 2.0 * e0.max(e1);
            (
                NurbsCurve {
                    p: 1,
                    n: 2,
                    u: vec![lo, lo, hi, hi],
                    pw: vec![p0.x, p0.y, p0.z, 1.0, p1.x, p1.y, p1.z, 1.0],
                },
                err,
            )
        }
        CurveKind::BSpline(_)
        | CurveKind::Ellipse(_)
        | CurveKind::Hyperbola(_)
        | CurveKind::Parabola(_) => (NurbsCurve::of(curve.b_spline_base()?).ok()?, 0.0),
        _ => return None,
    };
    let (d0, d1) = (*cn.u.get(cn.p)?, *cn.u.get(cn.n)?);
    (lo.is_finite() && hi.is_finite() && d0 <= lo && lo <= hi && hi <= d1 && err.is_finite())
        .then_some((cn, err))
}

/// The distinct knots of `cn` strictly inside `(lo, hi)`, in order.
fn interior_knots(cn: &NurbsCurve, lo: f64, hi: f64) -> Vec<f64> {
    let mut out: Vec<f64> = cn.u.iter().copied().filter(|&u| lo < u && u < hi).collect();
    out.dedup();
    out
}

/// The target of one piece, prepared: its NURBS data, the parameter map from the piece's
/// curve parameter and the error bound of its points.
enum Prepared {
    Point(V3<Ivl>),
    Surface(NetSurface, [Point2; 2]),
    Curve(NurbsCurve, [f64; 2]),
}

/// `w(t)` of the straight map from `[lo, hi]` onto `[w0, w1]`, enclosed for every `t` in `t`.
fn along(t: Ivl, [lo, hi]: [f64; 2], [w0, w1]: [f64; 2]) -> Ivl {
    let slope = (Ivl::from(w1) - Ivl::from(w0)) / (Ivl::from(hi) - Ivl::from(lo));
    Ivl::from(w0) + (t - Ivl::from(lo)) * slope
}

/// [`along`]'s slope, enclosed.
fn slope([lo, hi]: [f64; 2], [w0, w1]: [f64; 2]) -> Ivl {
    (Ivl::from(w1) - Ivl::from(w0)) / (Ivl::from(hi) - Ivl::from(lo))
}

/// `x` cut to `[a, b]` (both ends of `x` past it become its ends): the map's exact values lie
/// between its exact ends, which lie in `[a, b]`, so cutting the enclosure keeps them.
fn clamp_ivl(x: Ivl, a: f64, b: f64) -> Ivl {
    let (lo, hi) = (a.min(b), a.max(b));
    Ivl::new(x.lo.max(lo).min(hi), x.hi.min(hi).max(lo))
}

/// The bound of one piece `[a, b]` of the curve against the surface `s`, matched to it by the
/// straight map from `range` onto `[w0, w1]` ([`target_piece`]'s surface target): the larger
/// end value plus `h^2 / 8 sup |g''|` with the surface's slack past its net, or the Lipschitz
/// form `h sup |g'|` where the box crosses a knot line the surface is only C^0 across. `None`
/// when some enclosure is not finite.
fn surface_piece(
    cn: &NurbsCurve,
    range: [f64; 2],
    s: &NetSurface,
    [w0, w1]: [Point2; 2],
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(c0) = curve_derivs_over(cn, at, 0, None)?.first().copied() else {
            return Ok(None);
        };
        let w = V3 {
            x: clamp_ivl(along(at, range, [w0.x, w1.x]), w0.x, w1.x),
            y: clamp_ivl(along(at, range, [w0.y, w1.y]), w0.y, w1.y),
            z: Ivl::from(0.0),
        };
        let q = s.partials(&[w], 0, 0)?.0.at(0, 0);
        let Some(g) = norm_upper(minus(c0, q)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let h = Ivl::from(b) - Ivl::from(a);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let (Some(c1), Some(c2)) = (cd.get(1).copied(), cd.get(2).copied()) else {
        return Ok(None);
    };
    let (u1, v1) = (slope(range, [w0.x, w1.x]), slope(range, [w0.y, w1.y]));
    let w = V3 {
        x: clamp_ivl(along(span, range, [w0.x, w1.x]), w0.x, w1.x),
        y: clamp_ivl(along(span, range, [w0.y, w1.y]), w0.y, w1.y),
        z: Ivl::from(0.0),
    };
    // (S o w)'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 where the surface is C^1
    // across every knot line the box crosses; else g is Lipschitz, with the
    // one-sided first partials hulled
    let term = match s.partials(&[w], 2, 1) {
        Ok((p, slack)) => {
            let mut swpp = scaled(p.at(2, 0), u1 * u1);
            swpp = plus(swpp, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
            swpp = plus(swpp, scaled(p.at(0, 2), v1 * v1));
            let Some(m) = norm_upper(minus(c2, swpp)) else {
                return Ok(None);
            };
            (h * h * Ivl::from(m) * Ivl::from(0.125), slack)
        }
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => {
            let (p, slack) = s.partials(&[w], 1, 0)?;
            let sw = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
            let Some(m) = norm_upper(minus(c1, sw)) else {
                return Ok(None);
            };
            (h * Ivl::from(m), slack)
        }
    };
    Ok(PieceBound::with(ends, term.0, term.1))
}

/// The bound of one piece `[a, b]` of the curve against the target (a surface's with its slack
/// past its net, as [`second_term`]), `None` when some enclosure is not finite (module docs; the same chord bound as [`piece`] with the straight map in place of the
/// correspondence `W`). A target curve whose
/// knot lies inside the piece's image, or a surface only C^0 across a knot line the box
/// crosses, is bounded by the Lipschitz form, `h sup |g'|`.
fn target_piece(
    cn: &NurbsCurve,
    range: [f64; 2],
    target: &Prepared,
    a: f64,
    b: f64,
) -> Result<Option<PieceBound>> {
    if let Prepared::Surface(s, w) = target {
        return surface_piece(cn, range, s, *w, a, b);
    }
    let mut ends = 0.0_f64;
    for t in [a, b] {
        let at = Ivl::from(t);
        let Some(c0) = curve_derivs_over(cn, at, 0, None)?.first().copied() else {
            return Ok(None);
        };
        let q = match target {
            Prepared::Point(p) => *p,
            // (bounded by `surface_piece` above)
            Prepared::Surface(..) => return Ok(None),
            Prepared::Curve(dn, [s0, s1]) => {
                let s = clamp_ivl(along(at, range, [*s0, *s1]), *s0, *s1);
                let Some(d0) = curve_derivs_over(dn, s, 0, None)?.first().copied() else {
                    return Ok(None);
                };
                d0
            }
        };
        let Some(g) = norm_upper(minus(c0, q)) else {
            return Ok(None);
        };
        ends = ends.max(g);
    }
    let span = Ivl::new(a, b);
    let h = Ivl::from(b) - Ivl::from(a);
    let cd = curve_derivs_over(cn, span, 2, None)?;
    let (Some(c1), Some(c2)) = (cd.get(1).copied(), cd.get(2).copied()) else {
        return Ok(None);
    };
    // (g', g'') of g = c - target(w), the map straight (w'' = 0); `None` for the second
    // where it is not defined over the piece
    let (g1, g2) = match target {
        Prepared::Point(_) => (c1, Some(c2)),
        // (bounded by `surface_piece` above)
        Prepared::Surface(..) => return Ok(None),
        Prepared::Curve(dn, [s0, s1]) => {
            let w1 = slope(range, [*s0, *s1]);
            let s = clamp_ivl(along(span, range, [*s0, *s1]), *s0, *s1);
            let dd = curve_derivs_over(dn, s, 2, None)?;
            let (Some(d1), Some(d2)) = (dd.get(1).copied(), dd.get(2).copied()) else {
                return Ok(None);
            };
            // a knot of the target inside the image: its derivatives are hulled over two
            // pieces, the second-order bound needs one
            let kinked = !interior_knots(dn, s.lo, s.hi).is_empty();
            let g1 = minus(c1, scaled(d1, w1));
            let g2 = (!kinked).then(|| minus(c2, scaled(d2, w1 * w1)));
            (g1, g2)
        }
    };
    let term = if let Some(gpp) = g2 {
        let Some(m) = norm_upper(gpp) else {
            return Ok(None);
        };
        (h * h * Ivl::from(m) * Ivl::from(0.125)).hi
    } else {
        let Some(m) = norm_upper(g1) else {
            return Ok(None);
        };
        (h * Ivl::from(m)).hi
    };
    Ok(term.is_finite().then(|| PieceBound::exact(ends, term)))
}

/// Not upstream (WR-410): a proved upper bound on the distance from every point of `curve`
/// between its parameters `t[0]` and `t[1]` (either order) to `target`: no point of that
/// piece is farther from the target. A surface or curve target is matched to the piece by
/// the straight map from `t` onto its own parameters at the two ends (a witness only:
/// `dist(c(t), S) <= |c(t) - S(w(t))|` for any `w`, so the map decides how tight the bound
/// is, never whether it holds), and the bound is that of [`certified_gap`]'s module docs: the
/// larger end value plus `h^2 / 8 sup |g''|` per piece, the pieces cut at the curve's knots
/// and bisected, every value enclosed in outward-rounded intervals. Bisection stops once the
/// bound passes `limit` (it is then returned) or closes below `floor`.
///
/// `None` when no bound is made: a curve kind without NURBS data ([`piece_nurbs`]), a range
/// or target range outside its curve's domain, a surface without a net matching it at the
/// two parameters (`NetSurface::of`), or a piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn curve_piece_gap_bound(
    k: &Kernel,
    curve: &Curve,
    t: [f64; 2],
    target: PieceTarget<'_>,
    floor: f64,
    limit: f64,
) -> Result<Option<f64>> {
    let [t0, t1] = t;
    if !(t0.is_finite() && t1.is_finite() && t0 != t1 && floor >= 0.0) {
        return Ok(None);
    }
    // the piece runs from lo to hi; the target's ends follow it
    let (lo, hi, flip) = if t0 < t1 {
        (t0, t1, false)
    } else {
        (t1, t0, true)
    };
    let Some((cn, err)) = piece_nurbs(curve, [lo, hi]) else {
        return Ok(None);
    };
    let (prepared, target_err) = match target {
        PieceTarget::Point(p) => {
            let p = V3 {
                x: Ivl::from(p.x),
                y: Ivl::from(p.y),
                z: Ivl::from(p.z),
            };
            (Prepared::Point(p), 0.0)
        }
        PieceTarget::Surface(s, [w0, w1]) => {
            let ends = if flip { [w1, w0] } else { [w0, w1] };
            let Some(net) = NetSurface::of(k, s, &ends)? else {
                return Ok(None);
            };
            (Prepared::Surface(net, ends), 0.0)
        }
        PieceTarget::Curve(c, [s0, s1]) => {
            let ends = if flip { [s1, s0] } else { [s0, s1] };
            let Some((dn, derr)) = piece_nurbs(c, [ends[0].min(ends[1]), ends[0].max(ends[1])])
            else {
                return Ok(None);
            };
            (Prepared::Curve(dn, ends), derr)
        }
    };
    let range = [lo, hi];
    let mut cuts = vec![lo];
    cuts.extend(interior_knots(&cn, lo, hi));
    cuts.push(hi);
    // (each piece with its parent's slack, infinite for a first piece)
    let mut stack: Vec<(f64, f64, u32, f64)> = cuts
        .windows(2)
        .rev()
        .filter_map(|w| match *w {
            [a, b] => Some((a, b, 0, f64::INFINITY)),
            _ => None,
        })
        .collect();
    let extra = err + target_err;
    let mut worst = 0.0_f64;
    let mut made = stack.len();
    while let Some((a, b, depth, parent)) = stack.pop() {
        let mid = 0.5 * (a + b);
        let last = depth >= MAX_DEPTH || !(a < mid && mid < b) || made + 2 > MAX_PIECES;
        let mut slack = f64::INFINITY;
        match target_piece(&cn, range, &prepared, a, b) {
            Ok(Some(p)) => {
                let bound = (Ivl::from(p.ends) + Ivl::from(p.with_slack) + Ivl::from(extra)).hi;
                if !bound.is_finite() {
                    return Ok(None);
                }
                // (past the limit at an end: no cut can bring it back)
                if (Ivl::from(p.ends) + Ivl::from(extra)).hi > limit {
                    return Ok(Some(bound));
                }
                if last || p.closes(parent, CLOSE_RELATIVE, floor) {
                    worst = worst.max(bound);
                    continue;
                }
                slack = p.slack;
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(None) | Err(_) => {
                if last {
                    return Ok(None);
                }
            }
        }
        made += 2;
        stack.push((mid, b, depth + 1, slack));
        stack.push((a, mid, depth + 1, slack));
    }
    Ok(worst.is_finite().then_some(worst))
}

/// Points of a UV trim at which its surface's net is checked to give the surface's point
/// ([`NetSurface::of`]): a sanity probe that the net is parameterised as the surface, never
/// part of the bound.
const TRIM_PROBES: usize = 9;

/// The exact NURBS data of `c` as the kernel evaluates it, with the rounding by which its
/// points can differ from that data's: a B-spline or an ellipse is its NURB (the kernel
/// evaluates it there, so the slack is zero); a line is evaluated analytically, `P + s V`,
/// and its NURB is the segment through its two control points, the slack how far the two can
/// be apart ([`line_nurb_slack`]).
/// `None` for any other kind, or data that is not a valid NURBS net.
fn exact_curve_data(c: &Curve, is_2d: bool) -> Result<Option<(NurbsCurve, f64)>> {
    use crate::curve::curve::CurveKind;
    use crate::surface::surface::exact_pieces::exact_nurbs_curve_data;
    let line_base;
    let (bs, line) = match c.kind() {
        CurveKind::BSpline(bs) => (bs, None),
        CurveKind::Ellipse(e) => (e.base(), None),
        CurveKind::Line(l) => {
            line_base = l.base_with_nurb()?;
            (&*line_base, Some(l))
        }
        _ => return Ok(None),
    };
    let Ok((cn, _, _, _, _)) = exact_nurbs_curve_data(bs, is_2d) else {
        return Ok(None);
    };
    let Some(l) = line else {
        return Ok(Some((cn, 0.0)));
    };
    Ok(line_nurb_slack(l, &cn, is_2d).map(|slack| (cn, slack)))
}

/// A bound `L` with `|S(p) - S(q)| <= L d` for the plane `s` and UV points `p`, `q` within `d`
/// of each other in each coordinate, both within `d` of the segment between the UV points
/// `ends` (a UV line's NURB): `None` unless `s` is a plane whose NURB is a polynomial bilinear
/// patch. Over its domain `[u0, u1] x [v0, v1]` with corners `P_ij`, `S_u` at the `v` fraction
/// `β` is `((1 - β)(P_10 - P_00) + β (P_11 - P_01)) / (u1 - u0)`, whose norm is convex in `β`,
/// so over any `β` range it is at most the larger of its values at the range's ends; the range
/// taken is the segment's box widened by `d`, joined with `[0, 1]`. Likewise `S_v`, and
/// `|S(p) - S(q)| <= (max |S_u| + max |S_v|) d`. A difference of two floats rounds by at most
/// `ε` of itself, a quotient, product or square root likewise; each is covered by a factor
/// `1 + 16 ε` (and an absolute `8 ε` of the box's coordinates for its fractions).
///
/// Not upstream (#18166): it carries a UV line's evaluation slack ([`line_evaluation_slack`])
/// to the plane, for a trim that is a line.
#[must_use]
pub fn plane_uv_lipschitz(s: &Surface, ends: &[crate::nurbs::math::CPoint], d: f64) -> Option<f64> {
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::surface::SurfaceKind;
    let SurfaceKind::Plane(plane) = s.kind() else {
        return None;
    };
    let base = plane.base_with_nurb().ok()?;
    let net = base.gw_nurb()?;
    let [u0, _, _, u1] = <[f64; 4]>::try_from(net.u_knots()).ok()?;
    let [v0, _, _, v1] = <[f64; 4]>::try_from(net.v_knots()).ok()?;
    let rows = net.cpts();
    if net.degrees() != (1, 1)
        || !(u1 > u0 && v1 > v0)
        || rows.len() != 2
        || rows.iter().any(|r| r.len() != 2)
    {
        return None;
    }
    let polynomial = |c: &CPoint| c.w == NOW || c.w == 1.0;
    let pt = |i: usize, j: usize| -> Option<[f64; 3]> {
        let c = rows.get(i)?.get(j)?;
        polynomial(c).then_some([c.x, c.y, c.z])
    };
    let (p00, p10, p01, p11) = (pt(0, 0)?, pt(1, 0)?, pt(0, 1)?, pt(1, 1)?);
    let grow = 1.0 + 16.0 * f64::EPSILON;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for c in ends {
        if !polynomial(c) {
            return None;
        }
        for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip([c.x, c.y]) {
            *l = l.min(x - d);
            *h = h.max(x + d);
        }
    }
    // the fraction of `x` in `[a, b]`, moved out by its rounding (`lower` toward -inf)
    let frac = |x: f64, a: f64, b: f64, lower: bool| {
        let r = (x - a) / (b - a);
        let e = (r.abs() * 16.0 + (x.abs() + a.abs()) / (b - a) * 8.0) * f64::EPSILON;
        if lower {
            (r - e).min(0.0)
        } else {
            (r + e).max(1.0)
        }
    };
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    // `|(1 - t) a + t b|` rounded up: each term within `(|a| + |b|)(1 + |t|) 4 ε` of exact
    let norm_at = |a: [f64; 3], b: [f64; 3], t: f64| {
        let [a0, a1, a2] = a;
        let [b0, b1, b2] = b;
        let v = [
            (1.0 - t) * a0 + t * b0,
            (1.0 - t) * a1 + t * b1,
            (1.0 - t) * a2 + t * b2,
        ];
        let m = a.iter().chain(&b).fold(0.0_f64, |m, x| m.max(x.abs()));
        ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
            + m * (1.0 + t.abs()) * 16.0 * f64::EPSILON)
            * grow
    };
    let su = |t: f64| norm_at(sub(p10, p00), sub(p11, p01), t) * grow;
    let sv = |t: f64| norm_at(sub(p01, p00), sub(p11, p10), t) * grow;
    let (b_lo, b_hi) = (frac(lo[1], v0, v1, true), frac(hi[1], v0, v1, false));
    let (a_lo, a_hi) = (frac(lo[0], u0, u1, true), frac(hi[0], u0, u1, false));
    let lu = su(b_lo).max(su(b_hi)) / ((u1 - u0) / grow);
    let lv = sv(a_lo).max(sv(a_hi)) / ((v1 - v0) / grow);
    let l = (lu + lv) * grow;
    l.is_finite().then_some(l)
}

/// A bound on `|S(p) - S(q)|` for the surface `s` and UV points `p`, `q` within `d` of each
/// other in each coordinate, both within `d` of the segment between the UV points `ends` (a UV
/// line's NURB, so `d` its [`line_evaluation_slack`]): what a UV line's analytic evaluation
/// adds to the bound [`trim_image_gap_bound`] gives for its NURB. On a plane it is
/// [`plane_uv_lipschitz`] times `d`. On another surface with an exact net
/// ([`NetSurface::of_exact`], probed at `ends`), the net's first partials are enclosed in
/// interval arithmetic over the segment's box widened by `d` ([`NetSurface::partials`]),
/// which holds `p`, `q` and the segment between them: the net is continuous and piecewise
/// polynomial there, so `|S~(p) - S~(q)| <= (sup |S_u| + sup |S_v|) d` along that segment,
/// `S~` the net with its end spans continued past the net. Where the box overshoots the net
/// (a seam on the domain's boundary, its parameters a rounding out), the surface is taken at
/// the clamped point as the gap bound takes it, within the partials' slack of `S~` at each of
/// `p` and `q`: twice the slack is added. `None` for a surface without an exact net, `ends`
/// that are not polynomial points, a box the partials refuse, or numbers not finite.
///
/// Not upstream (#18923): the WR-450 certificate of the UV lines a copied analytic face keeps.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn uv_line_slack_on_surface(
    k: &Kernel,
    s: &Surface,
    ends: &[crate::nurbs::math::CPoint],
    d: f64,
) -> Result<Option<f64>> {
    use crate::nurbs::math::NOW;
    use crate::surface::surface::SurfaceKind;
    if matches!(s.kind(), SurfaceKind::Plane(_)) {
        return Ok(plane_uv_lipschitz(s, ends, d)
            .map(|l| l * d * (1.0 + 4.0 * f64::EPSILON))
            .filter(|x| x.is_finite()));
    }
    if !(d.is_finite() && d >= 0.0) || ends.is_empty() {
        return Ok(None);
    }
    let mut u = Ivl::new(f64::INFINITY, f64::NEG_INFINITY);
    let mut v = u;
    let mut probes = Vec::new();
    for c in ends {
        if !(c.w == NOW || c.w == 1.0) || !(c.x.is_finite() && c.y.is_finite()) {
            return Ok(None);
        }
        let du = Ivl::from(c.x) - Ivl::from(d);
        let dv = Ivl::from(c.y) - Ivl::from(d);
        let eu = Ivl::from(c.x) + Ivl::from(d);
        let ev = Ivl::from(c.y) + Ivl::from(d);
        u = Ivl::new(u.lo.min(du.lo), u.hi.max(eu.hi));
        v = Ivl::new(v.lo.min(dv.lo), v.hi.max(ev.hi));
        probes.push(Point2::new(c.x, c.y));
    }
    let Some(net) = NetSurface::of_exact(k, s, &probes)? else {
        return Ok(None);
    };
    let w = V3 {
        x: u,
        y: v,
        z: Ivl::from(0.0),
    };
    let Ok((p, slack)) = net.partials(&[w], 1, 0) else {
        return Ok(None);
    };
    let (Some(su), Some(sv)) = (norm_upper(p.at(1, 0)), norm_upper(p.at(0, 1))) else {
        return Ok(None);
    };
    let bound =
        ((Ivl::from(su) + Ivl::from(sv)) * Ivl::from(d) + Ivl::from(2.0) * Ivl::from(slack)).hi;
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream: [`line_nurb_slack`] of the line `l` and its NURB (made on a copy when it has
/// none), a UV line when `is_2d`: how far its analytic point can be from its NURB's exact point
/// at the same parameter. `None` where that NURB's data is not two control points of weight
/// one, or a number is not finite.
#[must_use]
pub fn line_evaluation_slack(l: &Line, is_2d: bool) -> Option<f64> {
    use crate::surface::surface::exact_pieces::exact_nurbs_curve_data;
    let base = l.base_with_nurb().ok()?;
    let (cn, _, _, _, _) = exact_nurbs_curve_data(&base, is_2d).ok()?;
    line_nurb_slack(l, &cn, is_2d)
}

/// Not upstream: a bound on the distance between a line's analytic point (`P + s scale V`,
/// [`Line::evaluate_point`]) at a NURB parameter `t` of the domain of `cn`, the line's NURB
/// data (two control points `Q_0`, `Q_1` of weight one), and the exact point of that NURB at
/// `t`. With `E(t)` the line's exact point (affine in `t`): the analytic point is within
/// `e(t)` ([`Curve::point_error_bound`]) of `E(t)` in each coordinate, and `e` grows with `|s|`,
/// affine in `t`, so it is at most its larger value at the domain's ends `t_0`, `t_1`. Each
/// `Q_i` is within `|Q_i - A(t_i)| + e(t_i)` of `E(t_i)` in each coordinate (`A` the analytic
/// point, the difference measured and rounded up), so the NURB's point, the blend of the `Q_i`
/// by `1 - λ` and `λ` with `E` affine, is within the larger of those of `E(t)`. The sum of the
/// two, per coordinate, times `√3` for the distance, rounded up. Unlike a bound in the size of
/// the NURB's own points, this holds for a piece of a line far from its point `P`, and for a
/// NURB that is not the line's (the difference is counted). `None` for data of another shape or
/// numbers not finite.
fn line_nurb_slack(l: &Line, cn: &NurbsCurve, is_2d: bool) -> Option<f64> {
    if cn.p != 1 || cn.n != 2 {
        return None;
    }
    let (t0, t1) = (*cn.u.get(1)?, *cn.u.get(2)?);
    let up = |x: f64| x * (1.0 + 4.0 * f64::EPSILON);
    let dims = if is_2d { 2 } else { 3 };
    let mut e_max = 0.0_f64;
    let mut q_max = 0.0_f64;
    for (i, t) in [t0, t1].into_iter().enumerate() {
        let q = cn.pw.get(4 * i..4 * i + 4)?;
        if q.get(3) != Some(&1.0) {
            return None;
        }
        let e = l.point_error(t)?;
        let a = l.evaluate_point(t);
        let off = [a.x, a.y, a.z]
            .iter()
            .zip(q)
            .take(dims)
            .fold(0.0_f64, |m, (a, q)| m.max(up((a - q).abs())));
        e_max = e_max.max(e);
        q_max = q_max.max(up(off + e));
    }
    let slack = up(up(e_max + q_max) * up(3.0_f64.sqrt()));
    slack.is_finite().then_some(slack)
}

/// Newton steps toward the nearest curve parameter of each trim image point ([`reparametrisation`]):
/// only how tight the bound is depends on them, never whether it holds.
const NEAREST_STEPS: usize = 6;
/// The most points `ψ` passes through, and the deepest it halves a gap between two of them
/// ([`reparametrisation`]).
const PSI_MAX_POINTS: usize = 4097;
const PSI_MAX_DEPTH: u32 = 16;
/// The narrowest gap between two points of `ψ` that halving makes: a piece of the bisection
/// this many halvings above its deepest ([`MAX_DEPTH`]), so a steep stretch of `ψ` spans
/// pieces the bisection can still split (`ψ` steep over a narrower gap leaves its second-order
/// term large in the deepest piece that holds it).
const PSI_MIN_GAP_LEVELS: u32 = 10;
/// The pieces each knot span of the curve and of the trim is cut into for `ψ`'s first points.
const PSI_PER_SPAN: usize = 4;

/// The curve parameter near `guess` (within `[lo, hi]`) of the curve point nearest `target`,
/// by a few Newton steps, with the curve's speed there; the guess itself where a step fails.
/// The parameter returned is the one of the steps' points (the guess included) nearest
/// `target`: where the curve is slow, a step can overshoot to a far stretch of it, and a
/// point of `ψ` there pulls `C(ψ)` off the image (#18166, `brep_mirror` `mat0094`).
fn nearest_param(
    k: &Kernel,
    curve: &Curve,
    target: crate::math::Point3,
    guess: f64,
    [lo, hi]: [f64; 2],
) -> Result<(f64, f64)> {
    use crate::error::ignored;
    let mut t = guess;
    // (parameter, distance, speed) of the nearest point so far
    let mut best = (guess, f64::INFINITY, 0.0);
    for step in 0..=NEAREST_STEPS {
        let Some(d) = ignored(curve.evaluate(k, t, 1, true, false))? else {
            break;
        };
        let (Some(&c0), Some(&c1)) = (d.first(), d.get(1)) else {
            break;
        };
        let den = c1.dot(c1);
        let dist = c0.distance(target);
        if dist < best.1 {
            best = (t, dist, den.sqrt());
        }
        let next = t + (target - c0).dot(c1) / den;
        if step == NEAREST_STEPS || !(den > 0.0 && next.is_finite()) {
            break;
        }
        t = next.max(lo).min(hi);
    }
    Ok((best.0, best.2))
}

/// The reparametrisation `ψ` of the curve onto the trim's parameter `s` over `range`: the C^1
/// cubic [`hermite_psi`] through points `(s_i, t_i)`, `t_i` the parameter of the curve point
/// nearest the trim image at `s_i` (a few Newton steps), `ψ(lo) = lo` and `ψ(hi) = hi`. The
/// points are uniform and cut each span between the `knots` (the curve's and the trim's) into
/// [`PSI_PER_SPAN`], and a gap between
/// two is halved where the nearest parameter at its middle is off their mean by more than
/// `refine` along the curve (a trim image that runs ahead or behind the curve over a short
/// stretch), up to [`PSI_MAX_POINTS`] and [`PSI_MAX_DEPTH`]. The built `ψ` is then checked at
/// each gap's middle and refined where `C(ψ)` there is farther from the image than at both of
/// the gap's ends by more than `refine`, in at most [`PSI_MAX_DEPTH`] rounds. Neither halves a
/// gap below twice the piece of [`PSI_MIN_GAP_LEVELS`]: where the nearest parameter jumps (the
/// trim's image leaves one stretch of the curve for another, #18166 `feature_executive`
/// `single_face_fillet1019_8`), halving would chase the jump down to a gap a few units in the
/// last place wide, and `ψ`'s steep step there would hold the bound at 15. Any `ψ` gives a
/// bound; this one keeps `C(ψ(s))` near the image point at `s`, so the bound measures the
/// distance between the two curves, not how far apart their common parameters slip. `None`
/// when it cannot be built.
fn reparametrisation(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    [lo, hi]: [f64; 2],
    knots: &[f64],
    refine: f64,
) -> Result<Option<NurbsCurve>> {
    use crate::error::ignored;

    let image = |si: f64| -> Result<Option<crate::math::Point3>> {
        let Some(w) = ignored(uv.evaluate_point(k, si))? else {
            return Ok(None);
        };
        ignored(surface.evaluate_point(k, Point2::new(w.x, w.y)))
    };
    // the first points: uniform, and each knot span of the trim and of the curve (on the
    // curve's parameter, close to the trim's) cut into a few, where the image can turn
    let uniform = CORRESPONDENCE_POINTS - 1;
    let mut breaks: Vec<f64> = vec![lo];
    breaks.extend(knots.iter().copied().filter(|&x| lo < x && x < hi));
    breaks.push(hi);
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    let mut at: Vec<f64> = (0..uniform)
        .map(|i| lo + (hi - lo) * (i as f64) / (uniform as f64))
        .collect();
    if (breaks.len() - 1) * PSI_PER_SPAN + uniform < PSI_MAX_POINTS {
        for w in breaks.windows(2) {
            if let [a, b] = w {
                at.extend(
                    (0..PSI_PER_SPAN).map(|j| a + (b - a) * (j as f64) / (PSI_PER_SPAN as f64)),
                );
            }
        }
    }
    at.push(hi);
    at.retain(|&x| lo <= x && x <= hi);
    at.sort_by(f64::total_cmp);
    // (a knot at a uniform point, rounded apart by a few units in the last place: two points
    // that close, each with its own nearest parameter, would make `ψ` all but vertical there;
    // the ends stay `lo` and `hi`)
    at.dedup_by(|x, prev| *x - *prev <= 4.0 * f64::EPSILON * x.abs().max(prev.abs()));
    if let Some(last) = at.last_mut() {
        *last = hi;
    }
    if at.len() < 2 {
        at = vec![lo, hi];
    }
    let n = at.len();
    // the narrowest gap halving makes: a bisection piece `PSI_MIN_GAP_LEVELS` above its deepest
    let min_gap =
        (hi - lo) / (FIRST_PIECES as f64) / f64::from(1_u32 << (MAX_DEPTH - PSI_MIN_GAP_LEVELS));
    let mut first: Vec<(f64, f64)> = Vec::new();
    for (i, &si) in at.iter().enumerate() {
        k.check_cancelled(OP)?;
        let Some(p) = image(si)? else {
            return Ok(None);
        };
        let t = if i == 0 {
            lo
        } else if i + 1 == n {
            hi
        } else {
            nearest_param(k, curve, p, si, [lo, hi])?.0
        };
        first.push((si, t));
    }
    let mut points: Vec<(f64, f64)> = first.iter().take(1).copied().collect();
    let mut count = n;
    for pair in first.windows(2) {
        let [a, b] = pair else {
            return Ok(None);
        };
        let mut stack = vec![(*a, *b, 0_u32)];
        while let Some((a, b, depth)) = stack.pop() {
            let m = 0.5 * (a.0 + b.0);
            if depth < PSI_MAX_DEPTH
                && count < PSI_MAX_POINTS
                && b.0 - a.0 >= 2.0 * min_gap
                && a.0 < m
                && m < b.0
            {
                k.check_cancelled(OP)?;
                let guess = 0.5 * (a.1 + b.1);
                let Some(p) = image(m)? else {
                    return Ok(None);
                };
                let (tm, speed) = nearest_param(k, curve, p, guess, [lo, hi])?;
                if (tm - guess).abs() * speed > refine {
                    count += 1;
                    stack.push(((m, tm), b, depth + 1));
                    stack.push((a, (m, tm), depth + 1));
                    continue;
                }
            }
            points.push(b);
        }
    }
    // The halving above compares the nearest parameter at a gap's middle with the mean of its
    // ends, but `ψ` is a cubic whose slopes come from the neighbouring points: a gap whose ends
    // are on the correspondence can still bow off it (a slope set to zero where the points turn
    // back over a sub-micron knot span leaves the next gap lagging by 4/27 of its width, #18166).
    // So the built `ψ` is checked at each gap's middle: where `C(ψ)` there is farther from the
    // image than at both of the gap's ends by more than `refine`, it bows the gap's `g` above
    // what its end values show, and the nearest point found from `ψ` is added. The test is on
    // the distance, not the parameter: where the parameter is off but the distance is not (the
    // last gap, where `ψ(hi) = hi` holds `ψ` to the curve's end while the image's nearest point
    // is short of it), a point would only make `ψ` steeper and its second-order term larger.
    // `ψ` is rebuilt until no middle bows, in at most [`PSI_MAX_DEPTH`] rounds.
    let Some(mut psi) = hermite_psi(&points, [lo, hi]) else {
        return Ok(None);
    };
    // |C(ψ(s)) - S(W(s))| in double arithmetic (only steering), with `ψ(s)` and the image point
    // (`ψ(s)` by `psi_at`, not enclosed: a reparametrisation's checks are a thousand of these
    // per trim, and only which points it gets, never whether its bound holds, depends on them)
    let off = |psi: &NurbsCurve, si: f64| -> Result<Option<(f64, f64, crate::math::Point3)>> {
        let Some(at) = psi_at(psi, si, [lo, hi]) else {
            return Ok(None);
        };
        let (Some(p), Some(c)) = (image(si)?, ignored(curve.evaluate_point(k, at))?) else {
            return Ok(None);
        };
        Ok(Some((c.distance(p), at, p)))
    };
    // (each gap's piece of `ψ` depends only on a few points around it, so a round checks only
    // the gaps a point added in the round before can have changed (`dirty`): the others have
    // the same piece, bit for bit, and did not bow; and a point's own distance, which two
    // gaps share, is found once a round (`ends`))
    let mut dirty = vec![true; points.len().saturating_sub(1)];
    for _ in 0..PSI_MAX_DEPTH {
        let mut extra: Vec<(f64, f64)> = Vec::new();
        let mut ends: Vec<Option<f64>> = vec![None; points.len()];
        let mut end_off = |psi: &NurbsCurve, i: usize, s: f64| -> Result<Option<f64>> {
            if let Some(&Some(d)) = ends.get(i) {
                return Ok(Some(d));
            }
            let d = off(psi, s)?.map(|x| x.0);
            if let (Some(slot), Some(d)) = (ends.get_mut(i), d) {
                *slot = Some(d);
            }
            Ok(d)
        };
        for (i, pair) in points.windows(2).enumerate() {
            let [a, b] = pair else {
                return Ok(None);
            };
            let m = 0.5 * (a.0 + b.0);
            if !(b.0 - a.0 >= 2.0 * min_gap && a.0 < m && m < b.0) {
                continue;
            }
            if count >= PSI_MAX_POINTS {
                break;
            }
            if dirty.get(i) != Some(&true) {
                continue;
            }
            k.check_cancelled(OP)?;
            let (Some(da), Some(db), Some((dm, at, p))) = (
                end_off(&psi, i, a.0)?,
                end_off(&psi, i + 1, b.0)?,
                off(&psi, m)?,
            ) else {
                return Ok(None);
            };
            // (refined only where the middle is farther than both ends by `refine`; a NaN
            // anywhere is no reason to)
            if dm.partial_cmp(&(da.max(db) + refine)) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            let (tm, _) = nearest_param(k, curve, p, at, [lo, hi])?;
            count += 1;
            extra.push((m, tm));
        }
        if extra.is_empty() {
            break;
        }
        // (points added, flagged, merged in order: each extra point lies inside its own gap)
        let mut merged: Vec<((f64, f64), bool)> = points.iter().map(|&x| (x, false)).collect();
        merged.extend(extra.into_iter().map(|x| (x, true)));
        merged.sort_by(|x, y| x.0 .0.total_cmp(&y.0 .0));
        points = merged.iter().map(|x| x.0).collect();
        // gap `i` (points `i`, `i + 1`) is checked again where a point from `i - 3` to `i + 4`
        // is new: its piece's control values come from the pieces either side of it, whose
        // slopes reach one point further (`i - 2` to `i + 3`), and an end's distance may be
        // taken on the piece before it
        dirty = (0..points.len().saturating_sub(1))
            .map(|i| {
                merged
                    .get(i.saturating_sub(3)..(i + 5).min(merged.len()))
                    .is_some_and(|w| w.iter().any(|x| x.1))
            })
            .collect();
        let Some(rebuilt) = hermite_psi(&points, [lo, hi]) else {
            return Ok(None);
        };
        psi = rebuilt;
    }
    Ok(Some(psi))
}

/// The C^1 piecewise cubic through `points` (`(s_i, t_i)`, `s` increasing from `lo` to `hi`)
/// with Fritsch-Butland slopes (no overshoot where the `t_i` are monotone), as a non-rational
/// B-spline with double interior knots: its control values are the inner Bezier points of each
/// piece, clamped into `[lo, hi]`, its first `lo` and its last `hi`. The double knots make the
/// B-spline C^1 whatever its control values are; the values only decide how close it stays to
/// the points. `None` for fewer than two points or `s` not increasing.
fn hermite_psi(points: &[(f64, f64)], [lo, hi]: [f64; 2]) -> Option<NurbsCurve> {
    let pieces = points.len().saturating_sub(1);
    if pieces == 0 || points.windows(2).any(|w| !matches!(w, [a, b] if a.0 < b.0)) {
        return None;
    }
    let slope = |i: usize| -> f64 {
        match (points.get(i), points.get(i + 1)) {
            (Some(a), Some(b)) => (b.1 - a.1) / (b.0 - a.0),
            _ => 0.0,
        }
    };
    let width = |i: usize| -> f64 {
        match (points.get(i), points.get(i + 1)) {
            (Some(a), Some(b)) => b.0 - a.0,
            _ => 0.0,
        }
    };
    let tangent = |i: usize| -> f64 {
        if i == 0 {
            return slope(0);
        }
        if i == pieces {
            return slope(pieces - 1);
        }
        let (d0, d1) = (slope(i - 1), slope(i));
        if d0 * d1 <= 0.0 {
            return 0.0;
        }
        let (h0, h1) = (width(i - 1), width(i));
        let (w1, w2) = (2.0 * h1 + h0, h1 + 2.0 * h0);
        (w1 + w2) / (w1 / d0 + w2 / d1)
    };
    let mut u = vec![lo; 4];
    let mut pw = vec![lo, 0.0, 0.0, 1.0];
    for i in 0..pieces {
        let (Some(a), Some(b)) = (points.get(i), points.get(i + 1)) else {
            return None;
        };
        let h = b.0 - a.0;
        let inner = [a.1 + h * tangent(i) / 3.0, b.1 - h * tangent(i + 1) / 3.0];
        for x in inner {
            if !x.is_finite() {
                return None;
            }
            pw.extend([x.max(lo).min(hi), 0.0, 0.0, 1.0]);
        }
        if i + 1 < pieces {
            u.extend([b.0, b.0]);
        }
    }
    pw.extend([hi, 0.0, 0.0, 1.0]);
    u.extend([hi; 4]);
    let n = pw.len() / 4;
    if u.len() != n + 4 {
        return None;
    }
    Some(NurbsCurve { p: 3, n, u, pw })
}

/// `ψ(s)` of a reparametrisation [`hermite_psi`] built, in double arithmetic (de Boor's
/// algorithm on its non-rational control values), clamped to `[lo, hi]`: the steering value of
/// [`reparametrisation`]'s checks, not an enclosure. `None` outside `ψ`'s domain or where the
/// value is not finite.
fn psi_at(psi: &NurbsCurve, s: f64, [lo, hi]: [f64; 2]) -> Option<f64> {
    let p = psi.p;
    let (&d0, &d1) = (psi.u.get(p)?, psi.u.get(psi.n)?);
    if !(d0 <= s && s <= d1) {
        return None;
    }
    // the span `[u_k, u_k+1)` holding `s` (the last non-empty one at the domain's end)
    let span = psi.u.get(p..psi.n)?;
    let k = p + span.partition_point(|&x| x <= s).saturating_sub(1);
    let mut d: Vec<f64> = (0..=p)
        .map(|j| psi.pw.get(4 * (k + j - p)).copied())
        .collect::<Option<_>>()?;
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = k + j - p;
            let (&ui, &uip) = (psi.u.get(i)?, psi.u.get(i + p + 1 - r)?);
            let den = uip - ui;
            let a = if den > 0.0 { (s - ui) / den } else { 0.0 };
            let (&prev, &cur) = (d.get(j - 1)?, d.get(j)?);
            *d.get_mut(j)? = (1.0 - a) * prev + a * cur;
        }
    }
    let at = *d.get(p)?;
    at.is_finite().then(|| at.max(lo).min(hi))
}

/// A proved upper bound on the distance between the curve `curve` (`C`, an edge's curve) over
/// its parameters `range` (`[lo, hi]`) and the image `S(W)` of the UV trim `uv` (`W`, on the
/// same parameters) on `surface` (`S`), in both directions: every point of the image is within
/// the bound of the curve and every point of the curve within it of the image (their Hausdorff
/// distance), so within it of the surface: what an edge's tolerance must cover for that trim.
/// `floor` is the caller's scale (below it a tighter bound changes nothing, as for
/// [`curve_face_gap_bound`]).
///
/// The two curves are compared through a reparametrisation `ψ` of `[lo, hi]` onto itself
/// ([`reparametrisation`]), continuous with `ψ(lo) = lo` and `ψ(hi) = hi`: for each `s`,
/// `dist(S(W(s)), C) <= |C(ψ(s)) - S(W(s))|`, and since `ψ` takes every value of `[lo, hi]`,
/// every `C(t)` is `C(ψ(s))` for some `s`, within the same of the image. The same-parameter
/// gap `|C(t) - S(W(t))|` would bound it too, but it also counts how far the two parameters
/// slip along the curve, which follows the fit's tolerance rather than the geometry.
/// `max |C(ψ) - S(W)|` is enclosed as [`certified_gap`] encloses its gap, with the chain rule
/// for `C o ψ`: the surface's exact NURBS net (an analytic surface's, or the B-spline's own),
/// the curve's, the trim's and `ψ`'s NURBS data, every coefficient in rounded interval
/// arithmetic, plus the rounding of a line's analytic evaluation ([`exact_curve_data`]).
///
/// An offset surface of a surface with an exact net is enclosed as the offset of that net
/// ([`NetSurface::of_exact`]): the trims a thickening or a shell refits while its faces are
/// still offsets are bounded as on any other face (#18219).
///
/// `None` when the bound is not enclosed: a surface without an exact net
/// ([`NetSurface::of_exact`]: a procedural blend or sweep, a step surface, an offset of
/// another kind ...) or whose net is not parameterised as the surface, a curve or trim of another kind than a B-spline,
/// an ellipse or a line, an empty or unordered `range`
/// or one past either curve's domain, a piece of the trim whose box runs past the net, or a
/// piece the bisection cannot bound.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    trim_image_gap_bound_up_to(k, surface, curve, uv, range, floor, f64::INFINITY)
}

/// [`trim_image_gap_bound`] for a caller that only asks whether the bound is above `over`: the
/// same bound where it is at most `over`, and `Some(f64::INFINITY)` as soon as a piece shows it
/// is above it, without bounding the rest. Every piece before that one is cut exactly as in
/// [`trim_image_gap_bound`], so the answer to that question is the same.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound_up_to(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    over: f64,
) -> Result<Option<f64>> {
    trim_image_gap_bound_within(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [f64::NEG_INFINITY, over],
        None,
    )
}

/// The exact data [`trim_image_gap_bound_within`] bounds a trim pair from: the curve's data
/// and its slack, the trim's (exactly evaluated), and the surface's net at the trim.
struct TrimPair {
    cn: NurbsCurve,
    c_slack: f64,
    wn: NurbsCurve,
    net: NetSurface,
}

impl TrimPair {
    /// The data of `curve` over `range` against `surface` through the UV trim `uv`; `None`
    /// where `range` is not a finite interval within both curves' domains, the surface has no
    /// exact net ([`NetSurface::of_exact`]: its own, or an offset's base's), a curve no exact
    /// data, or the trim is not evaluated exactly.
    ///
    /// # Errors
    ///
    /// The kernel's unported and cancelled statuses.
    fn of(
        k: &Kernel,
        surface: &Surface,
        curve: &Curve,
        uv: &Curve,
        [lo, hi]: [f64; 2],
    ) -> Result<Option<Self>> {
        use crate::error::ignored;

        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return Ok(None);
        }
        k.check_cancelled(OP)?;
        let (Some((cn, c_slack)), Some((wn, w_slack))) =
            (exact_curve_data(curve, false)?, exact_curve_data(uv, true)?)
        else {
            return Ok(None);
        };
        // A trim's own rounding (a line's) moves its parameters, not a point: no bound follows from
        // it, so only an exactly evaluated trim is enclosed.
        if w_slack != 0.0 {
            return Ok(None);
        }
        let within = |n: &NurbsCurve| match (n.u.get(n.p), n.u.get(n.n)) {
            (Some(&d0), Some(&d1)) => d0 <= lo && hi <= d1,
            _ => false,
        };
        if !(within(&cn) && within(&wn)) {
            return Ok(None);
        }
        let mut probes = Vec::new();
        for i in 0..TRIM_PROBES {
            let t = if i + 1 == TRIM_PROBES {
                hi
            } else {
                lo + (hi - lo) * (i as f64) / ((TRIM_PROBES - 1) as f64)
            };
            let Some(p) = ignored(uv.evaluate_point(k, t))? else {
                return Ok(None);
            };
            probes.push(Point2::new(p.x, p.y));
        }
        let Some(net) = NetSurface::of_exact(k, surface, &probes)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            cn,
            c_slack,
            wn,
            net,
        }))
    }
}

/// Not upstream (#20373): [`structured_trim_gap_bound`] of a pair, the curve's slack added;
/// `None` where it does not apply or is not finite, and for an offset surface's pair (its
/// net is its base's, not the surface's: [`NetSurface::of_exact`]). `off` gives the net's own
/// distance to the surface ([`net_to_surface_bound`]), taken only where the structured bound
/// applies; `same_parameter` bounds the gap at equal parameters
/// ([`structured_trim_gap_bound_as`], #22034).
fn structured_pair_bound(
    k: &Kernel,
    cn: &NurbsCurve,
    c_slack: f64,
    wn: &NurbsCurve,
    net: &NetSurface,
    range: [f64; 2],
    same_parameter: bool,
    off: impl FnOnce() -> Result<Option<f64>>,
) -> Result<Option<f64>> {
    // (an offset's net is its base's, not the surface: no structured bound)
    if net.offset.is_some() {
        return Ok(None);
    }
    let Some(g) = structured_trim_gap_bound_as(k, cn, wn, &net.net, range, same_parameter)? else {
        return Ok(None);
    };
    // (the bound is on the net's image: the net's own distance to the surface added)
    let Some(off) = off()? else {
        return Ok(None);
    };
    Ok(net_bound_on_surface(g, c_slack, off))
}

/// `g`, a bound on a pair's gap to the image on its net, as a bound on its gap to the image
/// on the surface: the curve's slack `c_slack` and the net's own distance `off` to the
/// surface ([`net_to_surface_bound`], [`bisection_residual`]) added. `None` where the sum is
/// not finite.
fn net_bound_on_surface(g: f64, c_slack: f64, off: f64) -> Option<f64> {
    let mut bound = Ivl::from(g) + Ivl::from(c_slack);
    if off > 0.0 {
        bound = bound + Ivl::from(off);
    }
    let bound = bound.hi;
    bound.is_finite().then_some(bound)
}

/// Not upstream (#22273): [`row_piece_trim_gap_bound`] of a pair at equal parameters, with the
/// curve's slack and the net's distance to the surface added as [`structured_pair_bound`] adds
/// them; `Ok(None)` where it does not apply or is not finite, and for an offset surface's
/// pair.
///
/// # Errors
///
/// The kernel's cancelled status.
fn row_piece_pair_bound(
    k: &Kernel,
    to_surface: f64,
    cn: &NurbsCurve,
    c_slack: f64,
    wn: &NurbsCurve,
    net: &NetSurface,
    range: [f64; 2],
) -> Result<Option<f64>> {
    // (an offset's net is its base's, not the surface: no structured bound)
    if net.offset.is_some() {
        return Ok(None);
    }
    let Some(g) = row_piece_trim_gap_bound(k, cn, wn, &net.net, range)? else {
        return Ok(None);
    };
    Ok(net_bound_on_surface(g, c_slack, to_surface))
}

/// Not upstream (#20373): the bound [`trim_image_gap_bound_within`] takes at once for a trim
/// whose image has the curve's own structure ([`structured_trim_gap_bound`], the curve's slack
/// added), whatever the fit: `None` where the pair has no such structure.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub(crate) fn structured_trim_image_gap_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
) -> Result<Option<f64>> {
    let Some(p) = TrimPair::of(k, surface, curve, uv, range)? else {
        return Ok(None);
    };
    structured_pair_bound(k, &p.cn, p.c_slack, &p.wn, &p.net, range, false, || {
        net_to_surface_bound_within(k, surface, &p.net.net, nurbs_uv_box(&p.wn))
    })
}

/// Not upstream (#19910): how much farther the image of a UV trim on its surface can lie from
/// its curve after a move than `‖A‖` times the distance [`trim_image_gap_bound_within`] proves
/// before it, where `A x = L x + o` is the affine map of `m` ([`Axis2Placement::matrix`]
/// layout, `L` its upper 3x3 block, `o` its last column) and `‖A‖` is
/// [`affine_linear_norm_upper`] of `m`. `before` and `after` are `(surface, curve, trim)` of
/// the pair before and after the move, both over `range`.
///
/// The bound is `δc + δs + slack + residual`, rounded up:
/// - `δc` is the largest distance from a control point of the curve's data after the move to
///   `A` of the matching one before (enclosed), and `δs` the same for the surface's net;
/// - `slack` is the curve's own slack after the move and `residual` its net's distance to the
///   surface ([`bisection_residual`]).
///
/// Why it holds: the data before and after have the same degrees, knots and weights (checked
/// bit for bit), so at every parameter they share their rational basis, which is non-negative
/// with sum `1` for positive weights. A point of the data after the move is then within the
/// largest control point distance of `A` of the point before (`A` is affine). The trims are
/// the same data and lie within the net's knot range (their control points do), so the net is
/// evaluated by that basis and not continued past its ends. For any correspondence `t -> w(t)`,
/// `|c1(t) - n1(w)| <= δc + ‖A‖ |c0(t) - n0(w)| + δs` and the curve and surface after the
/// move are within `slack` and `residual` of their data. The proof before bounds
/// `|c0(t) - n0(w)|` for its correspondence by its bound, whose other terms are not negative.
///
/// `None` where the pair has no exact data before or after ([`TrimPair::of`]), the data's
/// structure differs, a weight is not positive, the trim changed, the net is an offset's or
/// the trim is not within its knots, or a term is not finite.
///
/// [`Axis2Placement::matrix`]: crate::geom::axis2_placement::Axis2Placement::matrix
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
#[cfg(test)]
pub(crate) fn moved_pair_room(
    k: &Kernel,
    before: (&Surface, &Curve, &Curve),
    after: (&Surface, &Curve, &Curve),
    range: [f64; 2],
    m: [[f64; 4]; 4],
) -> Result<Option<f64>> {
    Ok(moved_pair_data(k, before, after, range, m)?.map(|p| p.room))
}

/// Not upstream (#22467): the extracted before/after exact data and rounding room of a rigid
/// move's trim pair ([`moved_pair_data`]), so `cover_moved_edge` builds each [`TrimPair`]
/// once and reuses the extracted data for any pre-move or direct post-move bound it runs.
pub(crate) struct MovedTrimPair {
    room: f64,
    range: [f64; 2],
    before: TrimPair,
    after: TrimPair,
}

impl MovedTrimPair {
    /// The rounding room `δc + δs + slack + residual` of [`moved_pair_data`].
    pub(crate) const fn room(&self) -> f64 {
        self.room
    }

    /// Bounds the pair before the move using its already-extracted [`TrimPair`]. With a piece
    /// `budget`, enables endpoint short-circuiting (`collapse_unproved`), returning
    /// [`TrimBoundOutcome::BudgetStopped`] if the cap stops the bisection while all evaluated
    /// endpoints stay within `limits[0]`. `any_within_enough` is true only when the caller
    /// treats every proved bound at most `limits[0]` identically (allowing the same-parameter
    /// structured and identity-correspondence fast paths).
    pub(crate) fn bound_before(
        &self,
        k: &Kernel,
        surface: &Surface,
        curve: &Curve,
        uv: &Curve,
        floor: f64,
        limits: [f64; 2],
        budget: Option<usize>,
        any_within_enough: bool,
    ) -> Result<TrimBoundOutcome> {
        if !(floor.is_finite() && floor >= 0.0) {
            return Ok(TrimBoundOutcome::Unproved);
        }
        bound_trim_pair(
            k,
            surface,
            curve,
            uv,
            self.range,
            floor,
            limits,
            budget,
            Correspondence::Fitted,
            budget.is_some(),
            any_within_enough,
            true,
            &self.before,
        )
    }

    /// Bounds the pair after the move using its already-extracted [`TrimPair`].
    pub(crate) fn bound_after(
        &self,
        k: &Kernel,
        surface: &Surface,
        curve: &Curve,
        uv: &Curve,
        floor: f64,
        limits: [f64; 2],
        budget: Option<usize>,
        any_within_enough: bool,
    ) -> Result<TrimBoundOutcome> {
        if !(floor.is_finite() && floor >= 0.0) {
            return Ok(TrimBoundOutcome::Unproved);
        }
        bound_trim_pair(
            k,
            surface,
            curve,
            uv,
            self.range,
            floor,
            limits,
            budget,
            Correspondence::Fitted,
            budget.is_some(),
            any_within_enough,
            true,
            &self.after,
        )
    }
}

/// Extracts the before/after [`TrimPair`]s and rounding room of a move for reuse in
/// `cover_moved_edge` ([`MovedTrimPair`]).
pub(crate) fn moved_pair_data(
    k: &Kernel,
    before: (&Surface, &Curve, &Curve),
    after: (&Surface, &Curve, &Curve),
    range: [f64; 2],
    m: [[f64; 4]; 4],
) -> Result<Option<MovedTrimPair>> {
    let (s0, c0, w0) = before;
    let (s1, c1, w1) = after;
    let Some(p0) = TrimPair::of(k, s0, c0, w0, range)? else {
        return Ok(None);
    };
    let Some(p1) = TrimPair::of(k, s1, c1, w1, range)? else {
        return Ok(None);
    };
    if p0.wn != p1.wn || p0.net.offset.is_some() || p1.net.offset.is_some() {
        return Ok(None);
    }
    let (a0, a1) = (&p0.cn, &p1.cn);
    if a0.p != a1.p || a0.n != a1.n || a0.u != a1.u {
        return Ok(None);
    }
    let (n0, n1) = (&p0.net.net, &p1.net.net);
    if n0.p != n1.p || n0.q != n1.q || n0.nu != n1.nu || n0.nv != n1.nv || n0.u != n1.u {
        return Ok(None);
    }
    if n0.v != n1.v || !trim_within_knots(&p1.wn, n1) {
        return Ok(None);
    }
    let Some(count) = n0.nu.checked_mul(n0.nv) else {
        return Ok(None);
    };
    let (Some(dc), Some(ds)) = (
        moved_points_room(&a0.pw, &a1.pw, a0.n, m),
        moved_points_room(&n0.pw, &n1.pw, count, m),
    ) else {
        return Ok(None);
    };
    // (#20643: the cone residual windowed to the trim's UV box, as the trim-image bound takes it)
    let Some(residual) = bisection_residual(k, s1, &p1.net, nurbs_uv_box(&p1.wn))? else {
        return Ok(None);
    };
    let room = Ivl::from(dc) + Ivl::from(ds) + Ivl::from(p1.c_slack) + Ivl::from(residual);
    if !room.hi.is_finite() {
        return Ok(None);
    }
    Ok(Some(MovedTrimPair {
        room: room.hi,
        range,
        before: p0,
        after: p1,
    }))
}

/// Not upstream (#19910): an upper bound on the spectral norm of the upper 3x3 block `L` of
/// `m` ([`moved_pair_data`]): `‖L‖² = λmax(LᵀL) <= 1 + ‖LᵀL - I‖`, and the spectral norm of
/// the symmetric `LᵀL - I` is at most its Frobenius norm, enclosed. `None` unless finite.
pub(crate) fn affine_linear_norm_upper(m: [[f64; 4]; 4]) -> Option<f64> {
    let [[a, b, c, _], [d, e, f, _], [g, h, i, _], _] = m;
    let col = |x: f64, y: f64, z: f64| V3 {
        x: Ivl::from(x),
        y: Ivl::from(y),
        z: Ivl::from(z),
    };
    let (x, y, z) = (col(a, d, g), col(b, e, h), col(c, f, i));
    let one = Ivl::from(1.0);
    let sq = |v: Ivl| {
        let m = Ivl::from(v.mag());
        m * m
    };
    let diagonal = sq(dot(x, x) - one) + sq(dot(y, y) - one) + sq(dot(z, z) - one);
    let off = sq(dot(x, y)) + sq(dot(x, z)) + sq(dot(y, z));
    let frobenius = (diagonal + Ivl::from(2.0) * off).sqrt();
    let norm = (one + Ivl::from(frobenius.hi)).sqrt().hi;
    norm.is_finite().then_some(norm)
}

/// The largest distance from a homogeneous control point of `after` to the affine map of `m`
/// ([`moved_pair_data`]) of the matching one of `before`, enclosed; `None` unless both hold
/// `count` points with equal positive weights and every distance is finite.
fn moved_points_room(before: &[f64], after: &[f64], count: usize, m: [[f64; 4]; 4]) -> Option<f64> {
    let [r0, r1, r2, _] = m;
    let row = |r: [f64; 4], p: V3<Ivl>| {
        let [a, b, c, o] = r;
        Ivl::from(a) * p.x + Ivl::from(b) * p.y + Ivl::from(c) * p.z + Ivl::from(o)
    };
    if before.len() != count.checked_mul(4)? || after.len() != before.len() {
        return None;
    }
    let mut worst = 0.0_f64;
    for (q0, q1) in before.chunks_exact(4).zip(after.chunks_exact(4)) {
        let (&[x0, y0, z0, w0], &[x1, y1, z1, w1]) = (q0, q1) else {
            return None;
        };
        // (the same weights give the same rational basis before and after)
        if w0.to_bits() != w1.to_bits() {
            return None;
        }
        let p0 = point_of([x0, y0, z0, w0])?;
        let p1 = point_of([x1, y1, z1, w1])?;
        let moved = V3 {
            x: row(r0, p0),
            y: row(r1, p0),
            z: row(r2, p0),
        };
        worst = worst.max(norm_upper(minus(p1, moved))?);
    }
    Some(worst)
}

/// Whether every control point of the trim data `wn` lies within the knot range of `net` in
/// `u` and `v` (enclosed), so the trim's image is evaluated within the net's knots
/// ([`moved_pair_data`]).
fn trim_within_knots(wn: &NurbsCurve, net: &Nurbs) -> bool {
    let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) = (
        net.u.get(net.p),
        net.u.get(net.nu),
        net.v.get(net.q),
        net.v.get(net.nv),
    ) else {
        return false;
    };
    // (`lo <= x / w <= hi` for a positive weight `w` is `lo w <= x <= hi w`, decided exactly: a
    // trim on the net's edge, as a sphere's seam trims are, lies on it, where an enclosed
    // quotient may round past it)
    let within = |x: f64, w: f64, lo: f64, hi: f64| {
        use crate::geom::expansion::{product, sign, sub};
        let (Some(l), Some(h)) = (product(lo, w), product(hi, w)) else {
            return false;
        };
        sign(&sub(&[x], &l)) >= 0 && sign(&sub(&h, &[x])) >= 0
    };
    wn.pw.chunks_exact(4).all(|q| {
        let &[x, y, _, w] = q else {
            return false;
        };
        [x, y, w, u0, u1, v0, v1].iter().all(|c| c.is_finite())
            && w > 0.0
            && within(x, w, u0, u1)
            && within(y, w, v0, v1)
    })
}

/// [`trim_image_gap_bound_up_to`] for a caller that also takes any bound at most `enough` as
/// it comes: a piece whose bound (with the curve's slack) is at most `enough` is not cut
/// further. The bound is still a proved bound, but where it is at most `enough` it may be any
/// value up to `enough` rather than the tightest the bisection reaches; whether it is at most
/// `enough` is answered as by [`trim_image_gap_bound_up_to`] wherever that one stays within its
/// piece budget (pruned pieces leave the budget to the others, which can only tighten it).
/// An `enough` of `f64::NEG_INFINITY` is [`trim_image_gap_bound_up_to`].
///
/// `budget` caps the bisection's pieces as for [`bisect`]: `None` is the whole budget, with a
/// piece still open at its end bounded as it stands; with a cap, which holds only while the
/// bound may still be at most `enough`, a gap the cap stops is not enclosed, and pieces within
/// the largest end value close as they stand.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_image_gap_bound_within(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    trim_gap_bound_with(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [enough, over],
        budget,
        Correspondence::Fitted,
    )
}

/// Not upstream (WR-450, #18923): a proved upper bound on the same-parameter gap
/// `sup |C(t) - S(W(t))|` over `t` in `range`, for the curve `curve` (`C`), its UV trim `uv`
/// (`W`) and `surface` (`S`): how far the trim's image at each parameter is from the curve's
/// point at that same parameter. It is [`trim_image_gap_bound_within`]'s bisection with `ψ`
/// the identity (degree 1 from `(lo, lo)` to `(hi, hi)`, its evaluation enclosed as any `ψ`'s),
/// so it also counts how far the two parameters slip along the curve: the bound for a trim
/// that is to stand where the edge's parameter pairs it with its other trims (both faces'
/// trims evaluated at one edge parameter, as a volume enclosure's strips and a vertex's
/// sectors take them), which a Hausdorff bound does not give (fillet1019's corner round
/// extended for a defeature: a trim within 4.2e-4 of its edge as a curve, 5.9e-3 from it at
/// equal parameters). The gap it bounds is at least the trim's distance from the curve as a
/// set, which [`trim_image_gap_bound_within`] bounds (the two computed bounds need not be
/// ordered). As there, a trim whose image has the curve's own structure is bounded by the
/// control points at once where that bound is at most `enough` (#22034), here at equal
/// parameters (`structured_trim_gap_bound_as`). `floor`,
/// `enough`, `over` and `budget` mean what they mean there. `None` only where the pair has no
/// exact data (a surface with no exact net at the trim, a curve without exact data, a UV
/// line, whose rounding moves its parameters, a range outside either curve's domain) or the
/// floor is not finite; a pair with exact data whose gap it does not enclose (a net whose
/// own distance from the surface is not enclosed among them, a bisection that does not
/// enclose a piece, or a slack that is not finite) gets `Some(f64::INFINITY)`, since nothing
/// proves that gap.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn trim_same_parameter_gap_bound_within(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    trim_gap_bound_with(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [enough, over],
        budget,
        Correspondence::SameParameter,
    )
}

/// How [`trim_gap_bound_with`] pairs the curve's parameter with the trim's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Correspondence {
    /// The fitted reparametrisation [`reparametrisation`] (a Hausdorff bound: the parameters
    /// may slip along the curve), with the structured bound tried as well, and the identity
    /// for a pair of degree 1.
    Fitted,
    /// The identity: the gap at equal parameters.
    SameParameter,
}

/// [`trim_image_gap_bound_within`] for `cover_moved_edge`'s structure-changed fallback so its
/// bisection pieces are counted in [`crate::kernel::WorkCounters::move_trim_bisection_pieces`].
/// Both fallback call sites treat every proved bound at most `enough` identically (covered /
/// no raise) and lift a piece `budget` to [`MAX_PIECES`] automatically whenever an evaluated
/// endpoint exceeds `enough`.
pub(crate) fn trim_image_gap_bound_for_move(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
) -> Result<Option<f64>> {
    if !(floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let Some(pair) = TrimPair::of(k, surface, curve, uv, range)? else {
        return Ok(None);
    };
    let outcome = bound_trim_pair(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [enough, over],
        budget,
        Correspondence::Fitted,
        budget.is_some(),
        true,
        true,
        &pair,
    )?;
    Ok(match outcome {
        TrimBoundOutcome::Proved(bound) => Some(bound),
        TrimBoundOutcome::ExceedsOver => Some(f64::INFINITY),
        TrimBoundOutcome::BudgetStopped | TrimBoundOutcome::Unproved => None,
    })
}

/// The bound of [`trim_image_gap_bound_within`] (`Correspondence::Fitted`) or
/// [`trim_same_parameter_gap_bound_within`] (`Correspondence::SameParameter`).
fn trim_gap_bound_with(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
    correspondence: Correspondence,
) -> Result<Option<f64>> {
    let fitted = correspondence == Correspondence::Fitted;
    if !(floor.is_finite() && floor >= 0.0) {
        return Ok(None);
    }
    let Some(pair) = TrimPair::of(k, surface, curve, uv, range)? else {
        return Ok(None);
    };
    let outcome = bound_trim_pair(
        k,
        surface,
        curve,
        uv,
        range,
        floor,
        [enough, over],
        budget,
        correspondence,
        false,
        false,
        false,
        &pair,
    )?;
    let bound = match outcome {
        TrimBoundOutcome::Proved(bound) => Some(bound),
        TrimBoundOutcome::ExceedsOver => Some(f64::INFINITY),
        TrimBoundOutcome::BudgetStopped | TrimBoundOutcome::Unproved => None,
    };
    // (a same-parameter bound is asked of a pair with exact data: one it does not enclose is
    // not proved within any bound, which `Some(INFINITY)` tells from a pair with none)
    Ok(if fitted {
        bound
    } else {
        Some(bound.unwrap_or(f64::INFINITY))
    })
}

/// [`trim_gap_bound_with`] once the pair's exact data are made: `collapse_unproved` forwards
/// to [`bisect_core`], `any_within_enough` enables fast proofs (`structured_pair_bound` at equal
/// parameters and identity-correspondence bisection) that are only valid when the caller treats
/// every proved bound at most `enough` identically, and `count_move_pieces` adds evaluated
/// pieces to [`crate::kernel::WorkCounters::move_trim_bisection_pieces`].
fn bound_trim_pair(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
    [enough, over]: [f64; 2],
    budget: Option<usize>,
    correspondence: Correspondence,
    collapse_unproved: bool,
    any_within_enough: bool,
    count_move_pieces: bool,
    pair: &TrimPair,
) -> Result<TrimBoundOutcome> {
    let fitted = correspondence == Correspondence::Fitted;
    let cn = &pair.cn;
    let c_slack = pair.c_slack;
    let wn = &pair.wn;
    let net = &pair.net;
    // (#20373: a trim whose image has the curve's own structure is bounded by their control
    // points at once, the curve's slack added ([`structured_pair_bound`]); where that is
    // at most `enough` (and `over`) it is the answer, otherwise the bisection runs as without
    // it. Its second case pairs the parameters by a map of its own, so at equal parameters it
    // also counts how far that map moves them (#22034: a join of many patterned copies
    // bisected each exact copy of a trim a boolean carries, for every union, down to just
    // under its tolerance)
    // (#20373 review r4: the bisection bounds the net's image; the net's own distance to the
    // surface ([`bisection_residual`]) is added to the curve's slack, so a bound past the
    // net's is not taken for the surface's; a net it does not enclose is not bounded. Taken
    // once, before the structured bound, which adds the same distance: #20643)
    let Some(to_surface) = bisection_residual(k, surface, net, nurbs_uv_box(wn))? else {
        return Ok(TrimBoundOutcome::Unproved);
    };
    if enough.is_finite() {
        let meets = |bound: f64| bound <= enough && bound <= over;
        let structured = structured_pair_bound(k, cn, c_slack, wn, net, range, !fitted, || {
            Ok(Some(to_surface))
        })?;
        if let Some(bound) = structured {
            if meets(bound) {
                return Ok(TrimBoundOutcome::Proved(bound));
            }
        }
        // (#22467: a same-parameter structured bound also bounds the Hausdorff distance and
        // covers end rows/columns of higher-degree nets such as ball patches and spheres as
        // well as sub-interval lines; only used when the caller treats all bounds <= `enough`
        // identically so exact bound values for other callers stay bit-for-bit unchanged.)
        if fitted && any_within_enough {
            if let Some(bound) =
                structured_pair_bound(k, cn, c_slack, wn, net, range, true, || {
                    Ok(Some(to_surface))
                })?
            {
                if meets(bound) {
                    return Ok(TrimBoundOutcome::Proved(bound));
                }
            }
        }
        // (#22273, #22820: where those cases give no bound or one above the cutoffs, a trim
        // along a piece of a row is bounded on that piece ([`row_piece_pair_bound`], the same
        // slack and residual added): a full row's check of a trim along a piece of it compares
        // the piece's ends with the row's. The smaller of the two certified bounds (`structured`
        // includes #23142's ruling bound) is the answer where it meets the cutoffs. The piece's
        // bound is at equal parameters, and it is a certified bound on the gap as sets too:
        // that gap is taken over the reparametrisations `ψ`, and the identity is one of them
        // where the curve and the trim share `range`, as the piece's bound asks. So it is taken
        // for a fitted trim as for one at equal parameters, whatever `any_within_enough`
        // (#22820 took it for a fitted trim only where that is set, and the stitch cover does
        // not set it, so it bisected every flank trim). For a fitted trim it can be tighter
        // than the bound bisection stops at (`topology.brep_mirror` mat0019, mat0032 and
        // mat0049's edge 42, through the mirror's move cover, which sets `any_within_enough`
        // since #22820). A screw sweep's joined flank holds such a trim, along half a turn of
        // its row, for each cylinder piece beside it, and each was bisected down to just under
        // its tolerance without it: stress corpus S0252, a modelled M100x6 thread, 22 turns)
        if let Some(piece) = row_piece_pair_bound(k, to_surface, cn, c_slack, wn, net, range)? {
            let bound = structured.map_or(piece, |s| s.min(piece));
            if meets(bound) {
                return Ok(TrimBoundOutcome::Proved(bound));
            }
        }
    }
    if fitted {
        k.work_counters.add_fitted_trim_bisection();
    } else {
        k.work_counters.add_same_parameter_bisection();
    }
    // (#22467: pure exact-input memoisation of `bound_trim_pair` for NURBS-evaluated move pairs.
    // Every input that affects the outcome is encoded in `memo_key` bit for bit, so clearing
    // `k.caches` or turning off `k.memoize_curve_caches` never changes any result.)
    let memo_key = if k.memoize_curve_caches && fitted && count_move_pieces {
        pure_nurbs_flags(surface, curve, uv).and_then(|eval_flags| {
            trim_bound_key(
                pair,
                range,
                to_surface,
                floor,
                [enough, over],
                budget,
                collapse_unproved,
                any_within_enough,
                eval_flags,
            )
        })
    } else {
        None
    };
    if let Some(key) = memo_key.as_ref() {
        if let Some(cached) = k.caches.trim_move_bounds.borrow().get(key) {
            k.check_cancelled(OP)?;
            return Ok(cached);
        }
    }
    let finish = |memo_key: Option<TrimBoundKey>, outcome: TrimBoundOutcome| {
        if let Some(key) = memo_key {
            k.caches.trim_move_bounds.borrow_mut().insert(key, outcome);
        }
        Ok(outcome)
    };
    // (only a positive residual is added: an interval sum rounds outward, so adding `0`
    // would move the slack an ulp)
    let c_slack = if to_surface > 0.0 {
        (Ivl::from(c_slack) + Ivl::from(to_surface)).hi
    } else {
        c_slack
    };
    if !c_slack.is_finite() {
        return finish(memo_key, TrimBoundOutcome::Unproved);
    }
    let [r0, r1] = range;
    let curve_kinks = kinks(cn, r0, r1);
    let trim_kinks = kinks(wn, r0, r1);
    // (#22467: for a caller that accepts any proved bound at most `enough`, try the identity
    // correspondence `ψ(s) = s` first within a small piece budget before fitting a dense
    // Hermite `ψ`. Most UV trims are parameterised on their edge's parameter and close in a
    // few dozen pieces without hundreds of Newton projections or knot cuts. Any correspondence
    // gives a proved upper bound; if the identity does not prove `bound <= enough.min(over)`
    // within the budget, fall through to [`reparametrisation`] unchanged.)
    let target = enough.min(over);
    let id_budget = budget.map_or(IDENTITY_FAST_PIECES, |b| b.min(IDENTITY_FAST_PIECES));
    if fitted
        && any_within_enough
        && enough.is_finite()
        && target > c_slack
        && id_budget >= FIRST_PIECES
    {
        let enough_gap = (Ivl::from(target) - Ivl::from(c_slack)).lo;
        if enough_gap > 0.0 {
            let id = identity_psi(range);
            let sides = Sides {
                curve: cn,
                trim: wn,
                psi: &id,
                curve_kinks: curve_kinks.clone(),
                trim_kinks: trim_kinks.clone(),
                range,
            };
            let cache = RefCell::new(BTreeMap::new());
            let end = |t: f64| {
                let mut cache = cache.borrow_mut();
                cached_trim_endpoint(&mut cache, t, || reparametrised_endpoint(&sides, net, t))
            };
            let (id_outcome, evaluated) = {
                let mut look = polled_end(k, end);
                bisect_core(
                    range,
                    FIRST_PIECES,
                    &[],
                    CLOSE_RELATIVE,
                    floor,
                    [enough_gap, enough_gap],
                    Some(id_budget),
                    true,
                    |a, b| {
                        k.check_cancelled(OP)?;
                        piece_reparametrised(&sides, net, a, b, end)
                    },
                    Some(&mut look),
                )?
            };
            if count_move_pieces {
                k.work_counters.add_move_trim_bisection_pieces(evaluated);
            }
            match id_outcome {
                BisectOutcome::Proved(gap) => {
                    let bound = (Ivl::from(gap) + Ivl::from(c_slack)).hi;
                    if bound.is_finite() && bound <= enough && bound <= over {
                        return finish(memo_key, TrimBoundOutcome::Proved(bound));
                    }
                }
                // (#22820: when the caller collapses budget-stopped runs whose endpoints all
                // stayed within `enough` (`enough <= over`, so `enough_gap` matches `run`'s),
                // every evaluated endpoint of `|C(t) - S(W(t))|` — an upper bound on the
                // Hausdorff distance at `t` — stayed within `enough`: skip fitting a dense
                // Hermite `ψ` just to hit `BudgetStopped` again on a pole or C^0 knot row.)
                BisectOutcome::BudgetStopped
                    if collapse_unproved && budget.is_some() && enough <= over =>
                {
                    return finish(memo_key, TrimBoundOutcome::BudgetStopped);
                }
                BisectOutcome::ExceedsOver
                | BisectOutcome::BudgetStopped
                | BisectOutcome::Unproved => {}
            }
        }
    }
    let psi = if fitted {
        let knots: Vec<f64> = cn.u.iter().chain(wn.u.iter()).copied().collect();
        let Some(psi) = reparametrisation(k, surface, curve, uv, range, &knots, floor)? else {
            return finish(memo_key, TrimBoundOutcome::Unproved);
        };
        psi
    } else {
        identity_psi(range)
    };
    if !kinks(&psi, r0, r1).is_empty() {
        return finish(memo_key, TrimBoundOutcome::Unproved);
    }
    let budget_stopped = Cell::new(false);
    // (the bound of the curve data `cn` against the net `net` through the reparametrisation
    // `psi`, the curve's slack added, with the pieces closing at `enough`)
    let run = |psi: &NurbsCurve,
               cn: &NurbsCurve,
               net: &NetSurface,
               enough: f64,
               cuts: &[f64],
               endpoints: &mut BTreeMap<u64, f64>|
     -> Result<Option<f64>> {
        let sides = Sides {
            curve: cn,
            trim: wn,
            psi,
            curve_kinks: curve_kinks.clone(),
            trim_kinks: trim_kinks.clone(),
            range,
        };
        // (the bound adds the curve's slack, at least 0, to the gap: a gap above `over` is
        // enough; a piece across a corner is bounded to first order by
        // `piece_reparametrised`, so the first pieces need no cut there)
        // (a gap at most `enough` less the curve's slack, rounded down, has a bound at most
        // `enough`: the sum's exact value is at most `enough`, so rounding it up stays there)
        let enough_gap = (Ivl::from(enough) - Ivl::from(c_slack)).lo;
        // (the pieces and the look at the end values share the cache: the same values either
        // way, [`bisect_with_ends`])
        let cache = RefCell::new(core::mem::take(endpoints));
        let end = |t: f64| {
            let mut cache = cache.borrow_mut();
            cached_trim_endpoint(&mut cache, t, || reparametrised_endpoint(&sides, net, t))
        };
        let bisect_result = {
            let mut look = polled_end(k, end);
            bisect_core(
                range,
                FIRST_PIECES,
                cuts,
                CLOSE_RELATIVE,
                floor,
                [enough_gap, over],
                budget,
                collapse_unproved,
                |a, b| {
                    k.check_cancelled(OP)?;
                    piece_reparametrised(&sides, net, a, b, end)
                },
                Some(&mut look),
            )
        };
        *endpoints = cache.into_inner();
        let (outcome, evaluated) = bisect_result?;
        if count_move_pieces {
            k.work_counters.add_move_trim_bisection_pieces(evaluated);
        }
        let gap = match outcome {
            BisectOutcome::Proved(gap) => gap,
            BisectOutcome::ExceedsOver => return Ok(Some(f64::INFINITY)),
            BisectOutcome::BudgetStopped => {
                budget_stopped.set(true);
                return Ok(None);
            }
            BisectOutcome::Unproved => return Ok(None),
        };
        let bound = (Ivl::from(gap) + Ivl::from(c_slack)).hi;
        Ok(bound.is_finite().then_some(bound))
    };
    // Where the bound is above `enough` (or not enclosed), the first pieces are cut again at
    // every interior knot of `ψ` and the smaller bound kept: `ψ` is a C^1 cubic spline whose
    // second derivative jumps at its knots, and where its refinement chased a steep step in the
    // correspondence down to spans far narrower than the bisection's deepest piece, a piece
    // across those spans has a second-order term as large as the steepest span's, which no
    // halving shrinks (#18166: `topology.brep_local_operation` fc0000, bounds 0.43 and 8.7e-3
    // from one piece at depth 30, for trims 1.2e-6 and 1.2e-7 from their edges by dense
    // samples). Pieces within one span of `ψ` each have a cubic's bounded second derivative.
    // (The cuts are `ψ`'s distinct interior knots: one per interpolated point, so at most
    // [`PSI_MAX_POINTS`]. The first pieces are then at most [`FIRST_PIECES`] plus the cuts,
    // and no piece is cut further past [`MAX_PIECES`]: the work stays bounded.)
    let with_psi = |psi: &NurbsCurve| -> Result<Option<f64>> {
        let mut endpoints = BTreeMap::new();
        let mut first = run(psi, cn, net, enough, &[], &mut endpoints)?;
        // (#22820: when `collapse_unproved` is enabled and `first` stopped on the piece budget
        // with every evaluated endpoint within `enough`, do not spend thousands of knot cuts or
        // a recentred retry — whose `enough - slack` can also lift the piece cap — on a trim the
        // caller already treats as covered.)
        if collapse_unproved && first.is_none() && budget_stopped.get() {
            return Ok(None);
        }
        let mut cuts: Vec<f64> = Vec::new();
        if !first.is_some_and(|g| g <= enough) {
            cuts = psi
                .u
                .iter()
                .copied()
                .filter(|&x| r0 < x && x < r1)
                .collect();
            cuts.dedup();
            if !cuts.is_empty() {
                if let Some(g) = run(psi, cn, net, enough, &cuts, &mut endpoints)? {
                    first = Some(first.map_or(g, |b| b.min(g)));
                }
            }
        }
        drop(endpoints);
        recentred_retry(first, [enough, over], cn, net, |cn, net, enough| {
            // Translation rounds the points in the enclosure, so it gets its own cache.
            run(psi, cn, net, enough, &cuts, &mut BTreeMap::new())
        })
    };
    let mut bound = with_psi(&psi)?;
    // Any reparametrisation gives a bound. A line's against a trim of degree 1 corresponds
    // by their common parameter as it stands (on a plane, the image is the line itself), and
    // the fitted `psi` only adds its own steps, each as long as the curve's speed makes it:
    // `primitive.api_primitives` gen1108, a line 5.5e8 long bounded at 1.4e-3 and 1e4 against
    // its trims, 1e-7 by samples. The identity is tried there too, the smaller bound kept.
    if fitted && cn.p == 1 && wn.p == 1 && !bound.is_some_and(|g| g <= enough) {
        if let Some(identity) = hermite_psi(&[(r0, r0), (r1, r1)], range) {
            if let Some(g) = with_psi(&identity)? {
                bound = Some(bound.map_or(g, |b| b.min(g)));
            }
        }
    }
    // (the cutoff applies to the bound returned, the curve's slack included: a gap at most
    // `over` whose slack takes it past `over` is above it too)
    let outcome = match bound {
        Some(b) if b.is_finite() && b <= over => TrimBoundOutcome::Proved(b),
        Some(_) => TrimBoundOutcome::ExceedsOver,
        None if budget_stopped.get() => TrimBoundOutcome::BudgetStopped,
        None => TrimBoundOutcome::Unproved,
    };
    finish(memo_key, outcome)
}

/// Not upstream (WR-450, #18923): [`trim_image_gap_bound`] for an edge curve that is a curve
/// on a surface, `C(r) = S_c(W_c(r))` (`CrvOnSurf`: its UV curve `W_c` on its own surface
/// `S_c`, as it evaluates), against the trim `uv` (`W`) on `surface` (`S`). The same
/// reparametrised bisection: for each piece `[a, b]` of the trim's parameter `s`, the gap
/// `g = C(ψ(s)) - S(W(s))` is at most its end values plus `h^2 / 8 sup |g''|` (or `h sup |g'|`
/// where either side may not be C^1 inside the piece), with the curve side's derivatives
/// lifted by the chain rule from `W_c`'s NURBS data (enclosed over the range `ψ` sweeps) and
/// `S_c`'s exact net's partials over the box `W_c` sweeps there ([`NetSurface::partials`]):
/// `C' = S_u u' + S_v v'`, `C'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v''`.
/// Each net's slack past its end is added to the piece (and to its end values), and each
/// net's own distance to its surface ([`bisection_residual`]) to the bound. Every
/// coefficient is in outward-rounded interval arithmetic.
///
/// `None` when it is not enclosed: `curve` not a curve on a surface, either UV curve without
/// exact data (or one evaluated with rounding, a UV line's), a surface without an exact net,
/// a range past either UV curve's domain or not finite, or a piece the bisection cannot
/// bound. The work is [`bisect`]'s, bounded by [`MAX_PIECES`].
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn crv_on_surf_trim_gap_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    range: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    use crate::curve::curve::CurveKind;
    use crate::error::ignored;

    let CurveKind::CrvOnSurf(cos) = curve.kind() else {
        return Ok(None);
    };
    let [lo, hi] = range;
    if !(floor.is_finite() && floor >= 0.0 && lo.is_finite() && hi.is_finite() && lo < hi) {
        return Ok(None);
    }
    k.check_cancelled(OP)?;
    let (Some((wc, wc_slack)), Some((wn, w_slack))) = (
        exact_curve_data(cos.uv_curve(), true)?,
        exact_curve_data(uv, true)?,
    ) else {
        return Ok(None);
    };
    // (the trim exact; the curve's UV curve exact, or a UV line enclosed as its NURB with
    // its analytic evaluation's slack, `wc_slack` apart in UV, carried to `S_c`)
    if w_slack != 0.0 {
        return Ok(None);
    }
    let line_slack = if wc_slack == 0.0 {
        0.0
    } else {
        let CurveKind::Line(l) = cos.uv_curve().kind() else {
            return Ok(None);
        };
        let nurb = l.base_with_nurb()?;
        let Ok(ends) = nurb.control_points() else {
            return Ok(None);
        };
        match uv_line_slack_on_surface(k, cos.surface(), ends, wc_slack)? {
            Some(x) => x,
            None => return Ok(None),
        }
    };
    let within = |n: &NurbsCurve| match (n.u.get(n.p), n.u.get(n.n)) {
        (Some(&d0), Some(&d1)) => d0 <= lo && hi <= d1,
        _ => false,
    };
    if !(within(&wc) && within(&wn)) {
        return Ok(None);
    }
    let probes = |c: &Curve| -> Result<Option<Vec<Point2>>> {
        let mut out = Vec::new();
        for i in 0..TRIM_PROBES {
            let t = if i + 1 == TRIM_PROBES {
                hi
            } else {
                lo + (hi - lo) * (i as f64) / ((TRIM_PROBES - 1) as f64)
            };
            let Some(p) = ignored(c.evaluate_point(k, t))? else {
                return Ok(None);
            };
            out.push(Point2::new(p.x, p.y));
        }
        Ok(Some(out))
    };
    let (Some(face_probes), Some(curve_probes)) = (probes(uv)?, probes(cos.uv_curve())?) else {
        return Ok(None);
    };
    let (Some(face_net), Some(curve_net)) = (
        NetSurface::of_exact(k, surface, &face_probes)?,
        NetSurface::of_exact(k, cos.surface(), &curve_probes)?,
    ) else {
        return Ok(None);
    };
    let (Some(r_face), Some(r_curve)) = (
        bisection_residual(k, surface, &face_net, None)?,
        bisection_residual(k, cos.surface(), &curve_net, None)?,
    ) else {
        return Ok(None);
    };
    let residual = (Ivl::from(r_face) + Ivl::from(r_curve)).hi;
    if !residual.is_finite() {
        return Ok(None);
    }
    let knots: Vec<f64> = wc.u.iter().chain(wn.u.iter()).copied().collect();
    let Some(psi) = reparametrisation(k, surface, curve, uv, range, &knots, floor)? else {
        return Ok(None);
    };
    if !kinks(&psi, lo, hi).is_empty() {
        return Ok(None);
    }
    let curve_kinks = kinks(&wc, lo, hi);
    let trim_kinks = kinks(&wn, lo, hi);
    let add = |a: f64, b: f64| (Ivl::from(a) + Ivl::from(b)).hi;
    // (the gap at one trim parameter, both nets' slacks past their ends added)
    let endpoint = |t: f64| -> Result<Option<f64>> {
        let at = Ivl::from(t);
        let Some(r) = psi_over(&psi, at, 0, range)? else {
            return Ok(None);
        };
        let Some(&r0) = r.first() else {
            return Ok(None);
        };
        let wcp = curve_derivs_over(&wc, r0, 0, None)?;
        let w = curve_derivs_over(&wn, at, 0, None)?;
        if wcp.is_empty() || w.is_empty() {
            return Ok(None);
        }
        let (cp, c_slack) = curve_net.partials(&wcp, 0, 0)?;
        let (sp, s_slack) = face_net.partials(&w, 0, 0)?;
        let Some(g) = norm_upper(minus(cp.at(0, 0), sp.at(0, 0))) else {
            return Ok(None);
        };
        let g = add(add(g, c_slack), s_slack);
        Ok(g.is_finite().then_some(g))
    };
    let piece = |a: f64, b: f64| -> Result<Option<PieceBound>> {
        let mut ends = 0.0_f64;
        for t in [a, b] {
            let Some(g) = endpoint(t)? else {
                return Ok(None);
            };
            ends = ends.max(g);
        }
        let span = Ivl::new(a, b);
        let Some(r) = psi_over(&psi, span, 2, range)? else {
            return Ok(None);
        };
        let (Some(&r0), Some(&p1), Some(&p2)) = (r.first(), r.get(1), r.get(2)) else {
            return Ok(None);
        };
        let wcd = uv_derivs_over(&wc, r0, 2)?;
        let wd = uv_derivs_over(&wn, span, 2)?;
        let (Some(c1w), Some(c2w), Some(w1)) = (wcd.get(1), wcd.get(2), wd.get(1)) else {
            return Ok(None);
        };
        let (u1, v1) = (c1w.x, c1w.y);
        let (u2, v2) = (c2w.x, c2w.y);
        // (the curve side's first two derivatives in `r`, where `S_c` is C^1 across every
        // knot line the box crosses; else its first alone, the one-sided partials hulled)
        let lifted = match curve_net.partials(&wcd, 2, 1) {
            Ok((p, slack)) => {
                let c1 = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
                let mut c2 = scaled(p.at(2, 0), u1 * u1);
                c2 = plus(c2, scaled(p.at(1, 1), Ivl::from(2.0) * u1 * v1));
                c2 = plus(c2, scaled(p.at(0, 2), v1 * v1));
                c2 = plus(c2, scaled(p.at(1, 0), u2));
                c2 = plus(c2, scaled(p.at(0, 1), v2));
                (c1, Some(c2), slack)
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => {
                let (p, slack) = curve_net.partials(&wcd, 1, 0)?;
                let c1 = plus(scaled(p.at(1, 0), u1), scaled(p.at(0, 1), v1));
                (c1, None, slack)
            }
        };
        let (c1, c2, c_slack) = lifted;
        let cp = scaled(c1, p1);
        let h = Ivl::from(b) - Ivl::from(a);
        let corner = trim_kinks.iter().any(|&x| a < x && x < b)
            || curve_kinks.iter().any(|&x| r0.lo < x && x < r0.hi);
        match c2 {
            Some(c2) if !corner => {
                let cpp = plus(scaled(c2, p1 * p1), scaled(c1, p2));
                let Some((term, slack)) = second_term(&face_net, &wd, cp, cpp, h)? else {
                    return Ok(None);
                };
                Ok(PieceBound::with(ends, term, add(slack, c_slack)))
            }
            _ => {
                let (p, slack) = face_net.partials(&wd, 1, 0)?;
                let sw = plus(scaled(p.at(1, 0), w1.x), scaled(p.at(0, 1), w1.y));
                let Some(m) = norm_upper(minus(cp, sw)) else {
                    return Ok(None);
                };
                Ok(PieceBound::with(
                    ends,
                    h * Ivl::from(m),
                    add(slack, c_slack),
                ))
            }
        }
    };
    let Some(gap) = bisect(
        range,
        FIRST_PIECES,
        &trim_kinks,
        CLOSE_RELATIVE,
        floor,
        [f64::NEG_INFINITY, f64::INFINITY],
        None,
        |a, b| {
            k.check_cancelled(OP)?;
            piece(a, b)
        },
    )?
    else {
        return Ok(None);
    };
    let bound = add(add(gap, residual), line_slack);
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream (#18923): [`trim_image_gap_bound`] for a B-spline trim whose domain
/// `[d0, d1]` ends inside `range` while the curve's domain holds all of it. Such a trim is
/// evaluated clamped (out-of-bounds evaluation off: `BSplineCurve::evaluate_point`), so on
/// `[lo, d0]` it is `W(d0)` and on `[d1, hi]` it is `W(d1)`, both points of the image over
/// `[d0', d1'] = [max(lo, d0), min(hi, d1)]`. That inner range is bounded by
/// [`trim_image_gap_bound`] at `B`, its reparametrisation fixing its ends, so
/// `|C(d0') - S(W(d0'))| <= B`; a curve point `C(t)` with `t` in a sliver is then within
/// `|C(t) - C(d0')| + B` of the image, and `|C(t) - C(d0')| <= (d0' - lo) max |C'|` over the
/// sliver ([`curve_derivs_over`], the curve's exact data), plus twice the curve's slack
/// (its evaluation's distance from that data at either point). The bound is `B` plus the
/// larger sliver's term, every step rounded up.
///
/// `None` where the trim is not such a B-spline (out-of-bounds evaluation on, or its domain
/// holds `range`), the curve's exact data or domain does not hold `range`, the inner range
/// is empty, or [`trim_image_gap_bound`] encloses nothing on it.
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
pub fn clamped_trim_gap_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv: &Curve,
    [lo, hi]: [f64; 2],
    floor: f64,
) -> Result<Option<f64>> {
    use crate::curve::curve::CurveKind;

    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Ok(None);
    }
    let CurveKind::BSpline(bs) = uv.kind() else {
        return Ok(None);
    };
    if bs.is_out_of_bounds_enabled() {
        return Ok(None);
    }
    let (Some((cn, c_slack)), Some((wn, _))) =
        (exact_curve_data(curve, false)?, exact_curve_data(uv, true)?)
    else {
        return Ok(None);
    };
    let (Some(&c0), Some(&c1), Some(&d0), Some(&d1)) = (
        cn.u.get(cn.p),
        cn.u.get(cn.n),
        wn.u.get(wn.p),
        wn.u.get(wn.n),
    ) else {
        return Ok(None);
    };
    if !(c0 <= lo && hi <= c1) || (d0 <= lo && hi <= d1) {
        return Ok(None);
    }
    let (a, b) = (lo.max(d0), hi.min(d1));
    if a.partial_cmp(&b) != Some(core::cmp::Ordering::Less) {
        return Ok(None);
    }
    let Some(inner) = trim_image_gap_bound(k, surface, curve, uv, [a, b], floor)? else {
        return Ok(None);
    };
    let mut worst = Ivl::from(0.0);
    for (s0, s1) in [(lo, a), (b, hi)] {
        if s0.partial_cmp(&s1) != Some(core::cmp::Ordering::Less) {
            continue;
        }
        let d = curve_derivs_over(&cn, Ivl::new(s0, s1), 1, None)?;
        let Some(m) = d.get(1).copied().and_then(norm_upper) else {
            return Ok(None);
        };
        let term = (Ivl::from(s1) - Ivl::from(s0)) * Ivl::from(m)
            + Ivl::from(c_slack)
            + Ivl::from(c_slack);
        if term.hi > worst.hi {
            worst = term;
        }
    }
    let bound = (Ivl::from(inner) + worst).hi;
    Ok(bound.is_finite().then_some(bound))
}

/// Not upstream (#18166): the bound `first` of a curve against a surface (`None` where it is
/// not enclosed), tightened where it is above `enough` (or not enclosed) and the curve lies
/// far from the origin for its size ([`recentre`]): `run` bounds the same pair with both moved
/// by `-T`, `T` the middle of the curve's control points, and the smaller bound is kept.
///
/// Far from the origin the interval enclosures widen with the coordinates themselves, not
/// with the geometry: a rational curve's derivative `(A' - w' C) / w` encloses `A'` and
/// `w' C` apart, each as wide as the coordinate `C` is large, so the second-order term of a
/// piece stops shrinking with it, and the bisection runs out of pieces with the bound held
/// there (`primitive.api_primitives` gen0353: an ellipse at `z = 4.5e8`, 1.2e-7 from its
/// faces by samples, bounded at 4.76e4; gen0748: lines at 6e9, 9.5e-7 by samples, bounded
/// at 1.2e-4). A translation keeps every distance, so it only moves the coordinates the
/// enclosures widen with.
///
/// The moved data are rounded ([`translated_pw`]): each control point lands within its
/// slack of the exact moved point, so each moved NURBS (positive weights: its points are
/// convex combinations of the control points) is within its largest slack of the exact
/// moved curve or surface, and the bound of the moved pair plus both slacks bounds the
/// distance of the given pair. `run` is given `enough` less those slacks; a moved bound that
/// is not finite, or past `over`, keeps `first`.
///
/// # Errors
///
/// Those of `run`.
fn recentred_retry(
    first: Option<f64>,
    [enough, over]: [f64; 2],
    cn: &NurbsCurve,
    net: &NetSurface,
    run: impl FnOnce(&NurbsCurve, &NetSurface, f64) -> Result<Option<f64>>,
) -> Result<Option<f64>> {
    if first.is_some_and(|g| g <= enough) {
        return Ok(first);
    }
    let Some(t) = recentre(cn) else {
        return Ok(first);
    };
    let (Some((moved_curve, curve_slack)), Some((moved_net, net_slack))) =
        (translated_curve(cn, t), net.translated(t))
    else {
        return Ok(first);
    };
    let slack = (Ivl::from(curve_slack) + Ivl::from(net_slack)).hi;
    let moved_enough = (Ivl::from(enough) - Ivl::from(slack)).lo;
    let Some(moved) = run(&moved_curve, &moved_net, moved_enough)? else {
        return Ok(first);
    };
    let moved = (Ivl::from(moved) + Ivl::from(slack)).hi;
    if !(moved.is_finite() && moved <= over) {
        return Ok(first);
    }
    Ok(Some(first.map_or(moved, |g| g.min(moved))))
}

/// The middle `T` of the box of `cn`'s control points, where some coordinate of it is larger
/// than the box's diagonal (the curve lies farther from the origin than its size, so its
/// coordinates, not its shape, set the enclosures' width: [`recentred_retry`]); `None` near
/// the origin, or for data that are not finite or a weight that is not positive.
fn recentre(cn: &NurbsCurve) -> Option<[f64; 3]> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in cn.pw.chunks_exact(4) {
        let [x, y, z, w] = [*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?];
        if !(w.is_finite() && w > 0.0) {
            return None;
        }
        for (i, c) in [x, y, z].into_iter().enumerate() {
            let c = c / w;
            if !c.is_finite() {
                return None;
            }
            let (l, h) = (lo.get_mut(i)?, hi.get_mut(i)?);
            *l = l.min(c);
            *h = h.max(c);
        }
    }
    let [l0, l1, l2] = lo;
    let [h0, h1, h2] = hi;
    let t = [0.5 * (l0 + h0), 0.5 * (l1 + h1), 0.5 * (l2 + h2)];
    let size = ((h0 - l0).powi(2) + (h1 - l1).powi(2) + (h2 - l2).powi(2)).sqrt();
    let far = t.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    (far.is_finite() && size.is_finite() && far > size).then_some(t)
}

/// The homogeneous control points `pw` (`(x w, y w, z w, w)` each) moved by `-t`, rounded,
/// with a slack no moved point is farther than from the exact moved point `x - t`.
///
/// Each moved coordinate is `X' = fl(X - fl(w t))`, so `|X' - (X - w t)| <= u |w t| +
/// u |X'| / (1 - u)` (`u` the unit roundoff), and the moved point's coordinate `X' / w` is
/// within `u (|t| + |X'| / w / (1 - u))` of `x - t`: bounded by `(|t| + fl(|X'| / w)) * 2u`
/// (twice `u` covers the quotient's rounding and the `1 / (1 - u)`), the three coordinates'
/// norm rounded up. `None` for data that are not finite or a weight that is not positive.
fn translated_pw(pw: &[f64], t: [f64; 3]) -> Option<(Vec<f64>, f64)> {
    let mut out = Vec::new();
    let mut slack = 0.0_f64;
    for p in pw.chunks_exact(4) {
        let [x, y, z, w] = [*p.first()?, *p.get(1)?, *p.get(2)?, *p.get(3)?];
        if !(w.is_finite() && w > 0.0) {
            return None;
        }
        let mut sq = Ivl::from(0.0);
        for (c, tc) in [x, y, z].into_iter().zip(t) {
            let moved = c - w * tc;
            if !moved.is_finite() {
                return None;
            }
            out.push(moved);
            let e = (Ivl::from(tc.abs()) + Ivl::from(moved.abs()) / Ivl::from(w))
                * Ivl::from(f64::EPSILON);
            sq = sq + e * e;
        }
        out.push(w);
        slack = slack.max(sq.sqrt().hi);
    }
    (out.len() == pw.len() && slack.is_finite()).then_some((out, slack))
}

/// `cn` moved by `-t` ([`translated_pw`]), with its slack.
fn translated_curve(cn: &NurbsCurve, t: [f64; 3]) -> Option<(NurbsCurve, f64)> {
    let (pw, slack) = translated_pw(&cn.pw, t)?;
    Some((
        NurbsCurve {
            p: cn.p,
            n: cn.n,
            u: cn.u.clone(),
            pw,
        },
        slack,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// A line `lift` above a box's top face, inside it: its gap to the top face's plane is
    /// `lift` everywhere, so the bound is at least `lift` and, the line and the plane being
    /// polynomial, no more than rounding above it; the faces it is far from bound it far.
    #[test]
    fn a_line_above_a_face_is_bounded_by_its_height() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let lift = 6.8e-5;
        let z = 3.0 + lift;
        let line = NurbCurve::new(
            vec![CPoint::new(0.2, 0.3, z, NOW), CPoint::new(0.7, 1.6, z, NOW)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let line = BSplineCurve::from_nurb(line, 3, false);
        let mut bounds = Vec::new();
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            if let Some(g) = curve_face_gap_bound(&k, &b, f, &line, [0.0, 1.0], 1e-7).unwrap() {
                bounds.push(g);
            }
        }
        let nearest = bounds.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            nearest >= lift && nearest <= lift * (1.0 + 1e-6),
            "{nearest:e} against {lift:e}: {bounds:?}"
        );
        // an empty or reversed range encloses nothing
        let top = crate::query::api_queries::get_faces(&b).unwrap();
        assert!(top.iter().all(|f| {
            curve_face_gap_bound(&k, &b, *f, &line, [0.5, 0.5], 1e-7)
                .unwrap()
                .is_none()
        }));
    }

    /// A cylinder face's bound counts the cone's distance off its net (#20643 review): each
    /// curve is the face net's image of an axial line at `e` before the `u` end, so its gap
    /// to the net is rounding, and the face's point there is the formula's, off it by up to
    /// the residual ([`net_to_surface_bound`]). The bound holds the formula point's distance
    /// from the line and the residual. (Within the seam snap, about 1e-7 before the end, the
    /// curve's nearest points jump across the seam and no bound is given.)
    #[test]
    fn a_cylinder_faces_gap_bound_counts_its_net_residual() {
        use crate::surface::surface::SurfaceKind;
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 12.5, 3.0)
            .unwrap();
        let mut checked = 0;
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            let rec = b.face(f).unwrap();
            let Some(s) = rec.surface.as_ref() else {
                continue;
            };
            let s = s.clone_for_evaluation().unwrap();
            let SurfaceKind::Cone(c) = s.kind() else {
                continue;
            };
            let d = &rec.uv_domain;
            let (v0, v1) = (d.v_interval().min(), d.v_interval().max());
            for e in [1e-2, 1e-3, 3e-4, 1e-4, 1e-5, 1e-6, 3e-7, 1.5e-7] {
                let u = d.u_interval().max() - e;
                let n0 = c.rev.base.evaluate_point(Point2::new(u, v0)).unwrap();
                let n1 = c.rev.base.evaluate_point(Point2::new(u, v1)).unwrap();
                let line = NurbCurve::new(
                    vec![
                        CPoint::new(n0.x, n0.y, n0.z, NOW),
                        CPoint::new(n1.x, n1.y, n1.z, NOW),
                    ],
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let line = BSplineCurve::from_nurb(line, 3, false);
                let axis = (n1 - n0) * (1.0 / (n1 - n0).length());
                let p = s
                    .evaluate_point(&k, Point2::new(u, 0.5 * (v0 + v1)))
                    .unwrap();
                let off = p - n0;
                let across = (off - axis * off.dot(axis)).length();
                let g = curve_face_gap_bound(&k, &b, f, &line, [0.0, 1.0], 1e-14)
                    .unwrap()
                    .unwrap();
                assert!(g >= across, "{e:e}: {g:e} under {across:e}");
                let (net, w) = correspondence(&k, &s, d, &line, [0.0, 1.0])
                    .unwrap()
                    .unwrap();
                let residual = cone_net_offset(&k, &s, &net, &w).unwrap().unwrap();
                assert!(
                    g >= residual,
                    "{e:e}: {g:e} under the residual {residual:e}"
                );
                // (the residual is the line's box's: the seam's tear only near the seam)
                let whole = net_to_surface_bound(&k, &s, &net.net).unwrap().unwrap();
                assert!(residual <= whole, "{e:e}: {residual:e} over {whole:e}");
                if e >= 1e-4 {
                    assert!(
                        residual * 100.0 < whole,
                        "{e:e}: {residual:e} not local against {whole:e}"
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 1);
    }

    /// A trim's bound counts its cone's distance off the net over the trim's UV box only
    /// (#20643): on a full-turn cylinder of radius 12.5 the net is torn from the formula by
    /// up to 1.05e-7 at the seam (`Ellipse::step_inversion`'s snap), which a trim along an
    /// axial line halfway round never reaches. Its edge is the net's image of that line, so
    /// the trim's bound is rounding and the residual over its box; with the whole net's
    /// residual counted it carried the seam's tear (above 1e-8) and the stitch and kept-trim
    /// covers raised edges far from any seam by it.
    #[test]
    fn a_trim_away_from_the_seam_carries_no_seam_tear() {
        use crate::surface::surface::SurfaceKind;
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 12.5, 3.0)
            .unwrap();
        let mut checked = 0;
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            let rec = b.face(f).unwrap();
            let Some(s) = rec.surface.as_ref() else {
                continue;
            };
            let mut s = s.clone_for_evaluation().unwrap();
            let _ = s.set_owner(None);
            let SurfaceKind::Cone(c) = s.kind() else {
                continue;
            };
            let d = &rec.uv_domain;
            let (u0, u1) = (d.u_interval().min(), d.u_interval().max());
            let (v0, v1) = (d.v_interval().min(), d.v_interval().max());
            let net = NetSurface::of(&k, &s, &[]).unwrap().unwrap();
            let whole = bisection_residual(&k, &s, &net, None).unwrap().unwrap();
            assert!(whole > 1e-8, "the seam tear {whole:e}");
            for at in [0.25, 0.5, 0.75] {
                let u = u0 + (u1 - u0) * at;
                let n0 = c.rev.base.evaluate_point(Point2::new(u, v0)).unwrap();
                let n1 = c.rev.base.evaluate_point(Point2::new(u, v1)).unwrap();
                let line = NurbCurve::new(
                    vec![
                        CPoint::new(n0.x, n0.y, n0.z, NOW),
                        CPoint::new(n1.x, n1.y, n1.z, NOW),
                    ],
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let line = Curve::from(BSplineCurve::from_nurb(line, 3, false));
                let trim = NurbCurve::new(
                    vec![CPoint::new(u, v0, 0.0, 1.0), CPoint::new(u, v1, 0.0, 1.0)],
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                )
                .unwrap();
                let trim = Curve::from(BSplineCurve::from_nurb(trim, 2, false));
                let (wn, _) = exact_curve_data(&trim, true).unwrap().unwrap();
                let local = bisection_residual(&k, &s, &net, nurbs_uv_box(&wn))
                    .unwrap()
                    .unwrap();
                assert!(local * 100.0 < whole, "{at}: {local:e} against {whole:e}");
                let g = trim_image_gap_bound(&k, &s, &line, &trim, [0.0, 1.0], 1e-14)
                    .unwrap()
                    .unwrap();
                assert!(g >= local, "{at}: {g:e} under the residual {local:e}");
                assert!(g < 1e-9, "{at}: {g:e} carries the seam tear {whole:e}");
                let at_once =
                    structured_trim_image_gap_bound(&k, &s, &line, &trim, [0.0, 1.0]).unwrap();
                if let Some(x) = at_once {
                    assert!(x >= local && x < 1e-9, "{at}: structured {x:e}");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 1);
    }

    /// A curve bowing within a plane parallel to a box's face is bounded against the face
    /// by its height to within a thousandth, whatever the bow, by the bound that drops every
    /// piece's ends afresh (#18166); one asked whether it is within a hundredth past the
    /// height is shown within it, and a bound past `over` is infinite.
    #[test]
    fn a_curve_bowing_over_a_face_is_bounded_by_its_height_as_the_pieces_shrink() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let lift = 6.8e-5;
        let z = 3.0 + lift;
        let bow = NurbCurve::new(
            vec![
                CPoint::new(0.2, 0.3, z, NOW),
                CPoint::new(0.9, 0.5, z, NOW),
                CPoint::new(0.3, 1.7, z, NOW),
            ],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let bow = BSplineCurve::from_nurb(bow, 3, false);
        let faces = crate::query::api_queries::get_faces(&b).unwrap();
        let run = |f, within: [f64; 2]| {
            curve_face_gap_bound_within(&k, &b, f, &bow, [0.0, 1.0], 1e-9, within).unwrap()
        };
        let tight: Vec<f64> = faces
            .iter()
            .filter_map(|&f| run(f, [f64::NEG_INFINITY, f64::INFINITY]))
            .collect();
        let nearest = tight.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            nearest >= lift && nearest <= lift * (1.0 + 1e-3),
            "{nearest:e} against {lift:e}: {tight:?}"
        );
        let top = faces
            .iter()
            .copied()
            .find(|&f| run(f, [f64::NEG_INFINITY, f64::INFINITY]) == Some(nearest))
            .unwrap();
        let within = run(top, [1.01 * lift, f64::INFINITY]);
        assert!(
            within.is_some_and(|g| g >= lift && g <= 1.01 * lift),
            "{within:?}"
        );
        assert_eq!(run(top, [0.5 * lift, 0.5 * lift]), Some(f64::INFINITY));
        // an empty range encloses nothing
        assert!(curve_face_gap_bound_within(
            &k,
            &b,
            top,
            &bow,
            [0.5, 0.5],
            1e-9,
            [f64::NEG_INFINITY, f64::INFINITY]
        )
        .unwrap()
        .is_none());
    }

    /// The top face of a box (its plane `surface`, its UV domain mapped to `[0, 1]^2` by
    /// `uv`), its unit normal, and a curve of `cps` control points (degree `p`, knots
    /// `knots`, each point `at(f)` of fractions `f` of the domain, lifted `lift` along the
    /// normal) as a B-spline of dimension `dim`.
    struct Top {
        k: Kernel,
        surface: Surface,
        lo: Point2,
        size: Point2,
        normal: Point3,
    }

    impl Top {
        fn new() -> Self {
            let mut k = Kernel::default();
            let b =
                crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                    .unwrap();
            for f in crate::query::api_queries::get_faces(&b).unwrap() {
                let rec = b.face(f).unwrap();
                let surface = rec.surface.clone().unwrap();
                let d = rec.uv_domain;
                let lo = Point2::new(d.min().x, d.min().y);
                let size = Point2::new(d.max().x - lo.x, d.max().y - lo.y);
                let at = |x: f64, y: f64| {
                    surface
                        .evaluate_point(&k, Point2::new(lo.x + x * size.x, lo.y + y * size.y))
                        .unwrap()
                };
                let (o, a, c) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
                if (o.z - 3.0).abs() > 1e-12
                    || (a.z - 3.0).abs() > 1e-12
                    || (c.z - 3.0).abs() > 1e-12
                {
                    continue;
                }
                let n = (a - o).cross(c - o);
                let normal = n * (1.0 / n.length());
                return Self {
                    k,
                    surface,
                    lo,
                    size,
                    normal,
                };
            }
            panic!("no top face");
        }

        fn uv(&self, f: [f64; 2]) -> Point3 {
            Point3::new(
                self.lo.x + f[0] * self.size.x,
                self.lo.y + f[1] * self.size.y,
                0.0,
            )
        }

        fn point(&self, f: [f64; 2], lift: f64) -> Point3 {
            let q = self.uv(f);
            self.surface
                .evaluate_point(&self.k, Point2::new(q.x, q.y))
                .unwrap()
                + self.normal * lift
        }

        fn curve(points: &[Point3], p: usize, knots: Vec<f64>, two_d: bool) -> Curve {
            let cps = points
                .iter()
                .map(|q| CPoint::new(q.x, q.y, q.z, NOW))
                .collect();
            let c = NurbCurve::new(cps, p, knots).unwrap();
            let mut c = BSplineCurve::from_nurb(c, 3, false);
            if two_d {
                c.convert_to_2d().unwrap();
            }
            Curve::from(c)
        }

        fn bound(&self, curve: &Curve, trim: &Curve) -> Option<f64> {
            trim_image_gap_bound(&self.k, &self.surface, curve, trim, [0.0, 1.0], 1e-9).unwrap()
        }
    }

    /// [`clamped_trim_gap_bound`] of a line `lift` above the plane patch over `[0, 1]` and
    /// its shadow as a trim over `[0.25, 1]` only (evaluated clamped before 0.25): the
    /// unclamped bound encloses nothing past the trim's domain; the clamped one holds the true
    /// distance of `C(0)` from the image, `sqrt((L / 4)^2 + lift^2)` (`L` the line's length),
    /// and is at most `lift` plus the sliver's `L / 4`, within the bisection's closing fraction.
    #[test]
    fn a_trim_clamped_inside_the_range_is_bounded() {
        let t = Top::new();
        let lift = 1.0 / 4096.0;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let mid = [0.325, 0.375];
        let (pa, pb) = (t.point(a, lift), t.point(b, lift));
        let curve = Top::curve(&[pa, pb], 1, vec![0.0, 0.0, 1.0, 1.0], false);
        let trim = Top::curve(&[t.uv(mid), t.uv(b)], 1, vec![0.25, 0.25, 1.0, 1.0], true);
        let unclamped =
            trim_image_gap_bound(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9).unwrap();
        assert!(unclamped.is_none());
        let g = clamped_trim_gap_bound(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        let quarter = 0.25 * pa.distance(pb);
        let truth = (quarter * quarter + lift * lift).sqrt();
        assert!(g >= truth, "{g:e} against {truth:e}");
        assert!(
            g <= (lift + quarter) * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {:e}",
            lift + quarter
        );
        // a trim whose domain holds the range is not this case
        let whole = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        assert!(
            clamped_trim_gap_bound(&t.k, &t.surface, &curve, &whole, [0.0, 1.0], 1e-9)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn trim_subdivision_reuses_certified_endpoints() {
        let t = Top::new();
        let lift = 1.0 / 4096.0;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, knots.clone(), true);
        let curve = Top::curve(&[t.point(a, lift), t.point(b, 2.0 * lift)], 1, knots, false);
        let net = NetSurface::of(&t.k, &t.surface, &[]).unwrap().unwrap();
        let (cn, _) = exact_curve_data(&curve, false).unwrap().unwrap();
        let (wn, _) = exact_curve_data(&trim, true).unwrap().unwrap();
        let range = [0.0, 1.0];
        let psi = hermite_psi(&[(0.0, 0.0), (1.0, 1.0)], range).unwrap();
        let sides = Sides {
            curve: &cn,
            trim: &wn,
            psi: &psi,
            curve_kinks: vec![],
            trim_kinks: vec![],
            range,
        };
        let mut endpoints = BTreeMap::new();
        let mut evaluations = 0;
        for [a, b] in [[0.0, 1.0], [0.0, 0.5], [0.5, 1.0], [0.0, 0.5]] {
            let direct = piece_reparametrised(&sides, &net, a, b, |at| {
                reparametrised_endpoint(&sides, &net, at)
            })
            .unwrap()
            .unwrap();
            let reused = piece_reparametrised(&sides, &net, a, b, |at| {
                cached_trim_endpoint(&mut endpoints, at, || {
                    evaluations += 1;
                    reparametrised_endpoint(&sides, &net, at)
                })
            })
            .unwrap()
            .unwrap();
            assert!(reused.ends >= (1.0 + b) * lift);
            let bits = |p: PieceBound| [p.ends, p.term, p.slack, p.with_slack].map(f64::to_bits);
            assert_eq!(bits(direct), bits(reused));
        }
        // The parent, two children and a repeated visit use only three distinct endpoints.
        assert_eq!(evaluations, 3);
    }

    /// A trim of the edge curve lifted `lift` off the plane, the curve parameterised at another
    /// speed than the trim (a quadratic with its middle point off centre along the line): the
    /// same-parameter gap counts that slip (here about a tenth of the line), but the trim's image
    /// lies `lift` from the curve, so the bound is at least `lift` (it is an upper bound of a
    /// distance at least `lift` at the ends) and within twice the bisection's closing fraction
    /// (`CLOSE_RELATIVE`) above it.
    #[test]
    fn a_trim_is_bounded_by_its_distance_not_its_parameter_slip() {
        let t = Top::new();
        let lift = 3.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let mid = [0.2 + 0.7 * 0.5, 0.3 + 0.7 * 0.3];
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let curve = Top::curve(
            &[t.point(a, lift), t.point(mid, lift), t.point(b, lift)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            false,
        );
        let slip = (t.point([0.45, 0.45], 0.0) - curve.evaluate_point(&t.k, 0.5).unwrap()).length();
        assert!(slip > 100.0 * lift, "{slip:e}");
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {lift:e}"
        );
    }

    /// WR-450 (#18923): the slipping trim of the test above, and the same trim with the curve's
    /// middle point at the line's centre (no slip). The same-parameter bound counts the slip
    /// (it is at least the gap at `t = 0.5`, an outward-rounded witness) where the Hausdorff
    /// bound does not, and with `over` below it the bound is turned away as infinite; without
    /// slip it is within the bisection's closing fraction of `lift`, as the Hausdorff bound.
    #[test]
    fn a_same_parameter_bound_counts_the_slip() {
        let t = Top::new();
        let lift = 3.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let quadratic = |mid: [f64; 2]| {
            Top::curve(
                &[t.point(a, lift), t.point(mid, lift), t.point(b, lift)],
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                false,
            )
        };
        let same = |curve: &Curve, over: f64| {
            trim_same_parameter_gap_bound_within(
                &t.k,
                &t.surface,
                curve,
                &trim,
                [0.0, 1.0],
                1e-9,
                [f64::NEG_INFINITY, over],
                None,
            )
            .unwrap()
        };
        let slipping = quadratic([0.2 + 0.7 * 0.5, 0.3 + 0.7 * 0.3]);
        let w = trim.evaluate_point(&t.k, 0.5).unwrap();
        let at_half = (t
            .surface
            .evaluate_point(&t.k, Point2::new(w.x, w.y))
            .unwrap()
            - slipping.evaluate_point(&t.k, 0.5).unwrap())
        .length();
        assert!(at_half > 100.0 * lift, "{at_half:e}");
        let g = same(&slipping, f64::INFINITY).unwrap();
        assert!(g >= at_half, "{g:e} against {at_half:e}");
        assert!(t.bound(&slipping, &trim).unwrap() < 2.0 * lift);
        assert_eq!(same(&slipping, 10.0 * lift), Some(f64::INFINITY));
        let even = quadratic([0.45, 0.45]);
        let g = same(&even, f64::INFINITY).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {lift:e}"
        );
    }

    /// A trim bowed off its curve (a quadratic whose middle point is pushed across the line in
    /// UV): the bound must cover the image's real distance from the curve, sampled here (a
    /// lower bound of the true maximum), so a trim off its edge is never certified close.
    #[test]
    fn a_trim_off_its_curve_is_not_bounded_close() {
        let t = Top::new();
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let bow = [0.45 - 0.03, 0.45 + 0.05];
        let trim = Top::curve(
            &[t.uv(a), t.uv(bow), t.uv(b)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, 0.0), t.point(b, 0.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let (pa, pb) = (t.point(a, 0.0), t.point(b, 0.0));
        let dir = (pb - pa) * (1.0 / (pb - pa).length());
        let mut seen = 0.0_f64;
        for i in 0..=1000 {
            let w = trim.evaluate_point(&t.k, f64::from(i) / 1000.0).unwrap();
            let q = t
                .surface
                .evaluate_point(&t.k, Point2::new(w.x, w.y))
                .unwrap();
            let off = q - pa;
            seen = seen.max((off - dir * off.dot(dir)).length());
        }
        assert!(seen > 1e-2, "{seen:e}");
        let g = t.bound(&curve, &trim).unwrap();
        assert!(g >= seen && g <= 2.0 * seen, "{g:e} against {seen:e}");
    }

    /// A trim that first steps back by a nanometre over a sub-micron knot span, then runs along
    /// its curve, `lift` off it (#18166, fillet1019's face 0): the nearest parameters on that
    /// span all clamp to the start, so the cubic `ψ` has a zero slope at the span's end, and the
    /// gap after it, whose middle sits on the chord of its ends, lags the image by up to 4/27 of
    /// its width along the curve (some 2e-4 here). `ψ` is checked against the image at each
    /// gap's middle, so the bound stays within the bisection's closing fraction of `lift`.
    #[test]
    fn a_trim_that_turns_back_over_a_tiny_span_is_bounded_by_its_distance() {
        let t = Top::new();
        let lift = 1.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let eps = 5.0e-7;
        let back = [a[0] - 1.0e-9 * 0.5, a[1] - 1.0e-9 * 0.3];
        let trim = Top::curve(
            &[t.uv(a), t.uv(back), t.uv(b)],
            1,
            vec![0.0, 0.0, eps, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, lift), t.point(b, lift)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE) + 1.0e-8,
            "{g:e} against {lift:e}"
        );
    }

    /// A trim `lift` off its curve whose image stops `short` of the curve's end along it: near
    /// the end the image's nearest curve point is short of the end, while `ψ(hi) = hi` holds
    /// `ψ` to it. The two curves are within `sqrt(lift^2 + short^2)` of each other (the image's
    /// end against the curve's), so the bound is at least that and within the bisection's
    /// closing fraction of it: `ψ` is refined only where it bows `C(ψ)` off the image, not
    /// toward the pinned end, which would make it ever steeper there.
    #[test]
    fn a_trim_that_stops_short_of_its_curve_end_is_bounded_by_its_distance() {
        let t = Top::new();
        let (lift, short) = (1.0e-4, 1.0e-5);
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let (pa, pb) = (t.point(a, 0.0), t.point(b, 0.0));
        let f = 1.0 - short / (pb - pa).length();
        let end = [a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])];
        let trim = Top::curve(&[t.uv(a), t.uv(end)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let curve = Top::curve(
            &[t.point(a, lift), t.point(b, lift)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let d = (lift * lift + short * short).sqrt();
        let g = t.bound(&curve, &trim).unwrap();
        // (the curve's end is `d` from the image, which `d` holds up to the rounding of the
        // construction, well inside a part in 1e9; a bound missing the shortfall along the
        // curve would sit near `lift`, half a per cent below)
        assert!(
            g >= d * (1.0 - 1e-9) && g <= d * (1.0 + 2.0 * CLOSE_RELATIVE) + 1.0e-8,
            "{g:e} against {d:e}"
        );
    }

    /// A trim `h` off its curve where the curve starts with a short drop from near the trim's
    /// image to `h` below it: the image's nearest curve point jumps from the drop's top to the
    /// long stretch below, so the nearest parameters step there, however finely the trim is
    /// cut. `ψ` is not halved past what the bisection can split, so the bound stays near the
    /// distance between the two curves (at least `h`, at the ends) and is not held up by a step
    /// of `ψ` a few units in the last place wide (#18166, `single_face_fillet1019_8`).
    #[test]
    fn a_trim_whose_nearest_curve_point_jumps_is_bounded_by_its_distance() {
        let t = Top::new();
        let h = 2.0e-4;
        let (x0, x1, y) = (0.2, 0.7, 0.3);
        let trim = Top::curve(
            &[t.uv([x0, y + h]), t.uv([x1, y + h])],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[
                t.point([x0, y + 0.9 * h], 0.0),
                t.point([x0 + 1.0e-6, y], 0.0),
                t.point([x1, y], 0.0),
            ],
            1,
            vec![0.0, 0.0, 1.0e-3, 1.0, 1.0],
            false,
        );
        let d = t.point([x1, y + h], 0.0).distance(t.point([x1, y], 0.0));
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= d * (1.0 - 1.0e-9) && g <= 2.0 * d,
            "{g:e} against {d:e}"
        );
    }

    /// The bisection of `[0, 1]` (32 first pieces, floor `floor`) of pieces whose end values
    /// are `ends`, whose second term is `0.1 h²` and whose slack is `slack(a, h)`.
    fn bisect_with_slack(ends: f64, floor: f64, slack: impl Fn(f64, f64) -> f64) -> f64 {
        bisect(
            [0.0, 1.0],
            32,
            &[],
            CLOSE_RELATIVE,
            floor,
            [f64::NEG_INFINITY, f64::INFINITY],
            None,
            |a, b| {
                let h = b - a;
                Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), slack(a, h)))
            },
        )
        .unwrap()
        .unwrap()
    }

    /// `psi_at` is `ψ` where its enclosure is: inside [`psi_over`]'s enclosure of a point (to
    /// a few units in the last place), at the points it passes through and between them.
    #[test]
    fn psi_at_is_within_psi_over() {
        let points = [
            (0.0, 0.0),
            (0.1, 0.13),
            (0.25, 0.24),
            (0.25 + 1e-9, 0.2400001),
            (0.6, 0.55),
            (0.61, 0.62),
            (1.0, 1.0),
        ];
        let psi = hermite_psi(&points, [0.0, 1.0]).unwrap();
        for &(s, t) in &points {
            let at = psi_at(&psi, s, [0.0, 1.0]).unwrap();
            assert!(
                (at - t).abs() <= 4.0 * f64::EPSILON,
                "{s}: {at} against {t}"
            );
        }
        for i in 0..=1000 {
            let s = f64::from(i) / 1000.0;
            let at = psi_at(&psi, s, [0.0, 1.0]).unwrap();
            let r = psi_over(&psi, Ivl::from(s), 0, [0.0, 1.0])
                .unwrap()
                .unwrap()[0];
            let pad = 4.0 * f64::EPSILON;
            assert!(
                r.lo - pad <= at && at <= r.hi + pad,
                "{s}: {at} not in {r:?}"
            );
        }
        assert_eq!(psi_at(&psi, -1e-9, [0.0, 1.0]), None);
        assert_eq!(psi_at(&psi, 1.0 + 1e-9, [0.0, 1.0]), None);
    }

    /// A budget the open pieces outlast leaves a gap just within `enough` not enclosed, where
    /// the whole budget bounds the pieces it stops as they stand (#18166); a budget the pieces
    /// close within gives the bound without one, bit for bit.
    #[test]
    fn a_budget_run_out_is_not_enclosed() {
        let run = |relative: f64, enough: f64, budget: Option<usize>| {
            bisect(
                [0.0, 1.0],
                32,
                &[],
                relative,
                0.0,
                [enough, f64::INFINITY],
                budget,
                |a, b| {
                    let h = b - a;
                    Ok(PieceBound::with(1.0, Ivl::from(0.1 * h * h), 0.0))
                },
            )
            .unwrap()
        };
        // (closing at 0.1 h² <= 5e-14 needs pieces of 7e-7: more than 1 << 11)
        let just_within = 1.0 + 1.0e-13;
        assert_eq!(run(0.0, just_within, Some(1 << 11)), None);
        assert!(run(0.0, just_within, None).is_some_and(|g| g >= 1.0));
        // (closing at 0.1 h² <= 1e-3 needs no cut of the 32 first pieces)
        assert_eq!(
            run(CLOSE_RELATIVE, f64::NEG_INFINITY, Some(1 << 11)).map(f64::to_bits),
            run(CLOSE_RELATIVE, f64::NEG_INFINITY, None).map(f64::to_bits)
        );
    }

    /// A piece whose end values are NaN (a non-finite control point) leaves the gap not
    /// enclosed, with or without a budget: its bound's interval sum is the whole line
    /// (`Ivl::checked`), so it is never dropped by the `max` of the closed pieces' bounds while
    /// the rest pass as the gap's bound (#18166).
    #[test]
    fn a_nan_piece_bound_is_not_enclosed() {
        for budget in [None, Some(64)] {
            let gap = bisect(
                [0.0, 1.0],
                32,
                &[],
                CLOSE_RELATIVE,
                0.0,
                [f64::NEG_INFINITY, f64::INFINITY],
                budget,
                |a, b| {
                    let h = b - a;
                    let ends = if (0.25..0.5).contains(&a) {
                        f64::NAN
                    } else {
                        1.0
                    };
                    Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), 0.0))
                },
            )
            .unwrap();
            assert_eq!(gap, None, "budget {budget:?}");
        }
    }

    /// With a budget, a gap whose end values pass `enough` is bounded with every piece, not the
    /// budget, whether the end value past it comes before or after the budget runs out (the
    /// budget only stops the search for a bound at most `enough`), a piece whose bound is
    /// within the largest end value closes as it stands, and an end value past `over` gives an
    /// infinite bound at once (#18166).
    #[test]
    fn a_budget_stops_only_the_search_below_enough() {
        // (end values 1 at the middle, 0.5 elsewhere; a term 0.1 h² closes at h of 1e-6)
        let piece = |a: f64, b: f64| {
            let h = b - a;
            let ends = if a <= 0.5 && 0.5 <= b { 1.0 } else { 0.5 };
            Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), 0.0))
        };
        let run = |[enough, over]: [f64; 2], budget: Option<usize>| {
            bisect(
                [0.0, 1.0],
                32,
                &[],
                0.0,
                1.0e-13,
                [enough, over],
                budget,
                piece,
            )
            .unwrap()
        };
        // (past `enough` = 0.9 at the middle: the budget of 64 does not stop it)
        let g = run([0.9, f64::INFINITY], Some(64));
        assert!(
            g.is_some_and(|g| (1.0..=1.0 + 1.0e-12).contains(&g)),
            "{g:?}"
        );
        // (the first pieces, within 1e-4 of `enough` = 0.5 + 1e-13, spend the budget before the
        // middle shows an end value past it: they are cut on after it, and close within it)
        let g = run([0.5 + 1.0e-13, f64::INFINITY], Some(64));
        assert!(
            g.is_some_and(|g| (1.0..=1.0 + 1.0e-12).contains(&g)),
            "{g:?}"
        );
        // (within `enough` = 1.0 + 1e-13 everywhere: the budget stops it)
        assert_eq!(run([1.0 + 1.0e-13, f64::INFINITY], Some(64)), None);
        // (an end value past `over`)
        assert_eq!(run([0.9, 0.95], Some(64)), Some(f64::INFINITY));
    }

    /// The look at the end values (#19889) changes no bound: over end-value profiles below,
    /// at and above `enough`, pieces left unbounded, cut first pieces (as at the knots of a
    /// reparametrisation), pieces left unbounded between known ends and budgets,
    /// [`bisect_with_ends`] gives [`bisect`]'s answer bit for
    /// bit, and where the budget stops a gap within `enough` on a thousand first pieces it
    /// bounds one of them, not all.
    #[test]
    fn looking_at_the_end_values_changes_no_bound() {
        let profiles: [fn(f64) -> Option<f64>; 6] = [
            |t| Some(0.5 + 0.25 * t),
            |t| Some(if (0.7..=0.71).contains(&t) { 1.0 } else { 0.5 }),
            |t| (!(0.3..0.4).contains(&t)).then_some(0.5),
            |t| (!(0.3..0.4).contains(&t)).then_some(if t > 0.9 { 2.0 } else { 0.5 }),
            |t| Some(0.75 - 1.0e-4 * (40.0 * t).sin()),
            |_| Some(0.75),
        ];
        let cuts: Vec<f64> = (1..1000).map(|i| f64::from(i) / 1000.0 + 1.0e-7).collect();
        for (n, profile) in profiles.iter().enumerate() {
            for corners in [&[][..], &cuts[..]] {
                for enough in [0.4, 0.75, 0.75 + 1.0e-13, 1.5, 3.0] {
                    for budget in [None, Some(64), Some(1 << 11)] {
                        for (over, lost) in [f64::INFINITY, 1.75]
                            .into_iter()
                            .flat_map(|over| [(over, false), (over, true)])
                        {
                            let mut calls = [0usize; 2];
                            let mut run = |with: bool| {
                                let calls = &mut calls[usize::from(with)];
                                let piece = |a: f64, b: f64| {
                                    *calls += 1;
                                    // (`lost`: pieces left unbounded with both ends known, as
                                    // a second term that cannot be bounded leaves them)
                                    if lost && (0.5..0.6).contains(&a) {
                                        return Ok(None);
                                    }
                                    let (Some(ga), Some(gb)) = (profile(a), profile(b)) else {
                                        return Ok(None);
                                    };
                                    let h = b - a;
                                    let ends = 0.0_f64.max(ga).max(gb);
                                    Ok(PieceBound::with(ends, Ivl::from(0.1 * h * h), 0.0))
                                };
                                let mut end = |t: f64| Ok(profile(t));
                                let ends: Option<&mut dyn FnMut(f64) -> Result<Option<f64>>> =
                                    if with { Some(&mut end) } else { None };
                                bisect_with_ends(
                                    [0.0, 1.0],
                                    32,
                                    corners,
                                    0.0,
                                    1.0e-13,
                                    [enough, over],
                                    budget,
                                    piece,
                                    ends,
                                )
                                .unwrap()
                                .map(f64::to_bits)
                            };
                            let (without, with) = (run(false), run(true));
                            let case = format!(
                                "profile {n}, {} cuts, enough {enough}, budget {budget:?}, \
                                 over {over}, lost {lost}",
                                corners.len()
                            );
                            assert_eq!(with, without, "{case}");
                            assert!(calls[1] <= calls[0], "{case}: {calls:?}");
                            // (a gap of 0.75 throughout, within `enough` = 0.75 + 1e-13 only by
                            // pieces far finer than a thousand first pieces)
                            if n == 5
                                && corners.len() == cuts.len()
                                && enough == 0.75 + 1.0e-13
                                && budget == Some(64)
                                && over == f64::INFINITY
                            {
                                assert_eq!(without, None, "{case}");
                                assert_eq!(calls[1], 1, "{case}: {calls:?}");
                                assert!(calls[0] > 1000, "{case}: {calls:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// A cancellation that arrives once the budget has parked the first piece cancels the
    /// look at the end values ([`polled_end`]), as it cancelled the pieces the look stands in
    /// for, instead of the bound finishing as "not within" (Ok(None)).
    #[test]
    fn a_cancellation_during_the_look_at_the_end_values_cancels_it() {
        use crate::kernel::CancelToken;
        let cuts: Vec<f64> = (1..1000).map(|i| f64::from(i) / 1000.0 + 1.0e-7).collect();
        for cancel in [false, true] {
            let mut k = Kernel::default();
            let token = CancelToken::new();
            k.set_cancel_token(token.clone());
            let k = k;
            let piece = |a: f64, b: f64| {
                k.check_cancelled(OP)?;
                // (the cancellation arrives right after the first piece's poll)
                if cancel {
                    token.cancel();
                }
                let h = b - a;
                Ok(PieceBound::with(0.75, Ivl::from(0.1 * h * h), 0.0))
            };
            let mut look = polled_end(&k, |_| Ok(Some(0.75)));
            let gap = bisect_with_ends(
                [0.0, 1.0],
                32,
                &cuts,
                0.0,
                1.0e-13,
                [0.75 + 1.0e-13, f64::INFINITY],
                Some(64),
                piece,
                Some(&mut look),
            );
            if cancel {
                assert!(matches!(gap, Err(SolidError::Cancelled { .. })), "{gap:?}");
            } else {
                assert_eq!(gap.unwrap(), None);
            }
        }
    }

    /// A slack no cut shrinks (a trim past its surface's net, #18166) does not hold a piece
    /// open: over half the range the pieces carry a slack above the closing threshold, and
    /// cutting them down to the deepest level would spend the pieces the other half needs,
    /// whose second-order term then stays at its first pieces' size (1e-4, against a bound of
    /// at most `ends + slack + floor`).
    #[test]
    fn a_slack_no_cut_shrinks_does_not_hold_the_bisection_open() {
        let (ends, slack, floor) = (1.0e-6, 1.0e-6, 1.0e-7);
        let g = bisect_with_slack(ends, floor, |a, _| if a < 0.5 { slack } else { 0.0 });
        assert!(g >= ends + slack && g <= ends + slack + floor, "{g:e}");
    }

    /// A slack that a cut shrinks (a box wider than the trim's own overshoot past the net,
    /// halved with the piece) is cut down as before: closing on the second term alone would
    /// stop with the slack still 1e-6 (at a piece of 1e-3), ten times the floor, where cutting
    /// on brings it below the floor.
    #[test]
    fn a_slack_a_cut_shrinks_is_cut_down() {
        let (ends, floor) = (1.0e-6, 1.0e-7);
        let g = bisect_with_slack(ends, floor, |a, h| if a < 0.5 { 1.0e-3 * h } else { 0.0 });
        assert!(g >= ends && g <= ends + floor, "{g:e}");
    }

    /// Newton's step toward the nearest parameter is `(target - C) . C' / |C'|^2`: on a stretch
    /// where the curve is slow (here its first three control points 1e-9 apart) it overshoots
    /// to a far part of the curve, and the steps after need not come back (#18166, `brep_mirror`
    /// `mat0094`, a `ψ` point 0.011 off along the curve). The parameter returned is never
    /// farther from the target than the guess.
    #[test]
    fn the_nearest_parameter_is_never_farther_than_its_guess() {
        let k = Kernel::default();
        let at = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let curve = Top::curve(
            &[at(0.0, 0.0), at(1.0e-9, 0.0), at(2.0e-9, 0.0), at(1.0, 1.0)],
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            false,
        );
        let target = at(1.0e-6, 1.0e-3);
        let near = |t: f64| curve.evaluate_point(&k, t).unwrap().distance(target);
        for guess in [0.0, 1.0e-4, 1.0e-3] {
            let (t, _) = nearest_param(&k, &curve, target, guess, [0.0, 1.0]).unwrap();
            assert!(
                near(t) <= near(guess),
                "{guess:e}: {t:e} at {:e}, guess at {:e}",
                near(t),
                near(guess)
            );
        }
    }

    /// The bowed trim of [`a_trim_off_its_curve_is_not_bounded_close`] asked only whether it is
    /// above `over`: at or above its full bound the answer is that bound, below it infinity.
    #[test]
    fn a_bound_up_to_a_cutoff_answers_as_the_full_bound() {
        let t = Top::new();
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let bow = [0.45 - 0.03, 0.45 + 0.05];
        let trim = Top::curve(
            &[t.uv(a), t.uv(bow), t.uv(b)],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            true,
        );
        let curve = Top::curve(
            &[t.point(a, 0.0), t.point(b, 0.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let up_to = |over: f64| {
            trim_image_gap_bound_up_to(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9, over)
                .unwrap()
                .unwrap()
        };
        let g = up_to(f64::INFINITY);
        assert_eq!(g, t.bound(&curve, &trim).unwrap());
        assert_eq!(up_to(g).to_bits(), g.to_bits());
        assert_eq!(up_to(2.0 * g).to_bits(), g.to_bits());
        assert_eq!(up_to(0.5 * g), f64::INFINITY);
    }

    /// A line edge whose point `P` is far from the piece, so its evaluation carries a slack
    /// (`line_evaluation_slack`), over a straight trim `lift` below it: asked whether the bound
    /// is above a cutoff between the bisected gap and the gap plus the slack, the answer is
    /// that it is (infinity), as the full bound says; at the full bound it is that bound.
    #[test]
    fn a_cutoff_counts_a_line_edges_slack() {
        use crate::curve::line::BoundedLineOptions;
        use crate::geom::extent1d::Extent1d;
        let t = Top::new();
        let lift = 2.0e-4;
        let (a, b) = ([0.2, 0.3], [0.7, 0.6]);
        let (pa, pb) = (t.point(a, lift), t.point(b, lift));
        let len = (pb - pa).length();
        let unit = (pb - pa) * (1.0 / len);
        let far = 1e6;
        let mut line = Line::bounded(
            pa - unit * far,
            unit,
            &Extent1d::from_min_max(far, far + len),
            &BoundedLineOptions::default(),
        );
        line.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))
            .unwrap();
        let slack = line_evaluation_slack(&line, false).unwrap();
        assert!(slack > 0.0, "{slack:e}");
        let curve = Curve::from(line);
        let trim = Top::curve(&[t.uv(a), t.uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let up_to = |over: f64| {
            trim_image_gap_bound_up_to(&t.k, &t.surface, &curve, &trim, [0.0, 1.0], 1e-9, over)
                .unwrap()
                .unwrap()
        };
        let g = up_to(f64::INFINITY);
        assert_eq!(g, t.bound(&curve, &trim).unwrap());
        assert!(
            g >= lift && slack > g - g.next_down(),
            "{g:e}, slack {slack:e}"
        );
        assert_eq!(up_to(g).to_bits(), g.to_bits());
        // (at most the gap plus a part of the slack: the bisection's gap is below it)
        assert_eq!(up_to(g - 0.5 * slack), f64::INFINITY);
        assert_eq!(up_to(g.next_down()), f64::INFINITY);
    }

    /// A curve with a corner (a degree-1 polyline with an interior knot of full multiplicity)
    /// and its trim with the same corner, lifted `lift`: the corner is bounded by the Lipschitz
    /// term, which still closes, so the bound stays at least `lift` and within twice the
    /// bisection's closing fraction above it.
    #[test]
    fn a_trim_with_a_corner_is_bounded() {
        let t = Top::new();
        let lift = 2.0e-4;
        let pts = [[0.2, 0.3], [0.5, 0.7], [0.8, 0.35]];
        let knots = vec![0.0, 0.0, 0.5, 1.0, 1.0];
        let trim = Top::curve(&pts.map(|f| t.uv(f)), 1, knots.clone(), true);
        let curve = Top::curve(&pts.map(|f| t.point(f, lift)), 1, knots, false);
        let g = t.bound(&curve, &trim).unwrap();
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE),
            "{g:e} against {lift:e}"
        );
        // a range past the curve's domain encloses nothing
        let past = trim_image_gap_bound(&t.k, &t.surface, &curve, &trim, [0.0, 1.5], 1e-9).unwrap();
        assert!(past.is_none());
    }

    /// [`curve_uv_gap_bound`] of a line `lift` above the bilinear plane patch z = 0 (x, y
    /// in 0..1), at the UV line under it with the same parameters: at least `lift` and no
    /// more than rounding above it. The same line read at the UV line's reversed parameters
    /// (the same image, other points) is bounded by its true gap, which at either end is
    /// the shadow's whole length (about 0.5831); an empty range encloses nothing. A corner of
    /// the curve inside a first piece (a spike to 1e-3 between two ends on the plane) is
    /// seen: the pieces are cut there.
    #[test]
    fn a_line_over_its_uv_line_is_bounded_by_its_height() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        let k = Kernel::default();
        let plane = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let surface = Surface::from(plane);
        let line = |a: [f64; 3], b: [f64; 3]| {
            let c = NurbCurve::new(
                vec![
                    CPoint::new(a[0], a[1], a[2], NOW),
                    CPoint::new(b[0], b[1], b[2], NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineCurve::from_nurb(c, 3, false)
        };
        let lift = 4.2e-7;
        let curve = line([0.2, 0.3, lift], [0.7, 0.6, lift]);
        let uv = line([0.2, 0.3, 0.0], [0.7, 0.6, 0.0]);
        let g = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        assert!(g >= lift && g <= lift * (1.0 + 1e-6), "{g:e}");
        let reversed = line([0.7, 0.6, 0.0], [0.2, 0.3, 0.0]);
        let g = curve_uv_gap_bound(&k, &curve, &reversed, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        // (at either end the curve is the shadow's length from its image: the whole gap, not
        // a part of it; its rounding is far below the 1e-12 allowed)
        let full = (0.5_f64 * 0.5 + 0.3 * 0.3 + lift * lift).sqrt();
        assert!(g >= full * (1.0 - 1e-12), "{g:e} against {full:e}");
        let empty = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.5, 0.5], 1e-9).unwrap();
        assert_eq!(empty, None);
        // (a corner of the curve inside a first piece, `[0, 1/32]`: the line over its UV
        // line but for a spike to 1e-3 at t = 1/64, back at t = 1/32; the piece's ends and
        // the spans' second derivatives are all 0, so only a cut at the corner sees it)
        let at = |t: f64| CPoint::new(0.2 + 0.5 * t, 0.3 + 0.3 * t, 0.0, NOW);
        let mut spike = at(1.0 / 64.0);
        spike.z = 1e-3;
        let c = NurbCurve::new(
            vec![at(0.0), spike, at(1.0 / 32.0), at(1.0)],
            1,
            vec![0.0, 0.0, 1.0 / 64.0, 1.0 / 32.0, 1.0, 1.0],
        )
        .unwrap();
        let spiked = BSplineCurve::from_nurb(c, 3, false);
        let g = curve_uv_gap_bound(&k, &spiked, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        assert!(g >= 1e-3, "{g:e}");
        // (a cancelled kernel stops the bisection: cancellation, not an unenclosed gap)
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let stopped = curve_uv_gap_bound(&cancelled, &curve, &uv, &surface, [0.0, 1.0], 1e-9);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
    }

    /// A half-unit piece of a line whose point `P` is `1e8` away: the piece lies near the origin,
    /// so its NURB's control points are small, but the analytic point `P + s V` rounds at the
    /// scale of `P` (its STEP parameter `s` is near `-1e8`), about `1e-8`. The slack is at
    /// least the measured distance between the analytic point and its NURB's point at every
    /// sampled parameter (this shows the measured gap only; the bound itself is derived), and
    /// a bound in the size of the control points alone (`γ_32` of `8 m`) falls short of it.
    #[test]
    fn a_line_piece_far_from_its_point_has_a_slack_at_the_point_scale() {
        use crate::curve::line::BoundedLineOptions;
        use crate::geom::extent1d::Extent1d;
        use crate::math::Vec3;
        let far = 1e8;
        for (dir, lo) in [
            (Vec3::new(1.0, 0.0, 0.0), -far + 0.25),
            (Vec3::new(3.0, 4.0, 12.0), -far + 0.1),
        ] {
            let unit = dir / dir.length();
            let p = Point3::new(far * unit.x, far * unit.y, far * unit.z);
            let mut line = Line::bounded(
                p,
                dir,
                &Extent1d::from_min_max(lo, lo + 0.5),
                &BoundedLineOptions::default(),
            );
            // (its NURB over `[0, 1]`: the STEP parameter is then computed from it, rounding
            // at the scale of `P`; over the STEP interval itself the NURB parameter carries
            // the same rounding and the two agree)
            line.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))
                .unwrap();
            let slack = line_evaluation_slack(&line, false).unwrap();
            let (cn, curve_slack) = exact_curve_data(&Curve::from(line.clone()), false)
                .unwrap()
                .unwrap();
            assert_eq!(slack.to_bits(), curve_slack.to_bits());
            let (t0, t1) = (cn.u[1], cn.u[2]);
            let q = |i: usize| [cn.pw[4 * i], cn.pw[4 * i + 1], cn.pw[4 * i + 2]];
            let (q0, q1) = (q(0), q(1));
            let m = q0.iter().chain(&q1).fold(0.0_f64, |a, x| a.max(x.abs()));
            assert!(m < 1.0, "the piece is near the origin: {m:e}");
            let old = crate::nurbs::math::gamma(32) * 8.0 * m;
            let mut measured = 0.0_f64;
            for i in 0..=1000 {
                let t = t0 + (t1 - t0) * f64::from(i) / 1000.0;
                let lam = (t - t0) / (t1 - t0);
                let a = line.evaluate_point(t);
                let d = [a.x, a.y, a.z]
                    .iter()
                    .zip(q0.iter().zip(&q1))
                    .map(|(a, (x0, x1))| a - ((1.0 - lam) * x0 + lam * x1))
                    .fold(0.0_f64, |s, d| s + d * d)
                    .sqrt();
                measured = measured.max(d);
            }
            assert!(measured > old, "{measured:e} against the old {old:e}");
            assert!(slack >= measured, "{slack:e} against {measured:e}");
            // and of the order of the point's rounding, not far beyond it
            assert!(slack < 1e-6, "{slack:e}");
        }
    }

    /// [`curve_uv_gap_lower_bound`] of a line `lift` above the bilinear plane patch z = 0 (x,
    /// y in 0..1) at its UV line: at every parameter the gap is exactly `lift`, and the lower
    /// bound is at most that (and at most [`curve_uv_gap_bound`]'s upper bound) and below it
    /// by no more than rounding, a UV point on the net's end (`u = 1`) included. Read at the
    /// reversed UV line, at `t = 0` the gap is the shadow's whole length. Nothing is enclosed
    /// at a parameter outside the domain or not finite, nor where the UV line runs far past
    /// the net's end.
    #[test]
    fn a_line_over_its_uv_line_has_its_height_as_lower_bound() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        let k = Kernel::default();
        let plane = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &[
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0; 4]),
            uv_domain: None,
        })
        .unwrap();
        let surface = Surface::from(plane);
        let line = |a: [f64; 3], b: [f64; 3]| {
            let c = NurbCurve::new(
                vec![
                    CPoint::new(a[0], a[1], a[2], NOW),
                    CPoint::new(b[0], b[1], b[2], NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineCurve::from_nurb(c, 3, false)
        };
        let lift = 4.2e-7;
        let lower = |c: &BSplineCurve, w: &BSplineCurve, t: f64| {
            curve_uv_gap_lower_bound(&k, c, w, &surface, t).unwrap()
        };
        // (to the net's end u = 1 at t = 1)
        let curve = line([0.2, 0.3, lift], [1.0, 0.6, lift]);
        let uv = line([0.2, 0.3, 0.0], [1.0, 0.6, 0.0]);
        let upper = curve_uv_gap_bound(&k, &curve, &uv, &surface, [0.0, 1.0], 1e-9)
            .unwrap()
            .unwrap();
        // the bound gives up rounding only: coordinates near 1 cost a few ulps of 1 (at
        // t = 1 the UV box rounds past the net's end, and the continuation's slack, about
        // 6e-15, is taken off)
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let lo = lower(&curve, &uv, t).unwrap_or_else(|| panic!("t {t}: not enclosed"));
            assert!(
                lo <= lift && lo <= upper && lo >= lift - 1e-13,
                "t {t}: {lo:e} against {lift:e} (upper {upper:e})"
            );
        }
        let reversed = line([1.0, 0.6, 0.0], [0.2, 0.3, 0.0]);
        let full = (0.8_f64 * 0.8 + 0.3 * 0.3 + lift * lift).sqrt();
        let lo = lower(&curve, &reversed, 0.0).unwrap();
        assert!(lo <= full && lo >= full * (1.0 - 1e-12), "{lo:e} {full:e}");
        for t in [-0.5, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(lower(&curve, &uv, t), None, "t {t}");
        }
        let far = line([0.2, 0.3, 0.0], [1.5, 0.3, 0.0]);
        let over = line([0.2, 0.3, lift], [1.5, 0.3, lift]);
        assert!(lower(&over, &far, 0.0).is_some());
        assert_eq!(lower(&over, &far, 1.0), None);
    }

    /// The top face (`z = 3`) of a 1 x 2 x 3 box: its surface, a plane with a net.
    fn top_plane(k: &mut Kernel) -> Surface {
        let b =
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
        for f in crate::query::api_queries::get_faces(&b).unwrap() {
            let s = crate::topology::face::p2::surface(&b, f)
                .unwrap()
                .unwrap()
                .clone();
            if let crate::surface::surface::p2::SurfaceKind::Plane(p) = s.kind() {
                let uv = p.project_point_to_uv_domain(Point3::new(0.5, 1.0, 7.0));
                if (s.evaluate_point(k, uv).unwrap().z - 3.0).abs() < 1e-12 {
                    return s;
                }
            }
        }
        panic!("no top face");
    }

    /// The UV of `p` dropped onto the plane `s`.
    fn plane_uv(s: &Surface, p: Point3) -> Point2 {
        match s.kind() {
            crate::surface::surface::p2::SurfaceKind::Plane(pl) => pl.project_point_to_uv_domain(p),
            _ => panic!("not a plane"),
        }
    }

    /// A line piece `lift` above a plane, matched to it at its two feet: every point is
    /// exactly `lift` above it, the line and the plane are polynomial, so the bound is at
    /// least `lift` and no more than rounding above it, either way round the parameters run.
    #[test]
    fn a_line_piece_above_a_plane_is_bounded_by_its_height() {
        let mut k = Kernel::default();
        let s = top_plane(&mut k);
        let lift = 6.8e-5;
        let (a, b) = (
            Point3::new(0.2, 0.3, 3.0 + lift),
            Point3::new(0.7, 1.6, 3.0 + lift),
        );
        let line = Curve::from(
            Line::create_line_segment(
                3,
                a,
                b,
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let (ua, ub) = (plane_uv(&s, a), plane_uv(&s, b));
        for (t, uv) in [([0.0, 1.0], [ua, ub]), ([1.0, 0.0], [ub, ua])] {
            let g = curve_piece_gap_bound(&k, &line, t, PieceTarget::Surface(&s, uv), 0.0, 1.0)
                .unwrap()
                .unwrap();
            assert!(
                g >= lift && g <= lift * (1.0 + 1e-6),
                "{g:e} against {lift:e}"
            );
        }
        // the half piece matched to the wrong half of the plane: the witness is a bound
        // only, so the bound grows with the mismatch, never below the true gap
        let mid = plane_uv(&s, Point3::new(0.45, 0.95, 3.0));
        let g = curve_piece_gap_bound(
            &k,
            &line,
            [0.0, 1.0],
            PieceTarget::Surface(&s, [ua, mid]),
            0.0,
            1.0,
        )
        .unwrap()
        .unwrap();
        assert!(g >= lift, "{g:e}");
    }

    /// A segment against a point off its middle: the farthest point of the segment is an
    /// end, `sqrt(0.25 + d^2)` away (the segment's second derivative is zero, so the bound
    /// is the end values and rounding).
    #[test]
    fn a_segment_against_a_point_is_bounded_by_its_farther_end() {
        let k = Kernel::default();
        let line = Curve::from(
            Line::create_line_segment(
                3,
                Point3::ZERO,
                Point3::new(1.0, 0.0, 0.0),
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let d = 3.0e-3;
        let want = (0.25_f64 + d * d).sqrt();
        let g = curve_piece_gap_bound(
            &k,
            &line,
            [0.0, 1.0],
            PieceTarget::Point(Point3::new(0.5, d, 0.0)),
            0.0,
            10.0,
        )
        .unwrap()
        .unwrap();
        assert!(
            g >= want && g <= want * (1.0 + 1e-9),
            "{g:e} against {want:e}"
        );
    }

    /// An arc of radius `r` over `2 theta` against its chord: the arc's middle is the
    /// sagitta `r (1 - cos theta)` from the chord, and the bound, whatever witness map is
    /// used, is no smaller; at a limit below the sagitta the certificate does not accept.
    #[test]
    fn an_arc_against_its_chord_is_bounded_by_at_least_the_sagitta() {
        let k = Kernel::default();
        let (r, deg) = (2.0_f64, 10.0_f64);
        let arc = Curve::from(
            crate::primitive::api_curves::create_arc(Point3::ZERO, r, -deg, deg).unwrap(),
        );
        let base = NurbsCurve::of(arc.b_spline_base().unwrap()).unwrap();
        let (t0, t1) = (base.u[base.p], base.u[base.n]);
        let th = deg.to_radians();
        let (a, b) = (
            Point3::new(r * th.cos(), -r * th.sin(), 0.0),
            Point3::new(r * th.cos(), r * th.sin(), 0.0),
        );
        let chord = Curve::from(
            Line::create_line_segment(
                3,
                a,
                b,
                Some(&crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0)),
            )
            .unwrap(),
        );
        let sagitta = r * (1.0 - th.cos());
        let g = curve_piece_gap_bound(
            &k,
            &arc,
            [t0, t1],
            PieceTarget::Curve(&chord, [0.0, 1.0]),
            0.0,
            1.0,
        )
        .unwrap()
        .unwrap();
        assert!(
            g >= sagitta && g < 2.0 * sagitta,
            "{g:e} against {sagitta:e}"
        );
        let tol = 0.5 * sagitta;
        let g = curve_piece_gap_bound(
            &k,
            &arc,
            [t0, t1],
            PieceTarget::Curve(&chord, [0.0, 1.0]),
            0.01 * tol,
            tol,
        )
        .unwrap()
        .unwrap();
        assert!(g > tol, "{g:e} accepted at {tol:e}");
    }

    /// The near-tangent counterexample to a reach argued from the crossing angle: a curve
    /// that touches the plane at both ends, `z = 3 + k u (1 - u)(u^2 + eps)`, nearly
    /// tangent at `u = 0` (slope `k eps`), whose middle bows `k / 8`-ish off it. Both ends
    /// lie on the plane, any reach argued from them accepts the piece; the certificate
    /// bounds the bow and refuses at a tolerance below it.
    #[test]
    fn a_piece_on_the_plane_at_both_ends_that_bows_off_it_is_refused() {
        let mut k = Kernel::default();
        let s = top_plane(&mut k);
        let (kk, eps) = (8.0e-4_f64, 1.0e-6_f64);
        // z - 3 in Bernstein form of degree 4: (0, k eps / 4, k eps / 3, k (eps + 1) / 4, 0)
        let z = [
            0.0,
            kk * eps / 4.0,
            kk * eps / 3.0,
            kk * (eps + 1.0) / 4.0,
            0.0,
        ];
        let pts = (0..5)
            .map(|i| CPoint::new(0.2 + 0.5 * i as f64 / 4.0, 0.5, 3.0 + z[i], NOW))
            .collect();
        let bez = NurbCurve::new(
            pts,
            4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let bow = Curve::from(BSplineCurve::from_nurb(bez, 3, false));
        // the true largest gap, from the polynomial on a fine grid (a lower bound)
        let peak = (0..=1000)
            .map(|i| {
                let u = f64::from(i) / 1000.0;
                kk * u * (1.0 - u) * (u * u + eps)
            })
            .fold(0.0_f64, f64::max);
        let uv = [
            plane_uv(&s, Point3::new(0.2, 0.5, 3.0)),
            plane_uv(&s, Point3::new(0.7, 0.5, 3.0)),
        ];
        let tol = 0.5 * peak;
        let g = curve_piece_gap_bound(
            &k,
            &bow,
            [0.0, 1.0],
            PieceTarget::Surface(&s, uv),
            0.01 * tol,
            tol,
        )
        .unwrap()
        .unwrap();
        assert!(g > tol, "{g:e} accepted at {tol:e} (peak {peak:e})");
        let g = curve_piece_gap_bound(&k, &bow, [0.0, 1.0], PieceTarget::Surface(&s, uv), 0.0, 1.0)
            .unwrap()
            .unwrap();
        assert!(
            g >= peak && g <= peak * (1.0 + 1e-2),
            "{g:e} against {peak:e}"
        );
    }

    /// A cap circle: its planar face, its curve and the curve's interval.
    type CapCircle = (crate::topology::FaceId, BSplineCurve, [f64; 2]);

    /// A cylinder of radius 12.5 (a round tube's profile, #20373): each of its two cap
    /// circles (an ellipse's B-spline, with its interval) and its planar cap face.
    fn cap_circles() -> (Kernel, crate::topology::Brep, Vec<CapCircle>) {
        use crate::curve::curve::p2::CurveKind;
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 12.5, 50.0)
            .unwrap();
        let mut out = Vec::new();
        for e in crate::query::api_queries::get_edges(&b).unwrap() {
            let rec = b.edge(e).unwrap();
            let Some(CurveKind::Ellipse(el)) = rec.curve.as_ref().map(Curve::kind) else {
                continue;
            };
            for f in crate::topology::edge::faces(&b, e, None).unwrap() {
                let s = b.face(f).unwrap().surface.as_ref().unwrap();
                if matches!(s.kind(), crate::surface::surface::SurfaceKind::Plane(_)) {
                    let ivl = rec.interval;
                    out.push((f, el.base().clone_exact(), [ivl.min(), ivl.max()]));
                }
            }
        }
        (k, b, out)
    }

    /// `cn` rebuilt as a B-spline with each control point moved by `dz` along z.
    fn moved(cn: &NurbsCurve, dz: f64) -> BSplineCurve {
        let cps = cn
            .pw
            .chunks_exact(4)
            .map(|q| CPoint::new(q[0], q[1], (q[2] / q[3] + dz) * q[3], q[3]))
            .collect();
        let c = NurbCurve::new(cps, cn.p, cn.u.clone()).unwrap();
        BSplineCurve::from_nurb(c, 3, false)
    }

    /// #20373: a circle lying in its plane face is shown within `1e-5` by its control points
    /// alone. Before the plane bound the bisection cut each circle into about 8160 pieces (the
    /// straight map between two feet is a chord of the circle's image, its sagitta the
    /// second-order term), about 60 ms each in a release build.
    #[test]
    fn a_circle_in_its_plane_face_is_bounded_without_cutting_it() {
        let (k, b, caps) = cap_circles();
        assert_eq!(caps.len(), 2, "two cap circles, each on its cap");
        for (f, circle, range) in &caps {
            let within = [1e-5, f64::INFINITY];
            let (g, pieces) =
                curve_face_gap_bound_within_counted(&k, &b, *f, circle, *range, 1e-8, within)
                    .unwrap();
            assert!(g.is_some_and(|g| (0.0..=1e-5).contains(&g)), "{g:?}");
            assert!(pieces <= FIRST_PIECES, "{pieces} pieces");
            // the bisection alone shows the same: within `enough` (only the value differs,
            // and the contract leaves any value up to `enough`)
            let cut = Cell::new(0);
            let alone =
                bisected_face_gap_bound(&k, &b, *f, circle, *range, 1e-8, within, &cut).unwrap();
            assert!(alone.is_some_and(|g| g <= 1e-5), "{alone:?}");
            assert!(cut.get() > 1000, "{} pieces", cut.get());
        }
    }

    /// A circle `2e-5` off its cap's plane is `2e-5` from it everywhere: the plane bound is at
    /// least that, above `enough = 1e-5`, so the bisection decides, exactly as without the
    /// plane bound, and the gap is still caught.
    #[test]
    fn a_circle_off_its_plane_face_is_bisected_as_before() {
        let (k, b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let cn = NurbsCurve::of(circle).unwrap();
        let lift = 2e-5;
        let off = moved(&cn, lift);
        let surface = b.face(*f).unwrap().surface.clone().unwrap();
        let plane = plane_face_gap_bound(&k, &surface, &NurbsCurve::of(&off).unwrap(), *range)
            .unwrap()
            .unwrap();
        assert!(plane >= lift && plane <= lift * (1.0 + 1e-6), "{plane:e}");
        for within in [[1e-5, f64::INFINITY], [1e-5, 1e-5]] {
            let (g, pieces) =
                curve_face_gap_bound_within_counted(&k, &b, *f, &off, *range, 1e-8, within)
                    .unwrap();
            let cut = Cell::new(0);
            let alone =
                bisected_face_gap_bound(&k, &b, *f, &off, *range, 1e-8, within, &cut).unwrap();
            assert_eq!(g.map(f64::to_bits), alone.map(f64::to_bits));
            assert_eq!(pieces, cut.get());
            assert!(pieces > 0);
            assert!(g.is_some_and(|g| g >= lift), "{g:?}");
        }
    }

    /// A cancelled kernel stops the plane shortcut too: cancellation, never a bound
    /// (a review of #20416).
    #[test]
    fn a_cancelled_kernel_stops_the_plane_shortcut() {
        let (_, b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let within = [1e-5, f64::INFINITY];
        let stopped =
            curve_face_gap_bound_within_counted(&cancelled, &b, *f, circle, *range, 1e-8, within);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
    }

    /// A plane whose stored net is moved `1e-6` off its analytic plane (each corner along the
    /// normal): the net is still a plane, so [`plane_gap_bound`] would bound against it, but
    /// the corner probes of [`plane_face_gap_bound`] see the net and the plane disagree and
    /// give no bound; the bisection decides, exactly as without the shortcut.
    #[test]
    fn a_plane_with_its_net_off_the_plane_gets_no_plane_bound() {
        use crate::nurbs::b_spline_curve::p2::ControlPointForm;
        use crate::surface::surface::SurfaceKind;
        let (k, mut b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let cn = NurbsCurve::of(circle).unwrap();
        let mut surface = b.face(*f).unwrap().surface.clone().unwrap();
        let SurfaceKind::Plane(plane) = surface.kind_mut() else {
            panic!("a cap is a plane");
        };
        let net = plane.base_mut();
        for (i, j) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let (p, w) = net
                .control_point(ControlPointForm::NonRational, i, j)
                .unwrap();
            let moved = Point3::new(p.x, p.y, p.z + 1e-6);
            net.set_control_point(ControlPointForm::NonRational, i, j, moved, w)
                .unwrap();
        }
        // the moved net alone still takes the circle as lying in it, 1e-6 off
        let stored = NetSurface::of(&k, &surface, &[]).unwrap().unwrap().net;
        let g = plane_gap_bound(&cn, &stored, *range).unwrap();
        assert!(g >= 0.9e-6, "{g:e}");
        // the face's own surface is probed against the analytic plane at the corners
        assert_eq!(
            plane_face_gap_bound(&k, &surface, &cn, *range).unwrap(),
            None
        );
        b.face_mut(*f).unwrap().surface = Some(surface);
        let within = [1e-5, f64::INFINITY];
        let (g, pieces) =
            curve_face_gap_bound_within_counted(&k, &b, *f, circle, *range, 1e-8, within).unwrap();
        let cut = Cell::new(0);
        let alone =
            bisected_face_gap_bound(&k, &b, *f, circle, *range, 1e-8, within, &cut).unwrap();
        assert_eq!(g.map(f64::to_bits), alone.map(f64::to_bits));
        assert_eq!(pieces, cut.get());
    }

    /// `surface` (a plane) with its stored net moved `dz` along z at each corner.
    fn lifted_plane(surface: &Surface, dz: f64) -> Surface {
        use crate::nurbs::b_spline_curve::p2::ControlPointForm;
        use crate::surface::surface::SurfaceKind;
        let mut surface = surface.clone();
        let SurfaceKind::Plane(plane) = surface.kind_mut() else {
            panic!("a cap is a plane");
        };
        let net = plane.base_mut();
        for (i, j) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let (p, w) = net
                .control_point(ControlPointForm::NonRational, i, j)
                .unwrap();
            let moved = Point3::new(p.x, p.y, p.z + dz);
            net.set_control_point(ControlPointForm::NonRational, i, j, moved, w)
                .unwrap();
        }
        surface
    }

    /// #20373 review: a plane whose stored net is `1e-8` off it, within the corner probes'
    /// `1e-9 (1 + |x| + |y| + |z|)` (the net still taken), and a circle moved `1e-8` with it:
    /// the circle lies in the net but `1e-8` off the plane. The plane bound and the structured
    /// trim bound each add the net's distance to the plane ([`net_to_surface_bound`], the
    /// largest at the corners), so both are at least `1e-8`; on the net alone each is a
    /// rounding.
    #[test]
    fn a_plane_net_off_its_plane_within_the_probes_is_counted_in_the_bounds() {
        use crate::surface::surface::SurfaceKind;
        let lift = 1e-8;
        let (k, b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let cn = NurbsCurve::of(circle).unwrap();
        let off = NurbsCurve::of(&moved(&cn, lift)).unwrap();
        let surface = lifted_plane(b.face(*f).unwrap().surface.as_ref().unwrap(), lift);
        let (u, v) = NetSurface::of(&k, &surface, &[]).unwrap().unwrap().domain();
        let corners = [
            Point2::new(u.lo, v.lo),
            Point2::new(u.hi, v.lo),
            Point2::new(u.lo, v.hi),
            Point2::new(u.hi, v.hi),
        ];
        let net = NetSurface::of(&k, &surface, &corners).unwrap().unwrap().net;
        assert!(plane_gap_bound(&off, &net, *range).unwrap() < 1e-10);
        let to_plane = net_to_surface_bound(&k, &surface, &net).unwrap().unwrap();
        assert!(to_plane >= lift && to_plane <= lift * 1.01, "{to_plane:e}");
        let g = plane_face_gap_bound(&k, &surface, &off, *range)
            .unwrap()
            .unwrap();
        assert!(g >= lift, "{g:e}");
        // the structured trim bound of a cap trim, the circle and the net moved alike
        let (k, trims) = cylinder_trims();
        let mut seen = 0;
        for (s, curve, trim, range) in &trims {
            if !matches!(s.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            let lifted = lifted_plane(s, lift);
            let crate::curve::curve::CurveKind::Ellipse(el) = curve.kind() else {
                continue;
            };
            let up = Curve::from(moved(&NurbsCurve::of(el.base()).unwrap(), lift));
            let Some(g) = structured_trim_image_gap_bound(&k, &lifted, &up, trim, *range).unwrap()
            else {
                continue;
            };
            assert!(g >= lift, "{g:e}");
            seen += 1;
        }
        assert!(seen >= 2, "{seen} cap trims structured");
    }

    /// #20373 review r4: a plane net's distance to its plane is held exactly at each corner
    /// ([`exact_corner_offset`]) and enclosed within a few ulps of it. The rotated plane of
    /// `topology.brep_local_operation` gen0014 over `[-1234567, 1234567]^2`: its net's corners
    /// are the plane's corners rounded, `1.6e-10` off it at most, where an interval evaluation
    /// of `O + u X + v Y` widens to `2.1e-9` (an ulp of `1.2e6` per step). The bound must be at
    /// least the exact largest corner distance (checked in expansions, [`crate::query::exact`])
    /// and within `1e-9` relative of it.
    #[test]
    fn a_plane_nets_distance_to_its_plane_is_enclosed_tightly() {
        use crate::geom::extent2d::Extent2d;
        use crate::math::{Vec2, Vec3};
        use crate::query::exact::{le, Exact};
        use crate::surface::plane::Plane;
        let k = Kernel::default();
        let a = 1_234_567.0;
        let plane = Plane::new(
            Point3::new(0.0, 2.5000000000000004, 0.055943466315470314),
            Vec3::new(
                -0.07001563037373039,
                0.9697429256825116,
                0.23387276368164822,
            ),
            Vec3::new(-0.9579909515227577, 0.0, -0.28679842538012873),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-a, -a, a, a),
        );
        let pos = *plane.position();
        let surface = Surface::from(plane);
        let net = NetSurface::of(&k, &surface, &[]).unwrap().unwrap().net;
        let r = net_to_surface_bound(&k, &surface, &net).unwrap().unwrap();
        let (o, x, y) = (pos.origin(), pos.x_axis(), pos.y_axis());
        let mut worst = Exact::of(0.0);
        for (i, u) in [net.u[1], net.u[2]].into_iter().enumerate() {
            for (j, v) in [net.v[1], net.v[2]].into_iter().enumerate() {
                let c = &net.pw[4 * (2 * i + j)..4 * (2 * i + j) + 4];
                assert_eq!(c[3], 1.0);
                let d2 = [
                    (c[0], o.x, x.x, y.x),
                    (c[1], o.y, x.y, y.y),
                    (c[2], o.z, x.z, y.z),
                ]
                .into_iter()
                .fold(Exact::of(0.0), |acc, (p, ok, xk, yk)| {
                    let d = Exact::of(p)
                        .sub(&Exact::of(ok))
                        .sub(&Exact::of(u).scale(xk))
                        .sub(&Exact::of(v).scale(yk));
                    acc.add(&d.square())
                });
                if d2.sub(&worst).sign() == std::cmp::Ordering::Greater {
                    worst = d2;
                }
            }
        }
        assert_ne!(
            worst.sign(),
            std::cmp::Ordering::Equal,
            "the net is off its plane"
        );
        // sound: worst <= r^2; tight: r^2 <= worst (1 + 1e-9)^2
        le(&worst, &Exact::of(r).square());
        le(&Exact::of(r).square(), &worst.scale(1.000_000_002));
        assert!(r < 3e-10, "{r:e}");
    }

    /// #20373 review r4: where the structured bound is above `enough`, the bisection answers,
    /// and it too adds the net's distance to the surface ([`bisection_residual`]). The cap
    /// trims with the plane's net and the circle both lifted `1e-8` (a gap of `1e-8` in z, a
    /// rounding on the net): asked within `5e-9`, the trim bound is at least `1e-8` (it was
    /// `4.7e-9` and `3.2e-9`, the net's alone); so is the face bound, where it is finite.
    #[test]
    fn a_plane_net_off_its_plane_is_counted_in_the_bisections() {
        use crate::surface::surface::SurfaceKind;
        let lift = 1e-8;
        let (k, trims) = cylinder_trims();
        let mut seen = 0;
        for (s, curve, trim, range) in &trims {
            if !matches!(s.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            let lifted = lifted_plane(s, lift);
            let crate::curve::curve::CurveKind::Ellipse(el) = curve.kind() else {
                continue;
            };
            let up = Curve::from(moved(&NurbsCurve::of(el.base()).unwrap(), lift));
            let structured = structured_trim_image_gap_bound(&k, &lifted, &up, trim, *range)
                .unwrap()
                .unwrap();
            assert!(structured > 5e-9, "{structured:e}");
            let g = trim_image_gap_bound_within(
                &k,
                &lifted,
                &up,
                trim,
                *range,
                1e-12,
                [5e-9, f64::INFINITY],
                None,
            )
            .unwrap()
            .unwrap();
            assert!(g >= lift, "{g:e}");
            seen += 1;
        }
        assert!(seen >= 2, "{seen} cap trims bisected");
        let (k, mut b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let lifted = lifted_plane(b.face(*f).unwrap().surface.as_ref().unwrap(), lift);
        b.face_mut(*f).unwrap().surface = Some(lifted);
        let up = moved(&NurbsCurve::of(circle).unwrap(), lift);
        let g = curve_face_gap_bound_within(&k, &b, *f, &up, *range, 1e-12, [5e-9, f64::INFINITY])
            .unwrap();
        assert!(g.is_none_or(|g| g >= lift), "{g:?}");
    }

    /// The plane bound holds only for positive weights and control points whose projections
    /// lie in the net's box; otherwise it gives none and the bisection decides.
    #[test]
    fn the_plane_bound_needs_positive_weights_and_projections_inside_the_box() {
        let (k, b, caps) = cap_circles();
        let (f, circle, range) = caps.first().unwrap();
        let surface = b.face(*f).unwrap().surface.clone().unwrap();
        let net = NetSurface::of(&k, &surface, &[]).unwrap().unwrap().net;
        let cn = NurbsCurve::of(circle).unwrap();
        let g = plane_gap_bound(&cn, &net, *range).unwrap();
        assert!(g <= 1e-9, "{g:e}");
        for w in [0.0, -1.0, f64::NAN] {
            let mut bad = cn.clone();
            bad.pw[7] = w;
            assert_eq!(plane_gap_bound(&bad, &net, *range), None, "weight {w}");
        }
        // a circle in the plane, its control polygon reaching past the net's box
        let (u, v) = NetSurface::of(&k, &surface, &[]).unwrap().unwrap().domain();
        let reach = 4.0 * (u.hi - u.lo).max(v.hi - v.lo);
        let mut wide = cn.clone();
        for q in wide.pw.chunks_exact_mut(4) {
            q[0] *= reach;
            q[1] *= reach;
        }
        assert_eq!(plane_gap_bound(&wide, &net, *range), None);
        // an empty or reversed range bounds nothing
        assert_eq!(plane_gap_bound(&cn, &net, [range[0], range[0]]), None);
        assert_eq!(plane_gap_bound(&cn, &net, [range[1], range[0]]), None);
    }

    /// A face's surface, its edge's curve, the edge use's trim and the edge's interval.
    type EdgeTrim = (Surface, Curve, Curve, [f64; 2]);

    /// The UV trims of a cylinder of radius 12.5 (a round tube's profile, #20373): each held
    /// trim with its surface, its edge's curve and the edge's interval.
    fn cylinder_trims() -> (Kernel, Vec<EdgeTrim>) {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 12.5, 50.0)
            .unwrap();
        let mut out = Vec::new();
        for e in crate::query::api_queries::get_edges(&b).unwrap() {
            let rec = b.edge(e).unwrap();
            let Some(curve) = rec.curve.as_ref() else {
                continue;
            };
            for &eu in &rec.edgeuses {
                let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.as_ref() else {
                    continue;
                };
                let f = crate::topology::edgeuse::p1::face(&b, eu).unwrap().unwrap();
                let s = b.face(f).unwrap().surface.as_ref().unwrap();
                let ivl = rec.interval;
                out.push((
                    s.clone_for_evaluation().unwrap(),
                    curve.clone(),
                    trim.clone(),
                    [ivl.min(), ivl.max()],
                ));
            }
        }
        (k, out)
    }

    /// The net and the curve and trim data [`trim_image_gap_bound_within`] bounds a trim on.
    fn trim_data(
        k: &Kernel,
        s: &Surface,
        curve: &Curve,
        trim: &Curve,
        range: [f64; 2],
    ) -> (NurbsCurve, NurbsCurve, Nurbs) {
        let (cn, _) = exact_curve_data(curve, false).unwrap().unwrap();
        let (wn, _) = exact_curve_data(trim, true).unwrap().unwrap();
        let probes: Vec<Point2> = (0..TRIM_PROBES)
            .map(|i| {
                let t = range[0] + (range[1] - range[0]) * (i as f64) / ((TRIM_PROBES - 1) as f64);
                let p = trim.evaluate_point(k, t).unwrap();
                Point2::new(p.x, p.y)
            })
            .collect();
        let net = NetSurface::of(k, s, &probes).unwrap().unwrap();
        (cn, wn, net.net)
    }

    /// #20373: a circle's trims in the UV of its plane caps, with the edge's knots and
    /// weights, and on its cylinder's side, the iso line along the whole side, are bounded by
    /// their control points at once: the bound is the structured one, far below the `1e-5`
    /// the bisection cut each of them for (about 1000 pieces, 17 ms a cap trim and 26 ms a
    /// side trim in a release build, the answer just under `1e-5`).
    #[test]
    fn a_circles_trims_on_its_caps_and_side_are_bounded_without_cutting_them() {
        use crate::surface::surface::SurfaceKind;
        let (k, trims) = cylinder_trims();
        let (mut planes, mut sides) = (0, 0);
        for (s, curve, trim, range) in &trims {
            let (cn, wn, net) = trim_data(&k, s, curve, trim, *range);
            let Some(g) = structured_trim_gap_bound(&cn, &wn, &net, *range) else {
                continue;
            };
            // (the seam's line, along `v` at the side's ends: the composed case bounds its
            // trims too (#22930), their boxes on the seam itself, clear of the circles' seam
            // tear `assert_trim_answered` looks for)
            if matches!(curve.kind(), crate::curve::curve::CurveKind::Line(_)) {
                assert!(
                    matches!(s.kind(), SurfaceKind::Cone(_)) && g <= 1e-9,
                    "{g:e}"
                );
                continue;
            }
            match s.kind() {
                SurfaceKind::Plane(_) => planes += 1,
                SurfaceKind::Cone(_) => sides += 1,
                _ => panic!("a structured trim off a plane or the side"),
            }
            assert_trim_answered(&k, s, curve, trim, *range, g);
        }
        assert!(
            planes >= 2 && sides >= 2,
            "{planes} cap and {sides} side trims structured"
        );
    }

    /// #20373: a round profile swept along a straight path (`sweep_along_segments`: the wall
    /// copied from a linear sweep, planar caps built from the circles, then
    /// `stitch_and_orient`, as SolidCraft's straight members are built) has every trim of its
    /// circle edges, on the caps and on the wall as the stitch and orientation leave it,
    /// answered by the structured bound far below `1e-5`: the bisection that cut each of them
    /// into about 1000 pieces is not reached.
    #[test]
    fn an_extruded_round_profiles_circle_trims_are_answered_at_once() {
        use crate::curve::arc::ArcDefinition;
        use crate::curve::ellipse::Ellipse;
        use crate::curve::path::{PathTolerances, Segment};
        use crate::math::Vec3;
        use crate::surface::surface::SurfaceKind;
        use crate::sweep::segments::sweep_along_segments;
        let mut k = Kernel::default();
        let d = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::X,
            reference: Vec3::Y,
            radius: 12.5,
            start: 0.0,
            end: std::f64::consts::TAU,
        };
        let profile = vec![Curve::from(Ellipse::arc(&k, &d).unwrap())];
        let path = [Segment::Line {
            start: Point3::ZERO,
            end: Point3::new(50.0, 0.0, 0.0),
        }];
        let tol = PathTolerances {
            gap: 1e-9,
            turn: 1e-12,
            length: 1e-12,
            closed: 1e-6,
        };
        let (b, _) = sweep_along_segments(&mut k, &profile, &path, true, &tol).unwrap();
        let (mut planes, mut sides) = (0, 0);
        for e in crate::query::api_queries::get_edges(&b).unwrap() {
            let rec = b.edge(e).unwrap();
            let Some(curve) = rec.curve.as_ref() else {
                continue;
            };
            if matches!(curve.kind(), crate::curve::curve::CurveKind::Line(_)) {
                continue;
            }
            for &eu in &rec.edgeuses {
                let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.as_ref() else {
                    continue;
                };
                let f = crate::topology::edgeuse::p1::face(&b, eu).unwrap().unwrap();
                let mut s = b
                    .face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .unwrap()
                    .clone_for_evaluation()
                    .unwrap();
                let _ = s.set_owner(None);
                let range = [rec.interval.min(), rec.interval.max()];
                let g = structured_trim_image_gap_bound(&k, &s, curve, trim, range)
                    .unwrap()
                    .unwrap_or_else(|| {
                        panic!("edge {} trim on {:?} not structured", e.index(), s.kind())
                    });
                assert!(g <= trim_cap(&s), "{g:e}");
                match s.kind() {
                    SurfaceKind::Plane(_) => planes += 1,
                    _ => sides += 1,
                }
            }
        }
        // Both wall trims, and the cap trims there are (the stitch leaves the cap built at
        // the path's start without UV trims: a trimless face, covered as one).
        assert!(
            sides == 2 && planes >= 1,
            "{planes} cap and {sides} wall trims structured"
        );
    }

    /// The cap on a structured trim's answer on `s`: `1e-9` off a cone, and on the full-turn
    /// cylinders of radius 12.5 these tests build `1e-9` plus the seam snap's reach
    /// (#20643): `Ellipse::step_inversion` snaps a polar point within `τ = 1e-7` of the
    /// domain's end onto the seam, so the surface is up to `asin(τ / rx) r ≤ (π/3) τ r / rx`
    /// (the map's norm `1` for an orthonormal frame and a round circle; `r = rx = 12.5` here:
    /// 1.05e-7) off its net there (measured 4.5e-8 at 5e-8 before the end, 9e-8 at 1e-7:
    /// `a_full_turn_cones_seam_snap_is_inside_the_bound`), and the bound counts it; 1% over
    /// for the bound's frame factor and margins. Still far below the `1e-5` the bisection was
    /// asked for.
    fn trim_cap(s: &Surface) -> f64 {
        use crate::surface::surface::SurfaceKind;
        match s.kind() {
            SurfaceKind::Cone(_) => 1e-9 + std::f64::consts::FRAC_PI_3 * 1e-7 * 1.01,
            _ => 1e-9,
        }
    }

    /// That [`trim_image_gap_bound_within`] answers `[1e-5, ∞)` with the structured bound `g`
    /// (an edge curve's slack 0 and the net's distance to the surface added, rounded up:
    /// [`structured_trim_image_gap_bound`]) for a trim the bisection bounds at `g` or above.
    fn assert_trim_answered(
        k: &Kernel,
        s: &Surface,
        curve: &Curve,
        trim: &Curve,
        range: [f64; 2],
        g: f64,
    ) {
        assert!((0.0..=1e-9).contains(&g), "{g:e}");
        let within = trim_image_gap_bound_within(
            k,
            s,
            curve,
            trim,
            range,
            1e-8,
            [1e-5, f64::INFINITY],
            None,
        )
        .unwrap();
        let answer = structured_trim_image_gap_bound(k, s, curve, trim, range)
            .unwrap()
            .unwrap();
        assert!(
            answer >= (Ivl::from(g) + Ivl::from(0.0)).hi,
            "{answer:e} against {g:e}"
        );
        assert!(answer <= trim_cap(s), "{answer:e}");
        assert_eq!(within, Some(answer), "the structured bound is the answer");
        let bisected = trim_image_gap_bound_within(
            k,
            s,
            curve,
            trim,
            range,
            1e-8,
            [f64::NEG_INFINITY, f64::INFINITY],
            None,
        )
        .unwrap()
        .unwrap();
        assert!(bisected >= g, "{bisected:e} against {g:e}");
        // (#20643 review: both answers carry the net's own distance to the surface over the
        // trim's UV box, the cone's seam tear on a full-turn cylinder, which every trim here
        // reaches; `0` off a cone; at most the whole net's)
        let net = NetSurface::of(k, s, &[]).unwrap().unwrap();
        let (wn, _) = exact_curve_data(trim, true).unwrap().unwrap();
        let window = nurbs_uv_box(&wn);
        assert!(window.is_some());
        let residual = bisection_residual(k, s, &net, window).unwrap().unwrap();
        let whole = bisection_residual(k, s, &net, None).unwrap().unwrap();
        assert!(
            residual <= whole,
            "{residual:e} over the whole net's {whole:e}"
        );
        if matches!(s.kind(), crate::surface::surface::SurfaceKind::Cone(_))
            && window.is_some_and(|[u, _]| u[1] - u[0] > 1e-6)
        {
            assert!(residual > 1e-8, "the seam tear {residual:e}");
        }
        assert!(
            answer >= residual,
            "{answer:e} under the residual {residual:e}"
        );
        assert!(
            bisected >= residual,
            "{bisected:e} under the residual {residual:e}"
        );
    }

    /// The side of a cylinder of radius 12.5 and its net's row at the top (`v` its domain's
    /// end): the side's surface, its net, and the row as a curve over `[0, 1]` (the net's `u`
    /// knots scaled), as a round tube's extruded circle on its side.
    fn side_row() -> (Kernel, Surface, Nurbs, BSplineCurve) {
        use crate::surface::surface::SurfaceKind;
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 12.5, 50.0)
            .unwrap();
        let s = crate::query::api_queries::get_faces(&b)
            .unwrap()
            .into_iter()
            .map(|f| {
                b.face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .unwrap()
                    .clone_for_evaluation()
                    .unwrap()
            })
            .find(|s| matches!(s.kind(), SurfaceKind::Cone(_)))
            .unwrap();
        let net = NetSurface::of(&k, &s, &[]).unwrap().unwrap().net;
        assert!(
            net.q == 1 && net.nv == 2,
            "a cylinder's net is linear along its axis"
        );
        let last = net.u[net.u.len() - 1];
        let knots = net.u.iter().map(|x| x / last).collect();
        let cps = (0..net.nu)
            .map(|i| {
                let q = &net.pw[4 * (2 * i + 1)..4 * (2 * i + 2)];
                CPoint::new(q[0], q[1], q[2], q[3])
            })
            .collect();
        let row = NurbCurve::new(cps, net.p, knots).unwrap();
        (k, s, net, BSplineCurve::from_nurb(row, 3, false))
    }

    /// The largest distance at equal parameters between the curve data `cn` and the image of
    /// the trim data `wn` on `net`, sampled at `n + 1` parameters over `range` (at the
    /// enclosures' middles).
    fn sampled_data_gap(
        cn: &NurbsCurve,
        wn: &NurbsCurve,
        net: &Nurbs,
        [lo, hi]: [f64; 2],
        n: usize,
    ) -> f64 {
        let s = NetSurface {
            net: net.clone(),
            ends: EndContinuation::default(),
            offset: None,
        };
        let mid = |v: V3<Ivl>| crate::math::Vec3::new(v.x.mid(), v.y.mid(), v.z.mid());
        let mut worst = 0.0_f64;
        for i in 0..=n {
            let t = lo + (hi - lo) * (i as f64) / (n as f64);
            let c = mid(curve_derivs_over(cn, Ivl::from(t), 0, None).unwrap()[0]);
            let w = mid(curve_derivs_over(wn, Ivl::from(t), 0, None).unwrap()[0]);
            let p = s.point(Point2::new(w.x, w.y)).unwrap();
            worst = worst.max(c.distance(p));
        }
        worst
    }

    /// #22930: a trim that is its curve taken back through the net (here a polyline in the
    /// UV of a cylinder's side through the side's knots, at the breakpoints of the row it
    /// runs along, under that row) is bounded at once by the net's patches composed with its
    /// segments, far under `1e-5`, as the third case of the structured bound. Its points a few
    /// roundings past the knots cost only their move into the patch; a curve moved off the
    /// side, or a trim moved along it, still shows its gap; and every bound is at least the
    /// gap sampled at equal parameters.
    #[test]
    fn a_trim_that_is_its_curve_taken_back_through_the_net_is_bounded_by_the_composition() {
        let (k, _, net, row) = side_row();
        let v1 = net.v[net.v.len() - 1];
        let row = Curve::from(row);
        let (cn, _) = exact_curve_data(&row, false).unwrap().unwrap();
        let breaks = distinct_knots(&cn.u);
        let us = distinct_knots(&net.u);
        assert_eq!(breaks.len(), us.len(), "a row's spans are the side's");
        let polyline = |us: &[f64]| {
            let mut u = vec![breaks[0]];
            u.extend_from_slice(&breaks);
            u.push(breaks[breaks.len() - 1]);
            NurbsCurve {
                p: 1,
                n: us.len(),
                u,
                pw: us.iter().flat_map(|&x| [x, v1, 0.0, 1.0]).collect(),
            }
        };
        let range = [0.0, 1.0];
        let composed = |c: &NurbsCurve, w: &NurbsCurve| {
            composed_trim_gap_bound(&k, c, w, &net, range).unwrap()
        };
        let wn = polyline(&us);
        let g = composed(&cn, &wn).unwrap();
        assert!(g <= 1e-9, "{g:e}");
        assert!(g >= sampled_data_gap(&cn, &wn, &net, range, 1000));
        assert_eq!(structured_trim_gap_bound(&cn, &wn, &net, range), Some(g));
        assert_eq!(
            structured_trim_gap_bound_as(&k, &cn, &wn, &net, range, true).unwrap(),
            Some(g)
        );
        // (a cancelled kernel stops it: cancellation, not a trim it cannot bound)
        let token = crate::kernel::CancelToken::default();
        token.cancel();
        let cancelled = Kernel {
            cancel: Some(token),
            ..Kernel::default()
        };
        let stopped = composed_trim_gap_bound(&cancelled, &cn, &wn, &net, range);
        assert!(
            matches!(&stopped, Err(e) if e.is_cancelled()),
            "{stopped:?}"
        );
        // (the inner points 8 roundings past their knots, either way)
        for sign in [1.0, -1.0] {
            let mut past = us.clone();
            let last = past.len() - 1;
            for x in &mut past[1..last] {
                *x += sign * 8.0 * f64::EPSILON * x.abs();
            }
            let h = composed(&cn, &polyline(&past)).unwrap();
            assert!(h <= 1e-9, "{h:e}");
        }
        // (no pass-over: the curve 1e-3 off the side, a trim point 1e-3 of a span along it)
        let off = moved(&cn, 1e-3);
        let (mn, _) = exact_curve_data(&Curve::from(off), false).unwrap().unwrap();
        let mut along = us.clone();
        along[2] += 1e-3 * (us[2] - us[1]);
        for (c, w) in [(&mn, &wn), (&cn, &polyline(&along))] {
            let h = composed(c, w).unwrap();
            let seen = sampled_data_gap(c, w, &net, range, 1000);
            assert!(seen > 1e-5 && h >= seen, "{h:e} against {seen:e}");
        }
        // (other structures are left to the other cases or the bisection: a trim on other
        // breakpoints, a weight that is not positive, a trim past the net's end)
        let mut other = wn.clone();
        other.u = vec![0.0, 0.0, 0.3, 0.5, 0.75, 1.0, 1.0];
        assert_eq!(composed(&cn, &other), None);
        let mut weightless = wn.clone();
        weightless.pw[7] = 0.0;
        assert_eq!(composed(&cn, &weightless), None);
        let mut beyond = us.clone();
        let end = beyond.len() - 1;
        beyond[end] += 1e-3;
        assert_eq!(composed(&cn, &polyline(&beyond)), None);
    }

    /// #22930 (review of #23045): a trim whose hull passes its patch's neighbours would be
    /// moved through spans whose speed the bound does not take, so it is not bounded. On a
    /// degree 1 net in the plane `x = 0`, `u` knots `0, 1, 2, 3, 10`, `z` rising by 100 over
    /// `[0, 1]` and flat after, a trim from `u = 0.25` to `9.75` would be moved into
    /// `[3, 10]`, some 2.75, at the flat spans' speed (a bound near 5.5), where the curve on
    /// the flat part lies 75 from the trim's first point. A hull within the neighbours is
    /// still bounded, at least the gap.
    #[test]
    fn a_composed_trim_past_its_patchs_neighbours_is_not_bounded() {
        let k = Kernel::default();
        let net = |z: [f64; 5]| {
            let mut pw = Vec::new();
            for z in z {
                for v in [0.0, 1.0] {
                    pw.extend_from_slice(&[0.0, v, z, 1.0]);
                }
            }
            Nurbs {
                p: 1,
                q: 1,
                nu: 5,
                nv: 2,
                u: vec![0.0, 0.0, 1.0, 2.0, 3.0, 10.0, 10.0],
                v: vec![0.0, 0.0, 1.0, 1.0],
                pw,
            }
        };
        let range = [0.0, 1.0];
        let on_flat = segment([[0.0, 0.5, 100.0, 1.0], [0.0, 0.5, 100.0, 1.0]]);
        let rising = net([0.0, 100.0, 100.0, 100.0, 100.0]);
        let far = segment([[0.25, 0.5, 0.0, 1.0], [9.75, 0.5, 0.0, 1.0]]);
        let seen = sampled_data_gap(&on_flat, &far, &rising, range, 1000);
        assert!(seen >= 75.0 - 1e-9, "{seen:e}");
        let g = composed_trim_gap_bound(&k, &on_flat, &far, &rising, range).unwrap();
        assert_eq!(g, None, "a bound {g:?} under the gap {seen:e}");
        // (from `u = 2.5`, in `[3, 10]`'s neighbour, on a net rising over `[2, 3]`: moved
        // through that span, whose speed is taken)
        let late = net([0.0, 0.0, 0.0, 100.0, 100.0]);
        let near = segment([[2.5, 0.5, 0.0, 1.0], [9.75, 0.5, 0.0, 1.0]]);
        let seen = sampled_data_gap(&on_flat, &near, &late, range, 1000);
        assert!(seen >= 50.0 - 1e-9, "{seen:e}");
        let g = composed_trim_gap_bound(&k, &on_flat, &near, &late, range)
            .unwrap()
            .unwrap();
        assert!(g >= seen, "{g:e} under the gap {seen:e}");
    }

    /// #20373: the extrudes' other side trims run against the side's `u` (from its end to an
    /// ulp from its start), with the circle running the row backwards: still bounded by the
    /// control points at once, and a trim backwards against a curve forwards still shows its
    /// gap (the circle's diameter).
    #[test]
    fn a_circles_backward_iso_trim_on_its_cylinder_is_bounded_without_cutting_it() {
        let (k, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let last = net.u[net.u.len() - 1];
        let knots = net.u.iter().rev().map(|x| 1.0 - x / last).collect();
        let cps = (0..net.nu)
            .rev()
            .map(|i| {
                let q = &net.pw[4 * (2 * i + 1)..4 * (2 * i + 2)];
                CPoint::new(q[0], q[1], q[2], q[3])
            })
            .collect();
        let back = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(cps, net.p, knots).unwrap(),
            3,
            false,
        ));
        let trim = iso_trim(u1, u0 + 2.842170943040401e-14, v1);
        let (cn, wn, probed) = trim_data(&k, &s, &back, &trim, [0.0, 1.0]);
        let g = structured_trim_gap_bound(&cn, &wn, &probed, [0.0, 1.0]).unwrap();
        assert_trim_answered(&k, &s, &back, &trim, [0.0, 1.0], g);
        let (fwd, _, _) = trim_data(&k, &s, &Curve::from(row), &trim, [0.0, 1.0]);
        let g = structured_trim_gap_bound(&fwd, &wn, &probed, [0.0, 1.0]).unwrap();
        assert!(g >= 25.0, "{g:e}");
    }

    /// The unit square net in `z = 0`, degree 1 by 1, its knots `[0, 0, 1, 1]` both ways.
    fn unit_plane_net() -> Nurbs {
        let mut pw = Vec::new();
        for (x, y) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
            pw.extend_from_slice(&[x, y, 0.0, 1.0]);
        }
        Nurbs {
            p: 1,
            q: 1,
            nu: 2,
            nv: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        }
    }

    /// A curve of degree 1 over `[0, 1]` through `points`, `(x, y, z, w)` homogeneous.
    fn segment(points: [[f64; 4]; 2]) -> NurbsCurve {
        NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            pw: points.concat(),
        }
    }

    /// #20373 review r4: [`product_enclosure`] takes a product as exact only where its fused
    /// residual cannot round to zero (`|r| >= 2^-969`). `a = 2 - 2^-52`, `w = a 2^-971`:
    /// `r = (2 - 2^-51) 2^-970`, the exact product `r + 2^-1075`, whose fused residual ties
    /// to `0`; the enclosure reaches above `r`. At `2^-500 2^-469 = 2^-969` the product is
    /// exact and taken as it is.
    #[test]
    fn a_product_whose_residual_ties_to_zero_is_enclosed() {
        let a = f64::from_bits(0x3fff_ffff_ffff_ffff);
        let w = f64::from_bits(0x034f_ffff_ffff_ffff);
        let r = a * w;
        assert_eq!(r.to_bits(), 0x035f_ffff_ffff_fffe);
        assert_eq!(a.mul_add(w, -r), 0.0, "the residual ties to zero");
        let (lo, hi) = product_enclosure(a, w);
        assert!(lo <= r && hi > r, "[{lo:e}, {hi:e}] around {r:e}");
        let (a, w) = (f64::from_bits(523 << 52), f64::from_bits(554 << 52));
        let r = a * w;
        assert_eq!(r, 2.0 * f64::MIN_POSITIVE / f64::EPSILON);
        assert_eq!(product_enclosure(a, w), (r, r));
    }

    /// #20373 (an extrude's rectangle sides): a trim whose points lie on its plane net's box
    /// edge is in the box, decided from the stored data, so the plane case bounds it (its
    /// fractions' enclosures round past `[0, 1]`, which refused it before). A side
    /// `(0, 0) -> (0, 1)`, a diagonal `(0, 0) -> (1, 1)` (no iso line: only the plane case
    /// takes it) and the diagonal with weights 2 (the box decided with enclosed products) each
    /// get a bound of a rounding, and a trim an ulp past the box gets none from the plane case.
    #[test]
    fn a_trim_on_its_plane_nets_edge_is_in_the_box() {
        let net = unit_plane_net();
        for (a, b, w) in [
            ([0.0, 0.0], [0.0, 1.0], 1.0),
            ([0.0, 0.0], [1.0, 1.0], 1.0),
            ([0.0, 0.0], [1.0, 1.0], 2.0),
        ] {
            let trim = segment([[a[0] * w, a[1] * w, 0.0, w], [b[0] * w, b[1] * w, 0.0, w]]);
            let curve = trim.clone();
            let plane = bilinear_trim_gap_bound(&curve, &trim, &net, [0.0, 1.0]);
            assert!(
                plane.is_some_and(|g| g <= 64.0 * f64::EPSILON),
                "{a:?} {b:?} {w}: {plane:?}"
            );
            let g = structured_trim_gap_bound(&curve, &trim, &net, [0.0, 1.0]);
            assert_eq!(g, plane);
        }
        let past = segment([[0.0, 0.0, 0.0, 1.0], [1.0, f64::EPSILON + 1.0, 0.0, 1.0]]);
        assert_eq!(
            bilinear_trim_gap_bound(&past, &past, &net, [0.0, 1.0]),
            None
        );
    }

    /// #20373 (the reporter's rectangle extrudes): a rectangle profile swept along a straight
    /// line (`create_linear_sweep`, as SolidCraft extrudes it) has every UV trim of its edges,
    /// on its sides (whose trims run along their nets' box edges) and on its caps, answered by
    /// the structured bound at once, a rounding: the bisection that cut each side trim into
    /// 32 pieces (about 1 ms each, 1040 of them in the reported case) is not reached. (A side
    /// trim is an iso line of its net, so either the plane case or the iso case answers it;
    /// which one is not pinned here: [`a_trim_on_its_plane_nets_edge_is_in_the_box`] pins
    /// the plane case's box test.)
    #[test]
    fn an_extruded_rectangles_trims_are_answered_at_once() {
        let mut k = Kernel::default();
        let c = [
            Point3::new(0.0, -3.0, -2.0),
            Point3::new(0.0, 3.0, -2.0),
            Point3::new(0.0, 3.0, 2.0),
            Point3::new(0.0, -3.0, 2.0),
        ];
        // (the sketch's sides as degree-1 B-splines, as SolidCraft hands them over: their trims
        // are B-splines too, evaluated exactly)
        let profile: Vec<Curve> = (0..4)
            .map(|i| {
                let cps = vec![CPoint::from_point(c[i]), CPoint::from_point(c[(i + 1) % 4])];
                let side = NurbCurve::new(cps, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
                Curve::from(BSplineCurve::from_nurb(side, 3, false))
            })
            .collect();
        let refs: Vec<Option<&Curve>> = profile.iter().map(Some).collect();
        let b = crate::sweep::api_brep::create_linear_sweep(
            &mut k,
            &refs,
            crate::math::Vec3::new(1.0, 0.0, 0.0),
            5.0,
            true,
        )
        .unwrap();
        let mut trims = 0;
        for e in crate::query::api_queries::get_edges(&b).unwrap() {
            let rec = b.edge(e).unwrap();
            let Some(curve) = rec.curve.as_ref() else {
                continue;
            };
            for &eu in &rec.edgeuses {
                let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.as_ref() else {
                    continue;
                };
                let f = crate::topology::edgeuse::p1::face(&b, eu).unwrap().unwrap();
                let mut s = b
                    .face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .unwrap()
                    .clone_for_evaluation()
                    .unwrap();
                let _ = s.set_owner(None);
                let range = [rec.interval.min(), rec.interval.max()];
                // (the stitch bounds each side trim as the degree-1 B-spline of its stored
                // points, evaluated exactly: the line it becomes carries its own rounding)
                let spline;
                let trim = match trim.kind() {
                    crate::curve::curve::CurveKind::Line(l) => {
                        spline = Curve::from(l.base().clone_exact());
                        &spline
                    }
                    _ => trim,
                };
                let g = structured_trim_image_gap_bound(&k, &s, curve, trim, range)
                    .unwrap()
                    .unwrap_or_else(|| {
                        panic!("edge {} trim on {:?} not structured", e.index(), s.kind())
                    });
                assert!(g <= 1e-9, "{g:e}");
                let within = trim_image_gap_bound_within(
                    &k,
                    &s,
                    curve,
                    trim,
                    range,
                    1e-8,
                    [1e-5, f64::INFINITY],
                    Some(0),
                )
                .unwrap();
                assert_eq!(within, Some(g), "edge {}: no bisection", e.index());
                trims += 1;
            }
        }
        assert!(trims >= 16, "{trims} trims");
    }

    /// The trim of degree 1 from `(u0, v)` to `(u1, v)` over `[0, 1]`.
    fn iso_trim(u0: f64, u1: f64, v: f64) -> Curve {
        let c = NurbCurve::new(
            vec![CPoint::new(u0, v, 0.0, 1.0), CPoint::new(u1, v, 0.0, 1.0)],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Curve::from(BSplineCurve::from_nurb(c, 2, false))
    }

    /// #20373: a circle on the side of its cylinder, trimmed by the iso line along the whole
    /// side at its height, is bounded by its control points against the net's row there at
    /// once (a round tube's extruded circle: about 1000 pieces and 26 ms each before).
    #[test]
    fn a_circles_iso_trim_on_its_cylinder_is_bounded_without_cutting_it() {
        let (k, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let curve = Curve::from(row);
        let trim = iso_trim(u0, u1, v1);
        let (cn, wn, probed) = trim_data(&k, &s, &curve, &trim, [0.0, 1.0]);
        let g = structured_trim_gap_bound(&cn, &wn, &probed, [0.0, 1.0]).unwrap();
        assert_trim_answered(&k, &s, &curve, &trim, [0.0, 1.0], g);
        // (a trim an ulp short of the side's end, as the extrudes of #20373 give: bounded with
        // the row's speed times the ulp, still far under `1e-5`)
        let ulp_short = u1 - f64::EPSILON * u1;
        assert!(ulp_short < u1);
        let trim = iso_trim(u0, ulp_short, v1);
        let (_, short, _) = trim_data(&k, &s, &curve, &trim, [0.0, 1.0]);
        let g = structured_trim_gap_bound(&cn, &short, &probed, [0.0, 1.0]).unwrap();
        assert_trim_answered(&k, &s, &curve, &trim, [0.0, 1.0], g);
        // (a trim that stops halfway along the side: its gap, across the circle at the end,
        // is still seen; one with a weight not 1 is left to the bisection)
        let (_, half, _) = trim_data(&k, &s, &curve, &iso_trim(u0, 0.5 * u1, v1), [0.0, 1.0]);
        let g = structured_trim_gap_bound(&cn, &half, &probed, [0.0, 1.0]).unwrap();
        assert!(g >= 25.0, "{g:e}");
        let mut heavy = wn.clone();
        for x in &mut heavy.pw {
            *x *= 2.0;
        }
        assert_eq!(
            structured_trim_gap_bound(&cn, &heavy, &probed, [0.0, 1.0]),
            None
        );
        // (a curve with the row's knots but not its weights)
        let mut off_weight = cn.clone();
        off_weight.pw[7] *= 0.5;
        assert_eq!(
            structured_trim_gap_bound(&off_weight, &wn, &probed, [0.0, 1.0]),
            None
        );
    }

    /// The largest distance between `curve` and the image of `trim` on `s` at equal
    /// parameters, sampled at 2001 points of `range`: a lower bound on the same-parameter gap.
    fn sampled_same_parameter_gap(
        k: &Kernel,
        s: &Surface,
        curve: &Curve,
        trim: &Curve,
        [lo, hi]: [f64; 2],
    ) -> f64 {
        (0..=2000)
            .map(|i| {
                let t = lo + (hi - lo) * f64::from(i) / 2000.0;
                let c = curve.evaluate_point(k, t).unwrap();
                let w = trim.evaluate_point(k, t).unwrap();
                let p = s.evaluate_point(k, Point2::new(w.x, w.y)).unwrap();
                ((c.x - p.x).powi(2) + (c.y - p.y).powi(2) + (c.z - p.z).powi(2)).sqrt()
            })
            .fold(0.0, f64::max)
    }

    /// #22034: every trim of a cylinder's edges (the circles on the caps, a trim on a plane
    /// with the circle's knots and weights; the circles on the side, along its net's rows; the
    /// seam line, a piece of a line's long domain along the side's first and last columns) is
    /// proved within `1e-5` of its edge at equal parameters by its control points at once, as
    /// a boolean's carried trims are gated: no bisection runs, and each bound is at least the
    /// gap sampled at equal parameters.
    #[test]
    fn a_cylinders_trims_are_proved_at_equal_parameters_without_bisecting_them() {
        let (k, trims) = cylinder_trims();
        let mut lines = 0;
        for (s, curve, trim, range) in &trims {
            let before = k.work_counters.same_parameter_bisections();
            let g = trim_same_parameter_gap_bound_within(
                &k,
                s,
                curve,
                trim,
                *range,
                1e-8,
                [1e-5, 1e-5],
                Some(MAX_PIECES),
            )
            .unwrap();
            let Some(g) = g else {
                // (a pair with no exact data to bound it from: a UV line's)
                continue;
            };
            let line = matches!(curve.kind(), crate::curve::curve::CurveKind::Line(_));
            assert_eq!(
                k.work_counters.same_parameter_bisections(),
                before,
                "the trim over {range:?} (a line: {line}) was bisected"
            );
            // (on the side, the seam snap's reach: [`trim_cap`], #20643)
            assert!(g <= trim_cap(s), "{g:e}");
            let sampled = sampled_same_parameter_gap(&k, s, curve, trim, *range);
            assert!(g >= sampled, "{g:e} under the sampled {sampled:e}");
            lines += usize::from(line);
        }
        assert!(lines >= 2, "{lines} seam line trims bounded");
    }

    /// #22034: no pass-over at equal parameters. The side's circle with the row's control
    /// points but its knots moved (each knot `t` to `t² (3 − 2t)`, so the arcs run at other
    /// speeds) is the same set as the row: a reparametrisation `ψ` takes one onto the other,
    /// and the structured bound of a trim's image (`ψ` free) is a rounding. At equal parameters
    /// the two are far apart, and the bound there counts it: it is at least the gap sampled at
    /// equal parameters, over `1e-5`, so a carried trim like it is turned away.
    #[test]
    fn a_same_parameter_bound_counts_how_far_the_structured_map_slips() {
        let (k, s, net, _) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        // (the row at the top, as `side_row` takes it, over the moved knots)
        let knots = net
            .u
            .iter()
            .map(|x| x / u1)
            .map(|t| t * t * (3.0 - 2.0 * t))
            .collect();
        let cps = (0..net.nu)
            .map(|i| {
                let q = &net.pw[4 * (2 * i + 1)..4 * (2 * i + 2)];
                CPoint::new(q[0], q[1], q[2], q[3])
            })
            .collect();
        let slipped = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(cps, net.p, knots).unwrap(),
            3,
            false,
        ));
        let trim = iso_trim(u0, u1, v1);
        let (cn, wn, probed) = trim_data(&k, &s, &slipped, &trim, [0.0, 1.0]);
        let free = structured_trim_gap_bound(&cn, &wn, &probed, [0.0, 1.0]).unwrap();
        assert!(free <= 1e-9, "{free:e}");
        let sampled = sampled_same_parameter_gap(&k, &s, &slipped, &trim, [0.0, 1.0]);
        assert!(sampled > 1e-2, "{sampled:e}");
        let same = structured_trim_gap_bound_as(&k, &cn, &wn, &probed, [0.0, 1.0], true)
            .unwrap()
            .unwrap();
        assert!(same >= sampled, "{same:e} under the sampled {sampled:e}");
        let gate = trim_same_parameter_gap_bound_within(
            &k,
            &s,
            &slipped,
            &trim,
            [0.0, 1.0],
            1e-8,
            [1e-5, 1e-5],
            Some(MAX_PIECES),
        )
        .unwrap();
        assert!(gate.is_some_and(|g| g > 1e-5), "{gate:?}");
    }

    /// #22273: a circle split into pieces, as a boolean splits a cylinder's end circle where it
    /// meets the other operand, keeps the structure of the piece of its cylinder's end row each
    /// piece lies on, not of the whole row. Each piece's trim, the iso line along its piece of
    /// the row, is proved within `1e-5` at equal parameters by the control points of the row's
    /// restriction at once: no bisection runs (before, each was bisected down to just under
    /// `1e-5`, about 8 ms a piece), and each bound is at least the gap sampled at equal
    /// parameters. The same pieces under trims slid along the row by a thousandth of it, or
    /// running against it, are as near as sets but far at equal parameters, and their bounds
    /// count it.
    #[test]
    fn the_pieces_of_a_split_circle_are_proved_on_their_pieces_of_the_row_without_bisecting() {
        let (k, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let trim = |a: f64, b: f64, lo: f64, hi: f64| {
            let c = NurbCurve::new(
                vec![CPoint::new(a, v1, 0.0, 1.0), CPoint::new(b, v1, 0.0, 1.0)],
                1,
                vec![lo, lo, hi, hi],
            )
            .unwrap();
            Curve::from(BSplineCurve::from_nurb(c, 2, false))
        };
        let at = |t: f64| u0 + (u1 - u0) * t;
        let bound = |curve: &Curve, trim: &Curve, range: [f64; 2]| {
            trim_same_parameter_gap_bound_within(
                &k,
                &s,
                curve,
                trim,
                range,
                1e-8,
                [1e-5, 1e-5],
                Some(MAX_PIECES),
            )
            .unwrap()
            .unwrap()
        };
        // (the circle cut at 0.401 and 0.901: a piece over two quarters' parts, one across a
        // quarter's end, one within a quarter)
        let mut first = Curve::from(row);
        let mut middle = first.create_by_split_curve(&k, 0.401).unwrap();
        let last = middle.create_by_split_curve(&k, 0.901).unwrap();
        for (piece, range) in [
            (&first, [0.0, 0.401]),
            (&middle, [0.401, 0.901]),
            (&last, [0.901, 1.0]),
        ] {
            let (lo, hi) = (range[0], range[1]);
            let along = trim(at(lo), at(hi), lo, hi);
            let before = k.work_counters.same_parameter_bisections();
            let g = bound(piece, &along, range);
            assert_eq!(
                k.work_counters.same_parameter_bisections(),
                before,
                "the piece over {range:?} was bisected"
            );
            // (the seam snap's reach: [`trim_cap`], #20643)
            assert!(g <= trim_cap(&s), "{range:?}: {g:e}");
            let sampled = sampled_same_parameter_gap(&k, &s, piece, &along, range);
            assert!(
                g >= sampled,
                "{range:?}: {g:e} under the sampled {sampled:e}"
            );
            // (slid along the row, within it, and run against it)
            let slide = if lo == 0.0 { 1e-3 } else { -1e-3 } * (u1 - u0);
            for wrong in [
                trim(at(lo) + slide, at(hi) + slide, lo, hi),
                trim(at(hi), at(lo), lo, hi),
            ] {
                let sampled = sampled_same_parameter_gap(&k, &s, piece, &wrong, range);
                assert!(sampled > 1e-5, "{range:?}: {sampled:e}");
                let g = bound(piece, &wrong, range);
                assert!(
                    g >= sampled,
                    "{range:?}: {g:e} under the sampled {sampled:e}"
                );
            }
        }
    }

    /// A trim along a piece of a row whose ends are a rounding off the row's knots, as a screw
    /// sweep's flank holds one along each half turn beside a cylinder piece (stress corpus
    /// S0252, a modelled M100x6 thread): the quarter of a cylinder's end circle between two of
    /// its row's knots, under the iso line from an ulp before the first knot to an ulp past
    /// the second. It is proved on that piece of the row by its control points at once, the
    /// ends' move counted: at equal parameters and as sets (where the stitch's and a move's
    /// covers ask), no bisection runs, and the bound at equal parameters is at least the gap
    /// sampled there. Cut at the trim's own ends, the piece began and ended with a segment an
    /// ulp wide, its segments no longer faced the circle's, and each trim was bisected down to
    /// just under its tolerance.
    #[test]
    fn a_trim_along_a_piece_of_a_row_a_rounding_off_its_knots_is_proved_on_the_piece() {
        let (k, s, net, row) = side_row();
        let v1 = net.v[net.v.len() - 1];
        // (the row's first two interior knots, and the circle's parameters there: its knots
        // are the row's over the last)
        let mut knots = net.u.clone();
        knots.dedup();
        let (k1, k2, last) = (knots[1], knots[2], knots[knots.len() - 1]);
        let range = [k1 / last, k2 / last];
        let mut piece = Curve::from(row)
            .create_by_split_curve(&k, range[0])
            .unwrap();
        let _rest = piece.create_by_split_curve(&k, range[1]).unwrap();
        let c = NurbCurve::new(
            vec![
                CPoint::new(k1.next_down(), v1, 0.0, 1.0),
                CPoint::new(k2.next_up(), v1, 0.0, 1.0),
            ],
            1,
            vec![range[0], range[0], range[1], range[1]],
        )
        .unwrap();
        let along = Curve::from(BSplineCurve::from_nurb(c, 2, false));
        let (same, fitted) = (
            k.work_counters.same_parameter_bisections(),
            k.work_counters.fitted_trim_bisections(),
        );
        let at_equal = trim_same_parameter_gap_bound_within(
            &k,
            &s,
            &piece,
            &along,
            range,
            1e-8,
            [1e-5, 1e-5],
            Some(MAX_PIECES),
        )
        .unwrap()
        .unwrap();
        let as_sets = trim_image_gap_bound_within(
            &k,
            &s,
            &piece,
            &along,
            range,
            1e-8,
            [1e-5, f64::INFINITY],
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (
                k.work_counters.same_parameter_bisections(),
                k.work_counters.fitted_trim_bisections()
            ),
            (same, fitted),
            "a bound was bisected"
        );
        assert!(
            at_equal <= 1e-9 && as_sets <= 1e-9,
            "{at_equal:e}, {as_sets:e}"
        );
        let sampled = sampled_same_parameter_gap(&k, &s, &piece, &along, range);
        assert!(
            at_equal >= sampled,
            "{at_equal:e} under the sampled {sampled:e}"
        );
    }

    /// #22273 review: a trim along a piece of a row that keeps the whole row's structure. The
    /// net is `S(u, v) = (u, v, u²)` over `u` in `[0, 2]`, a chain of two quadratic spans with
    /// unit weights (degree 1 in `v`). The curve is its row at `v = 0.5` restricted to
    /// `[0.5, 1.5]`: still two quadratic spans with unit weights, so the full row's check
    /// ([`iso_row_gap_bound`]) applies. It compares the piece's ends with the row's
    /// (`u = 1.5` against `u = 2`) and gives a bound over 1, while the gap at equal
    /// parameters is 0. The row piece's certificate is tried as well, and the trim is proved
    /// at a rounding with no bisection. A trim slid along the row is still bounded at least
    /// its sampled gap.
    #[test]
    fn a_trim_along_a_piece_of_a_row_with_the_whole_rows_structure_is_proved_on_the_piece() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        let k = Kernel::default();
        // (the Bézier points of `u²` over `[0, 1]` and `[1, 2]`: `a², a b, b²`)
        let xz = [(0.0, 0.0), (0.5, 0.0), (1.0, 1.0), (1.5, 2.0), (2.0, 4.0)];
        let net: Vec<Vec<CPoint>> = xz
            .iter()
            .map(|&(x, z)| {
                [0.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x, y, z, 1.0))
                    .collect()
            })
            .collect();
        let s = NurbSurface::new(
            net,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let s = Surface::from(BSplineSurface::from_nurb(s, false));
        // (the row at `v = 0.5` over `[0.5, 1.0]` and `[1.0, 1.5]`)
        let xz = [
            (0.5, 0.25),
            (0.75, 0.5),
            (1.0, 1.0),
            (1.25, 1.5),
            (1.5, 2.25),
        ];
        let cps = xz
            .iter()
            .map(|&(x, z)| CPoint::new(x, 0.5, z, 1.0))
            .collect();
        let knots = vec![0.5, 0.5, 0.5, 1.0, 1.0, 1.5, 1.5, 1.5];
        let curve = Curve::from(BSplineCurve::from_nurb(
            NurbCurve::new(cps, 2, knots).unwrap(),
            3,
            false,
        ));
        let range = [0.5, 1.5];
        let trim = |a: f64, b: f64| {
            let c = NurbCurve::new(
                vec![CPoint::new(a, 0.5, 0.0, 1.0), CPoint::new(b, 0.5, 0.0, 1.0)],
                1,
                vec![0.5, 0.5, 1.5, 1.5],
            )
            .unwrap();
            Curve::from(BSplineCurve::from_nurb(c, 2, false))
        };
        let bound = |trim: &Curve| {
            trim_same_parameter_gap_bound_within(
                &k,
                &s,
                &curve,
                trim,
                range,
                1e-8,
                [1e-5, 1e-5],
                Some(MAX_PIECES),
            )
            .unwrap()
            .unwrap()
        };
        let along = trim(0.5, 1.5);
        // (the full row's check applies and misses the cutoff)
        let (cn, wn, probed) = trim_data(&k, &s, &curve, &along, range);
        let full = structured_trim_gap_bound_as(&k, &cn, &wn, &probed, range, true)
            .unwrap()
            .unwrap();
        assert!(full > 1.0, "{full:e}");
        let before = k.work_counters.same_parameter_bisections();
        let g = bound(&along);
        assert_eq!(
            k.work_counters.same_parameter_bisections(),
            before,
            "the trim along the piece was bisected"
        );
        assert!(g <= 1e-9, "{g:e}");
        let sampled = sampled_same_parameter_gap(&k, &s, &curve, &along, range);
        assert!(g >= sampled, "{g:e} under the sampled {sampled:e}");
        // (slid along the row by a hundredth: far at equal parameters, and counted)
        let slid = trim(0.51, 1.51);
        let sampled = sampled_same_parameter_gap(&k, &s, &curve, &slid, range);
        assert!(sampled > 1e-5, "{sampled:e}");
        let g = bound(&slid);
        assert!(g >= sampled, "{g:e} under the sampled {sampled:e}");
    }

    /// #22273 review: the row piece's certificate polls the kernel's cancel token as it works
    /// through the row, and a cancellation there is the kernel's cancelled status, not a
    /// missing bound. On a piece of a split circle, the certificate alone polls, and a token
    /// cancelled from any of its polls cancels it. The whole same-parameter bound, whose answer
    /// is that certificate, is cancelled from each of its polls too, and answers one poll
    /// later.
    #[test]
    fn a_cancellation_during_the_row_piece_certificate_cancels_the_bound() {
        use crate::kernel::CancelToken;
        let (k0, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let at = |t: f64| u0 + (u1 - u0) * t;
        // (the circle's piece over `[0.401, 0.901]`)
        let mut piece = Curve::from(row).create_by_split_curve(&k0, 0.401).unwrap();
        let _last = piece.create_by_split_curve(&k0, 0.901).unwrap();
        let range = [0.401, 0.901];
        let c = NurbCurve::new(
            vec![
                CPoint::new(at(range[0]), v1, 0.0, 1.0),
                CPoint::new(at(range[1]), v1, 0.0, 1.0),
            ],
            1,
            vec![range[0], range[0], range[1], range[1]],
        )
        .unwrap();
        let along = Curve::from(BSplineCurve::from_nurb(c, 2, false));
        let (cn, wn, probed) = trim_data(&k0, &s, &piece, &along, range);
        let with = |token: CancelToken| {
            let mut k = Kernel::default();
            k.set_cancel_token(token);
            k
        };
        let cancelled = |r: &Result<Option<f64>>| matches!(r, Err(e) if e.is_cancelled());
        // (the certificate on its own)
        let counting = CancelToken::after_polls(u64::MAX);
        let k = with(counting.clone());
        let g = row_piece_trim_gap_bound(&k, &cn, &wn, &probed, range).unwrap();
        assert!(g.is_some_and(|g| g <= 1e-9), "{g:?}");
        let polls = counting.polls().unwrap();
        assert!(polls >= 2, "{polls} polls");
        for n in 1..=polls {
            let k = with(CancelToken::after_polls(n));
            let r = row_piece_trim_gap_bound(&k, &cn, &wn, &probed, range);
            assert!(cancelled(&r), "cancelled from poll {n} of {polls}: {r:?}");
        }
        // (the whole bound, answered by the certificate with no bisection)
        let gate = |k: &Kernel| {
            trim_same_parameter_gap_bound_within(
                k,
                &s,
                &piece,
                &along,
                range,
                1e-8,
                [1e-5, 1e-5],
                Some(MAX_PIECES),
            )
        };
        let counting = CancelToken::after_polls(u64::MAX);
        let k = with(counting.clone());
        let g = gate(&k).unwrap();
        assert!(g.is_some_and(|g| g <= 1e-9), "{g:?}");
        assert_eq!(k.work_counters.same_parameter_bisections(), 0);
        let total = counting.polls().unwrap();
        assert!(total >= polls, "{total} polls, the certificate's {polls}");
        for n in 1..=total {
            let r = gate(&with(CancelToken::after_polls(n)));
            assert!(cancelled(&r), "cancelled from poll {n} of {total}: {r:?}");
        }
        let r = gate(&with(CancelToken::after_polls(total + 1)));
        assert!(
            r.as_ref().is_ok_and(|g| g.is_some_and(|g| g <= 1e-9)),
            "{r:?}"
        );
    }

    /// #20373: the same side with `u` and `v` swapped (as the stitch's orientation leaves an
    /// extrude's side: degree 1 in `u`, the circle along `v`), with the trim's coordinates
    /// swapped, is the same surface under the same trim, and its iso trim gets the same bound
    /// bit for bit, whole and halfway; a trim that is not along the swapped side is left to
    /// the bisection.
    #[test]
    fn a_circles_iso_trim_on_its_swapped_cylinder_gets_the_same_bound() {
        let (k, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let curve = Curve::from(row);
        for end in [u1, 0.5 * u1] {
            let trim = iso_trim(u0, end, v1);
            let (cn, wn, probed) = trim_data(&k, &s, &curve, &trim, [0.0, 1.0]);
            let g = structured_trim_gap_bound(&cn, &wn, &probed, [0.0, 1.0]).unwrap();
            let (swapped, sw) = swapped_uv(&probed, &wn).unwrap();
            assert_eq!((swapped.p, swapped.q), (probed.q, probed.p));
            let h = structured_trim_gap_bound(&cn, &sw, &swapped, [0.0, 1.0]).unwrap();
            assert_eq!(g.to_bits(), h.to_bits());
            // (the unswapped trim on the swapped net runs across its circle, not along it)
            assert_eq!(
                structured_trim_gap_bound(&cn, &wn, &swapped, [0.0, 1.0]),
                None
            );
        }
    }

    /// #20373: no pass-over. A curve moved off the cylinder's side (one control point 1e-3
    /// out along z), or a cap trim moved off its circle (one point 1e-3 of the UV domain
    /// along u), still shows its gap through the structured bound, above `1e-5`.
    #[test]
    fn a_structured_trim_bound_still_sees_a_moved_curve_or_trim() {
        use crate::surface::surface::SurfaceKind;
        let (k, s, net, row) = side_row();
        let (u0, u1) = (net.u[0], net.u[net.u.len() - 1]);
        let v1 = net.v[net.v.len() - 1];
        let trim = iso_trim(u0, u1, v1);
        let (cn, wn, probed) = trim_data(&k, &s, &Curve::from(row), &trim, [0.0, 1.0]);
        let moved = Curve::from(moved(&cn, 1e-3));
        let (mn, _, _) = trim_data(&k, &s, &moved, &trim, [0.0, 1.0]);
        let g = structured_trim_gap_bound(&mn, &wn, &probed, [0.0, 1.0]).unwrap();
        assert!(g >= 1e-3, "{g:e}");
        let (k, trims) = cylinder_trims();
        let mut seen = false;
        for (s, curve, trim, range) in &trims {
            if !matches!(s.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            let (cn, wn, net) = trim_data(&k, s, curve, trim, *range);
            if structured_trim_gap_bound(&cn, &wn, &net, *range).is_none() {
                continue;
            }
            seen = true;
            let du = 1e-3 * (net.u[net.u.len() - 1] - net.u[0]);
            let mut off = wn.clone();
            off.pw[4] += du * off.pw[7];
            let g = structured_trim_gap_bound(&cn, &off, &net, *range);
            assert!(g.is_some_and(|g| g > 1e-5), "{g:?}");
            // (weights not the curve's: not the plane's case; the composed one (#22930)
            // bounds the trim at equal parameters, and sees its gap)
            let mut w = wn.clone();
            let last = w.pw.len() - 1;
            w.pw[last] *= 2.0;
            let g = structured_trim_gap_bound(&cn, &w, &net, *range);
            assert!(g.is_none_or(|g| g > 1e-5), "{g:?}");
            assert_eq!(
                structured_trim_gap_bound(&cn, &wn, &net, [range[0], range[0]]),
                None
            );
        }
        assert!(seen);
    }

    /// An offset face over a net extended past its natural domain (#18219, the hood's
    /// thickened faces): the base is the plane `z = 0.3 x + 0.2 y`, its `u` direction degree
    /// 1 with knots at the extension's joins (`0` and `1`, C^0 by count, straight in fact),
    /// offset by `d`. The trim crosses the join `u = 1` at a slant of `1e-13` (as the hood's
    /// trims run along it, crossing it by rounding), its curve the image lifted `lift` along
    /// the plane's normal, so the gap is `lift` exactly. The bound encloses the offset from
    /// its base's net, takes the part of a box past the join as an overshoot (the join is
    /// crossed at every depth), and closes: at least `lift` and within twice the bisection's
    /// closing fraction of it. Before #18219 an offset face had no bound at all.
    #[test]
    fn an_offset_trim_along_its_nets_join_is_bounded_by_its_distance() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (e, d, lift) = (0.02, -2.0, 1.0e-4);
        let z = |x: f64, y: f64| 0.3 * x + 0.2 * y;
        let net: Vec<Vec<CPoint>> = [-e, 0.0, 1.0, 1.0 + e]
            .iter()
            .map(|&x| {
                [0.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x, y, z(x, y), NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            1,
            1,
            vec![-e, -e, 0.0, 1.0, 1.0 + e, 1.0 + e],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, d, base, true, 0).unwrap());
        let n = Point3::new(-0.3, -0.2, 1.0);
        let n = n * (1.0 / n.length());
        let (a, b) = ([1.0 - 1.0e-13, 0.1], [1.0 + 1.0e-13, 0.9]);
        let on = |f: [f64; 2]| surface.evaluate_point(&k, Point2::new(f[0], f[1])).unwrap();
        // (the offset is a plane parallel to the base: `n` is its normal too)
        let uv = |f: [f64; 2]| Point3::new(f[0], f[1], 0.0);
        let trim = Top::curve(&[uv(a), uv(b)], 1, vec![0.0, 0.0, 1.0, 1.0], true);
        let curve = Top::curve(
            &[on(a) + n * lift, on(b) + n * lift],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            false,
        );
        let g = trim_image_gap_bound(&k, &surface, &curve, &trim, [0.0, 1.0], 1e-9)
            .unwrap()
            .expect("the offset face's trim is enclosed");
        assert!(
            g >= lift && g <= lift * (1.0 + 2.0 * CLOSE_RELATIVE) + 1.0e-12,
            "{g:e} against {lift:e}"
        );
        // a box whose part past the join `u = 1` runs on past the net's end `1 + e`: the
        // end's part would continue the last span while the part past the join continues
        // the one before it, so the continued surface would jump at the end (#18219 review)
        let probes = [Point2::new(0.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        let box_at = |hi: f64| V3 {
            x: Ivl::new(0.5, hi),
            y: Ivl::new(0.4, 0.6),
            z: Ivl::from(0.0),
        };
        assert!(net.partials(&[box_at(1.0 + 0.5 * e)], 1, 1).is_ok());
        assert!(net.partials(&[box_at(1.0 + 1.5 * e)], 1, 1).is_err());
    }

    /// The bound is to the image `S(W)` itself, not only to the surface's closure (#18219
    /// review), end to end: the base is flat for `u <= 1` and past it (degree 1
    /// in `u`, a C^0 join) tilts by `+10` degrees at `v = 0` and `v = 1` but by `-10`
    /// degrees at `v = 1/2` (its far row quadratic in `v`), offset by `d = 5`. The trim is at
    /// `u = 1.0005`, just past the join, at both ends and at `v* = 1/2` (an interior point of
    /// the one piece), and dips to about `0.97` between. Its curve is the flat sheet's plane
    /// continued, moved so it meets the image at the first end (and nearly at the other):
    /// `C = S~(W) + D`. Over the part past the join `g = C - S(W)` is `D - e` with
    /// `e = S(W) - S~(W)` the offset's jump, `+e` at the ends and nearly `-e` at `v*`, so the
    /// gap there is near `2 |e|` while the ends' gap is near 0: the bound must cover it. (Here the jump's enclosure over the
    /// piece's wide `v` range is loose enough that one `sup |e|` would too; the slack itself
    /// is checked against twice the jump where its enclosure is tight, in
    /// `an_offset_across_its_bases_kink_has_twice_its_jump_in_the_slack`.)
    #[test]
    fn a_trim_past_its_bases_kink_is_bounded_to_its_image_point() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (tilt, d) = (10.0_f64.to_radians().tan(), 5.0);
        let rows = [
            [[0.0, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 1.0, 0.0]],
            [[1.0, 0.0, 0.0], [1.0, 0.5, 0.0], [1.0, 1.0, 0.0]],
            [[2.0, 0.0, tilt], [2.0, 0.5, -3.0 * tilt], [2.0, 1.0, tilt]],
        ];
        let net: Vec<Vec<CPoint>> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|&[x, y, z]| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            1,
            2,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, d, base, true, 0).unwrap());
        let s_at = |u: f64, v: f64| surface.evaluate_point(&k, Point2::new(u, v)).unwrap();
        let flat = |u: f64, v: f64| Point3::new(u, v, d);
        // the tilt at v* is the other way: the jump there is nearly opposite the ends'
        let (e_end, e_mid) = (
            s_at(1.0005, 0.0) - flat(1.0005, 0.0),
            s_at(1.0005, 0.5) - flat(1.0005, 0.5),
        );
        assert!(e_end.dot(e_mid) < 0.0, "{e_end:?} {e_mid:?}");
        let us = [1.0005, 1.0005, 0.97, 1.0005, 1.0005, 0.97, 1.0005, 1.0005];
        let vs = [0.0, 0.1, 0.25, 0.45, 0.55, 0.75, 0.9, 1.0];
        let knots = vec![
            0.0,
            0.0,
            0.0,
            1.0 / 6.0,
            2.0 / 6.0,
            0.5,
            4.0 / 6.0,
            5.0 / 6.0,
            1.0,
            1.0,
            1.0,
        ];
        let trim_pts: Vec<Point3> = us
            .iter()
            .zip(vs)
            .map(|(&u, v)| Point3::new(u, v, 0.0))
            .collect();
        let curve_pts: Vec<Point3> = us
            .iter()
            .zip(vs)
            .map(|(&u, v)| flat(u, v) + e_end)
            .collect();
        let trim = Top::curve(&trim_pts, 2, knots.clone(), true);
        let curve = Top::curve(&curve_pts, 2, knots, false);
        let (cn, wn) = (
            NurbsCurve::of(curve.b_spline_base().unwrap()).unwrap(),
            NurbsCurve::of(trim.b_spline_base().unwrap()).unwrap(),
        );
        // the gap to the image point, sampled; at v* it is near twice the jump
        let mut gap = 0.0_f64;
        for i in 0..=1000 {
            let t = f64::from(i) / 1000.0;
            let w = trim.evaluate_point(&k, t).unwrap();
            gap = gap.max((curve.evaluate_point(&k, t).unwrap() - s_at(w.x, w.y)).length());
        }
        let w_mid = trim.evaluate_point(&k, 0.5).unwrap();
        assert!(w_mid.x > 1.0 && (w_mid.y - 0.5).abs() < 1e-12, "{w_mid:?}");
        assert!(
            gap > 1.5 * e_end.length(),
            "{gap:e} against {:e}",
            e_end.length()
        );
        let probes = [Point2::new(0.5, 0.5), Point2::new(1.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        let (ends, rest) = piece_bound(&cn, &wn, &net, 0.0, 1.0)
            .unwrap()
            .expect("the piece is enclosed");
        assert!(ends + rest >= gap, "{ends:e} + {rest:e} against {gap:e}");
    }

    /// A short part past a low knot line holding another knot (#18219 review): a cubic
    /// net with a C^0 line at `1` and a simple knot at `1.01`. The short part `[1, 1.02]`
    /// would be two spans of the surface while its jump is enclosed on one, so it is not
    /// taken; `[1, 1.005]` is one span and is.
    #[test]
    fn a_short_part_past_a_low_line_is_one_span() {
        let knots = [
            0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.01, 1.5, 2.0, 2.0, 2.0, 2.0,
        ];
        let n = knots.len() - 4;
        assert!(past_one_low_line(&knots, 3, n, 0.95, 1.02, 1, 0.1).is_none());
        let (cuts, sliver, lower) = past_one_low_line(&knots, 3, n, 0.95, 1.005, 1, 0.1).unwrap();
        assert!(!lower && (sliver - 0.005).abs() < 1e-15, "{cuts:?}");
    }

    /// An offset across a real kink of its base (#18219 review): the base is flat for
    /// `u <= 1` and at 45 degrees past it (degree 1, a C^0 join), offset by `d = 0.5`, so
    /// the offset's two sheets do not meet at the join. The trim dips twice from
    /// `u = 1.02` to `0.92` and back while `v` runs on; its curve is the flat sheet's
    /// plane continued, `Q~(W)`, moved by `D = Q_tilted(1.02, v) - Q~(1.02, v)`, so it lies
    /// on the tilted sheet where the trim is at 1.02 and runs `0.1` short of it, parallel
    /// to the flat sheet's plane, at each dip. A box across the join with its short part
    /// past it takes the flat sheet's polynomial continued there; were the offset's jump
    /// between the two not added, the bound would come out near the short part's slack
    /// (about 0.02) where the curve's distance to the surface is at least the smaller of
    /// its distances to the two sheets' planes at a dip (about 0.07). Any bound given is at
    /// least that; the slack assertion is the enclosure's proof, the end-to-end check only
    /// applies when a bound is given.
    #[test]
    fn an_offset_across_its_bases_kink_is_never_bounded_below_its_distance() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (c, sn, d) = (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.5,
        );
        let net: Vec<Vec<CPoint>> = [[0.0, 0.0], [1.0, 0.0], [1.0 + c, sn]]
            .iter()
            .map(|&[x, z]| {
                [0.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, d, base, true, 0).unwrap());
        // the tilted sheet at u = 1.02 and the flat sheet's plane continued there
        let tilted = Point3::new(1.0 + 0.02 * c - d * sn, 0.0, 0.02 * sn + d * c);
        let flat = Point3::new(1.02, 0.0, d);
        let shift = tilted - flat;
        assert!(
            (surface.evaluate_point(&k, Point2::new(1.02, 0.0)).unwrap() - tilted).length() < 1e-12
        );
        let us = [1.02, 0.82, 1.02, 0.82, 1.02];
        let vs = [0.1, 0.3, 0.5, 0.7, 0.9];
        let knots = vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
        let trim_pts: Vec<Point3> = us
            .iter()
            .zip(vs)
            .map(|(&u, v)| Point3::new(u, v, 0.0))
            .collect();
        let curve_pts: Vec<Point3> = us
            .iter()
            .zip(vs)
            .map(|(&u, v)| Point3::new(u, v, d) + shift)
            .collect();
        let trim = Top::curve(&trim_pts, 2, knots.clone(), true);
        let curve = Top::curve(&curve_pts, 2, knots, false);
        // at the first dip (t = 0.25, u = 0.92): its distances to the two sheets' planes
        let dip = curve.evaluate_point(&k, 0.25).unwrap();
        let n_tilted = Point3::new(-sn, 0.0, c);
        let to_tilted = (dip - tilted).dot(n_tilted).abs();
        let to_flat = (dip.z - d).abs();
        let lower = to_tilted.min(to_flat);
        assert!(lower > 0.06, "{lower:e}");
        // the enclosure itself: a box across the join whose upper part (0.02 of 0.1) is
        // past it takes the flat sheet's polynomial there, and its slack covers the
        // surface's own distance from that polynomial over the part (the sheets' jump), as
        // the bound's argument needs; without the jump it would be the overshoot's alone
        let probes = [Point2::new(0.5, 0.5), Point2::new(1.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        let w = V3 {
            x: Ivl::new(0.92, 1.02),
            y: Ivl::new(0.4, 0.6),
            z: Ivl::from(0.0),
        };
        let (_, slack) = net.partials(&[w], 2, 1).unwrap();
        let jump = (surface.evaluate_point(&k, Point2::new(1.02, 0.5)).unwrap()
            - Point3::new(1.02, 0.5, d))
        .length();
        assert!(slack >= jump && jump > 0.3, "{slack:e} against {jump:e}");
        if let Some(g) =
            trim_image_gap_bound(&k, &surface, &curve, &trim, [0.0, 1.0], 1e-9).unwrap()
        {
            assert!(g >= lower, "{g:e} against at least {lower:e}");
        }
    }

    /// The jump's deviation from its chord along a piece reaches twice its size (#18219
    /// review): one way at a piece's ends, the other way at a point between, as
    /// where the base's kink tilts one way at the ends' `v` and the other at that point's.
    /// The bound to the image point `S(W)` (`g = g~ - e`, `e = S(W) - S~(W)`) needs
    /// `|e - ℓe| <= 2 sup |e|` on top of the ends' gap and the term, so the slack holds twice
    /// the jump: the flat-and-45-degree kink at `u = 1` offset by `0.5`, a box whose upper
    /// part `[1, 1.02]` is past it, where the jump's enclosure is tight (it is the same all
    /// along `v`). Without the second `J` the slack is about `0.41` against `0.77`.
    #[test]
    fn an_offset_across_its_bases_kink_has_twice_its_jump_in_the_slack() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (c, sn, d) = (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.5,
        );
        let net: Vec<Vec<CPoint>> = [[0.0, 0.0], [1.0, 0.0], [1.0 + c, sn]]
            .iter()
            .map(|&[x, z]| {
                [0.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, d, base, true, 0).unwrap());
        let probes = [Point2::new(0.5, 0.5), Point2::new(1.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        let w = V3 {
            x: Ivl::new(0.92, 1.02),
            y: Ivl::new(0.4, 0.6),
            z: Ivl::from(0.0),
        };
        let jump = (0..=20)
            .map(|i| 1.0 + 0.001 * f64::from(i))
            .map(|u| {
                (surface.evaluate_point(&k, Point2::new(u, 0.5)).unwrap() - Point3::new(u, 0.5, d))
                    .length()
            })
            .fold(0.0_f64, f64::max);
        assert!(jump > 0.38, "{jump:e}");
        for (lk, continuity) in [(1, 0), (2, 1)] {
            let (_, slack) = net.partials(&[w], lk, continuity).unwrap();
            assert!(slack >= 2.0 * jump, "{slack:e} against twice {jump:e}");
        }
    }

    /// #23139: a join the net is smooth across has no jump to add. The base is
    /// `z = (x^2 / 4) (1 + y^2)` over `x` in `[0, 2]`, `y` in `[0, 1]`: two cubic Bézier
    /// pieces in `u` (a triple knot at `u = 1`: continuous by count only, one polynomial in
    /// fact), quadratic in `v`, offset by `0.5`. A box across the join with `1e-11` of it
    /// past the line, as a trim along the line crosses it by rounding, has that overshoot
    /// in its slack and nothing else: the overshoot times the offset's first partials,
    /// which are under 2 there (`|S_u| < 1.2`, the offset adding less than `0.5` times a
    /// curvature under 1) and enclosed under 10, so the slack is under `1e-10`, whether the
    /// box is `1/32` long or a half. The two sides enclosed apart differ by their
    /// enclosures' widths over the part's length along the line (`v` in `[0.4, 0.6]`):
    /// that gave a slack of `5e-2` here, and kept every piece of a trim along such a line
    /// open.
    #[test]
    fn an_offset_box_across_a_smooth_join_has_only_its_overshoot_in_the_slack() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let third = 1.0 / 3.0;
        let columns = [
            [0.0, 0.0],
            [third, 0.0],
            [2.0 * third, 1.0 / 12.0],
            [1.0, 0.25],
            [4.0 * third, 5.0 / 12.0],
            [5.0 * third, 2.0 * third],
            [2.0, 1.0],
        ];
        let net: Vec<Vec<CPoint>> = columns
            .iter()
            .map(|&[x, z]| {
                [[0.0, 1.0], [0.5, 1.0], [1.0, 2.0]]
                    .iter()
                    .map(|&[y, g]| CPoint::new(x, y, z * g, NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            3,
            2,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, 0.5, base, true, 0).unwrap());
        let probes = [Point2::new(0.5, 0.5), Point2::new(1.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        for length in [1.0 / 32.0, 0.5] {
            let w = V3 {
                x: Ivl::new(1.0 - length, 1.0 + 1.0e-11),
                y: Ivl::new(0.4, 0.6),
                z: Ivl::from(0.0),
            };
            let (_, slack) = net.partials(&[w], 2, 1).unwrap();
            assert!(slack > 0.0 && slack < 1.0e-10, "{slack:e} over {length}");
        }
    }

    /// A box whose upper end lies on its base's kink (#20544 review): the surface's value
    /// there is the next span's (the evaluator takes the span the knot starts), the tilted
    /// sheet's, `0.5 / sqrt(2)` high where the flat sheet's is `0.5`. The point values over
    /// `u` in `[31/32, 1]` hold it, and with the first partials (the flat sheet's polynomial
    /// over the box) the slack holds twice the jump at the end, as for a part past the line.
    #[test]
    fn an_offset_box_ending_on_its_bases_kink_holds_the_next_sheets_end() {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (c, sn, d) = (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.5,
        );
        let net: Vec<Vec<CPoint>> = [[0.0, 0.0], [1.0, 0.0], [1.0 + c, sn]]
            .iter()
            .map(|&[x, z]| {
                [0.0, 1.0]
                    .iter()
                    .map(|&y| CPoint::new(x, y, z, NOW))
                    .collect()
            })
            .collect();
        let base = NurbSurface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let surface = Surface::from(OffsetSurface::new(&k, d, base, true, 0).unwrap());
        let probes = [Point2::new(0.5, 0.5), Point2::new(1.5, 0.5)];
        let net = NetSurface::of_exact(&k, &surface, &probes)
            .unwrap()
            .unwrap();
        let w = V3 {
            x: Ivl::new(31.0 / 32.0, 1.0),
            y: Ivl::new(0.4, 0.6),
            z: Ivl::from(0.0),
        };
        let end = surface.evaluate_point(&k, Point2::new(1.0, 0.5)).unwrap();
        assert!((end.z - d * c).abs() < 1e-12, "{end:?}");
        let (p, _) = net.partials(&[w], 0, 0).unwrap();
        let v = p.at(0, 0);
        assert!(
            v.x.lo <= end.x && end.x <= v.x.hi && v.z.lo <= end.z && end.z <= v.z.hi,
            "{v:?} against {end:?}"
        );
        let jump = (end - Point3::new(1.0, 0.5, d)).length();
        assert!(jump > 0.38, "{jump:e}");
        for (lk, continuity) in [(1, 0), (2, 1)] {
            let (_, slack) = net.partials(&[w], lk, continuity).unwrap();
            assert!(slack >= 2.0 * jump, "{slack:e} against twice {jump:e}");
        }
    }

    /// Constant-`u` degree-1 ruling trims on a ruled NURBS net (`q == 1` and swapped `p == 1`)
    /// are bounded analytically by [`iso_ruling_gap_bound`] without bisection, on both full
    /// intervals and sub-intervals, and reject unequal `v`-row weights or slanted trims.
    #[test]
    fn iso_ruling_trim_on_cylinder_net_is_bounded_without_bisection() {
        let (_, trims) = cylinder_trims();
        let k = Kernel::default();
        // Extract the exact cylinder net from the cylinder side face.
        let cylinder_net = trims
            .iter()
            .find_map(|(s, curve, trim, range)| {
                matches!(s.kind(), crate::surface::surface::SurfaceKind::Cone(_))
                    .then(|| trim_data(&k, s, curve, trim, *range).2)
            })
            .expect("cylinder side net");
        let (u0, u1) = (
            *cylinder_net.u.get(cylinder_net.p).unwrap(),
            *cylinder_net.u.get(cylinder_net.nu).unwrap(),
        );
        let (v0, v1) = (
            *cylinder_net.v.first().unwrap(),
            *cylinder_net.v.last().unwrap(),
        );
        let u_mid = 0.5 * (u0 + u1);
        // Evaluate exact ruling endpoints via `NetSurface`.
        let cyl_surf = trims
            .iter()
            .find_map(|(s, _, _, _)| {
                matches!(s.kind(), crate::surface::surface::SurfaceKind::Cone(_)).then_some(s)
            })
            .unwrap();
        let p_a = cyl_surf.evaluate_point(&k, Point2::new(u_mid, v0)).unwrap();
        let p_b = cyl_surf.evaluate_point(&k, Point2::new(u_mid, v1)).unwrap();

        let make_line = |a: [f64; 3], b: [f64; 3], t0: f64, t1: f64| NurbsCurve {
            p: 1,
            n: 2,
            u: vec![t0, t0, t1, t1],
            pw: vec![a[0], a[1], a[2], 1.0, b[0], b[1], b[2], 1.0],
        };
        let cn = make_line([p_a.x, p_a.y, p_a.z], [p_b.x, p_b.y, p_b.z], 0.0, 1.0);
        let wn = make_line([u_mid, v0, 0.0], [u_mid, v1, 0.0], 0.0, 1.0);

        // Full interval, exact ruling: near-zero certified bound.
        let g = structured_trim_gap_bound(&cn, &wn, &cylinder_net, [0.0, 1.0]).unwrap();
        assert!(g <= 1.0e-12, "exact cylinder ruling bound too loose: {g:e}");

        // Sub-interval `[0.25, 0.75]` on the same line: still exact.
        let g_sub = structured_trim_gap_bound(&cn, &wn, &cylinder_net, [0.25, 0.75]).unwrap();
        assert!(
            g_sub <= 1.0e-12,
            "sub-interval ruling bound too loose: {g_sub:e}"
        );

        // Perturbed endpoint by `delta = 2e-4` along x: bound covers `delta` tightly.
        let delta = 2.0e-4;
        let cn_off = make_line(
            [p_a.x + delta, p_a.y, p_a.z],
            [p_b.x, p_b.y, p_b.z],
            0.0,
            1.0,
        );
        let g_off = structured_trim_gap_bound(&cn_off, &wn, &cylinder_net, [0.0, 1.0]).unwrap();
        assert!(
            g_off >= delta && g_off <= delta + 1.0e-11,
            "perturbed ruling bound {g_off:e} vs {delta:e}"
        );

        // Swapped `(u, v)` net (`net.p == 1`) with matching swapped trim gives the same bound.
        let (swap_net, swap_wn) = swapped_uv(&cylinder_net, &wn).unwrap();
        let g_swap = structured_trim_gap_bound(&cn_off, &swap_wn, &swap_net, [0.0, 1.0]).unwrap();
        assert!(
            g_swap >= delta && g_swap <= delta + 1.0e-11,
            "swapped ruling bound {g_swap:e} vs {delta:e}"
        );

        // Slanted trim (`u_a != u_b`) is not a ruling: the ruling case returns `None`. The
        // composed case (#22930) still bounds it, at least the gap at its first point.
        let wn_slant = make_line([u0, v0, 0.0], [u_mid, v1, 0.0], 0.0, 1.0);
        assert_eq!(
            iso_ruling_gap_bound(&cn, &wn_slant, &cylinder_net, [0.0, 1.0]),
            None
        );
        let first = (p_a - cyl_surf.evaluate_point(&k, Point2::new(u0, v0)).unwrap()).length();
        let g_slant = structured_trim_gap_bound(&cn, &wn_slant, &cylinder_net, [0.0, 1.0]);
        assert!(
            g_slant.is_none_or(|g| g >= first),
            "slanted trim bound {g_slant:?} below its first point's gap {first:e}"
        );

        // Unequal weights along `v` breaks affinity in `v`: the ruling case returns `None`.
        let mut bad_net = cylinder_net.clone();
        bad_net.pw[7] *= 1.5;
        assert_eq!(iso_ruling_gap_bound(&cn, &wn, &bad_net, [0.0, 1.0]), None);
    }

    /// Even with `budget == None`, [`bisect`] stops immediately with `Some(INFINITY)` as soon
    /// as any piece's endpoint gap exceeds `over`, without subdividing it further.
    #[test]
    fn bisect_without_budget_stops_at_once_when_ends_exceed_over() {
        let mut evals = 0usize;
        let res = bisect(
            [0.0, 1.0],
            4,
            &[],
            0.0,
            1.0e-13,
            [0.0, 1.0],
            None,
            |_a, _b| {
                evals += 1;
                // Huge unclosed slack so without the early `p.ends > over` cutoff it would
                // bisect deeply before closing.
                Ok(PieceBound::with(2.0, Ivl::from(100.0), 0.0))
            },
        )
        .unwrap();
        assert_eq!(res, Some(f64::INFINITY));
        assert_eq!(
            evals, 1,
            "should return on the very first piece, got {evals}"
        );
    }

    /// #22037: `cylinder.smb`'s cap 1 (a bilinear B-spline patch over the circle's bounding
    /// square, `u` in `[-5, 5]`, `v` in `[-10, 0]`), its edge 0 (a radius-5 circle of four
    /// rational quadratic quarters) and its UV trim, as the file stores them.
    fn flat_bspline_cap() -> (Kernel, Surface, Curve, Curve, [f64; 2]) {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::srf_basic::Surface as NurbSurface;
        let k = Kernel::default();
        let net = vec![
            vec![
                CPoint::new(0.0, 4.999999999999999, -5.0, NOW),
                CPoint::new(0.0, 4.999999999999999, 5.0, NOW),
            ],
            vec![
                CPoint::new(0.0, -5.000000000000001, -5.0, NOW),
                CPoint::new(0.0, -5.000000000000001, 5.0, NOW),
            ],
        ];
        let surface = NurbSurface::new(
            net,
            1,
            1,
            vec![-4.999999999999999, -4.999999999999999, 5.0, 5.0],
            vec![-10.0, -10.0, 0.0, 0.0],
        )
        .unwrap();
        let surface = Surface::from(BSplineSurface::from_nurb(surface, false));
        let knots = vec![
            0.0,
            0.0,
            0.0,
            7.653668647301794,
            7.653668647301794,
            15.30733729460359,
            15.30733729460359,
            22.96100594190538,
            22.96100594190538,
            30.61467458920718,
            30.61467458920718,
            30.61467458920718,
        ];
        let c = |x: f64, y: f64, z: f64, w: f64| CPoint::new(x, y, z, w);
        let curve = vec![
            c(0.0, 0.0, 5.0, 1.0),
            c(
                0.0,
                -3.5355339059327373,
                3.5355339059327378,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            c(0.0, -5.0, 0.0000000000000014, 1.0),
            c(
                0.0,
                -3.5355339059327373,
                -3.535533905932736,
                0.7071067811865475,
            ),
            c(0.0, -0.0000000000000008, -4.999999999999998, 1.0),
            c(
                0.0,
                3.535533905932737,
                -3.535533905932737,
                0.7071067811865475,
            ),
            c(0.0, 4.999999999999999, 0.0000000000000006, 1.0),
            c(
                0.0,
                3.535533905932737,
                3.535533905932737,
                0.7071067811865475,
            ),
            c(0.0, 0.0, 5.0, 1.0),
        ];
        let curve = NurbCurve::new(curve, 2, knots.clone()).unwrap();
        let curve = Curve::from(BSplineCurve::from_nurb(curve, 3, false));
        let trim = vec![
            c(0.0, 0.0, 0.0, 1.0),
            c(
                3.5355339059327373,
                0.0,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
            c(5.0, -4.999999999999998, 0.0, 1.0),
            c(
                3.5355339059327373,
                -7.071067811865474,
                0.0,
                0.7071067811865475,
            ),
            c(0.0000000000000008, -9.999999999999998, 0.0, 1.0),
            c(
                -3.535533905932737,
                -7.071067811865475,
                0.0,
                0.7071067811865475,
            ),
            c(-4.999999999999999, -4.999999999999999, 0.0, 1.0),
            c(
                -3.535533905932737,
                -0.0000000000000006,
                0.0,
                0.7071067811865475,
            ),
            c(0.0, 0.0, 0.0, 1.0),
        ];
        let trim = NurbCurve::new(trim, 2, knots).unwrap();
        let trim = Curve::from(BSplineCurve::from_nurb(trim, 2, false));
        (k, surface, curve, trim, [0.0, 30.61467458920718])
    }

    /// #22037: the circle trims of `cylinder.smb`'s flat B-spline cap are bounded below the
    /// bisection's floor, at least their sampled distance. The trim is tangent to the patch's
    /// four sides; its UV box over a piece, enclosed from the origin, ran past the side at
    /// `(0, -10)` by about `0.76 h` (the rational quotient's widening, first order in the
    /// piece), and that overshoot is the slack the bisection adds: it shrank only as fast as
    /// the pieces, and the pieces about each tangency ran out with the bound at `0.097` for
    /// the whole circle, `7.2e-4` for a quarter and no bound at all at equal parameters. Each
    /// piece closes once its second term with the slack is at most the floor `1e-7`, so the
    /// bound is at most the floor plus the largest end value (about `2e-13` here): `2e-7`.
    #[test]
    fn a_circle_trim_tangent_to_its_patchs_sides_is_bounded_below_the_floor() {
        let (k, s, curve, trim, range) = flat_bspline_cap();
        let knots = [
            range[0],
            7.653668647301794,
            15.30733729460359,
            22.96100594190538,
            range[1],
        ];
        let mut ranges = vec![range];
        ranges.extend(knots.windows(2).map(|w| [w[0], w[1]]));
        for r in ranges {
            let mut sampled = 0.0_f64;
            for i in 0..=1000 {
                let t = r[0] + (r[1] - r[0]) * f64::from(i) / 1000.0;
                let uv = trim.evaluate_point(&k, t).unwrap();
                let on = s.evaluate_point(&k, Point2::new(uv.x, uv.y)).unwrap();
                sampled = sampled.max((on - curve.evaluate_point(&k, t).unwrap()).length());
            }
            let image = trim_image_gap_bound(&k, &s, &curve, &trim, r, 1e-7)
                .unwrap()
                .unwrap();
            let same = trim_same_parameter_gap_bound_within(
                &k,
                &s,
                &curve,
                &trim,
                r,
                1e-7,
                [f64::NEG_INFINITY, f64::INFINITY],
                None,
            )
            .unwrap()
            .unwrap();
            for g in [image, same] {
                assert!(
                    sampled <= g && g <= 2e-7,
                    "{r:?}: {g:e} against {sampled:e}"
                );
            }
        }
    }

    /// #22037: the trim's UV box over a piece `h = 1.9e-3` long from its tangency with the
    /// patch's side `v = -10` (the knot `15.307...`, where `v' = 0`): enclosed from the
    /// origin it runs past the side by more than `0.5 h`; about the piece's middle
    /// ([`uv_derivs_over`]) by at most the curve's second-order term, `|v''| h^2 / 4` with
    /// `|v''|` about `0.2` (the UV circle of radius 5 at about unit speed), below `h^2`. The
    /// true trim stays within the side there (its control points do, and it is their convex
    /// combination), so either box holds it.
    #[test]
    fn a_trims_box_runs_past_the_side_it_touches_by_second_order() {
        let (_, _, _, trim, range) = flat_bspline_cap();
        let (wn, _) = exact_curve_data(&trim, true).unwrap().unwrap();
        let a = 15.30733729460359;
        let h = range[1] / 16384.0;
        let span = Ivl::new(a, a + h);
        let past = |w: &[V3<Ivl>]| (-10.0 - w[0].y.lo).max(0.0);
        let plain = past(&curve_derivs_over(&wn, span, 2, None).unwrap());
        let about = past(&uv_derivs_over(&wn, span, 2).unwrap());
        assert!(plain > 0.5 * h, "{plain:e} at h {h:e}");
        assert!(about <= h * h, "{about:e} at h {h:e}");
    }

    /// #22467: `TrimBoundMemo` stores each key once behind an `Arc`, evicts oldest entries
    /// once the retained byte budget (`TRIM_MEMO_MAX_BYTES`) is reached, and updates existing
    /// keys in place without growing entry count or retained bytes.
    #[test]
    fn trim_bound_memo_enforces_retained_byte_budget() {
        let mut memo = TrimBoundMemo::default();
        let make_key = |id: u64, words: usize| {
            let mut bits = Vec::new();
            bits.try_reserve_exact(words).unwrap();
            bits.push(id);
            bits.resize(words, id.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            TrimBoundKey { hash: id, bits }
        };
        // ~64 KiB per key: 300 keys (~19.2 MiB) exceed TRIM_MEMO_MAX_BYTES (16 MiB) well
        // before TRIM_MEMO_CAPACITY (4096).
        let words = 8192;
        for id in 0_u32..300 {
            memo.insert(
                make_key(u64::from(id), words),
                TrimBoundOutcome::Proved(f64::from(id)),
            );
            assert!(
                memo.retained_bytes() <= TRIM_MEMO_MAX_BYTES,
                "retained {} > {}",
                memo.retained_bytes(),
                TRIM_MEMO_MAX_BYTES
            );
        }
        assert!(memo.len() < 300, "byte cap did not evict oldest entries");
        assert_eq!(memo.get(&make_key(0, words)), None);
        assert_eq!(
            memo.get(&make_key(299, words)),
            Some(TrimBoundOutcome::Proved(299.0))
        );
        // Updating an existing key keeps len and retained_bytes unchanged.
        let len_before = memo.len();
        let bytes_before = memo.retained_bytes();
        memo.insert(make_key(299, words), TrimBoundOutcome::BudgetStopped);
        assert_eq!(memo.len(), len_before);
        assert_eq!(memo.retained_bytes(), bytes_before);
        assert_eq!(
            memo.get(&make_key(299, words)),
            Some(TrimBoundOutcome::BudgetStopped)
        );
    }

    /// #22820: `end_row` recognizes interior `C^0` knot rows of exact multiplicity `q`
    /// (such as a canonical sphere's equator at `v = 0.5`) as well as clamped end rows, while
    /// refusing simple interior knots where the surface does not interpolate a single row.
    #[test]
    fn end_row_finds_interior_c0_equator_of_sphere() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::sphere::Sphere;

        let k = Kernel::default();
        let sphere = Sphere::create_canonical(&k, &Axis2Placement::IDENTITY, 120.0).unwrap();
        let net = Nurbs::of(sphere.base().base()).unwrap();
        assert_eq!(net.q, 2);
        assert_eq!(net.nv, 5);
        // Clamped south pole (v = 0), interior double-knot equator (v = 0.5), clamped north pole (v = 1).
        let south = end_row(&net, 0.0).unwrap();
        let equator = end_row(&net, 0.5).unwrap();
        let north = end_row(&net, 1.0).unwrap();
        assert_eq!(south, (0..net.nu).map(|j| j * net.nv).collect::<Vec<_>>());
        assert_eq!(
            equator,
            (0..net.nu).map(|j| j * net.nv + 2).collect::<Vec<_>>()
        );
        assert_eq!(
            north,
            (0..net.nu).map(|j| j * net.nv + 4).collect::<Vec<_>>()
        );
        assert!(end_row(&net, 0.25).is_none());
        assert!(end_row(&net, 0.75).is_none());

        // Simple interior knot (multiplicity 1 < q = 2): not an interpolated control-point row.
        let simple_v = Nurbs {
            p: 1,
            q: 2,
            nu: 2,
            nv: 4,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            pw: vec![0.0; 2 * 4 * 4],
        };
        assert!(end_row(&simple_v, 0.5).is_none());
    }

    /// #22820: `pure_nurbs_flags` accepts NURBS-evaluated analytic surfaces (`Sphere`) and
    /// curves (`Ellipse`), so `TrimBoundMemo` caches sphere/ellipse move bounds across moves.
    #[test]
    fn pure_nurbs_flags_accepts_sphere_and_ellipse() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::nurbs::b_spline_surface::SurfParamType;
        use crate::surface::sphere::Sphere;

        let k = Kernel::default();
        let sphere = Sphere::create_canonical(&k, &Axis2Placement::IDENTITY, 120.0).unwrap();
        let iso = sphere
            .create_iso_parametric_curve(&k, SurfParamType::V, 0.5, None, true)
            .unwrap();
        let surface = Surface::from(sphere);
        let curve = iso.curve;
        let uv = Curve::from(
            BSplineCurve::create_line_segment(
                2,
                Point3::new(0.0, 0.5, 0.0),
                Point3::new(1.0, 0.5, 0.0),
            )
            .unwrap(),
        );
        assert!(pure_nurbs_flags(&surface, &curve, &uv).is_some());
    }
}
