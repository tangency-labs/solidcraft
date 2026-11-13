//! Tangent rollover of a whole rail (not upstream; solidcore-native).
//!
//! A rolling ball's support on each side is a chain of faces meeting smoothly, not one face.
//! Where a face beside a round is narrower than the ball reaches and its far edge is smooth
//! (G1) to the face beyond, the ball does not leave the body there: it rolls on across that
//! edge onto the face beyond, and the face it rolled off lies wholly inside the round's band
//! (it is consumed). A concave round beside a strip whose far rim is already rounded is the
//! case: rolling on the strip's plane continued, the ball's contact lies past the strip, over
//! the rim's round, and is refused as a blend leaving its face; rolling on the rim's round, it
//! is an ordinary round between the wall and that round.
//!
//! SMLib's rollover recovery (`ProcessRollOver`) switches a rail's support where it crosses a
//! tangent edge partway along (`FilletGeomType::TangentRollover`); a rail that never touches its
//! face is a full rollover there, which upstream builds only round a cliff
//! ("Unimplemented Tangent Rollover Case", [`FilletSolver::process_full_roll_over`]). Here the
//! full tangent rollover is decided before the trace, in the round's section, where it is exact
//! ([`tangent_rollover_supports`]), and the solver rolls the ball on the face beyond from the
//! start, as [`crate::blend::fillet_outline`] gives the solver its rim supports.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`tangent_rollover_supports`] | none (solidcore-native) |
//! | [`rolled_edge_point`] | none (solidcore-native) |
//!
//! Deviations: none of upstream's; there is no recursion.
//!
//! [`FilletSolver::process_full_roll_over`]: crate::blend::fillet_solver::p1::FilletSolver::process_full_roll_over

use crate::blend::fillet_solver::p1::test_manifold_tangent_rollover;
use crate::error::{Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::knots::Side as KnotSide;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_tangent_rollover";

/// The tolerance the edges' curves are recognised as lines within.
const CURVE_TOL: f64 = 1.0e-8;

/// The sine of the angle two directions may make and still count as parallel.
const DIR_TOL: f64 = 1.0e-9;

/// A face the ball rolls onto across a smooth edge of the face beside the round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollover {
    /// The smooth edge the ball rolls over (the far edge of the face beside the round).
    pub edge: EdgeId,
    /// The face beyond it, which the ball rolls on.
    pub face: FaceId,
    /// Whether that face's surface normal is opposite the rolled-off face's across the edge
    /// (`sm_TestManifoldTangentRollover`'s second answer): the offset changes sign.
    pub opposite_normals: bool,
}

/// The faces a constant round's ball rolls onto across a smooth far edge of the face on each
/// side, or `None` on a side where it touches its own face.
///
/// `filleted` is the round's edge, `faces[i]` the face of its rail `i` and `offsets[i]` the
/// solver's signed offset of that face's surface (the ball's centre lies at the face's point
/// plus its surface normal times the offset). Decided in the section across the middle of a
/// straight edge between two planes: the ball's centre is where the offset lines meet, and its
/// contact on a face is that centre's foot on the face's line. A side rolls over when the face's
/// other edge that runs the filleted edge's whole length (a straight edge parallel to it whose
/// span covers it) is tangent to the face beyond (`sm_TestManifoldTangentRollover` within
/// `tangency_tol_rad`) and the contact lies past it, by more than the body's tolerance. Any
/// other round, and a round with no such side, gives `[None, None]`.
///
/// # Errors
///
/// Those of the topology queries, the curves' recognisers, the evaluations and the tangency
/// test; `SM_ERR` for a face without a surface.
pub fn tangent_rollover_supports(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
    tangency_tol_rad: f64,
) -> Result<[Option<Rollover>; 2]> {
    let none = [None, None];
    let tol = target.tolerance();
    let Some(curve) = edge::curve(target, filleted)?.cloned() else {
        return Ok(none);
    };
    let Some((_, vector)) = curve.is_line(k, 3, CURVE_TOL)? else {
        return Ok(none);
    };
    let Ok(d) = vector.unitize() else {
        return Ok(none);
    };
    let ivl = edge::interval(target, filleted)?;
    let ends = [
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ];
    let m = curve.evaluate_point(k, ivl.evaluate(0.5))?;
    // each face a plane, its surface normal at the section, square to the edge
    let mut normals = [Vec3::UNDEF; 2];
    for (normal, &face) in normals.iter_mut().zip(&faces) {
        let surface = crate::topology::face::p2::surface(target, face)?
            .ok_or_else(|| SolidError::blend_failed(OP, "a face of the edge has no surface"))?
            .clone_for_evaluation()?;
        if !surface.is_planar(k, tol)?.is_planar {
            return Ok(none);
        }
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let dropped =
            surface.drop_point::<NoObject>(k, m, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(none);
        }
        let n = surface.evaluate_normal(k, dropped.result_uv, KnotSide::Left, KnotSide::Left)?;
        let Ok(n) = (n - d * n.dot(d)).unitize() else {
            return Ok(none);
        };
        *normal = n;
    }
    // the section's frame: x along the first face's normal, y square to it and the edge
    let x = normals[0];
    let y = d.cross(x);
    let to2 = |p: Point3| {
        let v = p - m;
        (v.dot(x), v.dot(y))
    };
    let n2 = normals.map(|n| (n.dot(x), n.dot(y)));
    // the ball's centre: n_i . p = offset_i (both face lines pass through the section origin)
    let [(a0, b0), (a1, b1)] = n2;
    let det = a0 * b1 - b0 * a1;
    if det.is_nan() || det.abs() <= DIR_TOL {
        return Ok(none);
    }
    let [o0, o1] = offsets;
    let centre = ((o0 * b1 - b0 * o1) / det, (a0 * o1 - o0 * a1) / det);
    let mut out = none;
    for (((slot, &(na, nb)), &offset), &face) in out.iter_mut().zip(&n2).zip(&offsets).zip(&faces) {
        // the contact: the centre's foot on the face's line
        let contact = (centre.0 - na * offset, centre.1 - nb * offset);
        for e in face_p4::edges(target, face, None, false, None)? {
            if e == filleted {
                continue;
            }
            let Some(far_curve) = edge::curve(target, e)?.cloned() else {
                continue;
            };
            let Some((point, far_vector)) = far_curve.is_line(k, 3, CURVE_TOL)? else {
                continue;
            };
            let Ok(fd) = far_vector.unitize() else {
                continue;
            };
            if d.cross(fd).length() > DIR_TOL {
                continue;
            }
            // its span along the filleted edge covers the filleted edge's
            let far_ivl = edge::interval(target, e)?;
            let along = |p: Point3| (p - point).dot(d);
            let r0 = along(far_curve.evaluate_point(k, far_ivl.min())?);
            let r1 = along(far_curve.evaluate_point(k, far_ivl.max())?);
            let [a, b] = ends.map(along);
            if r0.min(r1) > a.min(b) + tol || r0.max(r1) < a.max(b) - tol {
                continue;
            }
            // the far edge's point in the section, and how far along the face it lies
            let t = to2(point);
            let width = (t.0 * t.0 + t.1 * t.1).sqrt();
            if width <= tol {
                continue;
            }
            let reach = (contact.0 * t.0 + contact.1 * t.1) / width;
            if reach.is_nan() || reach <= width + tol {
                continue;
            }
            let (tangent, opposite_normals) =
                test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?;
            if !tangent {
                continue;
            }
            let Some(&beyond) = edge::faces(target, e, None)?.iter().find(|&&f| f != face) else {
                continue;
            };
            // the face beyond must carry the ball: its offset by the ball's offset (signed by the
            // normals across the edge) may not fold across the edge, or no ball rolls on it (a
            // convex round larger than the convex round beyond); the side is then left to the
            // refusals as before
            let offset_beyond = if opposite_normals { -offset } else { offset };
            let far_point = m + x * t.0 + y * t.1;
            if offset_folds(k, target, beyond, far_point, d, offset_beyond)? {
                continue;
            }
            *slot = Some(Rollover {
                edge: e,
                face: beyond,
                opposite_normals,
            });
            break;
        }
    }
    Ok(out)
}

/// The nearest point to `point` on the smooth edge a rail rolled over: where a ball of radius 0
/// rolling on the face beyond touches it, so the section of the rolled ball is searched from
/// there (the filleted edge's point lies off the face beyond, and a drop onto that face's
/// surface may land on another sheet of it, the far side of a cylinder). The edges are those
/// `from` (the face the rail rolled off) and `onto` (the face beyond) share that meet tangentially
/// within `tangency_tol_rad`; of these, the one whose nearest point is nearest `point` (two
/// half cylinders share two seams), the first of equally near ones in `onto`'s edge order.
/// `None` where they share no such edge, or the point drops onto none of them.
///
/// # Errors
///
/// Those of the topology queries, the tangency test and the drops.
pub fn rolled_edge_point(
    k: &Kernel,
    target: &mut Brep,
    from: FaceId,
    onto: FaceId,
    point: Point3,
    tangency_tol_rad: f64,
) -> Result<Option<Point3>> {
    let mut best: Option<(Point3, f64)> = None;
    for e in face_p4::edges(target, onto, None, false, None)? {
        if !edge::faces(target, e, None)?.contains(&from) {
            continue;
        }
        if !test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?.0 {
            continue;
        }
        let Some(curve) = edge::curve(target, e)?.cloned() else {
            continue;
        };
        let ivl = edge::interval(target, e)?;
        let dropped = curve.drop_point(
            k,
            &ivl,
            point,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success {
            continue;
        }
        let on = curve.evaluate_point(k, dropped.parameter)?;
        let distance = (on - point).length();
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((on, distance));
        }
    }
    Ok(best.map(|(p, _)| p))
}

/// Whether the offset of `face`'s surface by `offset` (along its surface normal) folds across the
/// direction square to `along` at the point of the face nearest `at`: `1 - kappa offset <= 0`, with
/// `kappa` the surface's normal curvature in that direction (`II(t, t) / I(t, t)`). A ball of that
/// signed offset cannot roll on the face there. A point the face does not take, or a degenerate
/// frame, counts as folding (the rollover is then not attempted).
fn offset_folds(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    at: Point3,
    along: Vec3,
    offset: f64,
) -> Result<bool> {
    let surface = crate::topology::face::p2::surface(target, face)?
        .ok_or_else(|| SolidError::blend_failed(OP, "the face has no surface"))?
        .clone_for_evaluation()?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<NoObject>(k, at, &domain, &DropPointOptions::default())?;
    if !dropped.success {
        return Ok(true);
    }
    let uv = dropped.result_uv;
    let [_, du, dv, duv, duu, dvv] = surface.evaluate_2nd_derivatives(k, uv, true, true)?;
    let n = surface.evaluate_normal(k, uv, KnotSide::Left, KnotSide::Left)?;
    let Ok(t) = n.cross(along).unitize() else {
        return Ok(true);
    };
    // t = a du + b dv, by the first fundamental form
    let (g11, g12, g22) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = g11 * g22 - g12 * g12;
    if det.is_nan() || det <= 0.0 {
        return Ok(true);
    }
    let (r1, r2) = (t.dot(du), t.dot(dv));
    let a = (g22 * r1 - g12 * r2) / det;
    let b = (g11 * r2 - g12 * r1) / det;
    let kappa = (duu * (a * a) + duv * (2.0 * a * b) + dvv * (b * b)).dot(n);
    Ok(1.0 - kappa * offset <= 0.0)
}
