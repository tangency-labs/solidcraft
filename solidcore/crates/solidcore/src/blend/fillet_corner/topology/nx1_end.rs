//! The end of an `NByOne` corner: the end rib and, at a mixed-convexity corner, the
//! surface extension and the cap topology around the original vertex.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp` (606–667, 6177–6443).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`find_surface_extension_dist_at_corner`] | `sm_FindSurfaceExtensionDistAtCorner` |
//! | `finish_nx1_end` (crate) | tail of `SmFilletNx1Corner::MakeCornerTopology` |
//! | `rolled_rail_turns_back`, `rail_passes_side_edge_end` (crate) | none (solidcore-native) |
//!
//! Deviations: the solver is the caller's concrete solver object instead of a
//! `GetSolverType` cast. A failed variable-radius law evaluation yields its status
//! code as the radius, as upstream's `SER` inside the `double`
//! `SmVariableRadiusFS::GetFilletRadius` does (an `Unported` callee and a law refused
//! past its curve, #2706, still propagate). The gap checks of a multi-neighbour end
//! face (vertex and side-edge geometry, extended-surface coincidence), the last statements of
//! `MakeCornerTopology`, are returned as an [`Nx1GapCheck`] for the caller, which
//! holds the corner table and solver store they need, to run before anything else.
//! The end face at a tangent side is extended `CInfinity` and that side's cap edge is
//! `OnExtendEdge` (WR-600, the fork's tangency-labs/smlib@856b54e). At a multi-neighbour
//! end face whose side edges are both cliff edges, the end face is the side face whose
//! surface holds the other side edge; upstream takes the first side's, whatever its
//! surface. There is no recursion.

use std::collections::BTreeMap;

use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_executive::session::{FilletBody, FilletObject, SolverObject};
use crate::blend::fillet_geom::p1::{
    FilletBrep, FilletCornerId, FilletEdgeType, FilletGeomId, FilletVertex, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::blend::fillet_standard_solver::p1::VariableRadiusFs;
use crate::blend::fillet_tangent_rollover::planar_section;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{BrepFaceServices, FaceCaches};
use crate::surface::surface::Surface;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, Vertex, VertexId};

const OP: &str = "blend::fillet_corner::topology::make_nx1_corner_topology";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn need<T>(value: Option<T>, reason: &'static str) -> Result<T> {
    value.ok_or_else(|| invalid(reason))
}

/// Whether the surface of `side`'s face holds the whole edge of `other` within `tol_3d`
/// (the surface's coincidence check, as the gap check runs it on the end face's extension).
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without face, surface, edge or curve; those
/// of the coincidence check.
fn face_holds_edge(
    k: &Kernel,
    target: &mut Brep,
    side: EdgeuseId,
    other: EdgeuseId,
    tol_3d: f64,
) -> Result<bool> {
    let face = need(eu1::face(target, side)?, "side edge use without face")?;
    let surface = need(
        crate::topology::face::p2::surface(target, face)?,
        "side face without surface",
    )?
    .clone_for_evaluation()?;
    let other_edge = need(eu1::edge(target, other)?, "side edge use without edge")?;
    let curve = need(edge::curve(target, other_edge)?, "side edge without curve")?
        .clone_for_evaluation()?;
    let interval = edge::interval(target, other_edge)?;
    // the face's own surface: its trimmed-surface services come from the B-rep
    let mut services = BrepFaceServices::with_caches(target, FaceCaches::new());
    Ok(surface
        .simple_coincidence_checker_with_context(
            k,
            &surface.natural_uv_domain_with_nurb(k)?,
            &curve,
            &interval,
            tol_3d,
            false,
            [
                Some(CacheObject::Curve(other_edge)),
                Some(CacheObject::Surface(face)),
            ],
            None,
            &mut services,
        )?
        .is_some())
}

/// `sm_FindSurfaceExtensionDistAtCorner`: an extension distance for the end surface of
/// an Nx1 or 3x2 mixed-convexity corner, `7.5` times the largest fillet radius of a
/// constant-radius solver, the distance of a constant-distance solver or the radius a
/// variable-radius solver has at the corner end of `fillet_edgeuse` (`0` for any other
/// solver, whose `SE(SM_ERR)` is only reported).
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without vertex use or edge (upstream
/// dereferences them); [`SolidError::Unported`] from the variable-radius law evaluation.
pub fn find_surface_extension_dist_at_corner(
    target: &Brep,
    fillet_edgeuse: EdgeuseId,
    solver: &mut SolverObject,
    corner_vertex: VertexId,
) -> Result<f64> {
    let mut ext_dist = 0.0;
    match solver {
        // return largest fillet radius
        SolverObject::ConstantRadius(cr) => {
            for i in 0..2 {
                let rad = cr.fillet_radius(i)?;
                if rad > ext_dist {
                    ext_dist = rad;
                }
            }
        }
        // return constant distance
        SolverObject::ConstantDistance(cd) => ext_dist = cd.distance(),
        // return radius at vertex
        SolverObject::VariableRadius(vr) => {
            ext_dist = variable_radius_at_vertex(target, fillet_edgeuse, vr, corner_vertex)?;
        }
        // SE(SM_ERR): reported only
        SolverObject::SurfaceSurface(_) => {}
    }

    // In prog_test, Fillet test my_test_suite_0 iter: 2 will create a surface
    // with a slight crease for extension values greater than about 7.5.
    let rad_factor = 7.5;
    Ok(rad_factor * ext_dist)
}

/// `SmVariableRadiusFS::GetFilletRadius` at the corner end of `fillet_edgeuse`: the law's
/// radius at the end of the filleted edge that lies on `corner_vertex`, by
/// [`FilletLaw::fillet_radius`](crate::blend::fillet_executive::p1::FilletLaw): a failed
/// law evaluation yields its status code as the radius.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an edge use without vertex use or edge (upstream
/// dereferences them); those [`FilletLaw::fillet_radius`] propagates: an unported callee,
/// and the refusal of a law continued past its curve to an unusable radius (the corner may
/// have narrowed the law to its setback, so the edge's end can be past the law's curve).
///
/// [`FilletLaw::fillet_radius`]: crate::blend::fillet_executive::p1::FilletLaw::fillet_radius
pub fn variable_radius_at_vertex(
    target: &Brep,
    fillet_edgeuse: EdgeuseId,
    vr: &VariableRadiusFs,
    corner_vertex: VertexId,
) -> Result<f64> {
    let e = need(eu1::edge(target, fillet_edgeuse)?, "edge use without edge")?;
    let ivl = edge::interval(target, e)?;
    let vu = need(
        eu1::vertexuse(target, fillet_edgeuse)?,
        "edge use without vertex use",
    )?;
    let vert = target.vertexuse(vu)?.vertex;
    let orient = eu1::orientation(target, fillet_edgeuse)?;
    let parameter = if (vert == Some(corner_vertex) && orient == OrientType::Opposite)
        || (vert != Some(corner_vertex) && orient == OrientType::Same)
    {
        ivl.max()
    } else {
        ivl.min()
    };
    // a copy: the evaluation may build the law's extension (`m_pExtendedLaw`), the solver's
    // own law keeps building it when it evaluates
    vr.law
        .clone()
        .fillet_radius(parameter, &vr.law_interval, vr.law_orientation)
}

/// Not upstream (solidcore-native, [`crate::blend::fillet_tangent_rollover`]): where a rail
/// rolled over a smooth edge T of its face S onto the face X beyond ends, when the side edge E
/// at the corner's vertex (between S and the end face P) reaches T's vertex and the edge N there
/// between X and P turns back along E (a boss standing taller than the end face, whose foot runs
/// round the boss away from the rail). The end face's surface meets X along N's curve continued
/// back past that vertex, and the rail ends there, inside X: the cap's edge runs from the rail's
/// end along that continuation to the vertex, then along E.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnedBack {
    /// T's vertex at E's far end, where N starts.
    pub far: VertexId,
    /// N, the edge between X and the end face that turns back at `far`.
    pub edge: EdgeId,
    /// X, the face the rail rolled onto.
    pub face: FaceId,
}

/// [`TurnedBack`] for the rail rolled onto `rolled` whose corner side edge use is `side` at the
/// corner's vertex `vertex`: `None` unless E's far vertex holds a smooth edge (within
/// `tangency_tol_rad`) between E's other face and `rolled`, and an edge between `rolled` and
/// the face of E it shares whose tangent leaving that vertex points back along E's arriving
/// tangent, nearer antiparallel to it than square (`turns_back`; at an exactly tangent T the
/// two are antiparallel, at one tangent only within the tolerance they are near it).
///
/// # Errors
/// Those of the topology walks, the evaluations and the tangency test.
pub(crate) fn rolled_rail_turns_back(
    k: &Kernel,
    target: &mut Brep,
    side: EdgeuseId,
    vertex: VertexId,
    rolled: FaceId,
    tangency_tol_rad: f64,
) -> Result<Option<TurnedBack>> {
    let Some(e) = eu1::edge(target, side)? else {
        return Ok(None);
    };
    if edge::is_closed(target, e)? {
        return Ok(None);
    }
    let (start, end) = (edge::start_vertex(target, e)?, edge::end_vertex(target, e)?);
    let (far, e_arrives_at_end) = if start == Some(vertex) {
        (end, true)
    } else if end == Some(vertex) {
        (start, false)
    } else {
        return Ok(None);
    };
    let Some(far) = far else {
        return Ok(None);
    };
    let e_faces = edge::faces(target, e, None)?;
    if e_faces.len() != 2 || e_faces.contains(&rolled) {
        return Ok(None);
    }
    let edges = crate::topology::vertex::edges(target, far, None)?;
    // T: between a face of E and `rolled`, smooth
    let mut rolled_off = None;
    for &t in &edges {
        if t == e {
            continue;
        }
        let t_faces = edge::faces(target, t, None)?;
        let Some(&s) = t_faces.iter().find(|f| e_faces.contains(f)) else {
            continue;
        };
        if !t_faces.contains(&rolled) {
            continue;
        }
        if crate::blend::fillet_solver::p1::test_manifold_tangent_rollover(
            k,
            target,
            t,
            tangency_tol_rad,
        )?
        .0
        {
            rolled_off = Some(s);
            break;
        }
    }
    let Some(s) = rolled_off else {
        return Ok(None);
    };
    let Some(&p) = e_faces.iter().find(|&&f| f != s) else {
        return Ok(None);
    };
    let tangent = |edge_id: EdgeId, at_end: bool| -> Result<Vec3> {
        crate::topology::vertex::edge_end_tangent(k, target, edge_id, !at_end)
    };
    // E's tangent arriving at `far`
    let te = if e_arrives_at_end {
        tangent(e, true)?
    } else {
        -tangent(e, false)?
    };
    for &n in &edges {
        if n == e || edge::is_closed(target, n)? {
            continue;
        }
        let n_faces = edge::faces(target, n, None)?;
        if !(n_faces.contains(&rolled) && n_faces.contains(&p)) {
            continue;
        }
        // N's tangent leaving `far`
        let Some(tn) = tangent_leaving(k, target, n, far)? else {
            continue;
        };
        if turns_back(te, tn) {
            return Ok(Some(TurnedBack {
                far,
                edge: n,
                face: rolled,
            }));
        }
    }
    Ok(None)
}

/// Not upstream (solidcore-native, [`TurnedBack`]): where a rail that did not roll ends past
/// its side edge's far end. The rail of solver `solver`'s rail `rail` lies on the face S of
/// the side edge E (`side`, at the corner's vertex `vertex`); E's other face is the end face
/// P. The rail ends where it crosses P's surface, on E's curve continued past E's far vertex
/// W, when all of these hold:
/// - the round is a constant one of circular section (`constant`) between two planes along a
///   straight edge, and E is a line: the rail is then the line along the edge through the
///   ball's contact on S, in closed form ([`planar_section`]), and it crosses E's line at R;
/// - R lies past W along E, by more than the body's tolerance;
/// - at W, an edge N between P and a face other than S leaves W turned back along E's
///   arriving tangent ([`turns_back`]): P's boundary turns back at W, so P does not reach R;
/// - R lies inside S (classified in S's interior, not on its boundary, within the body's
///   tolerance), and no boundary edge of S crosses the open segment from W to R, the cap
///   edge's place on S's plane ([`edge_clear_of_segment`]).
/// - P is a plane, so R, on E's line, is where the rail meets P's surface.
///
/// The cap's edge then runs from the rail's end along E's continuation on S to W, then along
/// E: [`TurnedBack`] with E as its edge (whose continuation past W leaves W against E's own
/// tangent leaving W) and S as its face. `None` otherwise (the corner is as before).
///
/// # Errors
/// Those of the topology walks, the closed form's evaluations and drops, the curve's
/// recogniser and the classification.
#[allow(clippy::too_many_lines)] // one decision, its conditions in order
pub(crate) fn rail_passes_side_edge_end(
    k: &Kernel,
    target: &mut Brep,
    solver: &FilletSolver,
    constant: bool,
    rail: usize,
    side: EdgeuseId,
    vertex: VertexId,
) -> Result<Option<TurnedBack>> {
    use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
    let circular = solver.fsg.as_ref().map(
        crate::blend::fillet_solver::p1::FilletSurfaceGenerator::fillet_surface_generator_type,
    ) == Some(FilletSurfaceGeneratorType::Circular);
    if !constant || !circular || rail > 1 {
        return Ok(None);
    }
    let tol = target.tolerance();
    // the solver's edge, its faces and offsets
    let [Some(u0), Some(u1)] = solver.edgeuses else {
        return Ok(None);
    };
    let Some(filleted) = eu1::edge(target, u0)? else {
        return Ok(None);
    };
    let (Some(f0), Some(f1)) = (eu1::face(target, u0)?, eu1::face(target, u1)?) else {
        return Ok(None);
    };
    let faces = [f0, f1];
    let mut offsets = [0.0; 2];
    for (offset, i) in offsets.iter_mut().zip(0..2) {
        let Some(slot) = solver.surface(i)? else {
            return Ok(None);
        };
        *offset = slot.surface.offset_distance();
    }
    let (Some(&s_face), Some(&offset)) = (faces.get(rail), offsets.get(rail)) else {
        return Ok(None);
    };
    // E: from the corner's vertex to W, between S and P
    let Some(e) = eu1::edge(target, side)? else {
        return Ok(None);
    };
    if e == filleted || edge::is_closed(target, e)? {
        return Ok(None);
    }
    let (start, end) = (edge::start_vertex(target, e)?, edge::end_vertex(target, e)?);
    let far = if start == Some(vertex) {
        end
    } else if end == Some(vertex) {
        start
    } else {
        None
    };
    let Some(far) = far else {
        return Ok(None);
    };
    let e_faces = edge::faces(target, e, None)?;
    if e_faces.len() != 2 || !e_faces.contains(&s_face) {
        return Ok(None);
    }
    let Some(&p_face) = e_faces.iter().find(|&&f| f != s_face) else {
        return Ok(None);
    };
    // P a plane too: R, on E's line, is then where the rail meets P's surface
    let p_planar = match target.face(p_face)?.surface.as_ref() {
        Some(surface) => surface.is_planar(k, tol)?.is_planar,
        None => false,
    };
    if !p_planar {
        return Ok(None);
    }
    // the rail in closed form, and E's line
    let Some(section) = planar_section(k, target, filleted, faces, offsets)? else {
        return Ok(None);
    };
    let Some(contact) = section.contact(rail, offset) else {
        return Ok(None);
    };
    let Some(e_curve) = edge::curve(target, e)?.cloned() else {
        return Ok(None);
    };
    if e_curve.is_line(k, 3, CURVE_TOL)?.is_none() {
        return Ok(None);
    }
    let origin = crate::topology::vertex::point(target, vertex)?;
    let w = crate::topology::vertex::point(target, far)?;
    let Ok(u) = (w - origin).unitize() else {
        return Ok(None);
    };
    // R: where the rail's line, contact + s d, crosses E's line, origin + l u (both on S's
    // plane); the two lines' nearest points, which must meet
    let d = section.d;
    let (a, b) = (d.dot(u), contact - origin);
    let det = 1.0 - a * a;
    if det.is_nan() || det <= DIR_TOL {
        return Ok(None);
    }
    let l = (b.dot(u) - a * b.dot(d)) / det;
    let s = (a * b.dot(u) - b.dot(d)) / det;
    let r = origin + u * l;
    let miss = (contact + d * s - r).length();
    if miss.is_nan() || miss > tol {
        return Ok(None);
    }
    let past = l - (w - origin).dot(u);
    if past.is_nan() || past <= tol {
        return Ok(None);
    }
    // N at W: between P and a face other than S, turned back along E
    let mut turned = false;
    for n in crate::topology::vertex::edges(target, far, None)? {
        if n == e || edge::is_closed(target, n)? {
            continue;
        }
        let n_faces = edge::faces(target, n, None)?;
        if !n_faces.contains(&p_face) || n_faces.contains(&s_face) {
            continue;
        }
        let Some(tn) = tangent_leaving(k, target, n, far)? else {
            continue;
        };
        if turns_back(u, tn) {
            turned = true;
            break;
        }
    }
    if !turned {
        return Ok(None);
    }
    // R on S
    let surface = need(
        target.face(s_face)?.surface.as_ref(),
        "side face without surface",
    )?
    .clone_for_evaluation()?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<crate::surface::surface::p4::NoObject>(
        k,
        r,
        &domain,
        &crate::surface::surface::p5::DropPointOptions::default(),
    )?;
    if !dropped.success {
        return Ok(None);
    }
    let mut class = crate::intersect::point_class::PointClassification::<EntityRef>::new(tol);
    crate::query::face::p2::point_classify(
        k,
        target,
        s_face,
        dropped.result_uv,
        &mut class,
        crate::query::face::p2::PointClassifyOptions {
            src_zone_tol_3d: tol,
            do_boundary_intersections: true,
            use_uv_classification: true,
            ..crate::query::face::p2::PointClassifyOptions::default()
        },
    )?;
    if class.point_class() != PointClassType::Face {
        return Ok(None);
    }
    // the cap edge from R to W on S: no boundary edge of S crosses it
    let normal = surface.evaluate_normal(
        k,
        dropped.result_uv,
        crate::nurbs::knots::Side::Left,
        crate::nurbs::knots::Side::Left,
    )?;
    let Ok(across) = normal.cross(u).unitize() else {
        return Ok(None);
    };
    let frame = SegmentFrame {
        w,
        along: u,
        across,
        length: (r - w).dot(u),
        tol,
    };
    for edge_id in crate::topology::face::p4::edges(target, s_face, None, false, None)? {
        if !edge_clear_of_segment(k, target, edge_id, far, &frame)? {
            return Ok(None);
        }
    }
    Ok(Some(TurnedBack {
        far,
        edge: e,
        face: s_face,
    }))
}

/// The open segment from W to R on S's plane, in its own frame: `along` from W towards R,
/// `across` square to it in the plane, `length` its length, `tol` the body's tolerance.
struct SegmentFrame {
    w: Point3,
    along: Vec3,
    across: Vec3,
    length: f64,
    tol: f64,
}

impl SegmentFrame {
    /// `p` in the frame: along and across the segment from W.
    fn coords(&self, p: Point3) -> (f64, f64) {
        let v = p - self.w;
        (v.dot(self.along), v.dot(self.across))
    }
}

/// Whether the boundary edge `edge_id` of S is proved not to cross the open segment of
/// `frame` (from W, the vertex `far`, to R), with the points in `frame`'s coordinates (a
/// along, h across) and `err` the rounding of each coordinate: `gamma(16)` of the largest
/// coordinate involved (the subtraction, the three-term dot product, and the quotient by a
/// weight), plus, for a B-spline whose edge is a part of its NURB, the extraction's: its knot
/// insertions leave each homogeneous coefficient within `gamma(16 (p + 1))` of the NURB's
/// largest (as [`crate::query::distance_bound`] counts them), carried through the quotient by
/// the smallest weight less that error.
/// - An edge with no end at W must keep clear by more than the tolerance plus `err`: its
///   points (a line's ends, a B-spline's Euclidean control points, which hold its curve in
///   their hull where the weights are positive) all lie before W, all past R, or all on one
///   side of the segment's line, each by more than that.
/// - An edge with an end at W must lie in one closed half-plane of the segment's line,
///   within `err` (every point's `h >= -err`, or every point's `h <= err`): it then never
///   reaches the other side by more than `err`, so it does not cross the segment. It may
///   touch the line h = 0 (within `err`) anywhere along the segment, not only at W; touching
///   is not crossing, and that is all this proves. A line from W running along the segment's
///   line toward R (its far end within the tolerance of the line, ahead of W) lies on the
///   segment and is not clear.
///
/// A curve of another kind, a 2D or non-finite net, or a weight not proved positive is not
/// proved.
///
/// # Errors
/// Those of the topology queries, the recogniser, the extraction and the evaluations.
fn edge_clear_of_segment(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    far: VertexId,
    frame: &SegmentFrame,
) -> Result<bool> {
    use crate::curve::curve::CurveKind;
    use crate::nurbs::math::{gamma, NOW, NOZ};
    let Some(curve) = edge::curve(target, edge_id)?.cloned() else {
        return Ok(false);
    };
    let ivl = edge::interval(target, edge_id)?;
    let at_w = edge::start_vertex(target, edge_id)? == Some(far)
        || edge::end_vertex(target, edge_id)? == Some(far);
    // the extraction's error on the homogeneous coefficients, and the smallest weight
    let mut extract_err = 0.0_f64;
    let mut w_min = f64::INFINITY;
    let points: Vec<Point3> = if curve.is_line(k, 3, CURVE_TOL)?.is_some() {
        vec![
            curve.evaluate_point(k, ivl.min())?,
            curve.evaluate_point(k, ivl.max())?,
        ]
    } else {
        let CurveKind::BSpline(base) = curve.kind() else {
            return Ok(false);
        };
        let Some(nurb) = base.nurb.as_ref() else {
            return Ok(false);
        };
        let (Some(&first), Some(&last)) = (nurb.knots().first(), nurb.knots().last()) else {
            return Ok(false);
        };
        let (lo, hi) = (ivl.min(), ivl.max());
        if !(first <= lo && lo < hi && hi <= last) {
            return Ok(false);
        }
        let segment = if first < lo || hi < last {
            let largest = nurb
                .cpts()
                .iter()
                .flat_map(|c| {
                    let wt = if c.w == NOW { 1.0 } else { c.w };
                    [c.x.abs(), c.y.abs(), c.z.abs(), wt.abs()]
                })
                .fold(0.0_f64, f64::max);
            extract_err = gamma(16 * (nurb.degree() + 1)) * largest;
            nurb.extract_seg(lo, hi)?
        } else {
            nurb.clone()
        };
        let mut out = Vec::new();
        for c in segment.cpts() {
            if c.z == NOZ {
                return Ok(false);
            }
            let wt = if c.w == NOW { 1.0 } else { c.w };
            // weights proved positive keep the curve in the hull
            let finite = c.x.is_finite() && c.y.is_finite() && c.z.is_finite();
            if !(finite && crate::geom::certified::above(wt - extract_err, 0.0)) {
                return Ok(false);
            }
            w_min = w_min.min(wt);
            out.push(Point3::new(c.x / wt, c.y / wt, c.z / wt));
        }
        out
    };
    let Some(&last_point) = points.last() else {
        return Ok(false);
    };
    let size = points
        .iter()
        .chain(std::iter::once(&frame.w))
        .flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()])
        .fold(frame.length.abs(), f64::max);
    if !size.is_finite() {
        return Ok(false);
    }
    // the extraction's error carried through the quotient by a weight (as
    // `RailPiece::ball` widens it), then the frame's own rounding
    let carried = if extract_err > 0.0 {
        extract_err * (1.0 + size) / (w_min - extract_err)
    } else {
        0.0
    };
    let err = gamma(16) * size + carried;
    if !err.is_finite() {
        return Ok(false);
    }
    let coords: Vec<(f64, f64)> = points.iter().map(|&p| frame.coords(p)).collect();
    let clear = frame.tol + err;
    if at_w {
        if curve.is_line(k, 3, CURVE_TOL)?.is_some() {
            // the line's end away from W
            let first_point = points.first().copied().unwrap_or(last_point);
            let away = if first_point.distance(frame.w) > last_point.distance(frame.w) {
                first_point
            } else {
                last_point
            };
            let (a, h) = frame.coords(away);
            if !(a.is_finite() && h.is_finite()) {
                return Ok(false);
            }
            // along the segment's line, ahead of W: on the segment
            if h.abs() <= clear && a > 0.0 {
                return Ok(false);
            }
            return Ok(true);
        }
        let above = coords.iter().all(|&(_, h)| h >= -err);
        let below = coords.iter().all(|&(_, h)| h <= err);
        return Ok(above || below);
    }
    let all = |f: &dyn Fn(f64, f64) -> bool| coords.iter().all(|&(a, h)| f(a, h));
    Ok(all(&|a, _| a < -clear)
        || all(&|a, _| a > frame.length + clear)
        || all(&|_, h| h > clear)
        || all(&|_, h| h < -clear))
}

/// The tolerance [`rail_passes_side_edge_end`] recognises a side edge's curve as a line
/// within, as [`crate::blend::fillet_tangent_rollover`] recognises the filleted edge's.
const CURVE_TOL: f64 = 1.0e-8;

/// The sine squared below which [`rail_passes_side_edge_end`] takes the rail and the side edge
/// as parallel (no crossing).
const DIR_TOL: f64 = 1.0e-9;

/// Not upstream (solidcore-native, [`TurnedBack`]): whether N's tangent `tn` leaving T's vertex
/// points back along E's tangent `te` arriving there: within 45 degrees of antiparallel
/// ([`TURNED_BACK_COS`]), `te·tn <= -cos(45°) |te| |tn|`, and at a negative dot product. A
/// tangent only past square to E is not turned back. A zero vector, and NaN, do not turn back:
/// every comparison with NaN fails.
///
/// Why 45 degrees, and not the tangency tolerance: E and N lie on P∩S and P∩X, so they run
/// along `nP × nS` and `nP × nX` (n the surfaces' normals at the vertex). T is smooth when nX
/// lies within δ (the tangency tolerance) of nS, and the angle β between the two lines in P is
/// then the angle between the projections of nS and nX onto P. With φ the angle between nP
/// and nS, the worst direction of the offset (part square to the nP, nS plane, part tilted
/// towards nP, which also shrinks the projection) gives
/// `tan β = sin δ / sqrt(sin²φ cos²δ - cos²φ sin²δ)`, so β can exceed δ many times over where
/// P meets S at a shallow angle (δ 0.02, φ 20°: β 0.0585). A turned-back N lies at β from
/// antiparallel and a continuing one at β from parallel, so the classes divide at square;
/// 45 degrees is the line midway between antiparallel and square, which every
/// tolerance-tangent input with `sin φ >= tan δ sqrt(1 + cos²φ)` (about `√2 tan δ`: φ above
/// 2.43° at δ 0.03) meets (`β <= 45°`), with the edge curves' own fitting error taken out of
/// the remaining margin. Below that, a T counted smooth may be classed not turned back: the
/// vertex then takes the type it had before the turned-back case was recognised. Rounding of
/// the dot product and the lengths (a few ulps relative) moves the decision only within
/// ~1e-15 rad of 45 degrees.
fn turns_back(te: Vec3, tn: Vec3) -> bool {
    let d = te.dot(tn);
    d < 0.0 && d <= -(TURNED_BACK_COS * te.length() * tn.length())
}

/// The cosine of the largest angle from antiparallel, 45 degrees (midway between antiparallel
/// and square), at which N's tangent counts as turned back along E's ([`turns_back`]).
const TURNED_BACK_COS: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Not upstream (solidcore-native, [`TurnedBack`]): the tangent of the edge `edge` leaving its
/// end `vertex`, along its curve away from it (the curve's tangent at its start, reversed at
/// its end); `None` where `vertex` is neither end. For an open edge: a closed one leaves its
/// vertex both ways.
///
/// # Errors
/// Those of the topology walks and the evaluation.
pub(crate) fn tangent_leaving(
    k: &Kernel,
    target: &Brep,
    edge: EdgeId,
    vertex: VertexId,
) -> Result<Option<Vec3>> {
    if edge::start_vertex(target, edge)? == Some(vertex) {
        Ok(Some(crate::topology::vertex::edge_end_tangent(
            k, target, edge, true,
        )?))
    } else if edge::end_vertex(target, edge)? == Some(vertex) {
        Ok(Some(-crate::topology::vertex::edge_end_tangent(
            k, target, edge, false,
        )?))
    } else {
        Ok(None)
    }
}

/// A fillet vertex on the original vertex `tb.far` (`SM_FV_ON_VERTEX`), for a [`TurnedBack`]
/// cap edge.
fn turned_back_vertex(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    corner: FilletCornerId,
    tb: TurnedBack,
) -> Result<VertexId> {
    let fv = pseudo.brep_mut().add_unlinked_vertex_record(Vertex {
        point: Point3::ZERO,
        tolerance: 0.0,
        ..Vertex::default()
    })?;
    pseudo.insert_fillet_vertex(fv, FilletVertex::new(k, Some(corner)));
    let record = pseudo.fillet_vertex_mut(fv)?;
    record.set_point_class(PointClassType::Vertex, Some(EntityRef::Vertex(tb.far)));
    record.set_fillet_vertex_type(FilletVertexType::OnVertex);
    Ok(fv)
}

/// The live objects of the end of `SmFilletNx1Corner::MakeCornerTopology`.
pub(crate) struct Nx1End<'a> {
    pub k: &'a Kernel,
    pub target: &'a mut Brep,
    pub pseudo: &'a mut FilletBrep,
    pub corner: FilletCornerId,
    pub solver: &'a mut SolverObject,
    pub extended_surfaces: &'a mut BTreeMap<FaceId, Surface>,
    pub topo_edges: &'a mut Vec<Option<FilletObject>>,
    pub geometry: FilletGeomId,
    pub filleted: EdgeuseId,
    pub vertex: VertexId,
    pub total: usize,
    pub mixed: bool,
    pub same: [bool; 2],
    /// `bTangentSide` (the fork's tangency-labs/smlib@856b54e): each side edge that is a
    /// tangent seam the rail on its side crosses; `false` without WR-600's fix.
    pub tangent_side: [bool; 2],
    pub sides: [EdgeuseId; 2],
    pub end_face: Option<FaceId>,
    pub extend_edge: Option<EdgeId>,
    /// Not upstream: each rail rolled onto a face whose foot edge turns back at the rolled-over
    /// edge's vertex ([`TurnedBack`]); `None` for every other rail.
    pub turned_back: [Option<TurnedBack>; 2],
}

/// The gap checks `SmFilletNx1Corner::MakeCornerTopology` ends with when the end face
/// of a mixed-convexity corner connects to more than the fillet surface and one other
/// face: `CalcCornerVertGeom`, `CalcCornerEdgeGeom` of the two new side edges (statuses
/// ignored) and, when the end face's extended surface was kept, its
/// `SimpleCoincidenceChecker` against the other side edge, clearing
/// `m_bExtendedSurfacePatch` when they are not coincident. The caller runs them right
/// after the topology returns, before anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nx1GapCheck {
    /// `pNewFilletEdge1` (rail-1 vertex side, along `pEU[0]`).
    pub edge1: EdgeId,
    /// `pNewFilletEdge2` (rail-2 vertex side, along `pEU[1]`).
    pub edge2: EdgeId,
    /// The end face whose new extended surface stayed in the extended-surface map
    /// (`pNewExtendSurface` not NULL), if any.
    pub extended_face: Option<FaceId>,
    /// `pOtherSideEdgeuse`.
    pub other_side_edgeuse: Option<EdgeuseId>,
}

fn pseudo_edge(edge: EdgeId) -> FilletObject {
    FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge))
}

impl FilletCorner {
    /// From the extension distance on: the end rib, its end face, the extended end
    /// surface and, at a mixed-convexity corner, the vertex on the original vertex and
    /// the two side edges joining it to the rail vertices; the gap checks of a
    /// multi-neighbour end face are returned for the caller.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub(crate) fn finish_nx1_end(&mut self, env: Nx1End<'_>) -> Result<Option<Nx1GapCheck>> {
        let Nx1End {
            k,
            target,
            pseudo,
            corner,
            solver,
            extended_surfaces,
            topo_edges,
            geometry,
            filleted,
            vertex,
            total,
            mixed,
            same,
            tangent_side,
            sides,
            mut end_face,
            mut extend_edge,
            turned_back,
        } = env;

        // compute how much to extend the endSurface to make sure it covers the 'hole'
        let mut ext_dist = if mixed {
            find_surface_extension_dist_at_corner(target, filleted, solver, vertex)?
        } else {
            0.0
        };

        let start = self.vertices.first().copied();
        let end = self.vertices.get(1).copied();
        let rib = pseudo.make_fillet_edge(start, end, Some(corner))?;
        pseudo
            .fillet_edge_mut(rib)?
            .set_fillet_edge_type(FilletEdgeType::FilletXSideFace);
        let primary = need(
            eu1::primary_edgeuse(pseudo.brep(), rib)?,
            "end rib without primary edge use",
        )?;
        pseudo.set_edgeuse_fillet_geom(primary, Some(geometry))?;
        self.edges.push(rib);

        // In the mixed-cvxty case, pEndFace will be NULL when we arrive here
        // if both side edges have opposite convexity to the filleted edge.
        let mut multi_neighbor_end_face = false;
        let mut other_side_edgeuse = None;
        if end_face.is_none() {
            let [first, second] = sides;
            end_face = face_by_two_edgeuses(target, Some(first), Some(second), None)?;
            if end_face.is_none() {
                multi_neighbor_end_face = true;
                if total > 3 {
                    // Use the side edge that is a cliff edge.
                    let (pick, other) = if same[0] {
                        (first, second)
                    } else if same[1] {
                        (second, first)
                    } else if !face_holds_edge(k, target, first, second, self.approx_tol_3d)?
                        && face_holds_edge(k, target, second, first, self.approx_tol_3d)?
                    {
                        // Both are cliff edges. The cap lies on the end face's surface
                        // between the end rib and both side edges, so the end face is the
                        // one whose surface holds the other side edge (the gap check asks
                        // that of its extension). Upstream picks the first, which made an
                        // End beside two faces meeting tangentially (a bore's flat wall
                        // and its round bottom) depend on which of them came first round
                        // the vertex: the mirror image built and the part did not (#15768).
                        (second, first)
                    } else {
                        (first, second)
                    };
                    end_face = eu1::face(target, pick)?;
                    extend_edge = eu1::edge(target, pick)?;
                    other_side_edgeuse = Some(other);
                } else {
                    // Invalid brep topology
                    return Err(SolidError::blend_failed(
                        OP,
                        "the brep's topology at the corner is not valid",
                    ));
                }
            }
        }
        pseudo.fillet_edge_mut(rib)?.set_original_face(end_face);

        // compute how much to extend the sideSurface to make sure it covers the 'hole'
        let mut do_extend = extend_edge.is_some();
        if ext_dist > EFF_ZERO && !do_extend && matches!(solver, SolverObject::ConstantRadius(_)) {
            ext_dist *= 0.1; // use only a small extension
            do_extend = true; // for constant radius only
        }

        // When a surface extension is needed to define the endFace shape,
        // build and save it.
        let mut new_extend_face = None;
        if do_extend {
            let face = need(end_face, "extended end face is null")?;
            let surface = need(
                target.face(face)?.surface.as_ref(),
                "end face without surface",
            )?;
            // create an extended surface large enough to fill in the endCap requirements
            // At a tangent end the end face is continued as itself: the end rib's shape
            // there depends on how its curvature differs from the rail's face across the
            // seam, which a linear (G1) extension would not keep (tangency-labs/smlib@856b54e).
            let extension = if mixed && (tangent_side[0] || tangent_side[1]) {
                crate::surface::extend::Continuity::Natural
            } else {
                crate::surface::extend::Continuity::Tangent
            };
            let new_surface = surface
                .extended_sides(k, &crate::surface::side::BoxSide::ALL, extension, ext_dist)?
                .surface;
            // Copy pNewSurface, when possible as an analytic surface
            let new_extend_surface = new_surface.copy_and_add_analytics(k)?;
            // If AddExtendedOriginalSurface() deleted pNewExtendSurface, then
            // pEndFace already had an extended surface, and we don't have to
            // deal with it anymore.
            if Self::add_extended_original_surface_with_map(
                extended_surfaces,
                face,
                new_extend_surface,
            ) {
                new_extend_face = Some(face);
            }
        }

        // When adding an endSurface to fill the 'hole',
        // set up the topology for the end cap.
        if mixed {
            // add a filletVertex at the origVertex location
            let fv = pseudo.brep_mut().add_unlinked_vertex_record(Vertex {
                point: Point3::ZERO,
                tolerance: 0.0,
                ..Vertex::default()
            })?;
            pseudo.insert_fillet_vertex(fv, FilletVertex::new(k, Some(corner)));
            pseudo
                .fillet_vertex_mut(fv)?
                .set_point_class(PointClassType::Vertex, Some(EntityRef::Vertex(vertex)));
            pseudo
                .fillet_vertex_mut(fv)?
                .set_fillet_vertex_type(FilletVertexType::OnVertex);
            self.vertices.push(fv);

            let v = |i: usize| -> Result<VertexId> {
                need(self.vertices.get(i).copied(), "corner vertex index")
            };
            let (v0, v1, v2) = (v(0)?, v(1)?, v(2)?);

            let rib_index = self.edges.len().saturating_sub(1);
            // Not upstream: a rail rolled onto a face whose foot edge turns back at the rolled-over
            // edge's vertex ends inside that face; the cap's edge from it runs along the foot
            // edge's continuation to that vertex, then along the side edge (`TurnedBack`)
            self.turned_back_arcs = [None, None];
            let edge2 = if let Some(tb) = turned_back[1] {
                let f = turned_back_vertex(k, pseudo, corner, tb)?;
                self.vertices.push(f);
                let arc = pseudo.make_fillet_edge(Some(v1), Some(f), Some(corner))?;
                let fe = pseudo.fillet_edge_mut(arc)?;
                fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                fe.set_original_edge(Some(tb.edge));
                fe.set_original_face(Some(tb.face));
                self.edges.push(arc);
                *need(self.turned_back_arcs.get_mut(1), "rail index")? = Some(arc);
                let line = pseudo.make_fillet_edge(Some(f), Some(v2), Some(corner))?;
                pseudo
                    .fillet_edge_mut(line)?
                    .set_fillet_edge_type(FilletEdgeType::OnEdge);
                if total <= 3 {
                    topo_edges.push(Some(pseudo_edge(line)));
                }
                let original = eu1::edge(target, sides[1])?;
                pseudo.fillet_edge_mut(line)?.set_original_edge(original);
                let face1 = eu1::face(target, sides[1])?;
                pseudo.fillet_edge_mut(line)?.set_original_face(face1);
                self.edges.push(line);
                line
            } else {
                // Add 1st fillet edge from expected rail2/sideEdge2 intersection to origVertex
                let edge2 = pseudo.make_fillet_edge(Some(v1), Some(v2), Some(corner))?;
                if pseudo.fillet_vertex(v1)?.fillet_vertex_type()
                    == FilletVertexType::RailXExtendedEdgeuse
                    || tangent_side[1]
                {
                    let fe = pseudo.fillet_edge_mut(edge2)?;
                    fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                    fe.set_original_edge(extend_edge);
                } else {
                    pseudo
                        .fillet_edge_mut(edge2)?
                        .set_fillet_edge_type(FilletEdgeType::OnEdge);
                    if total <= 3 {
                        // Register this edge and process it later
                        // when removing topological vertices
                        topo_edges.push(Some(pseudo_edge(edge2)));
                    }
                    let original = eu1::edge(target, sides[1])?;
                    pseudo.fillet_edge_mut(edge2)?.set_original_edge(original);
                }
                let face1 = eu1::face(target, sides[1])?;
                pseudo.fillet_edge_mut(edge2)?.set_original_face(face1);
                self.edges.push(edge2);
                edge2
            };

            let edge1 = if let Some(tb) = turned_back[0] {
                let f = turned_back_vertex(k, pseudo, corner, tb)?;
                self.vertices.push(f);
                let line = pseudo.make_fillet_edge(Some(v2), Some(f), Some(corner))?;
                pseudo
                    .fillet_edge_mut(line)?
                    .set_fillet_edge_type(FilletEdgeType::OnEdge);
                if total <= 3 {
                    topo_edges.push(Some(pseudo_edge(line)));
                }
                let original = eu1::edge(target, sides[0])?;
                pseudo.fillet_edge_mut(line)?.set_original_edge(original);
                let face0 = eu1::face(target, sides[0])?;
                pseudo.fillet_edge_mut(line)?.set_original_face(face0);
                self.edges.push(line);
                let arc = pseudo.make_fillet_edge(Some(f), Some(v0), Some(corner))?;
                let fe = pseudo.fillet_edge_mut(arc)?;
                fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                fe.set_original_edge(Some(tb.edge));
                fe.set_original_face(Some(tb.face));
                self.edges.push(arc);
                *need(self.turned_back_arcs.get_mut(0), "rail index")? = Some(arc);
                line
            } else {
                // Add 2nd fillet edge from origVertex to other expected rail1/sideEdge intersection
                let edge1 = pseudo.make_fillet_edge(Some(v2), Some(v0), Some(corner))?;
                if pseudo.fillet_vertex(v0)?.fillet_vertex_type()
                    == FilletVertexType::RailXExtendedEdgeuse
                    || tangent_side[0]
                {
                    let fe = pseudo.fillet_edge_mut(edge1)?;
                    fe.set_fillet_edge_type(FilletEdgeType::OnExtendEdge);
                    fe.set_original_edge(extend_edge);
                } else {
                    pseudo
                        .fillet_edge_mut(edge1)?
                        .set_fillet_edge_type(FilletEdgeType::OnEdge);
                    if total <= 3 {
                        topo_edges.push(Some(pseudo_edge(edge1)));
                    }
                    let original = eu1::edge(target, sides[0])?;
                    pseudo.fillet_edge_mut(edge1)?.set_original_edge(original);
                }
                let face0 = eu1::face(target, sides[0])?;
                pseudo.fillet_edge_mut(edge1)?.set_original_face(face0);
                self.edges.push(edge1);
                edge1
            };
            // The cap's face is oriented at the corner's vertex from the edges before and after
            // it (`MakeFaceBrep` reads the second and third edges): with a turned-back rail 2,
            // the loop starts after the end rib.
            if turned_back[1].is_some() {
                // the edges from the end rib on, the rib moved to their end
                let rotated: Vec<EdgeId> = self
                    .edges
                    .iter()
                    .take(rib_index)
                    .chain(self.edges.iter().skip(rib_index.saturating_add(1)))
                    .chain(self.edges.get(rib_index))
                    .copied()
                    .collect();
                self.edges = rotated;
            }

            // When endFace connects to more than the filletSurface and 1 other Face,
            // check for gaps
            if multi_neighbor_end_face {
                return Ok(Some(Nx1GapCheck {
                    edge1,
                    edge2,
                    extended_face: new_extend_face,
                    other_side_edgeuse,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::turns_back;
    use crate::math::Vec3;

    /// The turn-back test takes N's tangent within 45 degrees of E's reversed, and nothing
    /// short of it: a tangent merely past square to E (a negative dot product, the old test) is
    /// not turned back. Zero vectors and NaN are not either.
    #[test]
    fn turning_back_needs_to_be_nearer_antiparallel_than_square() {
        let te = Vec3::new(2.0, 0.0, 0.0);
        let at = |deg: f64| {
            let t = deg.to_radians();
            Vec3::new(-3.0 * t.cos(), 3.0 * t.sin(), 0.0)
        };
        // straight back, and either side of 45 degrees
        assert!(turns_back(te, at(0.0)));
        assert!(turns_back(te, at(44.9)));
        assert!(!turns_back(te, at(45.1)));
        // a tangent only past square (dot < 0): the old test took it
        assert!(te.dot(at(80.0)) < 0.0);
        assert!(!turns_back(te, at(80.0)));
        // along E, square to it, zero and NaN
        assert!(!turns_back(te, -at(0.0)));
        assert!(!turns_back(te, at(90.0)));
        assert!(!turns_back(te, Vec3::new(0.0, 0.0, 0.0)));
        assert!(!turns_back(Vec3::new(0.0, 0.0, 0.0), at(0.0)));
        let nan = Vec3::new(f64::NAN, f64::NAN, f64::NAN);
        assert!(!turns_back(te, nan));
        assert!(!turns_back(nan, at(0.0)));
    }

    /// E and N at a T tangent only within the tolerance: P = z = 0 meets S (normal at `phi`
    /// from P's) along E, and X along N, turned back. X's normal is `delta` off S's in the
    /// direction at `theta` from the one square to the nP, nS plane, towards the one in that
    /// plane (which tilts it towards or away from nP).
    fn tolerance_tangent(phi: f64, delta: f64, theta: f64) -> (Vec3, Vec3) {
        let n_p = Vec3::new(0.0, 0.0, 1.0);
        let n_s = Vec3::new(phi.sin(), 0.0, phi.cos());
        let square = Vec3::new(0.0, 1.0, 0.0);
        let in_plane = Vec3::new(phi.cos(), 0.0, -phi.sin());
        let off = square * theta.cos() + in_plane * theta.sin();
        let n_x = n_s * delta.cos() + off * delta.sin();
        (n_p.cross(n_s), -n_p.cross(n_x))
    }

    /// `sin phi / (tan delta sqrt(1 + cos² phi))`: at least 1 where every offset direction
    /// keeps N within 45 degrees of antiparallel (the worst case's closed form).
    fn margin(phi: f64, delta: f64) -> f64 {
        phi.sin() / (delta.tan() * (1.0 + phi.cos() * phi.cos()).sqrt())
    }

    /// The offset directions swept: 720 about S's normal.
    fn thetas() -> impl Iterator<Item = f64> {
        (0..720).map(|i| f64::from(i) * std::f64::consts::TAU / 720.0)
    }

    /// The review's case: tangency tolerance 0.03, X's normal 0.02 off S's at the vertex, P
    /// meeting S at 20 degrees. N lies 0.058 off antiparallel, past the tolerance (so a test
    /// at the tolerance would fall back to the vertex type #16027 replaced), and is turned
    /// back. So is every tolerance-tangent T on the safe side of the bound
    /// `sin phi >= tan delta sqrt(1 + cos² phi)`, in every swept offset direction, and an
    /// exactly tangent one at any phi; the same geometry continuing (N reversed) is not.
    /// Just inside the bound some direction is not turned back: the old vertex type.
    #[test]
    fn a_t_tangent_only_within_tolerance_still_turns_back() {
        let (te, tn) = tolerance_tangent(20.0_f64.to_radians(), 0.02, 0.0);
        let off = (-te.dot(tn) / (te.length() * tn.length())).acos();
        assert!(off > 0.03 && (off - 0.0585).abs() < 1e-3, "{off}");
        assert!(turns_back(te, tn));
        assert!(!turns_back(te, -tn));
        for delta in [0.0, 1e-6, 0.01, 0.03, 0.1] {
            for phi_deg in [1.0, 2.5, 5.0, 9.0, 20.0, 45.0, 90.0, 135.0, 175.0] {
                let phi = f64::to_radians(phi_deg);
                if margin(phi, delta) < 1.001 {
                    continue;
                }
                for theta in thetas() {
                    let (te, tn) = tolerance_tangent(phi, delta, theta);
                    assert!(
                        turns_back(te, tn),
                        "delta {delta}, phi {phi_deg}, theta {theta}"
                    );
                    assert!(
                        !turns_back(te, -tn),
                        "delta {delta}, phi {phi_deg}, theta {theta}"
                    );
                }
            }
        }
        // at the bound and just inside it (phi 2.43 degrees at delta 0.03)
        for delta in [0.03_f64, 0.1] {
            let bound = |m: f64| {
                // sin phi = m tan delta sqrt(1 + cos² phi), solved for sin² phi
                let t2 = (m * delta.tan()).powi(2);
                (t2 * 2.0 / (1.0 + t2)).sqrt().asin()
            };
            let outside = bound(1.001);
            assert!((margin(outside, delta) - 1.001).abs() < 1e-9);
            assert!(thetas().all(|theta| {
                let (te, tn) = tolerance_tangent(outside, delta, theta);
                turns_back(te, tn)
            }));
            let inside = bound(0.99);
            assert!(thetas().any(|theta| {
                let (te, tn) = tolerance_tangent(inside, delta, theta);
                !turns_back(te, tn)
            }));
        }
    }
}
