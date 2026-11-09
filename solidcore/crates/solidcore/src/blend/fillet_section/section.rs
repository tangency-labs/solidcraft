//! The G^n section generator: the blend surface of the order-n section between the rails,
//! on stations at the knots of the rails and the centre line (and where a foot crosses a
//! face's knot line), certified span by span, and the section's order of contact with a
//! face at a rail.
//!
//! Fork: `SmGnSectionFSG` (`SM_GN_MAX_ORDER`, `SM_GN_DEFAULT_THUMBWEIGHT`, the constructor,
//! `IsValid`, `CreateSurface`, `CreateSurfaceFromCurves`, `Build`, `ContactOrderAt`,
//! `GetOrder`, `GetThumbweight`, `GetCertifiedBound`, `GetCertifiedSpans`) in
//! `source/SMLib/inc/SmFilletSolver.h` and `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@11ea2d9; the equal area thumbweight resolved at every station
//! 7102bc5, any thumbweight below 1 b9d6a72; a ball whose radius varies, `SmGnLawRound`
//! = [`GnLawRound`], 0d43b4c); see [`super`] for the table.
//!
//! Deviations (0d43b4c): the law round's leader is the edge's curve as a B-spline curve;
//! a line edge's is its NURB, where the fork's copy of an `SmLine` evaluates by the
//! line's own `Evaluate` (a rounding-level difference, not compared). `std::sort` of
//! the solver's stations by t is the stable `sort_by` with `<`'s order (they differ only
//! for equal t).

use super::certificate::{arc_rows, gn_certify_span, law_series, GnLaw, GnVarying, NurbsCurve};
use super::definition_box::{definition_enclosable, gn_def_face_of, GnDefs};
use super::form::{gn_form, gn_form_normalized, hermite_knots, hermite_points_sided};
use super::series::{Ser, Trunc, VSer};
use super::station::{
    gn_solve_station_with, smax, surface_partials, vec3, GnStation, Nurbs, StationOptions,
};
use super::tay::{
    acos_clamped, gn_area_sum, gn_half_angle, gn_invert, gn_target, gn_wedge_cos, GnAreaCells,
    Ring, Tay,
};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::intersect::surface_intersector::p1::TsectPnt;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, PI};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::p1::CanonicalSurfaceData;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use crate::nurbs::knots::Side;
use crate::surface::procedural_blend::chamfer::ChamferDefinition;
use crate::surface::procedural_blend::{count_area_root, BlendDefinitionInput, ProceduralBlend};
use crate::surface::surface::{Surface, SurfaceKind};
use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::BTreeMap;
use std::sync::Arc;

const OP: &str = "blend::fillet_section::GnSectionFsg";
const OP_BUILD: &str = "blend::fillet_section::GnSectionFsg::build";
const OP_CERTIFY: &str = "blend::fillet_section::GnSectionFsg::certify";

/// `SM_GN_MAX_ORDER`: the highest order of a G^n section.
pub const GN_MAX_ORDER: usize = 4;
/// `SM_GN_THUMBWEIGHT_EQUAL_AREA` (the fork's 6148773 = d01d7b0's 1947a45; 2, outside the
/// thumbweights below 1, since b9d6a72): a value outside the thumbweights' range that a
/// generator resolves at every station, from that station's wedge, to the w at which its
/// section, on the same rails, takes off the area the ball's circle does
/// ([`GnSectionFsg::equal_area_thumbweight`]; 7102bc5): w varies along the blend.
pub const GN_THUMBWEIGHT_EQUAL_AREA: f64 = 2.0;
/// `SM_GN_DEFAULT_THUMBWEIGHT`: the equal area one, [`GN_THUMBWEIGHT_EQUAL_AREA`] (0, the
/// section with no thumbweight, before 6148773).
pub const GN_DEFAULT_THUMBWEIGHT: f64 = GN_THUMBWEIGHT_EQUAL_AREA;

/// `SmGnSectionFSG`: a blend's cross-section of contact order n with its faces along the
/// rails, `n = 2..=GN_MAX_ORDER`, with thumbweight w. It needs a ball of one radius
/// (constant offsets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GnSectionFsg {
    /// `m_lOrder`: n.
    pub order: usize,
    /// `m_dThumbweight`: w, `w < 1`, or [`GN_THUMBWEIGHT_EQUAL_AREA`] (resolved per
    /// station, not here).
    pub thumbweight: f64,
    /// `m_dCertifiedBound`: the last surface built: its certified distance from the blend
    /// (-1: none yet).
    pub certified_bound: f64,
    /// `m_lCertifiedSpans`: and its number of spans along the rails.
    pub certified_spans: usize,
    /// `m_sArcs` (3aae1c4): order 1, how the ball's circles are built.
    pub arcs: ArcSection,
}

/// `SmArcSection` (the fork's 3aae1c4): how the order-1 section, the ball's circle, is
/// built: with circular arcs as far as the rails allow (`approximate_arcs`, within
/// `tol_percent_radius` of the radius) or exactly; mirrored about the rails' chord; or the
/// larger arc (`complement`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcSection {
    /// `bApproximateArcs`.
    pub approximate_arcs: bool,
    /// `dTolPercentRadius`.
    pub tol_percent_radius: f64,
    /// `bMirror`.
    pub mirror: bool,
    /// `bComplement`.
    pub complement: bool,
}

impl Default for ArcSection {
    /// `SmArcSection()`: `TRUE, 0.05, FALSE, FALSE`.
    fn default() -> Self {
        Self {
            approximate_arcs: true,
            tol_percent_radius: 0.05,
            mirror: false,
            complement: false,
        }
    }
}

impl ArcSection {
    /// The ball's exact circle: not approximated, mirrored or the larger arc, the order-1
    /// section `Build` takes (cc08236).
    #[must_use]
    pub const fn is_exact_circle(&self) -> bool {
        !self.approximate_arcs && !self.mirror && !self.complement
    }
}

/// The faces' surfaces under a generator's rails and their uv curves.
#[derive(Clone, Copy, Debug)]
pub struct GnFaces<'a> {
    /// The faces' surfaces (`pSurf1`, `pSurf2`).
    pub surfaces: [&'a Surface; 2],
    /// The rails' uv curves on them (`pUV1`, `pUV2`).
    pub uv: [&'a BSplineCurve; 2],
}

/// `SmGnLawRound` (0d43b4c): a round whose ball's radius varies, as the variable radius
/// solver rolls it: the ball's centre in the plane square to `leader` (the edge's curve)
/// at t, at the distance `law(t)` (its x) from both faces; `points` the solver's stations
/// (their t in `user_doubles[0]`, their feet in `curve_parameter` on the uv curves), which
/// seed `Build`'s.
#[derive(Clone, Copy, Debug)]
pub struct GnLawRound<'a> {
    /// `pLeader`.
    pub leader: &'a BSplineCurve,
    /// `pLaw`.
    pub law: &'a BSplineCurve,
    /// `pPoints`.
    pub points: &'a [TsectPnt],
    /// Not the fork's (#430): the t the round runs over, within the points' (`None`: the
    /// points', as the fork's); the points about its ends seed its end stations there.
    pub range: Option<Extent1d>,
}

/// What `Build` makes: the surface, and with a law and `bReplaceRails` the centre line it
/// sets (the balls' centres at the stations, over t; 0d43b4c).
type Built = (Surface, Option<BSplineCurve>);

/// The surface under an offset surface (`GetBaseSurface`), else the surface itself.
fn base_of(s: &Surface) -> &Surface {
    match s.kind() {
        SurfaceKind::OffsetSurface(o) => o.base_surface(),
        _ => s,
    }
}

/// `sm_GnSolveStation(apFace, rLeader, s, seed, adR, adSide, sTr, tol, rStation, pR,
/// n, pSides, NULL, NULL, bBefore)` as `Build` calls it since 3bc54a4: the section's jets
/// of order n and up are the face curves' normal parts (`pR`: a radius law's series,
/// 0d43b4c).
#[allow(clippy::too_many_arguments)]
fn solve_top(
    k: &Kernel,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    s: f64,
    seed: [f64; 4],
    r: [f64; 2],
    side: [f64; 2],
    tr: Trunc,
    tol: f64,
    n: usize,
    sides: Option<[i32; 4]>,
    before: bool,
    law: Option<&Ser<f64>>,
) -> Result<GnStation> {
    let opts = StationOptions {
        law,
        top: n,
        sides,
        before,
    };
    gn_solve_station_with(k, faces, center, s, seed, r, side, tr, tol, opts)
        .0
        .map(|(st, _)| st)
}

/// `GetNaturalInterval` as `(min, max)`.
fn interval(c: &BSplineCurve) -> (f64, f64) {
    let i = c.natural_interval();
    (i.min(), i.max())
}

/// `EvaluateNormal(uv, TRUE, TRUE, sN)` then `Unitize` (both `SER`).
fn unit_normal(k: &Kernel, s: &Surface, uv: Point2) -> Result<Vec3> {
    s.evaluate_normal(k, uv, Side::Left, Side::Left)?.unitize()
}

/// A station's foot on face f at sigma^i eps^e.
fn foot(st: &GnStation, f: usize, i: usize, e: usize) -> Vec3 {
    let [a, b] = &st.f;
    let fs = if f == 0 { a } else { b };
    let [x, y, z] = &fs.a;
    Vec3::new(x.at(i, e), y.at(i, e), z.at(i, e))
}

/// [`foot`] as the span ending at the station takes it (`sm_InF`, 3bc54a4).
fn foot_in(st: &GnStation, f: usize, i: usize, e: usize) -> Vec3 {
    st.in_f(f)
        .map_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN), |fs| {
            let [x, y, z] = &fs.a;
            Vec3::new(x.at(i, e), y.at(i, e), z.at(i, e))
        })
}

/// [`param`] as the span ending at the station takes it (`sm_InX`, 3bc54a4).
pub(crate) fn param_in(st: &GnStation, q: usize, i: usize, e: usize) -> f64 {
    st.in_x(q).map_or(f64::NAN, |x| x.at(i, e))
}

/// Coordinate q (x, y, z) of a vector.
fn vec3_at(v: Vec3, q: usize) -> f64 {
    match q {
        0 => v.x,
        1 => v.y,
        2 => v.z,
        _ => f64::NAN,
    }
}

/// A station's foot parameter q (u1, v1, u2, v2) at sigma^i eps^e.
pub(crate) fn param(st: &GnStation, q: usize, i: usize, e: usize) -> f64 {
    st.x.get(q).map_or(f64::NAN, |x| x.at(i, e))
}

/// `smos_Min(a, b)`: `(a < b) ? a : b`.
#[inline]
fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `SM_GN_W_MEMO`: the most equal area thumbweights [`ThumbweightMemo`] keeps before it
/// starts again. A guard on memory only: no case reaches it (one operation solved 9 at most
/// here, on the boss root; the fork's largest gate, the twisted wedge with its definition's
/// grading, 12150), and clearing it changes no result.
const GN_W_MEMO: usize = 65536;

/// `t_sWMemo` (b8ec238, smlib#109): the equal area thumbweights [`gn_equal_area_w`]
/// solved, by order and u's bits (the same u, the same root). Emptied as the outermost
/// [`OperationScope`] opens, and when it grows past [`GN_W_MEMO`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThumbweightMemo {
    roots: BTreeMap<(usize, u64), f64>,
}

impl ThumbweightMemo {
    /// The memo's thumbweights.
    #[must_use]
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// Whether it keeps none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

/// `SmGnSectionFSG::OperationScope` (b8ec238, smlib#109): the scope of a blend operation,
/// a fillet session's life. The kernel's memo of equal area thumbweights is emptied as
/// the outermost scope opens, so no operation reads another's; nested scopes keep it. The
/// depth is the kernel's shared counter (the fork's `thread_local` one), so a scope holds
/// no borrow of the kernel.
#[derive(Debug)]
pub struct OperationScope {
    depth: Arc<AtomicU32>,
}

impl OperationScope {
    /// Opens a scope on `k`.
    #[must_use]
    pub fn new(k: &Kernel) -> Self {
        let depth = Arc::clone(&k.blend_operation_scopes);
        if depth.fetch_add(1, Ordering::Relaxed) == 0 {
            *k.blend_w_memo.borrow_mut() = ThumbweightMemo::default();
        }
        Self { depth }
    }
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        self.depth.fetch_sub(1, Ordering::Relaxed);
    }
}

/// `sm_GnEqualAreaW` (b8ec238): [`gn_equal_area_w_solve`], each order and u solved once
/// in an operation, kept in the kernel's [`ThumbweightMemo`]. A solve that fails is not
/// kept. The solve is a pure function of exactly the key (the order and u), so a w kept for
/// one round or definition is the w every other round or definition of the operation would
/// solve for the same order and u: sharing the memo between them cannot change a result.
pub(crate) fn gn_equal_area_w(k: &Kernel, order: usize, u: f64) -> Result<f64> {
    let key = (order, u.to_bits());
    if let Some(&w) = k.blend_w_memo.borrow().roots.get(&key) {
        return Ok(w);
    }
    count_area_root(k);
    let w = gn_equal_area_w_solve(order, u)?;
    let mut memo = k.blend_w_memo.borrow_mut();
    if memo.roots.len() >= GN_W_MEMO {
        memo.roots.clear();
    }
    memo.roots.insert(key, w);
    Ok(w)
}

/// `sm_GnEqualAreaWSolve`: the thumbweight at which the order-n section takes off what
/// the ball's circle does, for half the angle u between the ball's radii
/// ([`GnSectionFsg::equal_area_thumbweight`]): the root of `I(w) = target(u)`.
fn gn_equal_area_w_solve(order: usize, u: f64) -> Result<f64> {
    const OP_EA: &str = "blend::fillet_section::GnSectionFsg::equal_area_thumbweight";
    if !((2..=GN_MAX_ORDER).contains(&order) && u > 0.0 && u < 0.5 * PI) {
        return Err(SolidError::invalid_input(OP_EA, "order or wedge angle"));
    }
    let target = gn_target(&Tay::<f64>::lift(u)).at(0);
    let mut lo = -1.0_f64;
    let mut hi = 1.0 - 1.0e-9;
    let mut held = target >= gn_area_sum::<f64>(order, hi);
    while held && gn_area_sum::<f64>(order, lo) < target {
        hi = lo;
        lo = 2.0 * lo - 1.0;
        held = lo >= -1.0e3;
    }
    if !held {
        return Err(SolidError::invalid_input(
            OP_EA,
            "no thumbweight gives this wedge's section the ball's circle's area",
        ));
    }
    let mut it = 0;
    while it < 200 && hi - lo > 1.0e-15 * (1.0 - lo) {
        let mid = 0.5 * (lo + hi);
        if gn_area_sum::<f64>(order, mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
        it += 1;
    }
    Ok(0.5 * (lo + hi))
}

/// `sm_GnStationThumbweight`: a station's equal area thumbweight and its s-derivative: x
/// from the station's feet and V to first order in s, w the root at its u,
/// `dw/ds = target' / I'(w)`.
fn gn_station_thumbweight(k: &Kernel, n: usize, rr: f64, st: &mut GnStation) -> Result<()> {
    let u = gn_half_angle(&gn_wedge_cos::<f64>(&st.f, &st.v, rr, 1));
    let w = gn_equal_area_w(k, n, u.at(0))?;
    let d = gn_area_sum(n, Tay::of_terms(w, 1.0));
    let falls = d.at(1) < 0.0;
    if !falls {
        // "the section's area does not fall with its thumbweight"
        return Err(SolidError::sm_err(OP));
    }
    let sw = gn_invert(&gn_target(&u), &d, w);
    st.w = w;
    st.wds = sw.at(1);
    Ok(())
}

/// The faces' NURBS data for the certificate (`Build`'s): every face a NURBS surface (an
/// analytic one by its net), and the faces that are blends by their definitions enclosed
/// over the certificate's boxes (WR-714, c5b6bb7).
///
/// # Errors
///
/// `SM_ERR` for a face that is not a NURBS surface, or a blend by its definition the
/// certificate does not enclose (only a round of one radius, on a round of one radius
/// between NURBS faces, with WR-714); those of [`Nurbs::of`] and of the enclosure.
pub(crate) fn certificate_faces<'a>(
    k: &Kernel,
    faces: [&'a Surface; 2],
    order: usize,
    has_law: bool,
) -> Result<([Nurbs; 2], GnDefs<'a>)> {
    let mut nurbs = Vec::new();
    // a face that is a blend by its definition: its definition enclosed over the
    // certificate's boxes (definition_partials_over_box), on the faces under it and its
    // leader, whatever its net (c5b6bb7, WR-714, #2570)
    let mut defs = GnDefs::default();
    for (f, face) in faces.into_iter().enumerate() {
        let Some(bs) = super::station::as_b_spline(k, face)? else {
            // "a face that is not a NURBS surface: its blend of order 2 and up cannot
            // be certified"
            return Err(SolidError::sm_err(OP));
        };
        if matches!(face.kind(), SurfaceKind::ProceduralBlend(_)) {
            // the certificate takes the face's partials to total order 7 in a round of
            // one radius (its fourth s-derivatives' Taylor form, gn_fourth_over), which
            // the definition's feet's series hold (SM_GN_MAXS); a law's touch circle,
            // or a G^n section, takes more: "a face that is a blend by its definition:
            // only a round of one radius is certified on it, and only on a round of one
            // radius between NURBS faces" (WR-714)
            let enclosed = order == 1 && !has_law && definition_enclosable(face);
            if !enclosed {
                return Err(SolidError::sm_err(OP));
            }
            if let Some(slot) = defs.faces.get_mut(f) {
                *slot = Some(gn_def_face_of(k, face)?);
            }
        }
        nurbs.push(Nurbs::of(&bs)?);
    }
    let mut it = nurbs.into_iter();
    let (Some(n0), Some(n1)) = (it.next(), it.next()) else {
        return Err(SolidError::sm_err(OP));
    };
    Ok(([n0, n1], defs))
}

/// A face's inner knots in u (`c = 0`) or v, each once.
pub(crate) fn inner_knots(nf: &Nurbs, c: usize) -> Vec<f64> {
    let (kv, deg) = if c == 0 { (&nf.u, nf.p) } else { (&nf.v, nf.q) };
    let mut inner: Vec<f64> = Vec::new();
    let mut m = deg + 1;
    while m + deg + 1 < kv.len() {
        let km = kv.get(m).copied().unwrap_or(f64::NAN);
        if inner.last().is_none_or(|&b| km > b) {
            inner.push(km);
        }
        m += 1;
    }
    inner
}

/// Whether a foot's parameter crosses the knot `dk` between two stations, `xa` to `xb`.
pub(crate) fn crosses_knot(xa: f64, xb: f64, dk: f64) -> bool {
    let margin = 1.0e-9 * (1.0 + dk.abs());
    (xa < dk - margin && xb > dk + margin) || (xa > dk + margin && xb < dk - margin)
}

/// The varying thumbweight's certificate data: I's enclosures on a grid over the
/// stations' equal area thumbweights (a little wider: stations put in later lie between
/// these); empty cells without the equal area thumbweight.
pub(crate) fn varying(
    n: usize,
    r: [f64; 2],
    stations: &[GnStation],
    equal_area: bool,
) -> GnVarying {
    let mut vary = GnVarying {
        n,
        r,
        cells: GnAreaCells::default(),
        w: [f64::INFINITY, f64::NEG_INFINITY],
    };
    if equal_area {
        let mut w_lo = f64::INFINITY;
        let mut w_hi = f64::NEG_INFINITY;
        for st in stations {
            w_lo = smin(w_lo, st.w);
            w_hi = smax(w_hi, st.w);
        }
        let pad = 0.05 * (w_hi - w_lo) + 1.0e-6 * (1.0 + smax(w_lo.abs(), w_hi.abs()));
        vary.cells
            .init(n, w_lo - pad, smin(w_hi + pad, 0.5 * (1.0 + w_hi)), 64);
    }
    vary
}

/// A blend's station system as `Build` solves it: the faces, the leader, the ball's
/// offsets and sides, the series' truncation, the section's order, whether the equal area
/// thumbweight is resolved at each station, and the radius law.
#[derive(Clone, Copy)]
pub(crate) struct StationSystem<'a> {
    /// The faces.
    pub(crate) faces: [&'a Surface; 2],
    /// The leader.
    pub(crate) leader: &'a BSplineCurve,
    /// The offsets.
    pub(crate) r: [f64; 2],
    /// The sides.
    pub(crate) side: [f64; 2],
    /// The truncation.
    pub(crate) tr: Trunc,
    /// n.
    pub(crate) n: usize,
    /// The equal area thumbweight, resolved per station.
    pub(crate) equal_area: bool,
    /// The radius law.
    pub(crate) law: Option<&'a GnLaw<'a>>,
}

impl StationSystem<'_> {
    /// The station at `s` from the feet `seed`, on the pieces after it along s (`sides`:
    /// a face's side where a foot is on one of its knot lines), its equal area thumbweight
    /// resolved.
    pub(crate) fn solve_after(
        &self,
        k: &Kernel,
        s: f64,
        seed: [f64; 4],
        sides: Option<[i32; 4]>,
    ) -> Result<GnStation> {
        let m = self.leader.evaluate_point(s)?;
        let law_r = law_series(self.law, s, self.tr.s(), false)?;
        let mut st = solve_top(
            k,
            self.faces,
            self.leader,
            s,
            seed,
            self.r,
            self.side,
            self.tr,
            1.0e-9 * smax(1.0, m.length()),
            self.n,
            sides,
            false,
            law_r.as_ref(),
        )?;
        if self.equal_area {
            gn_station_thumbweight(k, self.n, self.r[0] * self.r[1], &mut st)?;
        }
        Ok(st)
    }

    /// The station's series on the pieces before it, for the span ending there: a Hermite
    /// span interpolates the values and s-derivatives of its own pieces, which the
    /// certificate bounds (3bc54a4).
    pub(crate) fn solve_before(&self, k: &Kernel, st: &mut GnStation) -> Result<()> {
        let on = [0, 1, 2, 3].map(|q| param(st, q, 0, 0));
        let m = self.leader.evaluate_point(st.s)?;
        let law_r = law_series(self.law, st.s, self.tr.s(), true)?;
        let mut before = solve_top(
            k,
            self.faces,
            self.leader,
            st.s,
            on,
            self.r,
            self.side,
            self.tr,
            1.0e-9 * smax(1.0, m.length()),
            self.n,
            Some(st.before),
            true,
            law_r.as_ref(),
        )?;
        if self.equal_area {
            gn_station_thumbweight(k, self.n, self.r[0] * self.r[1], &mut before)?;
            st.wdsl = before.wds;
        }
        st.xl = before.x;
        st.fl = before.f;
        st.vl = before.v;
        st.sided = true;
        Ok(())
    }
}

/// The order-n section's control points across: `3 n + 2`, 3 for the ball's circle, 2 for
/// the chord (#2486).
pub(crate) const fn section_width(n: usize) -> usize {
    match n {
        0 => 2,
        1 => 3,
        _ => 3 * n + 2,
    }
}

/// The net of a blend's Hermite spans (`Build`'s): the spans' homogeneous rows joined
/// along s (a span's first row is the one before's last) on the stations `ss`, cubic along
/// s, of `ld` control points across, its control points the rows divided by their weights
/// and its weights scaled by `1 / sqrt(w_min w_max)`.
///
/// # Errors
///
/// `SM_ERR` for spans that are not the stations' (their number or size) or a weight that is
/// not positive; those of the canonical surface.
pub(crate) fn net_of_spans(
    ss: &[f64],
    spans: &[(Vec<Vec3>, Vec<f64>)],
    ld: usize,
) -> Result<BSplineSurface> {
    let (u_knots, u_mult) = hermite_knots(ss);
    let lnu = 3 * spans.len() + 1;
    if spans.len() + 1 != ss.len() {
        return Err(SolidError::sm_err(OP));
    }
    let mut homog: Vec<Vec3> = Vec::new();
    let mut wv: Vec<f64> = Vec::new();
    for (j, (h, w)) in spans.iter().enumerate() {
        let skip = if j == 0 { 0 } else { ld };
        homog.extend(h.iter().copied().skip(skip));
        wv.extend(w.iter().copied().skip(skip));
    }
    if homog.len() != lnu * ld || wv.len() != lnu * ld {
        return Err(SolidError::sm_err(OP));
    }
    let mut w_min = BIG_DOUBLE;
    let mut w_max = 0.0_f64;
    for &w in &wv {
        w_min = if w_min < w { w_min } else { w }; // smos_Min
        w_max = smax(w_max, w);
    }
    let positive = w_min > 0.0;
    if !positive {
        // "a weight of the section's rows is not positive"
        return Err(SolidError::sm_err(OP));
    }
    let w_scale = 1.0 / crate::geom::vector3d::sqrt(w_min * w_max);
    let mut cps: Vec<Point3> = Vec::new();
    let mut wts: Vec<f64> = Vec::new();
    for (&h, &w) in homog.iter().zip(&wv) {
        cps.push(h * (1.0 / w));
        wts.push(w * w_scale);
    }
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 3,
        v_degree: ld - 1,
        control_points: &cps,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &u_mult,
        v_multiplicities: &[ld, ld],
        u_knots: &u_knots,
        v_knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&wts),
        uv_domain: None,
    })
}

/// How a blend's net follows its stations: the section's order, its thumbweight (the
/// equal area one resolved per station), and whether the ball's radius follows a law. The
/// net is the Hermite interpolant along s of the section's rows at the stations: the arc's
/// rows (n = 1), each station's own form's rows (the equal area thumbweight), or the jets
/// themselves, which one form's rows are linear in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NetForm {
    /// n.
    pub(crate) n: usize,
    /// w, or [`GN_THUMBWEIGHT_EQUAL_AREA`].
    pub(crate) thumbweight: f64,
    /// The thumbweight is the equal area one, resolved per station.
    pub(crate) equal_area: bool,
    /// The ball's radius follows a law (n = 1).
    pub(crate) law: bool,
}

/// What a station gives the net's Hermite curves: per curve its value, its s-derivative
/// as the span starting there takes it (`d`) and as the one ending there does (`d_in`).
/// The curves are the rows `W_j P_j` with their weights `W_j` in `w*` (in x), or for one
/// fixed form the jets `c_ik` (index `f (n + 1) + k`, `w*` empty).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StationRows {
    v: Vec<Vec3>,
    d: Vec<Vec3>,
    d_in: Vec<Vec3>,
    w: Vec<Vec3>,
    wd: Vec<Vec3>,
    wd_in: Vec<Vec3>,
}

impl NetForm {
    /// The section's control points across ([`section_width`]).
    pub(crate) const fn width(&self) -> usize {
        section_width(self.n)
    }

    /// A station's values and s-derivatives of the curves the net interpolates.
    pub(crate) fn station_rows(&self, st: &GnStation) -> Result<StationRows> {
        let n = self.n;
        let ld = self.width();
        let mut out = StationRows::default();
        if n == 1 {
            // the ball's circle: the station's arc rows (sm_ArcRows) and their
            // s-derivatives, as the span starting there and the one ending there take them
            let o = [0.0_f64; 3];
            for l_in in [false, true] {
                let mut rho = [[Tay::<f64>::zero(); 3]; 2];
                let mut mc = [Tay::<f64>::zero(); 3];
                for q in 0..3 {
                    for (f, rf) in rho.iter_mut().enumerate() {
                        let value = foot(st, f, 0, 0);
                        let d = if l_in {
                            foot_in(st, f, 1, 0)
                        } else {
                            foot(st, f, 1, 0)
                        };
                        if let Some(x) = rf.get_mut(q) {
                            *x = Tay::of_terms(vec3_at(value, q), vec3_at(d, q));
                        }
                    }
                    // the centre's s-derivatives, as the span the rows are for takes them
                    let cv = st.v.a.get(q).map_or(f64::NAN, |c| c.at(0, 0));
                    let from = if l_in && st.sided { &st.vl } else { &st.v };
                    let cd = from.a.get(q).map_or(f64::NAN, |c| c.at(1, 0));
                    if let Some(x) = mc.get_mut(q) {
                        *x = Tay::of_terms(cv, cd);
                        if self.law {
                            x.c[2] = from.a.get(q).map_or(f64::NAN, |c| c.at(2, 0));
                        }
                    }
                }
                if self.law {
                    // a varying ball: the circle it touches the envelope along, about
                    // m + k m', k = (rho1 - m) . m' / m' . m' (0d43b4c)
                    let mut dm = [Tay::<f64>::zero(); 3];
                    for (dq, mq) in dm.iter_mut().zip(mc.iter_mut()) {
                        *dq = Tay::of_terms(mq.c[1], 2.0 * mq.c[2]);
                        mq.c[2] = 0.0;
                    }
                    let mut dd = Tay::<f64>::zero();
                    let mut bd = Tay::<f64>::zero();
                    for ((&dq, &mq), &r0q) in dm.iter().zip(&mc).zip(&rho[0]) {
                        dd = dd + dq * dq;
                        bd = bd + (r0q - mq) * dq;
                    }
                    let moving = dd.c[0] > 0.0;
                    if !moving {
                        // "the balls' centres stand still"
                        return Err(SolidError::sm_err(OP));
                    }
                    let kk = bd / dd;
                    for (mq, &dq) in mc.iter_mut().zip(&dm) {
                        *mq = *mq + kk * dq;
                        for c in mq.c.iter_mut().skip(2) {
                            *c = 0.0;
                        }
                    }
                }
                let [rho0, rho1] = &rho;
                let Some((h, w)) = arc_rows(rho0, rho1, &mc, o) else {
                    // "a section of the ball's circle is a half turn or more"
                    return Err(SolidError::sm_err(OP));
                };
                for (hj, wj) in h.iter().zip(&w).take(ld) {
                    let sd = Vec3::new(hj[0].at(1), hj[1].at(1), hj[2].at(1));
                    if l_in {
                        out.d_in.push(sd);
                        out.wd_in.push(Vec3::new(wj.at(1), 0.0, 0.0));
                        continue;
                    }
                    out.v.push(Vec3::new(hj[0].at(0), hj[1].at(0), hj[2].at(0)));
                    out.d.push(sd);
                    out.w.push(Vec3::new(wj.at(0), 0.0, 0.0));
                    out.wd.push(Vec3::new(wj.at(1), 0.0, 0.0));
                }
            }
        } else if self.equal_area {
            // the station's own form (over W_0): every row W_j P_j and weight W_j, their
            // values and s-derivatives there (the form's in w(s) and the jets'), as the span
            // starting here takes them, then as the one ending here (3bc54a4)
            for l_in in [false, true] {
                let wds = if l_in { st.in_wds() } else { st.wds };
                let sf = gn_form_normalized(n, Tay::of_terms(st.w, wds));
                for j in 0..ld {
                    let mut sv = Vec3::new(0.0, 0.0, 0.0);
                    let mut sd = Vec3::new(0.0, 0.0, 0.0);
                    for (f, gf) in sf.g.iter().enumerate() {
                        for (kk, gk) in gf.iter().enumerate().take(n + 1) {
                            let c = foot(st, f, 0, kk);
                            let c1 = if l_in {
                                foot_in(st, f, 1, kk)
                            } else {
                                foot(st, f, 1, kk)
                            };
                            let g = gk.get(j).copied().unwrap_or_else(|| Tay::lift(f64::NAN));
                            sv += c * g.at(0);
                            sd = sd + c * g.at(1) + c1 * g.at(0);
                        }
                    }
                    let wj = sf.w.get(j).copied().unwrap_or_else(|| Tay::lift(f64::NAN));
                    if l_in {
                        out.d_in.push(sd);
                        out.wd_in.push(Vec3::new(wj.at(1), 0.0, 0.0));
                        continue;
                    }
                    out.v.push(sv);
                    out.d.push(sd);
                    out.w.push(Vec3::new(wj.at(0), 0.0, 0.0));
                    out.wd.push(Vec3::new(wj.at(1), 0.0, 0.0));
                }
            }
        } else {
            // one section's form: its rows are linear in the jets, so the jets carry them
            for f in 0..2 {
                for kk in 0..=n {
                    out.v.push(foot(st, f, 0, kk));
                    out.d.push(foot(st, f, 1, kk));
                    out.d_in.push(foot_in(st, f, 1, kk));
                }
            }
        }
        Ok(out)
    }

    /// The net's span between stations `a` and `b` (`ra`, `rb` their
    /// [`NetForm::station_rows`]): its homogeneous control points `W P` and weights `W`,
    /// 4 along s by [`NetForm::width`] across, `[i * width + j]`.
    pub(crate) fn span(
        &self,
        a: &GnStation,
        ra: &StationRows,
        b: &GnStation,
        rb: &StationRows,
    ) -> (Vec<Vec3>, Vec<f64>) {
        let ld = self.width();
        let ss = [a.s, b.s];
        let hermite = |va: &[Vec3], da: &[Vec3], vb: &[Vec3], db: &[Vec3], dib: &[Vec3], q| {
            let at = |x: &[Vec3]| {
                x.get(q)
                    .copied()
                    .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
            };
            hermite_points_sided(
                &ss,
                &[at(va), at(vb)],
                &[at(da), at(db)],
                &[at(&ra.d_in), at(dib)],
            )
        };
        let mut homog = vec![Vec3::new(0.0, 0.0, 0.0); 4 * ld];
        let mut wv = vec![0.0_f64; 4 * ld];
        if self.n == 1 || self.equal_area {
            for j in 0..ld {
                let hp = hermite(&ra.v, &ra.d, &rb.v, &rb.d, &rb.d_in, j);
                let wp = {
                    let at = |x: &[Vec3]| {
                        x.get(j)
                            .copied()
                            .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
                    };
                    hermite_points_sided(
                        &ss,
                        &[at(&ra.w), at(&rb.w)],
                        &[at(&ra.wd), at(&rb.wd)],
                        &[at(&ra.wd_in), at(&rb.wd_in)],
                    )
                };
                for i in 0..4 {
                    if let (Some(h), Some(w), Some(&p), Some(q)) = (
                        homog.get_mut(i * ld + j),
                        wv.get_mut(i * ld + j),
                        hp.get(i),
                        wp.get(i),
                    ) {
                        *h = p;
                        *w = q.x;
                    }
                }
            }
        } else if self.n == 0 {
            // the chord (#2486): its rows are the feet, weights 1
            for j in 0..ld {
                let hp = hermite(&ra.v, &ra.d, &rb.v, &rb.d, &rb.d_in, j);
                for i in 0..4 {
                    if let (Some(h), Some(w), Some(&p)) =
                        (homog.get_mut(i * ld + j), wv.get_mut(i * ld + j), hp.get(i))
                    {
                        *h = p;
                        *w = 1.0;
                    }
                }
            }
        } else {
            // the jets' Hermite curves, then one form's rows of them
            let n = self.n;
            let form = gn_form(n, self.thumbweight);
            let jets: Vec<Vec<Vec3>> = (0..2 * (n + 1))
                .map(|q| hermite(&ra.v, &ra.d, &rb.v, &rb.d, &rb.d_in, q))
                .collect();
            let jet = |f: usize, kk: usize, i: usize| -> Vec3 {
                jets.get(f * (n + 1) + kk)
                    .and_then(|j| j.get(i))
                    .copied()
                    .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
            };
            for i in 0..4 {
                for j in 0..ld {
                    let mut p = Vec3::new(0.0, 0.0, 0.0);
                    for (f, gf) in form.g.iter().enumerate() {
                        for (kk, g) in gf.iter().enumerate().take(n + 1) {
                            p += jet(f, kk, i) * g.get(j).copied().unwrap_or(f64::NAN);
                        }
                    }
                    if let (Some(h), Some(w)) = (homog.get_mut(i * ld + j), wv.get_mut(i * ld + j))
                    {
                        *h = p;
                        *w = form.w.get(j).copied().unwrap_or(f64::NAN);
                    }
                }
            }
        }
        (homog, wv)
    }
}

impl GnSectionFsg {
    /// `SmGnSectionFSG(lOrder, dThumbweight)`.
    #[must_use]
    pub const fn new(order: usize, thumbweight: f64) -> Self {
        Self {
            order,
            thumbweight,
            certified_bound: -1.0,
            certified_spans: 0,
            arcs: ArcSection {
                approximate_arcs: true,
                tol_percent_radius: 0.05,
                mirror: false,
                complement: false,
            },
        }
    }

    /// The chord, order 0 (a chamfer; solidcore's, #2486): the straight section between
    /// the feet.
    #[must_use]
    pub const fn chord() -> Self {
        Self {
            order: 0,
            thumbweight: 0.0,
            certified_bound: -1.0,
            certified_spans: 0,
            arcs: ArcSection {
                approximate_arcs: true,
                tol_percent_radius: 0.05,
                mirror: false,
                complement: false,
            },
        }
    }

    /// `SmGnSectionFSG(const SmArcSection & crArcs)` (3aae1c4): the order-1 section, the
    /// ball's circle, built as `arcs` says (thumbweight 0).
    #[must_use]
    pub const fn circle(arcs: ArcSection) -> Self {
        Self {
            order: 1,
            thumbweight: 0.0,
            certified_bound: -1.0,
            certified_spans: 0,
            arcs,
        }
    }

    /// `IsValid`: whether an order and a thumbweight make a G^n section:
    /// `2 <= n <= GN_MAX_ORDER`, and `w < 1` (the section's weights `1 - w + w t` and
    /// `1 - w t` stay positive on [0, 1] and its reparametrizations monotone; b9d6a72) or
    /// the equal area default.
    #[must_use]
    pub fn is_valid(order: usize, thumbweight: f64) -> bool {
        (2..=GN_MAX_ORDER).contains(&order)
            && (thumbweight == GN_THUMBWEIGHT_EQUAL_AREA
                || (thumbweight.is_finite() && thumbweight < 1.0))
    }

    /// `EqualAreaThumbweight`: the w at which the order-n section between two planes
    /// meeting at the angle `alpha` (radians, the angle of the wedge the ball sits in,
    /// `0 < alpha < pi`) takes off, on the same rails, the area the ball's circle does:
    /// `r^2 (cot(alpha/2) - (pi - alpha)/2)`.
    ///
    /// With `e1`, `e2` the rails' points from the edge (`|e_i| = d = r cot(alpha/2)`), the
    /// section takes off `(1/2) d^2 sin(alpha) I(w)`, `I` the integral of `a b' - a' b`
    /// (`sm_GnAreaSum`), so `I(w) = (cot(alpha/2) - (pi - alpha)/2) / (cot(alpha/2)
    /// cos^2(alpha/2))`: with `u = (pi - alpha)/2`, half the angle between the ball's radii
    /// to its contacts, `(sin u - u cos u) / sin^3 u`. `I` falls as w rises (to 0 as w
    /// nears 1) and nears the chord's 1 as w falls, so the root is bisected on a bracket
    /// whose lower end steps down from -1 until it holds it (b9d6a72): a sharp wedge's w is
    /// well below that.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for an order past 2..=4, an angle outside (0, pi), or a
    /// target the bracket does not reach by w = -1000 (a wedge so open or so shut that the
    /// circle takes off next to nothing, or all but the chord's area).
    ///
    /// Deviation (b8ec238): the fork's goes through the thread's memo and counts its
    /// solve; this takes no kernel, so it solves afresh and counts nothing (the same w:
    /// the memo keeps what the solve gives). No blend operation calls it.
    pub fn equal_area_thumbweight(order: usize, alpha: f64) -> Result<f64> {
        if !(alpha > 0.0 && alpha < PI) {
            return Err(SolidError::invalid_input(
                "blend::fillet_section::GnSectionFsg::equal_area_thumbweight",
                "wedge angle",
            ));
        }
        gn_equal_area_w_solve(order, 0.5 * (PI - alpha))
    }

    /// `ThumbweightAt`: the thumbweight at a station whose ball's radii to its contacts
    /// meet at the angle whose cosine is `cos_radii`: the one given, or the equal area one
    /// of the wedge `alpha = pi - (that angle)`, kept in `k`'s memo. Exact for two planes;
    /// elsewhere the section of the station's own wedge (7102bc5).
    ///
    /// # Errors
    ///
    /// Those of [`GnSectionFsg::equal_area_thumbweight`].
    pub fn thumbweight_at(&self, k: &Kernel, cos_radii: f64) -> Result<f64> {
        if self.thumbweight != GN_THUMBWEIGHT_EQUAL_AREA {
            return Ok(self.thumbweight);
        }
        gn_equal_area_w(k, self.order, 0.5 * acos_clamped(cos_radii))
    }

    /// `CreateSurface`: the blend surface between the solver's rails. The solver's faces
    /// are its offset surfaces; the section is built on the faces under them (an offset
    /// surface's base, else the surface itself). The rails and their uv curves are rebuilt
    /// on the section's stations from the same exact feet, so that the surface's
    /// boundaries are the rails.
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    pub fn create_surface(
        &mut self,
        k: &Kernel,
        approx_tol_3d: f64,
        center_line: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        surfaces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
    ) -> Result<Surface> {
        let [s1, s2] = surfaces;
        let faces = [base_of(s1), base_of(s2)];
        self.build(
            k,
            center_line,
            rails,
            approx_tol_3d,
            faces,
            uvs,
            true,
            [0.0, 0.0],
            false,
            None,
            None,
        )?
        .map(|(s, _)| s)
        .ok_or_else(|| SolidError::sm_err(OP))
    }

    /// `CreateSurfaceFromCurves`: the blend surface between two rails on the faces'
    /// surfaces, about the centre line. The rails and uv curves are not changed.
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    pub fn create_surface_from_curves(
        &mut self,
        k: &Kernel,
        center_line: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        approx_tol_3d: f64,
        faces: GnFaces<'_>,
    ) -> Result<Surface> {
        let [u1, u2] = faces.uv;
        let mut uv1 = u1.clone();
        let mut uv2 = u2.clone();
        self.build(
            k,
            center_line,
            rails,
            approx_tol_3d,
            faces.surfaces,
            [&mut uv1, &mut uv2],
            false,
            [0.0, 0.0],
            false,
            None,
            None,
        )?
        .map(|(s, _)| s)
        .ok_or_else(|| SolidError::sm_err(OP))
    }

    /// `Build(..., bReplaceRails, rpFilletSurface, dRadius)` (cc08236): [`Self::build`]
    /// with the ball's radius given (`radius > 0`: the solver's offsets; 0: as the rails
    /// stand off the centre line), on the faces themselves (not offset surfaces). The
    /// order-1 section, the ball's exact circle, is built only here (the fork's circle
    /// generator calls it, cc08236).
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_with_radius(
        &mut self,
        k: &Kernel,
        center: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        tol: f64,
        faces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
        replace_rails: bool,
        radius: f64,
    ) -> Result<Surface> {
        self.build(
            k,
            center,
            rails,
            tol,
            faces,
            uvs,
            replace_rails,
            [radius, radius],
            false,
            None,
            None,
        )?
        .map(|(s, _)| s)
        .ok_or_else(|| SolidError::sm_err(OP))
    }

    /// `Build(..., TRUE, rpFilletSurface, dRadius, &bNotTheFeet)` as the fork's circle
    /// generator calls it (cc08236, 308cc40, c596548): the rails and uv curves replaced,
    /// the ball's radius given; `None` (nothing built, the rails and uv curves unchanged)
    /// where the solver's rails are not that ball's feet on the two faces: a station that
    /// does not solve from them (Newton from within the tolerance of a regular root
    /// converges), or a foot farther than `tol` from its rail (a rail held on a face's
    /// boundary while the ball rolls on its edge).
    ///
    /// # Errors
    ///
    /// Those of `Build`.
    ///
    /// `law` (0d43b4c): a ball whose radius varies, its law and leader (`radius` unused);
    /// the stations are then the solver's own and the leader's and law's knots between,
    /// and the centre line becomes the balls' centres over t with the rails.
    #[allow(clippy::too_many_arguments)]
    pub fn build_given_ball(
        &mut self,
        k: &Kernel,
        center: &mut BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        tol: f64,
        faces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
        radius: f64,
        law: Option<&GnLawRound<'_>>,
    ) -> Result<Option<Surface>> {
        let built = self.build(
            k,
            center,
            rails,
            tol,
            faces,
            uvs,
            true,
            [radius, radius],
            true,
            law,
            None,
        )?;
        Ok(built.map(|(s, c)| {
            if let Some(c) = c {
                *center = c;
            }
            s
        }))
    }

    /// The chord (order 0, a chamfer; solidcore's, #2486) between the feet of the point at
    /// `offsets` off the two faces, as [`Self::build_given_ball`] builds the ball's circle:
    /// the rails and uv curves replaced by the stations' feet, the surface the chamfer by
    /// its definition, `chamfer` its sizes. `None` (nothing built, the rails and uv curves
    /// unchanged) where the solver's rails are not those feet.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a generator that is not the chord; those of `Build`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_chord(
        &mut self,
        k: &Kernel,
        center: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        tol: f64,
        faces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
        offsets: [f64; 2],
        chamfer: ChamferDefinition,
    ) -> Result<Option<Surface>> {
        if self.order != 0 {
            return Err(SolidError::invalid_input(
                OP,
                "a chamfer is the chord, order 0",
            ));
        }
        let built = self.build(
            k,
            center,
            rails,
            tol,
            faces,
            uvs,
            true,
            offsets,
            true,
            None,
            Some(chamfer),
        )?;
        Ok(built.map(|(s, _)| s))
    }

    /// `Build`: the G^n blend surface between the rails, its stations the knots of the
    /// rails and the centre line. `replace_rails`: the rails and uv curves become the
    /// jets' own order-0 curves (the surface's boundaries). The surface's u is the rails'
    /// parameter, v the section's t, from the first rail (v = 0) to the second (v = 1).
    /// `radius` (cc08236): the ball's radius where the solver gives it (its offsets'), 0
    /// for as the rails stand off the centre line. Order 1, the ball's exact circle
    /// ([`ArcSection::is_exact_circle`]), is the rational quadratic arc between the rails
    /// (cc08236).
    #[allow(clippy::too_many_arguments)]
    fn build(
        &mut self,
        k: &Kernel,
        center: &BSplineCurve,
        rails: [&mut BSplineCurve; 2],
        tol: f64,
        faces: [&Surface; 2],
        uvs: [&mut BSplineCurve; 2],
        replace_rails: bool,
        radii: [f64; 2],
        feet_check: bool,
        law_round: Option<&GnLawRound<'_>>,
        chamfer: Option<ChamferDefinition>,
    ) -> Result<Option<Built>> {
        // a ball whose radius varies (the ball's circle only): its law r(t) and the curve
        // whose planes square to it at t hold the ball's centre (the solver's), t the
        // stations' parameter (0d43b4c)
        if law_round.is_some() && self.order != 1 {
            return Err(SolidError::invalid_input(
                OP,
                "a radius law is for the ball's circle",
            ));
        }
        let gn_law = match law_round {
            Some(l) => Some(GnLaw::of(l.law)?),
            None => None,
        };
        let gn_law = gn_law.as_ref();
        let leader: &BSplineCurve = law_round.map_or(center, |l| l.leader);
        let n = self.order;
        let exact_circle = n == 1 && self.arcs.is_exact_circle();
        // the chord (#2486): a chamfer, its sizes given
        let chord = n == 0 && chamfer.is_some() && law_round.is_none();
        if !chord && !exact_circle && !Self::is_valid(n, self.thumbweight) {
            return Err(SolidError::invalid_input(
                OP,
                "the order must be 1 (the ball's circle) or 2 to 4 with a thumbweight below 1",
            ));
        }
        let [rail1, rail2] = rails;
        // (the fork's 4a10ed8, opt-in: the surface is the blend by its definition)
        let definition = k.fixes.fillet_round_as_definition;
        let (c_min, c_max) = interval(center);
        // the stations: the unique knots of the rails and the centre line; with a law, the
        // solver's own stations' t (their feet the seeds) and the leader's and the law's
        // knots between
        let mut ss: Vec<f64> = Vec::new();
        let mut law_seeds: Vec<[f64; 4]> = Vec::new(); // with a law: each station's seed
        let mut law_foot: Vec<bool> = Vec::new(); // and whether it is one of the solver's
        let mut law_cp: Vec<f64> = Vec::new(); // and then its parameter on the rails
        let (mut i_min, mut i_max) = (c_min, c_max);
        if let Some(lr) = law_round {
            // each solver's station: its t, and its feet as the solver's uv curves hold
            // them at its curve parameter (the rails' and uv curves' own, which is not t)
            let mut pts: Vec<(f64, [f64; 5])> = Vec::new();
            for p in lr.points {
                let cp = p.curve_parameter;
                let w0 = uvs[0].evaluate_point(cp)?;
                let w1 = uvs[1].evaluate_point(cp)?;
                let t = p.user_doubles.first().copied().unwrap_or(f64::NAN);
                pts.push((t, [w0.x, w0.y, w1.x, w1.y, cp]));
            }
            // (std::sort by `<`)
            pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            let (Some(&(t_front, _)), Some(&(t_back, _))) = (pts.first(), pts.last()) else {
                // "the round's stations have no length"
                return Err(SolidError::sm_err(OP));
            };
            let has_length = t_back > t_front;
            if pts.len() < 2 || !has_length {
                // "the round's stations have no length"
                return Err(SolidError::sm_err(OP));
            }
            i_min = t_front;
            i_max = t_back;
            if let Some(r) = lr.range {
                let inside = r.min() >= t_front && r.max() <= t_back && r.max() > r.min();
                if !inside {
                    // "the round's range is not within its stations"
                    return Err(SolidError::sm_err(OP));
                }
                i_min = r.min();
                i_max = r.max();
            }
            let law_scale = smax(1.0, i_max - i_min);
            let mut at: Vec<f64> = if lr.range.is_none() {
                pts.iter().map(|p| p.0).collect()
            } else {
                let mut at = vec![i_min, i_max];
                for p in &pts {
                    if p.0 > i_min && p.0 < i_max {
                        at.push(p.0);
                    }
                }
                at
            };
            for c in [lr.leader, lr.law] {
                for kn in c.knots(None)?.0 {
                    if kn > i_min + 1.0e-9 * law_scale && kn < i_max - 1.0e-9 * law_scale {
                        at.push(kn);
                    }
                }
            }
            at.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            for &aj in &at {
                if ss.last().is_some_and(|&b| aj - b <= 1.0e-9 * law_scale) {
                    continue;
                }
                // the seed: the solver's station at t, or between the two about it
                let mut kk = 0;
                while pts.get(kk + 1).is_some_and(|p| p.0 <= aj) {
                    kk += 1;
                }
                let (Some(&(tk, xk)), next) = (pts.get(kk), pts.get(kk + 1)) else {
                    return Err(SolidError::sm_err(OP));
                };
                let mut x = xk;
                let foot = (tk - aj).abs() <= 1.0e-9 * law_scale;
                if let (false, Some(&(tk1, xk1))) = (foot, next) {
                    let w = (aj - tk) / (tk1 - tk);
                    for (xq, (&a, &b)) in x.iter_mut().zip(xk.iter().zip(&xk1)) {
                        *xq = (1.0 - w) * a + w * b;
                    }
                }
                ss.push(aj);
                law_seeds.push([x[0], x[1], x[2], x[3]]);
                law_foot.push(foot);
                law_cp.push(x[4]);
            }
        }
        let scale = smax(1.0, i_max - i_min);
        // the equal area thumbweight is resolved per station, from its own wedge
        let equal_area = self.thumbweight == GN_THUMBWEIGHT_EQUAL_AREA;
        for c in [center, &*rail1, &*rail2] {
            if law_round.is_some() {
                break;
            }
            let (lo, hi) = interval(c);
            if (lo - c_min).abs() > 1.0e-9 * scale || (hi - c_max).abs() > 1.0e-9 * scale {
                // "the rails and the centre line run over different intervals"
                return Err(SolidError::sm_err(OP));
            }
            ss.extend(c.knots(None)?.0);
        }
        if law_round.is_none() {
            ss.sort_by(f64::total_cmp);
            let mut unique: Vec<f64> = Vec::new();
            for &s in &ss {
                if unique.last().is_none_or(|&b| s - b > 1.0e-9 * scale) {
                    unique.push(s);
                }
            }
            if let Some(f) = unique.first_mut() {
                *f = c_min;
            }
            if let Some(l) = unique.last_mut() {
                *l = c_max;
            }
            ss = unique;
        }
        if ss.len() < 2 {
            // "the rails have no length"
            return Err(SolidError::sm_err(OP));
        }
        // (on the curves' own parameter: with a law it is not t, and the first station is the
        // round's start, i_min, at its parameter on the rails; a piece of a law-cornered
        // round starts inside them, where the rails' own start may be past its ball's
        // fading to a point)
        let s_first = match law_cp.first() {
            Some(&cp) if law_round.is_some() => cp,
            _ => c_min,
        };

        // the offsets and the sides, at the first station: the centre line there is the
        // ball's centre
        let mut r = [0.0_f64; 2];
        let mut side = [0.0_f64; 2];
        {
            let m = center.evaluate_point(s_first)?;
            let law_r = match law_round {
                Some(l) => l.law.evaluate_point(i_min)?.x,
                None => 0.0,
            };
            let rl: [&BSplineCurve; 2] = [&*rail1, &*rail2];
            let rows = r
                .iter_mut()
                .zip(side.iter_mut())
                .zip(rl)
                .zip(uvs.iter())
                .zip(faces)
                .zip(radii);
            for (((((rf, sf), rail), uv), face), radius) in rows {
                let rho = rail.evaluate_point(s_first)?;
                let w = uv.evaluate_point(s_first)?;
                let nrm = unit_normal(k, face, Point2::new(w.x, w.y))?;
                let off = m - rho;
                // the ball's radius where the solver gives it (its offsets'), else as the
                // rails stand off
                *rf = if law_round.is_some() {
                    law_r
                } else if radius > 0.0 {
                    radius
                } else {
                    off.length()
                };
                *sf = if off.dot(nrm) >= 0.0 { 1.0 } else { -1.0 };
                let has_radius = *rf > tol;
                if !has_radius {
                    // "the ball has no radius at the rails' start"
                    return Err(SolidError::sm_err(OP));
                }
            }
        }

        // every station's feet and their series: values and s-derivatives (sigma order
        // 1), jets to order n (the ball's circle takes the rails and the centre only;
        // with a law the centre to one order more, for the touch circle's m')
        let tr = Trunc::new(
            if law_round.is_some() { 2 } else { 1 },
            if n >= 2 { n } else { 0 },
        )?;
        let mut stations: Vec<GnStation> = Vec::new();
        for (j, &s) in ss.iter().enumerate() {
            k.check_cancelled(OP_BUILD)?;
            let seed = if law_round.is_some() {
                law_seeds
                    .get(j)
                    .copied()
                    .ok_or_else(|| SolidError::sm_err(OP))?
            } else {
                let w0 = uvs[0].evaluate_point(s)?;
                let w1 = uvs[1].evaluate_point(s)?;
                [w0.x, w0.y, w1.x, w1.y]
            };
            // a constant ball: the station's centre on the centre line at the same offsets
            let m = leader.evaluate_point(s)?;
            let solve_tol = 1.0e-9 * smax(1.0, m.length());
            let law_r = law_series(gn_law, s, tr.s(), false)?;
            // with a ball of a given radius or law (feet_check), a station that does not
            // solve from the solver's rails (Newton from within the tolerance of a regular
            // root converges) says the rails are not that ball's feet there (c596548)
            let solved = solve_top(
                k,
                faces,
                leader,
                s,
                seed,
                r,
                side,
                tr,
                solve_tol,
                n,
                None,
                false,
                law_r.as_ref(),
            );
            let st = match solved {
                Ok(st) => st,
                // a cancel (#2136) is not a refused ball: it ends the operation
                Err(e @ SolidError::Cancelled { .. }) => return Err(e),
                Err(_) if feet_check => return Ok(None),
                Err(e) => return Err(e),
            };
            for (f, &rf) in r.iter().enumerate() {
                let rho = foot(&st, f, 0, 0);
                // the ball's circle of a given radius: the solver's rails are this ball's
                // feet on the two faces, or the round is not their ball's (a rail held on
                // a face's boundary, the ball rolling on its edge), which the caller
                // builds otherwise (308cc40)
                if feet_check {
                    // with a law, the solver's stations' feet are (0d43b4c)
                    if law_round.is_some() && !law_foot.get(j).copied().unwrap_or(false) {
                        continue;
                    }
                    let at = if law_round.is_some() {
                        law_cp.get(j).copied().unwrap_or(f64::NAN)
                    } else {
                        s
                    };
                    let rail: &BSplineCurve = if f == 0 { rail1 } else { rail2 };
                    if rail.evaluate_point(at)?.distance(rho) > tol {
                        return Ok(None);
                    }
                    continue;
                }
                if (m.distance(rho) - rf).abs() > smax(tol, 1.0e-6 * rf) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "a section of order 2 and up is built for a ball of one radius only",
                    ));
                }
            }
            stations.push(st);
        }

        // the certificate
        let (mut stations, bound, spans) = self.certify(
            k, faces, leader, stations, r, side, tr, tol, equal_area, gn_law,
        )?;
        self.certified_bound = bound;
        self.certified_spans = spans;
        let ss: Vec<f64> = stations.iter().map(|st| st.s).collect();

        // each station's series on the pieces before it, for the span ending there: a
        // Hermite span interpolates the values and s-derivatives of its own pieces, which
        // the certificate bounds (3bc54a4)
        let system = StationSystem {
            faces,
            leader,
            r,
            side,
            tr,
            n,
            equal_area,
            law: gn_law,
        };
        for st in stations.iter_mut().skip(1) {
            k.check_cancelled(OP_BUILD)?;
            system.solve_before(k, st)?;
        }

        let ld = section_width(n); // the section's control points across

        // the jets' Hermite curves along s, all on the stations' knots
        let (u_knots, u_mult) = hermite_knots(&ss);
        let mut jets: [Vec<Vec<Vec3>>; 2] = [Vec::new(), Vec::new()];
        for (f, jf) in jets.iter_mut().enumerate() {
            for kk in 0..=n {
                let v: Vec<Vec3> = stations.iter().map(|st| foot(st, f, 0, kk)).collect();
                let d: Vec<Vec3> = stations.iter().map(|st| foot(st, f, 1, kk)).collect();
                let d_in: Vec<Vec3> = stations.iter().map(|st| foot_in(st, f, 1, kk)).collect();
                jf.push(hermite_points_sided(&ss, &v, &d, &d_in));
            }
        }
        let jet = |f: usize, kk: usize, i: usize| -> Vec3 {
            jets.get(f)
                .and_then(|j| j.get(kk))
                .and_then(|j| j.get(i))
                .copied()
                .unwrap_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN))
        };
        let lnu = jets[0].first().map_or(0, Vec::len);

        // the homogeneous control points W P and weights W, [i along s][j across], span by
        // span (a span's first row is the one before's last)
        let net_form = NetForm {
            n,
            thumbweight: self.thumbweight,
            equal_area,
            law: law_round.is_some(),
        };
        let mut rows = Vec::new();
        for st in &stations {
            k.check_cancelled(OP_BUILD)?;
            rows.push(net_form.station_rows(st)?);
        }
        let mut spans: Vec<(Vec<Vec3>, Vec<f64>)> = Vec::new();
        for (pair, rp) in stations.windows(2).zip(rows.windows(2)) {
            k.check_cancelled(OP_BUILD)?;
            let ([a, b], [ra, rb]) = (pair, rp) else {
                return Err(SolidError::sm_err(OP));
            };
            spans.push(net_form.span(a, ra, b, rb));
        }
        let surface = net_of_spans(&ss, &spans, ld)?;

        // the feet's uv curves: the Hermite interpolants of the stations' feet, the
        // definition's seeds (4a10ed8); the rails and uv curves become them with
        // `replace_rails`
        let mut seeds: Vec<BSplineCurve> = Vec::new();
        for (f, (rail, uv)) in [rail1, rail2].into_iter().zip(uvs).enumerate() {
            let v: Vec<Vec3> = stations
                .iter()
                .map(|st| Vec3::new(param(st, 2 * f, 0, 0), param(st, 2 * f + 1, 0, 0), 0.0))
                .collect();
            let d: Vec<Vec3> = stations
                .iter()
                .map(|st| Vec3::new(param(st, 2 * f, 1, 0), param(st, 2 * f + 1, 1, 0), 0.0))
                .collect();
            let d_in: Vec<Vec3> = stations
                .iter()
                .map(|st| {
                    Vec3::new(
                        param_in(st, 2 * f, 1, 0),
                        param_in(st, 2 * f + 1, 1, 0),
                        0.0,
                    )
                })
                .collect();
            let uv_cps = hermite_points_sided(&ss, &v, &d, &d_in);
            let dim = uv.dim;
            let uv_data = CanonicalData {
                dim,
                degree: 3,
                control_points: &uv_cps,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &u_mult,
                knots: &u_knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            };
            if definition {
                // (apUV[f]->Copy, then SetCanonical on the copy)
                let mut seed = uv.clone();
                seed.set_canonical(&uv_data)?;
                seeds.push(seed);
            }
            if replace_rails {
                let rail_cps: Vec<Point3> = (0..lnu).map(|i| jet(f, 0, i)).collect();
                rail.set_canonical(&CanonicalData {
                    dim: 3,
                    degree: 3,
                    control_points: &rail_cps,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &u_mult,
                    knots: &u_knots,
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })?;
                uv.set_canonical(&uv_data)?;
            }
        }
        // with a law the rails now run over t: the centre line too, the balls' centres at
        // the stations (0d43b4c)
        let mut new_center: Option<BSplineCurve> = None;
        if replace_rails && law_round.is_some() {
            let mut v = Vec::new();
            let mut d = Vec::new();
            let mut d_in = Vec::new();
            for st in &stations {
                let vin = if st.sided { &st.vl } else { &st.v };
                let [x, y, z] = &st.v.a;
                let [xi, yi, zi] = &vin.a;
                v.push(Vec3::new(x.at(0, 0), y.at(0, 0), z.at(0, 0)));
                d.push(Vec3::new(x.at(1, 0), y.at(1, 0), z.at(1, 0)));
                d_in.push(Vec3::new(xi.at(1, 0), yi.at(1, 0), zi.at(1, 0)));
            }
            let cps = hermite_points_sided(&ss, &v, &d, &d_in);
            let mut c = center.clone();
            c.set_canonical(&CanonicalData {
                dim: 3,
                degree: 3,
                control_points: &cps,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &u_mult,
                knots: &u_knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })?;
            new_center = Some(c);
        }
        if !definition {
            return Ok(Some((Surface::from(surface), new_center)));
        }

        // the blend by its definition on this net (4a10ed8): the faces, the ball's offsets
        // and sides, the section, the centre line as the leader the stations were solved
        // in, the seeds, the certified bound
        let (Some(seed0), Some(seed1)) = (seeds.first(), seeds.get(1)) else {
            return Err(SolidError::sm_err(OP));
        };
        // its bound: the certificate's, of the Hermite rows; how far the net stored (its
        // control points divided out of the rows and its weights scaled, each rounded) is
        // from them; and three floors, how far rows computed again may be (#2388)
        let rounding = super::recertify::stored_net_distance(&surface, &spans, ld)?
            + 3.0 * super::recertify::precision_floor(k, faces, n as u32, &surface)?;
        let input = BlendDefinitionInput {
            supports: faces,
            sides: side,
            offsets: r,
            law: law_round.map(|l| l.law),
            order: u32::try_from(n).map_err(|_| SolidError::sm_err(OP))?,
            thumbweight: self.thumbweight,
            leader,
            seeds: [seed0, seed1],
            bound: self.certified_bound + rounding,
            chamfer,
        };
        let blend = ProceduralBlend::create(k, &surface, &input, |order, w| {
            Self::is_valid(order as usize, w)
        })?;
        Ok(Some((Surface::from(blend), new_center)))
    }

    /// The certificate block of `Build`: a station wherever a foot crosses a knot line of
    /// its face, then each span's bound (`sm_GnCertifySpan`) at most a tenth of the
    /// tolerance, a span over it halved (a station solved at its middle) until none is.
    /// Returns the stations, the largest bound and the number of spans.
    #[allow(clippy::too_many_arguments)]
    fn certify(
        &self,
        k: &Kernel,
        faces: [&Surface; 2],
        center: &BSplineCurve,
        mut stations: Vec<GnStation>,
        r: [f64; 2],
        side: [f64; 2],
        tr: Trunc,
        tol: f64,
        equal_area: bool,
        law: Option<&GnLaw<'_>>,
    ) -> Result<(Vec<GnStation>, f64, usize)> {
        let ([n0, n1], mut defs) = certificate_faces(k, faces, self.order, law.is_some())?;
        let any_defs = defs.faces.iter().any(Option::is_some);
        let face_n = [&n0, &n1];
        let leader = NurbsCurve::of(center)?;
        let target = 0.1 * tol;
        let solve_tol = |s: f64| -> Result<f64> {
            let m = center.evaluate_point(s)?;
            Ok(1.0e-9 * smax(1.0, m.length()))
        };
        // a station wherever a foot crosses a knot line of its face: the face is only as
        // smooth there as the knot allows, and a span's enclosures are taken on one side
        for (f, nf) in face_n.iter().enumerate() {
            for c in 0..2 {
                let inner = inner_knots(nf, c);
                let mut next: Vec<GnStation> = Vec::new();
                for pair in stations.windows(2) {
                    k.check_cancelled(OP_CERTIFY)?;
                    let [a, b] = pair else { continue };
                    next.push(*a);
                    for &dk in &inner {
                        let q = 2 * f + c;
                        let xa = param(a, q, 0, 0);
                        let xb = param(b, q, 0, 0);
                        if !crosses_knot(xa, xb, dk) {
                            continue;
                        }
                        // the parameter where that foot is on the knot line: the secant,
                        // kept in its bracket
                        let (mut s0, mut s1, mut x0, mut x1) = (a.s, b.s, xa - dk, xb - dk);
                        let mut at: Option<GnStation> = None;
                        for _ in 0..60 {
                            k.check_cancelled(OP_CERTIFY)?;
                            let mut ds = s0 - x0 * (s1 - s0) / (x1 - x0);
                            let inside = ds > s0 && ds < s1;
                            if !inside {
                                ds = 0.5 * (s0 + s1);
                            }
                            let t = (ds - a.s) / (b.s - a.s);
                            let mut seed = [0.0_f64; 4];
                            for (q2, sq) in seed.iter_mut().enumerate() {
                                *sq = (1.0 - t) * param(a, q2, 0, 0) + t * param(b, q2, 0, 0);
                            }
                            let tol_ds = solve_tol(ds)?;
                            let law_r = law_series(law, ds, tr.s(), false)?;
                            let st = solve_top(
                                k,
                                faces,
                                center,
                                ds,
                                seed,
                                r,
                                side,
                                tr,
                                tol_ds,
                                self.order,
                                None,
                                false,
                                law_r.as_ref(),
                            )?;
                            let x = param(&st, q, 0, 0) - dk;
                            at = Some(st);
                            if x.abs() <= 1.0e-12 * (1.0 + dk.abs()) {
                                break;
                            }
                            if (x < 0.0) == (x0 < 0.0) {
                                s0 = ds;
                                x0 = x;
                            } else {
                                s1 = ds;
                                x1 = x;
                            }
                        }
                        if let Some(st) = at {
                            // on the knot line the face's derivatives of order n + 1
                            // differ on its two sides, and with them the jets'
                            // s-derivatives: the station is solved on the piece after it
                            // (the foot moves on to xb's side) and on the piece before,
                            // for the span ending there (3bc54a4)
                            let after = if xb > xa { 1 } else { -1 };
                            let mut ai_after = [0_i32; 4];
                            let mut ai_before = [0_i32; 4];
                            if let (Some(x), Some(y)) = (ai_after.get_mut(q), ai_before.get_mut(q))
                            {
                                *x = after;
                                *y = -after;
                            }
                            let on = [0, 1, 2, 3].map(|q2| param(&st, q2, 0, 0));
                            let tol_on = solve_tol(st.s)?;
                            let law_r = law_series(law, st.s, tr.s(), false)?;
                            let mut st = solve_top(
                                k,
                                faces,
                                center,
                                st.s,
                                on,
                                r,
                                side,
                                tr,
                                tol_on,
                                self.order,
                                Some(ai_after),
                                false,
                                law_r.as_ref(),
                            )?;
                            st.before = ai_before;
                            next.push(st);
                        }
                    }
                }
                if let Some(last) = stations.last() {
                    next.push(*last);
                }
                next.sort_by(|a, b| a.s.total_cmp(&b.s));
                stations = next;
            }
        }
        // the equal area thumbweight at every station, and I's enclosures on a grid over
        // their range (a little wider: the stations put in later lie between these)
        let n = self.order;
        let rr = r[0] * r[1];
        if equal_area {
            for st in &mut stations {
                gn_station_thumbweight(k, n, rr, st)?;
            }
        }
        let mut vary = varying(n, r, &stations, equal_area);
        let mut bounds = vec![-1.0_f64; stations.len().saturating_sub(1)];
        loop {
            let mut all = true;
            for (j, bj) in bounds.iter_mut().enumerate() {
                k.check_cancelled(OP_CERTIFY)?;
                if *bj < 0.0 {
                    let (Some(a), Some(b)) = (stations.get(j), stations.get(j + 1)) else {
                        return Err(SolidError::sm_err(OP));
                    };
                    *bj = gn_certify_span(
                        k,
                        face_n,
                        &leader,
                        faces,
                        center,
                        a,
                        b,
                        r,
                        side,
                        self.order,
                        target,
                        if equal_area { Some(&mut vary) } else { None },
                        law,
                        if any_defs { Some(&mut defs) } else { None },
                    )?;
                }
                let within = *bj <= target;
                if !within {
                    all = false;
                }
            }
            if all {
                break;
            }
            if stations.len() > 4096 {
                // "the blend could not be certified within its tolerance"
                return Err(SolidError::sm_err(OP));
            }
            let mut next: Vec<GnStation> = Vec::new();
            let mut next_bounds: Vec<f64> = Vec::new();
            for (pair, &bj) in stations.windows(2).zip(&bounds) {
                k.check_cancelled(OP_CERTIFY)?;
                let [a, b] = pair else { continue };
                next.push(*a);
                if bj <= target {
                    next_bounds.push(bj);
                    continue;
                }
                let mid = 0.5 * (a.s + b.s);
                let inside = mid > a.s && mid < b.s;
                if !inside {
                    return Err(SolidError::sm_err(OP));
                }
                let mut seed = [0.0_f64; 4];
                for (c, sc) in seed.iter_mut().enumerate() {
                    *sc = 0.5 * (param(a, c, 0, 0) + param(b, c, 0, 0));
                }
                let tol_mid = solve_tol(mid)?;
                let law_r = law_series(law, mid, tr.s(), false)?;
                let mut st = solve_top(
                    k,
                    faces,
                    center,
                    mid,
                    seed,
                    r,
                    side,
                    tr,
                    tol_mid,
                    n,
                    None,
                    false,
                    law_r.as_ref(),
                )?;
                if equal_area {
                    gn_station_thumbweight(k, n, rr, &mut st)?;
                }
                next.push(st);
                next_bounds.push(-1.0);
                next_bounds.push(-1.0);
            }
            if let Some(last) = stations.last() {
                next.push(*last);
            }
            stations = next;
            bounds = next_bounds;
        }
        let mut bound = 0.0_f64;
        for &b in &bounds {
            bound = smax(bound, b);
        }
        // (the fork's SM_ERR_MESSAGE report of the spans and the bound is not logged)
        let spans = bounds.len();
        Ok((stations, bound, spans))
    }

    /// `ContactOrderAt`: the order of contact k of this section with face `rail` (0 or 1)
    /// at the station of the centre line's parameter `s`: the power at which the section
    /// stands off the face, `c s^k`. The coefficient of `t^(n+1)` in the standoff along the
    /// face's normal `N_i` is `[(T_o(0) - rho_i) - c_i,n+1 (-1/(1 - w))^(n+1)] . N_i`:
    /// `k = n + 1` where it is not zero, `n + 2` where it is (two planes).
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` for a rail past 1 or an invalid generator; those of the
    /// station solve and the evaluations.
    #[allow(clippy::too_many_arguments)]
    pub fn contact_order_at(
        &self,
        k: &Kernel,
        faces: [&Surface; 2],
        center: &BSplineCurve,
        s: f64,
        seed: [f64; 4],
        rail_points: [Point3; 2],
        rail: usize,
    ) -> Result<usize> {
        if rail > 1 || !Self::is_valid(self.order, self.thumbweight) {
            return Err(SolidError::invalid_input(OP, "ContactOrderAt"));
        }
        let n = self.order;
        let m = center.evaluate_point(s)?;
        let mut r = [0.0_f64; 2];
        let mut side = [0.0_f64; 2];
        let rows = r
            .iter_mut()
            .zip(side.iter_mut())
            .zip(faces)
            .zip(rail_points)
            .zip(seed.chunks_exact(2));
        for ((((rf, sf), face), rho), uv) in rows {
            let (Some(&u), Some(&v)) = (uv.first(), uv.get(1)) else {
                return Err(SolidError::sm_err(OP));
            };
            let nrm = unit_normal(k, face, Point2::new(u, v))?;
            let off = m - rho;
            *rf = off.length();
            *sf = if off.dot(nrm) >= 0.0 { 1.0 } else { -1.0 };
        }
        let tr = Trunc::new(0, n + 1)?;
        let opts = StationOptions {
            law: None,
            top: n,
            sides: None,
            before: false,
        };
        let (at, curves) = gn_solve_station_with(
            k,
            faces,
            center,
            s,
            seed,
            r,
            side,
            tr,
            1.0e-9 * smax(1.0, m.length()),
            opts,
        )
        .0?;
        // the thumbweight of the station's own wedge: the ball's radii to its contacts from V
        let v = Vec3::new(at.v.a[0].at(0, 0), at.v.a[1].at(0, 0), at.v.a[2].at(0, 0));
        let w = self.thumbweight_at(
            k,
            (foot(&at, 0, 0, 0) - v).dot(foot(&at, 1, 0, 0) - v) / (r[0] * r[1]),
        )?;
        let (i, o) = (rail, 1 - rail);
        let mut t_o = Vec3::new(0.0, 0.0, 0.0);
        for kk in 0..=n {
            t_o += foot(&at, o, 0, kk) * if kk % 2 == 0 { 1.0 } else { -1.0 };
        }
        let p = surface_partials(
            k,
            if i == 0 { faces[0] } else { faces[1] },
            Point2::new(param(&at, 2 * i, 0, 0), param(&at, 2 * i + 1, 0, 0)),
            1,
        )?;
        let ni = vec3(p.at(1, 0)).cross(vec3(p.at(0, 1))).unitize()?;
        let scale = (-1.0 / (1.0 - w)).powf((n + 1) as f64);
        // the jet's n-th coefficient leaves out the face curve's tangent part tau_n: the
        // section is off the curve by -tau_n (u - 1)^n in the tangent plane at the rail,
        // which the face's normal, turning by N_1 (u - 1) along the curve, sees at order
        // n + 1 (3bc54a4)
        let at_vs =
            |v: &VSer<f64>, e: usize| Vec3::new(v.a[0].at(0, e), v.a[1].at(0, e), v.a[2].at(0, e));
        let ci = if i == 0 { &curves.c[0] } else { &curves.c[1] };
        let nci = if i == 0 { &curves.n[0] } else { &curves.n[1] };
        let cn = at_vs(ci, n);
        let tau = cn - ni * cn.dot(ni);
        let n1 = at_vs(nci, 1);
        let coef = ((t_o - foot(&at, i, 0, 0)) - foot(&at, i, 0, n + 1) * scale).dot(ni)
            - tau.dot(n1) * scale;
        let size = foot(&at, 0, 0, 0).distance(foot(&at, 1, 0, 0));
        Ok(if coef.abs() > 1.0e-9 * smax(1.0, size) {
            n + 1
        } else {
            n + 2
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;

    fn plane(x_axis: Vec3, y_axis: Vec3) -> Surface {
        Surface::from(Plane::new(
            Vec3::ZERO,
            x_axis,
            y_axis,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    /// `EqualAreaThumbweight` for orders 2 to 4 over wedges from 0.3 to 3 radians: the
    /// values fork d01d7b0 gives (built locally, `-ffp-contract=off`), bit for bit; the
    /// wedges too shut for any thumbweight are refused, as there. At a square corner the
    /// section takes off what the circle does, `(1 - pi/4) r^2` (closed form).
    /// The fork's `EqualAreaThumbweight` at 7102bc5 (b9d6a72's bracket below -1, the
    /// closed-form integrand and the `sin`/`cos` target; built locally, `SMLib` and `SM_API`
    /// only, and run on these inputs): every value bit for bit. Before b9d6a72 the sharp
    /// wedges (0.05, 0.3, 1.0) had none.
    #[test]
    fn equal_area_thumbweights_are_the_fork_s() {
        let al = [
            0.05,
            0.3,
            1.0,
            core::f64::consts::FRAC_PI_2,
            2.0,
            2.5,
            3.0,
            3.1,
        ];
        let fork: [[f64; 8]; 3] = [
            [
                -40.158432198687606,
                -5.9593132523273784,
                -1.2200674332497643,
                -0.52521240458755813,
                -0.29072725674815347,
                -0.1505880430432826,
                -0.097163726970223796,
                -0.094769962349426001,
            ],
            [
                -41.536688806991449,
                -6.1966199386802323,
                -1.299175030152564,
                -0.58094928730219575,
                -0.3385247245324452,
                -0.19361792885821072,
                -0.13837023281846145,
                -0.13589468192029808,
            ],
            [
                -42.664956195284432,
                -6.3873279899518245,
                -1.3609708400917411,
                -0.62397886337619157,
                -0.37522338561877655,
                -0.22652960634299257,
                -0.16983693311526854,
                -0.16729661577416338,
            ],
        ];
        for (n, row) in (2..=4).zip(fork) {
            for (&alpha, want) in al.iter().zip(row) {
                let got = GnSectionFsg::equal_area_thumbweight(n, alpha).unwrap();
                assert_eq!(got.to_bits(), want.to_bits(), "n {n} alpha {alpha}: {got}");
            }
        }
        for alpha in al {
            assert!(GnSectionFsg::equal_area_thumbweight(1, alpha).is_err());
            assert!(GnSectionFsg::equal_area_thumbweight(5, alpha).is_err());
        }
        assert!(GnSectionFsg::equal_area_thumbweight(2, 0.0).is_err());
        assert!(GnSectionFsg::equal_area_thumbweight(2, PI).is_err());

        // a square corner: half the integral is the area over r^2, 1 - pi/4 (closed form)
        for n in 2..=GN_MAX_ORDER {
            let w = GnSectionFsg::equal_area_thumbweight(n, core::f64::consts::FRAC_PI_2).unwrap();
            let area = 0.5 * gn_area_sum(n, w);
            assert!((area - (1.0 - PI / 4.0)).abs() < 1e-12, "n {n}: {area}");
        }

        // at a station: the radii at a right angle (cos 0) give the square corner's w; a
        // thumbweight given is kept
        let k = Kernel::default();
        let g = GnSectionFsg::new(2, GN_DEFAULT_THUMBWEIGHT);
        assert_eq!(
            g.thumbweight_at(&k, 0.0).unwrap().to_bits(),
            (-0.52521240458755813_f64).to_bits()
        );
        assert_eq!(g.thumbweight, GN_THUMBWEIGHT_EQUAL_AREA);
        assert_eq!(
            GnSectionFsg::new(2, 0.3).thumbweight_at(&k, 0.7).unwrap(),
            0.3
        );
        // radii that coincide or oppose: no wedge
        assert!(g.thumbweight_at(&k, 1.0).is_err());
        assert!(g.thumbweight_at(&k, -1.0).is_err());
    }

    /// The marker, passed by name, selects the equal area section: its thumbweight is
    /// resolved from each station's wedge. A caller still passing the marker's value from
    /// before b9d6a72 (-2) gets an ordinary section of that fixed thumbweight instead.
    #[test]
    fn the_equal_area_marker_selects_the_equal_area_section() {
        let k = Kernel::default();
        let marked = GnSectionFsg::new(2, GN_THUMBWEIGHT_EQUAL_AREA);
        let square = GnSectionFsg::equal_area_thumbweight(2, core::f64::consts::FRAC_PI_2).unwrap();
        let sixty = GnSectionFsg::equal_area_thumbweight(2, PI / 3.0).unwrap();
        // radii at a right angle (cos 0): the square corner's; at 120 degrees: the 60 degree wedge's
        assert_eq!(
            marked.thumbweight_at(&k, 0.0).unwrap().to_bits(),
            square.to_bits()
        );
        assert!((marked.thumbweight_at(&k, -0.5).unwrap() - sixty).abs() < 1e-12);
        assert!((square - sixty).abs() > 0.1);
        assert_eq!(
            GN_DEFAULT_THUMBWEIGHT.to_bits(),
            GN_THUMBWEIGHT_EQUAL_AREA.to_bits()
        );
        // the stale value is an ordinary thumbweight, kept at every station
        let stale_marker = -2.0;
        let stale = GnSectionFsg::new(2, stale_marker);
        assert!(GnSectionFsg::is_valid(2, stale_marker));
        assert_eq!(stale.thumbweight_at(&k, 0.0).unwrap(), stale_marker);
        assert_eq!(stale.thumbweight_at(&k, -0.5).unwrap(), stale_marker);
    }

    /// b8ec238 (smlib#109): each order and u is solved once, and a kept thumbweight is
    /// the solve's, bit for bit. A solve that fails is not kept.
    #[test]
    fn equal_area_thumbweights_are_solved_once_each() {
        let k = Kernel::default();
        let us = [0.3, core::f64::consts::FRAC_PI_4, 1.2];
        for _ in 0..3 {
            for n in 2..=GN_MAX_ORDER {
                for u in us {
                    let w = gn_equal_area_w(&k, n, u).unwrap();
                    assert_eq!(w.to_bits(), gn_equal_area_w_solve(n, u).unwrap().to_bits());
                }
            }
        }
        assert_eq!(k.blend_counts.get().area_roots, 9);
        assert_eq!(k.blend_w_memo.borrow().len(), 9);
        // the next u up is another root
        let next = f64::from_bits(0.3_f64.to_bits() + 1);
        let _ = gn_equal_area_w(&k, 2, next).unwrap();
        assert_eq!(k.blend_counts.get().area_roots, 10);
        // no wedge: an error each time, solved each time
        assert!(gn_equal_area_w(&k, 2, 0.0).is_err());
        assert!(gn_equal_area_w(&k, 2, 0.0).is_err());
        assert_eq!(k.blend_counts.get().area_roots, 12);
        assert_eq!(k.blend_w_memo.borrow().len(), 10);
    }

    /// b8ec238: the outermost operation scope empties the memo as it opens; a nested one
    /// keeps it, and so does the end of a scope.
    #[test]
    fn an_operation_scope_starts_the_memo_afresh() {
        let k = Kernel::default();
        let _ = gn_equal_area_w(&k, 2, 0.5).unwrap();
        {
            let _outer = OperationScope::new(&k);
            assert!(k.blend_w_memo.borrow().is_empty());
            let _ = gn_equal_area_w(&k, 2, 0.5).unwrap();
            {
                let _inner = OperationScope::new(&k);
                assert_eq!(k.blend_w_memo.borrow().len(), 1);
                let _ = gn_equal_area_w(&k, 2, 0.5).unwrap();
            }
            assert_eq!(k.blend_w_memo.borrow().len(), 1);
        }
        assert_eq!(k.blend_w_memo.borrow().len(), 1);
        assert_eq!(k.blend_counts.get().area_roots, 2);
        let _again = OperationScope::new(&k);
        assert!(k.blend_w_memo.borrow().is_empty());
    }

    #[test]
    fn valid_orders_and_thumbweights() {
        assert!(GnSectionFsg::is_valid(2, 0.0));
        assert!(GnSectionFsg::is_valid(4, 0.999));
        assert!(!GnSectionFsg::is_valid(1, 0.0));
        assert!(!GnSectionFsg::is_valid(5, 0.0));
        assert!(!GnSectionFsg::is_valid(2, 1.0));
        // the fork's b9d6a72 (its IsValid at 7102bc5, run): any finite w below 1, and the
        // equal area default 2
        assert!(GnSectionFsg::is_valid(2, -0.1));
        assert!(GnSectionFsg::is_valid(2, -1.0));
        assert!(GnSectionFsg::is_valid(2, -5.0));
        assert!(GnSectionFsg::is_valid(2, -1e300));
        assert!(GnSectionFsg::is_valid(2, -2.0));
        assert!(GnSectionFsg::is_valid(2, GN_THUMBWEIGHT_EQUAL_AREA));
        assert!(!GnSectionFsg::is_valid(2, f64::NEG_INFINITY));
        assert!(!GnSectionFsg::is_valid(2, f64::NAN));
        let g = GnSectionFsg::new(3, 0.25);
        assert_eq!(g.certified_bound, -1.0);
        assert_eq!(g.certified_spans, 0);
    }

    /// Two planes: the feet are linear in tau, so `c_i,n+1 = 0` and `T_o(0)` is the edge's
    /// point, on face i: the `t^(n+1)` coefficient vanishes and the contact is of order
    /// n + 2 on both faces (the fork's `ContactOrderAt` note), at a square corner and at
    /// 60 degrees, for every order and a thumbweight.
    #[test]
    fn two_planes_meet_the_section_at_order_n_plus_2() {
        let k = Kernel::default();
        let r = 1.5;
        let (c, s60) = (0.5_f64, 3.0_f64.sqrt() / 2.0);
        // face 1: z = 0 (normal +z); face 2: the plane through the y axis at angle a to it
        for (dir2, center) in [
            (Vec3::Z, Vec3::new(r, 0.0, r)),
            // the second face along (c, 0, s60): the ball's centre on the bisector at
            // distance r from both
            (
                Vec3::new(c, 0.0, s60),
                Vec3::new(r / (30.0_f64.to_radians()).tan(), 0.0, r),
            ),
        ] {
            let f1 = plane(Vec3::X, Vec3::Y);
            let f2 = plane(Vec3::Y, dir2);
            let leader =
                BSplineCurve::create_line_segment(3, center, center + Vec3::new(0.0, 4.0, 0.0))
                    .unwrap();
            let s = 0.5;
            let m = center + Vec3::new(0.0, 2.0, 0.0);
            let rho1 = Vec3::new(m.x, m.y, 0.0);
            // the foot on face 2 along its direction, at the centre's projection
            let t2 = m.x * dir2.x + m.z * dir2.z;
            let rho2 = Vec3::new(t2 * dir2.x, m.y, t2 * dir2.z);
            assert!(((m - rho1).length() - r).abs() < 1e-12);
            assert!(((m - rho2).length() - r).abs() < 1e-12);
            let seed = [rho1.x, rho1.y, m.y, t2];
            for n in 2..=GN_MAX_ORDER {
                for w in [0.0, 0.3] {
                    let g = GnSectionFsg::new(n, w);
                    for rail in 0..2 {
                        let order = g
                            .contact_order_at(&k, [&f1, &f2], &leader, s, seed, [rho1, rho2], rail)
                            .unwrap();
                        assert_eq!(order, n + 2, "n {n} w {w} rail {rail}");
                    }
                }
            }
        }
    }
}
