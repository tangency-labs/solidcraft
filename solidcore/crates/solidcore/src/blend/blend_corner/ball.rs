//! The blend corner's Ball: `n >= 3` constant rounds of one radius `r` and one convexity
//! at a vertex of `n` faces that one ball touches (three faces each a plane or a natural
//! quadric: a sphere, a cylinder or a cone; or four or more planes whose offsets by `r`
//! meet in one point) end on great circles of that ball, and the corner's faces are the
//! ball's spherical n-gon between the contacts, fanned from contact 0 into `n - 2`
//! triangles, each one rational patch exactly on the sphere, G1 to every round.
//!
//! Upstream: none (pinned SMLib builds such a vertex as a 3x3 corner's Coons patch). The
//! fork's `source/SMLib/src/SmBlendCornerBall.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib 79d4763, a6243fa, f8e6709, 4241a1b; smlib#79 step 4c; the curved
//! faces 7048946, smlib#110; four or more rounds 278bf77, smlib#125, merged at 18e96f1). From the fork's file head: the ball of
//! radius `r` touching the three faces `F_i` is centred at `C`, where the faces' offsets by
//! `r` meet on the side the rounds' balls roll (the triple offset point); it touches `F_i`
//! at `P_i = C - s_i r n_i`, `n_i` the offset's normal at `C`. The round between `F_i` and
//! `F_j` ends on its section through `C`, from `P_i` to `P_j`, in the plane of `n_i` and
//! `n_j` whatever the faces: all three end sections are great circles of the sphere about
//! `C`. Where two faces or more are planes, `C` is in closed form (the offset planes' line
//! meets the third offset); otherwise Newton's method, the root taken only where a
//! Krawczyk test proves it the one root within rounding of the part's size.
//!
//! | Rust                         | Fork                                                  |
//! |------------------------------|-------------------------------------------------------|
//! | [`BallCorner`]               | `SmBlendCorner`'s `m_vBallSurfaces`, `m_vBallLoops`, `m_vBall3d`, `m_vBallUV`, `m_vBallOrients` |
//! | [`BallSolvers`]              | `SmFilletExecutive::GetFilletSolverOfEdgeuse` and the solvers' queries |
//! | [`classify_ball`]            | `SmBlendCorner::ClassifyBall`                         |
//! | [`place_ball_contacts`]      | `SmBlendCorner::PlaceBallContacts` (`CalcCornerVertGeom`'s Ball part) |
//! | [`ball_corner_edge_geom`]    | `SmBlendCorner::BuildBallSurface` (`CalcCornerEdgeGeom`'s Ball part) |
//! | [`create_ball_patch`]        | `SmBlendCorner::CreateBallPatch`                      |
//! | `ball_centre` (private)      | `SmBlendCorner::BallCentre`                           |
//! | `ball_face_quadric` (private) | `sm_BallFaceQuadric`                                 |
//! | `BallImplicit`, `ball_inverse`, `ball_certified_root` (private) | `SmBallImplicit`, `sm_BallInverse`, `sm_BallCertifiedRoot` |
//! | `ball_triangle_patch` (private) | `sm_BallTrianglePatch`                             |
//! | `ring_faces` (private)       | `sm_BallRingFaces`                                    |
//! | `planes_meet` (private)      | `sm_BallPlanesMeet`                                   |
//! | `BallPoly`, `choose`, `times`, `sum` (private) | `SmBallPoly`, `sm_Choose`, `sm_BallTimes`, `sm_BallSum` |
//! | `MakeBallTopology`           | `FilletCorner::make_ball_topology` (`fillet_corner::topology`) |
//!
//! Deviations: the face is, as the fork's f8e6709 makes it, an
//! [`crate::surface::ball_patch::BallPatch`] (`SmBallPatch`): the rational patch carrying
//! its sphere as its definition (its own type, exact offset and database IO). The patch
//! and its loop are kept in [`BallCorner`] from the edge geometry until the patch takes them.
//! With `Kernel::fixes.fillet_round_as_definition` the patch also keeps its corner
//! definition (#2357, not the fork's): its sphere and its loop, certified on it
//! ([`crate::surface::procedural_blend::corner::BallDefinition`]). The fork's
//! `SmFilletExecutive::IsSetBackVertex` is the caller's `set_back` argument (WR-697). No
//! recursion; the certified root's Newton
//! loop is bounded at 50 steps, as the fork's.

use crate::blend::blend_corner::BlendCornerEvent;
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p1::nurbs_curve;
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_custom_solver::{offset_local_point_solve, offset_natural_uv_domain};
use crate::blend::fillet_exact::{BlendQuadric, QuadricKind};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletEdgeType, FilletSolverId};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverType, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::krawczyk::{contraction, k_operator};
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p1::KnotType;
use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::surface::ball_patch::BallPatch;
use crate::surface::procedural_blend::corner::CornerDefinition;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{vertex, Brep, EdgeuseId, FaceId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::ball::classify_ball";
const OP_QUADRIC: &str = "blend::blend_corner::ball::ball_face_quadric";
const OP_CENTRE: &str = "blend::blend_corner::ball::ball_centre";
const OP_PLACE: &str = "blend::blend_corner::ball::place_ball_contacts";
const OP_BUILD: &str = "blend::blend_corner::ball::ball_corner_edge_geom";
const OP_PATCH_NET: &str = "blend::blend_corner::ball::ball_triangle_patch";
const OP_PATCH: &str = "blend::blend_corner::ball::create_ball_patch";

/// A Ball's own members on its corner, from the edge geometry until the patches take
/// them: the faces' surfaces (`m_vBallSurfaces`, the `n - 2` patches of the n-gon fanned
/// from contact 0, smlib#125 278bf77), each patch's count of loop curves (`m_vBallLoops`)
/// and the loops (`m_vBall3d`, `m_vBallUV`, `m_vBallOrients`: each patch's natural
/// iso-curves, the pole left out, in turn).
#[derive(Debug, Clone, Default)]
pub struct BallCorner {
    /// `m_vBallSurfaces`.
    pub surfaces: Vec<Surface>,
    /// `m_vBallLoops`: each patch's count of curves in the arrays below, in turn.
    pub loops: Vec<usize>,
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
        self.surfaces.clear();
        self.loops.clear();
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
pub(crate) fn solver_of_edgeuse_or_mate(
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

/// `sm_BallRingFaces`: face `i` between edge uses `i - 1` and `i` of a ring of `n`
/// (the corner's vertex `i`); `None` where two edge uses share no face.
fn ring_faces(b: &Brep, all_eus: &[EdgeuseId]) -> Result<Vec<Option<FaceId>>> {
    let n = all_eus.len();
    let mut faces = vec![None; n];
    for (ii, slot) in faces.iter_mut().enumerate() {
        let prev = all_eus.get((ii + n - 1) % n).copied();
        let this = all_eus.get(ii).copied();
        *slot = face_by_two_edgeuses(b, prev, this, None)?;
    }
    Ok(faces)
}

/// `sm_BallPlanesMeet` (smlib#125 278bf77): the point `C` on three offset planes (point
/// `on[i]`, unit normal `normal[i]`), `normal[i] . (C - on[i]) = 0`, by Cramer's rule.
fn planes_meet(on: &[Point3; 3], normal: &[Vec3; 3]) -> Result<Point3> {
    let [n0, n1, n2] = *normal;
    let det = n0.dot(n1.cross(n2));
    if det.abs() <= EFF_ZERO {
        return Err(SolidError::blend_failed(
            OP_CLASSIFY,
            "the three faces' planes do not meet in one point",
        ));
    }
    let mut rhs = [0.0; 3];
    for ((r, n), p) in rhs.iter_mut().zip(normal.iter()).zip(on.iter()) {
        *r = n.x * p.x + n.y * p.y + n.z * p.z;
    }
    let c = n1.cross(n2) * (rhs[0] / det)
        + n2.cross(n0) * (rhs[1] / det)
        + n0.cross(n1) * (rhs[2] / det);
    Ok(Point3::new(c.x, c.y, c.z))
}

fn max(a: f64, b: f64) -> f64 {
    // smos_Max
    if a > b {
        a
    } else {
        b
    }
}

/// `sm_BallFaceQuadric` (the fork's 7048946): the quadric (a plane or a natural quadric) a
/// ring face lies on, [`QuadricKind::None`] where it lies on none.
fn ball_face_quadric(k: &Kernel, b: &Brep, face: FaceId, tol: f64) -> Result<BlendQuadric> {
    let surface = face2::surface(b, face)?
        .ok_or_else(|| SolidError::blend_missing(OP_QUADRIC, "the ring face's surface"))?;
    let q = BlendQuadric::from_surface(k, surface, face2::uv_domain(b, face)?.mid(), tol)?;
    if !matches!(
        q.kind,
        QuadricKind::Plane | QuadricKind::Sphere | QuadricKind::Cylinder | QuadricKind::Cone
    ) {
        return Ok(BlendQuadric::default());
    }
    Ok(q)
}

/// A 3x3 matrix entry, `0` out of range (never: the indices are below 3).
fn at3(m: &[[f64; 3]; 3], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
}

/// `SmBallImplicit` (the fork's 7048946): a quadric's implicit form
/// `f(x) = x.A.x + 2 b.x + c` about an origin (`x = point - origin`).
#[derive(Debug, Clone, Copy, Default)]
struct BallImplicit {
    a: [[f64; 3]; 3],
    b: [f64; 3],
    c: f64,
}

impl BallImplicit {
    /// `Set`: the quadric's form about `at`.
    fn new(quadric: &BlendQuadric, at: Point3) -> Self {
        let mut local = *quadric;
        local.point = Point3::new(
            quadric.point.x - at.x,
            quadric.point.y - at.y,
            quadric.point.z - at.z,
        );
        let (a, b, c) = local.implicit();
        Self { a, b, c }
    }

    /// `Value`: `f(x)`, and the sum of its terms' magnitudes (`pdBound`).
    fn value(&self, x: &[f64; 3]) -> (f64, f64) {
        let mut d = self.c;
        let mut abs = self.c.abs();
        for ((bi, row), xi) in self.b.iter().zip(self.a.iter()).zip(x.iter()) {
            let t = 2.0 * bi * xi;
            d += t;
            abs += t.abs();
            for (aij, xj) in row.iter().zip(x.iter()) {
                let u = xi * aij * xj;
                d += u;
                abs += u.abs();
            }
        }
        (d, abs)
    }

    /// `Value` over a box, in rounded intervals: `c + Σ_i (2 b_i x_i + Σ_j x_i a_ij x_j)`,
    /// the terms in [`Self::value`]'s order.
    fn value_over(&self, x: &[Ivl; 3]) -> Ivl {
        let mut d = Ivl::from(self.c);
        for ((bi, row), xi) in self.b.iter().zip(self.a.iter()).zip(x.iter()) {
            d = d + Ivl::from(2.0 * bi) * *xi;
            for (aij, xj) in row.iter().zip(x.iter()) {
                d = d + *xi * Ivl::from(*aij) * *xj;
            }
        }
        d
    }

    /// `Gradient` over a box, in rounded intervals: `2 (A x + b)`, exact as an enclosure for
    /// the affine gradient (component `j`: `2 (Σ_l a_jl x_l + b_j)`).
    fn gradient_over(&self, x: &[Ivl; 3]) -> [Ivl; 3] {
        core::array::from_fn(|j| {
            let row = self.a.get(j).copied().unwrap_or([f64::NAN; 3]);
            let mut g = Ivl::from(self.b.get(j).copied().unwrap_or(f64::NAN));
            for (ajl, xl) in row.iter().zip(x.iter()) {
                g = g + Ivl::from(*ajl) * *xl;
            }
            Ivl::from(2.0) * g
        })
    }

    /// `Gradient`: `2 (A x + b)`.
    fn gradient(&self, x: &[f64; 3]) -> [f64; 3] {
        let [x0, x1, x2] = *x;
        let mut g = [0.0; 3];
        for ((gi, row), bi) in g.iter_mut().zip(self.a.iter()).zip(self.b.iter()) {
            let [r0, r1, r2] = *row;
            *gi = 2.0 * (r0 * x0 + r1 * x1 + r2 * x2 + bi);
        }
        g
    }
}

/// `sm_BallInverse` (the fork's 7048946): the inverse of `m` by its adjugate, `None` where
/// its determinant is below `SM_EFF_ZERO_SQRT` of its largest entry cubed.
fn ball_inverse(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let e = |i, j| at3(m, i, j);
    let det = e(0, 0) * (e(1, 1) * e(2, 2) - e(1, 2) * e(2, 1))
        - e(0, 1) * (e(1, 0) * e(2, 2) - e(1, 2) * e(2, 0))
        + e(0, 2) * (e(1, 0) * e(2, 1) - e(1, 1) * e(2, 0));
    let mut scale = 0.0;
    for row in m {
        for v in row {
            scale = max(scale, v.abs());
        }
    }
    // !(|det| > ...), NaN included
    if det
        .abs()
        .partial_cmp(&(EFF_ZERO_SQRT * scale * scale * scale))
        != Some(std::cmp::Ordering::Greater)
    {
        return None;
    }
    let mut inv = [[0.0; 3]; 3];
    for (i, row) in inv.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let (i1, i2, j1, j2) = ((j + 1) % 3, (j + 2) % 3, (i + 1) % 3, (i + 2) % 3);
            *v = (e(i1, j1) * e(i2, j2) - e(i1, j2) * e(i2, j1)) / det;
        }
    }
    Some(inv)
}

/// `sm_BallCertifiedRoot` (the fork's 7048946): the one point near the origin where three
/// quadrics' implicit equations vanish: Newton's method from the origin, then Krawczyk's
/// test on the box of half-width `half` about the iterate, which proves it holds exactly
/// one root (the gradients are affine, so the Jacobian over the box is the iterate's plus
/// `2 A` times the box, exactly), through the crate's one Krawczyk operator in rounded
/// intervals (`geom::krawczyk`, tangency-labs/solidcraft#15140; the fork bounds the rounding
/// by the terms' magnitudes in floating point instead). The iterate, and whether it is
/// proved (`false` where Newton fails or the test does not prove the box). Bounded: at most
/// 50 Newton steps.
fn ball_certified_root(f: &[BallImplicit; 3], half: f64) -> ([f64; 3], bool) {
    let mut x = [0.0; 3];
    let mut fx = [0.0; 3];
    let mut jac = [[0.0; 3]; 3];
    for it in 0..50 {
        for ((fi, ji), fq) in fx.iter_mut().zip(jac.iter_mut()).zip(f.iter()) {
            *fi = fq.value(&x).0;
            *ji = fq.gradient(&x);
        }
        let Some(y) = ball_inverse(&jac) else {
            return (x, false);
        };
        let mut step = 0.0;
        for (xi, yi) in x.iter_mut().zip(y.iter()) {
            let [y0, y1, y2] = *yi;
            let [f0, f1, f2] = fx;
            let d = y0 * f0 + y1 * f1 + y2 * f2;
            *xi -= d;
            step = max(step, d.abs());
        }
        if step <= 0.01 * half && it > 0 {
            break;
        }
    }
    // Krawczyk's test on ball's own box X = x ± half, through the crate's one operator
    // (#15140): Y the inverse of F′ at the iterate, F(x) and F′(X) in rounded intervals
    // (the gradients are affine, so F′(X) = 2 (A_i X + b_i) exactly as intervals), and
    // K = x − Y F(x) + (I − Y F′(X)) (X − x) strictly inside X proves exactly one root in X
    for (ji, fq) in jac.iter_mut().zip(f.iter()) {
        *ji = fq.gradient(&x);
    }
    let Some(y) = ball_inverse(&jac) else {
        return (x, false);
    };
    let xi = x.map(Ivl::from);
    let xb = x.map(|m| Ivl::new(m - half, m + half));
    let at_x: [Ivl; 3] = core::array::from_fn(|i| {
        f.get(i)
            .map_or_else(|| Ivl::from(f64::NAN), |q| q.value_over(&xi))
    });
    let over: [[Ivl; 3]; 3] = core::array::from_fn(|i| {
        f.get(i)
            .map_or_else(|| [Ivl::from(f64::NAN); 3], |q| q.gradient_over(&xb))
    });
    let (c, _) = contraction(&y, &over);
    let k = k_operator(&x, &y, &at_x, &c, &xb);
    let inside = k.iter().zip(&xb).all(|(a, b)| a.lo > b.lo && a.hi < b.hi);
    (x, inside)
}

/// `GetSurface(rail)->GetOffsetDistance()` of a solver, absolute (`NER` on none).
fn offset_radius(solver: &FilletSolver, rail: usize, op: &'static str) -> Result<f64> {
    let slot = solver
        .surface(rail)?
        .ok_or_else(|| SolidError::blend_missing(op, "the rail's offset surface"))?;
    Ok(slot.surface.offset_distance().abs())
}

/// `SmBlendCorner::ClassifyBall` (the fork's 79d4763, a6243fa, 7048946, 278bf77): whether
/// the filleted `vertex` is a Ball. Every edge there is blended (`n >= 3` solver edge uses,
/// `all_eus` the corner's ring of `n`), each a constant circular round of the same radius
/// (within the rounds' least tolerance), all convex or all concave, the `n` faces
/// distinct: for three, each a plane or a natural quadric (a cylinder or sphere wider than
/// the ball); for four or more, planes whose rounds' offset planes all pass through the
/// point where the first three meet (`sm_BallPlanesMeet`, within the tolerance). The first
/// three faces' normals at the vertex are independent, and the vertex is not set back
/// (`set_back`, the fork's `IsSetBackVertex`). [`BlendCornerEvent::Ball`] where it is, else
/// [`BlendCornerEvent::None`] (the corner hierarchy builds it).
///
/// # Errors
///
/// [`SolidError::null_pointer`] for a missing edge or face surface the fork
/// dereferences (`NER`); those of the solver lookups, the convexity test and the
/// quadric recognition.
pub fn classify_ball(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    set_back: bool,
    solvers: &dyn BallSolvers,
) -> Result<BlendCornerEvent> {
    let nul = || {
        SolidError::blend_missing(
            OP_CLASSIFY,
            "topology or a solver the ball corner's classification reads",
        )
    };
    let none = Ok(BlendCornerEvent::None);
    let n = all_eus.len();
    if n < 3 || solver_eus.len() != n || set_back {
        return none;
    }

    // constant circular rounds of one radius, of one convexity
    let mut radius = -1.0;
    let mut tol = BIG_DOUBLE;
    let mut concave0 = false;
    let mut ids = Vec::new();
    for (ii, &eu) in all_eus.iter().enumerate() {
        let Some(id) = solver_of_edgeuse_or_mate(b, eu, solvers)? else {
            return none;
        };
        ids.push(id);
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

    // n distinct faces: for three, each a plane or a natural quadric (a cylinder or sphere
    // wider than the ball); for four or more, planes; the first three's normals at the
    // vertex independent
    let faces = ring_faces(b, all_eus)?;
    let at = vertex::point(b, vertex)?;
    let mut normals = vec![Vec3::default(); n];
    for (ii, (face, normal)) in faces.iter().zip(normals.iter_mut()).enumerate() {
        let Some(face) = *face else {
            return none;
        };
        if faces.iter().take(ii).any(|f| *f == Some(face)) {
            return none;
        }
        let quadric = ball_face_quadric(k, b, face, tol)?;
        if quadric.kind == QuadricKind::None || (n > 3 && quadric.kind != QuadricKind::Plane) {
            return none;
        }
        let Ok(n) = quadric.normal(at).unitize() else {
            return none; // a cone's apex
        };
        *normal = n;
        if matches!(quadric.kind, QuadricKind::Sphere | QuadricKind::Cylinder)
            && quadric.radius.partial_cmp(&(radius + tol)) != Some(std::cmp::Ordering::Greater)
        {
            return none; // a ball as wide as the cylinder or sphere: its offset is its axis or centre
        }
    }
    let (Some(&n0), Some(&n1), Some(&n2)) = (normals.first(), normals.get(1), normals.get(2))
    else {
        return none;
    };
    if n0.dot(n1.cross(n2)).abs() <= EFF_ZERO_SQRT {
        return none;
    }
    if n > 3 {
        // the rounds' offset planes of their faces (round i rolls on faces i and i + 1), all
        // through one point
        let mut on = Vec::with_capacity(n);
        let mut off_normal = Vec::with_capacity(n);
        for (&id, face) in ids.iter().zip(faces.iter()) {
            let fs = solvers.solver(id)?;
            let rail = fs.find_index_of_rail_on_face(b, *face)?;
            if rail > 1 {
                return none;
            }
            let off = &fs.surface(rail)?.ok_or_else(nul)?.surface;
            let uv = off.surface.natural_uv_domain().evaluate(0.5, 0.5);
            on.push(off.surface.evaluate_point(k, uv)?);
            off_normal.push(
                off.surface
                    .evaluate_normal(k, uv, Side::Left, Side::Left)?
                    .unitize()?,
            );
        }
        let (Some(&o0), Some(&o1), Some(&o2), Some(&m0), Some(&m1), Some(&m2)) = (
            on.first(),
            on.get(1),
            on.get(2),
            off_normal.first(),
            off_normal.get(1),
            off_normal.get(2),
        ) else {
            return none;
        };
        let c = planes_meet(&[o0, o1, o2], &[m0, m1, m2])?;
        for (m, o) in off_normal.iter().zip(on.iter()).skip(3) {
            if m.dot(c - *o).abs() > tol {
                return none; // no common ball: a Setback
            }
        }
    }
    Ok(BlendCornerEvent::Ball)
}

/// `SmBlendCorner::BallCentre` (the fork's 4241a1b, 7048946, 278bf77): the ball's centre,
/// where the first three faces' offsets by the radius meet (in closed form where two or
/// three are planes, else the certified root), held for four or more planes to the others'
/// offsets; its radius and the contacts `P_i` with the `n` faces; checked to be the ball of
/// round 0's section at vertex 0 and the contacts to be the corner's vertices.
fn ball_centre(
    k: &Kernel,
    target: &Brep,
    pseudo: &FilletBrep,
    corner: &FilletCorner,
    solvers: &dyn BallSolvers,
) -> Result<(Point3, f64, Vec<Point3>)> {
    let nul = || {
        SolidError::blend_missing(
            OP_CENTRE,
            "topology or a solver the ball's centre is found from",
        )
    };
    let err = || SolidError::blend_failed(OP_CENTRE, "the ball's centre could not be found");
    let all_eus = corner.all_eus.as_deref().ok_or_else(nul)?;
    let n = all_eus.len();
    if n < 3 || corner.vertices.len() != n || corner.edges.len() != n {
        return Err(err());
    }
    let (Some(&v0), Some(&e0)) = (corner.vertices.first(), corner.edges.first()) else {
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

    // each face's quadric offset by r on the ball's side (the side of it the rounds' ball is
    // on, seen from vertex i, the contact on it)
    let faces = ring_faces(target, all_eus)?;
    let vertices = corner.vertices.as_slice();
    let mut offsets = vec![BlendQuadric::default(); n];
    let mut side = vec![0.0; n];
    let mut planes = 0usize; // of the first three
    for (ii, ((face, &v), (offset, sd))) in faces
        .iter()
        .zip(vertices.iter())
        .zip(offsets.iter_mut().zip(side.iter_mut()))
        .enumerate()
    {
        let face = face.ok_or_else(nul)?;
        let quadric = ball_face_quadric(k, target, face, tol)?;
        if quadric.kind == QuadricKind::None {
            return Err(err()); // ClassifyBall took the faces to be planes or quadrics
        }
        let on = pseudo.vertex_point(v)?;
        *sd = if quadric.normal(on).dot(ball - on) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        *offset = quadric.offset(*sd * radius, tol);
        if offset.kind != quadric.kind || offset.side != quadric.side {
            return Err(err()); // ClassifyBall took the quadrics wider than the ball
        }
        if ii < 3 && offset.kind == QuadricKind::Plane {
            planes += 1;
        }
        if ii >= 3 && offset.kind != QuadricKind::Plane {
            return Err(err()); // ClassifyBall took four or more faces to be planes
        }
    }

    // C: where the first three offsets meet, about the rounds' ball (each quadric's implicit
    // form there)
    let mut order = [0usize, 1, 2]; // the planes first
    let mut at = 0usize;
    for (ii, o) in offsets.iter().take(3).enumerate() {
        if o.kind == QuadricKind::Plane {
            if let Some(slot) = order.get_mut(at) {
                *slot = ii;
            }
            at += 1;
        }
    }
    let mut at = planes;
    for (ii, o) in offsets.iter().take(3).enumerate() {
        if o.kind != QuadricKind::Plane {
            if let Some(slot) = order.get_mut(at) {
                *slot = ii;
            }
            at += 1;
        }
    }
    let mut f = [BallImplicit::default(); 3];
    for (fi, &oi) in f.iter_mut().zip(order.iter()) {
        let o = offsets.get(oi).ok_or_else(err)?;
        *fi = BallImplicit::new(o, ball);
    }
    let [f0, f1, f2] = f;
    let mut x = [0.0; 3];
    if planes >= 2 {
        // the two offset planes' line, n_0 . x = -c_0 / 2 and n_1 . x = -c_1 / 2 (2 b = n)
        let n0 = Vec3::new(2.0 * f0.b[0], 2.0 * f0.b[1], 2.0 * f0.b[2]);
        let n1 = Vec3::new(2.0 * f1.b[0], 2.0 * f1.b[1], 2.0 * f1.b[2]);
        let dv = n0.cross(n1);
        let dd = dv.dot(dv);
        if dd <= EFF_ZERO {
            return Err(err());
        }
        // its point nearest the origin: (h_0 (n_1 x d) + h_1 (d x n_0)) / |d|^2, h_i = -c_i
        let p0v = (n1.cross(dv) * (-f0.c) + dv.cross(n0) * (-f1.c)) * (1.0 / dd);
        let p0 = [p0v.x, p0v.y, p0v.z];
        let d = [dv.x, dv.y, dv.z];
        if planes == 3 {
            // the third plane: n_2 . (p0 + t d) = -c_2
            let den = 2.0 * (f2.b[0] * d[0] + f2.b[1] * d[1] + f2.b[2] * d[2]);
            if den.abs() <= EFF_ZERO {
                return Err(err());
            }
            let t = -f2.value(&p0).0 / den;
            for ((xk, pk), dk) in x.iter_mut().zip(p0.iter()).zip(d.iter()) {
                *xk = pk + t * dk;
            }
        } else {
            // the quadric along the line: q2 t^2 + 2 q1 t + q0 = 0, the root nearest the
            // rounds' ball
            let mut qa = 0.0;
            let mut qb = 0.0;
            for ((row, dk), bk) in f2.a.iter().zip(d.iter()).zip(f2.b.iter()) {
                let mut ad = 0.0;
                let mut ap = 0.0;
                for ((akm, dm), pm) in row.iter().zip(d.iter()).zip(p0.iter()) {
                    ad += akm * dm;
                    ap += akm * pm;
                }
                qa += dk * ad;
                qb += dk * (ap + bk);
            }
            let qc = f2.value(&p0).0;
            let disc = qb * qb - qa * qc;
            if disc < 0.0 || (qa.abs() <= EFF_ZERO * dd && qb.abs() <= EFF_ZERO * dd) {
                return Err(err()); // the line misses the offset quadric
            }
            let mut roots = [0.0; 2];
            let count;
            if qa.abs() <= EFF_ZERO * dd {
                roots[0] = -qc / (2.0 * qb);
                count = 1;
            } else {
                // without cancellation: q = -(b + sign(b) sqrt(disc)), t = q / a and c / q
                let sign = if qb >= 0.0 { 1.0 } else { -1.0 };
                let q = -(qb + sign * disc.sqrt());
                roots[0] = q / qa;
                if q == 0.0 {
                    count = 1;
                } else {
                    roots[1] = qc / q;
                    count = 2;
                }
            }
            // the root nearest the origin (the rounds' ball): |p0 + t d|, p0 square to d
            let mut best = roots[0];
            for &t in roots.iter().take(count).skip(1) {
                if t.abs() < best.abs() {
                    best = t;
                }
            }
            for ((xk, pk), dk) in x.iter_mut().zip(p0.iter()).zip(d.iter()) {
                *xk = pk + best * dk;
            }
        }
    } else {
        // no closed form: the root certified within rounding of the part's size
        let mut size = 0.0;
        for face in faces.iter().take(3) {
            let face = face.ok_or_else(nul)?;
            let bx = crate::query::face::p1::calculate_bounding_box(k, target, face, false)?;
            size = max(size, (bx.max - bx.min).length());
        }
        k.work_counters.add_ball_certified_root();
        let (root, proved) = ball_certified_root(&f, 1e-12 * max(size, 1.0));
        if !proved {
            return Err(err());
        }
        x = root;
    }
    let centre = Point3::new(ball.x + x[0], ball.y + x[1], ball.z + x[2]);
    if centre.distance(ball) > tol {
        return Err(err()); // the rounds' ball is the offsets' meeting point
    }
    // four or more planes: the others' offsets pass through it (n . x + c, n = 2 b)
    for o in offsets.iter().skip(3) {
        let fo = BallImplicit::new(o, centre);
        let x0 = [0.0; 3];
        let dn = 2.0 * (fo.b[0] * fo.b[0] + fo.b[1] * fo.b[1] + fo.b[2] * fo.b[2]).sqrt();
        if dn.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || fo.value(&x0).0.abs() > tol * dn
        {
            return Err(err()); // ClassifyBall found the ball common to every plane
        }
    }
    // the contacts: the centre moved back along the offset's normal there (the face's at the
    // contact)
    let mut contacts = vec![Point3::default(); n];
    for (((&v, contact), o), &s) in vertices
        .iter()
        .zip(contacts.iter_mut())
        .zip(offsets.iter())
        .zip(side.iter())
    {
        let n = o.normal(centre).unitize()?;
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
    let nul = || {
        SolidError::blend_missing(
            OP_PLACE,
            "topology or a solver the ball's contacts are placed from",
        )
    };
    let err = || SolidError::blend_failed(OP_PLACE, "the ball's contacts could not be placed");
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
            .ok_or_else(|| {
                SolidError::blend_failed(OP_PATCH_NET, "a control point past the patch's net")
            })?;
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
pub(crate) fn ball_triangle_patch(
    centre: Point3,
    radius: f64,
    dirs: &[Vec3; 3],
    pole: usize,
    reverse_s: bool,
) -> Result<(BSplineSurface, usize, usize)> {
    let err = || {
        SolidError::blend_failed(
            OP_PATCH_NET,
            "the ball's triangular patch could not be built",
        )
    };
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

/// `SmBlendCorner::BuildBallSurface` (the fork's 79d4763, a6243fa, f8e6709, 278bf77; run by
/// `CalcCornerEdgeGeom` on a Ball before the edges' own kinds compute their curves): the
/// sphere's n-gon between the rounds' end sections, fanned from contact 0 into the `n - 2`
/// triangles `(0, k, k + 1)`, each one rational patch on the ball's sphere with the pole at
/// contact 0 and its normal out of the material (towards the centre where the rounds add
/// material, away from it where they remove it), its corners at the contacts, and its loop
/// (its natural boundary, the pole left out: round `k`'s section, and through the pole
/// round 0's and round `n - 1`'s sections for the first and last triangles, else the great
/// circles from contact 0 that the neighbouring patches share), kept in
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
    let nul = || {
        SolidError::blend_missing(
            OP_BUILD,
            "topology or geometry the ball corner's edges are built from",
        )
    };
    let err = || SolidError::blend_failed(OP_BUILD, "the ball corner's edges could not be built");
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

    let n = contacts.len();
    let mut dirs = Vec::with_capacity(n);
    for &contact in &contacts {
        dirs.push((contact - centre).unitize()?);
    }
    let mut ball = BallCorner::default();
    for kk in 1..n.saturating_sub(1) {
        // the triangle (0, k, k + 1): the pole at contact 0, its normal turned as the face's
        let (Some(&d0), Some(&dk), Some(&dk1)) = (dirs.first(), dirs.get(kk), dirs.get(kk + 1))
        else {
            return Err(err());
        };
        let tri = [d0, dk, dk1];
        let index = [0, kk, kk + 1];
        let pole = 0;
        let mut from = 0;
        let mut to = 0;
        let mut chosen: Option<Surface> = None;
        for turn in 0..2 {
            if chosen.is_some() {
                break;
            }
            let (net, f, t) = ball_triangle_patch(centre, radius, &tri, pole, turn == 1)?;
            from = f;
            to = t;
            let patch = Surface::from(net.clone());
            let mid = patch.natural_uv_domain_with_nurb(k)?.evaluate(0.5, 0.5);
            let at = patch.evaluate_point(k, mid)?;
            let normal = patch.evaluate_normal(k, mid, Side::Left, Side::Left)?;
            if (normal.dot(at - centre) < 0.0) == adds_material {
                // the face's surface carries its sphere as its definition (its offset is
                // exact; the fork's SmBallPatch::Create)
                chosen = Some(Surface::from(BallPatch::create(&net, centre, radius)?));
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
        for (&uv, at) in corners_uv.iter().zip([from, to, pole]) {
            let at_vertex = *index.get(at).ok_or_else(err)?;
            let v = *corner.vertices.get(at_vertex).ok_or_else(err)?;
            if surface
                .evaluate_point(k, uv)?
                .distance(pseudo.vertex_point(v)?)
                > tol
            {
                return Err(err());
            }
        }

        // its loop: three sides, the side at the pole a point
        let trims =
            surface.create_natural_uv_trim_curves(k, &nurb_domain, SurfParamType::V, true)?;
        let count = trims.curves_3d.len();
        // each patch keeps its definition (#2357): the ball's sphere and the patch's own loop,
        // certified on it
        let mut surface = surface;
        if k.fixes.fillet_round_as_definition {
            let boundary = trims
                .curves_3d
                .iter()
                .map(|c| nurbs_curve(c).cloned())
                .collect::<Result<_>>()?;
            let definition = CornerDefinition::ball(centre, radius, boundary)?;
            definition.recertify(k, &surface)?;
            surface.set_corner_definition(definition);
        }
        ball.curves_3d.extend(trims.curves_3d);
        ball.uv_curves
            .extend(trims.uv_curves.iter().cloned().map(Curve::from));
        ball.orients.extend(trims.orients.iter().copied());
        if count != 3 || trims.uv_curves.len() != 3 || trims.orients.len() != 3 {
            return Err(err());
        }
        ball.surfaces.push(surface);
        ball.loops.push(3);
    }
    *corner.ball.as_mut().ok_or_else(nul)? = ball;
    Ok(())
}

/// `SmBlendCorner::CreateBallPatch` (the fork's 79d4763, 278bf77): the corner's faces, each
/// patch bounded by its natural loop (`MakeFaceWithCurves` in the fillet B-rep's infinite
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
    let nul = || SolidError::blend_missing(OP_PATCH, "the ball corner's event or topology");
    let ball = corner.ball.as_mut().ok_or_else(nul)?;
    if ball.surfaces.is_empty() || ball.loops.len() != ball.surfaces.len() {
        return Err(SolidError::blend_failed(
            OP_PATCH,
            "the ball's surfaces and loops do not pair up",
        ));
    }
    let surfaces = std::mem::take(&mut ball.surfaces);
    let loops = std::mem::take(&mut ball.loops);
    let mut curves_3d = std::mem::take(&mut ball.curves_3d).into_iter();
    let mut uv_curves = std::mem::take(&mut ball.uv_curves).into_iter();
    let mut orients = std::mem::take(&mut ball.orients).into_iter();
    for (surface, count) in surfaces.into_iter().zip(loops) {
        // the face takes the surface and its loop's curves
        let mut c3 = Vec::new();
        let mut uv = Vec::new();
        let mut or = Vec::new();
        for _ in 0..count {
            let (Some(c), Some(u), Some(o)) = (curves_3d.next(), uv_curves.next(), orients.next())
            else {
                return Err(SolidError::blend_failed(
                    OP_PATCH,
                    "a ball loop lacks a curve, its uv curve or its orientation",
                ));
            };
            c3.push(Some(c));
            uv.push(u);
            or.push(o);
        }
        let fillet = env.fillet.as_mut().ok_or_else(nul)?;
        let input = FaceCurvesInput {
            loops: vec![count],
            curves_3d: Some(c3),
            uv_curves: Some(uv),
            orientations: or,
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
        // the corner's patch faces (SmFilletCorner::GetPatchFaces, the stitch)
        corner.surfaces.push(surface);
        corner.surface_faces.push(face);
    }
    if let Some(ball) = corner.ball.as_mut() {
        ball.delete_surface();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sphere of centre `c` and radius `r`, as `x . x + 2 b . x + c` ([`BallImplicit`]).
    fn sphere(c: [f64; 3], r: f64) -> BallImplicit {
        BallImplicit {
            a: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            b: [-c[0], -c[1], -c[2]],
            c: c[0] * c[0] + c[1] * c[1] + c[2] * c[2] - r * r,
        }
    }

    /// #15140: three spheres of radius 1 centred at `p + e_i`, which meet at `p` (near the
    /// origin, where Newton starts) and at `p + (2/3)(1, 1, 1)`: the ball's centre is proved
    /// through the crate's Krawczyk operator on the box `x ± half`, and that box holds the
    /// exact meeting point `p`.
    #[test]
    fn three_spheres_meeting_near_the_origin_are_proved_at_their_point() {
        let p = [1e-4, -2e-4, 3e-4];
        let f = [
            sphere([p[0] + 1.0, p[1], p[2]], 1.0),
            sphere([p[0], p[1] + 1.0, p[2]], 1.0),
            sphere([p[0], p[1], p[2] + 1.0], 1.0),
        ];
        let half = 1e-12;
        let (x, proved) = ball_certified_root(&f, half);
        assert!(proved, "not proved at {x:?}");
        for (xi, pi) in x.iter().zip(&p) {
            assert!((xi - pi).abs() <= half, "{x:?} against {p:?}");
        }
    }

    /// #15140: two of the spheres tangent at `p` (one inside the other, both normal to z
    /// there), so the three gradients at the meeting are dependent: no ball centre is proved.
    #[test]
    fn spheres_tangent_at_their_meeting_are_refused() {
        let p = [1e-4, -2e-4, 3e-4];
        let f = [
            sphere([p[0], p[1], p[2] + 1.0], 1.0),
            sphere([p[0], p[1], p[2] + 2.0], 2.0),
            sphere([p[0] + 1.0, p[1], p[2]], 1.0),
        ];
        let (x, proved) = ball_certified_root(&f, 1e-12);
        assert!(!proved, "proved at {x:?}");
    }

    /// Three planes through one line meet in no single point: a typed `Failed`, with the
    /// `SM_ERR` status the bare status had.
    #[test]
    fn planes_through_one_line_are_a_typed_failure() {
        let on = [Point3::ZERO; 3];
        let normal = [Vec3::X, Vec3::Y, (Vec3::X + Vec3::Y).unitize().unwrap()];
        let err = planes_meet(&on, &normal).unwrap_err();
        assert_eq!(
            err.blend_failure(),
            Some(&crate::error::blend::BlendFailure::Failed(
                "the three faces' planes do not meet in one point"
            ))
        );
        assert_eq!(
            crate::error::upstream_status(&err),
            Some(crate::error::SM_ERR)
        );
    }

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
