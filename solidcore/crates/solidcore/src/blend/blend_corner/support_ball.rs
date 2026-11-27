//! The blend corner's support change: three equal convex rounds at a vertex of four faces,
//! the fourth edge there not blended, a straight tangent seam between a plane W and a
//! cylinder Y. Round `B_W` rolls on the floor G (a plane square to Y's axis) and W, round `B_G`
//! on G and the face H, round `B_Y` on H and Y. `B_W`'s rails on G and on `B_Y`'s side run apart
//! (`B_G`'s ball touches H where `B_W`'s touches W), so the rounds' rails on G never cross: the
//! ball rolling on G and W rolls on across the seam onto Y, on G and Y, until it touches H
//! too. There one ball touches G, H and Y, as at a Ball corner of three faces.
//!
//! The corner is two faces. `B_W` ends square at the seam, on its section through the seam's
//! line; from there the ball, touching G and Y, sweeps a torus about Y's axis (major radius
//! the centre's distance from the axis, minor radius the rounds' radius), up to the ball C
//! that touches G, H and Y: the torus is the round of G and Y, which share no edge. `B_G` and
//! `B_Y` end on their sections through C, and the corner's sphere closes the triangle on C's
//! ball between its contacts with G, H and Y, as the Ball does ([`super::ball`]). Every face
//! is exact: the torus a rational biquadratic, the sphere's patch the Ball's.
//!
//! Upstream: none (pinned SMLib builds this vertex as a 4x3 corner, whose vertex on G is
//! where the rails of `B_W` and `B_G` cross: they do not, so it is refused; #19089). The
//! construction follows the generic blend's events (`docs/generic-blend.md` §3.2): the
//! torus is the round of a support change across the tangent seam, the sphere the Ball.
//!
//! The corner's topology in the executive's pseudo B-rep, for the ring at the vertex: on H
//! the vertex `Pc` where `B_G`'s and `B_Y`'s rails cross (`SM_FV_RAIL_X_RAIL`, the ball C's
//! contact with H); `Pb` and `Py`, the other ends of their sections through C, on G and on Y
//! (`SM_FV_PRECOMPUTED`, placed in closed form); `Vs`, where `B_W`'s rail on W crosses the
//! seam (`SM_FV_RAIL_X_EDGEUSE`), and its mate `Vb` on G. Edges: `Vs-Vb`, `B_W`'s end
//! section; `Vb-Pb` on G and `Py-Vs` on Y, the torus's contacts; `Pb-Pc` and `Pc-Py`, the
//! end sections of `B_G` and `B_Y`; and `Pb-Py`, the arc the torus and the sphere share.
//!
//! | Rust                                  | What                                              |
//! |---------------------------------------|---------------------------------------------------|
//! | [`SupportRing`], [`SupportGeometry`]  | the ring's faces and rounds, the balls in closed form |
//! | [`classify_support_ball`]             | whether a 4x3 vertex is the support change         |
//! | [`support_ball_corner_of`]            | the blend corner that builds it                    |
//! | `FilletCorner::make_support_ball_topology` (crate) | its pseudo topology                   |
//! | [`place_support_ball_vertices`]       | its vertices at the balls' contacts                |
//! | [`support_ball_edge_geom`]            | the torus, the sphere's patch and the edges on them |
//! | [`create_support_ball_patches`]       | the two faces                                     |
//!
//! Deviations: none (not a port). No recursion.

use crate::blend::blend_corner::ball::{
    bad_input, bad_setup, ball_triangle_face, defect, edge_use_without_edge, face_without_surface,
    missing_defect, solver_input, solver_of_edgeuse_or_mate, unbuildable, unbuildable_missing,
    BallSolvers,
};
use crate::blend::blend_corner::{BlendCorner, BlendCornerEvent, BlendEnd, BlendTurn};
use crate::blend::fillet_corner::p1::{face_by_two_edgeuses, nurbs_curve};
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner, FilletCornerKind};
use crate::blend::fillet_corner::topology::{find_side_edgeuse, new_corner_vertex};
use crate::blend::fillet_custom_solver::{offset_local_point_solve, offset_natural_uv_domain};
use crate::blend::fillet_exact::{BlendQuadric, QuadricKind};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletGeomId,
    FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverType, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::blend::{CornerInput, CornerStep, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Result};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
use crate::surface::procedural_blend::corner::CornerDefinition;
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::support_ball::classify_support_ball";
const OP_TOPOLOGY: &str = "blend::blend_corner::support_ball::make_support_ball_topology";
const OP_PLACE: &str = "blend::blend_corner::support_ball::place_support_ball_vertices";
const OP_BUILD: &str = "blend::blend_corner::support_ball::support_ball_edge_geom";
const OP_PATCH: &str = "blend::blend_corner::support_ball::create_support_ball_patches";

/// Why a corner without its support change's members is the kernel's fault.
const NO_EVENT: &str = "the support change's members are set with its event, when the corner \
     is classified, before its topology is made";

/// Why a corner whose vertices or edges are not the topology's is the kernel's fault.
const NOT_ITS_TOPOLOGY: &str = "the support change's topology makes five vertices and six \
     edges, in its order, and nothing else changes them";

/// The ring of a support change at its vertex: its four faces and its edge uses (outward
/// from the vertex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportRing {
    /// G: the plane square to the cylinder's axis that the torus rolls on.
    pub floor: FaceId,
    /// H: the face between `B_G` and `B_Y`.
    pub between: FaceId,
    /// W: the plane running tangent into the cylinder along the seam.
    pub wall: FaceId,
    /// Y: the cylinder.
    pub cylinder: FaceId,
    /// The seam between W and Y, not blended.
    pub seam: EdgeuseId,
    /// `B_W`'s edge, between G and W.
    pub floor_wall: EdgeuseId,
    /// `B_G`'s edge, between G and H.
    pub floor_between: EdgeuseId,
    /// `B_Y`'s edge, between H and Y.
    pub between_cylinder: EdgeuseId,
    /// The solvers of `B_W`, `B_G` and `B_Y`.
    pub solvers: [FilletSolverId; 3],
}

/// The balls of a support change in closed form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupportGeometry {
    /// The rounds' radius.
    pub radius: f64,
    /// The torus's centre: where the balls' centres' plane meets the cylinder's axis.
    pub origin: Point3,
    /// The cylinder's unit axis.
    pub axis: Vec3,
    /// The balls' centres' distance from the axis (the torus's major radius).
    pub major: f64,
    /// The cylinder's radius.
    pub cylinder_radius: f64,
    /// `+1` where the cylinder's normal out of the material points away from its axis,
    /// `-1` where toward it.
    pub cylinder_out: f64,
    /// G's unit normal out of the material.
    pub floor_out: Vec3,
    /// C0: the centre of `B_W`'s ball on its section through the seam's line.
    pub seam_centre: Point3,
    /// C0's contact with G.
    pub seam_on_floor: Point3,
    /// C0's contact with W (on the seam).
    pub seam_on_wall: Point3,
    /// C: the centre of the ball touching G, H and Y.
    pub centre: Point3,
    /// C's contact with G.
    pub on_floor: Point3,
    /// C's contact with H.
    pub on_between: Point3,
    /// C's contact with Y.
    pub on_cylinder: Point3,
}

/// A face of the corner until its patch takes it: its surface and its natural loop.
#[derive(Debug, Clone)]
pub struct SupportPatch {
    /// The surface (the torus, or the sphere's patch).
    pub surface: Surface,
    /// Its loop's curves.
    pub curves_3d: Vec<Curve>,
    /// Their uv curves.
    pub uv_curves: Vec<Curve>,
    /// Their orientations.
    pub orients: Vec<OrientType>,
}

/// A support change's own members on its corner.
#[derive(Debug, Clone)]
pub struct SupportBall {
    /// The ring.
    pub ring: SupportRing,
    /// The balls.
    pub geometry: SupportGeometry,
    /// The two faces, from the edge geometry until the patches take them.
    pub patches: Vec<SupportPatch>,
}

fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// The part of `v` square to the unit `axis`.
fn across(v: Vec3, axis: Vec3) -> Vec3 {
    v - axis * v.dot(axis)
}

/// The quadric `face` lies on.
fn face_quadric(k: &Kernel, b: &Brep, face: FaceId, tol: f64) -> Result<BlendQuadric> {
    let surface =
        face2::surface(b, face)?.ok_or_else(|| face_without_surface(OP_CLASSIFY, face))?;
    BlendQuadric::from_surface(k, surface, face2::uv_domain(b, face)?.mid(), tol)
}

/// Whether `point`, on `face`'s surface, lies in the face's trimmed domain (on its edges
/// too), not past them.
fn contact_in_face(k: &Kernel, b: &mut Brep, face: FaceId, point: Point3) -> Result<bool> {
    let zone = face2::tolerance(b, face)?;
    let mut class = PointClassification::new(zone);
    point_3d_classify(
        k,
        b,
        face,
        point,
        zone,
        true,
        &mut class,
        Point3dClassifyOptions::default(),
    )?;
    Ok(class.point_class() != PointClassType::Unknown)
}

/// The radius the rounds `(radius, tolerance)` share and the least tolerance: each radius
/// within that tolerance of the first, the radius placement measures every rail against
/// (neighbours within the tolerance of each other can drift apart by more). `None` for
/// rounds of different radii or no rounds.
fn common_radius(rounds: &[(f64, f64)]) -> Option<(f64, f64)> {
    let &(first, _) = rounds.first()?;
    let mut tol = f64::MAX;
    for &(_, t) in rounds {
        tol = smin(tol, t);
    }
    for &(r, _) in rounds {
        // !(|r - first| <= tol), NaN included
        if (r - first)
            .abs()
            .partial_cmp(&tol)
            .is_none_or(std::cmp::Ordering::is_gt)
        {
            return None;
        }
    }
    Some((first, tol))
}

/// The edge of `eu`.
fn edge_of(b: &Brep, eu: EdgeuseId, op: &'static str) -> Result<EdgeId> {
    eu1::edge(b, eu)?.ok_or_else(|| edge_use_without_edge(op, eu))
}

/// A point of the offset surface by which the round of `fs` rolls on `face` (the middle of
/// its natural domain): the ball's centres' side of the face. `None` where the solver has no
/// offset there.
fn offset_point(k: &Kernel, b: &Brep, fs: &FilletSolver, face: FaceId) -> Result<Option<Point3>> {
    let rail = fs.find_index_of_rail_on_face(b, Some(face))?;
    let Some(slot) = fs.surface(rail)? else {
        return Ok(None);
    };
    let off = &slot.surface;
    let uv = off.surface.natural_uv_domain().evaluate(0.5, 0.5);
    Ok(Some(off.surface.evaluate_point(k, uv)?))
}

/// The round of solver `id` is a constant circular round of one radius on both rails, its
/// radius; `None` where it is not.
fn round_radius(solvers: &dyn BallSolvers, id: FilletSolverId) -> Result<Option<(f64, f64)>> {
    let fs = solvers.solver(id)?;
    let (Some(fsg), Some(s0), Some(s1)) = (fs.fsg.as_ref(), fs.surface(0)?, fs.surface(1)?) else {
        return Ok(None);
    };
    if fsg.fillet_surface_generator_type() != FilletSurfaceGeneratorType::Circular
        || solvers.solver_type(id)? != FilletSolverType::ConstRadius
        || solvers.offset_radii_can_change(id)?
    {
        return Ok(None);
    }
    let tol = fs.approx_tol_3d;
    let r0 = s0.surface.offset_distance().abs();
    let r1 = s1.surface.offset_distance().abs();
    if (r0 - r1).abs() > tol {
        return Ok(None);
    }
    Ok(Some((r0, tol)))
}

/// Whether the filleted `vertex` is a support change, and its ring and balls: four edges in
/// the ring (`all_eus`), three blended (`solver_eus`) with equal constant convex rounds, the
/// fourth a straight seam along which a plane W runs tangent into a cylinder Y wider than
/// the ball; the face G beside W a plane square to Y's axis, the face H past Y a plane; and
/// the ball on G and H (`B_G`'s) reaching Y on Y's side of the seam. `None` where any of that
/// does not hold (the vertex stays the 4x3 corner). `tan_tol` is the corner's tangency
/// tolerance, in radians.
///
/// # Errors
///
/// Those of the solver lookups, the face and edge queries, the convexity test and the
/// quadric recognition.
#[allow(clippy::too_many_lines)] // one classification, its steps in turn
pub fn classify_support_ball(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    tan_tol: f64,
    solvers: &dyn BallSolvers,
) -> Result<Option<(SupportRing, SupportGeometry)>> {
    if all_eus.len() != 4 || solver_eus.len() != 3 {
        return Ok(None);
    }
    let at = |i: usize| all_eus.get(i % 4).copied();
    // the one ring edge use without a solver: the seam
    let mut ids = [None; 4];
    for (slot, &eu) in ids.iter_mut().zip(all_eus) {
        *slot = solver_of_edgeuse_or_mate(b, eu, solvers)?;
    }
    let mut unblended = None;
    for (i, id) in ids.iter().enumerate() {
        if id.is_none() {
            if unblended.is_some() {
                return Ok(None);
            }
            unblended = Some(i);
        }
    }
    let Some(j) = unblended else {
        return Ok(None);
    };
    // face i lies between edge uses i - 1 and i
    let face = |b: &Brep, i: usize| -> Result<Option<FaceId>> {
        face_by_two_edgeuses(b, at(i + 3), at(i), None)
    };
    let (Some(before), Some(after)) = (face(&*b, j)?, face(&*b, j + 1)?) else {
        return Ok(None);
    };
    let tol0 = 1e-6;
    let q_before = face_quadric(k, b, before, tol0)?;
    let q_after = face_quadric(k, b, after, tol0)?;
    // (Y, eu of B_Y, eu of B_G, eu of B_W, H, G, W) in ring indices
    let (cyl_face, i_y, i_g, i_w, i_h_face, i_g_face, wall_face) =
        match (q_before.kind, q_after.kind) {
            (QuadricKind::Plane, QuadricKind::Cylinder) => {
                (after, j + 1, j + 2, j + 3, j + 2, j + 3, before)
            }
            (QuadricKind::Cylinder, QuadricKind::Plane) => {
                (before, j + 3, j + 2, j + 1, j + 3, j + 2, after)
            }
            _ => return Ok(None),
        };
    let (Some(seam), Some(eu_y), Some(eu_g), Some(eu_w)) = (at(j), at(i_y), at(i_g), at(i_w))
    else {
        return Ok(None);
    };
    let (Some(between), Some(floor)) = (face(&*b, i_h_face)?, face(&*b, i_g_face)?) else {
        return Ok(None);
    };
    let id_of = |i: usize| ids.get(i % 4).copied().flatten();
    let (Some(s_w), Some(s_g), Some(s_y)) = (id_of(i_w), id_of(i_g), id_of(i_y)) else {
        return Ok(None);
    };

    // three equal constant convex rounds
    let mut rounds = [(0.0, 0.0); 3];
    for ((id, eu), slot) in [(s_w, eu_w), (s_g, eu_g), (s_y, eu_y)]
        .into_iter()
        .zip(rounds.iter_mut())
    {
        let Some(round) = round_radius(solvers, id)? else {
            return Ok(None);
        };
        *slot = round;
        let e = edge_of(b, eu, OP_CLASSIFY)?;
        if is_concave_edge(k, b, e)? {
            return Ok(None);
        }
    }
    let Some((radius, tol)) = common_radius(&rounds) else {
        return Ok(None);
    };
    let r = radius;
    // !(r > tol), NaN included
    if r.partial_cmp(&tol) != Some(std::cmp::Ordering::Greater) {
        return Ok(None);
    }

    // the faces: G, H and W planes, Y a cylinder wider than the ball
    let q_floor = face_quadric(k, b, floor, tol)?;
    let q_between = face_quadric(k, b, between, tol)?;
    let q_wall = face_quadric(k, b, wall_face, tol)?;
    let q_cyl = face_quadric(k, b, cyl_face, tol)?;
    if q_floor.kind != QuadricKind::Plane
        || q_between.kind != QuadricKind::Plane
        || q_wall.kind != QuadricKind::Plane
        || q_cyl.kind != QuadricKind::Cylinder
        || q_cyl.radius.partial_cmp(&(r + tol)) != Some(std::cmp::Ordering::Greater)
    {
        return Ok(None);
    }
    // each face's side out of the material: away from the offset its round rolls on
    let (Some(off_floor), Some(off_wall), Some(off_between), Some(off_cyl)) = (
        offset_point(k, b, solvers.solver(s_w)?, floor)?,
        offset_point(k, b, solvers.solver(s_w)?, wall_face)?,
        offset_point(k, b, solvers.solver(s_g)?, between)?,
        offset_point(k, b, solvers.solver(s_y)?, cyl_face)?,
    ) else {
        return Ok(None);
    };
    let away = |q: &BlendQuadric, off: Point3| -> Result<Vec3> {
        let n = q.dir.unitize()?;
        Ok(if (off - q.point).dot(n) > 0.0 { -n } else { n })
    };
    let n_floor = away(&q_floor, off_floor)?;
    let n_between = away(&q_between, off_between)?;
    let n_wall = away(&q_wall, off_wall)?;
    // the ball beyond the cylinder (a concave cylinder: its normal out of the material
    // toward its axis) or within it
    let off_radius = across(off_cyl - q_cyl.point, q_cyl.dir.unitize()?).length();
    let cylinder_out = if off_radius > q_cyl.radius { -1.0 } else { 1.0 };
    let axis = q_cyl.dir.unitize()?;
    let axis_point = q_cyl.point;
    let big_r = q_cyl.radius;
    let sin_tol = smin(tan_tol.abs().max(1e-9), 1e-3);

    // G square to the axis
    if n_floor.cross(axis).length() > sin_tol {
        return Ok(None);
    }
    // the seam: along the axis, on the cylinder, W tangent to Y there
    let v = vertex::point(b, vertex)?;
    let e_seam = edge_of(b, seam, OP_CLASSIFY)?;
    let along_seam = super::mixed::leaving(k, b, e_seam, vertex, OP_CLASSIFY)?;
    if along_seam.cross(axis).length() > sin_tol {
        return Ok(None);
    }
    let radial = across(v - axis_point, axis);
    if (radial.length() - big_r).abs() > tol {
        return Ok(None);
    }
    let radial = radial.unitize()?;
    if (n_wall - radial * cylinder_out).length() > sin_tol {
        return Ok(None);
    }
    let major = big_r - cylinder_out * r;
    if major <= tol {
        return Ok(None);
    }

    // C0: B_W's ball on its section through the seam's line
    let seam_centre = v - n_floor * r - n_wall * r;
    let seam_foot = axis_point + axis * (seam_centre - axis_point).dot(axis);
    if (seam_centre.distance(seam_foot) - major).abs() > tol {
        return Ok(None);
    }

    // C: on G's and H's offsets (their line) at the major radius from the axis, on Y's side
    // of the seam (behind the seam from B_W's edge)
    let g = n_floor.dot(n_between);
    if 1.0 + g <= EFF_ZERO {
        return Ok(None);
    }
    let Ok(dir) = n_floor.cross(n_between).unitize() else {
        return Ok(None);
    };
    let on_line = v - (n_floor + n_between) * (r / (1.0 + g));
    let w = across(on_line - axis_point, axis);
    let d = across(dir, axis);
    let qa = d.dot(d);
    let qb = 2.0 * w.dot(d);
    let qc = w.dot(w) - major * major;
    if qa <= EFF_ZERO {
        return Ok(None);
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc.partial_cmp(&0.0) == Some(std::cmp::Ordering::Less) || disc.is_nan() {
        return Ok(None);
    }
    let root = disc.max(0.0).sqrt();
    let e_w = edge_of(b, eu_w, OP_CLASSIFY)?;
    let along_wall = super::mixed::leaving(k, b, e_w, vertex, OP_CLASSIFY)?;
    let mut best: Option<Point3> = None;
    for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
        let c = on_line + dir * t;
        if (c - v).dot(along_wall) >= -tol {
            continue;
        }
        if best.is_none_or(|p| c.distance(v) < p.distance(v)) {
            best = Some(c);
        }
    }
    let Some(centre) = best else {
        return Ok(None);
    };
    let foot = axis_point + axis * (centre - axis_point).dot(axis);
    if foot.distance(seam_foot) > tol {
        return Ok(None);
    }
    let Ok(out) = across(centre - foot, axis).unitize() else {
        return Ok(None);
    };
    let geometry = SupportGeometry {
        radius: r,
        origin: seam_foot,
        axis,
        major,
        cylinder_radius: big_r,
        cylinder_out,
        floor_out: n_floor,
        seam_centre,
        seam_on_floor: seam_centre + n_floor * r,
        seam_on_wall: seam_centre + n_wall * r,
        centre,
        on_floor: centre + n_floor * r,
        on_between: centre + n_between * r,
        on_cylinder: foot + out * big_r,
    };
    // each contact lies inside its face's trim, not only on its surface: a contact past
    // the face's edge would put a boundary outside the face's domain
    let contacts = [
        (floor, geometry.on_floor),
        (floor, geometry.seam_on_floor),
        (between, geometry.on_between),
        (wall_face, geometry.seam_on_wall),
        (cyl_face, geometry.on_cylinder),
    ];
    for (face, point) in contacts {
        if !contact_in_face(k, b, face, point)? {
            return Ok(None);
        }
    }
    let ring = SupportRing {
        floor,
        between,
        wall: wall_face,
        cylinder: cyl_face,
        seam,
        floor_wall: eu_w,
        floor_between: eu_g,
        between_cylinder: eu_y,
        solvers: [s_w, s_g, s_y],
    };
    Ok(Some((ring, geometry)))
}

/// The blend corner that builds the support change `ring`, `geometry` at `corner`'s vertex,
/// on its edge uses.
#[must_use]
pub fn support_ball_corner_of(
    corner: &FilletCorner,
    ring: SupportRing,
    geometry: &SupportGeometry,
) -> FilletCorner {
    let mut made = FilletCorner::new(
        FilletCornerKind::NByTwo,
        corner.vertex,
        corner.solver_eus.clone(),
        corner.all_eus.clone(),
        corner.approx_tol_3d,
        corner.tangency_tol_radians,
    );
    made.blend = Some(BlendCorner {
        event: BlendCornerEvent::SupportBall,
        turn: BlendTurn::Mitre,
        end: BlendEnd::OnEndFace,
    });
    made.support_ball = Some(SupportBall {
        ring,
        geometry: *geometry,
        patches: Vec::new(),
    });
    made
}

/// What the support change's topology operates on besides the corner itself.
pub(crate) struct SupportBallTopologyEnv<'a> {
    /// The executive's kernel.
    pub(crate) k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub(crate) target: &'a mut Brep,
    /// The executive's pseudo-B-rep.
    pub(crate) pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub(crate) corner: FilletCornerId,
    /// The solvers of `B_W`, `B_G` and `B_Y`.
    pub(crate) solvers: [&'a FilletSolver; 3],
}

impl FilletCorner {
    /// The support change's topology (module docs): vertices `Pc`, `Pb`, `Py`, `Vs`, `Vb`
    /// and edges `Vs-Vb`, `Vb-Pb`, `Pb-Pc`, `Pc-Py`, `Py-Vs`, `Pb-Py`, in that order. The
    /// corner is blending and not a bevel.
    ///
    /// # Errors
    ///
    /// Reviewed `Defect`s where the corner is not a support change or the seam's side edge
    /// use is missing; those of the solver and pseudo-topology operations.
    pub(crate) fn make_support_ball_topology(
        &mut self,
        env: SupportBallTopologyEnv<'_>,
    ) -> Result<()> {
        fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
            CornerLookup {
                this: Some((Some(corner), this)),
                ..Default::default()
            }
        }
        let SupportBallTopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solvers,
        } = env;
        let ring = self
            .support_ball
            .as_ref()
            .map(|sb| sb.ring)
            .ok_or_else(|| missing_defect(OP_TOPOLOGY, NO_EVENT))?;
        let [s_w, s_g, s_y] = solvers;
        let g_w = s_w.first_fillet_geom()?;
        let g_g = s_g.first_fillet_geom()?;
        let g_y = s_y.first_fillet_geom()?;

        let mut made = Vec::new();
        for _ in 0..5 {
            let fv = new_corner_vertex(k, pseudo, corner)?;
            self.vertices.push(fv);
            made.push(fv);
        }
        let &[pc, pb, py, vs, vb] = made.as_slice() else {
            return Err(defect(OP_TOPOLOGY, NOT_ITS_TOPOLOGY));
        };
        let side = find_side_edgeuse(target, ring.floor_wall, ring.seam)?.ok_or_else(|| {
            missing_defect(
                OP_TOPOLOGY,
                "the seam has no edge use beside B_W's edge, though they share the wall",
            )
        })?;
        for (fv, class, on, vtype) in [
            (
                pc,
                PointClassType::Face,
                EntityRef::Face(ring.between),
                FilletVertexType::RailXRail,
            ),
            (
                pb,
                PointClassType::Face,
                EntityRef::Face(ring.floor),
                FilletVertexType::Precomputed,
            ),
            (
                py,
                PointClassType::Face,
                EntityRef::Face(ring.cylinder),
                FilletVertexType::Precomputed,
            ),
            (
                vs,
                PointClassType::Edgeuse,
                EntityRef::Edgeuse(side),
                FilletVertexType::RailXEdgeuse,
            ),
            (
                vb,
                PointClassType::Face,
                EntityRef::Face(ring.floor),
                FilletVertexType::Mate,
            ),
        ] {
            let v = pseudo.fillet_vertex_mut(fv)?;
            v.set_point_class(class, Some(on));
            v.set_fillet_vertex_type(vtype);
        }
        pseudo.fillet_vertex_mut(vs)?.set_mate(0, Some(vb));
        pseudo.fillet_vertex_mut(vb)?.set_mate(0, Some(vs));

        // the rails: B_G and B_Y on H at Pc, B_G on G at Pb, B_Y on Y at Py, B_W on W at Vs
        // and on G at Vb
        let rail_w_wall = s_w.find_index_of_rail_x_side_edgeuse(target, side)?;
        for (g, fs, rail, fv) in [
            (
                g_g,
                s_g,
                s_g.find_index_of_rail_on_face(target, Some(ring.between))?,
                pc,
            ),
            (
                g_y,
                s_y,
                s_y.find_index_of_rail_on_face(target, Some(ring.between))?,
                pc,
            ),
            (
                g_g,
                s_g,
                s_g.find_index_of_rail_on_face(target, Some(ring.floor))?,
                pb,
            ),
            (
                g_y,
                s_y,
                s_y.find_index_of_rail_on_face(target, Some(ring.cylinder))?,
                py,
            ),
            (g_w, s_w, rail_w_wall, vs),
            (
                g_w,
                s_w,
                s_w.find_index_of_rail_on_face(target, Some(ring.floor))?,
                vb,
            ),
        ] {
            pseudo.make_rail_edgeuse_on(
                g,
                rail,
                Some(fv),
                OrientType::Unknown,
                None,
                lookup(corner, self),
                fs,
                target,
            )?;
        }

        // the edges: three end sections, the torus's contacts on G and Y, and the arc the
        // torus and the sphere share
        for (from, to, section, face) in [
            (vs, vb, Some(g_w), None),
            (vb, pb, None, Some(ring.floor)),
            (pb, pc, Some(g_g), None),
            (pc, py, Some(g_y), None),
            (py, vs, None, Some(ring.cylinder)),
            (pb, py, None, None),
        ] {
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner))?;
            if let Some(g) = section {
                pseudo
                    .fillet_edge_mut(e)?
                    .set_fillet_edge_type(FilletEdgeType::CrossSection);
                let primary = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(|| {
                    missing_defect(
                        OP_TOPOLOGY,
                        "the fillet edge just made has no primary edge use, though making a \
                         fillet edge makes its primary edge use",
                    )
                })?;
                pseudo.set_edgeuse_fillet_geom(primary, Some(g))?;
            } else {
                let fe = pseudo.fillet_edge_mut(e)?;
                fe.set_fillet_edge_type(FilletEdgeType::Precomputed);
                if face.is_some() {
                    fe.set_original_face(face);
                }
            }
            self.edges.push(e);
        }

        // each round ends on its own section; the hole is the corner's
        self.blending = true;
        self.bevel = false;
        Ok(())
    }
}

/// The vertex use of `fv` at the end of a rail of geometry `g`.
fn rail_vertexuse(
    pseudo: &FilletBrep,
    fv: VertexId,
    g: FilletGeomId,
) -> Result<crate::topology::VertexuseId> {
    pseudo
        .vu_at_rail_end(fv, Some(g), None, None)?
        .ok_or_else(|| bad_setup(OP_PLACE, FilletSetUp::RailEndVertexUse, None))
}

/// Every round's section at `fv` through the ball centred at `centre`: the tsect point of
/// each vertex use of a round's rail there, solved onto `centre` on both its offset
/// surfaces (as the Ball places its contacts). Returns the uv of `fv`'s own rail side on its
/// face, from the first rail there.
fn sections_through(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    solvers: &dyn BallSolvers,
    fv: VertexId,
    centre: Point3,
    tol: f64,
) -> Result<Option<Point2>> {
    let mut own = None;
    for vu in vertex::vertexuses(pseudo.brep(), fv)? {
        let Some(eu) = eu1::vertexuse_edgeuse(pseudo.brep(), vu)? else {
            continue;
        };
        let Some(g) = pseudo.fillet_edgeuse(eu)?.fillet_geom() else {
            continue;
        };
        let e = pseudo
            .brep()
            .edgeuse(eu)?
            .edge
            .ok_or_else(|| edge_use_without_edge(OP_PLACE, eu))?;
        if pseudo.fillet_edge(e)?.fillet_edge_type() != FilletEdgeType::Rail {
            continue;
        }
        let solver_id = pseudo.geom(g)?.fillet_solver().ok_or_else(|| {
            solver_input(
                OP_PLACE,
                SolverFailure::FilletGeomWithoutSolver { fillet_geom: g },
                None,
            )
        })?;
        let solver = solvers.solver(solver_id)?;
        let this_side = usize::from(pseudo.geom(g)?.rail(0) != Some(e));
        for side in [SrfIndex::First, SrfIndex::Second] {
            let rail = usize::from(side == SrfIndex::Second);
            let guess = pseudo.fillet_vertexuse(vu)?.tsect_pnt().uv_pos(side);
            let offset = pseudo
                .geom(g)?
                .offset_surface(rail, Some(solver))?
                .ok_or_else(|| bad_setup(OP_PLACE, FilletSetUp::RailOffsetSurface, None))?;
            let domain = offset_natural_uv_domain(offset);
            let Some((uv, _)) = offset_local_point_solve(k, offset, &domain, centre, guess)? else {
                return Err(unbuildable(OP_PLACE, CornerStep::BallContactFootNotFound));
            };
            if offset.surface.evaluate_point(k, uv)?.distance(centre) > tol {
                return Err(unbuildable(OP_PLACE, CornerStep::BallContactOffOffset));
            }
            *pseudo
                .fillet_vertexuse_mut(vu)?
                .tsect_pnt_mut()
                .uv_pos_mut(side) = uv;
            if rail == this_side && own.is_none() {
                own = Some(uv);
            }
        }
    }
    Ok(own)
}

/// The support change's vertices at the balls' contacts, once the base's vertex geometry
/// has placed `Pc` (the rails' crossing on H) and `Vs`, `Vb` (`B_W`'s section at the seam):
/// `Pb` and `Py` take their rounds' section at `Pc`, and every vertex is set at its contact
/// in closed form, each round's section there at its ball's centre on both its offset
/// surfaces, so that `B_G` and `B_Y` run to the ball C and `B_W` to the seam's ball.
///
/// # Errors
///
/// `SM_ERR` where the corner is not as its topology made it, a base vertex lies farther
/// than ten times the tolerance from its contact, or a section's offset point cannot be
/// solved onto its centre; those of the evaluations and the point solves.
pub fn place_support_ball_vertices(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<()> {
    let sb = corner
        .support_ball
        .as_ref()
        .ok_or_else(|| bad_input(OP_PLACE, CornerInput::EventNotSet, None))?;
    let &[pc, pb, py, vs, vb] = corner.vertices.as_slice() else {
        return Err(bad_input(OP_PLACE, CornerInput::TopologyMismatch, None));
    };
    let geo = sb.geometry;
    let tol = corner.approx_tol_3d;
    let [_, id_g, id_y] = sb.ring.solvers;
    let g_g = solvers.solver(id_g)?.first_fillet_geom()?;
    let g_y = solvers.solver(id_y)?.first_fillet_geom()?;
    for fv in [pc, vs, vb] {
        if !pseudo.fillet_vertex(fv)?.is_processed() {
            return Err(bad_input(OP_PLACE, CornerInput::VertexNotProcessed, None));
        }
    }
    // the base's crossings are the contacts
    for (fv, want) in [
        (pc, geo.on_between),
        (vs, geo.seam_on_wall),
        (vb, geo.seam_on_floor),
    ] {
        if pseudo.vertex_point(fv)?.distance(want) > 10.0 * tol {
            return Err(unbuildable(OP_PLACE, CornerStep::BallContactOffVertex));
        }
    }
    // Pb and Py: the other ends of B_G's and B_Y's sections at Pc
    for (fv, g) in [(pb, g_g), (py, g_y)] {
        let from = rail_vertexuse(pseudo, pc, g)?;
        let tsect = pseudo.fillet_vertexuse(from)?.tsect_pnt().clone();
        let to = rail_vertexuse(pseudo, fv, g)?;
        pseudo.fillet_vertexuse_mut(to)?.set_tsect_pnt(&tsect);
    }
    for (fv, centre, at) in [
        (pc, geo.centre, geo.on_between),
        (pb, geo.centre, geo.on_floor),
        (py, geo.centre, geo.on_cylinder),
        (vs, geo.seam_centre, geo.seam_on_wall),
        (vb, geo.seam_centre, geo.seam_on_floor),
    ] {
        let own = sections_through(k, pseudo, solvers, fv, centre, tol)?;
        pseudo.set_vertex_point(fv, at)?;
        let v = pseudo.fillet_vertex_mut(fv)?;
        if let Some(uv) = own {
            v.set_original_uv(uv);
        }
        v.set_status(FilStatus::Processed);
    }
    Ok(())
}

/// The curve of `curves` from `a` to `b` (one of them reversed where it runs from `b` to
/// `a`), within `tol`; `None` where none does.
fn curve_between(
    k: &Kernel,
    curves: &[Curve],
    a: Point3,
    b: Point3,
    tol: f64,
) -> Result<Option<Curve>> {
    for curve in curves {
        let ivl = curve.natural_interval();
        let lo = curve.evaluate_point(k, ivl.min())?;
        let hi = curve.evaluate_point(k, ivl.max())?;
        let forward = lo.distance(a) <= tol && hi.distance(b) <= tol;
        let backward = lo.distance(b) <= tol && hi.distance(a) <= tol;
        if !forward && !backward {
            continue;
        }
        let mut copy = curve.clone();
        if backward {
            copy.reverse_parameterization(k, &ivl)?;
        }
        return Ok(Some(copy));
    }
    Ok(None)
}

/// The natural loop of `surface`, its corners left out where degenerate.
fn natural_loop(k: &Kernel, surface: Surface) -> Result<SupportPatch> {
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let trims = surface.create_natural_uv_trim_curves(k, &natural, SurfParamType::V, true)?;
    Ok(SupportPatch {
        surface,
        curves_3d: trims.curves_3d,
        uv_curves: trims.uv_curves.into_iter().map(Curve::from).collect(),
        orients: trims.orients,
    })
}

/// The support change's two faces and the curves of its edges on them (`Vb-Pb` on G,
/// `Py-Vs` on Y, `Pb-Py`), before the edges' own kinds compute the end sections: the torus
/// about Y's axis from C0's section to C's, its contacts with G and Y, keeping its
/// definition (the section revolved about the axis on G, [`CornerDefinition::swept`]); and
/// the sphere's patch on C's ball between its contacts, keeping the Ball's definition.
///
/// # Errors
///
/// `SM_ERR` where the corner is not as its topology made it, a face's corner is farther
/// than the tolerance from its vertex, an edge's curve is not one of the torus's sides, or
/// the sphere's patch is not one triangle; those of the surfaces, their definitions and
/// their loops.
#[allow(clippy::too_many_lines)] // the two faces in turn
pub fn support_ball_edge_geom(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
) -> Result<()> {
    let tol = corner.approx_tol_3d;
    let (geo, ring) = {
        let sb = corner
            .support_ball
            .as_mut()
            .ok_or_else(|| bad_input(OP_BUILD, CornerInput::EventNotSet, None))?;
        sb.patches.clear();
        (sb.geometry, sb.ring)
    };
    let &[_, pb, py, vs, vb] = corner.vertices.as_slice() else {
        return Err(bad_input(OP_BUILD, CornerInput::TopologyMismatch, None));
    };
    let &[_, on_floor, _, _, on_cyl, inner] = corner.edges.as_slice() else {
        return Err(bad_input(OP_BUILD, CornerInput::TopologyMismatch, None));
    };
    let (p_vb, p_vs, p_pb, p_py) = (
        pseudo.vertex_point(vb)?,
        pseudo.vertex_point(vs)?,
        pseudo.vertex_point(pb)?,
        pseudo.vertex_point(py)?,
    );
    let r = geo.radius;

    // the torus: u about the axis from C0 to C, v round the tube from its contact with Y to
    // its contact with G (or back)
    let x = (geo.seam_centre - geo.origin).unitize()?;
    let to_c = geo.centre - geo.origin;
    let y = (to_c - x * to_c.dot(x)).unitize()?;
    let z = x.cross(y);
    let turn_deg = rad_to_deg(to_c.dot(y).atan2(to_c.dot(x)));
    if turn_deg <= 0.0 {
        return Err(unbuildable(OP_BUILD, CornerStep::SweptTorusOffVertex));
    }
    let v_floor = if geo.floor_out.dot(z) < 0.0 {
        270.0
    } else {
        90.0
    };
    let v_cyl = if geo.cylinder_out < 0.0 {
        180.0
    } else if v_floor > 180.0 {
        360.0
    } else {
        0.0
    };
    let domain = Extent2d::from_bounds(
        0.0,
        smin(v_floor, v_cyl),
        turn_deg,
        if v_floor > v_cyl { v_floor } else { v_cyl },
    );
    let mut frame = Axis2Placement::default();
    frame.set_canonical(geo.origin, x, y)?;
    // the rounds take material off: the torus's normal out of its tube is out of the
    // material
    let torus = Torus::create_canonical(
        k,
        &frame,
        geo.major,
        r,
        TorusOptions {
            swap_uv: false,
            inside_out: false,
            circle_nurb: None,
        },
        Some(&domain),
    )?;
    for (uv, want) in [
        (Point2::new(0.0, v_floor), p_vb),
        (Point2::new(0.0, v_cyl), p_vs),
        (Point2::new(turn_deg, v_floor), p_pb),
        (Point2::new(turn_deg, v_cyl), p_py),
    ] {
        if torus.evaluate_step_point(uv)?.distance(want) > tol {
            return Err(unbuildable(OP_BUILD, CornerStep::SweptTorusOffVertex));
        }
    }
    let mut surface = Surface::from(torus);
    let floor_surface = face2::surface(target, ring.floor)?
        .ok_or_else(|| face_without_surface(OP_BUILD, ring.floor))?;
    let definition = CornerDefinition::swept(
        k,
        floor_surface,
        geo.origin,
        z,
        [geo.seam_centre, geo.centre],
        r,
        tol,
    )?;
    definition.recertify(k, &surface)?;
    surface.set_corner_definition(definition);
    let torus_face = natural_loop(k, surface)?;
    if torus_face.curves_3d.len() != 4 {
        return Err(defect(
            OP_BUILD,
            "the torus's natural loop is not four curves, though its domain is a quarter of \
             its tube over a turn of less than half its circle",
        ));
    }
    for (e, a, b) in [
        (on_floor, p_vb, p_pb),
        (on_cyl, p_py, p_vs),
        (inner, p_pb, p_py),
    ] {
        let curve = curve_between(k, &torus_face.curves_3d, a, b, tol)?
            .ok_or_else(|| unbuildable_missing(OP_BUILD, CornerStep::SweptArcNotFound))?;
        set_edge_curve(pseudo.brep_mut(), e, Some(curve))?;
    }

    // the sphere's patch: C's ball between its contacts with H, G and Y
    let dirs = [
        (geo.on_between - geo.centre).unitize()?,
        (geo.on_floor - geo.centre).unitize()?,
        (geo.on_cylinder - geo.centre).unitize()?,
    ];
    let mut ball = None;
    for turn in 0..3 {
        let d = |i: usize| dirs.get((i + turn) % 3).copied().unwrap_or(Vec3::ZERO);
        if let Some(s) = ball_triangle_face(k, geo.centre, r, &[d(0), d(1), d(2)], false)? {
            ball = Some(s);
            break;
        }
    }
    let ball =
        ball.ok_or_else(|| unbuildable_missing(OP_BUILD, CornerStep::BallPatchNormalUndecided))?;
    let mut ball_face = natural_loop(k, ball)?;
    if ball_face.curves_3d.len() != 3 {
        return Err(defect(
            OP_BUILD,
            "the sphere's triangular patch's natural loop is not three curves, though the \
             patch's side at its pole is a point, left out of its loop",
        ));
    }
    let boundary = ball_face
        .curves_3d
        .iter()
        .map(|c| nurbs_curve(c).cloned())
        .collect::<Result<_>>()?;
    let definition = CornerDefinition::ball(geo.centre, r, boundary)?;
    definition.recertify(k, &ball_face.surface)?;
    ball_face.surface.set_corner_definition(definition);

    if let Some(sb) = corner.support_ball.as_mut() {
        sb.patches = vec![torus_face, ball_face];
    }
    Ok(())
}

/// The support change's two faces, each its surface bounded by its natural loop
/// (`MakeFaceWithCurves` in the fillet B-rep's infinite region, its natural UV domain,
/// `SM_OT_SAME`); the surfaces join `corner.surfaces` and the faces `corner.surface_faces`.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without the event, its faces or the fillet B-rep, or a `Defect`
/// for a face construction that succeeds without a face; those of the face construction.
pub fn create_support_ball_patches(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    let sb = corner
        .support_ball
        .as_mut()
        .ok_or_else(|| bad_input(OP_PATCH, CornerInput::EventNotSet, None))?;
    let patches = std::mem::take(&mut sb.patches);
    if patches.len() != 2 {
        return Err(bad_input(OP_PATCH, CornerInput::FacesNotBuilt, None));
    }
    for patch in patches {
        let SupportPatch {
            surface,
            curves_3d,
            uv_curves,
            orients,
        } = patch;
        let fillet = env
            .fillet
            .as_mut()
            .ok_or_else(|| bad_input(OP_PATCH, CornerInput::FilletBrepNotGiven, None))?;
        let input = FaceCurvesInput {
            loops: vec![curves_3d.len()],
            curves_3d: Some(curves_3d.into_iter().map(Some).collect()),
            uv_curves: Some(uv_curves),
            orientations: orients,
            loop_points: Vec::new(),
        };
        let uv_domain = surface.natural_uv_domain_with_nurb(env.k)?;
        let region = fillet.brep.infinite_region();
        let key_of = fillet.notify.key_of;
        let face_ref = |f| key_of(EntityRef::Face(f));
        let made = make_face_with_curves(
            env.k,
            fillet.brep,
            region,
            input,
            surface.clone(),
            &uv_domain,
            OrientType::Same,
            None,
            &face_ref,
            &mut fillet.notify,
        )?;
        let face = made.face.ok_or_else(|| {
            missing_defect(
                OP_PATCH,
                "making a support change's face returned no face, though the face \
                 construction succeeded",
            )
        })?;
        corner.surfaces.push(surface);
        corner.surface_faces.push(face);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::common_radius;

    #[test]
    fn rounds_each_within_tolerance_of_the_first_share_a_radius() {
        let got = common_radius(&[(2.0, 1e-3), (2.0005, 1e-3), (1.9995, 1e-3)]);
        assert_eq!(got, Some((2.0, 1e-3)));
    }

    #[test]
    fn a_chain_of_near_radii_that_drifts_past_the_tolerance_is_not_one_radius() {
        // adjacent radii differ by 0.8 tol, the first and last by 1.6 tol
        assert_eq!(
            common_radius(&[(2.0, 1e-3), (2.0008, 1e-3), (2.0016, 1e-3)]),
            None
        );
    }

    #[test]
    fn the_least_tolerance_decides() {
        assert_eq!(
            common_radius(&[(2.0, 1e-3), (2.0005, 1e-4), (2.0, 1e-3)]),
            None
        );
        assert_eq!(common_radius(&[(2.0, 1e-3), (f64::NAN, 1e-3)]), None);
        assert_eq!(common_radius(&[]), None);
    }
}
