//! Weld beads as bodies of their own (#15129, the welds epic #15132): a bead built from its own
//! faces, apart from the body it is laid on, which is left as it was. No boolean with the
//! body. The foot blend reads the edge's binormals on copies of the body, one per reading
//! (a shortcut in [`crate::blend::foot_blend`]), so that cost grows with the body.
//!
//! Between planes along a straight edge the exact bead's body ([`beads_apart`]) is its
//! section swept along the edge: the edge's point, the feet at the setbacks on each face, and
//! the arc between them ([`BeadArc`], the exact circle the blend builds the bead face from).
//! Its faces are the bead face, the strips of the two planes between the edge and the feet,
//! and the two section ends. On any joint the bead is built on the foot blend
//! ([`bead_body`], [`crate::blend::foot_blend`]); a span ([`BeadSpan`]) builds its share of
//! the edge only, so an intermittent weld is one body per span.
//!
//! A span ends square to the edge, or tapers, or is rounded ([`BeadEnd`]); the last two only
//! straight between two planes so far, cut from the span's own small planar body: the span is swept past its end first, so each cut crosses
//! the bead's edges instead of passing through its corners, then
//! - a taper is cut by the plane through the span's end on the edge at 45 degrees to the edge,
//!   rising into the span along the bead's middle direction: a ramp as long as the bead is
//!   high, the strips narrowing to the end's point;
//! - a rounded end is cut by the sphere about the edge one bead reach inside the span's end:
//!   the bead's outline on each plate ends in a round, the section shrinking to the end's
//!   point on the edge.
//!
//! Refused before anything is built, the part left as it was, as
//! [`BlendFailure::WeldNotBuilt`] (class `Unsupported`) naming the edge or vertex: a joint
//! that is not a straight edge between two planes ([`WeldGap::CurvedJoint`]: the bead on any
//! joint, the foot blend, comes with its certificate), beads that meet at a vertex
//! ([`WeldGap::BeadsMeetAtVertex`]: their corner) and a bead with an obtuse toe
//! ([`WeldGap::ObtuseToe`]). As too large (class `FilletTooLarge`), naming the size that
//! builds: a toe past its face's trims ([`NoRoomReason::LegPastFace`], the leg) and a sagitta
//! past its bound ([`NoRoomReason::BeadCutsFace`], the sagitta). As invalid input: an edge
//! that is not an inside corner (an outside or a tangent edge), an edge listed twice, sizes
//! not one per edge, and a leg below the body's resolution
//! ([`BlendFailure::SizeBelowResolution`] naming the edge and the leg, as a chamfer's).
//!
//! The bead face keeps the generic blend's definition, as the blend's own bead face does: the
//! chord (order 0) of the bead's sizes on the edge's two planes, with its sagitta
//! ([`crate::surface::procedural_blend::exact::ExactRound::with_sagitta`]), re-certified on
//! the cylinder through the feet's lines. So a bead apart is exact and saved as one.
//!
//! The bead never trims or changes the body (#15132).
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::blend_refusal::setback_below_resolution;
use crate::blend::fillet_exact::ClassInputs;
use crate::blend::fillet_executive::p1::is_radius_supported;
use crate::blend::fillet_executive::NoRoomReason;
use crate::boolean::api_brep::boolean_difference;
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::blend::{BlendFailure, WeldGap};
use crate::error::{
    Failure, Fit, FitMeasure, Result, Site, SolidError, TargetEdge, TargetFace, TargetVertex,
};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_RAD;
use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::primitive::api_curves::create_line_segment;
use crate::primitive::api_primitives::create_sphere;
use crate::surface::procedural_blend::bead::{BeadArc, BeadDefinition, BeadSpan};
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::surface::procedural_blend::exact::ExactRound;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{Brep, EdgeId, FaceId, VertexId};
use core::f64::consts::TAU;

const OP: &str = "blend::weld_bead";

/// The refusal of a bead the kernel does not build yet, for `gap`, at `site`.
#[track_caller]
fn not_built(gap: WeldGap, site: Site) -> SolidError {
    Failure::new(OP, BlendFailure::WeldNotBuilt(gap))
        .or_at(site)
        .into_error()
}

/// How many pieces [`toes_on_faces`] cuts the edge into: it checks the toes at their ends.
const TOE_PIECES: u32 = 32;

/// Checks that each toe lies on its face of `target`, its trims included, not only on the
/// face's plane: per side `(face, surface, u, d)` the toe `d` from the edge along `u`, at the
/// ends of [`TOE_PIECES`] equal pieces of the edge from `start` over `run`. `target` is only
/// read.
///
/// shortcut: checked at those stations, not between them (a notch in a face narrower than a
/// piece is missed); upgrade to the toe line against the face's boundary when a part can be
/// notched that finely
///
/// # Errors
///
/// The refusal [`NoRoomReason::LegPastFace`] naming the edge and the face, with the largest
/// leg on that side whose toes stay on the face at the stations (halved down from the leg
/// until the bracket is `1e-7 (1 + hi)` wide, `hi` its upper end, for legs below about
/// `1e-7 * 2^64`, past which the 64 halvings end first), or 0 where that leg is below the
/// smallest the body builds ([`is_radius_supported`]: eight times its tolerance), as no leg
/// on that side builds; those of the drop and of the classification.
fn toes_on_faces(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    sides: [(FaceId, &Surface, Vec3, f64); 2],
    start: Point3,
    run: Vec3,
) -> Result<()> {
    for (face, surface, u, d) in sides {
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let holds = |leg: f64| -> Result<bool> {
            for j in 0..=TOE_PIECES {
                let p = start + run * (f64::from(j) / f64::from(TOE_PIECES)) + u * leg;
                if !toe_on_face(k, target, face, surface, &domain, p)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        if holds(d)? {
            continue;
        }
        // the largest leg that holds, halved down until the bracket is a ten-millionth of its
        // upper end: of the leg found, not of the leg asked, so a face far shorter than the leg
        // asked still gets its fit (64 halvings take 1e6 down to well under 1e-7)
        let (mut lo, mut hi) = (0.0_f64, d);
        for _ in 0..64 {
            if hi - lo <= 1e-7 * (1.0 + hi) {
                break;
            }
            let mid = 0.5 * (lo + hi);
            if holds(mid)? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // a toe holds up to the face's tolerance past its edge: the fit named keeps it on the
        // face itself
        let fit = (lo - target.face(face)?.tolerance).max(0.0);
        // a leg below the body's resolution is refused before the toes are read
        // (`setback_below_resolution`), so a retry at it would not build: no leg fits
        let fit = if is_radius_supported(fit, target.tolerance()) {
            fit
        } else {
            0.0
        };
        let edge = Some(TargetEdge::of_target(edge_id));
        return Err(
            Failure::new(OP, BlendFailure::NoRoom(NoRoomReason::LegPastFace))
                .or_at(Site {
                    edge,
                    face: Some(TargetFace::of_target(face)),
                    at: Some(start + u * fit),
                    ..Site::default()
                })
                .with_fit(Fit::new(fit, FitMeasure::Leg, edge))
                .into_error(),
        );
    }
    Ok(())
}

/// Whether the point `p` on the plane of `face` (its `surface`, over `domain`) lies on `face`
/// of `target`, its boundary included ([`face_holds`]). A point past the surface's own domain
/// drops onto the domain's rim, so it holds only where the surface there is `p` itself (to
/// the face's tolerance). `target` is only read.
///
/// # Errors
///
/// A defect where `p` does not drop onto the surface or the surface gives no point there;
/// those of [`face_holds`].
fn toe_on_face(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    surface: &Surface,
    domain: &Extent2d,
    p: Point3,
) -> Result<bool> {
    let uv = dropped(k, surface, domain, p, None)?.ok_or_else(|| {
        SolidError::blend(
            OP,
            BlendFailure::Defect {
                why: "dropping a toe onto its face's plane found no parameter, though the toe was built in that plane",
            },
        )
    })?;
    let on = surface
        .evaluate(k, uv, 0, 0, EvaluateFlags::default())?
        .first()
        .and_then(|r| r.first())
        .copied()
        .ok_or_else(|| {
            SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "evaluating a face's plane at a parameter of its domain returned no point",
                },
            )
        })?;
    if on.distance(p) > target.face(face)?.tolerance {
        return Ok(false);
    }
    face_holds(k, target, face, surface, domain, [None, None], uv)
}

/// The parameters of `surface` (over `domain`) nearest `point`, from `near` when given;
/// `None` where the drop finds none.
///
/// # Errors
///
/// Those of the drop.
fn dropped(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    point: Point3,
    near: Option<Point2>,
) -> Result<Option<Point2>> {
    let d = surface.drop_point::<crate::surface::surface::p4::NoObject>(
        k,
        point,
        domain,
        &crate::surface::surface::p5::DropPointOptions {
            guess_uv: near,
            ..crate::surface::surface::p5::DropPointOptions::default()
        },
    )?;
    Ok(d.success.then_some(d.result_uv))
}

/// Whether `uv` of `surface` (the support of `face`, over `domain` with `periods`) lies on
/// `face` of `target`, its boundary included, reading `target` only: the kernel's
/// classification where every trim curve it visits is already made
/// ([`crate::query::face::p2::point_classify_prepared`]); where one is not (a part saved
/// without some edge uses' uv curves), the classification would make it in the part, so
/// [`boundary_holds`] decides instead.
///
/// # Errors
///
/// Those of the classification other than a trim curve it would have to make, and those of
/// [`boundary_holds`].
pub(crate) fn face_holds(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    surface: &Surface,
    domain: &Extent2d,
    periods: [Option<f64>; 2],
    uv: Point2,
) -> Result<bool> {
    use crate::intersect::point_class::{PointClassType as C, PointClassification};
    use crate::query::face::p2::{point_classify_prepared, PointClassifyOptions};
    let tolerance = crate::geom::tol::zone_tol3d(k, crate::geom::tol::USE_DEFAULT);
    let mut classification = PointClassification::new(tolerance);
    let options = PointClassifyOptions {
        src_zone_tol_3d: target.face(face)?.tolerance,
        use_uv_classification: true,
        ..PointClassifyOptions::default()
    };
    match point_classify_prepared(k, target, face, uv, &mut classification, options) {
        Ok(()) => Ok(matches!(
            classification.point_class(),
            C::Face | C::Edge | C::Vertex | C::Edgeuse
        )),
        Err(e) if e.is_unported() => boundary_holds(k, target, face, surface, domain, periods, uv),
        Err(e) => Err(e),
    }
}

/// How many points [`boundary_holds`] samples on each edge of a face's boundary.
const BOUNDARY_SAMPLES: usize = 32;

/// Whether `uv` lies inside `face` of `target` by its boundary dropped onto `surface`: each
/// loop's edges, each at [`BOUNDARY_SAMPLES`] points in its edge use's direction, dropped onto
/// the surface (a periodic direction unwrapped along the loop), and `uv` inside by the
/// even-odd rule over all loops (an outer loop and its holes), moved by whole periods to the
/// loops' range; on the boundary (within a millionth of the loops' size) counts as on the face,
/// as the kernel's classification counts an edge or a vertex. Sampled, not exact: it decides only where the kernel's classification would
/// have to make a trim curve in the part ([`face_holds`]). `target` is only read.
///
/// # Errors
///
/// A defect where an edge has no curve or a boundary point does not drop onto the surface;
/// those of the topology and the evaluations.
fn boundary_holds(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    surface: &Surface,
    domain: &Extent2d,
    periods: [Option<f64>; 2],
    uv: Point2,
) -> Result<bool> {
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::topology::edgeuse::p1 as eu1;
    let unwrap = |x: f64, near: f64, period: Option<f64>| match period {
        Some(p) if p > 0.0 && p.is_finite() => x + p * ((near - x) / p).round(),
        _ => x,
    };
    let mut polygons: Vec<Vec<Point2>> = Vec::new();
    for l in crate::topology::face::p2::loops(target, face, false, None)? {
        let mut polygon: Vec<Point2> = Vec::new();
        for eu in crate::topology::loopuse::loop_edgeuses(target, l)? {
            let e = eu1::edge(target, eu)?.ok_or_else(|| {
                SolidError::invalid_input(OP, "an edge use of a face loop with no edge")
            })?;
            let curve = edge::curve(target, e)?
                .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
            let ivl = edge::interval(target, e)?;
            let reversed = eu1::orientation(target, eu)? == OrientType::Opposite;
            for j in 0..BOUNDARY_SAMPLES {
                let f = j as f64 / BOUNDARY_SAMPLES as f64;
                let f = if reversed { 1.0 - f } else { f };
                let t = ivl.min() + (ivl.max() - ivl.min()) * f;
                let p = curve.evaluate_point(k, t)?;
                let near = polygon.last().copied();
                let q = dropped(k, surface, domain, p, near)?.ok_or_else(|| SolidError::blend(OP, BlendFailure::Defect { why: "dropping a point of the face's own boundary edge onto the face's surface found no parameter, though the edge lies on that face" }))?;
                let [pu, pv] = periods;
                let q = match near {
                    Some(n) => Point2::new(unwrap(q.x, n.x, pu), unwrap(q.y, n.y, pv)),
                    None => q,
                };
                polygon.push(q);
            }
        }
        if polygon.len() >= 3 {
            polygons.push(polygon);
        }
    }
    // the point moved by whole periods to the middle of the loops' range
    let (mut lo, mut hi) = (
        Point2::new(f64::INFINITY, f64::INFINITY),
        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for q in polygons.iter().flatten() {
        lo = Point2::new(lo.x.min(q.x), lo.y.min(q.y));
        hi = Point2::new(hi.x.max(q.x), hi.y.max(q.y));
    }
    let [pu, pv] = periods;
    let at = Point2::new(
        unwrap(uv.x, 0.5 * (lo.x + hi.x), pu),
        unwrap(uv.y, 0.5 * (lo.y + hi.y), pv),
    );
    // on the boundary (to a millionth of the loops' size, in the parameters): on the face, as
    // the kernel's classification counts an edge or a vertex
    let near = 1e-6 * (hi.x - lo.x).max(hi.y - lo.y).max(f64::MIN_POSITIVE);
    for polygon in &polygons {
        for (j, &a) in polygon.iter().enumerate() {
            let Some(&b) = (j + 1)
                .checked_rem(polygon.len())
                .and_then(|n| polygon.get(n))
            else {
                continue;
            };
            let (ab, aq) = (b - a, at - a);
            let t = (aq.dot(ab) / ab.dot(ab).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
            if (aq - ab * t).length() <= near {
                return Ok(true);
            }
        }
    }
    let mut inside = false;
    for polygon in &polygons {
        for (j, &a) in polygon.iter().enumerate() {
            let Some(&b) = (j + 1)
                .checked_rem(polygon.len())
                .and_then(|n| polygon.get(n))
            else {
                continue;
            };
            if (a.y > at.y) != (b.y > at.y) {
                let x = a.x + (at.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if at.x < x {
                    inside = !inside;
                }
            }
        }
    }
    Ok(inside)
}

/// How a bead's span ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BeadEnd {
    /// Cut off square to the edge: the bead's section is the end face.
    #[default]
    Square,
    /// A 45-degree ramp down to the end's point on the edge, as long as the bead is high.
    Taper,
    /// Rounded: the bead shrinks to the end's point on the edge within one bead reach, its
    /// outline on each plate ending in a round.
    Round,
}

/// One bead's frame: where its edge runs, its section at the edge's start, and its sizes.
struct Frame {
    /// The edge's start (the start of its curve's interval).
    start: Point3,
    /// The unit direction along the edge.
    dir: Vec3,
    /// The edge's length.
    length: f64,
    /// The feet's offsets from the edge's point, in the plane square to the edge.
    feet: [Vec3; 2],
    /// The arc between the feet, at the edge's start; none for a flat bead (sagitta 0), whose
    /// face is the chord.
    arc: Option<BeadArc>,
    /// The unit direction from the edge into the bead, square to it: to the chord's middle.
    up: Vec3,
    /// How far the bead reaches from the edge (its section's farthest point), and how high it
    /// stands along `up`.
    reach: f64,
    height: f64,
    /// The bead's sizes and sagitta.
    bead: BeadDefinition,
    /// The edge's two planes, the first the one `size`'s first distance runs along.
    supports: [Surface; 2],
    /// What the chord's class is read with: the planes at the edge, the ball's centre and its
    /// signed offsets.
    inputs: ClassInputs,
}

/// The bead along the straight edge `edge_id` of `target` between two planes, at `size` and
/// `sagitta`: the first of `size`'s distances runs along the face `along` when given (one of the
/// edge's two faces), else along the edge's first face.
///
/// # Errors
///
/// [`WeldGap::CurvedJoint`] naming the edge for an edge that is not straight or a face on it
/// that is not a plane; [`WeldGap::ObtuseToe`] naming the edge for legs whose toe is obtuse;
/// native invalid input for an edge with no curve, not between two faces, or not an inside
/// corner, and for an `along` that is not one of the edge's faces;
/// [`BlendFailure::SizeBelowResolution`] naming the edge for a leg below the smallest
/// size a fillet builds on `target`; [`NoRoomReason::BeadCutsFace`] naming the edge, with the
/// sagitta that builds, for a sagitta past its bound; those of the evaluations.
fn frame(
    k: &Kernel,
    target: &mut Brep,
    edge_id: EdgeId,
    size: ChamferSize,
    sagitta: f64,
    along: Option<FaceId>,
) -> Result<Frame> {
    let curve = target
        .edge(edge_id)?
        .curve
        .clone()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    let at_edge = Site::edge(TargetEdge::of_target(edge_id));
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Err(not_built(WeldGap::CurvedJoint, at_edge));
    }
    let interval = edge::interval(target, edge_id)?;
    let at = |t: f64| -> Result<Point3> {
        curve
            .evaluate(k, interval.evaluate(t), 0, false, false)?
            .first()
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "the edge's curve gave no point"))
    };
    let start = at(0.0)?;
    let run = at(1.0)? - start;
    let length = run.length();
    let dir = run.unitize()?;
    // the edge's two faces: each one's direction into it from the edge and its normal
    let middle = interval.evaluate(0.5);
    let mut sides: Vec<(FaceId, Vec3, Vec3, Surface, Point2)> = Vec::with_capacity(2);
    // the edge uses on the void's side (the infinite region's shell): their faces' normals
    // point out of the material, whichever way the faces' surfaces run
    let void = target.infinite_region();
    for eu in edge::edgeuses(target, edge_id, None, None)? {
        let Some(face) = eu1::face(target, eu)? else {
            continue;
        };
        let region = match eu1::shell(target, eu)? {
            Some(shell) => target.shell(shell)?.region,
            None => None,
        };
        if void.is_some() && region != void {
            continue;
        }
        if sides.iter().any(|s| s.0 == face) {
            continue;
        }
        let planar = target
            .face(face)?
            .surface
            .as_ref()
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        if !planar {
            return Err(not_built(WeldGap::CurvedJoint, at_edge));
        }
        let eval = evaluate_binormal(k, target, eu, middle, true, &BinormalOptions::default())?;
        let surface = target
            .face(face)?
            .surface
            .clone()
            .ok_or_else(|| SolidError::invalid_input(OP, "a face with no surface"))?;
        sides.push((
            face,
            eval.binormal.unitize()?,
            eval.face_normal.unitize()?,
            surface,
            eval.uv,
        ));
    }
    let [first, second] = <[_; 2]>::try_from(sides)
        .map_err(|_| SolidError::invalid_input(OP, "an edge between two faces"))?;
    if along.is_some_and(|f| f != first.0 && f != second.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a bead's first leg runs along one of its edge's two faces",
        ));
    }
    let [(face1, u1, n1, s1, uv1), (face2, u2, _, s2, uv2)] = if along == Some(second.0) {
        [second, first]
    } else {
        [first, second]
    };
    // an inside corner: the second face rises on the first one's outer side. `n1 . u2` is the
    // sine of the second face's rise off the first one's plane (both unit), an angle: at or
    // below the angle zero the edge is an outside corner (negative) or tangent (zero), where
    // the chord would run through the part or through the edge
    if n1.dot(u2) <= EFF_ZERO_RAD {
        return Err(SolidError::invalid_input(
            OP,
            "a weld bead lies in an inside corner (a concave edge) only, not on an outside or a tangent edge",
        ));
    }
    let wedge = u1.cross(u2).length().atan2(u1.dot(u2));
    let chamfer = ChamferDefinition::new(size, wedge)?;
    let [d1, d2] = chamfer.setbacks();
    // a leg is a setback, how far a toe lies from the edge along its face, whether given as a
    // distance or computed from an angle: one below the smallest size a fillet builds on the
    // body (an angle so small its second leg is far below the body's tolerance) puts the toe
    // on the edge within tolerance, a sliver, refused as the chamfer refuses it (#21138).
    // Checked before the sagitta's bound: the sliver's tiny chord would otherwise be
    // reported as a sagitta too large, hiding the leg that is wrong
    setback_below_resolution(OP, edge_id, [d1, d2], target.tolerance())
        .map_err(|e| e.or_at(at_edge))?;
    // the legs before the sagitta: an obtuse toe and a toe past its face do not depend on the
    // sagitta, so a retry at the sagitta that fits is not refused for a leg. Legs with an
    // obtuse toe fold the chamfer's ball (its offsets are not both positive), and the
    // cylinder's side is read from that ball, so they are refused here
    // (`ChamferDefinition::offsets` refuses nothing else)
    let Ok(offsets) = chamfer.offsets() else {
        return Err(not_built(WeldGap::ObtuseToe, at_edge));
    };
    toes_on_faces(
        k,
        target,
        edge_id,
        [(face1, &s1, u1, d1), (face2, &s2, u2, d2)],
        start,
        run,
    )?;
    // a sagitta past its bound: refused with the bound, the size that builds
    if let Err(refused) = (BeadDefinition { chamfer, sagitta }).check() {
        return Err(sagitta_refusal(edge_id, refused.bound));
    }
    let bead = BeadDefinition::new(chamfer, sagitta)?;
    let feet = [u1 * d1, u2 * d2];
    let up_raw = (feet[0] + feet[1]) * 0.5;
    let up = up_raw.unitize()?;
    let arc = (sagitta != 0.0)
        .then(|| BeadArc::new(start + feet[0], start + feet[1], dir, up_raw, sagitta))
        .transpose()?;
    // a flat bead's section is the triangle: its feet are its farthest points
    let reach = d1
        .max(d2)
        .max(arc.as_ref().map_or(0.0, |a| arc_extreme(a, start, None)));
    let height = (feet[0].dot(up)).max(feet[1].dot(up)).max(
        arc.as_ref()
            .map_or(0.0, |a| arc_extreme(a, start, Some(up))),
    );
    // the ball's centre at the edge's start: its offsets from the planes along the normals
    // into the wedge, its feet the bead's (the chamfer's offsets, positive)
    let into = (u2 - u1 * u1.dot(u2)).unitize()?;
    let centre = start + feet[0] + into * offsets[0];
    // signed along each plane's own normal, as the blend reads them
    let mut signed = [0.0; 2];
    for ((o, (surface, uv)), (foot, r)) in signed
        .iter_mut()
        .zip([(&s1, uv1), (&s2, uv2)])
        .zip(feet.iter().zip(offsets))
    {
        let normal = surface
            .evaluate_normal(k, uv, Side::Left, Side::Left)?
            .unitize()?;
        *o = (centre - (start + *foot)).dot(normal).signum() * r;
    }
    let inputs = ClassInputs {
        uv: [uv1, uv2],
        offsets: signed,
        centre,
        tol: edge::tolerance(target, edge_id)?,
        ball_on_collapsed: false,
        radii_can_change: false,
    };
    Ok(Frame {
        start,
        dir,
        length,
        feet,
        arc,
        up,
        reach,
        height,
        bead,
        supports: [s1, s2],
        inputs,
    })
}

/// The refusal of a bead whose sagitta is past `bound` along `edge`: the bound is open, so the
/// fit is the float below it, and a retry at the fit builds.
fn sagitta_refusal(edge: EdgeId, bound: f64) -> SolidError {
    use crate::blend::fillet_executive::NoRoomReason;
    use crate::error::{Fit, FitMeasure};
    let edge = TargetEdge::of_target(edge);
    // bits, not next_down, for the 1.85 MSRV
    let fit = if bound > 0.0 {
        f64::from_bits(bound.to_bits() - 1)
    } else {
        0.0
    };
    Failure::new(OP, BlendFailure::NoRoom(NoRoomReason::BeadCutsFace))
        .or_at(Site::edge(edge))
        .with_fit(Fit::new(fit, FitMeasure::Sagitta, Some(edge)))
        .into_error()
}

/// The arc's extreme from `from`: its greatest distance from it (`along` `None`) or its
/// greatest extent along the unit `along`, where that is reached inside the arc (else 0: the
/// feet bound it then). On a circle the distance from a point is greatest along the ray from
/// the point through the centre, the extent along a direction at the centre plus the radius
/// along it; each varies one way between the arc's ends otherwise.
fn arc_extreme(arc: &BeadArc, from: Point3, along: Option<Vec3>) -> f64 {
    let towards = match along {
        Some(d) => d,
        None => match (arc.centre - from).unitize() {
            Ok(d) => d,
            Err(_) => return 0.0,
        },
    };
    let mut angle = towards.dot(arc.y).atan2(towards.dot(arc.x));
    if angle < 0.0 {
        angle += TAU;
    }
    if angle > arc.turn {
        return 0.0;
    }
    match along {
        Some(d) => (arc.centre - from).dot(d) + arc.radius,
        None => (arc.centre - from).length() + arc.radius,
    }
}

impl Frame {
    /// The bead swept from `t0` to `t1` along the edge (distances from its start; either may
    /// lie past the edge's ends).
    fn sweep(&self, k: &mut Kernel, t0: f64, t1: f64) -> Result<Brep> {
        // swept counter-clockwise about the sweep (the feet's turn about the edge)
        let ccw = self.feet[0].cross(self.feet[1]).dot(self.dir) > 0.0;
        let (base, way) = if ccw { (t0, self.dir) } else { (t1, -self.dir) };
        let shift = self.dir * base;
        let edge = self.start + shift;
        let f1 = edge + self.feet[0];
        let f2 = edge + self.feet[1];
        let face = match &self.arc {
            Some(arc) => {
                let mut placement = Axis2Placement::default();
                placement.set_canonical(arc.centre + shift, arc.x, arc.y)?;
                Curve::from(BSplineCurve::create_circle_segment(
                    3,
                    &placement,
                    arc.radius,
                    0.0,
                    arc.turn.to_degrees(),
                    NurbCircleParam::Quadratic,
                )?)
            }
            None => Curve::from(create_line_segment(f1, f2)?),
        };
        let curves = [
            Curve::from(create_line_segment(edge, f1)?),
            face,
            Curve::from(create_line_segment(f2, edge)?),
        ];
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let mut body = create_linear_sweep(k, &refs, way, t1 - t0, true)?;
        self.define_bead_face(k, &mut body)?;
        Ok(body)
    }

    /// Gives the swept body's bead face the chord's definition with the bead's sagitta,
    /// re-certified on it: its one face that is not a plane, or for a flat bead the plane
    /// through both feet's lines (its normal along the chord's square in the section).
    ///
    /// # Errors
    ///
    /// Native invalid input for a body with no such face or more than one; those of the
    /// normals, the definition and its re-certification.
    fn define_bead_face(&self, k: &Kernel, body: &mut Brep) -> Result<()> {
        let chord_normal = (self.feet[1] - self.feet[0]).cross(self.dir).unitize()?;
        let mut picked: Vec<FaceId> = Vec::new();
        for &f in body.face_list() {
            let Some(surface) = body.face(f)?.surface.as_ref() else {
                continue;
            };
            let is_plane = matches!(surface.kind(), SurfaceKind::Plane(_));
            let bead_face = match self.arc {
                Some(_) => !is_plane,
                // the chord's plane: square to the chord and to the edge (the other planes
                // hold the edge or are square to it)
                None => {
                    is_plane
                        && surface
                            .evaluate_normal(k, Point2::new(0.5, 0.5), Side::Left, Side::Left)?
                            .unitize()?
                            .dot(chord_normal)
                            .abs()
                            > 1.0 - 1e-9
                }
            };
            if bead_face {
                picked.push(f);
            }
        }
        let [face] = picked.as_slice() else {
            return Err(SolidError::invalid_input(
                OP,
                "a bead body has one bead face beside its planes",
            ));
        };
        let [a, b] = &self.supports;
        let round = ExactRound::chord(k, [a, b], self.bead.chamfer, None, self.inputs)?
            .with_sagitta(self.bead.sagitta)?;
        let surface = body
            .face_mut(*face)?
            .surface
            .as_mut()
            .ok_or_else(|| SolidError::invalid_input(OP, "a bead face with no surface"))?;
        round.recertify(k, surface)?;
        surface.set_exact_round(Some(round));
        Ok(())
    }

    /// The box past the plane through `at` square to the unit `normal`, on the side `normal`
    /// points to, `half` wide each way and `half` deep: big enough to hold every part of a
    /// span's body past the plane.
    fn cutter(k: &mut Kernel, at: Point3, normal: Vec3, half: f64) -> Result<Brep> {
        let [u, v] = square_to(normal)?;
        let corners = [
            at + u * half + v * half,
            at - u * half + v * half,
            at - u * half - v * half,
            at + u * half - v * half,
        ];
        let sides = corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .map(|(&a, &b)| create_line_segment(a, b).map(Curve::from))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        create_linear_sweep(k, &refs, normal, half, true)
    }

    /// One span `[t0, t1]` (distances along the edge) with `ends` at its start and its end.
    fn span(&self, k: &mut Kernel, t0: f64, t1: f64, ends: [BeadEnd; 2]) -> Result<Brep> {
        // the sphere of a rounded end, a little past the bead's reach so it never touches the
        // bead's faces tangentially
        let round = 1.05 * self.reach;
        let need = |end: BeadEnd| match end {
            BeadEnd::Square => 0.0,
            BeadEnd::Taper => self.height,
            BeadEnd::Round => round,
        };
        let [start, end] = ends;
        if t1 - t0 < need(start) + need(end) {
            return Err(SolidError::invalid_input(
                OP,
                "a span shorter than its two ends need (a bead height tapered, a bead reach rounded, at each)",
            ));
        }
        if ends == [BeadEnd::Square; 2] {
            return self.sweep(k, t0, t1);
        }
        // swept past each shaped end, so each cut crosses the bead's edges
        let past = 1.5 * self.reach.max(self.height) + 1.0;
        let beyond = |e: BeadEnd| if e == BeadEnd::Square { 0.0 } else { past };
        let mut body = self.sweep(k, t0 - beyond(start), t1 + beyond(end))?;
        let half = 4.0 * (self.reach + past) + 1.0;
        for (t, out, shape) in [(t0, -self.dir, start), (t1, self.dir, end)] {
            if shape == BeadEnd::Square {
                continue;
            }
            let tip = self.start + self.dir * t;
            let cutter = if shape == BeadEnd::Taper {
                let normal = (out + self.up).unitize()?;
                Self::cutter(k, tip, normal, half)?
            } else {
                let centre = tip - out * round;
                let outside = Self::cutter(k, centre, out, half)?;
                let sphere = create_sphere(k, centre, round)?;
                boolean_difference(k, outside, sphere)?
            };
            body = boolean_difference(k, body, cutter)?;
        }
        Ok(body)
    }
}

/// Refuses, before anything is built, an edge listed twice in `edges` (native invalid input)
/// and two of `edges` of `target` that meet at a vertex (a bead not built yet, naming the
/// vertex: their corner).
///
/// # Errors
///
/// Those named; those of the edges' vertices.
fn refuse_repeats_and_corners(target: &Brep, edges: &[EdgeId]) -> Result<()> {
    // an edge listed twice would be two beads in one place
    for (i, e) in edges.iter().enumerate() {
        if edges.get(..i).is_some_and(|before| before.contains(e)) {
            return Err(SolidError::invalid_input(OP, "each edge is listed once"));
        }
    }
    // beads meeting at a vertex need a corner piece
    let mut seen: std::collections::BTreeMap<VertexId, EdgeId> = std::collections::BTreeMap::new();
    for &e in edges {
        let (start, end) = edge::start_end_vertices(target, e)?;
        for v in [start, end].into_iter().flatten() {
            if seen.insert(v, e).is_some_and(|other| other != e) {
                return Err(not_built(
                    WeldGap::BeadsMeetAtVertex,
                    Site::vertex(TargetVertex::of_target(v)),
                ));
            }
        }
    }
    Ok(())
}

/// The weld beads along `edges` of `target`, each as a body of its own built from its own
/// faces, in the order of `edges`; `target` is left as it was, whether the beads are built or
/// refused. Each edge is straight between two planes in an inside corner; `sizes` are one per
/// edge (the chamfer's setbacks, the bead's toes), `sagitta` is the arc's height over the
/// chord (`> 0` convex, `< 0` concave, `0` flat, [`BeadDefinition`]), and the first distance of
/// edge i's size runs along face `along[i]` when given (one of that edge's two faces; `along`
/// is one per edge, or empty for none). A convex bead
/// overhanging its toe is built: no trimming is involved.
///
/// `target` is mutable only for evaluating its edges' faces (a missing trim curve may be
/// cached); its geometry and topology are left as they were.
///
/// # Errors
///
/// Native invalid input for sizes not one per edge, `along` neither empty nor one per edge, an
/// `along` face not on its edge, an edge listed twice and an edge not in an inside corner (an
/// outside or a tangent edge); [`BlendFailure::SizeBelowResolution`]
/// naming the edge for a leg below the smallest size a fillet builds on `target` (eight
/// times its tolerance), whether given or computed from an angle;
/// [`NoRoomReason::BeadCutsFace`] for a bead past its bound ([`BeadDefinition::new`]), with the
/// smallest sagitta that fits over all of `edges` and naming that edge; each edge's legs and
/// toes are checked before its sagitta and their refusal on any edge comes first, so a retry
/// at that sagitta is not refused for a leg; and
/// [`NoRoomReason::LegPastFace`] with the leg that fits, naming the face, for a toe past its
/// face's trims; [`BlendFailure::WeldNotBuilt`] for two of `edges` that meet at a
/// vertex ([`WeldGap::BeadsMeetAtVertex`], naming it), an edge that is not straight between
/// two planes ([`WeldGap::CurvedJoint`]) or legs with an obtuse toe ([`WeldGap::ObtuseToe`]),
/// naming the edge; those of the evaluations and the sweeps. Every refusal comes before any
/// bead is built.
pub fn beads_apart(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[Option<FaceId>],
) -> Result<Vec<Brep>> {
    if sizes.len() != edges.len() {
        return Err(SolidError::invalid_input(OP, "one size per edge"));
    }
    if !along.is_empty() && along.len() != edges.len() {
        return Err(SolidError::invalid_input(OP, "one along per edge, or none"));
    }
    refuse_repeats_and_corners(target, edges)?;
    // every frame first, so a refused edge leaves nothing built; a sagitta too large is
    // refused with the smallest fit over all edges, and only once no edge is refused for its
    // legs or toes (each frame checks them before its sagitta), so a retry at it builds on
    // every one
    let sagitta_fit = |e: &SolidError| {
        e.fit()
            .filter(|f| matches!(f.measure, FitMeasure::Sagitta))
            .map(|f| f.size)
    };
    let mut frames = Vec::new();
    let mut refusal: Option<(f64, SolidError)> = None;
    for (i, (&e, &size)) in edges.iter().zip(sizes).enumerate() {
        let along = along.get(i).copied().flatten();
        match frame(k, target, e, size, sagitta, along) {
            Ok(f) => frames.push(f),
            Err(err) => match sagitta_fit(&err) {
                Some(fit) if refusal.as_ref().is_none_or(|(kept, _)| fit < *kept) => {
                    refusal = Some((fit, err));
                }
                Some(_) => {}
                None => return Err(err),
            },
        }
    }
    if let Some((_, err)) = refusal {
        return Err(err);
    }
    frames.iter().map(|f| f.sweep(k, 0.0, f.length)).collect()
}

/// The weld bead along `edges` of `target` as one body of its own, built on the foot blend
/// (#15129, [`crate::blend::foot_blend`]): along each edge, at every station, each toe lies on
/// its face at its leg's distance from the root, measured straight in the plane square to the
/// edge (what a leg gauge measures), on flat and curved faces alike, with the bead's section
/// (the chord, or the arc at the sagitta) between the toes. Edges picked together make one
/// body (a lump per bead or span where they do not touch). The one entry point of the welds:
/// each section face the foot blend builds keeps its definition with the weld's spans and end
/// shapes ([`crate::blend::foot_blend::FootBlendDefinition`]).
///
/// `sizes` are one per edge, as the two legs ([`ChamferSize::Distances`]: support 1's,
/// support 2's), `sagitta` is the arc's height over the chord (`> 0` convex, `< 0` concave,
/// `0` flat), and edge i's first leg runs along face `along[i]` when given (one of that edge's
/// two faces; `along` is one per edge, or empty for none). A convex bead overhanging its toe
/// is built: nothing is trimmed. `spans` are where the
/// bead runs along each edge, fractions of the edge's length from the start of its curve's
/// interval ([`BeadSpan`]; none: the whole edge), and `ends` the shape at each span's start
/// and end: square on any joint; tapered or rounded only straight between two planes with a
/// sagitta so far, where the bead is the planar sweep ([`beads_apart`]'s) cut at its ends, its
/// bead face keeping the chord's exact definition with its sagitta.
///
/// A weld never changes the part it lies on: `target`'s geometry and topology are left as
/// they were whether the bead is built or refused (it is mutable for the edges' binormals,
/// asked in 3D only, and for the classification of the toes on their faces, which builds
/// nothing on it).
///
/// # Errors
///
/// Native invalid input for no edge, sizes not one per edge, `along` neither empty nor one per
/// edge, an `along` face not on its edge, an edge listed twice, a size that is not two legs,
/// or spans out of order, overlapping or too short for their ends;
/// [`crate::error::blend::BlendFailure::WeldNotBuilt`]
/// naming the vertex for two of `edges` that meet at one, and for a shaped end the foot blend
/// does not build; the foot blend's refusals, typed and with the size that builds (a toe off
/// its face: checked on every edge before anything that depends on the sagitta; no toe at
/// the leg, found while a bead is built; an arc that would cut into a face: the smallest
/// sagitta that fits over all of `edges`, naming that edge, so a retry at it is not refused
/// for the sagitta or for a toe off its face on any edge); those of the closing.
#[allow(clippy::too_many_arguments)]
pub fn bead_body(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[Option<FaceId>],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
) -> Result<Brep> {
    use crate::blend::foot_blend::{close_bead_body, BeadWeld, FootBlend, WELD_FIT_TOLERANCE};
    if edges.is_empty() || sizes.len() != edges.len() {
        return Err(SolidError::invalid_input(
            OP,
            "a weld bead takes edges and one size per edge",
        ));
    }
    if !along.is_empty() && along.len() != edges.len() {
        return Err(SolidError::invalid_input(OP, "one along per edge, or none"));
    }
    refuse_repeats_and_corners(target, edges)?;
    let spans = if spans.is_empty() {
        vec![BeadSpan::new(0.0, 1.0)?]
    } else {
        BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?
    };
    let shaped = ends.iter().any(|e| *e != BeadEnd::Square);
    let tol = WELD_FIT_TOLERANCE;
    let mut body = Brep::new_with_infinite_region()?;
    body.set_editing_enabled(true);
    let mut defined = Vec::new();
    let mut apart: Vec<Brep> = Vec::new();
    // the legs before the sagitta, on every joint: a toe off its face does not depend on the
    // sagitta, so a retry at the sagitta that fits is not refused for one
    let mut joints = Vec::new();
    for (i, (&e, size)) in edges.iter().zip(sizes).enumerate() {
        let ChamferSize::Distances(legs) = *size else {
            return Err(SolidError::invalid_input(
                OP,
                "a weld bead's size is its two legs",
            ));
        };
        let first = along.get(i).copied().flatten();
        if let Some(f) = first {
            if !edge_faces(target, e)?.contains(&f) {
                return Err(SolidError::invalid_input(
                    OP,
                    "a bead's first leg runs along one of its edge's two faces",
                ));
            }
        }
        let blend = FootBlend::of_edge(k, target, e, first)?;
        let params = span_parameters(k, &blend, &spans)?;
        blend.toes_on_faces(k, target, legs, &params)?;
        joints.push((e, *size, legs, first, blend, params));
    }
    for (i, (e, size, legs, first, blend, params)) in joints.into_iter().enumerate() {
        if shaped {
            // the shaped ends: the planar sweep's, cut at its ends
            let gap = if !between_planes(target, e)? {
                Some(WeldGap::EndShapeOnCurvedJoint)
            } else if sagitta == 0.0 {
                Some(WeldGap::EndShapeOfFlatBead)
            } else {
                None
            };
            if let Some(gap) = gap {
                return Err(not_built(gap, Site::edge(TargetEdge::of_target(e))));
            }
            let f = match frame(k, target, e, size, sagitta, first) {
                Ok(f) => f,
                Err(refused) => {
                    return Err(smallest_sagitta_fit(
                        k, target, edges, sizes, along, i, &spans, sagitta, refused,
                    ))
                }
            };
            for s in &spans {
                apart.push(f.span(k, s.from * f.length, s.to * f.length, ends)?);
            }
            continue;
        }
        let mut made = Vec::new();
        for &p in &params {
            match blend.add_span(k, &mut body, legs, sagitta, tol, p) {
                Ok(faces) => made.extend(faces),
                // a refusal's size over all of the joint's spans, not the failed one's alone,
                // and a sagitta's over the joints still to come
                Err(refused) => {
                    let refused = blend.fit_over(k, legs, sagitta, &params, refused)?;
                    let refused = no_leg_below_resolution(target, refused);
                    return Err(smallest_sagitta_fit(
                        k, target, edges, sizes, along, i, &spans, sagitta, refused,
                    ));
                }
            }
        }
        let weld = BeadWeld {
            legs,
            sagitta,
            spans: params,
            ends,
            tol,
        };
        defined.push((blend, made, weld));
    }
    if !defined.is_empty() {
        close_bead_body(k, &mut body, tol)?;
        for (blend, made, weld) in &defined {
            blend.define_faces(&mut body, made, weld)?;
        }
    }
    for piece in apart {
        body = if body.face_list().is_empty() {
            piece
        } else {
            crate::boolean::api_brep::boolean_union(k, body, piece)?
        };
    }
    Ok(body)
}

/// Whether `edge_id` of `target` is straight between two planes.
fn between_planes(target: &Brep, edge_id: EdgeId) -> Result<bool> {
    let straight = target
        .edge(edge_id)?
        .curve
        .as_ref()
        .is_some_and(|c| matches!(c.kind(), CurveKind::Line(_)));
    let mut planes = true;
    for f in edge_faces(target, edge_id)? {
        planes &= target
            .face(f)?
            .surface
            .as_ref()
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
    }
    Ok(straight && planes)
}

/// `refused` with a leg that fits below the smallest `target` builds
/// ([`is_radius_supported`]: eight times its tolerance) made 0: a retry at such a leg is
/// refused for its size before the toes are read, so no leg on that side builds.
fn no_leg_below_resolution(target: &Brep, refused: SolidError) -> SolidError {
    let mut failure = match refused {
        SolidError::Failed(failure) => failure,
        other => return other,
    };
    if let Some(fit) = failure.fit {
        if matches!(fit.measure, FitMeasure::Leg)
            && !is_radius_supported(fit.size, target.tolerance())
        {
            failure.fit = Some(Fit::new(0.0, FitMeasure::Leg, fit.edge));
        }
    }
    SolidError::Failed(failure)
}

/// `refused`, the refusal of joint `at` of `edges` for a sagitta too large, with the smallest
/// sagitta that fits over the joints after it too, naming the edge whose fit it is, so a retry
/// at it is not refused for the sagitta on any joint (those before `at` were built at the
/// sagitta asked, and every joint's toes were found on their faces before). Each of
/// those joints' fits is its foot blend's over its stations
/// ([`crate::blend::foot_blend::FootBlend::sagitta_that_fits`]); one refused for another
/// reason is left to its own refusal at the retry. Any other error is returned as it is.
#[allow(clippy::too_many_arguments)]
fn smallest_sagitta_fit(
    k: &Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    along: &[Option<FaceId>],
    at: usize,
    spans: &[BeadSpan],
    sagitta: f64,
    refused: SolidError,
) -> SolidError {
    use crate::blend::foot_blend::FootBlend;
    let mut failure = match refused {
        SolidError::Failed(failure) => failure,
        other => return other,
    };
    let Some(mut fit) = failure
        .fit
        .filter(|f| matches!(f.measure, FitMeasure::Sagitta))
    else {
        return SolidError::Failed(failure);
    };
    for (j, (&e, size)) in edges.iter().zip(sizes).enumerate().skip(at + 1) {
        let ChamferSize::Distances(legs) = *size else {
            continue;
        };
        let first = along.get(j).copied().flatten();
        let Ok(blend) = FootBlend::of_edge(k, target, e, first) else {
            continue;
        };
        let Ok(params) = span_parameters(k, &blend, spans) else {
            continue;
        };
        let Ok(size) = blend.sagitta_that_fits(k, legs, sagitta, &params) else {
            continue;
        };
        if size < fit.size {
            let edge = Some(TargetEdge::of_target(e));
            fit = Fit::new(size, FitMeasure::Sagitta, edge);
            failure.site = Site {
                edge,
                ..Site::default()
            };
        }
    }
    failure.fit = Some(fit);
    SolidError::Failed(failure)
}

/// `spans` (fractions of the joint's length from the start of its interval) as the foot
/// blend's leader parameters, by arc length; the ends of the interval exactly.
///
/// # Errors
///
/// A failure where a parameter at a length is not found; those of the lengths.
fn span_parameters(
    k: &Kernel,
    blend: &crate::blend::foot_blend::FootBlend,
    spans: &[BeadSpan],
) -> Result<Vec<[f64; 2]>> {
    let (lo, hi) = (blend.interval.min(), blend.interval.max());
    let curve = Curve::from(blend.leader.clone());
    let length = curve.length(k, &blend.interval, LENGTH_ACCURACY)?;
    let at = |share: f64| -> Result<f64> {
        if share <= 0.0 {
            return Ok(lo);
        }
        if share >= 1.0 {
            return Ok(hi);
        }
        let found = curve.find_parameter_at_arc_length(k, lo, share * length)?;
        if !found.converged || found.past_end {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the arc-length search along the edge's curve did not converge or ran past its end for a share of the edge's own length",
                },
            ));
        }
        Ok(found.end_param.max(lo).min(hi))
    };
    spans.iter().map(|s| Ok([at(s.from)?, at(s.to)?])).collect()
}

/// The faces on `edge_id` of `target`, through its edge uses.
///
/// # Errors
///
/// Those of the edge uses.
fn edge_faces(target: &Brep, edge_id: EdgeId) -> Result<Vec<FaceId>> {
    let mut faces = Vec::new();
    for eu in edge::edgeuses(target, edge_id, None, None)? {
        if let Some(f) = crate::topology::edgeuse::p1::face(target, eu)? {
            if !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
}

/// Two unit vectors square to the unit `dir` and to each other, `u x v = dir`.
fn square_to(dir: Vec3) -> Result<[Vec3; 2]> {
    let pick = if dir.x.abs() <= dir.y.abs() && dir.x.abs() <= dir.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if dir.y.abs() <= dir.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let u = pick.cross(dir).unitize()?;
    let v = dir.cross(u);
    Ok([u, v])
}

/// The length of the bead along `edge_id` of `target` in each of `spans`, in their order:
/// the edge's arc length times the span's share (spans are fractions of that length). For a
/// weld table; a bead's volume is its body's ([`crate::mass::integrator::properties::brep_volume`] of a body from
/// [`beads_apart`] or [`bead_body`]).
///
/// # Errors
///
/// Native invalid input for spans out of order or overlapping ([`BeadSpan`]) or an edge with
/// no curve; those of the edge's arc length.
pub fn span_lengths(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    spans: &[BeadSpan],
) -> Result<Vec<f64>> {
    let spans = BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?;
    let curve = target
        .edge(edge_id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    let length = curve.length(k, &edge::interval(target, edge_id)?, LENGTH_ACCURACY)?;
    Ok(spans.iter().map(|s| length * s.share()).collect())
}

/// The relative accuracy [`span_lengths`] asks of an edge's arc length (a line's and a
/// circle's are exact whatever is asked).
const LENGTH_ACCURACY: f64 = 1e-12;
