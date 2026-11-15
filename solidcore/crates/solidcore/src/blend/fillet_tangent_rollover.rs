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
//! start, as [`crate::blend::fillet_outline`] gives the solver its rim supports. A round that is
//! not a lone one between planes along a straight edge (one of a chain, #15767) is decided from
//! its ball's sections along its edge (the session's `roll_over_tangent_edges`).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`tangent_rollover_supports`] | none (solidcore-native) |
//! | [`rolled_edge_point`] | none (solidcore-native) |
//! | `section_seeds`, `SectionSeed` (crate) | none (solidcore-native) |
//! | `planar_section`, `PlanarSection` (crate) | none (solidcore-native) |
//!
//! Deviations: none of upstream's; there is no recursion.
//!
//! [`FilletSolver::process_full_roll_over`]: crate::blend::fillet_solver::p1::FilletSolver::process_full_roll_over

use crate::blend::fillet_solver::p1::{test_manifold_tangent_rollover, FilletSolver};
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
    let Some(PlanarSection {
        d,
        ends,
        m,
        x,
        y,
        n2,
        centre,
    }) = planar_section(k, target, filleted, faces, offsets)?
    else {
        return Ok(none);
    };
    let to2 = |p: Point3| {
        let v = p - m;
        (v.dot(x), v.dot(y))
    };
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

/// A constant round's section across the middle of a straight edge between two planes, in
/// closed form ([`planar_section`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanarSection {
    /// The edge's unit direction.
    pub d: Vec3,
    /// The edge's ends.
    pub ends: [Point3; 2],
    /// The edge's middle, the section's origin.
    pub m: Point3,
    /// The section's x axis: the first face's surface normal there, square to the edge.
    pub x: Vec3,
    /// The section's y axis, `d x x`.
    pub y: Vec3,
    /// Each face's surface normal in the section's frame.
    pub n2: [(f64, f64); 2],
    /// The ball's centre in the section's frame, where the offset lines meet.
    pub centre: (f64, f64),
}

impl PlanarSection {
    /// The ball's contact on face `rail`'s plane (the centre's foot on its line), in space,
    /// for the offset `offset` of that face; `None` for a rail other than 0 or 1.
    pub(crate) fn contact(&self, rail: usize, offset: f64) -> Option<Point3> {
        let &(na, nb) = self.n2.get(rail)?;
        let c = (self.centre.0 - na * offset, self.centre.1 - nb * offset);
        Some(self.m + self.x * c.0 + self.y * c.1)
    }
}

/// The section of the constant round of signed offsets `offsets` on `faces` across the middle
/// of the straight edge `filleted`, both faces planes: the ball's centre is where the offset
/// lines meet (the ball's centre lies at a face's point plus its surface normal times the
/// offset). `None` where the edge has no curve or is not a line, a face is not a plane or its
/// normal does not drop or is along the edge, or the faces' lines are parallel in the section.
///
/// # Errors
///
/// Those of the curve's recogniser, the evaluations and the drops; `SM_ERR` for a face without
/// a surface.
pub(crate) fn planar_section(
    k: &Kernel,
    target: &Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
) -> Result<Option<PlanarSection>> {
    let tol = target.tolerance();
    let Some(curve) = edge::curve(target, filleted)?.cloned() else {
        return Ok(None);
    };
    let Some((_, vector)) = curve.is_line(k, 3, CURVE_TOL)? else {
        return Ok(None);
    };
    let Ok(d) = vector.unitize() else {
        return Ok(None);
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
            return Ok(None);
        }
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let dropped =
            surface.drop_point::<NoObject>(k, m, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(None);
        }
        let n = surface.evaluate_normal(k, dropped.result_uv, KnotSide::Left, KnotSide::Left)?;
        let Ok(n) = (n - d * n.dot(d)).unitize() else {
            return Ok(None);
        };
        *normal = n;
    }
    // the section's frame: x along the first face's normal, y square to it and the edge
    let x = normals[0];
    let y = d.cross(x);
    let n2 = normals.map(|n| (n.dot(x), n.dot(y)));
    // the ball's centre: n_i . p = offset_i (both face lines pass through the section origin)
    let [(a0, b0), (a1, b1)] = n2;
    let det = a0 * b1 - b0 * a1;
    if det.is_nan() || det.abs() <= DIR_TOL {
        return Ok(None);
    }
    let [o0, o1] = offsets;
    let centre = ((o0 * b1 - b0 * o1) / det, (a0 * o1 - o0 * a1) / det);
    Ok(Some(PlanarSection {
        d,
        ends,
        m,
        x,
        y,
        n2,
        centre,
    }))
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

/// A guess of a rolling ball's section: the point dropped onto a face's surface for it, and
/// for a rail rolled over a smooth edge, the direction the ball rolls across that edge in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SectionSeed {
    /// The point dropped onto the face's surface.
    pub point: Point3,
    /// For a rolled rail, the direction from the filleted edge's point to the rolled-over
    /// edge's point: the side of a seam of the surface (a closed boss's, running up the
    /// rolled-over edge) on which the face beyond lies, as the drop's
    /// `DropPointOptions::tgt_seam_side`. `None` for every other rail, and where the two points
    /// coincide within the body's tolerance (no direction: [`seam_side_hint`]).
    pub seam_side: Option<Vec3>,
    /// Whether `point` is the rolled-over edge's point of a rolled rail (with or without a
    /// `seam_side`): the section is then solved on the surface extended past that edge.
    pub rolled: bool,
}

/// The seam-side hint of a rolled rail's seed ([`SectionSeed::seam_side`]): the direction from
/// the filleted edge's point `point` to the rolled-over edge's point `on`, `None` where the two
/// lie within `tol` of each other (a zero or near-zero vector names no side), or the distance
/// is not a number.
fn seam_side_hint(on: Point3, point: Point3, tol: f64) -> Option<Vec3> {
    let side = on - point;
    (side.length() > tol).then_some(side)
}

/// Where a ball of radius 0 touches each of `solver`'s faces at the point `point` of its
/// filleted edge, the guesses of the ball's section there: the edge point, or for a rail
/// rolled over a smooth far edge of its face (`solver.rolled_faces`) the point of that edge
/// nearest it ([`rolled_edge_point`]), on the face beyond. The edge point lies off the face
/// beyond, and dropped on its surface it may land where the section solve finds the ball
/// rolling on another sheet of that surface (the far side of a boss). The rolled-over edge's
/// point may lie on the surface's seam (a boss whose seam runs up that edge), where it has two
/// parameters: the one on the side the ball rolls onto is the guess
/// ([`SectionSeed::seam_side`]).
///
/// # Errors
///
/// Those of the topology queries and of [`rolled_edge_point`].
pub(crate) fn section_seeds(
    k: &Kernel,
    target: &mut Brep,
    solver: &FilletSolver,
    point: Point3,
) -> Result<[SectionSeed; 2]> {
    let mut seeds = [SectionSeed {
        point,
        seam_side: None,
        rolled: false,
    }; 2];
    let tol = target.tolerance();
    for (rail, seed) in seeds.iter_mut().enumerate() {
        let Some(onto) = solver.rolled_faces.get(rail).copied().flatten() else {
            continue;
        };
        let Some(own) = solver.edgeuse(rail)? else {
            continue;
        };
        let Some(from) = crate::topology::edgeuse::p1::face(target, own)? else {
            continue;
        };
        if let Some(p) = rolled_edge_point(k, target, from, onto, point, solver.tangency_tolerance)?
        {
            *seed = SectionSeed {
                point: p,
                seam_side: seam_side_hint(p, point, tol),
                rolled: true,
            };
        }
    }
    Ok(seeds)
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

#[cfg(test)]
mod tests {
    use super::{seam_side_hint, section_seeds, SectionSeed};
    use crate::blend::fillet_solver::p1::FilletSolver;
    use crate::curve::curve::Curve;
    use crate::kernel::Kernel;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_curves::create_line_segment;
    use crate::sweep::api_brep::create_linear_sweep;
    use crate::topology::{edge, Brep, EdgeId, FaceId};

    /// The one edge of `b` from `p` to `q` (either way round).
    fn edge_from_to(b: &Brep, p: Point3, q: Point3) -> EdgeId {
        let at = |v: Option<crate::topology::VertexId>| b.vertex(v.unwrap()).unwrap().point;
        let found: Vec<EdgeId> = b
            .edge_list()
            .iter()
            .copied()
            .filter(|&e| {
                let (s, t) = (
                    at(edge::start_vertex(b, e).unwrap()),
                    at(edge::end_vertex(b, e).unwrap()),
                );
                (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                    || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
            })
            .collect();
        assert_eq!(found.len(), 1, "edges {p:?} to {q:?}: {found:?}");
        found[0]
    }

    /// A rolled rail's seed where the filleted edge's point lies on the rolled-over edge, old to
    /// new (#16354's zero seam hint). The prism 10 tall on the polygon (0, -20), (40, -20),
    /// (40, 0.1), (20, 0), (0, 0): its side faces S (y = 0) and X (on to (40, 0.1)) meet along
    /// the vertical edge T at (20, 0), smooth (a crease of atan(0.1 / 20) = 0.005 rad within
    /// the solver's tangency tolerance 0.01). The round is on S's top edge from (0, 0, 10) to
    /// T's top V = (20, 0, 10), its rail rolled from S onto X. At V itself and 0.5 tol short of
    /// it along the edge (tol the body's tolerance), the rolled-over edge's nearest point is V,
    /// within tol of the edge's point: the seed is V with no seam-side hint (before #16354 the
    /// hint was the zero or near-zero vector `V - point`), still solved on the extended surface
    /// (`rolled`). At (10, 0, 10) the hint is the direction to V, (10, 0, 0). The other rail is
    /// does not roll; its seed is the edge's point, with no hint.
    #[test]
    fn a_rolled_seed_on_the_rolled_over_edge_has_no_seam_side_hint() {
        let mut k = Kernel::default();
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let poly = [
            p(0.0, -20.0),
            p(40.0, -20.0),
            p(40.0, 0.1),
            p(20.0, 0.0),
            p(0.0, 0.0),
        ];
        let sides: Vec<Curve> = (0..poly.len())
            .map(|i| Curve::from(create_line_segment(poly[i], poly[(i + 1) % poly.len()]).unwrap()))
            .collect();
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        let mut b =
            create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap();
        let v = Point3::new(20.0, 0.0, 10.0);
        let filleted = edge_from_to(&b, Point3::new(0.0, 0.0, 10.0), v);
        let t = edge_from_to(&b, Point3::new(20.0, 0.0, 0.0), v);
        let t_faces = edge::faces(&b, t, None).unwrap();
        let s = *edge::faces(&b, filleted, None)
            .unwrap()
            .iter()
            .find(|f| t_faces.contains(f))
            .unwrap();
        let x: FaceId = *t_faces.iter().find(|&&f| f != s).unwrap();
        let on_s = edge::edgeuses(&b, filleted, None, None)
            .unwrap()
            .into_iter()
            .find(|&eu| crate::topology::edgeuse::p1::face(&b, eu).unwrap() == Some(s))
            .unwrap();
        let mut solver = FilletSolver::new(1e-6, 0.1, 0.01);
        solver.edgeuses = [Some(on_s), None];
        solver.rolled_faces = [Some(x), None];
        let tol = b.tolerance();
        assert!(tol > 0.0);
        let hint = Point3::new(10.0, 0.0, 10.0);
        for (point, seam_side) in [
            (v, None),
            (v - Vec3::new(0.5 * tol, 0.0, 0.0), None),
            (hint, Some(v - hint)),
        ] {
            let seeds = section_seeds(&k, &mut b, &solver, point).unwrap();
            assert!(
                seeds[0].point.distance(v) <= 1e-12,
                "{point:?}: seed {:?}",
                seeds[0].point
            );
            match (seeds[0].seam_side, seam_side) {
                (None, None) => {}
                (Some(got), Some(want)) => {
                    assert!((got - want).length() <= 1e-12, "{point:?}: hint {got:?}");
                }
                (got, want) => panic!("{point:?}: hint {got:?}, want {want:?}"),
            }
            assert!(seeds[0].rolled, "{point:?}");
            assert_eq!(
                seeds[1],
                SectionSeed {
                    point,
                    seam_side: None,
                    rolled: false,
                },
                "{point:?}"
            );
        }
    }

    /// A rolled rail's seam-side hint is the direction to the rolled-over edge's point, and no
    /// hint at all where that point is the filleted edge's own within the tolerance (the zero
    /// vector the hint was before names no side of a seam), or the distance is not a number.
    #[test]
    fn a_seam_side_hint_is_never_a_zero_vector() {
        let tol = 1e-6;
        let point = Point3::new(1.0, 2.0, 3.0);
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.5, 3.0), point, tol),
            Some(Vec3::new(0.0, 0.5, 0.0))
        );
        assert_eq!(seam_side_hint(point, point, tol), None);
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.0, 3.0 + 0.5 * tol), point, tol),
            None
        );
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.0, 3.0 + 2.0 * tol), point, tol),
            Some(Vec3::new(0.0, 0.0, 3.0 + 2.0 * tol - 3.0))
        );
        let nan = Point3::new(f64::NAN, 2.0, 3.0);
        assert_eq!(seam_side_hint(nan, point, tol), None);
    }
}
