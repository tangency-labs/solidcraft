//! The side walls of a draft extrusion that keep their curvature at the profile's
//! corners, or continue as their tangent planes there (SolidCraft change).
//!
//! Upstream: none at the pin. Ported from the SolidCraft fork of SMLib
//! (tangency-labs/smlib 54148db "draft corners that keep the curves' curvature" and
//! e8f23e0 "the tangent draft corner continues lines' and arcs' walls as their
//! tangent planes"), `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust                          | Fork                                  |
//! |-------------------------------|---------------------------------------|
//! | [`TaperCornerRefusal`]        | `SmTaperCornerRefusal`                |
//! | [`CornerRefusal`]             | `pOptRefusal`, `pOptRefusedCorner`    |
//! | `Wall` (private)              | `sm_DraftWall`                        |
//! | `Seam` (private)              | `sm_DraftSeam`                        |
//! | `section` (private)           | `sm_DraftSection`                     |
//! | `crossing` (private)          | `sm_DraftCrossing`                    |
//! | `off_section` (private)       | `sm_DraftOffSection`                  |
//! | `wall_normal` (private)       | `sm_DraftNormal`                      |
//! | `angle` (private)             | `sm_DraftAngle`                       |
//! | `param` (private)             | `sm_DraftParam`                       |
//! | `seam_point` (private)        | `sm_DraftSeamPoint`                   |
//! | `wall_curve` (private)        | `sm_DraftWallCurve`                   |
//! | `quartic` (private)           | `sm_Quartic`                          |
//! | `seam_pcurve` (private)       | `sm_DraftSeamPcurve`                  |
//! | `iso_pcurve` (private)        | `sm_DraftIsoPcurve`                   |
//! | `seam_curve` (private)        | `sm_DraftSeamCurve`                   |
//! | `draft_cone` (private)        | `sm_DraftCone`                        |
//! | `on_cone` (private)           | none (#2497)                          |
//! | `profile_plane` (private)     | none (#2497)                          |
//! | `lean_offsets` (private)      | none (#2497)                          |
//! | `seam_angles` (private)       | none (#2497)                          |
//! | `cone_face_angles` (private)  | none (#2497, WR-722)                  |
//! | `half_quadratic_roots` (private) | the quadratic of `sm_DraftSeamPcurve` |
//! | `has_corner`                  | `sm_DraftHasCorner`                   |
//! | `draft_input` (private)       | `sm_DraftInput`                       |
//! | `lines_and_arcs`              | `sm_DraftLinesAndArcs`                |
//! | `curvature_keeping_walls`     | `sm_CreateCurvatureKeepingWalls`      |
//!
//! The method (from the fork): each profile curve's wall is the surface its draft
//! sweeps, a line's plane or an arc's cone, ruled from the curve to its offset at the
//! far end. At a sharp corner the two walls continue as themselves until they meet,
//! and the seam is their intersection, a conic: the rational quadratic through the
//! corner and the far crossing with the walls' common tangents there, through the
//! crossing at half height. On a plane wall its pcurve is its affine image; on a cone
//! wall a rational quartic in each of the cone's spans it crosses. An arc's wall whose
//! height runs along its axis is its exact cone (tangency-labs/smlib 3ae7bfa), any other
//! wall the surface ruled between its ends. A draft whose height leans off the profile's
//! plane (a normal near Z that `CreateTaperExtrude` snapped to Z) takes its sections in
//! planes parallel to the profile's, each line offset by its offset's part in the plane
//! (#2497; the fork reads the plane's normal as the height's, and refuses the draft). With
//! `keep_exact_pcurves` (WR-722) an arc's wall on its cone covers just its face, so the
//! B-rep's shrink keeps its exact pcurves. Every seam and pcurve is checked. With the tangent rule a
//! corner that opens a gap continues an arc's end along its tangent line (a flat
//! piece with no edge on the profile); a line's wall continues as itself.
//!
//! Deviations: out-params become return values (a refusal is
//! `Ok(Err(CornerRefusal))`, the fork's `SM_ERR_INVALID_INPUT` with its refusal set);
//! a profile curve is matched to its input by index into the caller's list; a cone
//! wall's seam pcurve has its weights scaled to start at 1 (the fork's grow with the
//! arc's radius, past the weight range of a B-spline, so a large arc's draft was refused); two
//! sections that touch within the rounding of computing their crossing touch (the fork's took
//! rounding's sign, so a tip where a line or an arc touches an arc was refused). No recursion;
//! every loop is bounded by the profile's curve count or a fixed sample count (the arc span
//! count is at most the arc's angle over a quarter turn).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use super::PrimitiveCreationAccess;
use crate::curve::composite_curve::{CompositeCurve, OffsetDirection};
use crate::curve::curve::p2::{CIRCLE_TYPE, LINE_TYPE};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::certified::gamma;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::line_line_closest_point;
use crate::kernel::Kernel;
use crate::math::{max2, min2, rad_to_deg, Point2, Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::p1::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::{BSplineSurface, SurfParamType};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::cone::Cone;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::FaceCurvesInput;
use crate::topology::BrepId;

const OP: &str = "primitive::primitive_creation::p2::taper_walls";

/// Why a draft corner could not be closed (`SmTaperCornerRefusal`, SolidCraft change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaperCornerRefusal {
    /// `SM_TCR_APART`: the continued curves do not meet.
    Apart,
    /// `SM_TCR_NOT_EXACT`: a curve there is neither a line nor an arc, so its wall has
    /// no exact continuation (or the seam is not exact on its walls).
    NotExact,
    /// `SM_TCR_WALL_CLOSES`: a wall closes before the far end: its neighbours meet
    /// across it, or its arc shrinks to its centre (WR-571: a wall closes at the far end,
    /// the whole section closes, or an arc shrinks to its centre).
    WallCloses,
}

/// A refused draft corner: why, and the corner's point on the profile
/// (`pOptRefusal`, `pOptRefusedCorner`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRefusal {
    /// Why the corner could not be closed.
    pub reason: TaperCornerRefusal,
    /// The corner's point on the profile curves.
    pub corner: Point3,
}

/// One profile curve, as its wall reads it (`sm_DraftWall`).
#[derive(Debug, Clone, Default)]
struct Wall {
    /// An arc of a circle, else a line.
    arc: bool,
    /// A line of no length on the profile: an arc's tangent plane at a corner.
    point: bool,
    /// Where the profile runs along it from.
    start: Point3,
    /// And to.
    end: Point3,
    // line
    /// Unit direction of travel.
    dir: Vec3,
    /// The offset at the far end, in the profile's plane.
    offset: Vec3,
    // arc
    center: Point3,
    /// The profile runs counterclockwise about it.
    axis: Vec3,
    x: Vec3,
    /// `axis x x`.
    y: Vec3,
    radius: f64,
    /// The radius at a fraction f of the height is `radius - f * shrink`.
    shrink: f64,
    /// Angles about `axis` from `x`, `end_ang > start_ang`.
    start_ang: f64,
    end_ang: f64,
    // the wall
    /// The parameter range its surface covers.
    lo: f64,
    hi: f64,
    /// Arc: the span ends, `lo` to `hi`.
    breaks: Vec<f64>,
}

/// A corner (or smooth joint) between the curve before and the curve after it
/// (`sm_DraftSeam`); with WR-571, a seam between
/// two walls next to each other over a stretch of the height: a corner of the profile,
/// or one where two walls meet once a wall between them has closed.
#[derive(Debug, Clone, Copy, Default)]
struct Seam {
    /// The seam's control points: the corner (its low end), its shoulder, the far end
    /// (its high end).
    p: Point3,
    s: Point3,
    x: Point3,
    /// The shoulder's weight.
    weight: f64,
    /// WR-571: the fractions of the height at `p` and at `x`.
    f0: f64,
    f1: f64,
}

/// Element `i`, or `SM_ERR` (upstream indexes unchecked).
fn at<T: Copy>(v: &[T], i: usize) -> Result<T> {
    v.get(i)
        .copied()
        .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))
}

/// Element `i` by reference, or `SM_ERR`.
fn at_ref<T>(v: &[T], i: usize) -> Result<&T> {
    v.get(i)
        .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))
}

/// The section of `w` at fraction `f` of the height, in the profile's plane: a line
/// (point, unit direction) or a circle (centre, radius) (`sm_DraftSection`).
fn section(w: &Wall, f: f64) -> (Point3, Vec3, f64) {
    if w.arc {
        (w.center, w.axis, w.radius - f * w.shrink)
    } else {
        (w.start + f * w.offset, w.dir, 0.0)
    }
}

/// Where the sections of `a` and `b` at fraction `f` cross, nearest `near` (all in the
/// profile's plane, whose unit normal is `n`); `None` when they do not cross
/// (`sm_DraftCrossing`). Sections that touch, as at a profile's tip where a line or an arc
/// meets an arc at 0 degrees, come out a hair apart or crossing by rounding: they are apart
/// only where the rounding bound proves it ([`touch_bound`], [`circles_may_meet`]), and touch at
/// one point otherwise.
fn crossing(a: &Wall, b: &Wall, f: f64, n: Vec3, near: Point3) -> Option<Point3> {
    let (pa, da, ra) = section(a, f);
    let (pb, db, rb) = section(b, f);
    let mut cand: Vec<Point3> = Vec::with_capacity(2);
    if !a.arc && !b.arc {
        let den = da.cross(db).dot(n);
        if den.abs() < EFF_ZERO {
            return None;
        }
        let s = (pb - pa).cross(db).dot(n) / den;
        cand.push(pa + s * da);
    } else if a.arc != b.arc {
        let l = if a.arc { pb } else { pa };
        let d = if a.arc { db } else { da };
        let c = if a.arc { pa } else { pb };
        let r = if a.arc { ra } else { rb };
        if r <= 0.0 {
            return None;
        }
        let w = l - c;
        let bb = d.dot(w);
        let disc = bb * bb - (w.dot(w) - r * r);
        if disc >= 0.0 {
            let root = disc.sqrt();
            cand.push(l + (-bb - root) * d);
            cand.push(l + (-bb + root) * d);
        } else {
            // negative (or NaN) as computed: decided from the line's foot from the centre, c + q
            let dd = d.dot(d);
            let q = w - (bb / dd) * d;
            let qn = q.length();
            if !(qn - r <= touch_bound(w.length(), qn, r)) {
                return None;
            }
            let root = ((r - qn) * (r + qn) / dd).max(0.0).sqrt();
            let foot = c + q;
            cand.push(foot - root * d);
            cand.push(foot + root * d);
        }
    } else {
        if ra <= 0.0 || rb <= 0.0 {
            return None;
        }
        let mut e = pb - pa;
        let dd = e.length();
        if dd < EFF_ZERO {
            return None;
        }
        let d2 = e.dot(e);
        e /= dd;
        let mut along = (ra * ra - rb * rb + dd * dd) / (2.0 * dd);
        let mut h2 = ra * ra - along * along;
        if h2 < 0.0 {
            // negative as computed: decided from the radii, the chord from the factored squares
            if !circles_may_meet(d2, ra, rb) {
                return None;
            }
            along = ((ra - rb) * (ra + rb) + dd * dd) / (2.0 * dd);
            h2 = (ra - along) * (ra + along);
        }
        let h = h2.max(0.0).sqrt();
        let q = n.cross(e);
        cand.push(pa + along * e + h * q);
        cand.push(pa + along * e - h * q);
    }
    let mut it = cand.into_iter();
    let mut x = it.next()?;
    for c in it {
        if c.distance_squared(near) < x.distance_squared(near) {
            x = c;
        }
    }
    Some(x)
}

/// A bound on how far [`crossing`]'s computed `|q| - r` lies from the exact one of the given
/// `l`, `c`, `d`, `r`, `q = w - (d.w / d.d) d` with `w = l - c` the line's foot from the
/// centre (the line `l + s d` meets the circle where `|q| <= r`), from the computed `wn = |w|`
/// and `qn = |q|`. Higham's model: `w` rounds once, `d.w` within `gamma_3 |d| |w|`
/// (Cauchy-Schwarz) and `d.d` within `gamma_3 |d|^2`, so their quotient times `d` within
/// `gamma_8` of `|d.w| / |d| <= |w|`, and the difference once, so `q` within `gamma_11 |w| + u
/// |q|`; `|q|` within `gamma_4`, and `|q| - r` rounds once. Taken as `gamma_16` of
/// `|w| + |q| + r` for the computed `|w|` and the bound's own rounding. A line that touches
/// its circle, as at a profile's tip where a line meets an arc at 0 degrees, has `|q| - r`
/// of about 0 and a computed one of either sign; within the bound the foot (or the points
/// either side of it) is within the bound of both sections. Read only where the discriminant
/// `(d.w)^2 - (w.w - r^2)` comes out negative (it rounds with `|w|^2`), so lines that crossed
/// as computed still do.
fn touch_bound(wn: f64, qn: f64, r: f64) -> f64 {
    gamma(16) * (wn + qn + r)
}

/// Whether two circles of radii `ra` and `rb` whose centres' computed squared distance is
/// `d2` are not proved apart (they touch or cross): they meet where
/// `(ra - rb)^2 <= D^2 <= (ra + rb)^2` (from
/// `4 D^2 h^2 = ((ra + rb)^2 - D^2)(D^2 - (ra - rb)^2)`, `h` the half chord), which
/// [`crossing`]'s `h^2 = ra^2 - along^2` decides badly conditioned (`along` divides by D);
/// where they meet, the chord is then taken from `(ra - rb)(ra + rb)` and
/// `(ra - along)(ra + along)`, which round little where the radii are close.
/// `d2` is within `gamma_5` of the exact `D^2` (the difference rounds once, the squares and
/// their sum), each square of a radius sum or difference within `gamma_3`; taken as
/// `gamma_12` of the two sides for the comparison's own rounding. Read only where `h^2` comes
/// out negative, so circles that crossed as computed still do.
fn circles_may_meet(d2: f64, ra: f64, rb: f64) -> bool {
    let lo = (ra - rb) * (ra - rb);
    let hi = (ra + rb) * (ra + rb);
    d2 + gamma(12) * (d2 + lo) >= lo && d2 - gamma(12) * (d2 + hi) <= hi
}

/// The distance from `q` (in the profile's plane) to the section of `w` at fraction
/// `f` (`sm_DraftOffSection`).
fn off_section(w: &Wall, f: f64, q: Point3) -> f64 {
    let (p, d, r) = section(w, f);
    let v = q - p;
    if w.arc {
        return (v.length() - r).abs();
    }
    (v - v.dot(d) * d).length()
}

/// The wall's normal (unnormalised) at `q`, a point of its section (in the profile's
/// plane), for the height `h`: the curve's tangent across the ruling there
/// (`sm_DraftNormal`).
fn wall_normal(w: &Wall, q: Point3, h: Vec3) -> Result<Vec3> {
    if !w.arc {
        return Ok(w.dir.cross(w.offset + h));
    }
    let r = (q - w.center).unitize()?;
    let t = w.axis.cross(r);
    Ok(t.cross(h - w.shrink * r))
}

/// The angle of `q` about the arc's axis, unwrapped to within half a turn of `near`
/// (`sm_DraftAngle`).
fn angle(w: &Wall, q: Point3, near: f64) -> f64 {
    let r = q - w.center;
    let mut a = r.dot(w.y).atan2(r.dot(w.x));
    while a < near - PI {
        a += 2.0 * PI;
    }
    while a > near + PI {
        a -= 2.0 * PI;
    }
    a
}

/// The wall's parameter of `q` (in the profile's plane, on its section at fraction
/// `f`) (`sm_DraftParam`).
fn param(w: &Wall, q: Point3, f: f64, near: f64) -> Result<f64> {
    if !w.arc {
        return Ok((q - w.start - f * w.offset).dot(w.dir));
    }
    // the span holding the angle, then its parameter: tan((a - mid)/2) = tan(half/2) (2 tau - 1)
    let a = angle(w, q, near);
    let n = w.breaks.len();
    let mut j = 0;
    while j + 2 < n && a > at(&w.breaks, j + 1)? {
        j += 1;
    }
    let t0 = at(&w.breaks, j)?;
    let t1 = at(&w.breaks, j + 1)?;
    let half = 0.5 * (t1 - t0);
    let tau = 0.5 * ((0.5 * (a - 0.5 * (t0 + t1))).tan() / (0.5 * half).tan() + 1.0);
    Ok(t0 + (t1 - t0) * tau)
}

/// The point of the seam at `s` in `[0, 1]` (`sm_DraftSeamPoint`).
fn seam_point(seam: &Seam, s: f64) -> Point3 {
    let b0 = (1.0 - s) * (1.0 - s);
    let b1 = 2.0 * s * (1.0 - s) * seam.weight;
    let b2 = s * s;
    (b0 * seam.p + b1 * seam.s + b2 * seam.x) / (b0 + b1 + b2)
}

/// `SmBSplineCurve::CreateCanonical` with piecewise Bezier knots and no trim.
fn canonical(
    dim: u64,
    degree: usize,
    points: &[Point3],
    form: BSplineCurveForm,
    mults: &[usize],
    knots: &[f64],
    weights: Option<&[f64]>,
) -> Result<BSplineCurve> {
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree,
        control_points: points,
        form,
        knot_multiplicities: mults,
        knots,
        knot_type: KnotType::PiecewiseBezierKnots,
        weights,
        trim: None,
    })
}

/// The wall's curve at fraction `f` (0 the profile's curve, 1 the far end's), over
/// `lo..hi` (`sm_DraftWallCurve`).
fn wall_curve(w: &Wall, f: f64, h: Vec3) -> Result<BSplineCurve> {
    let mut points = Vec::new();
    let mut weights = Vec::new();
    let mut mults = Vec::new();
    let mut knots = Vec::new();
    if !w.arc {
        let base = w.start + f * (w.offset + h);
        points.push(base + w.lo * w.dir);
        points.push(base + w.hi * w.dir);
        mults.push(2);
        mults.push(2);
        knots.push(w.lo);
        knots.push(w.hi);
        return canonical(
            3,
            1,
            &points,
            BSplineCurveForm::PolylineForm,
            &mults,
            &knots,
            None,
        );
    }
    let r = w.radius - f * w.shrink;
    let c = w.center + f * h;
    let n = w.breaks.len();
    let mut j = 0;
    while j + 1 < n {
        let t0 = at(&w.breaks, j)?;
        let t1 = at(&w.breaks, j + 1)?;
        let half = 0.5 * (t1 - t0);
        let mid = 0.5 * (t0 + t1);
        if j == 0 {
            points.push(c + r * (t0.cos() * w.x + t0.sin() * w.y));
            weights.push(1.0);
        }
        points.push(c + (r / half.cos()) * (mid.cos() * w.x + mid.sin() * w.y));
        weights.push(half.cos());
        points.push(c + r * (t1.cos() * w.x + t1.sin() * w.y));
        weights.push(1.0);
        knots.push(t0);
        mults.push(if j == 0 { 3 } else { 2 });
        j += 1;
    }
    knots.push(at(&w.breaks, n.wrapping_sub(1))?);
    mults.push(3);
    canonical(
        3,
        2,
        &points,
        BSplineCurveForm::CircularArc,
        &mults,
        &knots,
        Some(&weights),
    )
}

/// Bernstein coefficients of the product of two quadratics (`sm_Quartic`).
fn quartic(a: [f64; 3], b: [f64; 3]) -> [f64; 5] {
    const BINOM2: [f64; 3] = [1.0, 2.0, 1.0];
    const BINOM4: [f64; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];
    let mut c = [0.0; 5];
    for (k, (ck, bk4)) in c.iter_mut().zip(BINOM4).enumerate() {
        *ck = 0.0;
        for (i, (ai, bi2)) in a.iter().zip(BINOM2).enumerate() {
            let Some(j) = k.checked_sub(i) else {
                continue;
            };
            let (Some(bj), Some(bj2)) = (b.get(j), BINOM2.get(j)) else {
                continue;
            };
            *ck += bi2 * bj2 * ai * bj;
        }
        *ck /= bk4;
    }
    c
}

/// The real roots of `qa s^2 + 2 qb s + qc`, `qa` taken for zero within `EFF_ZERO * scale`
/// (the linear root then), in the stable form.
fn half_quadratic_roots(qa: f64, qb: f64, qc: f64, scale: f64) -> Vec<f64> {
    let mut r = Vec::with_capacity(2);
    if qa.abs() <= EFF_ZERO * scale {
        if qb != 0.0 {
            r.push(-qc / (2.0 * qb));
        }
    } else {
        let disc = qb * qb - qa * qc;
        if disc >= 0.0 {
            let q = -(qb + if qb >= 0.0 { disc.sqrt() } else { -disc.sqrt() });
            if q != 0.0 {
                r.push(qc / q);
            }
            r.push(q / qa);
        }
    }
    r
}

/// The least and greatest angle about the arc wall `w`'s axis, unwrapped near `near`, of
/// the seam's points in the profile's plane (each less its height): at the seam's ends,
/// or where its angle turns. The point less its height and the centre is `Q(s) / W(s)`,
/// `Q` a quadratic (the height fraction is affine in space) and `W > 0`, so the angle
/// turns where `Q x Q'` about the axis vanishes, `(b x a) s^2 + 2 (c x a) s + c x b` for
/// `Q = a s^2 + b s + c`: a quadratic's roots (#2497).
fn seam_angles(w: &Wall, seam: &Seam, n: Vec3, h: Vec3, near: f64) -> (f64, f64) {
    let height = h.dot(n);
    let in_plane = |q: Point3| q - (seam.f0 + (q - seam.p).dot(n) / height) * h;
    let [q0, q1, q2] = [(seam.p, 1.0), (seam.s, seam.weight), (seam.x, 1.0)].map(|(c, wk)| {
        let r = in_plane(c) - w.center;
        (wk * r.dot(w.x), wk * r.dot(w.y))
    });
    let a = (q0.0 - 2.0 * q1.0 + q2.0, q0.1 - 2.0 * q1.1 + q2.1);
    let b = (2.0 * (q1.0 - q0.0), 2.0 * (q1.1 - q0.1));
    let cross = |u: (f64, f64), v: (f64, f64)| u.0 * v.1 - u.1 * v.0;
    let (qa, qb, qc) = (cross(b, a), cross(q0, a), cross(q0, b));
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let turns = half_quadratic_roots(qa, qb, qc, qa.abs() + qb.abs() + qc.abs());
    for t in [0.0, 1.0]
        .into_iter()
        .chain(turns.into_iter().filter(|t| *t > 0.0 && *t < 1.0))
    {
        let ang = angle(w, in_plane(seam_point(seam, t)), near);
        lo = min2(lo, ang);
        hi = max2(hi, ang);
    }
    (lo, hi)
}

/// The seam's pcurve on wall `w`, whose surface parameters are
/// `(u0 + f (u1 - u0), v0 + (t - t0) * v_scale)` for the wall's fraction `f` and curve
/// parameter `t` (`sm_DraftSeamPcurve`). On an arc's wall the seam is cut where it
/// crosses from one span to the next, each piece the rational quartic of its span, its
/// weights scaled to start at 1. `SM_ERR` when a piece leaves its span or a weight is not
/// positive.
#[allow(clippy::too_many_arguments)] // the fork's arguments, and the WR-571 mode
fn seam_pcurve(
    w: &Wall,
    seam: &Seam,
    n: Vec3,
    h: Vec3,
    [u0, u1]: [f64; 2],
    t0: f64,
    v0: f64,
    v_scale: f64,
    apex: bool,
) -> Result<BSplineCurve> {
    let cp = [seam.p, seam.s, seam.x];
    let sw = [1.0, seam.weight, 1.0];
    let height = h.dot(n);
    // f of each control point (the height fraction is affine in space; WR-571: from the
    // seam's low end)
    let f0 = if apex { seam.f0 } else { 0.0 };
    let sf = cp.map(|c| f0 + (c - seam.p).dot(n) / height);
    let mut points = Vec::new();
    let mut weights = Vec::new();
    let knots = [0.0, 1.0];
    if !w.arc {
        // a plane: (f, t) affine in space
        for ((c, f), wk) in cp.iter().zip(sf).zip(sw) {
            let q = *c - f * h;
            let t = (q - w.start - f * w.offset).dot(w.dir);
            points.push(Point3::new(
                u0 + f * (u1 - u0),
                v0 + (t - t0) * v_scale,
                0.0,
            ));
            weights.push(wk);
        }
        return canonical(
            2,
            2,
            &points,
            BSplineCurveForm::Unspecified,
            &[3, 3],
            &knots,
            Some(&weights),
        );
    }
    // a cone: in each span of the wall's surface the seam crosses, its piece's pcurve is a
    // rational quartic (the span's parameter is a Moebius map of tan of the half angle, a
    // ratio of quadratics along the conic); the seam is cut where its angle crosses a
    // span's end (a quadratic's root) (the fork's tangency-labs/smlib@3ae7bfa)
    let nb = w.breaks.len();
    if nb < 2 {
        return Err(SolidError::primitive_failed(
            OP,
            "a seam span has fewer than two breaks",
        ));
    }
    // the seam's homogeneous control points and their height fractions
    let swh = sw;
    let sfh = sf;
    let sph = [cp[0] * sw[0], cp[1] * sw[1], cp[2] * sw[2]];
    // the seam's parameters where it crosses a span's end: c(s) . n_b = 0 for the end's
    // half-plane normal n_b, c(s) the seam's homogeneous point less its height, less the
    // centre
    let mut cuts = vec![0.0];
    {
        let mut roots = Vec::new();
        for b in 1..nb.saturating_sub(1) {
            let db_ang = at(&w.breaks, b)?;
            let nrm = -db_ang.sin() * w.x + db_ang.cos() * w.y;
            let dir = db_ang.cos() * w.x + db_ang.sin() * w.y;
            let mut c = [0.0; 3];
            let mut d = [0.0; 3];
            for ((ck, dk), ((p, f), wk)) in c
                .iter_mut()
                .zip(d.iter_mut())
                .zip(cp.iter().zip(sf).zip(sw))
            {
                let r = *p - f * h - w.center;
                *ck = wk * r.dot(nrm);
                *dk = wk * r.dot(dir);
            }
            // c0 (1-s)^2 + 2 c1 s (1-s) + c2 s^2 = a s^2 + 2 b s + c0
            let qa = c[0] - 2.0 * c[1] + c[2];
            let qb = c[1] - c[0];
            let qc = c[0];
            let r = half_quadratic_roots(qa, qb, qc, c[0].abs() + c[1].abs() + c[2].abs());
            for t in r {
                if !(t > EFF_ZERO && t < 1.0 - EFF_ZERO) {
                    continue;
                }
                // on the end's own half-plane, not its opposite
                let dd = (1.0 - t) * (1.0 - t) * d[0] + 2.0 * t * (1.0 - t) * d[1] + t * t * d[2];
                if dd > 0.0 {
                    roots.push(t);
                }
            }
        }
        roots.sort_by(f64::total_cmp);
        cuts.extend(roots);
    }
    cuts.push(1.0);
    // the homogeneous seam point at blossom (x, y): B(x, x) is the point at x
    let blossom = |x: f64, y: f64| -> (Vec3, f64, f64) {
        let c0 = (1.0 - x) * (1.0 - y);
        let c1 = (1.0 - x) * y + x * (1.0 - y);
        let c2 = x * y;
        (
            c0 * sph[0] + c1 * sph[1] + c2 * sph[2],
            c0 * swh[0] + c1 * swh[1] + c2 * swh[2],
            c0 * swh[0] * sfh[0] + c1 * swh[1] * sfh[1] + c2 * swh[2] * sfh[2],
        )
    };
    // the homogeneous seam point at s (`fnAt`)
    let point_at = |t: f64| -> (Vec3, f64, f64) {
        let b0 = (1.0 - t) * (1.0 - t);
        let b1 = 2.0 * t * (1.0 - t);
        let b2 = t * t;
        (
            b0 * sph[0] + b1 * sph[1] + b2 * sph[2],
            b0 * swh[0] + b1 * swh[1] + b2 * swh[2],
            b0 * swh[0] * sfh[0] + b1 * swh[1] * sfh[1] + b2 * swh[2] * sfh[2],
        )
    };
    let mut prev_w = 0.0;
    for (piece, pair) in cuts.windows(2).enumerate() {
        let (Some(&c0), Some(&c1)) = (pair.first(), pair.get(1)) else {
            return Err(SolidError::primitive_failed(OP, "a seam cut is missing"));
        };
        // the piece as a rational quadratic: its ends and the crossing of their tangents,
        // by blossoming the homogeneous curve: B(t0, t0), B(t0, t1), B(t1, t1)
        let q = [blossom(c0, c0), blossom(c0, c1), blossom(c1, c1)];
        // the span holding the piece: its middle's angle
        let (mid_p, mid_w, mid_f) = point_at(0.5 * (c0 + c1));
        let mid_q = mid_p / mid_w - (mid_f / mid_w) * h;
        let am = angle(w, mid_q, 0.5 * (w.lo + w.hi));
        let mut j = 0;
        while j + 2 < nb && am > at(&w.breaks, j + 1)? {
            j += 1;
        }
        let s0 = at(&w.breaks, j)?;
        let s1 = at(&w.breaks, j + 1)?;
        let mid = 0.5 * (s0 + s1);
        let tan_q = (0.25 * (s1 - s0)).tan();
        let xm = mid.cos() * w.x + mid.sin() * w.y;
        let ym = w.axis.cross(xm);
        // tau = (dy + tanq (rho + dx)) / (2 tanq (rho + dx)), each term affine in space
        let mut num = [0.0; 3];
        let mut den = [0.0; 3];
        let mut su = [0.0; 3];
        let mut wt = [0.0; 3];
        for (k, (qp, qw, qf)) in q.iter().copied().enumerate() {
            let f = qf / qw;
            let c = qp / qw;
            let r = c - f * h - w.center;
            let rho = w.radius - f * w.shrink;
            let dx = r.dot(xm);
            let dy = r.dot(ym);
            if let (Some(nk), Some(dk), Some(uk), Some(tk)) =
                (num.get_mut(k), den.get_mut(k), su.get_mut(k), wt.get_mut(k))
            {
                *nk = qw * (dy + tan_q * (rho + dx));
                *dk = qw * 2.0 * tan_q * (rho + dx);
                *uk = qw * (u0 + f * (u1 - u0));
                *tk = qw;
            }
        }
        let d4 = quartic(wt, den);
        let u4 = quartic(su, den);
        let t4 = quartic(num, wt);
        // the pieces join with one weight: scale this piece's to the last one's end, and the
        // first piece's to start at 1 (scaled by one factor the curve is the same; as built
        // the weights grow with the arc's radius, past the range a B-spline takes)
        let scale = if !(d4[0] > 0.0) {
            1.0
        } else if piece == 0 {
            1.0 / d4[0]
        } else {
            prev_w / d4[0]
        };
        for (k, ((dk, uk), tk)) in d4.iter().zip(u4).zip(t4).enumerate() {
            if !(*dk > 0.0) {
                return Err(SolidError::primitive_failed(
                    OP,
                    "a seam span weight is not positive",
                ));
            }
            let tau = tk / dk;
            if tau < -EFF_ZERO_SQRT || tau > 1.0 + EFF_ZERO_SQRT {
                // the piece leaves its span
                return Err(SolidError::primitive_failed(
                    OP,
                    "a seam piece leaves its span",
                ));
            }
            let t = s0 + (s1 - s0) * tau;
            if piece > 0 && k == 0 {
                // shared with the last piece's end
                continue;
            }
            points.push(Point3::new(uk / dk, v0 + (t - t0) * v_scale, 0.0));
            weights.push(*dk * scale);
        }
        prev_w = d4[4] * scale;
    }
    let last = cuts.len().saturating_sub(1);
    let mults: Vec<usize> = (0..cuts.len())
        .map(|k| if k == 0 || k == last { 5 } else { 4 })
        .collect();
    canonical(
        2,
        4,
        &points,
        BSplineCurveForm::Unspecified,
        &mults,
        &cuts,
        Some(&weights),
    )
}

/// A 2D line from `(ua, va)` to `(ub, vb)` over `[t0, t1]` (`sm_DraftIsoPcurve`).
fn iso_pcurve(ua: f64, va: f64, ub: f64, vb: f64, t0: f64, t1: f64) -> Result<BSplineCurve> {
    canonical(
        2,
        1,
        &[Point3::new(ua, va, 0.0), Point3::new(ub, vb, 0.0)],
        BSplineCurveForm::PolylineForm,
        &[2, 2],
        &[t0, t1],
        None,
    )
}

/// The seam as a curve: the rational quadratic on `[0, 1]` (`sm_DraftSeamCurve`).
fn seam_curve(seam: &Seam) -> Result<BSplineCurve> {
    canonical(
        3,
        2,
        &[seam.p, seam.s, seam.x],
        BSplineCurveForm::Unspecified,
        &[3, 3],
        &[0.0, 1.0],
        Some(&[1.0, seam.weight, 1.0]),
    )
}

/// An arc's wall as its exact cone, in the cone's own parametrization (`sm_DraftCone`,
/// the fork's tangency-labs/smlib@3ae7bfa): about the arc's axis (its angles the
/// wall's, from `x`), NURBS `u` along the height from the profile's curve (`f = 0`) to
/// the far end's (`f = 1`), NURBS `v` round the angles `lo..hi` in the cone's equal
/// spans of at most a quarter turn. Its normal is the ruled wall's. `None` for a line's
/// wall, or when the height is not along the axis (an oblique wall is no cone of
/// revolution about it).
fn draft_cone(k: &Kernel, w: &Wall, h: Vec3) -> Result<Option<Cone>> {
    if !on_cone(w, h) {
        return Ok(None);
    }
    let along = h.dot(w.axis);
    let r0 = w.radius;
    let r1 = w.radius - w.shrink;
    // the profile's end (f = 0) at the base when the height runs along the axis, else at
    // the top with the linear direction run top to bottom (the cone's `inside_out`)
    let down = along < 0.0;
    let origin = Axis2Placement::new(if down { w.center + h } else { w.center }, w.x, w.y);
    let mut cone = Cone::create_canonical(
        k,
        &origin,
        if down { r1 } else { r0 },
        if down { r0 } else { r1 },
        along.abs(),
        true,
        down,
    )?;
    cone.adjust_step_uv_domain(
        k,
        &Extent2d::from_bounds(rad_to_deg(w.lo), 0.0, rad_to_deg(w.hi), along.abs()),
    )?;
    Ok(Some(cone))
}

/// Whether `w` is an arc whose wall is its cone ([`draft_cone`]): the height `h` runs
/// along its axis.
fn on_cone(w: &Wall, h: Vec3) -> bool {
    let along = h.dot(w.axis);
    w.arc && (h - along * w.axis).length() <= EFF_ZERO * (1.0 + along.abs())
}

/// The angles an arc's wall on its cone covers with `keep_exact_pcurves` (WR-722): just
/// its face, from the least angle of its seams to the greatest ([`seam_angles`]; the
/// face's bottom and top arcs run between the seams' ends), so `SmBrep::ShrinkGeometry`
/// finds the face's domain its surface's and keeps its exact pcurves. The reach padded
/// past the seams leaves the face short of its surface, which `ShrinkGeometry` trims,
/// removing every trim curve to project the edges again (#2497). `None` otherwise.
fn cone_face_angles<'s>(
    k: &Kernel,
    w: &Wall,
    seams: impl IntoIterator<Item = (&'s Seam, f64)>,
    n: Vec3,
    h: Vec3,
) -> Option<(f64, f64)> {
    if !k.fixes.keep_exact_pcurves || !on_cone(w, h) {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (seam, near) in seams {
        let (a, b) = seam_angles(w, seam, n, h, near);
        lo = min2(lo, a);
        hi = max2(hi, b);
    }
    Some((lo, hi))
}

/// The tangent at `t` (`Evaluate(t, 1, TRUE, v)`), `None` when it fails.
fn tangent(k: &Kernel, c: &Curve, t: f64) -> Option<Vec3> {
    c.evaluate(k, t, 1, true, true)
        .ok()
        .and_then(|v| v.get(1).copied())
}

/// Whether the profile, its segments in `composite`, has a sharp corner
/// (`sm_DraftHasCorner`).
pub(super) fn has_corner(k: &Kernel, composite: &CompositeCurve, n: Vec3) -> bool {
    let canon = composite.canonical();
    let count = canon.curves.len();
    for i in 0..count {
        let prev = (i + count - 1) % count;
        let (Some(a), Some(b), Some(&sa), Some(&sb)) = (
            canon.curves.get(prev),
            canon.curves.get(i),
            canon.senses.get(prev),
            canon.senses.get(i),
        ) else {
            return false;
        };
        let (ia, ib) = (a.natural_interval(), b.natural_interval());
        let Some(mut va) = tangent(k, a, if sa { ia.max() } else { ia.min() }) else {
            return false;
        };
        let Some(mut vb) = tangent(k, b, if sb { ib.min() } else { ib.max() }) else {
            return false;
        };
        if !sa {
            va = -va;
        }
        if !sb {
            vb = -vb;
        }
        let Ok(turn) = n.ccw_angle_between(va, vb) else {
            return false;
        };
        if turn.abs() > EFF_ZERO_SQRT {
            return true;
        }
    }
    false
}

/// The line or circle among `inputs` that the profile's segment `c` is (its copy,
/// split at tangent breaks, is a plain B-spline; a line or a circle has none, so it is
/// the one with the same ends and middle), or `None` (`sm_DraftInput`).
fn draft_input<'a>(
    k: &Kernel,
    c: &Curve,
    inputs: &'a [Curve],
    tol: f64,
) -> Result<Option<&'a Curve>> {
    let ivl = c.natural_interval();
    let start = c.evaluate_point(k, ivl.min())?;
    let end = c.evaluate_point(k, ivl.max())?;
    let mid = c.evaluate_point(k, ivl.mid())?;
    for input in inputs {
        if !(input.is_kind_of(CIRCLE_TYPE) || input.is_kind_of(LINE_TYPE)) {
            continue;
        }
        let j = input.natural_interval();
        // (GetEnds' status is ignored)
        let (a, b) = input
            .ends(k)
            .map_or((Point3::ZERO, Point3::ZERO), |e| (e.start, e.end));
        let m = input.evaluate_point(k, j.mid())?;
        let ends = (a.distance(start) <= tol && b.distance(end) <= tol)
            || (b.distance(start) <= tol && a.distance(end) <= tol);
        if ends && m.distance(mid) <= tol {
            return Ok(Some(input));
        }
    }
    Ok(None)
}

/// Whether every segment of the profile `composite` is a line or an arc among
/// `inputs` (`sm_DraftLinesAndArcs`).
pub(super) fn lines_and_arcs(
    k: &Kernel,
    composite: &CompositeCurve,
    inputs: &[Curve],
    tol: f64,
) -> Result<bool> {
    for c in composite.canonical().curves {
        if draft_input(k, c, inputs, tol)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A refusal of `reason` at `corner`.
const fn refuse(reason: TaperCornerRefusal, corner: Point3) -> Walls {
    Err(CornerRefusal { reason, corner })
}

/// The far end's curves, or the refused corner.
type Walls = core::result::Result<Vec<Curve>, CornerRefusal>;

/// The inputs of [`curvature_keeping_walls`].
pub(super) struct WallsInput<'a> {
    /// `bTangent`: gaps closed by the walls' tangent planes.
    pub tangent: bool,
    /// The profile, one closed chain.
    pub composite: &'a CompositeCurve,
    /// The curves it was made from (its lines and circles).
    pub inputs: &'a [Curve],
    /// The unit normal the height runs along (`CreateTaperExtrude`'s): the profile's
    /// plane's, or near it where `CreateTaperExtrude` snapped a normal near Z to Z (the
    /// draft then leans off the plane: `profile_plane`).
    pub normal: Vec3,
    /// `+` along the normal, `-` opposite.
    pub height: f64,
    /// How far the far end's section is offset (> 0).
    pub offset: f64,
    /// The side it is offset to.
    pub side: OffsetDirection,
    /// The B-rep's tolerance.
    pub tol: f64,
}

/// The walls of the profile, read from its segments (the first loop of
/// `sm_CreateCurvatureKeepingWalls`).
fn read_walls(
    k: &Kernel,
    input: &WallsInput<'_>,
) -> Result<core::result::Result<Vec<Wall>, CornerRefusal>> {
    let WallsInput {
        composite,
        inputs,
        normal: n,
        offset,
        side,
        tol,
        ..
    } = *input;
    let side = if side == OffsetDirection::LeftHandSide {
        1.0
    } else {
        -1.0
    };
    let canon = composite.canonical();
    let mut walls = Vec::new();
    for (pc, &sense) in canon.curves.iter().zip(&canon.senses) {
        let mut w = Wall::default();
        let ivl = pc.natural_interval();
        w.start = pc.evaluate_point(k, if sense { ivl.min() } else { ivl.max() })?;
        w.end = pc.evaluate_point(k, if sense { ivl.max() } else { ivl.min() })?;
        let v = pc.evaluate(k, if sense { ivl.min() } else { ivl.max() }, 1, true, true)?;
        let v1 = at(&v, 1)?;
        let t = if sense { v1 } else { -v1 };
        let found = draft_input(k, pc, inputs, tol)?;
        match found.map(Curve::kind) {
            Some(CurveKind::Ellipse(circle))
                if found.is_some_and(|c| c.is_kind_of(CIRCLE_TYPE)) =>
            {
                let frame = circle.position();
                w.arc = true;
                w.center = frame.origin();
                w.radius = circle.radius();
                w.x = frame.x_axis().unitize()?;
                let kk = w.x.cross(frame.y_axis()).unitize()?;
                // counterclockwise about the axis it is run along
                w.axis = if kk.cross(w.start - w.center).dot(t) >= 0.0 {
                    kk
                } else {
                    -kk
                };
                w.y = w.axis.cross(w.x);
                w.shrink = side * n.dot(w.axis) * offset;
                w.start_ang = angle(&w, w.start, 0.0);
                w.end_ang = angle(&w, w.end, w.start_ang);
                if w.end_ang <= w.start_ang + EFF_ZERO_SQRT {
                    w.end_ang += 2.0 * PI;
                }
                if w.radius - w.shrink <= tol {
                    let corner = pc.evaluate_point(k, ivl.mid())?;
                    return Ok(Err(CornerRefusal {
                        reason: TaperCornerRefusal::WallCloses,
                        corner,
                    }));
                }
            }
            Some(_) => {
                w.arc = false;
                w.dir = (w.end - w.start).unitize()?;
                w.offset = side * offset * n.cross(w.dir);
            }
            None => {
                return Ok(Err(CornerRefusal {
                    reason: TaperCornerRefusal::NotExact,
                    corner: w.start,
                }));
            }
        }
        walls.push(w);
    }
    Ok(Ok(walls))
}

/// The unit normal of the profile's plane, on the side of `d`, the direction the height
/// runs along: `d` itself when every wall lies within `tol` of the plane through the
/// profile's first corner normal to `d` (an arc by its largest distance from it,
/// `|(c - p) . d| + r |(x . d, y . d)|`), else the plane the walls span (an arc's axis,
/// or the cross of the two lines' directions furthest from parallel). The two differ
/// where `CreateTaperExtrude` snapped the computed normal of a profile on a plane tilted
/// from XY (`|n.z| > 0.98`) to Z: the draft leans, each section in a plane parallel to
/// the profile's, moved along `d` (#2497; the fork takes `d` for the plane's normal, so
/// a leaning draft's seams leave their walls and it is refused).
fn profile_plane(walls: &[Wall], d: Vec3, tol: f64) -> Result<Vec3> {
    let first = at_ref(walls, 0)?.start;
    let off = |q: Point3| (q - first).dot(d).abs();
    let in_plane = walls.iter().all(|w| {
        if w.arc {
            off(w.center) + w.radius * w.x.dot(d).hypot(w.y.dot(d)) <= tol
        } else {
            off(w.start) <= tol && off(w.end) <= tol
        }
    });
    if in_plane {
        return Ok(d);
    }
    let m = if let Some(arc) = walls.iter().find(|w| w.arc) {
        arc.axis
    } else {
        let mut best = Vec3::ZERO;
        for (i, wa) in walls.iter().enumerate() {
            for wb in walls.iter().skip(i + 1) {
                let c = wa.dir.cross(wb.dir);
                if c.length_squared() > best.length_squared() {
                    best = c;
                }
            }
        }
        best.unitize()?
    };
    Ok(if m.dot(d) < 0.0 { -m } else { m })
}

/// A leaning draft's lines (the profile's plane `n` not normal to `d`): each line's
/// offset, `side offset d x dir`, becomes its part in the profile's plane,
/// `side offset (d . n) n x dir`, so the far end's section lies in a plane parallel to
/// the profile's, offset as an arc's is (`shrink = side offset (d . axis)`). A line's
/// offset lies in the plane already when it is normal to `d`.
fn lean_offsets(walls: &mut [Wall], n: Vec3, d: Vec3) {
    if n == d {
        return;
    }
    for w in walls.iter_mut().filter(|w| !w.arc) {
        w.offset -= w.offset.dot(n) * n;
    }
}

/// Tangent planes: where a corner opens a gap, an arc's end continues along its
/// tangent line, a wall of no length on the profile (a line's wall continues as
/// itself) (the `bTangent` step of `sm_CreateCurvatureKeepingWalls`).
fn with_tangent_planes(
    walls: &[Wall],
    n: Vec3,
    d: Vec3,
    offset: f64,
    side: f64,
) -> Result<Vec<Wall>> {
    let count = walls.len();
    let mut out = Vec::new();
    for i in 0..count {
        let a = at_ref(walls, (i + count - 1) % count)?;
        let b = at_ref(walls, i)?;
        let ta = if a.arc {
            a.axis.cross(b.start - a.center)
        } else {
            a.dir
        };
        let tb = if b.arc {
            b.axis.cross(b.start - b.center)
        } else {
            b.dir
        };
        let ta = ta.unitize()?;
        let tb = tb.unitize()?;
        let turn = n.ccw_angle_between(ta, tb)?;
        // the offset parts where the corner turns away from the side it goes to
        let gap = turn.abs() > EFF_ZERO_SQRT && side * turn < 0.0;
        if gap {
            for (arc, dir) in [(a.arc, ta), (b.arc, tb)] {
                if !arc {
                    continue;
                }
                // after the arc ending here (side 0), before the one starting here (side 1)
                out.push(Wall {
                    arc: false,
                    point: true,
                    start: b.start,
                    end: b.start,
                    dir,
                    offset: side * offset * d.cross(dir),
                    ..Wall::default()
                });
            }
        }
        out.push(b.clone());
    }
    Ok(out)
}

// ---- WR-571: walls that close partway up (the fork's tangency-labs/smlib@183ea65) ------

/// The unit tangent of `w`'s section at `q` (in the profile's plane), the way the
/// profile runs (`sm_DraftTangent`).
fn tangent_at(w: &Wall, q: Point3) -> Result<Vec3> {
    let t = if w.arc {
        w.axis.cross(q - w.center)
    } else {
        w.dir
    };
    t.unitize()
}

/// The seam where walls `a` and `b` cross, from `seam.p` (at the fraction `seam.f0` of
/// the height) to `seam.x` (at `seam.f1`), both on the crossing: the rational quadratic
/// through them with the walls' common tangents there, through their crossing halfway
/// up. A seam that is not that conic (checked on both walls) is refused
/// (`sm_DraftSharpSeam`).
fn sharp_seam(
    a: &Wall,
    b: &Wall,
    n: Vec3,
    h: Vec3,
    tol: f64,
    seam: &mut Seam,
) -> Result<core::result::Result<(), CornerRefusal>> {
    let height = h.dot(n);
    let p2 = seam.p - seam.f0 * h;
    let x2 = seam.x - seam.f1 * h;
    let f_mid = 0.5 * (seam.f0 + seam.f1);
    let refusal = |reason| Ok(Err(CornerRefusal { reason, corner: p2 }));
    let Some(mut mid) = crossing(a, b, f_mid, n, p2) else {
        return refusal(TaperCornerRefusal::Apart);
    };
    mid += f_mid * h;
    // the walls' normals at both ends
    let na = wall_normal(a, p2, h)?;
    let nb = wall_normal(b, p2, h)?;
    let na1 = wall_normal(a, x2, h)?;
    let nb1 = wall_normal(b, x2, h)?;
    let mut t0 = na.cross(nb);
    let mut t1 = na1.cross(nb1);
    if t0.dot(h) < 0.0 {
        t0 = -t0;
    }
    if t1.dot(h) < 0.0 {
        t1 = -t1;
    }
    let p = seam.p;
    let chord = seam.x - p;
    let scale = chord.length();
    let t0 = t0.unitize()?;
    let t1 = t1.unitize()?;
    let off_mid = (mid - p) - ((mid - p).dot(chord) / chord.dot(chord)) * chord;
    if off_mid.length() <= EFF_ZERO_SQRT * scale {
        // a straight seam (two planes)
        seam.s = 0.5 * (p + seam.x);
        seam.weight = 1.0;
    } else {
        // the shoulder where the end tangents cross, the weight from the halfway point; end
        // tangents that are parallel have none, and no conic through a middle off the chord
        let Ok((d0, d1)) = line_line_closest_point(p, t0, seam.x, t1) else {
            return refusal(TaperCornerRefusal::NotExact);
        };
        let s0 = p + d0 * t0;
        let s1 = seam.x + d1 * t1;
        if s0.distance(s1) > EFF_ZERO_SQRT * scale || !(d0 > 0.0) || !(d1 < 0.0) {
            return refusal(TaperCornerRefusal::NotExact);
        }
        seam.s = 0.5 * (s0 + s1);
        // M - P = b1 (S - P) + b2 (X - P)
        let e1 = seam.s - p;
        let e2 = seam.x - p;
        let m = mid - p;
        let a11 = e1.dot(e1);
        let a12 = e1.dot(e2);
        let a22 = e2.dot(e2);
        let r1 = m.dot(e1);
        let r2 = m.dot(e2);
        let det = a11 * a22 - a12 * a12;
        let b1 = (r1 * a22 - r2 * a12) / det;
        let b2 = (a11 * r2 - a12 * r1) / det;
        let b0 = 1.0 - b1 - b2;
        if !(det.abs() > 0.0) || !(b0 > 0.0) || !(b1 > 0.0) || !(b2 > 0.0) {
            return refusal(TaperCornerRefusal::NotExact);
        }
        seam.weight = b1 / (2.0 * (b0 * b2).sqrt());
    }
    // the seam must lie on both walls
    for k in 1..8 {
        let q = seam_point(seam, f64::from(k) / 8.0);
        let f = seam.f0 + (q - seam.p).dot(n) / height;
        let qp = q - f * h;
        if off_section(a, f, qp) > tol || off_section(b, f, qp) > tol {
            return refusal(TaperCornerRefusal::NotExact);
        }
    }
    Ok(Ok(()))
}

/// What a piece's seam follows (`sm_DraftPiece::eKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    /// Where the two walls cross.
    Crossing,
    /// Their common ruling (a smooth joint).
    Ruling,
    /// The line where two walls facing each other meet, at one height.
    Ridge,
}

/// Two walls next to each other from one height to another, and the seam between them
/// there (`sm_DraftPiece`).
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// The wall before.
    a: usize,
    /// The wall after.
    b: usize,
    kind: PieceKind,
    /// Its ends (`p`, `f0` and `x`, `f1`), then its curve.
    seam: Seam,
    /// Of no length: left out.
    none: bool,
}

/// Where the piece's seam is at fraction `f` of the height, in the profile's plane;
/// `None` when its walls do not cross there (`sm_DraftPieceAt`).
fn piece_at(walls: &[Wall], piece: &Piece, n: Vec3, h: Vec3, f: f64) -> Result<Option<Point3>> {
    let base = piece.seam.p - piece.seam.f0 * h;
    match piece.kind {
        PieceKind::Crossing => Ok(crossing(
            at_ref(walls, piece.a)?,
            at_ref(walls, piece.b)?,
            f,
            n,
            base,
        )),
        PieceKind::Ridge => Ok(Some(base)),
        PieceKind::Ruling => {
            let wb = at_ref(walls, piece.b)?;
            if !wb.arc {
                return Ok(Some(base + (f - piece.seam.f0) * wb.offset));
            }
            let Ok(r) = (base - wb.center).unitize() else {
                return Ok(None);
            };
            Ok(Some(base - (f - piece.seam.f0) * wb.shrink * r))
        }
    }
}

/// The state of the walls' sections followed up the height.
struct Sections<'a> {
    walls: &'a [Wall],
    pieces: Vec<Piece>,
    /// A live wall's open pieces: the one it starts from and the one it ends at.
    ins: Vec<usize>,
    outs: Vec<usize>,
    n: Vec3,
    h: Vec3,
}

impl Sections<'_> {
    /// Where piece `p` is at `f`, in the profile's plane.
    fn at(&self, p: usize, f: f64) -> Result<Option<Point3>> {
        piece_at(self.walls, at_ref(&self.pieces, p)?, self.n, self.h, f)
    }

    /// Piece `p` ends at `f`, at the point `q` of the profile's plane.
    fn end(&mut self, p: usize, f: f64, q: Point3) -> Result<()> {
        let h = self.h;
        let piece = self.pieces.get_mut(p).ok_or_else(|| {
            SolidError::primitive_failed(OP, "a wall piece index is out of range")
        })?;
        piece.seam.x = q + f * h;
        piece.seam.f1 = f;
        Ok(())
    }

    /// The length of live wall `w`'s section at `f` (an arc's as an angle): its parameter
    /// where it crosses the wall after less where it crosses the wall before. An arc's
    /// start angle `start` is followed from the call before, its length from `hint`.
    /// `None` when a crossing is missing.
    fn length(&self, w: usize, f: f64, start: &mut f64, hint: f64) -> Result<Option<f64>> {
        let wall = at_ref(self.walls, w)?;
        let (Some(xa), Some(xb)) = (
            self.at(at(&self.ins, w)?, f)?,
            self.at(at(&self.outs, w)?, f)?,
        ) else {
            return Ok(None);
        };
        if !wall.arc {
            *start = (xa - wall.start - f * wall.offset).dot(wall.dir);
            return Ok(Some(
                (xb - wall.start - f * wall.offset).dot(wall.dir) - *start,
            ));
        }
        *start = angle(wall, xa, *start);
        Ok(Some(angle(wall, xb, *start + hint) - *start))
    }

    /// A refusal of `reason` at piece `p`'s low end, in the profile's plane.
    fn refuse(&self, reason: TaperCornerRefusal, p: usize) -> Result<Walls> {
        let seam = at_ref(&self.pieces, p)?.seam;
        Ok(refuse(reason, seam.p - seam.f0 * self.h))
    }
}

/// Steps in which each live wall's section is followed up the height.
const SECTION_STEPS: u32 = 64;

/// Halvings to a wall's closing height.
const CLOSING_HALVINGS: u32 = 100;

/// Where a wall's seam pcurves go: the profile's plane normal, the height, and the
/// wall surface's `u` range, `v` origin and `v` scale.
struct PcurveFrame {
    n: Vec3,
    h: Vec3,
    u: [f64; 2],
    v0: f64,
    v_scale: f64,
}

/// A wall face's loop: its edges' curves, pcurves and orientations, and each edge's seam
/// (for a refusal's corner).
#[derive(Default)]
struct EdgeLoop {
    curves: Vec<BSplineCurve>,
    pcurves: Vec<BSplineCurve>,
    orientations: Vec<OrientType>,
    of: Vec<Seam>,
}

impl EdgeLoop {
    fn push(&mut self, curve: BSplineCurve, pcurve: BSplineCurve, orient: OrientType, of: Seam) {
        self.curves.push(curve);
        self.pcurves.push(pcurve);
        self.orientations.push(orient);
        self.of.push(of);
    }

    /// A seam's curve and pcurve on wall `w`; `Some(corner)` when the pcurve cannot be
    /// made (refused at the seam's low end, in the profile's plane).
    fn seam(
        &mut self,
        w: &Wall,
        seam: &Seam,
        frame: &PcurveFrame,
        orient: OrientType,
    ) -> Result<Option<Point3>> {
        let curve = seam_curve(seam)?;
        let pcurve = match seam_pcurve(
            w,
            seam,
            frame.n,
            frame.h,
            frame.u,
            w.lo,
            frame.v0,
            frame.v_scale,
            true,
        ) {
            Ok(pc) => pc,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(Some(seam.p - seam.f0 * frame.h)),
        };
        self.push(curve, pcurve, orient, *seam);
        Ok(None)
    }
}

/// [`curvature_keeping_walls`] (WR-571; the fork's
/// tangency-labs/smlib@183ea65): the seams come from following the section up. Each live
/// wall's length between its crossings with its neighbours is followed to the first
/// height where it is gone (halved to the root); there the wall ends at its apex and its
/// neighbours meet from that point up, a new seam built the same exact way. Two lines
/// brought face to face so (a slot's sides) close together along their common line, and
/// the walls beyond them meet from its other end. Each wall's face runs over its seams in
/// order; a closed wall has no far edge. A wall closing at the far end, the whole section
/// closing, and an arc shrinking to its centre are still refused.
#[allow(clippy::too_many_lines)] // one fork routine
fn walls_closing_at_apexes(
    a: &mut impl PrimitiveCreationAccess,
    kernel_brep: BrepId,
    target: BrepId,
    input: &WallsInput<'_>,
    mut walls: Vec<Wall>,
    n: Vec3,
    h: Vec3,
) -> Result<Walls> {
    let tol = input.tol;
    let count = walls.len();

    // The section at every height is the walls' sections, each from where it crosses the
    // wall before to where it crosses the wall after. Piece i starts at the profile's
    // corner where wall i-1 ends and wall i starts; each event ends pieces and starts one.
    let mut sections = Sections {
        walls: &walls,
        pieces: Vec::new(),
        ins: vec![0; count],
        outs: vec![0; count],
        n,
        h,
    };
    let mut prev = vec![0; count];
    let mut next = vec![0; count];
    let mut live = vec![true; count];
    let mut live_count = count;
    for i in 0..count {
        let before = (i + count - 1) % count;
        let wa = at_ref(&walls, before)?;
        let wb = at_ref(&walls, i)?;
        let ta = tangent_at(wa, wb.start)?;
        let tb = tangent_at(wb, wb.start)?;
        let turn = n.ccw_angle_between(ta, tb)?;
        sections.pieces.push(Piece {
            a: before,
            b: i,
            kind: if turn.abs() > EFF_ZERO_SQRT {
                PieceKind::Crossing
            } else {
                PieceKind::Ruling
            },
            seam: Seam {
                p: wb.start,
                f0: 0.0,
                ..Seam::default()
            },
            none: false,
        });
        *sections
            .ins
            .get_mut(i)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? = i;
        *sections
            .outs
            .get_mut(before)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? = i;
        *prev
            .get_mut(i)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? =
            before;
        *next
            .get_mut(i)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? =
            (i + 1) % count;
    }
    let set = |v: &mut Vec<usize>, i: usize, x: usize| -> Result<()> {
        *v.get_mut(i)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? = x;
        Ok(())
    };
    let mut f_cur = 0.0;
    for _event in 0..=count {
        // the lowest height at which a live wall closes: its length followed up in steps
        // (an arc's angles stay continuous), the first step at which it is gone, then
        // halved to the root
        let mut f_event = 2.0;
        let mut w_event = 0;
        for w in 0..count {
            if !at(&live, w)? {
                continue;
            }
            let wall = at_ref(&walls, w)?;
            let mut start = if wall.arc { wall.start_ang } else { 0.0 };
            let hint = if wall.arc {
                wall.end_ang - wall.start_ang
            } else {
                0.0
            };
            let Some(mut len) = sections.length(w, f_cur, &mut start, hint)? else {
                return sections.refuse(TaperCornerRefusal::Apart, at(&sections.ins, w)?);
            };
            let mut f_lo = f_cur;
            let mut f_hi = 1.0;
            let mut closes = false;
            for k in 1..=SECTION_STEPS {
                if closes {
                    break;
                }
                let f = f_cur + (1.0 - f_cur) * f64::from(k) / f64::from(SECTION_STEPS);
                let mut s = start;
                let Some(l) = sections.length(w, f, &mut s, len)? else {
                    return sections.refuse(TaperCornerRefusal::Apart, at(&sections.ins, w)?);
                };
                if l <= 0.0 {
                    closes = true;
                    f_hi = f;
                } else {
                    f_lo = f;
                    start = s;
                    len = l;
                }
            }
            if !closes {
                // a wall reaching the far end with no length closes there: the far end
                // would pinch
                if len <= if wall.arc { tol / wall.radius } else { tol } {
                    return sections.refuse(TaperCornerRefusal::WallCloses, at(&sections.ins, w)?);
                }
                continue;
            }
            for _ in 0..CLOSING_HALVINGS {
                if !(f_lo < f_hi) {
                    break;
                }
                let f = 0.5 * (f_lo + f_hi);
                let mut s = start;
                let Some(l) = sections.length(w, f, &mut s, len)? else {
                    return sections.refuse(TaperCornerRefusal::Apart, at(&sections.ins, w)?);
                };
                if l > 0.0 {
                    if f <= f_lo {
                        break;
                    }
                    f_lo = f;
                    start = s;
                    len = l;
                } else {
                    if f >= f_hi {
                        break;
                    }
                    f_hi = f;
                }
            }
            let f = 0.5 * (f_lo + f_hi);
            if f < f_event {
                f_event = f;
                w_event = w;
            }
        }
        if f_event > 1.0 {
            break;
        }
        let f = f_event;
        let w = w_event;
        let w_in = at(&sections.ins, w)?;
        if (1.0 - f) * input.height.abs() <= tol || at_ref(&walls, w)?.point {
            return sections.refuse(TaperCornerRefusal::WallCloses, w_in);
        }
        // the wall ends at its apex
        let (Some(xa), Some(xb)) = (
            sections.at(w_in, f)?,
            sections.at(at(&sections.outs, w)?, f)?,
        ) else {
            return sections.refuse(TaperCornerRefusal::Apart, w_in);
        };
        let mut apex = 0.5 * (xa + xb);
        sections.end(w_in, f, apex)?;
        sections.end(at(&sections.outs, w)?, f, apex)?;
        let mut wa = at(&prev, w)?;
        let mut wb = at(&next, w)?;
        *live
            .get_mut(w)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a wall index is out of range"))? =
            false;
        live_count -= 1;
        set(&mut next, wa, wb)?;
        set(&mut prev, wb, wa)?;
        // its neighbours meet from there up (each ridge closes two walls, so the loop ends
        // before every wall has closed)
        let mut met = false;
        for _ in 0..=count {
            if live_count < 3 || wa == wb {
                return sections.refuse(TaperCornerRefusal::WallCloses, w_in);
            }
            let ta = tangent_at(at_ref(&walls, wa)?, apex)?;
            let tb = tangent_at(at_ref(&walls, wb)?, apex)?;
            let lines = !at_ref(&walls, wa)?.arc && !at_ref(&walls, wb)?.arc;
            if lines && ta.dot(tb) < -1.0 + EFF_ZERO_SQRT {
                // face to face: both close along their common line, to where the walls
                // beyond meet them
                let (Some(ea), Some(eb)) = (
                    sections.at(at(&sections.ins, wa)?, f)?,
                    sections.at(at(&sections.outs, wb)?, f)?,
                ) else {
                    return sections.refuse(TaperCornerRefusal::Apart, at(&sections.ins, wa)?);
                };
                if ea.distance(eb) > tol {
                    return sections.refuse(TaperCornerRefusal::WallCloses, w_in);
                }
                let end = 0.5 * (ea + eb);
                sections.pieces.push(Piece {
                    a: wa,
                    b: wb,
                    kind: PieceKind::Ridge,
                    seam: Seam {
                        p: apex + f * h,
                        f0: f,
                        ..Seam::default()
                    },
                    none: false,
                });
                sections.end(sections.pieces.len() - 1, f, end)?;
                sections.end(at(&sections.ins, wa)?, f, end)?;
                sections.end(at(&sections.outs, wb)?, f, end)?;
                let c = at(&prev, wa)?;
                let d = at(&next, wb)?;
                *live.get_mut(wa).ok_or_else(|| {
                    SolidError::primitive_failed(OP, "a wall index is out of range")
                })? = false;
                *live.get_mut(wb).ok_or_else(|| {
                    SolidError::primitive_failed(OP, "a wall index is out of range")
                })? = false;
                live_count -= 2;
                if live_count < 3 || c == wb || d == wa {
                    return sections.refuse(TaperCornerRefusal::WallCloses, w_in);
                }
                set(&mut next, c, d)?;
                set(&mut prev, d, c)?;
                wa = c;
                wb = d;
                apex = end;
                continue;
            }
            let turn = n.ccw_angle_between(ta, tb)?;
            sections.pieces.push(Piece {
                a: wa,
                b: wb,
                kind: if turn.abs() > EFF_ZERO_SQRT {
                    PieceKind::Crossing
                } else {
                    PieceKind::Ruling
                },
                seam: Seam {
                    p: apex + f * h,
                    f0: f,
                    ..Seam::default()
                },
                none: false,
            });
            let p = sections.pieces.len() - 1;
            set(&mut sections.outs, wa, p)?;
            set(&mut sections.ins, wb, p)?;
            met = true;
            break;
        }
        if !met {
            return Err(SolidError::invalid_input(
                OP,
                "draft wall closing bound exceeded",
            ));
        }
        f_cur = f;
    }
    // the pieces still open reach the far end
    for w in 0..count {
        if !at(&live, w)? {
            continue;
        }
        let p = at(&sections.outs, w)?;
        let Some(x) = sections.at(p, 1.0)? else {
            return sections.refuse(TaperCornerRefusal::Apart, p);
        };
        sections.end(p, 1.0, x)?;
    }
    // the seams' curves; a piece of no length is left out
    let mut pieces = sections.pieces;
    for piece in &mut pieces {
        let seam = &mut piece.seam;
        if seam.p.distance(seam.x) <= tol {
            piece.none = true;
            continue;
        }
        if piece.kind == PieceKind::Crossing {
            let wa = at_ref(&walls, piece.a)?;
            let wb = at_ref(&walls, piece.b)?;
            if let Err(refusal) = sharp_seam(wa, wb, n, h, tol, seam)? {
                return Ok(Err(refusal));
            }
        } else {
            // a ruling, or the line two walls close along
            seam.s = 0.5 * (seam.p + seam.x);
            seam.weight = 1.0;
        }
    }
    // each wall's seams from its start corner up and from its end corner up, in order
    let mut starts: Vec<Vec<Seam>> = vec![Vec::new(); count];
    let mut ends: Vec<Vec<Seam>> = vec![Vec::new(); count];
    for piece in &pieces {
        if piece.none {
            continue;
        }
        ends.get_mut(piece.a)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a seam piece's wall is out of range"))?
            .push(piece.seam);
        starts
            .get_mut(piece.b)
            .ok_or_else(|| SolidError::primitive_failed(OP, "a seam piece's wall is out of range"))?
            .push(piece.seam);
    }
    for (i, (s, e)) in starts.iter().zip(&ends).enumerate() {
        if s.is_empty() || e.is_empty() {
            return Ok(refuse(
                TaperCornerRefusal::WallCloses,
                at_ref(&walls, i)?.start,
            ));
        }
    }

    // each wall's extent: from its start seams' reach to its end seams', in spans that
    // each hold a seam
    let h2 = h.dot(n);
    for (i, w) in walls.iter_mut().enumerate() {
        let chains = [at_ref(&starts, i)?, at_ref(&ends, i)?];
        let ref0 = if w.arc { w.start_ang } else { 0.0 };
        let ref1 = if w.arc {
            w.end_ang
        } else {
            (w.end - w.start).dot(w.dir)
        };
        let mut reach = [(0.0, 0.0); 2];
        for (slot, (chain, rf)) in reach.iter_mut().zip(chains.into_iter().zip([ref0, ref1])) {
            let mut lo = rf;
            let mut hi = rf;
            for seam in chain {
                for k in 0..=8 {
                    let q = seam_point(seam, f64::from(k) / 8.0);
                    let f = seam.f0 + (q - seam.p).dot(n) / h2;
                    let qp = q - f * h;
                    let t = if w.arc {
                        angle(w, qp, rf)
                    } else {
                        (qp - w.start - f * w.offset).dot(w.dir)
                    };
                    lo = min2(lo, t);
                    hi = max2(hi, t);
                }
            }
            let pad = 0.05 * (hi - lo) + if w.arc { 1e-3 } else { 1e-3 * input.offset };
            *slot = (lo - pad, hi + pad);
        }
        let [(lo0, hi0), (lo1, hi1)] = reach;
        let first_start = at(at_ref(&starts, i)?, 0)?.p;
        let first_end = at(at_ref(&ends, i)?, 0)?.p;
        let chains = at_ref(&starts, i)?
            .iter()
            .map(|seam| (seam, ref0))
            .chain(at_ref(&ends, i)?.iter().map(|seam| (seam, ref1)));
        (w.lo, w.hi) =
            cone_face_angles(a.kernel(kernel_brep)?, w, chains, n, h).unwrap_or((lo0, hi1));
        w.breaks.clear();
        // an arc's wall on its cone: the cone's own equal spans (its seams' pcurves are cut
        // where they cross from one to the next) (the fork's tangency-labs/smlib@3ae7bfa)
        let wall_cone = if w.arc {
            draft_cone(a.kernel(kernel_brep)?, w, h)?
        } else {
            None
        };
        if let Some(cone) = wall_cone {
            let (v_knots, _) = Surface::from(cone).knots_with_nurb(
                a.kernel(kernel_brep)?,
                SurfParamType::V,
                None,
            )?;
            if v_knots.len() < 2 {
                return Err(SolidError::primitive_failed(
                    OP,
                    "the wall surface has fewer than two V knots",
                ));
            }
            let spans = v_knots.len() - 1;
            for k in 0..=spans {
                w.breaks
                    .push(w.lo + (w.hi - w.lo) * k as f64 / spans as f64);
            }
        } else if w.arc {
            let quarter = 0.5 * PI;
            w.breaks.push(lo0);
            if hi0 >= lo1 {
                // one span holds both ends' seams
                if hi1 - lo0 >= PI - EFF_ZERO_SQRT {
                    return Ok(refuse(TaperCornerRefusal::WallCloses, first_start));
                }
            } else {
                w.breaks.push(hi0);
                // (at most the arc's angle over a quarter turn: lo1 and hi0 lie within
                // a turn and a half of the arc's start angle)
                let spans = ((lo1 - hi0) / quarter).ceil() as u64;
                for k in 1..spans {
                    w.breaks.push(hi0 + (lo1 - hi0) * k as f64 / spans as f64);
                }
                w.breaks.push(lo1);
                if hi0 - lo0 >= PI - EFF_ZERO_SQRT || hi1 - lo1 >= PI - EFF_ZERO_SQRT {
                    let corner = if hi0 - lo0 >= hi1 - lo1 {
                        first_start
                    } else {
                        first_end
                    };
                    return Ok(refuse(TaperCornerRefusal::NotExact, corner));
                }
            }
            w.breaks.push(hi1);
        }
    }

    // the faces
    let mut far_curves = Vec::new();
    for i in 0..count {
        let w = at_ref(&walls, i)?;
        let w_starts = at_ref(&starts, i)?;
        let w_ends = at_ref(&ends, i)?;
        let face = {
            let k = a.kernel(kernel_brep)?;
            let low = wall_curve(w, 0.0, h)?;
            let high = wall_curve(w, 1.0, h)?;
            // an arc's wall is its cone (as an undrafted arc's is its cylinder), any other
            // the surface ruled between its ends (the fork's tangency-labs/smlib@3ae7bfa)
            let cone = if w.arc { draft_cone(k, w, h)? } else { None };
            let (surface, domain) = if let Some(cone) = cone {
                let surface = Surface::from(cone);
                let domain = surface.natural_uv_domain_with_nurb(k)?;
                (surface, domain)
            } else {
                let surface = BSplineSurface::create_ruled_surface(&low, &high, SurfParamType::U)?;
                let domain = surface.natural_uv_domain();
                (Surface::from(surface), domain)
            };
            let u0 = domain.u_min();
            let u1 = domain.u_max();
            let v_scale = (domain.v_max() - domain.v_min()) / (w.hi - w.lo);
            let v0 = domain.v_min();
            let mid = if w.arc {
                0.5 * (w.start_ang + w.end_ang)
            } else {
                0.0
            };
            let near0 = if w.arc { w.start_ang } else { mid };
            let near1 = if w.arc { w.end_ang } else { mid };
            // the loop: the profile's curve (none for a tangent plane's piece), the end
            // seams up, the far end's curve back (none for a wall that closes), the start
            // seams down; each edge's seam, for a refusal's corner
            let frame = PcurveFrame {
                n,
                h,
                u: [u0, u1],
                v0,
                v_scale,
            };
            let mut edges = EdgeLoop::default();
            if !w.point {
                let s0 = *w_starts
                    .first()
                    .ok_or_else(|| SolidError::primitive_failed(OP, "a wall has no seam ends"))?;
                let e0 = *w_ends
                    .first()
                    .ok_or_else(|| SolidError::primitive_failed(OP, "a wall has no seam ends"))?;
                let tp0 = param(w, s0.p, 0.0, near0)?;
                let tp1 = param(w, e0.p, 0.0, near1)?;
                let mut bottom = low.copy();
                bottom.trim(k, &mut Extent1d::from_min_max(tp0, tp1), TrimMode::Notify)?;
                let now = bottom.natural_interval();
                let pc_bottom = iso_pcurve(
                    u0,
                    v0 + (now.min() - w.lo) * v_scale,
                    u0,
                    v0 + (now.max() - w.lo) * v_scale,
                    now.min(),
                    now.max(),
                )?;
                edges.push(bottom, pc_bottom, OrientType::Same, s0);
            }
            for seam in w_ends {
                if let Some(corner) = edges.seam(w, seam, &frame, OrientType::Same)? {
                    return Ok(refuse(TaperCornerRefusal::NotExact, corner));
                }
            }
            let mut top_curve = None;
            if at(&live, i)? {
                let last_start = *w_starts
                    .last()
                    .ok_or_else(|| SolidError::primitive_failed(OP, "a wall has no seam ends"))?;
                let last_end = *w_ends
                    .last()
                    .ok_or_else(|| SolidError::primitive_failed(OP, "a wall has no seam ends"))?;
                let tx0 = param(w, last_start.x - h, 1.0, near0)?;
                let tx1 = param(w, last_end.x - h, 1.0, near1)?;
                let mut top = high.copy();
                top.trim(k, &mut Extent1d::from_min_max(tx0, tx1), TrimMode::Notify)?;
                let top_now = top.natural_interval();
                let pc_top = iso_pcurve(
                    u1,
                    v0 + (top_now.min() - w.lo) * v_scale,
                    u1,
                    v0 + (top_now.max() - w.lo) * v_scale,
                    top_now.min(),
                    top_now.max(),
                )?;
                top_curve = Some(top.copy());
                edges.push(top, pc_top, OrientType::Opposite, last_end);
            }
            for seam in w_starts.iter().rev() {
                if let Some(corner) = edges.seam(w, seam, &frame, OrientType::Opposite)? {
                    return Ok(refuse(TaperCornerRefusal::NotExact, corner));
                }
            }
            let EdgeLoop {
                curves,
                pcurves,
                orientations,
                of,
            } = edges;
            // every pcurve must lift onto its curve
            for ((c, pc), seam) in curves.iter().zip(&pcurves).zip(&of) {
                let ivl = c.natural_interval();
                for step in 0..=8 {
                    let t = ivl.evaluate(f64::from(step) / 8.0);
                    let uv = pc.evaluate_point(t)?;
                    let on = surface.evaluate_point(k, Point2::new(uv.x, uv.y))?;
                    let at_t = c.evaluate_point(t)?;
                    if on.distance(at_t) > tol {
                        return Ok(refuse(TaperCornerRefusal::NotExact, seam.p - seam.f0 * h));
                    }
                }
            }
            // the far end's curve, for its cap
            if let Some(top) = top_curve {
                far_curves.push(Curve::from(top));
            }
            let face_curves = FaceCurvesInput {
                loops: vec![curves.len()],
                curves_3d: Some(curves.into_iter().map(|c| Some(Curve::from(c))).collect()),
                uv_curves: Some(pcurves.into_iter().map(Curve::from).collect()),
                orientations,
                loop_points: Vec::new(),
            };
            (face_curves, surface, domain)
        };
        // the loop runs clockwise in (u, v): opposite to the surface's normal
        let (curves, surface, domain) = face;
        a.face_with_curves(target, curves, surface, domain, OrientType::Opposite)?;
    }
    Ok(Ok(far_curves))
}

/// The side faces of a draft extrusion of the closed profile `input.composite`, into
/// the B-rep `target`, and the far end's curves in order
/// (`sm_CreateCurvatureKeepingWalls`). Its walls keep their curvature at its corners;
/// with `input.tangent`, where a corner opens a gap, each continues as the plane
/// tangent to it there instead. A corner that cannot be closed is `Ok(Err(refusal))`
/// (upstream's `SM_ERR_INVALID_INPUT` with why and where). `kernel_brep` names the
/// B-rep whose kernel evaluates the curves.
pub(super) fn curvature_keeping_walls(
    a: &mut impl PrimitiveCreationAccess,
    kernel_brep: BrepId,
    target: BrepId,
    input: &WallsInput<'_>,
) -> Result<Walls> {
    let n = input.normal;
    let tol = input.tol;
    if input.composite.segments().is_empty() {
        return Err(SolidError::invalid_input(OP, "the profile has no curves"));
    }
    let h = input.height * n;
    let side = if input.side == OffsetDirection::LeftHandSide {
        1.0
    } else {
        -1.0
    };

    // the walls, and the plane they lie in (`n` from here on; the height `h` runs along
    // the draft's normal, `d`)
    let mut walls = match read_walls(a.kernel(kernel_brep)?, input)? {
        Ok(walls) => walls,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let d = n;
    let n = profile_plane(&walls, d, tol)?;
    if input.tangent {
        walls = with_tangent_planes(&walls, n, d, input.offset, side)?;
    }
    lean_offsets(&mut walls, n, d);
    walls_closing_at_apexes(a, kernel_brep, target, input, walls, n, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quartic_multiplies_bernstein_quadratics() {
        // (1 - t)^2 * t^2 = t^2 - 2t^3 + t^4: Bernstein coefficients [0, 0, 1/6, 0, 0]
        let c = quartic([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(c, [0.0, 0.0, 1.0 / 6.0, 0.0, 0.0]);
        // 1 * 1 = 1
        assert_eq!(quartic([1.0; 3], [1.0; 3]), [1.0; 5]);
    }

    #[test]
    fn seam_point_interpolates_the_conic() {
        let seam = Seam {
            p: Point3::new(0.0, 0.0, 0.0),
            s: Point3::new(1.0, 1.0, 0.0),
            x: Point3::new(2.0, 0.0, 0.0),
            weight: 1.0,
            ..Seam::default()
        };
        assert_eq!(seam_point(&seam, 0.0), seam.p);
        assert_eq!(seam_point(&seam, 1.0), seam.x);
        assert_eq!(seam_point(&seam, 0.5), Point3::new(1.0, 0.5, 0.0));
    }

    #[test]
    fn two_lines_cross_at_their_offset_corner() {
        let n = Vec3::Z;
        let a = Wall {
            start: Point3::new(0.0, 0.0, 0.0),
            dir: Vec3::X,
            offset: Vec3::new(0.0, 1.0, 0.0),
            ..Wall::default()
        };
        let b = Wall {
            start: Point3::new(10.0, 0.0, 0.0),
            dir: Vec3::Y,
            offset: Vec3::new(-1.0, 0.0, 0.0),
            ..Wall::default()
        };
        let x = crossing(&a, &b, 1.0, n, b.start);
        assert_eq!(x, Some(Point3::new(9.0, 1.0, 0.0)));
        // parallel lines do not cross
        assert_eq!(crossing(&a, &a, 1.0, n, b.start), None);
    }

    /// Two circles touching inside each other, as the arcs of a crescent at its tip: these
    /// centres and radii, as doubles, are exactly 63.6 = 591.6 - 528 apart, touching at
    /// (113.92, 0); `h^2` comes out -2.3e-10. Moved apart by a micrometre, they do not meet.
    #[test]
    fn two_circles_touching_within_rounding_touch() {
        let circle = |x: f64, r: f64| Wall {
            arc: true,
            center: Point3::new(x, 0.0, 0.0),
            axis: Vec3::Z,
            radius: r,
            ..Wall::default()
        };
        let a = circle(-477.68, 591.6);
        let b = circle(-414.08, 528.0);
        let tip = Point3::new(113.92, 0.0, 0.0);
        let x = crossing(&a, &b, 0.0, Vec3::Z, tip);
        assert!(x.is_some_and(|x| x.distance(tip) < 1e-9), "{x:?}");
        let c = circle(-414.081, 528.0);
        assert_eq!(crossing(&a, &c, 0.0, Vec3::Z, tip), None);
    }

    /// Two large circles touching inside each other, radii 1e8 and 1e8 - 1 with centres 1
    /// apart: `ra^2 - rb^2` rounds by 1, which put the touching point half a unit off both.
    #[test]
    fn two_large_circles_touching_inside_touch_where_they_do() {
        let circle = |x: f64, r: f64| Wall {
            arc: true,
            center: Point3::new(x, 0.0, 0.0),
            axis: Vec3::Z,
            radius: r,
            ..Wall::default()
        };
        let tip = Point3::new(1e8, 0.0, 0.0);
        let x = crossing(
            &circle(0.0, 1e8),
            &circle(1.0, 99_999_999.0),
            0.0,
            Vec3::Z,
            tip,
        );
        assert!(x.is_some_and(|x| x.distance(tip) < 1e-6), "{x:?}");
    }

    /// A line starting 1e8 from a circle of radius 1: `(d.w)^2` and `w.w` are 1e16 and round
    /// by units, so the discriminant's bound did not prove that the line y = 2 misses the
    /// circle (it touched at (0, 2), a radius off); its foot from the centre does.
    #[test]
    fn a_line_far_from_its_start_misses_a_circle_it_misses() {
        let circle = Wall {
            arc: true,
            axis: Vec3::Z,
            radius: 1.0,
            ..Wall::default()
        };
        let line = |y: f64| Wall {
            start: Point3::new(1e8, y, 0.0),
            dir: Vec3::X,
            ..Wall::default()
        };
        assert_eq!(
            crossing(&line(2.0), &circle, 0.0, Vec3::Z, Point3::ZERO),
            None
        );
    }

    /// A line touching a circle, as at the issue's lower horn's tip (the line from (406, s.y)
    /// to the circle's point t = (c + r, 0), leaning 1.6e-8 off x = 406): the discriminant
    /// `(d.w)^2 - (w.w - r^2)` came out -2.2e-11, the foot's `|q| - r` 0, and they meet at the
    /// line's foot from the centre, 5.3e-8 from t (the line
    /// crosses the circle at t and 1.1e-7 above it). Moved a micrometre off the circle, they do
    /// not meet.
    #[test]
    fn a_line_touching_a_circle_within_rounding_touches_it() {
        let c = Point3::new(-340.614_824_473_321_73, 0.0, 0.0);
        let r = 746.614_824_457_444_7;
        let t = Point3::new(405.999_999_984_122_95, 0.0, 0.0);
        let circle = Wall {
            arc: true,
            center: c,
            axis: Vec3::Z,
            radius: r,
            ..Wall::default()
        };
        let line = |x: f64| {
            let start = Point3::new(x, -223.013_212_796_215_44, 0.0);
            Wall {
                start,
                dir: (Point3::new(x - 406.0 + t.x, 0.0, 0.0) - start)
                    .unitize()
                    .unwrap_or(Vec3::Y),
                ..Wall::default()
            }
        };
        let x = crossing(&line(406.0), &circle, 0.0, Vec3::Z, t);
        assert!(x.is_some_and(|x| x.distance(t) < 1e-7), "{x:?}");
        assert_eq!(crossing(&line(406.001), &circle, 0.0, Vec3::Z, t), None);
    }

    #[test]
    fn has_corner_empty_composite_does_not_panic() {
        let k = Kernel::default();
        let empty = CompositeCurve::new(3);
        assert!(!has_corner(&k, &empty, Vec3::Z));
    }
}
