//! The blend corner's Ball: three constant rounds of one radius `r` and one convexity at
//! a vertex of three planes end on great circles of the one ball touching the three
//! planes, and the corner's face is that ball's spherical triangle between the contacts,
//! one rational patch exactly on the sphere, G1 to every round.
//!
//! Upstream: none (pinned SMLib builds such a vertex as a 3x3 corner's Coons patch). The
//! fork's `source/SMLib/src/SmBlendCornerBall.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib 79d4763, a6243fa, f8e6709, 4241a1b, unchanged to 5e8c9c1; smlib#79
//! step 4c). From the fork's file head: the ball of radius `r` touching the three planes
//! `F_i` (unit normals `n_i`) is centred at `C`, `n_i . C = n_i . p_i + s_i r` on the side
//! the rounds' balls roll; it touches `F_i` at `P_i = C - s_i r n_i`. The round between
//! `F_i` and `F_j` ends on its section through `C`, from `P_i` to `P_j`: all three end
//! sections are great circles of the sphere about `C`.
//!
//! | Rust                         | Fork                                                  |
//! |------------------------------|-------------------------------------------------------|
//! | [`BallCorner`]               | `SmBlendCorner`'s `m_pBallSurface`, `m_vBall3d`, `m_vBallUV`, `m_vBallOrients` |
//! | [`BallSolvers`]              | `SmFilletExecutive::GetFilletSolverOfEdgeuse` and the solvers' queries |
//! | [`classify_ball`]            | `SmBlendCorner::ClassifyBall`                         |
//! | [`place_ball_contacts`]      | `SmBlendCorner::PlaceBallContacts` (`CalcCornerVertGeom`'s Ball part) |
//! | [`ball_corner_edge_geom`]    | `SmBlendCorner::BuildBallSurface` (`CalcCornerEdgeGeom`'s Ball part) |
//! | [`create_ball_patch`]        | `SmBlendCorner::CreateBallPatch`                      |
//! | `ball_centre` (private)      | `SmBlendCorner::BallCentre`                           |
//! | `ball_triangle_patch` (private) | `sm_BallTrianglePatch`                             |
//! | `ring_faces` (private)       | `sm_BallRingFaces`                                    |
//! | `BallPoly`, `choose`, `times`, `sum` (private) | `SmBallPoly`, `sm_Choose`, `sm_BallTimes`, `sm_BallSum` |
//! | `MakeBallTopology`           | `FilletCorner::make_ball_topology` (`fillet_corner::topology`) |
//!
//! Deviations: the corner's face is the fork's rational patch as a plain
//! `SurfaceKind::BSpline` (the same net, so the same points). The fork's f8e6709 wraps it
//! in `SmBallPatch`, which carries the sphere as its definition (its own type, exact
//! offset and database IO); that class is not ported here, but `SmBallPatch::Create`'s
//! refusals are checked before the face takes the net. The patch and its loop are kept in
//! [`BallCorner`] from the edge geometry until the patch takes them. The fork's
//! `SmFilletExecutive::IsSetBackVertex` is the caller's `set_back` argument. Opt-in:
//! `Kernel::fixes.fillet_blend_corner` (WR-697). No recursion.

use crate::blend::blend_corner::BlendCornerEvent;
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_custom_solver::{offset_local_point_solve, offset_natural_uv_domain};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletEdgeType, FilletSolverId};
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverType, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p1::KnotType;
use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{vertex, Brep, EdgeuseId, FaceId};

const OP_CLASSIFY: &str = "blend::blend_corner::ball::classify_ball";
const OP_CENTRE: &str = "blend::blend_corner::ball::ball_centre";
const OP_PLACE: &str = "blend::blend_corner::ball::place_ball_contacts";
const OP_BUILD: &str = "blend::blend_corner::ball::ball_corner_edge_geom";
const OP_PATCH_NET: &str = "blend::blend_corner::ball::ball_triangle_patch";
const OP_PATCH: &str = "blend::blend_corner::ball::create_ball_patch";

/// A Ball's own members on its corner, from the edge geometry until the patch takes
/// them: the face's surface (`m_pBallSurface`) and its loop (`m_vBall3d`, `m_vBallUV`,
/// `m_vBallOrients`: the patch's natural iso-curves, the pole left out).
#[derive(Debug, Clone, Default)]
pub struct BallCorner {
    /// `m_pBallSurface`.
    pub surface: Option<Surface>,
    /// `m_vBall3d`.
    pub curves_3d: Vec<Curve>,
    /// `m_vBallUV`.
    pub uv_curves: Vec<Curve>,
    /// `m_vBallOrients`.
    pub orients: Vec<OrientType>,
}

impl BallCorner {
    /// `DeleteBallSurface`.
    fn delete_surface(&mut self) {
        self.curves_3d.clear();
        self.uv_curves.clear();
        self.orients.clear();
        self.surface = None;
    }
}

/// The solvers as the Ball reads them.
pub trait BallSolvers {
    /// `GetFilletSolverOfEdgeuse(eu)`: the solver of a blended edge use, `None` where it
    /// has none.
    ///
    /// # Errors
    ///
    /// Where the lookup is not available.
    fn solver_of_edgeuse(&self, eu: EdgeuseId) -> Result<Option<FilletSolverId>>;
    /// The solver `id`.
    ///
    /// # Errors
    ///
    /// Where it is not live.
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver>;
    /// `GetSolverType()` of the solver `id`.
    ///
    /// # Errors
    ///
    /// Where it is not live.
    fn solver_type(&self, id: FilletSolverId) -> Result<FilletSolverType>;
    /// `OffsetRadiiCanChange()` of the solver `id`.
    ///
    /// # Errors
    ///
    /// Where it is not live.
    fn offset_radii_can_change(&self, id: FilletSolverId) -> Result<bool>;
}

fn min(a: f64, b: f64) -> f64 {
    // smos_Min
    if a < b {
        a
    } else {
        b
    }
}

/// `GetFilletSolverOfEdgeuse(pEU)`, else of its mate.
fn solver_of_edgeuse_or_mate(
    b: &Brep,
    eu: EdgeuseId,
    solvers: &dyn BallSolvers,
) -> Result<Option<FilletSolverId>> {
    if let Some(id) = solvers.solver_of_edgeuse(eu)? {
        return Ok(Some(id));
    }
    match eu1::mate(b, eu)? {
        Some(mate) => solvers.solver_of_edgeuse(mate),
        None => Ok(None),
    }
}

/// `sm_BallRingFaces`: face `i` between edge uses `i - 1` and `i` of a ring of three
/// (the corner's vertex `i`); `None` where two edge uses share no face.
fn ring_faces(b: &Brep, all_eus: &[EdgeuseId]) -> Result<[Option<FaceId>; 3]> {
    let mut faces = [None; 3];
    for (ii, slot) in faces.iter_mut().enumerate() {
        let prev = all_eus.get((ii + 2) % 3).copied();
        let this = all_eus.get(ii).copied();
        *slot = face_by_two_edgeuses(b, prev, this, None)?;
    }
    Ok(faces)
}

/// `GetSurface(rail)->GetOffsetDistance()` of a solver, absolute (`NER` on none).
fn offset_radius(solver: &FilletSolver, rail: usize, op: &'static str) -> Result<f64> {
    let slot = solver
        .surface(rail)?
        .ok_or_else(|| SolidError::null_pointer(op))?;
    Ok(slot.surface.offset_distance().abs())
}

/// `SmBlendCorner::ClassifyBall` (the fork's 79d4763, a6243fa): whether a filleted
/// vertex is a Ball. Every edge there is blended (three solver edge uses, `all_eus` the
/// corner's ring of three), each a constant circular round of the same radius (within
/// the rounds' least tolerance), all convex or all concave, the three faces distinct
/// planes meeting at a point, and the vertex not set back (`set_back`, the fork's
/// `IsSetBackVertex`). [`BlendCornerEvent::Ball`] where it is, else
/// [`BlendCornerEvent::None`] (the corner hierarchy builds it).
///
/// # Errors
///
/// [`SolidError::null_pointer`] for a missing edge or face surface the fork
/// dereferences (`NER`); those of the solver lookups, the convexity test and the
/// planarity test.
pub fn classify_ball(
    k: &Kernel,
    b: &mut Brep,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    set_back: bool,
    solvers: &dyn BallSolvers,
) -> Result<BlendCornerEvent> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let none = Ok(BlendCornerEvent::None);
    if solver_eus.len() != 3 || all_eus.len() != 3 || set_back {
        return none;
    }

    // constant circular rounds of one radius, of one convexity
    let mut radius = -1.0;
    let mut tol = BIG_DOUBLE;
    let mut concave0 = false;
    for (ii, &eu) in all_eus.iter().enumerate() {
        let Some(id) = solver_of_edgeuse_or_mate(b, eu, solvers)? else {
            return none;
        };
        let fs = solvers.solver(id)?;
        let (Some(fsg), Some(_), Some(_)) = (fs.fsg.as_ref(), fs.surface(0)?, fs.surface(1)?)
        else {
            return none;
        };
        if fsg.fillet_surface_generator_type() != FilletSurfaceGeneratorType::Circular
            || solvers.solver_type(id)? != FilletSolverType::ConstRadius
            || solvers.offset_radii_can_change(id)?
        {
            return none;
        }
        tol = min(tol, fs.approx_tol_3d);
        let r0 = offset_radius(fs, 0, OP_CLASSIFY)?;
        let r1 = offset_radius(fs, 1, OP_CLASSIFY)?;
        if (r0 - r1).abs() > tol || (radius >= 0.0 && (r0 - radius).abs() > tol) {
            return none;
        }
        radius = r0;
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let concave = is_concave_edge(k, b, e)?;
        if ii == 0 {
            concave0 = concave;
        } else if concave != concave0 {
            return none;
        }
    }
    // !(radius > tol), NaN included
    if radius.partial_cmp(&tol) != Some(std::cmp::Ordering::Greater) {
        return none;
    }

    // three planes
    let faces = ring_faces(b, all_eus)?;
    let mut normals = [Vec3::default(); 3];
    for (face, normal) in faces.iter().zip(normals.iter_mut()) {
        let Some(face) = *face else {
            return none;
        };
        let Some(surface) = face2::surface(b, face)? else {
            return none;
        };
        let planar = surface.is_planar(k, tol)?;
        if !planar.is_planar {
            return none;
        }
        *normal = planar.normal;
    }
    let [n0, n1, n2] = normals;
    if faces[0] == faces[1]
        || faces[1] == faces[2]
        || faces[2] == faces[0]
        || n0.dot(n1.cross(n2)).abs() <= EFF_ZERO_SQRT
    {
        return none;
    }
    Ok(BlendCornerEvent::Ball)
}

/// `SmBlendCorner::BallCentre` (the fork's 4241a1b): the ball's centre, the planes'
/// triple offset point solved from them (Cramer's rule), its radius and the contacts
/// `P_i` with the three faces; checked to be the ball of round 0's section at vertex 0
/// and the contacts to be the corner's vertices.
fn ball_centre(
    k: &Kernel,
    target: &Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<(Point3, f64, [Point3; 3])> {
    let nul = || SolidError::null_pointer(OP_CENTRE);
    let err = || SolidError::sm_err(OP_CENTRE);
    let all_eus = corner.all_eus.as_deref().ok_or_else(nul)?;
    let (&[v0, v1, v2], &[e0, _, _], 3) = (
        corner.vertices.as_slice(),
        corner.edges.as_slice(),
        all_eus.len(),
    ) else {
        return Err(err());
    };
    let tol = corner.approx_tol_3d;

    // a ball of the rounds at the contacts: round 0's offset surfaces at vertex 0
    let section0 = eu1::primary_edgeuse(pseudo.brep(), e0)?.ok_or_else(nul)?;
    let g0 = pseudo
        .fillet_edgeuse(section0)?
        .fillet_geom()
        .ok_or_else(nul)?;
    let vu = pseudo
        .vu_at_rail_end(v0, Some(g0), None, None)?
        .ok_or_else(nul)?;
    let tsect = pseudo.fillet_vertexuse(vu)?.tsect_pnt().clone();
    let solver_id = pseudo.geom(g0)?.fillet_solver().ok_or_else(nul)?;
    let solver = solvers.solver(solver_id)?;
    let mut on = [Point3::default(); 2];
    for (p, side) in on.iter_mut().zip([SrfIndex::First, SrfIndex::Second]) {
        let rail = usize::from(side == SrfIndex::Second);
        let offset = pseudo
            .geom(g0)?
            .offset_surface(rail, Some(solver))?
            .ok_or_else(nul)?;
        *p = offset.surface.evaluate_point(k, tsect.uv_pos(side))?;
    }
    if on[0].distance(on[1]) > tol {
        return Err(err()); // the section's offset points are its ball's centre
    }
    let ball = on[0];
    let radius = pseudo
        .geom(g0)?
        .offset_surface(0, Some(solver))?
        .ok_or_else(nul)?
        .surface
        .offset_distance()
        .abs();

    // the planes, and the centre r from each on the ball's side: n_i . C = n_i . p_i + s_i r
    let faces = ring_faces(target, all_eus)?;
    let mut normals = [Vec3::default(); 3];
    let mut rhs = [0.0; 3];
    let mut side = [0.0; 3];
    for (ii, face) in faces.iter().enumerate() {
        let face = face.ok_or_else(nul)?;
        let surface = face2::surface(target, face)?.ok_or_else(nul)?;
        let planar = surface.is_planar(k, tol)?;
        if !planar.is_planar {
            return Err(err()); // ClassifyBall took the faces to be planes
        }
        let n = planar.normal.unitize()?;
        let on_plane = planar.point;
        let s = if n.dot(ball - on_plane) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        if let (Some(normal), Some(r), Some(sd)) =
            (normals.get_mut(ii), rhs.get_mut(ii), side.get_mut(ii))
        {
            *normal = n;
            *sd = s;
            *r = n.x * on_plane.x + n.y * on_plane.y + n.z * on_plane.z + s * radius;
        }
    }
    let [n0, n1, n2] = normals;
    // Cramer's rule: C = (rhs_0 n_1 x n_2 + rhs_1 n_2 x n_0 + rhs_2 n_0 x n_1) / (n_0 . n_1 x n_2)
    let det = n0.dot(n1.cross(n2));
    if det.abs() <= EFF_ZERO {
        return Err(err());
    }
    let centre: Point3 = n1.cross(n2) * (rhs[0] / det)
        + n2.cross(n0) * (rhs[1] / det)
        + n0.cross(n1) * (rhs[2] / det);
    if centre.distance(ball) > tol {
        return Err(err()); // the rounds' ball is the planes' triple offset point
    }
    let mut contacts = [Point3::default(); 3];
    for (ii, (&v, contact)) in [v0, v1, v2].iter().zip(contacts.iter_mut()).enumerate() {
        let (Some(&n), Some(&s)) = (normals.get(ii), side.get(ii)) else {
            return Err(err());
        };
        *contact = centre - n * (s * radius);
        if contact.distance(pseudo.vertex_point(v)?) > tol {
            return Err(err()); // vertex i is the ball's contact with face i
        }
    }
    Ok((centre, radius, contacts))
}

/// `SmBlendCorner::PlaceBallContacts` (the fork's 4241a1b; `CalcCornerVertGeom` runs it
/// after the base's vertex geometry on a Ball): the contacts as they are in closed form,
/// not as the rails' crossings were solved. Each vertex is set at its contact, and each
/// round's section there (the tsect point of each vertex use of a round's rail, which the
/// round's trace ends on) at the ball's centre on both its offset surfaces, so the exact
/// round runs its span to that section and its end circle is the ball's.
///
/// # Errors
///
/// `SM_ERR` where the corner is not as the Ball topology made it, the ball's centre does
/// not hold, or a section's offset point cannot be solved onto the centre;
/// [`SolidError::null_pointer`] for missing topology or geometry the fork dereferences;
/// those of the evaluations and the point solves.
pub fn place_ball_contacts(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_PLACE);
    let err = || SolidError::sm_err(OP_PLACE);
    let (centre, _radius, contacts) = ball_centre(k, target, pseudo, corner, solvers)?;
    let tol = corner.approx_tol_3d;
    for (&fv, &contact) in corner.vertices.iter().zip(contacts.iter()) {
        pseudo.set_vertex_point(fv, contact)?;
        // every round's section there: the tsect point of each vertexuse of a round's rail
        for vu in vertex::vertexuses(pseudo.brep(), fv)? {
            let Some(eu) = eu1::vertexuse_edgeuse(pseudo.brep(), vu)? else {
                continue;
            };
            let Some(g) = pseudo.fillet_edgeuse(eu)?.fillet_geom() else {
                continue;
            };
            let e = pseudo.brep().edgeuse(eu)?.edge.ok_or_else(nul)?;
            if pseudo.fillet_edge(e)?.fillet_edge_type() != FilletEdgeType::Rail {
                continue;
            }
            let solver_id = pseudo.geom(g)?.fillet_solver().ok_or_else(nul)?;
            let solver = solvers.solver(solver_id)?;
            for side in [SrfIndex::First, SrfIndex::Second] {
                let rail = usize::from(side == SrfIndex::Second);
                let guess = pseudo.fillet_vertexuse(vu)?.tsect_pnt().uv_pos(side);
                let offset = pseudo
                    .geom(g)?
                    .offset_surface(rail, Some(solver))?
                    .ok_or_else(nul)?;
                let domain = offset_natural_uv_domain(offset);
                let solved = offset_local_point_solve(k, offset, &domain, centre, guess)?;
                let Some((uv, _)) = solved else {
                    return Err(err());
                };
                let at = offset.surface.evaluate_point(k, uv)?;
                if at.distance(centre) > tol {
                    return Err(err());
                }
                *pseudo
                    .fillet_vertexuse_mut(vu)?
                    .tsect_pnt_mut()
                    .uv_pos_mut(side) = uv;
            }
        }
    }
    Ok(())
}

/// `SmBallPoly`: a polynomial in `(s, t)` of degree `(m, n)` in the Bernstein basis,
/// `c[i][j]` the coefficient of `B^m_i(s) B^n_j(t)`; degrees up to `(4, 2)`.
#[derive(Debug, Clone, Copy, Default)]
struct BallPoly {
    m: usize,
    n: usize,
    c: [[f64; 3]; 5],
}

impl BallPoly {
    fn at(&self, i: usize, j: usize) -> f64 {
        self.c
            .get(i)
            .and_then(|row| row.get(j))
            .copied()
            .unwrap_or(0.0)
    }

    fn set(&mut self, i: usize, j: usize, v: f64) -> Result<()> {
        let slot = self
            .c
            .get_mut(i)
            .and_then(|row| row.get_mut(j))
            .ok_or_else(|| SolidError::sm_err(OP_PATCH_NET))?;
        *slot = v;
        Ok(())
    }
}

/// `sm_Choose`: the binomial coefficient, as a product.
fn choose(n: usize, k: usize) -> f64 {
    let mut d = 1.0;
    for i in 1..=k {
        d = d * (n - k + i) as f64 / i as f64;
    }
    d
}

/// `sm_BallTimes`: the product of two polynomials in the Bernstein basis.
fn times(a: &BallPoly, b: &BallPoly) -> Result<BallPoly> {
    let mut p = BallPoly {
        m: a.m + b.m,
        n: a.n + b.n,
        ..BallPoly::default()
    };
    for i in 0..=p.m {
        for j in 0..=p.n {
            let mut d = 0.0;
            for ai in 0..=a.m {
                for bj in 0..=a.n {
                    if ai > i || bj > j || i - ai > b.m || j - bj > b.n {
                        continue;
                    }
                    d += a.at(ai, bj)
                        * b.at(i - ai, j - bj)
                        * choose(a.m, ai)
                        * choose(b.m, i - ai)
                        * choose(a.n, bj)
                        * choose(b.n, j - bj);
                }
            }
            p.set(i, j, d / (choose(p.m, i) * choose(p.n, j)))?;
        }
    }
    Ok(p)
}

/// `sm_BallSum`: `da a + db b`, of one degree.
fn sum(da: f64, a: &BallPoly, db: f64, b: &BallPoly) -> Result<BallPoly> {
    let mut p = *a;
    for i in 0..=p.m {
        for j in 0..=p.n {
            p.set(i, j, da * a.at(i, j) + db * b.at(i, j))?;
        }
    }
    Ok(p)
}

/// `sm_BallTrianglePatch` (the fork's a6243fa): the ball's triangle between the
/// contacts as one rational patch of degree `(4, 2)` on the sphere about `centre`
/// (`dirs` the unit contact directions), the pole at contact `pole`: `t = 0` the pole,
/// `s = 0` and `s = 1` its sides through the pole, `t = 1` the third. The sphere's
/// stereographic projection from the pole's antipode takes the third side to a circle's
/// arc, a rational quadratic; the sector `t c(s)` taken back by the inverse projection is
/// the patch. With `reverse_s`, `s` runs the other way. Returns the patch and the
/// contacts at `s = 0` and `s = 1` (`t = 1`).
fn ball_triangle_patch(
    centre: Point3,
    radius: f64,
    dirs: &[Vec3; 3],
    pole: usize,
    reverse_s: bool,
) -> Result<(BSplineSurface, usize, usize)> {
    let err = || SolidError::sm_err(OP_PATCH_NET);
    let mut from = (pole + 1) % 3;
    let mut to = (pole + 2) % 3;
    let (Some(&d_pole), Some(&d_from), Some(&d_to)) =
        (dirs.get(pole), dirs.get(from), dirs.get(to))
    else {
        return Err(err());
    };
    let e3 = -d_pole;
    let e1 = (d_from - e3 * d_from.dot(e3)).unitize()?;
    let e2 = e3.cross(e1);
    // the projection: p -> (p.e1, p.e2) / (1 - p.e3)
    let mid = (d_from + d_to).unitize()?;
    let mut out = [[0.0; 2]; 3];
    for (p, o) in [d_from, d_to, mid].iter().zip(out.iter_mut()) {
        let den = 1.0 - p.dot(e3);
        if den <= EFF_ZERO {
            return Err(err());
        }
        *o = [p.dot(e1) / den, p.dot(e2) / den];
    }
    let [a, b, m] = out;
    // the third side's great circle (normal m) projects to the circle about -(m1, m2) / m3,
    // of radius^2 1 + (m1^2 + m2^2) / m3^2
    let sm = d_from.cross(d_to).unitize()?;
    let m3 = sm.dot(e3);
    if m3.abs() <= EFF_ZERO {
        return Err(err()); // the third side runs through the pole: no triangle
    }
    let o = [-sm.dot(e1) / m3, -sm.dot(e2) / m3];
    let r = (1.0 + (o[0] * o[0] + o[1] * o[1])).sqrt();
    let ua = [(a[0] - o[0]) / r, (a[1] - o[1]) / r];
    let ub = [(b[0] - o[0]) / r, (b[1] - o[1]) / r];
    let cos = ua[0] * ub[0] + ua[1] * ub[1];
    // the arc from A to B through the middle's image, less than a half turn: its middle
    // control point where the tangents at A and B cross, its weight the cosine of half its
    // angle
    if 1.0 + cos <= EFF_ZERO_SQRT
        || (m[0] - o[0]) * (ua[0] + ub[0]) + (m[1] - o[1]) * (ua[1] + ub[1]) <= 0.0
    {
        return Err(err());
    }
    let w = (0.5 * (1.0 + cos)).sqrt();
    let d = [
        o[0] + r * (ua[0] + ub[0]) / (1.0 + cos),
        o[1] + r * (ua[1] + ub[1]) / (1.0 + cos),
    ];

    // (U, V, T) of degree (2, 1)
    let base = BallPoly {
        m: 2,
        n: 1,
        ..BallPoly::default()
    };
    let (mut pu, mut pv, mut pt) = (base, base, base);
    let xs = [a[0], w * d[0], b[0]];
    let ys = [a[1], w * d[1], b[1]];
    let wts = [1.0, w, 1.0];
    for i in 0..3 {
        let (Some(&x), Some(&y), Some(&wt)) = (xs.get(i), ys.get(i), wts.get(i)) else {
            return Err(err());
        };
        pu.set(i, 1, x)?;
        pv.set(i, 1, y)?;
        pt.set(i, 0, wt)?;
        pt.set(i, 1, wt)?;
    }
    let ut = times(&pu, &pt)?;
    let vt = times(&pv, &pt)?;
    let sq = sum(1.0, &times(&pu, &pu)?, 1.0, &times(&pv, &pv)?)?;
    let tt = times(&pt, &pt)?;
    let h3 = sum(1.0, &sq, -1.0, &tt)?;
    let h4 = sum(1.0, &sq, 1.0, &tt)?;

    // control points, s (u) outer, t (v) inner
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for ii in 0..=4 {
        let i = if reverse_s { 4 - ii } else { ii };
        for j in 0..=2 {
            let wij = h4.at(i, j);
            if wij <= EFF_ZERO {
                return Err(err());
            }
            let off = (e1 * (2.0 * ut.at(i, j)) + e2 * (2.0 * vt.at(i, j)) + e3 * h3.at(i, j))
                * (radius / wij);
            points.push(centre + off);
            weights.push(wij);
        }
    }
    if reverse_s {
        std::mem::swap(&mut from, &mut to);
    }
    let knots = [0.0, 1.0];
    let patch = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 4,
        v_degree: 2,
        control_points: &points,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[5, 5],
        v_multiplicities: &[3, 3],
        u_knots: &knots,
        v_knots: &knots,
        knot_type: KnotType::PiecewiseBezierKnots,
        weights: Some(&weights),
        uv_domain: None,
    })?;
    Ok((patch, from, to))
}

/// `SmBallPatch::Create`'s refusals (the fork's f8e6709): a sphere's centre is finite and
/// its radius finite and positive, and a patch on it is rational with positive weights.
/// The face takes the net as it is (module docs).
fn check_ball_patch(net: &BSplineSurface, centre: Point3, radius: f64) -> Result<()> {
    let invalid = |reason| SolidError::invalid_input(OP_BUILD, reason);
    if !radius.is_finite()
        || radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        || !centre.x.is_finite()
        || !centre.y.is_finite()
        || !centre.z.is_finite()
    {
        return Err(invalid(
            "SmBallPatch: a sphere's centre is finite and its radius finite and positive",
        ));
    }
    let cpn = net.control_point_net()?;
    if !net.is_rational() || cpn.weights.len() != cpn.points.len() {
        return Err(invalid("SmBallPatch: a patch on a sphere is rational"));
    }
    if cpn
        .weights
        .iter()
        .any(|w| w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater))
    {
        return Err(invalid(
            "SmBallPatch: a patch on a sphere has positive weights",
        ));
    }
    Ok(())
}

/// `SmBlendCorner::BuildBallSurface` (the fork's 79d4763, a6243fa, f8e6709; run by
/// `CalcCornerEdgeGeom` on a Ball before the edges' own kinds compute their curves): the
/// corner's face, one rational patch on the ball's sphere with the pole at contact 0 and
/// its normal out of the material (towards the centre where the rounds add material,
/// away from it where they remove it), its corners at the contacts, and its loop (its
/// natural boundary, the pole left out: the three sections), kept in
/// [`FilletCorner::ball`].
///
/// # Errors
///
/// `SM_ERR` where the corner is not as the Ball topology made it, the ball does not hold,
/// the patch's corners are not the corner's vertices, or its loop is other than three
/// curves; [`SolidError::null_pointer`] for missing topology or geometry the fork
/// dereferences; those of the evaluations, the convexity test and the patch.
pub fn ball_corner_edge_geom(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_BUILD);
    let err = || SolidError::sm_err(OP_BUILD);
    corner.ball.as_mut().ok_or_else(nul)?.delete_surface();
    let (centre, radius, contacts) = ball_centre(k, target, pseudo, corner, solvers)?;
    let tol = corner.approx_tol_3d;

    // The face's normal is its surface's (SmBrep::OrientTrimmedSurfaces turns a face the
    // other way by swapping its surface's parameters): out of the material, towards the
    // centre where the rounds add material (concave), away from it where they remove it.
    let first = corner
        .all_eus
        .as_deref()
        .and_then(<[EdgeuseId]>::first)
        .copied()
        .ok_or_else(nul)?;
    let first_edge = eu1::edge(target, first)?.ok_or_else(nul)?;
    let adds_material = is_concave_edge(k, target, first_edge)?;

    // the face: one rational patch on the sphere, the pole at contact 0, its normal turned
    // as the face's
    let pole = 0;
    let mut from = 0;
    let mut to = 0;
    let mut chosen: Option<Surface> = None;
    let mut dirs = [Vec3::default(); 3];
    for (dir, &contact) in dirs.iter_mut().zip(contacts.iter()) {
        *dir = (contact - centre).unitize()?;
    }
    for turn in 0..2 {
        if chosen.is_some() {
            break;
        }
        let (net, f, t) = ball_triangle_patch(centre, radius, &dirs, pole, turn == 1)?;
        from = f;
        to = t;
        let patch = Surface::from(net.clone());
        let mid = patch.natural_uv_domain_with_nurb(k)?.evaluate(0.5, 0.5);
        let at = patch.evaluate_point(k, mid)?;
        let normal = patch.evaluate_normal(k, mid, Side::Left, Side::Left)?;
        if (normal.dot(at - centre) < 0.0) == adds_material {
            // the fork's SmBallPatch::Create (its refusals; the definition is not kept)
            check_ball_patch(&net, centre, radius)?;
            chosen = Some(patch);
        }
    }
    let surface = chosen.ok_or_else(nul)?;

    // its corners are the contacts: the pole at v = vmin, the others at v = vmax
    let nurb_domain = surface.natural_uv_domain_with_nurb(k)?;
    let (lo, hi) = (nurb_domain.min(), nurb_domain.max());
    let corners_uv = [
        Point2::new(lo.x, hi.y),
        Point2::new(hi.x, hi.y),
        Point2::new(lo.x, lo.y),
    ];
    for (&uv, at_vertex) in corners_uv.iter().zip([from, to, pole]) {
        let v = *corner.vertices.get(at_vertex).ok_or_else(err)?;
        if surface
            .evaluate_point(k, uv)?
            .distance(pseudo.vertex_point(v)?)
            > tol
        {
            return Err(err());
        }
    }

    let trims = surface.create_natural_uv_trim_curves(k, &nurb_domain, SurfParamType::V, true)?;
    if trims.curves_3d.len() != 3 || trims.uv_curves.len() != 3 || trims.orients.len() != 3 {
        return Err(err()); // the three sections; the side at the pole a point
    }
    let ball = corner.ball.as_mut().ok_or_else(nul)?;
    ball.surface = Some(surface);
    ball.curves_3d = trims.curves_3d;
    ball.uv_curves = trims.uv_curves.into_iter().map(Curve::from).collect();
    ball.orients = trims.orients;
    Ok(())
}

/// `SmBlendCorner::CreateBallPatch` (the fork's 79d4763): the corner's face, the patch
/// bounded by its natural loop (`MakeFaceWithCurves` in the fillet B-rep's infinite
/// region, its natural UV domain, `SM_OT_SAME`); the surface joins `corner.surfaces` and
/// the face `corner.surface_faces` (the corner's patch face for the stitch). Its edges
/// and the rounds' meet in the stitch and the merge.
///
/// # Errors
///
/// [`SolidError::null_pointer`] without the patch, the fillet B-rep or a new face
/// (`NER`); those of the face construction.
pub fn create_ball_patch(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_PATCH);
    let ball = corner.ball.as_mut().ok_or_else(nul)?;
    let surface = ball.surface.take().ok_or_else(nul)?;
    // the face takes the surface and the curves
    let curves_3d = std::mem::take(&mut ball.curves_3d);
    let uv_curves = std::mem::take(&mut ball.uv_curves);
    let orients = ball.orients.clone();
    let fillet = env.fillet.as_mut().ok_or_else(nul)?;
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
    let face = made.face.ok_or_else(nul)?;
    // the corner's patch face (SmFilletCorner::GetPatchFaces, the stitch)
    corner.surfaces.push(surface);
    corner.surface_faces.push(face);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_is_binomial() {
        assert_eq!(choose(4, 2), 6.0);
        assert_eq!(choose(2, 0), 1.0);
        assert_eq!(choose(3, 3), 1.0);
    }

    /// The octant of the unit sphere (contacts +x, +y, +z, pole at +x): every net point's
    /// rational image is on the sphere.
    #[test]
    fn triangle_patch_is_on_the_sphere() {
        let k = Kernel::default();
        let dirs = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let c = Point3::new(1.0, 2.0, 3.0);
        for rev in [false, true] {
            let (net, from, to) = ball_triangle_patch(c, 2.0, &dirs, 0, rev).unwrap();
            assert_eq!((from, to), if rev { (2, 1) } else { (1, 2) });
            let s = Surface::from(net);
            let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
            for i in 0..=8 {
                for j in 0..=8 {
                    let p = s
                        .evaluate_point(&k, dom.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0))
                        .unwrap();
                    assert!(((p - c).length() - 2.0).abs() < 1e-13);
                }
            }
        }
    }
}
