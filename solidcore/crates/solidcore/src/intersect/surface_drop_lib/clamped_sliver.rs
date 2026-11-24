//! Spans of a dropped curve that clamping into the domain collapses to a point (not
//! upstream).
//!
//! Upstream: none. The drop's span loop (`SmSurface::DropCurve`,
//! `source/SMLib/src/SmSurfaceDropLib.cpp`) adds every span it accepts with
//! `sm_AddToCurve`, which this guards.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`is_clamped_sliver`]      | none     |
//! | [`Span`], [`Span::stretch`] | none    |
//! | [`gap_within`]             | none     |
//! | `polygon_in_domain` (private) | none  |
//! | `fix_up_moves` (private)   | none     |
//! | `straightened` (private)   | none     |
//!
//! **The defect.** `sm_AddToCurve` makes a span's Bezier polygon from its Hermite data:
//! `P1 = uv(t0)`, `P2 = P1 + h uv'(t0) / 3`, `P3 = P4 - h uv'(t1) / 3`, `P4 = uv(t1)`, and
//! then clamps `P1` and `P4` into the domain only. Where a curve end lies a little outside
//! the domain (by less than the drop's tolerance), the span that covers that sliver gets
//! both ends clamped onto the boundary while `P2` and `P3` keep the unclamped tangent:
//! the span runs out of the domain and back, an S whose UV image doubles back on itself.
//! The fix-up of the flush (`sm_FixBoundaryCurves`) moves the polygon onto the boundary
//! only where the curve runs along it, so the S stays in the trim. The drop's own span
//! test does not see it: it clamps its samples into the domain before measuring them.
//!
//! **The rule here.** A span is a *clamped sliver* when
//! 1. neither end is on a pole (the pole's dog-leg straightening moves those polygons),
//! 2. a control point of its polygon lies outside the domain (the span leaves it: a span
//!    clamping never moves is a genuine piece of the trim, however short),
//! 3. the box of its four control points, each clamped into the domain, is no wider than
//!    the drop's UV tolerance in either coordinate: clamping is monotone in each
//!    coordinate and the span lies in the convex hull of its polygon, so the span clamped
//!    into the domain lies in that box (exact: the box is of the polygon `sm_AddToCurve`
//!    would emit, made by the same operations), and
//! 4. the curve piece it replaces lies in a box ([`Curve::certified_bounding_box`]) whose
//!    diagonal, rounded up, is within the drop's 3D tolerance.
//!
//! Such a span carries no trim information inside the domain: its clamped image is one
//! UV point to within the UV tolerance. The drop leaves it out where it is the first or
//! the last span of a curve, and the neighbouring span takes over its parameter range
//! ([`Span::stretch`]): the same polygon, so the span joins its neighbour at the clamped end
//! point it already had, over the neighbour's range and the sliver's. A sliver between
//! two spans is added as before, and so is a sliver whose stretched neighbour's polygon
//! leaves the domain: that neighbour has the same defect, and stretched to the curve's end
//! it would lose the flush's fix-up of the knot it shared with the sliver (the neighbours
//! of an interior knot on the boundary move onto it; those of an end knot are only
//! clamped), which can make its fold larger than the sliver's. Only a neighbour whose
//! polygon neither the clamp nor the fix-up moves takes over: its polygon lies in the
//! domain, the fix-up's tests (made the same way) leave the point next to the knot it
//! keeps where it is (`fix_up_moves`), and it is not a dog-leg polygon with an end on a
//! pole, which the straightening moves (`straightened`). Any span after a first one was
//! left out is added as before: at most one first span is left out, and a last span is left out only after another was
//! added. A sliver is left out only where the trace has no bounded boundary fix-up (whose
//! budget each span added spends).
//!
//! **The stretch enclosure.** Stretching keeps the neighbour's polygon bit for bit (its
//! tangents are scaled by the neighbour's own length, [`Span::tangent_scale`]) over the
//! longer knot range `from..to`, which reparametrises it: the point it shows at each
//! curve parameter moves. The stretched span is accepted only where its image on the
//! surface is proved within the drop's 3D tolerance of the curve at every parameter,
//! with no sampling ([`gap_within`]). With `q` the Bezier polygon's curve in UV and
//! `L = to - from`, the gap `D(tau) = S(q(tau)) - C(from + tau L)` over a piece `I` of
//! the Bezier parameter lies in the centred (mean value) form
//! `D(m) + D'(I) (I - m)`, `m` the piece's midpoint, in rounded interval arithmetic:
//! `q` and `q'` by their Bernstein forms (cut to the domain, which holds the polygon and
//! so the curve), the surface's point and first partials over a UV box from its affine
//! map (a plane) or its rational net ([`partials_over`]: a B-spline's net, the NURB a
//! cone, sphere, torus, surface of revolution or extrusion is evaluated on), and the
//! curve's point and derivative from its rational net ([`curve_derivs_over`]) or, for a
//! line, its affine map. A net with an interior knot of multiplicity above its degree
//! (where it may jump) is not enclosed, so `D` is continuous and piecewise smooth across
//! the knots of either; the derivative enclosures hold both one-sided derivatives at a
//! knot, so the form holds on every piece. The farthest point of the enclosure, rounded up,
//! bounds the gap over the piece; a piece whose bound exceeds the tolerance is bisected
//! (an explicit stack, at most [`MAX_ENCLOSURE_PIECES`] pieces). A surface or curve
//! kind with no enclosure (a procedural surface, any other curve), a box that is not
//! proved, or a proof that needs more pieces keeps the sliver: a first span is added
//! back ahead of its neighbour, and a last span is added as before, so the curve is the
//! one the trace builds without this rule. The drop's reported deviation stays the
//! trace's measure of the spans it tested. A first span left out is likewise added back
//! where no span follows it, and before the trace stops at a failing span or end point,
//! so the end shortcut and the failure record see the curve the trace built. The end
//! shortcut (a trace that walks off the domain just short of the curve's end moves the
//! last knot to the end) moves a stretched last span's end only where the stretch is
//! proved over that longer range too; else the span keeps the range it was proved on.
//! The quantity proved is the gap between the stretched span's image and the curve at
//! the same parameter (`<=` the tolerance); the drop's reported deviation stays the
//! trace's measure against the drop points.
//!
//! Deviations: not upstream (above). The centred enclosure bisects with an explicit
//! stack (no recursion) and gives up past [`MAX_ENCLOSURE_PIECES`] pieces, keeping the
//! sliver.

use core::cmp::Ordering;

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::{native_failure, Result};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::is_dog_leg;
use crate::geom::tol::is_zero;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::mass::enclosure::partials_over;
use crate::surface::surface::p2::{Surface, SurfaceKind};

use super::p1::DropPt;

/// Whether the span from `start` to `end` is a clamped sliver (module docs): its polygon
/// leaves `uv_domain`, clamped into it spans at most `uv_tol_x` by `uv_tol_y`, and the curve piece
/// over it is proved within `tol_3d` across. `false` for a span of no length, a span with
/// an end on a pole, and a curve with no certified box (including one whose box fails
/// with an error that may stand for an ordinary failure, [`native_failure`]: the span
/// is then added as before).
///
/// # Errors
///
/// Those of the curve's certified box that [`native_failure`] passes on (unported,
/// cancelled and port-invariant errors).
#[allow(clippy::too_many_arguments)] // the span, the domain and the drop's tolerances
pub(crate) fn is_clamped_sliver(
    k: &Kernel,
    curve: &Curve,
    uv_domain: &Extent2d,
    start: &DropPt,
    end: &DropPt,
    uv_tol_x: f64,
    uv_tol_y: f64,
    tol_3d: f64,
) -> Result<bool> {
    if start.on_pole != Some(false) || end.on_pole != Some(false) {
        return Ok(false);
    }
    let Some(polygon) = polygon(start, end, end.t - start.t) else {
        return Ok(false);
    };
    if !polygon.iter().any(|&p| uv_domain.clamp_point(p) != p) {
        return Ok(false);
    }
    if !clamped_box_within(uv_domain, polygon, uv_tol_x, uv_tol_y) {
        return Ok(false);
    }
    let piece =
        native_failure(curve.certified_bounding_box(k, &Extent1d::from_min_max(start.t, end.t)))?
            .flatten();
    let Some(piece) = piece else {
        return Ok(false);
    };
    // the box's diagonal in rounded interval arithmetic: its upper end bounds the length
    let (lo, hi) = (piece.min(), piece.max());
    let side = |a: f64, b: f64| {
        let d = Ivl::from(b) - Ivl::from(a);
        d * d
    };
    let diagonal = (side(lo.x, hi.x) + side(lo.y, hi.y) + side(lo.z, hi.z)).sqrt();
    Ok(diagonal.is_finite() && diagonal.mag() <= tol_3d)
}

/// The Bezier polygon `sm_AddToCurve` makes of the span from `start` to `end`, before its
/// clamp, made the same way; `None` for a span of no length.
fn polygon(start: &DropPt, end: &DropPt, scale: f64) -> Option<[Point2; 4]> {
    if scale.is_nan() || scale <= 0.0 || end.t.partial_cmp(&start.t) != Some(Ordering::Greater) {
        return None;
    }
    let p1 = start.uv_pd[0];
    let d1 = start.uv_pd[1] * scale;
    let p4 = end.uv_pd[0];
    let d2 = end.uv_pd[1] * scale;
    let p2 = p1 + d1 / 3.0;
    let p3 = p4 - d2 / 3.0;
    Some([p1, p2, p3, p4])
}

/// The polygon `sm_AddToCurve` makes of the span from `start` to `end` with tangent
/// scale `scale`, where it lies in `uv_domain`, so that its clamp does not move it.
fn polygon_in_domain(
    uv_domain: &Extent2d,
    start: &DropPt,
    end: &DropPt,
    scale: f64,
) -> Option<[Point2; 4]> {
    polygon(start, end, scale).filter(|q| q.iter().all(|&p| uv_domain.clamp_point(p) == p))
}

/// Whether `add_span_with_policy` straightens `polygon`, the span from `start` to `end`
/// (its dog-leg test, made the same way): an end on a pole, or not known to be off one,
/// with a dog-leg polygon. The straightening moves the polygon, so a stretch proved on
/// the polygon as made would not be proved on the one added.
fn straightened(start: &DropPt, end: &DropPt, polygon: [Point2; 4]) -> bool {
    let [p1, p2, p3, p4] = polygon;
    (start.on_pole != Some(false) || end.on_pole != Some(false))
        && is_dog_leg(p1, p2, p3, p4, 35.0, 45.0)
}

/// Whether the flush's fix-up (`fix_boundary_curves`) moves `ours`, the polygon point
/// next to the interior knot point `knot` on the stretched span's side, made with the
/// fix-up's own tests the same way: the knot lies within the `uv_tol_x` by `uv_tol_y`
/// band of a boundary and is on it or the curve runs along it there, which needs
/// `ours` to lie no farther across the boundary than along it; the point then takes the
/// knot's coordinate, a move unless it has it already. The fix-up's other moves leave
/// a polygon in the domain alone (the knots are not snapped, the end neighbours not
/// clamped), and a move of the neighbour on the other side is the same with the
/// stretch or without it.
fn fix_up_moves(
    domain: &Extent2d,
    knot: Point2,
    ours: Point2,
    uv_tol_x: f64,
    uv_tol_y: f64,
) -> bool {
    let (u0, v0) = (domain.min().x, domain.min().y);
    let (u1, v1) = (domain.max().x, domain.max().y);
    let along_u = (ours.x - knot.x).abs() <= (ours.y - knot.y).abs();
    let along_v = (ours.y - knot.y).abs() <= (ours.x - knot.x).abs();
    let moves_u = (knot.x < u0 + uv_tol_x || knot.x > u1 - uv_tol_x)
        && ((knot.x == u0 || knot.x == u1) || along_u)
        && ours.x != knot.x;
    let moves_v = (knot.y < v0 + uv_tol_y || knot.y > v1 - uv_tol_y)
        && ((knot.y == v0 || knot.y == v1) || along_v)
        && ours.y != knot.y;
    moves_u || moves_v
}

/// Whether the points clamped into `domain` lie in a box at most `tol_x` by `tol_y`.
fn clamped_box_within(domain: &Extent2d, points: [Point2; 4], tol_x: f64, tol_y: f64) -> bool {
    let clamped = points.map(|p| domain.clamp_point(p));
    let extent = |f: fn(&Point2) -> f64| {
        let (lo, hi) = clamped
            .iter()
            .map(f)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                (lo.min(x), hi.max(x))
            });
        hi - lo
    };
    let width = extent(|p| p.x);
    let height = extent(|p| p.y);
    // a NaN coordinate fails both comparisons
    width <= tol_x && height <= tol_y
}

/// A span the drop added, kept so that a clamped sliver next to it can be stretched over
/// (module docs): its ends and the tangent scale of the polygon
/// [`add_span_with_policy`](super::p2::add_span_with_policy) makes of it (its length,
/// or the length of the span it was stretched from, so a stretched span's polygon is
/// that span's, made by the same operations from the same values, bit for bit).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span {
    /// The span's start.
    pub start: DropPt,
    /// The span's end.
    pub end: DropPt,
    /// The polygon's tangent scale.
    pub tangent_scale: f64,
    /// Whether the span was stretched ([`Span::stretch`]): a stretched span's range
    /// moves only where the stretch is proved over the new range.
    pub stretched: bool,
}

/// The most pieces [`gap_within`] encloses before it gives up (module docs, the stretch
/// enclosure): `2^13`, so a stretch costs at most that many centred enclosures (each
/// two of the surface's partials and two of the curve's). The face-7 stretch of the
/// regression test needs 3359 at its tolerance; past the cap the sliver is kept.
pub(crate) const MAX_ENCLOSURE_PIECES: usize = 1 << 13;

impl Span {
    /// The span from `start` to `end`.
    pub(crate) fn new(start: DropPt, end: DropPt) -> Self {
        Self {
            start,
            end,
            tangent_scale: end.t - start.t,
            stretched: false,
        }
    }

    /// This span stretched to run over `from..to` (module docs, the stretch enclosure),
    /// where the range contains its own with one end in common, its polygon lies in
    /// `uv_domain`, the drop's flush leaves that polygon where it is (it is not one
    /// `add_span_with_policy` straightens, [`straightened`], and the fix-up with the
    /// `uv_tol` band does not move the point next to the end in common,
    /// [`fix_up_moves`]), and the image on `surface` of the stretched span is proved
    /// within `tol_3d` of `curve` at every parameter ([`gap_within`]). `None` otherwise,
    /// including where a certified box fails: the proof is then not made, and the sliver
    /// is kept, so the drop builds the curve it builds without this rule.
    #[allow(clippy::too_many_arguments)] // the span, the geometry, the range and the tolerance
    pub(crate) fn stretch(
        &self,
        k: &Kernel,
        surface: &Surface,
        curve: &Curve,
        uv_domain: &Extent2d,
        from: f64,
        to: f64,
        uv_tol: (f64, f64),
        tol_3d: f64,
    ) -> Option<Self> {
        let one_end_common = (from == self.start.t) != (to == self.end.t);
        let contains_own = from <= self.start.t && self.end.t <= to;
        if !one_end_common || !contains_own {
            return None;
        }
        let polygon = polygon_in_domain(uv_domain, &self.start, &self.end, self.tangent_scale)?;
        if straightened(&self.start, &self.end, polygon) {
            return None;
        }
        let [p1, p2, p3, p4] = polygon;
        let (knot, ours) = if from == self.start.t {
            (p1, p2)
        } else {
            (p4, p3)
        };
        if fix_up_moves(uv_domain, knot, ours, uv_tol.0, uv_tol.1) {
            return None;
        }
        if !gap_within(k, surface, curve, uv_domain, polygon, from, to, tol_3d) {
            return None;
        }
        let mut start = self.start;
        let mut end = self.end;
        start.t = from;
        end.t = to;
        Some(Self {
            start,
            end,
            tangent_scale: self.tangent_scale,
            stretched: true,
        })
    }
}

/// The surface as [`gap_within`] encloses it: a plane's affine map, or the net it is
/// evaluated on.
enum SurfaceNet {
    /// `origin + u su X + v sv Y`.
    Plane {
        /// The origin.
        o: Vec3,
        /// The x axis times the u scale.
        x: Vec3,
        /// The y axis times the v scale.
        y: Vec3,
        /// The u and v scales.
        scale: (f64, f64),
    },
    /// A rational net.
    Net(Nurbs),
}

impl SurfaceNet {
    /// The enclosure of `surface`: a plane's map; the net of a B-spline, and the NURB of a
    /// cone, sphere, torus, surface of revolution or extrusion (the net the surface is
    /// evaluated on, as [`Surface::certified_bounding_box`] takes it); `None` for any
    /// other kind or a surface with a procedural definition (evaluated exactly, not on its
    /// net).
    fn of(k: &Kernel, surface: &Surface) -> Option<Self> {
        if surface.procedural_definition().is_some() {
            return None;
        }
        match surface.kind() {
            SurfaceKind::Plane(plane) => {
                let (su, sv) = (plane.uv_scale().x, plane.uv_scale().y);
                Some(Self::Plane {
                    o: plane.position().origin(),
                    x: plane.position().x_axis(),
                    y: plane.position().y_axis(),
                    scale: (su, sv),
                })
            }
            SurfaceKind::BSpline(b) => Nurbs::of(b).ok().and_then(Self::continuous),
            SurfaceKind::Cone(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_)
            | SurfaceKind::SurfOfRevolution(_) => surface
                .analytic_base_with_nurb(k)
                .ok()
                .and_then(|b| Nurbs::of(b).ok())
                .and_then(Self::continuous),
            SurfaceKind::SurfOfExtrusion(_) => surface
                .extrusion_base_with_nurb(k)
                .ok()
                .and_then(|b| Nurbs::of(&b).ok())
                .and_then(Self::continuous),
            _ => None,
        }
    }

    /// The net, where no interior knot of it repeats more than its degree in either
    /// direction: the surface is then continuous, as the centred form needs; `None`
    /// otherwise (the proof is not made).
    fn continuous(net: Nurbs) -> Option<Self> {
        (interior_knots_continuous(&net.u, net.p, net.nu)
            && interior_knots_continuous(&net.v, net.q, net.nv))
        .then_some(Self::Net(net))
    }

    /// The point and first partials at every point of the box `(u, v)`, enclosed; `None`
    /// where they are not proved.
    fn partials(&self, u: Ivl, v: Ivl) -> Option<[IV3; 3]> {
        match self {
            Self::Plane { o, x, y, scale } => {
                let (su, sv) = (Ivl::from(scale.0), Ivl::from(scale.1));
                let (us, vs) = (u * su, v * sv);
                let at =
                    |o: f64, x: f64, y: f64| Ivl::from(o) + us * Ivl::from(x) + vs * Ivl::from(y);
                let point = V3 {
                    x: at(o.x, x.x, y.x),
                    y: at(o.y, x.y, y.y),
                    z: at(o.z, x.z, y.z),
                };
                let times = |a: Vec3, s: Ivl| V3 {
                    x: Ivl::from(a.x) * s,
                    y: Ivl::from(a.y) * s,
                    z: Ivl::from(a.z) * s,
                };
                Some([point, times(*x, su), times(*y, sv)])
            }
            Self::Net(net) => {
                let p = partials_over(net, u, v, 1).ok().flatten()?;
                Some([p.at(0, 0), p.at(1, 0), p.at(0, 1)])
            }
        }
    }
}

type IV3 = V3<Ivl>;

/// The curve as [`gap_within`] encloses it: a B-spline's rational net, or a line, whose
/// point is affine in its NURB parameter.
enum CurveNet<'a> {
    /// A B-spline's net.
    Net(NurbsCurve),
    /// A line.
    Line(&'a Line),
}

impl<'a> CurveNet<'a> {
    /// The enclosure of `curve`: a line, or a B-spline whose interior knots repeat at most
    /// its degree times (so it is continuous, as the centred form needs); `None` for any
    /// other.
    fn of(curve: &'a Curve) -> Option<Self> {
        match curve.kind() {
            CurveKind::BSpline(b) => NurbsCurve::of(b)
                .ok()
                .filter(|cn| interior_knots_continuous(&cn.u, cn.p, cn.n))
                .map(Self::Net),
            CurveKind::Line(line) => Some(Self::Line(line)),
            _ => None,
        }
    }

    /// The point at every parameter of `t`, enclosed.
    fn point(&self, t: Ivl) -> Option<IV3> {
        match self {
            Self::Net(cn) => curve_derivs_over(cn, t, 0, None).ok()?.first().copied(),
            Self::Line(line) => {
                // affine in t: every point over t lies between the end points, each within
                // its proved error of the computed one
                let near = |t: f64| -> Option<IV3> {
                    let q = line.evaluate_point(t);
                    let e = line.point_error(t)?;
                    let at = |c: f64| Ivl::from(c) + Ivl::new(-e, e);
                    Some(V3 {
                        x: at(q.x),
                        y: at(q.y),
                        z: at(q.z),
                    })
                };
                let (a, b) = (near(t.lo)?, near(t.hi)?);
                Some(V3 {
                    x: a.x.hull(b.x),
                    y: a.y.hull(b.y),
                    z: a.z.hull(b.z),
                })
            }
        }
    }

    /// The first derivative at every parameter of `t`, enclosed.
    fn derivative(&self, t: Ivl) -> Option<IV3> {
        match self {
            Self::Net(cn) => curve_derivs_over(cn, t, 1, None).ok()?.get(1).copied(),
            Self::Line(line) => {
                // P + s(t) scale V with s affine in t (`ConvertTFromNURBSToSTEP`): its
                // slope A / L, -A / L inside out, on a bounded line with L not zero; 1 on
                // a bounded line with L zero (s(t) = t); 1, -1 inside out, on an
                // unbounded line
                let nurb_ivl = line.natural_interval();
                let sign = if line.inside_out() { -1.0 } else { 1.0 };
                let slope = if line.is_bounded() {
                    if is_zero(nurb_ivl.length()) {
                        Ivl::from(1.0)
                    } else {
                        let a = Ivl::from(line.step_interval().max())
                            - Ivl::from(line.step_interval().min());
                        let l = Ivl::from(nurb_ivl.max()) - Ivl::from(nurb_ivl.min());
                        Ivl::from(sign) * a / l
                    }
                } else {
                    Ivl::from(sign)
                };
                let v = line.line_vector();
                let f = Ivl::from(line.line_scale()) * slope;
                Some(V3 {
                    x: Ivl::from(v.x) * f,
                    y: Ivl::from(v.y) * f,
                    z: Ivl::from(v.z) * f,
                })
            }
        }
    }
}

/// Whether no interior knot of the full knot vector `knots` (degree `p`, `n` control
/// points: the knots `p + 1 .. n`) repeats more than `p` times, so the B-spline is
/// continuous across every knot. `false` for a knot vector of the wrong length.
fn interior_knots_continuous(knots: &[f64], p: usize, n: usize) -> bool {
    if knots.len() != n + p + 1 {
        return false;
    }
    let Some(interior) = knots.get(p + 1..n) else {
        // no interior knot
        return true;
    };
    let mut run = 0usize;
    let mut prev: Option<f64> = None;
    for &k in interior {
        run = if prev == Some(k) { run + 1 } else { 1 };
        if run > p {
            return false;
        }
        prev = Some(k);
    }
    true
}

/// `a + b`, `a - b` and `a * s` of interval vectors.
fn add3(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn sub3(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn scale3(a: IV3, s: Ivl) -> IV3 {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

/// An upper bound of the length of every vector in `a`, rounding included.
fn norm_up(a: IV3) -> f64 {
    let (x, y, z) = (a.x.mag(), a.y.mag(), a.z.mag());
    round_up(round_up(round_up(round_up(x * x) + round_up(y * y)) + round_up(z * z)).sqrt())
}

/// The cubic Bezier `polygon` and its derivative at every parameter of `tau`, enclosed:
/// de Casteljau's construction and the derivative's quadratic Bernstein form in rounded
/// interval arithmetic.
fn bezier(polygon: [Point2; 4], tau: Ivl) -> ([Ivl; 2], [Ivl; 2]) {
    let one = Ivl::from(1.0);
    let s = one - tau;
    let coord = |c: [f64; 4]| {
        let [p0, p1, p2, p3] = c.map(Ivl::from);
        let three = Ivl::from(3.0);
        let point = s * s * s * p0
            + three * s * s * tau * p1
            + three * s * tau * tau * p2
            + tau * tau * tau * p3;
        let (d0, d1, d2) = (p1 - p0, p2 - p1, p3 - p2);
        let deriv = three * (s * s * d0 + Ivl::from(2.0) * s * tau * d1 + tau * tau * d2);
        (point, deriv)
    };
    let (u, du) = coord(polygon.map(|p| p.x));
    let (v, dv) = coord(polygon.map(|p| p.y));
    ([u, v], [du, dv])
}

/// `x` cut to `[lo, hi]`, which holds the exact value; `None` where they do not meet.
fn cut(x: Ivl, lo: f64, hi: f64) -> Option<Ivl> {
    let (a, b) = (x.lo.max(lo), x.hi.min(hi));
    (a <= b).then_some(Ivl::new(a, b))
}

/// Whether the cubic Bezier `polygon` in UV, run over `from..to`, has its image on
/// `surface` within `tol_3d` of `curve` at every parameter, proved with no sampling
/// (module docs, the stretch enclosure). `false` where the surface or curve kind has no
/// enclosure, a piece's partials are not proved, or the proof needs more than
/// [`MAX_ENCLOSURE_PIECES`] pieces. Infallible: a box that is not proved (an
/// enclosure's error, a NaN, a piece outside the domain) fails the proof instead.
#[allow(clippy::too_many_arguments)] // the geometry, the polygon, the range and the tolerance
pub(crate) fn gap_within(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    uv_domain: &Extent2d,
    polygon: [Point2; 4],
    from: f64,
    to: f64,
    tol_3d: f64,
) -> bool {
    if from.partial_cmp(&to) != Some(Ordering::Less)
        || !from.is_finite()
        || !to.is_finite()
        || tol_3d.partial_cmp(&0.0) != Some(Ordering::Greater)
    {
        return false;
    }
    if !polygon.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        return false;
    }
    let Some(net) = SurfaceNet::of(k, surface) else {
        return false;
    };
    let Some(cn) = CurveNet::of(curve) else {
        return false;
    };
    let (d_lo, d_hi) = (uv_domain.min(), uv_domain.max());
    let length = Ivl::from(to) - Ivl::from(from);
    // the gap D(tau) = S(q(tau)) - C(from + tau (to - from)) over a piece [a, b] of the
    // Bezier parameter lies in D(m) + D'([a, b]) ([a, b] - m), m = (a + b) / 2 (the mean
    // value form; D is continuous and piecewise smooth across the knots of either)
    let gap_over = |a: f64, b: f64| -> Option<f64> {
        let piece = Ivl::new(a, b);
        let m = 0.5 * (a + b);
        let ([u, v], [du, dv]) = bezier(polygon, piece);
        let (u, v) = (cut(u, d_lo.x, d_hi.x)?, cut(v, d_lo.y, d_hi.y)?);
        let [_, su, sv] = net.partials(u, v)?;
        let t = cut(Ivl::from(from) + piece * length, from, to)?;
        let dc = scale3(cn.derivative(t)?, length);
        let slope = sub3(add3(scale3(su, du), scale3(sv, dv)), dc);
        let ([um, vm], _) = bezier(polygon, Ivl::from(m));
        let (um, vm) = (cut(um, d_lo.x, d_hi.x)?, cut(vm, d_lo.y, d_hi.y)?);
        let [sm, _, _] = net.partials(um, vm)?;
        let tm = cut(Ivl::from(from) + Ivl::from(m) * length, from, to)?;
        let cm = cn.point(tm)?;
        let offset = piece - Ivl::from(m);
        let gap = add3(sub3(sm, cm), scale3(slope, offset));
        let bound = norm_up(gap);
        bound.is_finite().then_some(bound)
    };
    let mut stack = vec![(0.0_f64, 1.0_f64)];
    let mut pieces = 0usize;
    while let Some((a, b)) = stack.pop() {
        pieces += 1;
        if pieces > MAX_ENCLOSURE_PIECES {
            return false;
        }
        if gap_over(a, b).is_some_and(|g| g <= tol_3d) {
            continue;
        }
        let mid = 0.5 * (a + b);
        if !(a < mid && mid < b) {
            return false;
        }
        stack.push((mid, b));
        stack.push((a, mid));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::{BoundedLineOptions, Line};
    use crate::geom::vector2d::Vec2;

    fn unit() -> Extent2d {
        Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0)
    }

    fn pt(t: f64, uv: Point2, tan: Vec2) -> DropPt {
        let mut d = DropPt::new(t, Some(false));
        d.uv_pd = [uv, tan];
        d.on_pole = Some(false);
        d
    }

    /// The line `t len X` over `[0, 1]`: speed `len`.
    fn line(len: f64) -> Curve {
        Curve::from(Line::bounded(
            Vec3::ZERO,
            Vec3::X,
            &Extent1d::from_min_max(0.0, 1.0),
            &BoundedLineOptions {
                scale: len,
                ..BoundedLineOptions::default()
            },
        ))
    }

    /// The face 7 sliver: a span from just outside `v = 0` to the boundary whose
    /// tangent runs across it clamps to one point.
    #[test]
    fn a_span_outside_the_boundary_clamps_to_a_sliver() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A neighbour whose tangent leaves the domain at its clamped first point does not
    /// take over a sliver; one whose polygon lies in the domain does.
    #[test]
    fn only_a_polygon_in_the_domain_takes_over() {
        let start = pt(0.0, Point2::new(0.0, 0.0), Vec2::new(-0.3, 0.6));
        let end = pt(0.1, Point2::new(0.0, 0.05), Vec2::new(0.0, 0.5));
        assert!(polygon_in_domain(&unit(), &start, &end, 0.1).is_none());
        let start = pt(0.0, Point2::new(0.0, 0.0), Vec2::new(0.3, 0.6));
        assert!(polygon_in_domain(&unit(), &start, &end, 0.1).is_some());
        // a span of no length has no polygon
        assert!(polygon_in_domain(&unit(), &start, &start, 0.0).is_none());
    }

    /// A span on the boundary that runs along it is kept: its clamped image is long.
    #[test]
    fn a_span_along_the_boundary_is_kept() {
        let k = Kernel::default();
        let start = pt(0.0, Point2::new(0.25, 0.0), Vec2::new(0.5, 0.0));
        let end = pt(0.1, Point2::new(0.30, 0.0), Vec2::new(0.5, 0.0));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A span that touches the boundary and leaves it into the domain is kept.
    #[test]
    fn a_span_leaving_the_boundary_inward_is_kept() {
        let k = Kernel::default();
        let start = pt(0.0, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let end = pt(0.1, Point2::new(0.25, 0.05), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A tiny span that stays inside the domain is kept: clamping moves none of its
    /// points, so it is a genuine piece of the trim however short.
    #[test]
    fn a_tiny_span_inside_the_domain_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, 0.5), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.5 + 2.1e-8), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A sliver in UV whose curve piece is longer than the 3D tolerance is kept.
    #[test]
    fn a_long_curve_piece_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        // a line of length 1e3 over [0, 1]: the piece is 4.2e-5 long
        let c = line(1.0e3);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// An end on a pole keeps the span (the dog-leg straightening owns it).
    #[test]
    fn a_span_on_a_pole_is_kept() {
        let k = Kernel::default();
        let h = 4.2e-8;
        let mut start = pt(0.5 - h, Point2::new(0.25, -2.1e-8), Vec2::new(0.0, 0.5));
        start.on_pole = Some(true);
        let end = pt(0.5, Point2::new(0.25, 0.0), Vec2::new(0.0, 0.5));
        let c = line(1.0e-6);
        assert!(!is_clamped_sliver(&k, &c, &unit(), &start, &end, 1e-9, 1e-9, 1e-5).unwrap());
    }

    /// A plane `z = 0` with `uv` its `xy` coordinates over the unit square.
    fn unit_plane() -> Surface {
        Surface::from(crate::surface::plane::Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &unit(),
        ))
    }

    /// The line from `(0.25, 0.1)` to `(0.4, 0.2)` in the plane `z = h` over `[0, 1]`.
    fn line_at(h: f64) -> Curve {
        Curve::from(Line::bounded(
            Vec3::new(0.25, 0.1, h),
            Vec3::new(0.15, 0.1, 0.0),
            &Extent1d::from_min_max(0.0, 1.0),
            &BoundedLineOptions {
                scale: Vec3::new(0.15, 0.1, 0.0).length(),
                ..BoundedLineOptions::default()
            },
        ))
    }

    /// The span of the line's drop onto the plane over `a..b`: its exact image.
    fn line_span(a: f64, b: f64) -> (DropPt, DropPt) {
        let p = |t: f64| Point2::new(0.25 + 0.15 * t, 0.1 + 0.1 * t);
        let d = Vec2::new(0.15, 0.1);
        (pt(a, p(a), d), pt(b, p(b), d))
    }

    /// A stretched span keeps its ends and the neighbour's tangent scale, so the polygon
    /// made of it is the neighbour's bit for bit; on a plane its image is proved within
    /// the tolerance where the shift of parameter is small.
    #[test]
    fn a_stretched_span_keeps_its_polygon() {
        let k = Kernel::default();
        let s = unit_plane();
        let c = line_at(1.0e-7);
        let (start, end) = line_span(0.5, 0.75);
        let span = Span::new(start, end);
        let st = span
            .stretch(&k, &s, &c, &unit(), 0.5 - 4.2e-8, 0.75, (1e-9, 1e-9), 1e-5)
            .unwrap();
        assert_eq!((st.start.t, st.end.t), (0.5 - 4.2e-8, 0.75));
        assert_eq!((st.start.uv_pd, st.end.uv_pd), (start.uv_pd, end.uv_pd));
        assert_eq!(
            polygon(&st.start, &st.end, st.tangent_scale),
            polygon(&start, &end, end.t - start.t)
        );
        // stretched again over a last sliver
        let st2 = st
            .stretch(
                &k,
                &s,
                &c,
                &unit(),
                0.5 - 4.2e-8,
                0.75 + 4.2e-8,
                (1e-9, 1e-9),
                1e-5,
            )
            .unwrap();
        assert_eq!(st2.tangent_scale, end.t - start.t);
    }

    /// A span is not stretched over a range that does not keep one of its ends, where its
    /// polygon leaves the domain, or where its image is not proved within the tolerance:
    /// a curve farther from the plane than the tolerance, or a stretch whose shift of
    /// parameter moves the curve point farther than it.
    #[test]
    fn a_span_is_stretched_only_where_proved_within_the_tolerance() {
        let k = Kernel::default();
        let s = unit_plane();
        let d = unit();
        let c = line_at(1.0e-7);
        let (start, end) = line_span(0.5, 0.75);
        let span = Span::new(start, end);
        let stretch = |sp: &Span, c: &Curve, from: f64, to: f64| {
            sp.stretch(&k, &s, c, &d, from, to, (1e-9, 1e-9), 1e-5)
        };
        // both ends moved, or neither; a range inside the span's
        assert!(stretch(&span, &c, 0.4, 0.8).is_none());
        assert!(stretch(&span, &c, 0.5, 0.75).is_none());
        assert!(stretch(&span, &c, 0.6, 0.75).is_none());
        // the curve 2e-3 off the plane
        assert!(stretch(&span, &line_at(2.0e-3), 0.5 - 4.2e-8, 0.75).is_none());
        // a shift of 0.1 in parameter: the curve point moves about 0.018
        assert!(stretch(&span, &c, 0.4, 0.75).is_none());
        // a polygon leaving the domain
        let mut out = start;
        out.uv_pd = [Point2::new(0.0, 0.1), Vec2::new(-0.3, 0.5)];
        assert!(stretch(&Span::new(out, end), &c, 0.4, 0.75).is_none());
        // an end on a pole (or not known to be off one) whose polygon is no dog leg is
        // not straightened, so the stretch stands
        let mut on_pole = end;
        on_pole.on_pole = Some(true);
        assert!(stretch(&Span::new(start, on_pole), &c, 0.5 - 4.2e-8, 0.75).is_some());
        let mut unknown = start;
        unknown.on_pole = None;
        assert!(stretch(&Span::new(unknown, end), &c, 0.5 - 4.2e-8, 0.75).is_some());
        // the end in common within the tolerance band of a boundary, with the curve
        // running along it, where the flush's fix-up moves the polygon point next to it:
        // v = 0.175 within 0.2 of v = 0
        let near = span.stretch(&k, &s, &c, &d, 0.5 - 4.2e-8, 0.75, (1e-9, 0.2), 1e-5);
        assert!(near.is_none());
        let clear = span.stretch(&k, &s, &c, &d, 0.5 - 4.2e-8, 0.75, (1e-9, 0.17), 1e-5);
        assert!(clear.is_some());
    }

    /// The fix-up moves the point next to a knot in a boundary's band only where the
    /// curve runs along the boundary there (or the knot is on it) and the point does not
    /// already have the knot's coordinate.
    #[test]
    fn the_fix_up_moves_only_a_point_along_a_boundary() {
        let d = unit();
        let knot = Point2::new(0.5, 0.05);
        // in the v band, running along v = 0: moved
        assert!(fix_up_moves(&d, knot, Point2::new(0.6, 0.07), 0.0, 0.1));
        // running across it: kept
        assert!(!fix_up_moves(&d, knot, Point2::new(0.52, 0.15), 0.0, 0.1));
        // out of the band: kept
        assert!(!fix_up_moves(&d, knot, Point2::new(0.6, 0.07), 0.0, 0.04));
        // the knot on the boundary moves even a point across it
        let on = Point2::new(0.5, 0.0);
        assert!(fix_up_moves(&d, on, Point2::new(0.52, 0.15), 0.0, 0.1));
        // a point with the knot's coordinate already does not move
        assert!(!fix_up_moves(&d, knot, Point2::new(0.6, 0.05), 0.0, 0.1));
    }

    /// A dog-leg polygon is straightened where an end is on a pole or not known to be off
    /// one, so it is never stretched there; off a pole, or with no dog leg, it is not.
    #[test]
    fn a_dog_leg_on_a_pole_is_straightened() {
        // a 'z': legs (0, 0.02) either side of the middle (0.2, 0), 11 degrees off p1 p4
        let z = [
            Point2::new(0.3, 0.3),
            Point2::new(0.3, 0.32),
            Point2::new(0.5, 0.32),
            Point2::new(0.5, 0.34),
        ];
        let (start, end) = line_span(0.5, 0.75);
        let (mut pole, mut unknown) = (start, end);
        pole.on_pole = Some(true);
        unknown.on_pole = None;
        assert!(straightened(&pole, &end, z));
        assert!(straightened(&start, &unknown, z));
        assert!(!straightened(&start, &end, z));
        let straight = polygon(&start, &end, 0.25).unwrap();
        assert!(!straightened(&pole, &unknown, straight));
        // the stretch refuses it on a pole
        let k = Kernel::default();
        let tan = Vec2::new(0.0, 0.24);
        let (a, mut b) = (pt(0.5, z[0], tan), pt(0.75, z[3], tan));
        b.on_pole = Some(true);
        let span = Span::new(a, b);
        assert!(straightened(
            &a,
            &b,
            polygon(&a, &b, span.tangent_scale).unwrap()
        ));
        let st = span.stretch(
            &k,
            &unit_plane(),
            &line_at(1e-7),
            &unit(),
            0.4,
            0.75,
            (1e-9, 1e-9),
            1e-5,
        );
        assert!(st.is_none());
    }

    /// A B-spline whose interior knot repeats more than its degree may jump there, so it
    /// gets no centred enclosure; one repeated at most its degree times does.
    #[test]
    fn only_a_continuous_net_is_enclosed() {
        // cubic, 6 control points: interior knots are indices 4 and 5
        let clamped = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0];
        assert!(interior_knots_continuous(&clamped, 3, 6));
        // degree 1, 4 control points: the interior knot 0.5 twice is a jump
        let jump = [0.0, 0.0, 0.5, 0.5, 1.0, 1.0];
        assert!(!interior_knots_continuous(&jump, 1, 4));
        // one interior knot once is continuous
        assert!(interior_knots_continuous(&[0.0, 0.0, 0.5, 1.0, 1.0], 1, 3));
        // no interior knot, and a knot vector of the wrong length
        assert!(interior_knots_continuous(&[0.0, 0.0, 1.0, 1.0], 1, 2));
        assert!(!interior_knots_continuous(&[0.0, 0.0, 1.0], 1, 2));
    }

    /// The enclosure gives up past its piece budget instead of looping: a tolerance far
    /// below the boxes' rounding is never proved.
    #[test]
    fn the_enclosure_is_bounded() {
        let k = Kernel::default();
        let (start, end) = line_span(0.0, 1.0);
        let q = polygon(&start, &end, 1.0).unwrap();
        let c = line_at(0.0);
        let s = unit_plane();
        assert!(gap_within(&k, &s, &c, &unit(), q, 0.0, 1.0, 1e-5));
        assert!(!gap_within(&k, &s, &c, &unit(), q, 0.0, 1.0, 1e-300));
    }

    /// The line's derivative enclosure holds its slope in every parameterisation
    /// `ConvertTFromNURBSToSTEP` has: bounded or not, inside out or not, and a bounded
    /// line with a zero-length NURB domain (which maps a parameter to itself). The
    /// difference quotient over `[0, 1]` is exact here (`P = 0`, `V = X`, speed 2).
    #[test]
    fn a_line_derivative_holds_its_slope_in_every_parameterisation() {
        let bounded = |lo: f64, hi: f64, inside_out: bool| {
            Line::bounded(
                Vec3::ZERO,
                Vec3::X,
                &Extent1d::from_min_max(lo, hi),
                &BoundedLineOptions {
                    scale: 2.0,
                    inside_out,
                    ..BoundedLineOptions::default()
                },
            )
        };
        let unbounded = |inside_out: bool| Line::infinite(Vec3::ZERO, Vec3::X, 3, inside_out);
        let lines = [
            bounded(0.0, 1.0, false),
            bounded(0.0, 1.0, true),
            bounded(0.5, 0.5, false),
            bounded(0.5, 0.5, true),
            unbounded(false),
            unbounded(true),
        ];
        for line in &lines {
            let quotient = line.evaluate_point(1.0) - line.evaluate_point(0.0);
            let d = CurveNet::Line(line).derivative(Ivl::new(0.0, 1.0)).unwrap();
            assert!(
                d.x.lo <= quotient.x && quotient.x <= d.x.hi,
                "{line:?}: {quotient:?} not in {d:?}"
            );
            assert!(d.x.lo > 0.0 || d.x.hi < 0.0, "{line:?}: {d:?}");
        }
    }
}
