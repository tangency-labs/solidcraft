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
//! The patch also keeps its corner
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
use crate::error::blend::{BlendFailure, CornerInput, CornerStep, FilletSetUp, SolverFailure};
use crate::error::{EntityRef, Failure, Result, SolidError, SM_ERR_NULL_POINTER};
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

/// A defect of `op`: a step the ball corner's own checks or constructions promise did not
/// hold (`why`).
#[track_caller]
fn defect(op: &'static str, why: &'static str) -> SolidError {
    SolidError::blend(op, BlendFailure::Defect { why })
}

/// A defect of `op` where something the kernel built is missing (`why`), with the
/// `SM_ERR_NULL_POINTER` status of the `NER` it replaces.
#[track_caller]
fn missing_defect(op: &'static str, why: &'static str) -> SolidError {
    Failure::new(op, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Invalid input to `op`: the caller's corner, fillet B-rep or body is not what the corner's
/// construction reads (`input`), about `entity` where the check reads one, with the status
/// the site returned before ([`CornerInput::upstream_status`]). The entry points are public
/// and take a caller's corner.
#[track_caller]
fn bad_input(op: &'static str, input: CornerInput, entity: Option<EntityRef>) -> SolidError {
    corner_input(op, input, entity).into_error()
}

/// The [`Failure`] of [`bad_input`], located at `entity` where there is one.
#[track_caller]
fn corner_input(op: &'static str, input: CornerInput, entity: Option<EntityRef>) -> Failure {
    let failure = Failure::new(op, BlendFailure::CornerInputInvalid { input, entity });
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
}

/// A face of the caller's body that the corner reads, `face`, has no surface: the caller's
/// input, [`BlendFailure::FaceHasNoSurface`] at the face, status `SM_ERR_NULL_POINTER` (the
/// site's).
#[track_caller]
fn face_without_surface(op: &'static str, face: FaceId) -> SolidError {
    Failure::new(op, BlendFailure::FaceHasNoSurface)
        .with_status(SM_ERR_NULL_POINTER)
        .at(EntityRef::Face(face))
        .into_error()
}

/// An edge use of the caller's body or fillet B-rep, `edgeuse`, has no edge: the caller's
/// input, [`BlendFailure::EdgeUseWithoutEdge`] at the edge use, status
/// `SM_ERR_NULL_POINTER`.
#[track_caller]
fn edge_use_without_edge(op: &'static str, edgeuse: EdgeuseId) -> SolidError {
    SolidError::blend_at(
        op,
        BlendFailure::EdgeUseWithoutEdge,
        EntityRef::Edgeuse(edgeuse),
    )
}

/// Invalid input to `op`: the caller's fillet solver set-up lacks what the corner reads
/// (`cause`, [`BlendFailure::Solver`]), about `entity` where the check reads one; status
/// the cause's, `SM_ERR_NULL_POINTER`, the site's.
#[track_caller]
fn solver_input(op: &'static str, cause: SolverFailure, entity: Option<EntityRef>) -> SolidError {
    let failure = Failure::new(op, BlendFailure::Solver(cause));
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
    .into_error()
}

/// Invalid input to `op`: a link of the caller's fillet structures that the corner reads is
/// not there (`part`), about `entity` where the check reads one; status
/// `SM_ERR_NULL_POINTER`, the site's.
#[track_caller]
fn bad_setup(op: &'static str, part: FilletSetUp, entity: Option<EntityRef>) -> SolidError {
    let failure = Failure::new(op, BlendFailure::FilletSetUpInvalid(part));
    match entity {
        Some(at) => failure.at(at),
        None => failure,
    }
    .into_error()
}

/// The corner has no construction at `step` (status `SM_ERR`).
#[track_caller]
fn unbuildable(op: &'static str, step: CornerStep) -> SolidError {
    SolidError::blend(op, BlendFailure::CornerUnbuildable(step))
}

/// [`unbuildable`] with the `SM_ERR_NULL_POINTER` status of the site it replaces.
#[track_caller]
fn unbuildable_missing(op: &'static str, step: CornerStep) -> SolidError {
    Failure::new(op, BlendFailure::CornerUnbuildable(step))
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// `a[i]` of a triple, the last for any `i` past the second (the callers' indices are
/// below 3).
fn of3<T: Copy>(a: &[T; 3], i: usize) -> T {
    let [x, y, z] = *a;
    match i {
        0 => x,
        1 => y,
        _ => z,
    }
}

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
        // the offset planes' normals are the first three faces' normals, which
        // classify_ball checked are independent (|det| > EFF_ZERO_SQRT)
        return Err(unbuildable(
            OP_CLASSIFY,
            CornerStep::BallOffsetPlanesDegenerate,
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
    let surface = face2::surface(b, face)?.ok_or_else(|| face_without_surface(OP_QUADRIC, face))?;
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
        // `GetSurface(rail)->GetOffsetDistance()`, absolute, of the two rails just found
        let (Some(fsg), Some(s0), Some(s1)) = (fs.fsg.as_ref(), fs.surface(0)?, fs.surface(1)?)
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
        let r0 = s0.surface.offset_distance().abs();
        let r1 = s1.surface.offset_distance().abs();
        if (r0 - r1).abs() > tol || (radius >= 0.0 && (r0 - radius).abs() > tol) {
            return none;
        }
        radius = r0;
        let e = eu1::edge(b, eu)?.ok_or_else(|| edge_use_without_edge(OP_CLASSIFY, eu))?;
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
            // both rails' offset surfaces were found above, for every round
            let (Some(s0), Some(s1)) = (fs.surface(0)?, fs.surface(1)?) else {
                return none;
            };
            let off = match rail {
                0 => &s0.surface,
                1 => &s1.surface,
                _ => return none,
            };
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
    let all_eus = corner
        .all_eus
        .as_deref()
        .ok_or_else(|| bad_input(OP_CENTRE, CornerInput::RingNotSet, None))?;
    let n = all_eus.len();
    let (true, Some(&v0), Some(&e0)) = (
        n >= 3 && corner.vertices.len() == n && corner.edges.len() == n,
        corner.vertices.first(),
        corner.edges.first(),
    ) else {
        return Err(bad_input(OP_CENTRE, CornerInput::TopologyMismatch, None));
    };
    let tol = corner.approx_tol_3d;

    // a ball of the rounds at the contacts: round 0's offset surfaces at vertex 0
    let section0 = eu1::primary_edgeuse(pseudo.brep(), e0)?.ok_or_else(|| {
        bad_setup(
            OP_CENTRE,
            FilletSetUp::PrimaryEdgeUse,
            Some(EntityRef::Edge(e0)),
        )
    })?;
    let g0 = pseudo
        .fillet_edgeuse(section0)?
        .fillet_geom()
        .ok_or_else(|| {
            bad_setup(
                OP_CENTRE,
                FilletSetUp::CrossSectionGeom,
                Some(EntityRef::Edgeuse(section0)),
            )
        })?;
    let vu = pseudo
        .vu_at_rail_end(v0, Some(g0), None, None)?
        .ok_or_else(|| {
            bad_setup(
                OP_CENTRE,
                FilletSetUp::RailEndVertexUse,
                Some(EntityRef::Vertex(v0)),
            )
        })?;
    let tsect = pseudo.fillet_vertexuse(vu)?.tsect_pnt().clone();
    let solver_id = pseudo.geom(g0)?.fillet_solver().ok_or_else(|| {
        solver_input(
            OP_CENTRE,
            SolverFailure::FilletGeomWithoutSolver { fillet_geom: g0 },
            None,
        )
    })?;
    let solver = solvers.solver(solver_id)?;
    let mut on = [Point3::default(); 2];
    for (p, side) in on.iter_mut().zip([SrfIndex::First, SrfIndex::Second]) {
        let rail = usize::from(side == SrfIndex::Second);
        let offset = pseudo
            .geom(g0)?
            .offset_surface(rail, Some(solver))?
            .ok_or_else(|| bad_setup(OP_CENTRE, FilletSetUp::RailOffsetSurface, None))?;
        *p = offset.surface.evaluate_point(k, tsect.uv_pos(side))?;
    }
    if on[0].distance(on[1]) > tol {
        return Err(unbuildable(OP_CENTRE, CornerStep::RoundSectionOffsetsApart));
    }
    let ball = on[0];
    let radius = pseudo
        .geom(g0)?
        .offset_surface(0, Some(solver))?
        .ok_or_else(|| bad_setup(OP_CENTRE, FilletSetUp::RailOffsetSurface, None))?
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
    let mut face_ids = Vec::new();
    for (ii, ((face, &v), (offset, sd))) in faces
        .iter()
        .zip(vertices.iter())
        .zip(offsets.iter_mut().zip(side.iter_mut()))
        .enumerate()
    {
        let face = face.ok_or_else(|| {
            solver_input(
                OP_CENTRE,
                SolverFailure::EdgeUseWithoutFace,
                all_eus.get(ii).copied().map(EntityRef::Edgeuse),
            )
        })?;
        face_ids.push(face);
        let quadric = ball_face_quadric(k, target, face, tol)?;
        if quadric.kind == QuadricKind::None {
            return Err(bad_input(
                OP_CENTRE,
                CornerInput::FaceNotAsClassified,
                Some(EntityRef::Face(face)),
            ));
        }
        let on = pseudo.vertex_point(v)?;
        *sd = if quadric.normal(on).dot(ball - on) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        *offset = quadric.offset(*sd * radius, tol);
        if offset.kind != quadric.kind || offset.side != quadric.side {
            return Err(bad_input(
                OP_CENTRE,
                CornerInput::FaceNotAsClassified,
                Some(EntityRef::Face(face)),
            ));
        }
        if ii < 3 && offset.kind == QuadricKind::Plane {
            planes += 1;
        }
        if ii >= 3 && offset.kind != QuadricKind::Plane {
            return Err(bad_input(
                OP_CENTRE,
                CornerInput::FaceNotAsClassified,
                Some(EntityRef::Face(face)),
            ));
        }
    }

    // C: where the first three offsets meet, about the rounds' ball (each quadric's implicit
    // form there)
    let first3 = offsets.iter().take(3);
    let planes_first = first3
        .clone()
        .filter(|o| o.kind == QuadricKind::Plane)
        .chain(first3.filter(|o| o.kind != QuadricKind::Plane)); // the planes first
    let mut f = [BallImplicit::default(); 3];
    for (fi, o) in f.iter_mut().zip(planes_first) {
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
            return Err(unbuildable(
                OP_CENTRE,
                CornerStep::BallOffsetPlanesDegenerate,
            ));
        }
        // its point nearest the origin: (h_0 (n_1 x d) + h_1 (d x n_0)) / |d|^2, h_i = -c_i
        let p0v = (n1.cross(dv) * (-f0.c) + dv.cross(n0) * (-f1.c)) * (1.0 / dd);
        let p0 = [p0v.x, p0v.y, p0v.z];
        let d = [dv.x, dv.y, dv.z];
        if planes == 3 {
            // the third plane: n_2 . (p0 + t d) = -c_2
            let den = 2.0 * (f2.b[0] * d[0] + f2.b[1] * d[1] + f2.b[2] * d[2]);
            if den.abs() <= EFF_ZERO {
                return Err(unbuildable(
                    OP_CENTRE,
                    CornerStep::BallOffsetPlanesDegenerate,
                ));
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
                // the rounds' ball is on all three offsets, so the line meets the quadric
                // but for rounding: where the line is tangent to the offset, or in it,
                // the computed discriminant can fall below zero
                return Err(SolidError::blend(
                    OP_CENTRE,
                    BlendFailure::CornerUnbuildable(CornerStep::BallCentreLineMissesOffset),
                ));
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
        for &face in face_ids.iter().take(3) {
            let bx = crate::query::face::p1::calculate_bounding_box(k, target, face, false)?;
            size = max(size, (bx.max - bx.min).length());
        }
        k.work_counters.add_ball_certified_root();
        let (root, proved) = ball_certified_root(&f, 1e-12 * max(size, 1.0));
        if !proved {
            // classify_ball checked the normals' independence at the vertex, not at the
            // ball, and only to EFF_ZERO_SQRT: nearly dependent normals leave the box too
            // small for the proof, though a root is there
            return Err(SolidError::blend(
                OP_CENTRE,
                BlendFailure::CornerUnbuildable(CornerStep::BallCentreNotProved),
            ));
        }
        x = root;
    }
    let centre = Point3::new(ball.x + x[0], ball.y + x[1], ball.z + x[2]);
    if centre.distance(ball) > tol {
        return Err(unbuildable(OP_CENTRE, CornerStep::BallCentreOffRound));
    }
    // four or more planes: the others' offsets pass through it (n . x + c, n = 2 b)
    for o in offsets.iter().skip(3) {
        let fo = BallImplicit::new(o, centre);
        let x0 = [0.0; 3];
        let dn = 2.0 * (fo.b[0] * fo.b[0] + fo.b[1] * fo.b[1] + fo.b[2] * fo.b[2]).sqrt();
        if dn.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || fo.value(&x0).0.abs() > tol * dn
        {
            return Err(unbuildable(OP_CENTRE, CornerStep::BallPlaneOffCentre));
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
            return Err(unbuildable(OP_CENTRE, CornerStep::BallContactOffVertex));
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
            for side in [SrfIndex::First, SrfIndex::Second] {
                let rail = usize::from(side == SrfIndex::Second);
                let guess = pseudo.fillet_vertexuse(vu)?.tsect_pnt().uv_pos(side);
                let offset = pseudo
                    .geom(g)?
                    .offset_surface(rail, Some(solver))?
                    .ok_or_else(|| bad_setup(OP_PLACE, FilletSetUp::RailOffsetSurface, None))?;
                let domain = offset_natural_uv_domain(offset);
                let solved = offset_local_point_solve(k, offset, &domain, centre, guess)?;
                let Some((uv, _)) = solved else {
                    return Err(unbuildable(OP_PLACE, CornerStep::BallContactFootNotFound));
                };
                let at = offset.surface.evaluate_point(k, uv)?;
                if at.distance(centre) > tol {
                    return Err(unbuildable(OP_PLACE, CornerStep::BallContactOffOffset));
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

    /// Sets `c[i][j]`; nothing out of range (never: the degrees are at most `(4, 2)`).
    fn set(&mut self, i: usize, j: usize, v: f64) {
        if let Some(slot) = self.c.get_mut(i).and_then(|row| row.get_mut(j)) {
            *slot = v;
        }
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
fn times(a: &BallPoly, b: &BallPoly) -> BallPoly {
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
            p.set(i, j, d / (choose(p.m, i) * choose(p.n, j)));
        }
    }
    p
}

/// `sm_BallSum`: `da a + db b`, of one degree.
fn sum(da: f64, a: &BallPoly, db: f64, b: &BallPoly) -> BallPoly {
    let mut p = *a;
    for i in 0..=p.m {
        for j in 0..=p.n {
            p.set(i, j, da * a.at(i, j) + db * b.at(i, j));
        }
    }
    p
}

/// The far side of the ball's triangle `dirs` (unit directions from the ball's centre) as
/// [`ball_triangle_patch`] takes it with the pole at `pole`: the frame of the sphere's
/// stereographic projection from the pole's antipode (`e3` towards that antipode), and in
/// its plane the far side's ends `a` (of contact `pole + 1`) and `b` (of `pole + 2`), the
/// centre `o` and radius `r` of the circle the far side's great circle projects to, the
/// ends' unit directions from `o` (`ua`, `ub`) with their cosine, and where the far side's
/// middle lies along the bisector of those directions (`middle`).
struct FarSide {
    e1: Vec3,
    e2: Vec3,
    e3: Vec3,
    a: [f64; 2],
    b: [f64; 2],
    o: [f64; 2],
    r: f64,
    ua: [f64; 2],
    ub: [f64; 2],
    cos: f64,
    middle: f64,
}

impl FarSide {
    /// Whether the far side's image is one rational quadratic arc: the arc from `a` to `b`
    /// through the middle's image, under a half turn (its middle control point is where
    /// the tangents at its ends cross, its weight the cosine of half its angle). A triangle
    /// with a side so long that no pole makes it one is not one patch: the ball's triangle
    /// at a corner sharper than a regular tetrahedron's (three faces whose normals stand
    /// arccos(-1/3) apart) is the first.
    fn is_one_arc(&self) -> bool {
        1.0 + self.cos > EFF_ZERO_SQRT && self.middle > 0.0
    }
}

/// The far side of the triangle `dirs` from the pole at `pole` ([`FarSide`]); `None` where
/// it has no image to judge: a contact lies at the pole's antipode, or the far side runs
/// through the pole (no triangle).
///
/// # Errors
///
/// Those of the unit vectors.
fn far_side(dirs: &[Vec3; 3], pole: usize) -> Result<Option<FarSide>> {
    let (Some(&d_pole), Some(&d_from), Some(&d_to)) = (
        dirs.get(pole),
        dirs.get((pole + 1) % 3),
        dirs.get((pole + 2) % 3),
    ) else {
        return Ok(None);
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
            return Ok(None);
        }
        *o = [p.dot(e1) / den, p.dot(e2) / den];
    }
    let [a, b, m] = out;
    // the third side's great circle (normal m) projects to the circle about -(m1, m2) / m3,
    // of radius^2 1 + (m1^2 + m2^2) / m3^2
    let sm = d_from.cross(d_to).unitize()?;
    let m3 = sm.dot(e3);
    if m3.abs() <= EFF_ZERO {
        return Ok(None); // the third side runs through the pole: no triangle
    }
    let o = [-sm.dot(e1) / m3, -sm.dot(e2) / m3];
    let r = (1.0 + (o[0] * o[0] + o[1] * o[1])).sqrt();
    let ua = [(a[0] - o[0]) / r, (a[1] - o[1]) / r];
    let ub = [(b[0] - o[0]) / r, (b[1] - o[1]) / r];
    let cos = ua[0] * ub[0] + ua[1] * ub[1];
    let middle = (m[0] - o[0]) * (ua[0] + ub[0]) + (m[1] - o[1]) * (ua[1] + ub[1]);
    Ok(Some(FarSide {
        e1,
        e2,
        e3,
        a,
        b,
        o,
        r,
        ua,
        ub,
        cos,
        middle,
    }))
}

/// The point a ball's triangle `dirs` is fanned about where it is not one patch: the point of
/// the sphere equally far from its three corners, on their side. `None` where that point is
/// not inside the triangle: about a point outside it (an obtuse triangle's) or on a side, the
/// three triangles (centre, i, i + 1) would overlap or fold, not cover it once; and where the
/// corners lie on one great circle, which has no such point. Inside means on the triangle's
/// side of each of its three great circles.
fn fan_centre(dirs: &[Vec3; 3]) -> Option<Vec3> {
    let [d0, d1, d2] = *dirs;
    let (_, mut mid) = (d1 - d0).cross(d2 - d0).length_and_unit()?;
    if mid.dot(d0) < 0.0 {
        mid = -mid;
    }
    let turn = d0.dot(d1.cross(d2));
    let inside = [(d0, d1), (d1, d2), (d2, d0)]
        .iter()
        .all(|&(a, b)| mid.dot(a.cross(b)) * turn > EFF_ZERO);
    inside.then_some(mid)
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
    let not_one_patch =
        || SolidError::blend(OP_PATCH_NET, BlendFailure::BallTriangleNotOnePatch { pole });
    let mut from = (pole + 1) % 3;
    let mut to = (pole + 2) % 3;
    // the arc from A to B through the middle's image, less than a half turn: its middle
    // control point where the tangents at A and B cross, its weight the cosine of half its
    // angle
    let Some(far) = far_side(dirs, pole)?.filter(FarSide::is_one_arc) else {
        return Err(not_one_patch());
    };
    let FarSide {
        e1,
        e2,
        e3,
        a,
        b,
        o,
        r,
        ua,
        ub,
        cos,
        ..
    } = far;
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
    for (i, ((&x, &y), &wt)) in xs.iter().zip(&ys).zip(&wts).enumerate() {
        pu.set(i, 1, x);
        pv.set(i, 1, y);
        pt.set(i, 0, wt);
        pt.set(i, 1, wt);
    }
    let ut = times(&pu, &pt);
    let vt = times(&pv, &pt);
    let sq = sum(1.0, &times(&pu, &pu), 1.0, &times(&pv, &pv));
    let tt = times(&pt, &pt);
    let h3 = sum(1.0, &sq, -1.0, &tt);
    let h4 = sum(1.0, &sq, 1.0, &tt);

    // control points, s (u) outer, t (v) inner
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for ii in 0..=4 {
        let i = if reverse_s { 4 - ii } else { ii };
        for j in 0..=2 {
            let wij = h4.at(i, j);
            if wij <= EFF_ZERO {
                return Err(not_one_patch());
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
    let Some(slot) = corner.ball.as_mut() else {
        return Err(bad_input(OP_BUILD, CornerInput::EventNotSet, None));
    };
    slot.delete_surface();
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
        .ok_or_else(|| bad_input(OP_BUILD, CornerInput::RingNotSet, None))?;
    let first_edge =
        eu1::edge(target, first)?.ok_or_else(|| edge_use_without_edge(OP_BUILD, first))?;
    let adds_material = is_concave_edge(k, target, first_edge)?;

    // each contact's direction from the centre and its corner vertex's point (ball_centre
    // checked one vertex per contact)
    let n = contacts.len();
    let mut dirs = Vec::with_capacity(n);
    for (&contact, &v) in contacts.iter().zip(corner.vertices.iter()) {
        dirs.push(((contact - centre).unitize()?, pseudo.vertex_point(v)?));
    }
    // the triangles, each with the pole first, and the corner's vertex's point at each of
    // its corners (none at a fan's centre)
    let mut triangles: Vec<([Vec3; 3], [Option<Point3>; 3])> = Vec::new();
    if let Some(&(d0, p0)) = dirs.first() {
        for pair in dirs.windows(2).skip(1) {
            // the triangle (0, k, k + 1): the pole at contact 0
            if let &[(dk, pk), (dk1, pk1)] = pair {
                triangles.push(([d0, dk, dk1], [Some(p0), Some(pk), Some(pk1)]));
            }
        }
    }
    // Not the fork's (SolidCraft #19583): three contacts whose triangle is not one patch (a
    // side too long, at a corner as sharp as a regular tetrahedron's or sharper) are fanned
    // about the point of the ball equally far from the three, which faces the vertex: the
    // three triangles (centre, i, i + 1), on the same sphere, each far side a shorter arc.
    // Only where that point lies inside the triangle (`fan_centre`): otherwise the triangle
    // stays one, and its patch refuses it as before
    if let ([first], &[(d0, p0), (d1, p1), (d2, p2)]) = (triangles.as_slice(), dirs.as_slice()) {
        if far_side(&first.0, 0)?.is_some_and(|far| !far.is_one_arc()) {
            if let Some(mid) = fan_centre(&[d0, d1, d2]) {
                let fan = [(d0, d1, p0, p1), (d1, d2, p1, p2), (d2, d0, p2, p0)];
                triangles = fan
                    .iter()
                    .map(|&(a, b, i, j)| ([mid, a, b], [None, Some(i), Some(j)]))
                    .collect();
            }
        }
    }
    let mut ball = BallCorner::default();
    for (tri, index) in triangles {
        // the pole first, its normal turned as the face's
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
        // the two turns of s give the patch opposite normals, so one of them faces the
        // material's way unless its normal is square to the radius
        let surface = chosen
            .ok_or_else(|| unbuildable_missing(OP_BUILD, CornerStep::BallPatchNormalUndecided))?;

        // its corners are the contacts: the pole at v = vmin, the others at v = vmax
        let nurb_domain = surface.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (nurb_domain.min(), nurb_domain.max());
        let corners_uv = [
            Point2::new(lo.x, hi.y),
            Point2::new(hi.x, hi.y),
            Point2::new(lo.x, lo.y),
        ];
        for (&uv, at) in corners_uv.iter().zip([from, to, pole]) {
            // a contact is the corner's vertex; a fan's centre is its point of the sphere
            let want = match of3(&index, at) {
                Some(vertex_point) => vertex_point,
                None => centre + of3(&tri, at) * radius,
            };
            if surface.evaluate_point(k, uv)?.distance(want) > tol {
                return Err(unbuildable(OP_BUILD, CornerStep::BallPatchOffVertex));
            }
        }

        // its loop: three sides, the side at the pole a point
        let trims =
            surface.create_natural_uv_trim_curves(k, &nurb_domain, SurfParamType::V, true)?;
        let count = trims.curves_3d.len();
        // each patch keeps its definition (#2357): the ball's sphere and the patch's own loop,
        // certified on it
        let mut surface = surface;
        let boundary = trims
            .curves_3d
            .iter()
            .map(|c| nurbs_curve(c).cloned())
            .collect::<Result<_>>()?;
        let definition = CornerDefinition::ball(centre, radius, boundary)?;
        definition.recertify(k, &surface)?;
        surface.set_corner_definition(definition);
        ball.curves_3d.extend(trims.curves_3d);
        ball.uv_curves
            .extend(trims.uv_curves.iter().cloned().map(Curve::from));
        ball.orients.extend(trims.orients.iter().copied());
        if count != 3 || trims.uv_curves.len() != 3 || trims.orients.len() != 3 {
            return Err(defect(
                OP_BUILD,
                "the ball's triangular patch's natural loop is not three curves, though the patch's side at the pole is a point, left out of its loop",
            ));
        }
        ball.surfaces.push(surface);
        ball.loops.push(3);
    }
    if let Some(slot) = corner.ball.as_mut() {
        *slot = ball; // there since the start
    }
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
    let ball = corner
        .ball
        .as_mut()
        .ok_or_else(|| bad_input(OP_PATCH, CornerInput::EventNotSet, None))?;
    if ball.surfaces.is_empty() || ball.loops.len() != ball.surfaces.len() {
        return Err(bad_input(OP_PATCH, CornerInput::EventMalformed, None));
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
                return Err(bad_input(OP_PATCH, CornerInput::EventMalformed, None));
            };
            c3.push(Some(c));
            uv.push(u);
            or.push(o);
        }
        let fillet = env
            .fillet
            .as_mut()
            .ok_or_else(|| bad_input(OP_PATCH, CornerInput::FilletBrepNotGiven, None))?;
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
        let face = made.face.ok_or_else(|| {
            missing_defect(
                OP_PATCH,
                "making a ball patch's face returned no face, though the face construction succeeded",
            )
        })?;
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

    /// Three planes through one line meet in no single point: the ball's offset planes are
    /// degenerate there (neither a defect nor a refusal), with the `SM_ERR` status the bare
    /// status had.
    #[test]
    fn planes_through_one_line_are_degenerate() {
        let on = [Point3::ZERO; 3];
        let normal = [Vec3::X, Vec3::Y, (Vec3::X + Vec3::Y).unitize().unwrap()];
        let err = planes_meet(&on, &normal).unwrap_err();
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::CornerUnbuildable(
                CornerStep::BallOffsetPlanesDegenerate
            ))
        );
        assert!(!err.is_defect() && !err.is_refusal(), "{err:?}");
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

    /// Three unit directions each `angle` from the other two, about +z.
    fn equal_sides(angle: f64) -> [Vec3; 3] {
        let z = ((1.0 + 2.0 * angle.cos()) / 3.0).sqrt();
        let r = (1.0 - z * z).sqrt();
        [0.0_f64, 120.0, 240.0].map(|a| {
            let a = a.to_radians();
            Vec3::new(r * a.cos(), r * a.sin(), z)
        })
    }

    /// One patch holds a triangle only while its far side's image is under a half turn. That
    /// image is a half turn where its ends and its circle's centre are in line: with the
    /// projection from `-p` and the far side from `f` to `t`,
    /// `((f + p) x (t + p)) . (f x t) = 0`. For three directions with one cosine `c` between
    /// them that is `(1 - c)(1 + 3 c) = 0`: `c = -1/3`, the corner of a regular tetrahedron
    /// (its faces' normals stand arccos(-1/3) = 109.47 degrees apart). Under that angle
    /// the triangle is one patch from any pole; over it, from none.
    #[test]
    fn a_triangle_is_one_patch_up_to_a_regular_tetrahedrons_corner() {
        let limit = (-1.0_f64 / 3.0).acos();
        for (angle, one) in [
            (90.0_f64.to_radians(), true),
            (limit - 1e-3, true),
            (limit + 1e-3, false),
            (116.0_f64.to_radians(), false),
        ] {
            let dirs = equal_sides(angle);
            for pole in 0..3 {
                assert_eq!(
                    far_side(&dirs, pole).unwrap().unwrap().is_one_arc(),
                    one,
                    "{angle} {pole}"
                );
                let patch = ball_triangle_patch(Point3::ZERO, 1.0, &dirs, pole, false);
                assert_eq!(patch.is_ok(), one, "{angle} {pole}");
            }
        }
    }

    /// A triangle too large for one patch, fanned about the point equally far from its three
    /// corners (+z for equal sides): each of the three triangles (centre, i, i + 1) is one
    /// patch, on the sphere, its corners the centre and the two contacts.
    #[test]
    fn a_large_triangle_fans_into_three_patches_on_the_sphere() {
        let k = Kernel::default();
        let dirs = equal_sides(116.0_f64.to_radians());
        let c = Point3::new(1.0, 2.0, 3.0);
        let mid = fan_centre(&dirs).unwrap();
        assert!((mid - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-15);
        for i in 0..3 {
            let tri = [mid, dirs[i], dirs[(i + 1) % 3]];
            assert!(far_side(&tri, 0).unwrap().unwrap().is_one_arc());
            let (net, from, to) = ball_triangle_patch(c, 2.0, &tri, 0, false).unwrap();
            assert_eq!((from, to), (1, 2));
            let s = Surface::from(net);
            let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
            for a in 0..=8 {
                for b in 0..=8 {
                    let p = s
                        .evaluate_point(&k, dom.evaluate(f64::from(a) / 8.0, f64::from(b) / 8.0))
                        .unwrap();
                    assert!(((p - c).length() - 2.0).abs() < 1e-13);
                }
            }
            let (lo, hi) = (dom.min(), dom.max());
            let at = |u: f64, v: f64| s.evaluate_point(&k, Point2::new(u, v)).unwrap();
            assert!((at(lo.x, lo.y) - (c + mid * 2.0)).length() < 1e-13);
            assert!((at(lo.x, hi.y) - (c + tri[1] * 2.0)).length() < 1e-13);
            assert!((at(hi.x, hi.y) - (c + tri[2] * 2.0)).length() < 1e-13);
        }
    }

    /// A fan's centre has to lie inside its triangle. Three corners nearly on one great circle,
    /// the middle one 0.05 off it (40 degrees between neighbours along the equator): the circle
    /// through them is nearly that great circle, and the point equally far from the three is
    /// near its pole, far outside the thin triangle. About it the three triangles would
    /// overlap, so no centre is given. Three corners on one great circle have none either.
    #[test]
    fn a_fan_centre_outside_its_triangle_is_not_taken() {
        let on_equator = |degrees: f64| {
            let a = degrees.to_radians();
            Vec3::new(a.cos(), a.sin(), 0.0)
        };
        let lifted = (on_equator(40.0) + Vec3::new(0.0, 0.0, 0.05))
            .unitize()
            .unwrap();
        let thin = [on_equator(0.0), lifted, on_equator(80.0)];
        // the point equally far from the three is within 26 degrees of the pole: not in the triangle
        let pole = (thin[1] - thin[0])
            .cross(thin[2] - thin[0])
            .unitize()
            .unwrap();
        assert!(pole.z.abs() > 0.9, "{pole:?}");
        assert_eq!(fan_centre(&thin), None);
        let flat = [on_equator(0.0), on_equator(120.0), on_equator(240.0)];
        assert_eq!(fan_centre(&flat), None);
        // and the same corners turned the other way round
        assert_eq!(fan_centre(&[thin[2], thin[1], thin[0]]), None);
        assert!(fan_centre(&equal_sides(116.0_f64.to_radians())).is_some());
        let turned = equal_sides(116.0_f64.to_radians());
        assert!(fan_centre(&[turned[2], turned[1], turned[0]]).is_some());
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
