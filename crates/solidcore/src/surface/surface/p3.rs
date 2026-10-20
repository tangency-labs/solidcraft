//! `SmSurface`, part 3: the geometric properties and normal section at a point, the
//! singular sides, the coverage of another coincident surface, the shape match of two
//! evaluations, and the local solves of the surface/surface/plane, surface/surface
//! (minimize, maximize, normalize, signed directed minimize) and three-surface
//! intersections with their Newton equation sets.
//!
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//! The file is split over several plan units (`surface.surface_p1`, `_p2`, ...); each lives
//! in its own submodule and adds methods to [`Surface`] (or free functions).
//!
//! | Rust                                              | Upstream                                         |
//! |---------------------------------------------------|--------------------------------------------------|
//! | [`Surface::evaluate_geometric`], [`evaluate_geometric_of`] | `SmSurface::EvaluateGeometric`          |
//! | [`Surface::evaluate_normal_section`], [`evaluate_normal_section_of`], [`SurfaceNormalSection`] | `SmSurface::EvaluateNormalSection` |
//! | [`Surface::get_singularities`], [`Surface::get_singularities_of_face`], [`Singularities`] | `SmSurface::GetSingularities`                    |
//! | [`Surface::cover_coincident_surface`], [`CoverCoincident`] | `SmSurface::CoverCoincidentSurface`     |
//! | [`check_shape_match`]                             | `smsurf_CheckShapeMatch`                         |
//! | [`FindPsNormalEnfo`] (`EvalNFunctions`)           | `SmFindPSNormalENFO::Evaluate` (struct: `surface.surface_p1`) |
//! | [`SssIntersectEnfo`] (`EvalNFunctions`)           | `SmSSSIntersectENFO::Evaluate` (struct: `surface.surface_p1`) |
//! | [`Surface::local_plane_surface_intersect`]        | `SmSurface::LocalPlaneSurfaceIntersect`          |
//! | [`Surface::local_surface_solve`], [`LocalSurfaceSolution`] | `SmSurface::LocalSurfaceSolve`          |
//! | [`Surface::local_surface_surface_intersect`], [`LocalSssSolution`] | `SmSurface::LocalSurfaceSurfaceIntersect` |
//! | `DropCurveFail::copy` ([`Clone`])                 | `SmDropCurveFail::Copy`                          |
//! | [`is_point_on_normal`](super::p4::is_point_on_normal), [`SurfaceEval::eval_normal`](super::p2::SurfaceEval) (co-ported in `p4`) | `smsurf_IsPointOnNormal`, `SmSurfaceEval::EvalNormal` |
//!
//! Deviations:
//! - **Out-params** become return values; a solve that finds nothing returns `None`.
//! - **Temporary out-of-bounds evaluation.** `SmTemporaryChangeValue` on the
//!   `OutOfBoundsEnabled` flag of the (B-spline based) surfaces is done on the surface
//!   itself where it is `&mut` ([`Surface::cover_coincident_surface`]) and on an
//!   out-of-bounds copy where upstream changes a `const` surface
//!   ([`Surface::local_surface_surface_intersect`]).
//! - **Ignored statuses.** Where upstream ignores the status of an evaluation its
//!   outputs keep their previous values (initially undefined); the port does the same
//!   and still propagates [`SolidError::Unported`].
//! - **Tolerance of a surface.** `SmTol::GetApproxTol3d(this)` is the system value for a
//!   surface without an owner; for an owned surface it needs the face tolerance
//!   (`topology.face_p2`) and returns [`SolidError::Unported`], unless the caller that
//!   has the face passes it ([`Surface::get_singularities_of_face`], used by the face
//!   props).
//! - **Unported callees** (return [`SolidError::Unported`]): `SmFace::RemoveUVTrimCurves`
//!   (`topology.face_p1`) when the surface is owned by a face.

use crate::error::{ignored, Result, SolidError};
use crate::geom::assert_array::labels;
use crate::geom::extent1d::{Extent1d, ExtentNd};
use crate::geom::extent2d::{Extent2d, SS_NONE, SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::local_solve_nd::{
    BoundaryHandler, EvalNFunctions, LocalSolveNd, TerminationReason,
};
use crate::geom::matrix::Matrix;
use crate::geom::tol::{approx_tol3d, approx_tol_3d, SmallTopoType};
use crate::geom::vector3d::sqrt as sm_sqrt;
use crate::kernel::Kernel;
use crate::math::{
    max2, max3, rad_to_deg, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT,
};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::plane::Plane;

use super::p1::{
    crossed_seam_or_pole, evaluate_1st_derivatives, evaluate_2nd_derivatives,
    evaluate_normal_section as smsurf_evaluate_normal_section, FindPsNormalEnfo, SsNormalizeEnfo,
    SspIntersectEnfo, SssIntersectEnfo,
};
use super::p2::evaluate_geometric as smsurf_evaluate_geometric;
use super::p2::{GeometricProperties, SurfaceEvaluate};
use super::p7::{approx_derivative_lengths, check_seam_drop};
use super::{Surface, SurfaceKind};

const OP_FIND_PS_NORMAL: &str = "surface::surface::p3::FindPsNormalEnfo::evaluate";
const OP_SSS_INTERSECT: &str = "surface::surface::p3::SssIntersectEnfo::evaluate";
const OP_LOCAL_SOLVE: &str = "surface::surface::p3::Surface::local_surface_solve";
const OP_PVV: &str = "surface::surface::p3::evaluate_pvv";

/// `lSmpCnt` of `GetSingularities`: the samples along a natural boundary.
const SMP_CNT: usize = 5;
/// `lNumSamples` of `CoverCoincidentSurface`: the grid is `6 x 6` points.
const NUM_SAMPLES: usize = 6;
/// `lNumBdrySamples` of `CoverCoincidentSurface`: the samples along each boundary.
const NUM_BDRY_SAMPLES: usize = 15;

/// `Side` of an upstream `bFromLeft` flag.
const fn side(from_left: bool) -> Side {
    if from_left {
        Side::Left
    } else {
        Side::Right
    }
}

/// The tolerance a surface is solved at, `SmTol::GetApproxTol3d(const SmObject*)` of a
/// surface: half the zone tolerance of its face (`SmSurface::GetFace()->GetTolerance()`),
/// the system one for a surface without face.
///
/// A [`Surface`] holds only an owner handle, not the B-rep, so the face's tolerance
/// (`SmFace::GetTolerance`, `topology.face_p2`) is passed by a caller that has the face:
/// `face_tolerance` is `Some` for a surface known to be that face's own surface, and the
/// zone tolerance of the face is then used whether or not the handle is set.
fn surface_approx_tol_3d(
    k: &Kernel,
    surface: &Surface,
    face_tolerance: Option<f64>,
) -> Result<f64> {
    if let Some(zone) = face_tolerance {
        // SmApproxTol3d(.5 * GetZoneTol3d(pSurf)), the face's stored zone tolerance
        return Ok(approx_tol3d(k, 0.5 * zone));
    }
    // SmTol::GetApproxTol3d(this) = .5 * GetZoneTol3d(this): GetFace() ? its tolerance
    // : the system's (a surface owned by an offset with no face uses the latter)
    if surface.face()?.is_some() {
        // UNPORTED(topology.face_p2): SmFace::GetTolerance of the owner, for the callers
        // that cannot pass it (a surface does not reach its B-rep; justified: plan/findings.md,
        // heal.face_props, "Why UNPORTED(topology.face_p2) stays"); the face props pass it through `get_singularities_of_face`
        return Err(SolidError::unported(
            "SmFace::GetTolerance",
            "topology.face_p2",
        ));
    }
    Ok(approx_tol_3d(k, SmallTopoType::NormalSize))
}

/// `GetOutOfBoundsEnabled` of a surface that is an `SmBSplineSurface` (a B-spline
/// surface, a plane or a surface of revolution); `None` for the other kinds
/// (`SM_CAST_NONNULL_PTR(SmBSplineSurface, ..)` is null, so `SmTemporaryChangeValue`
/// changes a local instead). An `SmSTEPSurface` is an `SmSurface`, not a B-spline surface.
pub(crate) fn out_of_bounds_enabled(surface: &Surface) -> Option<bool> {
    match surface.kind() {
        SurfaceKind::BSpline(s) => Some(s.is_out_of_bounds_enabled()),
        SurfaceKind::Plane(s) => Some(s.is_out_of_bounds_enabled()),
        SurfaceKind::SurfOfRevolution(s) => Some(s.is_out_of_bounds_enabled()),
        SurfaceKind::OffsetSurface(_)
        | SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::SrfInVolume(_) => None,
    }
}

/// Sets the `m_bOutOfBoundsEnabled` member of a B-spline based surface (no-op for other
/// kinds). Only the `SmBSplineSurface` member changes, as `SmTemporaryChangeValue`
/// through `GetOutOfBoundsEnabled()` does: a surface of revolution's generator keeps its
/// own flag (the virtual `SmSurfOfRevolution::SetOutOfBoundsEnabled` is not called).
pub(crate) fn set_out_of_bounds_enabled(surface: &mut Surface, enabled: bool) {
    match surface.kind_mut() {
        SurfaceKind::BSpline(s) => *s.out_of_bounds_enabled_mut() = enabled,
        SurfaceKind::Plane(s) => s.set_out_of_bounds_enabled(enabled),
        SurfaceKind::SurfOfRevolution(s) => *s.base_mut().out_of_bounds_enabled_mut() = enabled,
        SurfaceKind::OffsetSurface(_)
        | SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::SrfInVolume(_) => {}
    }
}

/// A copy of `surface` that evaluates out of bounds, when it is a B-spline based
/// surface that does not already (the `SmTemporaryChangeValue` of a `const` surface).
fn out_of_bounds_copy(surface: &Surface) -> Option<Surface> {
    if out_of_bounds_enabled(surface) == Some(false) {
        let mut copy = surface.clone();
        set_out_of_bounds_enabled(&mut copy, true);
        Some(copy)
    } else {
        None
    }
}

/// The 3x3 evaluation matrix of `Evaluate(uv, 2, 2, .., TRUE, ..)`: entry `[i][j]` is the
/// derivative `i` times in `u` and `j` times in `v`; the entries of the lower half (not
/// computed) are undefined.
fn evaluate_pvv<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<[[Vec3; 3]; 3]> {
    let m = surface.evaluate(
        uv,
        2,
        2,
        EvaluateFlags {
            u_side,
            v_side,
            only_upper_half: true,
            non_zero_tangents: false, // exact Tangent values
        },
    )?;
    let mut pvv = [[Vec3::UNDEF; 3]; 3];
    for (i, row) in pvv.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            if let Some(v) = m.get(i).and_then(|r| r.get(j)) {
                *slot = *v;
            }
        }
    }
    if m.len() < 3 {
        return Err(SolidError::invalid_input(
            OP_PVV,
            "evaluation result too short",
        ));
    }
    Ok(pvv)
}

/// The unit normal, the tangent-plane direction and the curvature of a normal section
/// ([`Surface::evaluate_normal_section`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceNormalSection {
    /// `rTangentPlaneDirection`: the unit vector in the direction projected into the
    /// tangent plane.
    pub tangent: Vec3,
    /// `rdCurvatureValue`: the curvature of the theoretical surface curve with that
    /// tangent.
    pub curvature: f64,
    /// `rSurfaceNormalVector`: the unit surface normal.
    pub normal: Vec3,
}

/// The singular sides of a surface ([`Surface::get_singularities`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Singularities {
    /// The return value: an or of `SS_UMIN`, `SS_VMIN`, `SS_UMAX`, `SS_VMAX` for the
    /// sides within `SM_EFF_ZERO` of being a pole.
    pub singular: u32,
    /// `plApproxSingularities`: the sides whose sample points are within the
    /// approximation tolerance of one another.
    pub approx_singular: u32,
    /// `pPolePoints`: `[UMin, VMin, UMax, VMax]`, undefined for a side that is not
    /// (approximately) singular; `None` when not asked for.
    pub pole_points: Option<[Point3; 4]>,
    /// `pPoleNormals`: as `pole_points`, the normals.
    pub pole_normals: Option<[Vec3; 4]>,
}

/// The result of [`Surface::cover_coincident_surface`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverCoincident {
    /// `rbSuccess`: the other surface fits inside this one.
    pub success: bool,
    /// `rdMaxDist`: the maximum distance found (`SM_BIG_DOUBLE` when not successful).
    pub max_dist: f64,
    /// `pOptCoveredSTEPDomain`: the covered subset in this surface's STEP parameters,
    /// valid when `success`.
    pub covered_step_domain: Option<Extent2d>,
}

/// A two-surface local solution ([`Surface::local_surface_solve`] and
/// [`Surface::local_plane_surface_intersect`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalSurfaceSolution {
    /// `m_dSolutionValue`: the distance between the surface points (signed for a signed
    /// directed minimize).
    pub distance: f64,
    /// The parameter on this surface.
    pub uv: Point2,
    /// The parameter on the other surface.
    pub other_uv: Point2,
}

/// The solution of [`Surface::local_surface_surface_intersect`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalSssSolution {
    /// `m_dSolutionValue`: the maximum distance between the surface points.
    pub distance: f64,
    /// The parameter on the first surface.
    pub uv1: Point2,
    /// The parameter on the second surface.
    pub uv2: Point2,
    /// The parameter on the third surface.
    pub uv3: Point2,
}

/// Bounds of 2-D parameter domains as the intervals of a Newton problem.
fn domain_intervals(domains: &[&Extent2d], extra: Option<Extent1d>) -> ExtentNd {
    let mut intervals = ExtentNd::new(domains.len() * 2 + usize::from(extra.is_some()));
    let mut at = 0;
    for domain in domains {
        for ivl in [
            Extent1d::from_min_max(domain.min().x, domain.max().x),
            Extent1d::from_min_max(domain.min().y, domain.max().y),
        ] {
            if let Some(slot) = intervals.get_mut(at) {
                *slot = ivl;
            }
            at += 1;
        }
    }
    if let Some(ivl) = extra {
        if let Some(slot) = intervals.get_mut(at) {
            *slot = ivl;
        }
    }
    intervals
}

/// Entry `i` of a solution vector.
fn nth(v: &[f64], i: usize, op: &'static str) -> Result<f64> {
    v.get(i)
        .copied()
        .ok_or_else(|| SolidError::invalid_input(op, "solution vector too short"))
}

/// The parameter pair at entries `i` and `i + 1` of a solution vector.
fn uv_at(v: &[f64], i: usize, op: &'static str) -> Result<Point2> {
    Ok(Point2::new(nth(v, i, op)?, nth(v, i + 1, op)?))
}

/// Whether two surface evaluations (matrices with two derivatives) match in position,
/// normal and the normal curvatures in the directions of the first surface's first
/// derivatives (`smsurf_CheckShapeMatch`).
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` (zero-length vector) for a vanishing normal; [`SolidError::Unported`] from the evaluations.
pub fn check_shape_match(
    pvv0: &mut [[Vec3; 3]; 3],
    pvv1: &mut [[Vec3; 3]; 3],
    this_xsect_tol_3d: f64,
    ang_tol_rad: f64,
) -> Result<bool> {
    let p00 = pvv0[0][0];
    let p10 = pvv0[1][0];
    let p01 = pvv0[0][1];
    let q00 = pvv1[0][0];
    let q10 = pvv1[1][0];
    let q01 = pvv1[0][1];

    // Check positions first (order 0).
    if !p00.closer_than(this_xsect_tol_3d, q00) {
        return Ok(false);
    }

    // Next check normals (order 1).
    let norm0 = p10.cross(p01).unitize()?;
    let norm1 = q10.cross(q01).unitize()?;

    if !norm1.is_parallel_to(norm0, rad_to_deg(ang_tol_rad)) {
        return Ok(false);
    }

    // Now check 2nd-order shape: use normal curvatures in two directions.  Normal
    // curvature is signed, postitive in the direction of the surface parametric normal.
    // Those could be opposite, so:
    let flip = if norm0.dot(norm1) > 0.0 { 1.0 } else { -1.0 };

    // Use 1st derivs of first srf as directions.
    let dir1 = p01;
    let dir2 = p10;

    // upstream ignores the statuses of the section evaluations
    let mut kappa0 = 0.0;
    let mut kappa1 = 0.0;
    if let Some(s) = ignored(smsurf_evaluate_normal_section(pvv0, dir1))? {
        kappa0 = s.1;
    }
    if let Some(s) = ignored(smsurf_evaluate_normal_section(pvv1, dir1))? {
        kappa1 = s.1;
    }
    if (flip * kappa0 - kappa1).abs() > ang_tol_rad {
        return Ok(false);
    }

    if let Some(s) = ignored(smsurf_evaluate_normal_section(pvv0, dir2))? {
        kappa0 = s.1;
    }
    if let Some(s) = ignored(smsurf_evaluate_normal_section(pvv1, dir2))? {
        kappa1 = s.1;
    }
    if (flip * kappa0 - kappa1).abs() > ang_tol_rad {
        return Ok(false);
    }

    // If we arrive here then everything matches.
    Ok(true)
}

/// `SmSurface::EvaluateGeometric` for code generic over the surface type (the
/// `SmSurface` body, which no ported surface kind overrides).
///
/// # Errors
///
/// Those of the evaluations and of `smsurf_EvaluateGeometric`.
pub fn evaluate_geometric_of<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
) -> Result<GeometricProperties> {
    // Evaluate Target parameter for position and derivatives
    let mut pvv = evaluate_pvv(surface, uv, u_side, v_side)?; // exact Tangent values

    // make sure there is a normal vector - even at singular points
    let norm = pvv[0][1].cross(pvv[1][0]);
    if norm.length_squared() < EFF_ZERO {
        pvv[2][2] = surface.evaluate_normal(uv, u_side, v_side)?;
    }

    // pass the call along
    smsurf_evaluate_geometric(&mut pvv)
}

/// `SmSurface::EvaluateNormalSection` for code generic over the surface type (the
/// `SmSurface` body, which no ported surface kind overrides).
///
/// # Errors
///
/// Those of the evaluations, the zero-length-vector status for a vanishing normal and those of
/// `smsurf_EvaluateNormalSection`.
pub fn evaluate_normal_section_of<S: SurfaceEvaluate + ?Sized>(
    surface: &S,
    uv: Point2,
    u_side: Side,
    v_side: Side,
    direction: Vec3,
) -> Result<SurfaceNormalSection> {
    // Get SurfacePoint Evaluation
    let mut pvv = evaluate_pvv(surface, uv, u_side, v_side)?; // exact Tangent values

    // get surface Normal
    let mut surface_normal = pvv[1][0].cross(pvv[0][1]);
    if surface_normal.length_squared() < EFF_ZERO_SQ {
        surface_normal = surface.evaluate_normal(uv, u_side, v_side)?;
    } else {
        surface_normal = surface_normal.unitize()?;
    }

    // get output TangentPlaneDirection and Curvature
    pvv[2][2] = surface_normal;
    let section = smsurf_evaluate_normal_section(&mut pvv, direction)?;
    Ok(SurfaceNormalSection {
        tangent: section.0,
        curvature: section.1,
        normal: surface_normal,
    })
}

impl Surface {
    /// The curvatures, fundamental forms and principal directions at `uv`
    /// (`EvaluateGeometric`); `u_side` and `v_side` pick the interval at a boundary.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and of `smsurf_EvaluateGeometric`.
    pub fn evaluate_geometric(
        &self,
        uv: Point2,
        u_side: Side,
        v_side: Side,
    ) -> Result<GeometricProperties> {
        evaluate_geometric_of(self, uv, u_side, v_side)
    }

    /// The tangent-plane direction, curvature and unit normal of the normal section
    /// through `direction` (non-unit is fine) at `uv` (`EvaluateNormalSection`).
    ///
    /// # Errors
    ///
    /// Those of the evaluations, the zero-length-vector status for a vanishing normal and those of
    /// `smsurf_EvaluateNormalSection`.
    pub fn evaluate_normal_section(
        &self,
        uv: Point2,
        u_side: Side,
        v_side: Side,
        direction: Vec3,
    ) -> Result<SurfaceNormalSection> {
        evaluate_normal_section_of(self, uv, u_side, v_side, direction)
    }

    /// The singular sides: a natural boundary is singular (or approximately singular)
    /// when a row of natural boundary samples lies within `SM_EFF_ZERO` (the
    /// approximation tolerance) of one another (`GetSingularities`). The pole points and
    /// normals are computed when asked for.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: the tolerance of an owned surface (`topology.face_p2`);
    /// those of the evaluations.
    pub fn get_singularities(
        &self,
        k: &Kernel,
        want_pole_points: bool,
        want_pole_normals: bool,
    ) -> Result<Singularities> {
        self.get_singularities_at(k, None, want_pole_points, want_pole_normals)
    }

    /// [`Surface::get_singularities`] of the face's own surface: the approximation
    /// tolerance is half `face_tolerance`, the zone tolerance of the face that owns the
    /// surface (`SmFace::GetTolerance`; the caller has the B-rep, the surface only an
    /// owner handle), so an owned surface does not stop at `SmFace::GetTolerance`.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    pub fn get_singularities_of_face(
        &self,
        k: &Kernel,
        face_tolerance: f64,
        want_pole_points: bool,
        want_pole_normals: bool,
    ) -> Result<Singularities> {
        self.get_singularities_at(k, Some(face_tolerance), want_pole_points, want_pole_normals)
    }

    fn get_singularities_at(
        &self,
        k: &Kernel,
        face_tolerance: Option<f64>,
        want_pole_points: bool,
        want_pole_normals: bool,
    ) -> Result<Singularities> {
        // init return and output values
        let mut singularities: u32 = SS_NONE;
        let mut approx_singularities: u32 = SS_NONE;

        // locals
        let uv_domain = self.natural_uv_domain();
        let u_ivl = uv_domain.u_interval();
        let v_ivl = uv_domain.v_interval();
        let approx_tol = surface_approx_tol_3d(k, self, face_tolerance)?;

        // Get a tolerance for zero deriv length.  At a singularity, even if all
        // involved control points are exactly identical, for the derivatives, they get
        // multiplied by the various basis derivatives and added together.  Any
        // numerical noise in the basis derivs is multiplied by the size of the control
        // point numbers.  On very large parts, this can cause problems.  [B253]
        let mut size = approx_derivative_lengths(self, &uv_domain)?;
        // Multiply by domain size.
        size.x *= uv_domain.x_length();
        size.y *= uv_domain.y_length();
        let mut srf_size = size.length();
        if srf_size < 1.0 {
            srf_size = 1.0; // Don't shrink it below this tolerance.
        }
        let deriv_tol = srf_size * EFF_ZERO_SQRT * 10.0;

        // the evaluation of the last sample point; upstream ignores its status
        let mut pv: [[Vec3; 2]; 2] = [[Vec3::UNDEF; 2]; 2];

        // for every natural boundary
        for ii in 0..4_usize {
            // set iso parameter and pointers into variable memory
            //   cases 0 & 2 = Const U, Varying V
            //   cases 1 & 3 = Const V, Varying U
            let (const_uv, ivl, deriv_is_dv, size_along, test, vary_is_v) = match ii {
                0 => (uv_domain.u_min(), v_ivl, true, size.y, SS_UMIN, true),
                1 => (uv_domain.v_min(), u_ivl, false, size.x, SS_VMIN, false),
                2 => (uv_domain.u_max(), v_ivl, true, size.y, SS_UMAX, true),
                _ => (uv_domain.v_max(), u_ivl, false, size.x, SS_VMAX, false),
            };

            // interval length
            let domain_len = ivl.length();

            // init accumulation
            let mut edge_box = Extent3d::new();

            // init test result
            let mut singular = true;
            let mut approx_singular = true;

            // for every sample point
            for jj in 0..SMP_CNT {
                // set this param
                let param = if jj == 0 {
                    ivl.min()
                } else if jj == SMP_CNT - 1 {
                    ivl.max()
                } else {
                    ivl.evaluate(jj as f64 / (SMP_CNT - 1) as f64)
                };
                let uv = if vary_is_v {
                    Point2::new(const_uv, param)
                } else {
                    Point2::new(param, const_uv)
                };

                // evaluate this surface point - the 1st derivative needing checking is
                // in p1stDir
                if let Some(m) = ignored(self.evaluate(
                    uv,
                    1,
                    1,
                    EvaluateFlags {
                        u_side: Side::Left,
                        v_side: Side::Left,
                        only_upper_half: true,
                        non_zero_tangents: false,
                    },
                ))? {
                    for (i, row) in pv.iter_mut().enumerate() {
                        for (j, slot) in row.iter_mut().enumerate() {
                            if let Some(v) = m.get(i).and_then(|r| r.get(j)) {
                                *slot = *v;
                            }
                        }
                    }
                }

                // get 1st derivative speed
                let first_dir = if deriv_is_dv { pv[0][1] } else { pv[1][0] };
                let speed = first_dir.length();

                // large 1st derivatives are not singular
                if domain_len * speed > approx_tol {
                    approx_singular = false;
                    singular = false;
                } else if domain_len * speed > deriv_tol {
                    singular = false;
                }

                // quit as soon as the edge is found NotSingular
                if !singular && !approx_singular {
                    break;
                }

                // accumulate test points
                edge_box.add_point(pv[0][0]);
            } // end iter every sample point

            // arrive here when singular is set for the sample tests on this boundary
            if singular || approx_singular {
                // make an extra 3d check on the size of the boundary curve
                let edge_max_length = edge_box.max_length();

                approx_singular &= edge_max_length <= approx_tol;
                if approx_singular {
                    singular &= edge_max_length <= EFF_ZERO * (1.0 + size_along);
                } else {
                    singular = false;
                }

                // accumulate test results
                if singular {
                    singularities |= test;
                }
                if approx_singular {
                    approx_singularities |= test;
                }
            }
        } // end iter every natural domain boundary

        let is_pole = |side: u32| (singularities & side) != 0 || (approx_singularities & side) != 0;

        // when asked - compute pole points
        let pole_points = if want_pole_points {
            let mut points = [Vec3::UNDEF; 4];
            let sides = [
                (SS_UMIN, uv_domain.evaluate(0.0, 0.5)),
                (SS_VMIN, uv_domain.evaluate(0.5, 0.0)),
                (SS_UMAX, uv_domain.evaluate(1.0, 0.5)),
                (SS_VMAX, uv_domain.evaluate(0.5, 1.0)),
            ];
            for (slot, (flag, uv)) in points.iter_mut().zip(sides) {
                if is_pole(flag) {
                    if let Some(p) = ignored(self.evaluate_point(uv))? {
                        *slot = p;
                    }
                }
            }
            Some(points)
        } else {
            None
        };

        // when asked - compute pole normals
        let pole_normals = if want_pole_normals {
            let mut normals = [Vec3::UNDEF; 4];
            let sides = [
                (SS_UMIN, uv_domain.evaluate(0.0, 0.5), true, true),
                (SS_VMIN, uv_domain.evaluate(0.5, 0.0), true, true),
                (SS_UMAX, uv_domain.evaluate(1.0, 0.5), false, true),
                (SS_VMAX, uv_domain.evaluate(0.5, 1.0), true, false),
            ];
            for (slot, (flag, uv, u_left, v_left)) in normals.iter_mut().zip(sides) {
                if is_pole(flag) {
                    if let Some(n) = ignored(self.evaluate_normal(uv, side(u_left), side(v_left)))?
                    {
                        *slot = n;
                    }
                }
            }
            Some(normals)
        } else {
            None
        };

        Ok(Singularities {
            singular: singularities,
            approx_singular: approx_singularities,
            pole_points,
            pole_normals,
        })
    }

    /// Whether `other` (or the subset `other_domain` of it) fits inside this surface: a
    /// grid of points of `other` is dropped to this surface and, when all coincide within
    /// `tol`, this surface's STEP domain is expanded to cover them
    /// (`CoverCoincidentSurface`). The surface is changed only on success.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`]: `SmFace::RemoveUVTrimCurves` for a surface owned by a face
    /// (`topology.face_p1`); those of the evaluations and of `STEPInversion`.
    pub fn cover_coincident_surface(
        &mut self,
        k: &Kernel,
        other: &Surface,
        tol: f64,
        other_domain: Option<&Extent2d>,
    ) -> Result<CoverCoincident> {
        // Expand our domain, for the scope of this routine.
        let saved = out_of_bounds_enabled(self);
        if saved.is_some() {
            set_out_of_bounds_enabled(self, true);
        }
        let result = self.cover_coincident_surface_inner(k, other, tol, other_domain);
        if let Some(saved) = saved {
            set_out_of_bounds_enabled(self, saved);
        }
        result
    }

    /// The body of [`Surface::cover_coincident_surface`] with out-of-bounds evaluation on.
    fn cover_coincident_surface_inner(
        &mut self,
        k: &Kernel,
        other: &Surface,
        tol: f64,
        other_domain: Option<&Extent2d>,
    ) -> Result<CoverCoincident> {
        // init outputs
        let failure = CoverCoincident {
            success: false,
            max_dist: BIG_DOUBLE,
            covered_step_domain: None,
        };

        // We're going to have to expand ourselves right away.
        let big_domain = self.max_analytic_domain()?;
        if big_domain.is_degenerate(0.0) {
            return Ok(failure);
        }

        // Start with a rudimentary coincidence check.  Just drop some points.  Start
        // with the corner points.  We can save their uv values for dropping the boundary
        // curves.
        let mut corner_uvs = [[Point2::new(0.0, 0.0); 2]; 2];
        // Also set up what will be the expanded domain.
        let mut new_domain = Extent2d::new();

        let other_dom = other_domain
            .copied()
            .unwrap_or_else(|| other.natural_uv_domain());

        // upstream ignores the status of the point evaluations
        let mut pt = Vec3::UNDEF;
        let mut pt2 = Vec3::UNDEF; // keeps its last value when an evaluation fails
        let mut max_dist = -1.0;

        // Before even doing corners, just check one point in the middle.  This can rule
        // out most, and save lots of drops on corners and edges.  (Picture two fillet
        // surfaces end to end that form a closed loop.)
        let uv_other = other_dom.evaluate(0.5, 0.5);
        if let Some(p) = ignored(other.evaluate_point(uv_other))? {
            pt = p;
        }
        if ignored(self.step_inversion(k, &big_domain, pt, tol, None))?.is_none() {
            return Ok(failure);
        }

        // First drop corner points.
        for ii in 0..2_usize {
            for jj in 0..2_usize {
                let uv_other = other_dom.evaluate(ii as f64, jj as f64);
                if let Some(p) = ignored(other.evaluate_point(uv_other))? {
                    pt = p;
                }

                let Some((mut uv_this, location)) =
                    ignored(self.step_inversion(k, &big_domain, pt, tol, None))?
                else {
                    return Ok(failure);
                };

                match ignored(check_seam_drop(
                    self,
                    uv_this,
                    &big_domain,
                    other,
                    uv_other,
                    &other_dom,
                    location,
                ))? {
                    Some(moved) => uv_this = moved,
                    None => return Ok(failure),
                }

                if let Some(row) = corner_uvs.get_mut(ii) {
                    if let Some(slot) = row.get_mut(jj) {
                        *slot = uv_this;
                    }
                }

                // Add to resulting domain.
                new_domain.add_point(uv_this);

                // Check max distance.
                if let Some(p) = ignored(self.evaluate_step_point(uv_this))? {
                    pt2 = p;
                }
                let dist = pt.distance(pt2);
                max_dist = max2(dist, max_dist);

                // Shouldn't happen because STEPInversion shouldn't allow it, but:
                if dist > tol {
                    return Ok(failure);
                }
            }
        }

        // If both surfaces are bilinear, we're done.  (Four positions uniquely determine
        // a bilinear surface.)
        if self.is_bilinear() && other.is_bilinear() {
            // We're done: finish up and return.  Union the projection of Other surface on
            // our domain with our original domain, and update our domain to it.
            let orig_domain = self.step_uv_domain();
            let update_domain = orig_domain.union(&new_domain);
            // (status ignored upstream)
            ignored(self.adjust_step_uv_domain(k, &update_domain))?;

            // Have to do this after AdjustSTEPUVDomain call:
            self.remove_uv_trim_curves_of_owner()?;

            return Ok(CoverCoincident {
                success: true,
                max_dist,
                covered_step_domain: Some(new_domain),
            });
        }

        // Now just check a grid of sample points.  No reason to do boundaries specially.
        // We'll evaluate the other surface, and STEPInversion to this, using the previous
        // uv result as a guess points.  (Taken from SmSurface::CoincidenceCheck().)

        // Sample a grid of 6x6 points.  In order to use the previous drop result as a
        // guess for this, we'll go in a 'serpentine' fashion: go back and forth on the
        // inner loop.
        let mut backwards = false; // we'll flip this each time.

        // Also, we have a value for the first drop-point guess.
        let mut guess_uv = corner_uvs[0][0];

        let mut denom = (NUM_SAMPLES - 1) as f64;
        for ii in 0..NUM_SAMPLES {
            let du = ii as f64 / denom;
            for jj in 0..NUM_SAMPLES {
                let kk = if backwards { NUM_SAMPLES - 1 - jj } else { jj };

                // Skip corners - already did them.
                if (ii == 0 || ii == NUM_SAMPLES - 1) && (kk == 0 || kk == NUM_SAMPLES - 1) {
                    continue;
                }

                let dv = kk as f64 / denom;

                let uv_other = other_dom.evaluate(du, dv);
                if let Some(p) = ignored(other.evaluate_point(uv_other))? {
                    pt = p;
                }

                let Some((mut uv_this, location)) =
                    ignored(self.step_inversion(k, &big_domain, pt, tol, Some(guess_uv)))?
                else {
                    // If any DropPoint fails, they're not coincident.
                    return Ok(failure);
                };

                match ignored(check_seam_drop(
                    self,
                    uv_this,
                    &big_domain,
                    other,
                    uv_other,
                    &other_dom,
                    location,
                ))? {
                    Some(moved) => uv_this = moved,
                    None => return Ok(failure),
                }

                // Check max distance.
                if let Some(p) = ignored(self.evaluate_step_point(uv_this))? {
                    pt2 = p;
                }
                let dist = pt.distance(pt2);
                max_dist = max2(dist, max_dist);

                // Shouldn't happen because STEPInversion shouldn't allow it, but:
                if dist > tol {
                    return Ok(failure);
                }

                // Add to resulting domain.
                new_domain.add_point(uv_this);

                // Set next guess point.
                guess_uv = uv_this;
            } // end iter jj sample points

            // Switch direction of the inner loop.
            backwards = !backwards;
        } // end iter ii sample points

        // If we made it here, then every grid point dropped well, and we are pretty
        // confident of coincidence.  Now we check the boundaries to get a better result
        // for the required domain on this surface.
        //
        // Sampling: we already sampled the edges with NUM_SAMPLES (including the ends).
        // Use an odd number here, to make sure we get the midpoints, which will often be
        // the extrema.
        denom = (NUM_BDRY_SAMPLES - 1) as f64;

        let dom_u = other_dom.u_interval();
        let dom_v = other_dom.v_interval();

        for ii in 0..4_usize {
            // Get the corner guess.  (Sorry, a bit slick:)
            let (row, col) = (if ii < 2 { ii } else { 3 - ii }, ii / 2);
            guess_uv = corner_uvs
                .get(row)
                .and_then(|r| r.get(col))
                .copied()
                .unwrap_or(guess_uv);

            // Can leave off the corners:
            for jj in 1..NUM_BDRY_SAMPLES - 1 {
                let frac = jj as f64 / denom;

                let uv_other = match ii {
                    // Bottom, left to right: step along V Min, increasing U
                    0 => Point2::new(dom_u.evaluate(frac), dom_v.min()),
                    // Right, bottom to top: step along U Max, increasing V
                    1 => Point2::new(dom_u.max(), dom_v.evaluate(frac)),
                    // Top, right to left: step along V Max, decreasing U
                    2 => Point2::new(dom_u.evaluate(1.0 - frac), dom_v.max()),
                    // Left, top to bottom: step along U Min, decreasing V
                    _ => Point2::new(dom_u.min(), dom_v.evaluate(1.0 - frac)),
                };

                // Do the drop.
                if let Some(p) = ignored(other.evaluate_point(uv_other))? {
                    pt = p;
                }

                let Some((mut drop_uv, location)) =
                    ignored(self.step_inversion(k, &big_domain, pt, tol, Some(guess_uv)))?
                else {
                    // If any DropPoint fails, they're not coincident.
                    return Ok(failure);
                };

                match ignored(check_seam_drop(
                    self,
                    drop_uv,
                    &big_domain,
                    other,
                    uv_other,
                    &other_dom,
                    location,
                ))? {
                    Some(moved) => drop_uv = moved,
                    None => return Ok(failure),
                }

                // Here we check seams/poles.
                if crossed_seam_or_pole(self, guess_uv, drop_uv, &big_domain) {
                    return Ok(failure);
                }

                // Check max distance.
                if let Some(p) = ignored(self.evaluate_step_point(drop_uv))? {
                    pt2 = p;
                }
                let dist = pt.distance(pt2);
                max_dist = max2(dist, max_dist);

                // Shouldn't happen because STEPInversion shouldn't allow it, but:
                if dist > tol {
                    return Ok(failure);
                }

                // Add to resulting domain.
                new_domain.add_point(drop_uv);

                // Set next guess point.
                guess_uv = drop_uv;
            } // end for jj, along this boundary
        } // end for ii, each boundary

        // Since we created the new domain just by dropping points, as opposed to dropping
        // boundary curves, we should expand it a bit.
        new_domain.expand_relative(0.02);

        // But make sure that we didn't expand it to spill outside of sBigDomain.  This
        // would happen if the domain is against the seam of a surface of revolution, for
        // example.  (Upstream ignores the status of the intersection.)
        if let Ok(clipped) = new_domain.intersect(&big_domain) {
            new_domain = clipped;
        }

        // Now union the projection of Other surface on our domain with our original
        // domain, and update our domain to it.
        let orig_domain = self.step_uv_domain();
        let update_domain = orig_domain.union(&new_domain);

        // (status ignored upstream)
        ignored(self.adjust_step_uv_domain(k, &update_domain))?;

        // Have to do this after AdjustSTEPUVDomain call:
        self.remove_uv_trim_curves_of_owner()?;

        Ok(CoverCoincident {
            success: true,
            max_dist,
            covered_step_domain: Some(new_domain),
        })
    }

    /// `SmFace::RemoveUVTrimCurves` of the face that owns this surface, if any.
    fn remove_uv_trim_curves_of_owner(&self) -> Result<()> {
        if self
            .owner()
            .is_some_and(|o| o.type_code == labels::FACE_TYPE)
        {
            // UNPORTED(topology.face_p1): SmFace::RemoveUVTrimCurves
            // (ported as topology::face::p1::remove_uv_trim_curves, but a surface cannot
            // reach the B-rep of its owner face here: the call needs the B-rep passed in)
            return Err(SolidError::unported(
                "SmFace::RemoveUVTrimCurves",
                "topology.face_p1",
            ));
        }
        Ok(())
    }

    /// The point where this surface and `other` meet the plane through `plane_origin`
    /// with normal `plane_normal`, near the guesses (`LocalPlaneSurfaceIntersect`);
    /// `None` when no solution within `xsect_tol_3d` was found.
    ///
    /// # Errors
    ///
    /// Those of the solver and of the evaluations.
    pub fn local_plane_surface_intersect(
        &self,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        xsect_tol_3d: f64,
        uv_guess: Point2,
        other_uv_guess: Point2,
        plane_origin: Point3,
        plane_normal: Vec3,
    ) -> Result<Option<LocalSurfaceSolution>> {
        // allowed interval for the 4 NR unknowns (U0, V0, U1, V1)
        let intervals = domain_intervals(&[uv_domain, other_uv_domain], None);

        // periodicity of 4 NR unknowns (U0, V0, U1, V1)
        let periodicities = [false; 4];

        // initial guesses of 4 NR unknowns (U0, V0, U1, V1)
        let guess = [uv_guess.x, uv_guess.y, other_uv_guess.x, other_uv_guess.y];

        // In some cases like tracing tangency curves we need to get a normal from
        // outside because of the problems with the surface normals being the same.
        let steepest_descent = false;
        let plane_norm = plane_normal.unitize_or_z();

        let plane_surface = Plane::from_normal(plane_origin, plane_norm);

        // construct surf/surf/plane
        let node_tol = xsect_tol_3d / 100.0; // max allowed Param Space deviations for solution
        let sq_node_tol = node_tol * node_tol;
        let mut equations = SspIntersectEnfo::new(
            self,
            other,
            steepest_descent,
            Some(&plane_surface),
            sq_node_tol,
        );
        let mut solver = LocalSolveNd::new(&mut equations, Some(&intervals), Some(&periodicities));

        solver.set_desired_accuracy(EFF_ZERO_SQ);
        solver.set_boundary_handler(BoundaryHandler::TotalBoundaryHits, 3);
        solver.set_maximum_iterations(20);

        // Solve at 4 times tolerance and filter solutions after down to tolerance
        let solution = solver.solve_it(&guess, xsect_tol_3d * 4.0)?;

        // when a solution was found
        if solution.found {
            let uv = uv_at(&solution.x, 0, OP_LOCAL_SOLVE)?;
            let p1 = self.evaluate_point(uv)?;
            let other_uv = uv_at(&solution.x, 2, OP_LOCAL_SOLVE)?;
            let p2 = other.evaluate_point(other_uv)?;
            let dist = p1.distance(p2);

            // when dist between surf pts is less than dist Tolerance - save a solution
            if dist < xsect_tol_3d {
                return Ok(Some(LocalSurfaceSolution {
                    distance: dist,
                    uv,
                    other_uv,
                }));
            }
        }

        Ok(None)
    }

    /// The local minimum, maximum, normalization (the gap vector perpendicular to both
    /// tangent planes) or signed directed minimum (the gap vector parallel to
    /// `opt_vectors[0]`) of two surfaces near the guesses (`LocalSurfaceSolve`); `None`
    /// when no solution was found.
    ///
    /// # Errors
    ///
    /// Those of the solver and of the evaluations; `InvalidInput` for a signed directed
    /// solve without a vector (upstream dereferences `NULL`).
    pub fn local_surface_solve(
        &self,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        operation: SolverOperation,
        opt_vectors: &[Vec3],
        uv_guess: Point2,
        other_uv_guess: Point2,
    ) -> Result<Option<LocalSurfaceSolution>> {
        let signed = operation == SolverOperation::Signed3dDirectedMinimize;

        // for SM_SO_3D_SIGNED_DIRECTED_MINIMIZE call SmSSNormalizeENFO solver: the
        // solution value is the Surf1Point/Surf2Point signed gap size in cpOptVectors[0]
        // direction; and a fifth equation to solve.  ALL OTHER CASES call the equations
        // without using the vector.
        let intervals = domain_intervals(
            &[uv_domain, other_uv_domain],
            signed.then(|| Extent1d::from_min_max(-BIG_DOUBLE, BIG_DOUBLE)),
        );
        let periodicities = [false; 5];
        let mut guess = vec![uv_guess.x, uv_guess.y, other_uv_guess.x, other_uv_guess.y];
        if signed {
            guess.push(0.0);
        }
        let n = guess.len();
        let periodicities = periodicities.get(..n).unwrap_or(&[]);

        let mut equations =
            SsNormalizeEnfo::new(self, other, operation, Some(opt_vectors), EFF_ZERO_SQ);
        let mut solver = LocalSolveNd::new(&mut equations, Some(&intervals), Some(periodicities));
        solver.set_boundary_handler(BoundaryHandler::TotalBoundaryHits, 8);

        // Solve it
        let solution = solver.solve_it(&guess, EFF_ZERO_SQRT)?;

        if !solution.found {
            // no solutions - exit; when not signed directed, if we terminate because of
            // hitting bounds that is ok in the closest point problem.  We have returned
            // the bound hit in dFoundT.
            if signed
                || solver.termination_reason() != TerminationReason::OutOfBounds
                || operation == SolverOperation::Normalize
            {
                // when normalizing must be within bounds
                return Ok(None);
            }
        }

        // compute Surf1Point/Surf2Point gap distance
        let uv = uv_at(&solution.x, 0, OP_LOCAL_SOLVE)?;
        let p1 = self.evaluate_point(uv)?;
        let other_uv = uv_at(&solution.x, 2, OP_LOCAL_SOLVE)?;
        let p2 = other.evaluate_point(other_uv)?;
        let mut dist = p1.distance(p2);
        if signed {
            // Gap Vector going from surface1Point to surface2Point
            let gap = p2 - p1;
            let along = opt_vectors
                .first()
                .ok_or_else(|| SolidError::invalid_input(OP_LOCAL_SOLVE, "no direction vector"))?;
            if gap.dot(*along) < 0.0 {
                dist = -dist;
            }
        }

        Ok(Some(LocalSurfaceSolution {
            distance: dist,
            uv,
            other_uv,
        }))
    }

    /// The local intersection of three surfaces (this one, `surface2` and `surface3`)
    /// near the guesses (`LocalSurfaceSurfaceIntersect`); `None` when no solution (or one
    /// with a degenerate surface normal) was found.
    ///
    /// # Errors
    ///
    /// Those of the solver and of the evaluations.
    pub fn local_surface_surface_intersect(
        &self,
        uv_domain1: &Extent2d,
        surface2: &Surface,
        uv_domain2: &Extent2d,
        surface3: &Surface,
        uv_domain3: &Extent2d,
        uv_guess1: Point2,
        uv_guess2: Point2,
        uv_guess3: Point2,
    ) -> Result<Option<LocalSssSolution>> {
        // temporarily enable OutOfBounds evaluations for all B-spline based surfaces
        let copy1 = out_of_bounds_copy(self);
        let copy2 = out_of_bounds_copy(surface2);
        let copy3 = out_of_bounds_copy(surface3);
        let s1 = copy1.as_ref().unwrap_or(self);
        let s2 = copy2.as_ref().unwrap_or(surface2);
        let s3 = copy3.as_ref().unwrap_or(surface3);

        // gather parameter bounds into a single array
        let intervals = domain_intervals(&[uv_domain1, uv_domain2, uv_domain3], None);

        // create a periodicity array set to ignore periodicity behavior
        let periodicities = [false; 6];

        // create a current problem parameter value guess array
        let guess = [
            uv_guess1.x,
            uv_guess1.y,
            uv_guess2.x,
            uv_guess2.y,
            uv_guess3.x,
            uv_guess3.y,
        ];

        // Construct the Newton-Raphson Solver class with the surf/surf/surf equation set
        let mut equations = SssIntersectEnfo::new(s1, s2, s3, EFF_ZERO_SQ);
        let mut solver = LocalSolveNd::new(&mut equations, Some(&intervals), Some(&periodicities));

        // set the solver's exit behavior for solutions that run outside the boundary
        solver.set_boundary_handler(BoundaryHandler::TotalBoundaryHits, 8);

        // Make the Newton-Raphson solve call
        let solution = solver.solve_it(&guess, EFF_ZERO_SQRT)?;

        // failure case 1 - no solution found
        if !solution.found {
            return Ok(None);
        }

        // failure case 2 - one or more of the surfaces have a degenerate surface normal.
        let [[du1, dv1], [du2, dv2], [du3, dv3]] = equations.srf_tangents;
        let len1 = du1.cross(dv1).length();
        let len2 = du2.cross(dv2).length();
        let len3 = du3.cross(dv3).length();
        if len1 < 1.0e-8 || len2 < 1.0e-8 || len3 < 1.0e-8 {
            return Ok(None);
        }

        // failure case 3 (no unique solution: two or more of the solution point surface
        // normals are parallel) is ok: it happens when removing fillet surface with a
        // miter.

        // find the max distance between any pair of surface evaluations
        let dist = equations.srf_points[0].distance(equations.srf_points[1]);
        let dist2 = equations.srf_points[1].distance(equations.srf_points[2]);
        let dist3 = equations.srf_points[2].distance(equations.srf_points[0]);
        let dist = max3(dist, dist2, dist3);

        Ok(Some(LocalSssSolution {
            distance: dist,
            uv1: uv_at(&solution.x, 0, OP_LOCAL_SOLVE)?,
            uv2: uv_at(&solution.x, 2, OP_LOCAL_SOLVE)?,
            uv3: uv_at(&solution.x, 4, OP_LOCAL_SOLVE)?,
        }))
    }
}

/// `SmFindPSNormalENFO::Evaluate` (the struct is `surface.surface_p1`): `x = [u v]`,
/// `F = [S_u.(S - P)/|S_u|, S_v.(S - P)/|S_v|]` with the normalized first derivatives.
/// Converges when both projected distances are below the tolerance.
impl EvalNFunctions for FindPsNormalEnfo<'_> {
    /// `Evaluate`: true when both projected distances are below the tolerance.
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        let (Some(&u), Some(&v)) = (x.first(), x.get(1)) else {
            return Err(SolidError::sm_err(OP_FIND_PS_NORMAL));
        };
        if x.len() != 2 || f.len() != 2 {
            return Err(SolidError::invalid_input(
                OP_FIND_PS_NORMAL,
                "parameter or function array has the wrong length",
            ));
        }

        // get Surface evaluations
        let uv = Point2::new(u, v);
        let d = evaluate_2nd_derivatives(self.surface, uv, Side::Left, Side::Left)?;
        let (point, su, sv, suv, suu, svv) = (d.point, d.du, d.dv, d.duv, d.duu, d.dvv);

        // failure - zero length surface tangents
        let mu = su.length();
        let mv = sv.length();
        if mu < EFF_ZERO || mv < EFF_ZERO {
            return Err(SolidError::sm_err(OP_FIND_PS_NORMAL));
        }

        // get gap vector
        let gap = point - self.point;

        // A surface is normalized relative to a point when the tangents of the iso
        // parametric curves are perpendicular to the vector from the point on the
        // surface to the normalizing point:  fun[0] = du*(s-p), fun[1] = dv*(s-p).
        let su_n = su / mu;
        let sv_n = sv / mv;

        // Compute function values.
        let f0 = gap.dot(su_n);
        let f1 = gap.dot(sv_n);
        if let Some(slot) = f.get_mut(0) {
            *slot = f0;
        }
        if let Some(slot) = f.get_mut(1) {
            *slot = f1;
        }

        // Compute Jacobian (derivs of function values: Fu, Gv, Fv, Gu).
        if let Some(jac) = jacobian {
            // Note: if we're not close enough to a solution, the second-order terms can
            // dominate the first-order terms, and can change the direction.  The
            // first-order terms always give the correct sign for the step, so the
            // second-order term should not cause a sign change.  In fact, limiting the
            // second-order term to half the size of the first term (if opposite sign)
            // works well.  Note, the first order terms are always positive.
            const SECOND_DERIV_TERM_LIMIT: f64 = 0.5;

            // Calculate all of the normalized derivatives.
            let su_nu = su.unitized_derivative(suu);
            let su_nv = su.unitized_derivative(suv);
            let sv_nu = sv.unitized_derivative(suv);
            let sv_nv = sv.unitized_derivative(svv);

            // dFu:
            let dfu_1 = su.dot(su_n);
            let mut dfu_2 = gap.dot(su_nu);
            if dfu_2 < -dfu_1 * SECOND_DERIV_TERM_LIMIT {
                dfu_2 = -dfu_1 * SECOND_DERIV_TERM_LIMIT;
            }
            let dfu = dfu_1 + dfu_2;

            // dGv:
            let dgv_1 = sv.dot(sv_n);
            let mut dgv_2 = gap.dot(sv_nv);
            if dgv_2 < -dgv_1 * SECOND_DERIV_TERM_LIMIT {
                dgv_2 = -dgv_1 * SECOND_DERIV_TERM_LIMIT;
            }
            let dgv = dgv_1 + dgv_2;

            // and the other two:
            let dfv = sv.dot(su_n) + gap.dot(su_nv);
            let dgu = su.dot(sv_n) + gap.dot(sv_nu);

            jac.set_at(0, 0, dfu)?;
            jac.set_at(0, 1, dfv)?;
            jac.set_at(1, 0, dgu)?;
            jac.set_at(1, 1, dgv)?;
        }

        // check convergence - angle ~= cos(angle) * gapSize < Tolerance
        let proj_dist_u = (f0 / mu).abs();
        let proj_dist_v = (f1 / mv).abs();

        let tol = sm_sqrt(self.squared_tolerance);
        Ok(proj_dist_u < tol && proj_dist_v < tol)
    }
}

/// A tangent with its near-zero components forced to zero (prevents matrix inversions
/// of non-invertible matrices).
fn snap_tangent(v: Vec3) -> Vec3 {
    let snap = |c: f64| if c.abs() < EFF_ZERO { 0.0 } else { c };
    Vec3::new(snap(v.x), snap(v.y), snap(v.z))
}

/// `SmSSSIntersectENFO::Evaluate` (the struct is `surface.surface_p1`): `x = [u1 v1 u2 v2
/// u3 v3]`, `F = [S1 - S2, S1 - S3]`; keeps the surface points and tangents of the last
/// evaluation (`m_sSrfPoint*`, `m_sDU*`, `m_sDV*`).
impl EvalNFunctions for SssIntersectEnfo<'_> {
    /// `Evaluate`: never reports an answer found.
    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> Result<bool> {
        if x.len() != 6 || f.len() != 6 {
            return Err(SolidError::invalid_input(
                OP_SSS_INTERSECT,
                "parameter or function array has the wrong length",
            ));
        }
        self.count += 1;

        // evaluate the positions and tangents of the three surfaces
        let surfaces = [self.surface1, self.surface2, self.surface3];
        for (i, surface) in surfaces.iter().enumerate() {
            let uv = uv_at(x, 2 * i, OP_SSS_INTERSECT)?;
            let (p, du, dv) = evaluate_1st_derivatives(*surface, uv, Side::Left, Side::Left)?;
            if let (Some(sp), Some(st)) = (self.srf_points.get_mut(i), self.srf_tangents.get_mut(i))
            {
                *sp = p;
                *st = [du, dv];
            }
        }

        // if we have a zero length tangent then we must fail
        if self
            .srf_tangents
            .iter()
            .flatten()
            .any(|t| t.length_squared() < EFF_ZERO_SQ)
        {
            return Err(SolidError::sm_err(OP_SSS_INTERSECT));
        }

        // force near zero tangent components to equal zero to prevent matrix inversions
        // of non-invertable matrices
        for t in self.srf_tangents.iter_mut().flatten() {
            *t = snap_tangent(*t);
        }

        let [p1, p2, p3] = self.srf_points;
        let [[du1, dv1], [du2, dv2], [du3, dv3]] = self.srf_tangents;

        // Intersection simply tries to minimize the following six functions
        // fun[0..3] = S1(u1,v1) - S2(u2,v2); fun[3..6] = S1(u1,v1) - S3(u3,v3)
        let s1_minus_s2 = p1 - p2;
        let s1_minus_s3 = p1 - p3;

        // It produces the following Jacobian
        //  | dS1/du1   dS1/dv1  -dS2/du2  -dS2/dv2   0          0         |
        //  | dS1/du1   dS1/dv1  0          0           -dS3/du3  -dS3/dv3 |
        if let Some(jac) = jacobian {
            let rows = [
                (du1.x, dv1.x, du2.x, dv2.x, du3.x, dv3.x),
                (du1.y, dv1.y, du2.y, dv2.y, du3.y, dv3.y),
                (du1.z, dv1.z, du2.z, dv2.z, du3.z, dv3.z),
            ];
            for (k, (a1, b1, a2, b2, a3, b3)) in rows.into_iter().enumerate() {
                jac.set_at(k, 0, a1)?;
                jac.set_at(k, 1, b1)?;
                jac.set_at(k, 2, -a2)?;
                jac.set_at(k, 3, -b2)?;
                jac.set_at(k, 4, 0.0)?;
                jac.set_at(k, 5, 0.0)?;

                jac.set_at(k + 3, 0, a1)?;
                jac.set_at(k + 3, 1, b1)?;
                jac.set_at(k + 3, 2, 0.0)?;
                jac.set_at(k + 3, 3, 0.0)?;
                jac.set_at(k + 3, 4, -a3)?;
                jac.set_at(k + 3, 5, -b3)?;
            }
        }

        // Compute function values
        for (slot, value) in f.iter_mut().zip([
            s1_minus_s2.x,
            s1_minus_s2.y,
            s1_minus_s2.z,
            s1_minus_s3.x,
            s1_minus_s3.y,
            s1_minus_s3.z,
        ]) {
            *slot = value;
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::nurbs::b_spline_curve::BSplineCurve as Bsc;
    use crate::surface::surf_of_revolution::p1::gen_base;
    use crate::surface::surf_of_revolution::SurfOfRevolution;

    /// The temporary out-of-bounds change sets only the surface's own flag member: a
    /// surface of revolution's generator keeps its flag (upstream
    /// `SmTemporaryChangeValue` never calls the virtual `SetOutOfBoundsEnabled`).
    #[test]
    fn out_of_bounds_change_leaves_the_generator_flag() {
        let k = Kernel::default();
        let mut l = Line::from_points(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 2.0), 3);
        l.make_nurb().unwrap();
        let mut rev =
            SurfOfRevolution::create_canonical(&k, Curve::from(l), Vec3::ZERO, Vec3::Z, false)
                .unwrap();
        rev.set_out_of_bounds_enabled(false);
        let gen_flag = |s: &Surface| match s.kind() {
            SurfaceKind::SurfOfRevolution(r) => r
                .gen_curve()
                .and_then(|g| gen_base(g).ok())
                .map(Bsc::is_out_of_bounds_enabled),
            _ => None,
        };
        let mut s = Surface::from(rev);
        assert_eq!(out_of_bounds_enabled(&s), Some(false));
        assert_eq!(gen_flag(&s), Some(false));
        let copy = out_of_bounds_copy(&s).unwrap();
        assert_eq!(out_of_bounds_enabled(&copy), Some(true));
        assert_eq!(gen_flag(&copy), Some(false));
        set_out_of_bounds_enabled(&mut s, true);
        set_out_of_bounds_enabled(&mut s, false);
        assert_eq!(gen_flag(&s), Some(false));
    }
}
