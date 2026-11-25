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
//! A span ends square to the edge, or tapers, or is rounded ([`BeadEnd`]), on any joint: a
//! leg law over the end's zone inside the span ([`crate::blend::foot_blend::FootRamp`]).
//!
//! Refused before anything is built, the part left as it was, as
//! [`BlendFailure::WeldNotBuilt`] (class `Unsupported`) naming the edge or vertex: a joint
//! that is not a straight edge between two planes for the exact bead ([`beads_apart`],
//! [`WeldGap::CurvedJoint`]), a corner of beads at a vertex not built yet
//! ([`WeldGap::BeadsMeetAtVertex`]) and a bead with an obtuse toe ([`WeldGap::ObtuseToe`]).
//! As too large (class `FilletTooLarge`), naming the size that builds: a toe past its face's trims ([`NoRoomReason::LegPastFace`], the leg) and a sagitta
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
    let arc = (sagitta != 0.0)
        .then(|| BeadArc::new(start + feet[0], start + feet[1], dir, up_raw, sagitta))
        .transpose()?;
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
    let mut frames = Vec::new();
    let mut refusal: Option<(f64, SolidError)> = None;
    for (i, (&e, &size)) in edges.iter().zip(sizes).enumerate() {
        let along = along.get(i).copied().flatten();
        match frame(k, target, e, size, sagitta, along) {
            Ok(f) => frames.push(f),
            Err(err) => match sagitta_fit_of(&err) {
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

/// The sagitta that fits, where `refused` is a refusal with one.
fn sagitta_fit_of(refused: &SolidError) -> Option<f64> {
    refused
        .fit()
        .filter(|f| matches!(f.measure, FitMeasure::Sagitta))
        .map(|f| f.size)
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
/// and end, on any joint: square (a planar cap), or tapered or rounded, a leg law over the
/// end's zone inside the span (the section scaled down to the root at the tip:
/// [`crate::blend::foot_blend::FootRamp`]; a taper's zone as long as the bead is high, a
/// rounded end's one bead reach). Nothing is cut.
///
/// Beads whose edges meet at a vertex are joined there (#17916): at an inside corner as they
/// are, below 90 degrees each cut at the other's wall first; round an outside corner by the
/// first bead's end section turned to the second's (a wrap-around); where two edges run on
/// smoothly (a tangent line and arc) as they are, with no piece. Beads that meet no other
/// are built together in one body, as without corners. See `generic-blend.md` §2.5, Corners.
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
/// [`BlendFailure::SizeBelowResolution`] naming the edge for a leg below the smallest size a
/// fillet builds on `target`, with square ends as with shaped ones;
/// [`crate::error::blend::BlendFailure::WeldNotBuilt`]
/// naming the vertex where `edges` meet in a corner not built yet: spans or a shaped end
/// there, an edge with both ends there, a third bead or different legs round an outside
/// corner, an outside corner whose floor (the face both beads lie on) is not a plane or whose
/// walls are not square to it within the weld's fit tolerance, an inside corner below 90
/// degrees whose walls are not both planes, beads not on one floor, and a bead so short that
/// it lies wholly behind the other's wall; the foot blend's refusals, typed and with the size
/// that builds (a toe off its face: checked on every edge built together, with the legs'
/// sizes, before anything that depends on the sagitta; no toe at the leg, found while a bead
/// is built; an arc that would cut into a face: the smallest sagitta that fits over the edges
/// built together, naming that edge, so a retry at it is not refused for a leg or a toe off
/// its face on any of them. It is read at each joint's check stations, so a retry can be
/// refused again with a smaller one); those of the closing.
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
    use crate::error::blend::{BlendFailure, WeldGap};
    use crate::error::{Failure, Site, TargetVertex};
    if edges.is_empty() || sizes.len() != edges.len() {
        return Err(SolidError::invalid_input(
            OP,
            "a weld bead takes edges and one size per edge",
        ));
    }
    if !along.is_empty() && along.len() != edges.len() {
        return Err(SolidError::invalid_input(OP, "one along per edge, or none"));
    }
    // An edge listed twice would be two beads in one place. Check this before collecting
    // corners: a closed edge has the same vertex at both ends, but is still one bead.
    for (i, e) in edges.iter().enumerate() {
        if edges.get(..i).is_some_and(|before| before.contains(e)) {
            return Err(SolidError::invalid_input(OP, "each edge is listed once"));
        }
    }
    // beads meeting at a vertex: built one by one and joined at their corners
    let mut seen: std::collections::BTreeMap<VertexId, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (i, &e) in edges.iter().enumerate() {
        let (start, end) = edge::start_end_vertices(target, e)?;
        let mut ends_of = vec![start, end];
        ends_of.dedup();
        for v in ends_of.into_iter().flatten() {
            seen.entry(v).or_default().push(i);
        }
    }
    seen.retain(|_, at| at.len() > 1);
    if !seen.is_empty() {
        let refuse = |v: VertexId| {
            Failure::new(OP, BlendFailure::WeldNotBuilt(WeldGap::BeadsMeetAtVertex))
                .or_at(Site::vertex(TargetVertex::of_target(v)))
                .into_error()
        };
        return corner_beads(
            k, target, edges, sizes, sagitta, along, spans, ends, &seen, &refuse,
        );
    }
    beads_together(k, target, edges, sizes, sagitta, along, spans, ends, &[])
}

/// The beads along `edges` in one body, none of them meeting at a vertex ([`bead_body`]);
/// `behind` per edge the blocks a neighbour's wall cuts from it afterwards ([`WallCut`]):
/// a toe inside one of them may leave its face, as the cut removes it.
///
/// # Errors
///
/// Those of [`bead_body`].
#[allow(clippy::too_many_arguments)]
fn beads_together(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[Option<FaceId>],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
    behind: &[Vec<WallCut>],
) -> Result<Brep> {
    use crate::blend::foot_blend::{close_bead_body, BeadWeld, FootBlend, WELD_FIT_TOLERANCE};
    let spans = if spans.is_empty() {
        vec![BeadSpan::new(0.0, 1.0)?]
    } else {
        BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?
    };
    let tol = WELD_FIT_TOLERANCE;
    let mut body = Brep::new_with_infinite_region()?;
    body.set_editing_enabled(true);
    let mut defined = Vec::new();
    // what does not depend on the sagitta first, on every joint: a leg below the body's
    // resolution (as the planar bead's frame refuses it: built, such a leg is a sliver, or
    // fails in the skin) and a toe off its face. So a retry at the sagitta that fits is not
    // refused for one of them
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
        setback_below_resolution(OP, e, legs, target.tolerance())
            .map_err(|refused| refused.or_at(Site::edge(TargetEdge::of_target(e))))?;
        let cuts = behind.get(i).map_or(&[][..], Vec::as_slice);
        // a toe at the wall's own foot lies on the wall's plane to rounding only
        let slack = 1e-9 * (1.0 + legs[0].max(legs[1]));
        let params = span_parameters(k, &blend, &spans)?;
        blend.toes_on_faces_except(k, target, legs, &params, &|p: Point3| {
            cuts.iter().any(|c| c.holds(p, slack))
        })?;
        joints.push((legs, blend, params));
    }
    for (i, (legs, blend, params)) in joints.into_iter().enumerate() {
        let mut made = Vec::new();
        for &p in &params {
            match blend.add_span(k, &mut body, legs, sagitta, tol, p, ends) {
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
    close_bead_body(k, &mut body, tol)?;
    for (blend, made, weld) in &defined {
        blend.define_faces(&mut body, made, weld)?;
    }
    Ok(body)
}

/// `refused` with a leg that fits below the smallest `target` builds
/// ([`is_radius_supported`]: eight times its tolerance) made 0: a retry at such a leg is
/// refused for its size before anything else is read ([`bead_body`]'s first pass, as the
/// planar bead's frame), so no leg on that side builds.
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
/// sagitta that fits over the joints after it too, naming the edge whose fit it is (the site is
/// then that edge alone: a joint not yet built has no station that failed). Those before `at`
/// were built at the sagitta asked, and every joint's legs and toes were checked before. Each
/// of those joints' fits is its foot blend's over its check stations
/// ([`crate::blend::foot_blend::FootBlend::sagitta_that_fits`]): sampled, where the build
/// reads every station it makes, so a retry at the fit can be refused again, with a smaller
/// one, where a joint's least bound lies between two of its check stations. A joint refused
/// for another reason is left to its own refusal at the retry; one that fails otherwise (a
/// defect) is returned in place of `refused`, whose fit would not have looked at it. Any other
/// `refused` is returned as it is.
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
        let size = FootBlend::of_edge(k, target, e, first).and_then(|blend| {
            let params = span_parameters(k, &blend, spans)?;
            blend.sagitta_that_fits(k, legs, sagitta, &params)
        });
        let size = match size {
            Ok(size) => size,
            Err(other) if other.is_refusal() => continue,
            Err(other) => return other,
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

/// A bead's end at a corner: its foot blend, the leader's parameter there, the station there,
/// and the leader's unit direction away from the vertex.
struct CornerEnd {
    blend: crate::blend::foot_blend::FootBlend,
    t: f64,
    station: crate::blend::foot_blend::FootStation,
    away: Vec3,
}

impl CornerEnd {
    /// Which support of this bead is on `face`.
    fn side_on(&self, face: FaceId) -> Option<usize> {
        self.blend.sides.iter().position(|s| s.face == Some(face))
    }

    /// The unit direction from the root to the foot on support `i`.
    fn toward(&self, i: usize) -> Result<Vec3> {
        let foot = self.station.feet.get(i).copied().ok_or_else(|| defect("a corner end's station had no foot on one of its two supports, though every station is built with both"))?;
        (foot - self.station.root).unitize()
    }

    /// Whether support `i` is a plane.
    fn planar(&self, i: usize) -> bool {
        self.blend
            .sides
            .get(i)
            .is_some_and(|s| matches!(s.surface.kind(), SurfaceKind::Plane(_)))
    }
}

/// A failure of the bead corners that is the kernel's fault: `why` names the step and why
/// (the docs' failure-codes §6).
fn defect(why: &'static str) -> SolidError {
    SolidError::blend(OP, BlendFailure::Defect { why })
}

/// A cut at an inside corner below 90 degrees: the block behind a neighbour's wall that is
/// taken from a bead ([`wall_block`]), the square of side `4 reach` in the wall's plane
/// through `o` (unit normal `n` into the wall), centred on `o` along `x` and `y`, swept
/// `reach` along `n`. Its toes inside the block are excused from the toe check
/// ([`WallCut::holds`]): just what the cut removes, no more.
#[derive(Debug, Clone, Copy)]
struct WallCut {
    o: Point3,
    n: Vec3,
    x: Vec3,
    y: Vec3,
    reach: f64,
}

impl WallCut {
    /// The block behind the plane through `o` with unit normal `n`, of size `reach`.
    ///
    /// # Errors
    ///
    /// Those of the frame.
    fn new(o: Point3, n: Vec3, reach: f64) -> Result<Self> {
        let [x, y] = square_to(n)?;
        Ok(Self { o, n, x, y, reach })
    }

    /// Whether `p` lies in the block, to `slack`.
    fn holds(&self, p: Point3, slack: f64) -> bool {
        let d = p - self.o;
        let (depth, side) = (d.dot(self.n), 2.0 * self.reach + slack);
        depth > -slack
            && depth <= self.reach + slack
            && d.dot(self.x).abs() <= side
            && d.dot(self.y).abs() <= side
    }
}

/// The beads along `edges` where some meet at the vertices of `corners` (each with the
/// indices of its edges, two or more): the beads that meet no other in one body
/// ([`beads_together`]), and each bead at a corner alone, an inside corner's beads joined as
/// they are (below 90° each cut at the other's wall first, its toes in the cut excused), an
/// outside corner closed by the first bead's end section turned about the floor's normal to
/// the second's (a wrap-around), and two edges running on smoothly joined as they are. What a
/// corner needs:
/// - every corner: square ends there, no spans, each edge with one end at the vertex, and the
///   beads on one face (the floor), each with its other face (its wall);
/// - an outside corner: two beads of the same legs on each face, the floor a plane (its
///   normal the turning axis), each wall square to the floor where the bead meets it, to the
///   weld's fit tolerance over the wall's leg (the turned section's gap to the second's);
/// - an inside corner below 90°: both walls planes (each bead is cut in the other's plane),
///   and each bead longer than its part behind the other's wall.
///
/// Everything else at a corner is refused by `refuse`.
///
/// # Errors
///
/// `refuse`'s for a corner not built yet; those of [`bead_body`] and of the booleans.
#[allow(clippy::too_many_arguments)]
fn corner_beads(
    k: &mut Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[Option<FaceId>],
    spans: &[BeadSpan],
    ends: [BeadEnd; 2],
    corners: &std::collections::BTreeMap<VertexId, Vec<usize>>,
    refuse: &dyn Fn(VertexId) -> SolidError,
) -> Result<Brep> {
    use crate::blend::foot_blend::{normal_at, FootBlend, WELD_FIT_TOLERANCE};
    let mut cuts: Vec<Vec<WallCut>> = vec![Vec::new(); edges.len()];
    let mut pieces: Vec<Brep> = Vec::new();
    for (&v, at) in corners {
        let mut wraps = Vec::new();
        if !spans.is_empty() {
            return Err(refuse(v));
        }
        // Each bead's end at this topological vertex, from the edge's own vertices: two edges
        // can share both vertices (a D-shaped loop), where distances between bead ends cannot
        // tell the corners apart.
        let vertex = target.vertex(v)?.point;
        let mut corner = Vec::new();
        for &i in at {
            let (Some(&e), Some(size)) = (edges.get(i), sizes.get(i)) else {
                return Err(defect("a corner listed an edge index with no edge or no size, though its indices come from the edges given"));
            };
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
            let (start, end) = edge::start_end_vertices(target, e)?;
            let at_start = match (start == Some(v), end == Some(v)) {
                (true, false) => true,
                (false, true) => false,
                // a closed edge has both ends at this vertex: two bead ends, which this
                // corner construction does not represent yet
                _ => return Err(refuse(v)),
            };
            // a square end only: ends[0] at the start of the span, ends[1] at its end
            if !matches!(if at_start { ends[0] } else { ends[1] }, BeadEnd::Square) {
                return Err(refuse(v));
            }
            let length =
                Curve::from(blend.leader.clone()).length(k, &blend.interval, LENGTH_ACCURACY)?;
            let t = if at_start {
                blend.interval.min()
            } else {
                blend.interval.max()
            };
            let station = blend.station(k, t, legs, None)?;
            let away = if at_start {
                station.tangent
            } else {
                -station.tangent
            };
            corner.push((
                CornerEnd {
                    blend,
                    t,
                    station,
                    away,
                },
                legs,
                length,
            ));
        }
        // the unit normal of support `i` of a corner's bead end at `uv`
        let normal = |k: &Kernel, e: &CornerEnd, i: usize, uv: Point2| -> Result<Vec3> {
            let side = e.blend.sides.get(i).ok_or_else(|| {
                defect("a corner's bead end had no support of an index its own faces gave")
            })?;
            normal_at(k, &side.surface, uv)?
                .ok_or_else(|| {
                    defect("a support's surface had no normal at a foot just solved on it")
                })?
                .unitize()
        };
        let pairs: Vec<(usize, usize)> = (0..corner.len())
            .flat_map(|a| ((a + 1)..corner.len()).map(move |b| (a, b)))
            .collect();
        for (a, b) in pairs {
            let (Some((ea, legs_a, _)), Some((eb, legs_b, _))) = (corner.get(a), corner.get(b))
            else {
                return Err(defect("a pair of a corner's beads named an end the corner does not hold, though the pairs are built from its ends"));
            };
            // Two edges running on smoothly (a tangent line and arc, or two collinear pieces)
            // already share one square end: joined as they are, with no piece. Edges leaving
            // the vertex the same way overlap, and are refused. Taken first: collinear pieces
            // share both their faces, which the corner below refuses.
            let sine = ea.away.cross(eb.away).length();
            if sine <= EFF_ZERO_RAD {
                if ea.away.dot(eb.away) < 0.0 {
                    continue;
                }
                return Err(refuse(v));
            }
            // the face both beads lie on (the floor), and each bead's other face (its wall)
            let faces_a: Vec<FaceId> = ea.blend.sides.iter().filter_map(|s| s.face).collect();
            let common: Vec<FaceId> = eb
                .blend
                .sides
                .iter()
                .filter_map(|s| s.face)
                .filter(|f| faces_a.contains(f))
                .collect();
            let [face] = common[..] else {
                return Err(refuse(v));
            };
            let (Some(ja), Some(jb)) = (ea.side_on(face), eb.side_on(face)) else {
                return Err(refuse(v));
            };
            let (wa, wb) = (1 - ja, 1 - jb);
            let (ua, ub) = (ea.toward(ja)?, eb.toward(jb)?);
            let (wa_dir, wb_dir) = (ea.toward(wa)?, eb.toward(wb)?);
            // Inside or outside, and below 90 degrees, by signs: on the floor `eb.away` leaves
            // the vertex at `sine` off `ea.away`'s line, so `eb.away . ua` is that sine, to the
            // feet's accuracy, past the angle zero (the smooth case is taken out above); at 90
            // degrees exactly the cut and the plain join are the same body.
            let inside_ab = eb.away.dot(ua) > 0.0;
            let inside_ba = ea.away.dot(ub) > 0.0;
            if inside_ab != inside_ba {
                return Err(refuse(v));
            }
            let (ia, ib) = (at.get(a).copied(), at.get(b).copied());
            let (Some(ia), Some(ib)) = (ia, ib) else {
                return Err(defect("a pair of a corner's beads had no index in the corner's list, though the pairs are built from it"));
            };
            if inside_ab {
                // below 90 degrees each bead runs into the other's wall: cut it there
                // (90 degrees to rounding is the plain join, the same body)
                if ea.away.dot(eb.away) > EFF_ZERO_RAD {
                    if !(ea.planar(wa) && eb.planar(wb)) {
                        return Err(refuse(v));
                    }
                    // each bead is cut by a block sized from its own legs: its part behind
                    // the other's wall runs at most `lead / sine` along it from the vertex
                    for (me, cut_end, wall_end, w, u) in [
                        (ia, (legs_a, a), eb, wb_dir, ub),
                        (ib, (legs_b, b), ea, wa_dir, ua),
                    ] {
                        let (legs, own) = cut_end;
                        let length = corner.get(own).map_or(0.0, |c| c.2);
                        let lead = legs[0].max(legs[1]) + sagitta.abs();
                        if lead / sine >= length {
                            return Err(refuse(v));
                        }
                        let mut n = wall_end.away.cross(w).unitize()?;
                        if n.dot(u) > 0.0 {
                            n = -n;
                        }
                        if let Some(c) = cuts.get_mut(me) {
                            c.push(WallCut::new(vertex, n, 4.0 * lead / sine)?);
                        }
                    }
                }
            } else {
                // an outside corner: two beads of the same legs, a planar floor
                if corner.len() != 2 || !ea.planar(ja) || !eb.planar(jb) {
                    return Err(refuse(v));
                }
                if legs_a.get(ja) != legs_b.get(jb) || legs_a.get(wa) != legs_b.get(wb) {
                    return Err(refuse(v));
                }
                // the floor's normal, exact for a plane, is the turning axis; turning `ua` to
                // `ub` about it, positive about `ua x ub`
                let root_uv = ea.station.root_uv.get(ja).copied().ok_or_else(|| {
                    defect("a corner's bead end had no root parameters on its floor, though every station is built with both")
                })?;
                let axis = normal(k, ea, ja, root_uv)?;
                let ax = if ua.cross(ub).dot(axis) >= 0.0 {
                    axis
                } else {
                    -axis
                };
                // each wall square to the floor where the bead meets it: a wall `delta` off
                // square leaves the turned section's wall foot `leg sin(delta)` off the
                // second's, for each wall; built where that stays within the weld's fit
                // tolerance, as every other rung of a bead
                let off = |e: &CornerEnd, w: usize| -> Result<f64> {
                    let uv = e.station.uv.get(w).copied().ok_or_else(|| {
                        defect("a corner's bead end had no foot on its wall, though every station is built with both")
                    })?;
                    Ok(normal(k, e, w, uv)?.dot(axis).abs())
                };
                let wall_leg = legs_a.get(wa).copied().unwrap_or(0.0);
                if wall_leg * (off(ea, wa)? + off(eb, wb)?) > WELD_FIT_TOLERANCE {
                    return Err(refuse(v));
                }
                let angle = ua.dot(ub).clamp(-1.0, 1.0).acos();
                wraps.push((a, ax, angle));
            }
        }
        // the outside corners' pieces: the first bead's end section turned to the second's
        for (a, ax, angle) in wraps {
            let Some((ea, legs, _)) = corner.get(a) else {
                return Err(defect("an outside corner's wrap named an end the corner does not hold, though the wraps are built from its ends"));
            };
            let (_, _, section) = ea.blend.cut_curves(k, ea.t, *legs, sagitta)?;
            let s = &ea.station;
            let [f0, f1] = s.feet;
            let r0 = Curve::from(create_line_segment(s.root, f0)?);
            let r1 = Curve::from(create_line_segment(f1, s.root)?);
            let arc = Curve::from(section);
            pieces.push(crate::sweep::api_brep::create_rotational_sweep(
                k,
                &[Some(&r0), Some(&arc), Some(&r1)],
                s.root,
                ax,
                angle.to_degrees(),
                true,
            )?);
        }
    }
    // the beads that meet no other in one body, as without corners
    let at_corner: Vec<usize> = corners.values().flatten().copied().collect();
    // each edge's `along` goes with it
    let along_of = |i: usize| along.get(i).copied().flatten();
    let (mut free_edges, mut free_sizes, mut free_along) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (&e, &size)) in edges.iter().zip(sizes).enumerate() {
        if !at_corner.contains(&i) {
            free_edges.push(e);
            free_sizes.push(size);
            free_along.push(along_of(i));
        }
    }
    let mut joined: Option<Brep> = if free_edges.is_empty() {
        None
    } else {
        Some(beads_together(
            k,
            target,
            &free_edges,
            &free_sizes,
            sagitta,
            &free_along,
            &[],
            ends,
            &[],
        )?)
    };
    // each bead at a corner alone, cut at the walls it runs into, then all joined
    for (i, (&e, size)) in edges.iter().zip(sizes).enumerate() {
        if !at_corner.contains(&i) {
            continue;
        }
        let mine = cuts.get(i).cloned().unwrap_or_default();
        let mut bead = beads_together(
            k,
            target,
            &[e],
            core::slice::from_ref(size),
            sagitta,
            &[along_of(i)],
            &[],
            ends,
            core::slice::from_ref(&mine),
        )?;
        for cut in &mine {
            let block = wall_block(k, cut)?;
            bead = boolean_difference(k, bead, block)?;
        }
        joined = Some(match joined {
            None => bead,
            Some(body) => crate::boolean::api_brep::boolean_union(k, body, bead)?,
        });
    }
    let mut body = joined.ok_or_else(|| {
        defect("a corner joined no bead, though it is built only for two or more ends")
    })?;
    for piece in pieces {
        body = crate::boolean::api_brep::boolean_union(k, body, piece)?;
    }
    Ok(body)
}

/// The block of `cut` ([`WallCut`]).
///
/// # Errors
///
/// Those of the sweep.
fn wall_block(k: &mut Kernel, cut: &WallCut) -> Result<Brep> {
    let (o, x, y) = (cut.o, cut.x, cut.y);
    let h = 2.0 * cut.reach;
    let corners = [
        o - x * h - y * h,
        o + x * h - y * h,
        o + x * h + y * h,
        o - x * h + y * h,
    ];
    let mut sides = Vec::with_capacity(4);
    for (i, &p) in corners.iter().enumerate() {
        let q = corners.get((i + 1) % 4).copied().ok_or_else(|| {
            defect("a wall block's square lost a corner while its sides were walked")
        })?;
        sides.push(Curve::from(create_line_segment(p, q)?));
    }
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, cut.n, cut.reach, true)
}
