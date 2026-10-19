//! Fillet parameter setup, surface/surface filleting, smooth edge sequences, the
//! temporary radius change and the fillet-law constructors of `SmFilletExecutive`.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletExecutive::set_fillet_parameters`], [`FilletParameters`] | `SmFilletExecutive::SetFilletParameters` (edge pointers, per-edge parameters) |
//! | [`surface_surface_fillet`], [`SurfaceFilletOptions`] | `SmFilletExecutive::SurfaceSurfaceFillet` |
//! | [`FilletExecutive::do_surface_filleting`] | `SmFilletExecutive::DoSurfaceFilleting` |
//! | [`FilletExecutive::set_fillet_brep_edge_map`] | `SmFilletExecutive::SetFilletBrepEdgeMap` |
//! | [`FilletExecutive::process_smooth_edge_sequences`] | `SmFilletExecutive::ProcessSmoothEdgeSequences` |
//! | [`FilletExecutive::collect_tangent_edge_sequence`] | `SmFilletExecutive::CollectTangentEdgeSequence` |
//! | [`TemporaryRadiusChange::new`], [`TemporaryRadiusChange::revert`], [`with_temporary_radius_change`] | `SmTemporaryRadiusChange` (constructor, destructor) |
//! | [`TemporaryRadiusChange::set_up`] | `SmTemporaryRadiusChange::SetUp` |
//! | `revert_change` (private) | `SmTemporaryRadiusChange::Revert` |
//! | [`FilletLaw::linear`] | `SmLinearFilletLaw::SmLinearFilletLaw` |
//! | [`FilletLaw::bspline_elliptical`] | `SmBSplineFilletLaw::SmBSplineFilletLaw` (elliptical blend) |
//!
//! Deviations:
//! - **Object model.** As in `p3`, solvers, corners, fillet geometry, surface
//!   generators, laws handed in by the caller and B-reps are handles `O` resolved by an
//!   [`ExecutiveAccess`] adapter; the executive is p3's [`FilletExecutive`] state.
//!   Constructors that allocate upstream objects (`new SmFilletSolver`,
//!   `new SmFilletSurfaceGenerator`) are adapter calls taking a spec
//!   ([`SolverSpec`], [`SurfaceGeneratorSpec`]) with every upstream argument.
//! - **Fillet laws** extend the `FilletLaw` co-ported by `blend.fillet_standard_solver_p1`
//!   (PR #168). An elliptical B-spline law whose interpolation fails keeps a null law
//!   curve upstream (`SE` only logs), which every later use dereferences; the port
//!   returns that interpolation error instead (UB-198 in `plan/upstream-issues.md`).
//! - **RAII.** `SmTemporaryRadiusChange`'s destructor is
//!   [`TemporaryRadiusChange::revert`]; [`with_temporary_radius_change`] runs it after
//!   the body, also on errors. `SmTemporaryChangeValue` on the context's doing-Boolean
//!   flag and the fillet B-rep's editing flag is a save/restore around the body.
//! - **Error notes.** `SmFilletErrorInfo::NoteError` and
//!   `SmFilletExecutive::NoteFilletError` belong to `blend.fillet_executive_p1`; their
//!   `%p` message text cannot be reproduced and the messages carry the edge index only.
//! - **Null input.** `SetFilletParameters` dereferences a null B-rep (with a non-empty
//!   edge list) and a null variable-radius law list (for a variable-radius type);
//!   `SurfaceSurfaceFillet` dereferences a null UV curve. The port returns
//!   `SM_ERR_NULL_POINTER` there.
//! - **Bounds.** `CollectTangentEdgeSequence` follows tangent solver chains through
//!   the corners; a chain that cycles without returning to its first solver never
//!   ends upstream (HANG-36). Each walk is bounded by the solver count plus one step,
//!   past which the port returns [`SolidError::InvalidInput`].
//! - The upstream debug drawing (`SM_GFX_CODE`) is omitted.
//! - Calls into units that are not ported return [`SolidError::Unported`]: the
//!   executive constructor, `NoteError`, `NoteFilletError`,
//!   `AddFilletSurfaceGenerator`, `LoadFilletSolver`, `GetFilletSolverOfEdge`,
//!   `CreateSmoothEdgeSequence` and `SmFilletLaw::Evaluate`
//!   (`blend.fillet_executive_p1`), and the adapter defaults of [`ExecutiveAccess`].

use crate::blend::fillet_executive::p3::{
    ContextMark, ErrorInfo, ExecutionOptions, ExecutiveAccess, FilletExecutive,
};
use crate::blend::fillet_standard_solver::p1::{
    BoundaryTrimmingType, FilletLaw, FilletLawKind, FilletSolverType,
    FilletSurfaceGeneratorType,
};
use crate::curve::curve::Curve;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::error::{ignored, Result, SolidError, SM_ERR, SM_ERR_INVALID_INPUT};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::SolutionObject;
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::Point3;
use crate::intersect::point_class::ClassObject;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, PI};
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use crate::surface::surface::Surface;

const OP: &str = "blend::fillet_executive::p2";

/// `SM_FILERR_INVALID_INPUT` (`SmFilletErrorType`).
pub const FILERR_INVALID_INPUT: i32 = 2;
/// `SM_FILERR_EDGE_PROBLEM` (`SmFilletErrorType`).
pub const FILERR_EDGE_PROBLEM: i32 = 4;
/// `SM_FIL_FAILURE` (`SmFilStatus`).
pub const FIL_FAILURE: i32 = 2;

fn status(code: i32) -> SolidError {
    SolidError::Upstream {
        operation: OP,
        code,
        entity: None,
    }
}

/// `SM_ERR_NULL_POINTER` (`NER`, or a null upstream dereferences); `what` names it.
fn null(what: &'static str) -> SolidError {
    let _ = what;
    SolidError::null_pointer(OP)
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `SM_DEG2RAD(adeg)`: `adeg * (SM_PI / 180)`.
fn deg_to_rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

// ---- fillet laws -------------------------------------------------------------------------

impl FilletLaw {
    /// `SmLinearFilletLaw(dStartValue, dEndValue, pOptEdgeMap)`: a radius running
    /// linearly from `start_value` to `end_value`. The edge map is `edge_map` when
    /// given, and `[0, 1]` when absent or shorter than `SM_EFF_ZERO`.
    #[must_use]
    pub fn linear(start_value: f64, end_value: f64, edge_map: Option<&Extent1d>) -> Self {
        let mut law = Self::new(FilletLawKind::Linear {
            start_radius: start_value,
            end_radius: end_value,
        });
        if let Some(map) = edge_map {
            law.edge_map = *map;
        }
        if law.edge_map.length() < EFF_ZERO {
            law.edge_map = Extent1d::from_raw(0.0, 1.0);
        }
        law
    }

    /// `SmBSplineFilletLaw(crContext, dStartRadius, dEndRadius, dOuterRadius,
    /// dAngleSpan, dStartDeriv, dEndDeriv, pOptEdgeMap)`: an elliptical cosine/sine
    /// blend from `start_radius` to `end_radius` under an arc of `outer_radius`
    /// spanning `angle_span`, as a degree-2 B-spline interpolating 101 samples
    /// `(radius, angle, 0)` at chord-length parameters. An outer radius not above
    /// both end radii becomes their sum. `start_deriv` and `end_deriv` are unused, as
    /// upstream.
    ///
    /// # Errors
    ///
    /// Those of the interpolation (upstream keeps a law with a null curve; UB-198).
    #[allow(clippy::too_many_arguments)] // upstream's constructor
    pub fn bspline_elliptical(
        start_radius: f64,
        end_radius: f64,
        mut outer_radius: f64,
        angle_span: f64,
        start_deriv: f64,
        end_deriv: f64,
        edge_map: Option<&Extent1d>,
    ) -> Result<Self> {
        let _ = (start_deriv, end_deriv);
        let mut map = Extent1d::from_raw(0.0, 1.0);
        if let Some(m) = edge_map {
            map = *m;
        }
        if map.length() < EFF_ZERO {
            map = Extent1d::from_raw(0.0, 1.0);
        }

        if outer_radius <= end_radius || outer_radius <= start_radius {
            // SE(SM_ERR) only logs
            outer_radius = end_radius + start_radius;
        }

        // Change from Hermite to Elliptical cosine/sine blending. In the case where
        // the dTotalAngle is 90 degrees it will produce an ellipse in the box case.
        let ivl = Extent1d::from_min_max(0.0, 1.0);

        let b = outer_radius - end_radius;
        let a = outer_radius - start_radius;
        let mut points = Vec::new();
        let mut params = Vec::new();
        for i in 0..=100u32 {
            let t = ivl.evaluate(f64::from(i) / 100.0);
            let t_ang = t * angle_span;
            let tan = t_ang.tan();
            let val = tan * a / b;
            let t_ang2 = val.atan();
            let x = a * t_ang2.cos();
            let y = b * t_ang2.sin();
            let rad = outer_radius - (x * x + y * y).sqrt();
            points.push(Point3::new(rad, t_ang, 0.0));
            params.push(t);
        }

        let law_curve = BSplineCurve::interpolate_points(
            &points,
            Some(&params),
            2,
            None,
            None,
            false,
            ParamMethod::ChordLength,
        )?;
        let mut law = Self::new(FilletLawKind::BSpline { law_curve });
        law.edge_map = map;
        Ok(law)
    }
}

/// `SmFilletLaw::Evaluate(dParameter, crCurveInterval, bReverseOrientation, dValues)`
/// (unit `blend.fillet_executive_p1`).
fn law_evaluate(law: &FilletLaw, parameter: f64, interval: &Extent1d, reverse: bool) -> Result<[f64; 3]> {
    law.evaluate(parameter, interval, reverse)
}

// ---- adapter specs -----------------------------------------------------------------------

/// The fillet surface generator to allocate (`new SmLinearCrossSectionFSG`,
/// `SmCircularCrossSectionFSG(bApproximateArcs, dTolPercentRadius, bMirror,
/// bComplement)`, `SmBlendCurveCrossSectionFSG(dBlendScale, lContinuity)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceGeneratorSpec {
    /// `SmLinearCrossSectionFSG`.
    Linear,
    /// `SmCircularCrossSectionFSG`.
    Circular {
        /// `bApproximateArcs`.
        approximate_arcs: bool,
        /// `dTolPercentRadius`.
        tol_percent_radius: f64,
        /// `bMirror`.
        mirror: bool,
        /// `bComplement`.
        complement: bool,
    },
    /// `SmBlendCurveCrossSectionFSG`.
    BlendCurve {
        /// `dBlendScale` (the thumbweight).
        blend_scale: f64,
        /// `lContinuity`.
        continuity: u32,
    },
}

/// The fillet solver to allocate, with every constructor argument and the setters
/// `SurfaceSurfaceFillet` applies right after construction.
#[derive(Debug, Clone, Copy)]
pub enum SolverSpec<'a, O> {
    /// `SmConstantRadiusFS(ctx, dApproxTol, dAngTolRad, dTanTolRad, dRadius, pEU)`.
    ConstantRadius {
        /// `dApproxTol`.
        approx_tol: f64,
        /// `dAngTolRad`.
        ang_tol_rad: f64,
        /// `dTanTolRad`.
        tan_tol_rad: f64,
        /// `dRadius`.
        radius: f64,
        /// `pEU`.
        edgeuse: O,
    },
    /// `SmConstantDistanceFS(ctx, dApproxTol, dAngTolRad, dTanTolRad, dRadius, pEU)`.
    ConstantDistance {
        /// `dApproxTol`.
        approx_tol: f64,
        /// `dAngTolRad`.
        ang_tol_rad: f64,
        /// `dTanTolRad`.
        tan_tol_rad: f64,
        /// `dRadius`.
        radius: f64,
        /// `pEU`.
        edgeuse: O,
    },
    /// `SmVariableRadiusFS(ctx, dApproxTol, dAngTolRad, dTanTolRad, dRadius, pEU,
    /// rLaw, bReverse)`.
    VariableRadius {
        /// `dApproxTol`.
        approx_tol: f64,
        /// `dAngTolRad`.
        ang_tol_rad: f64,
        /// `dTanTolRad`.
        tan_tol_rad: f64,
        /// `dRadius`.
        radius: f64,
        /// `pEU`.
        edgeuse: O,
        /// The law (`*(*pOptVarRadLaws)[ii]`).
        law: O,
        /// The law orientation argument (`FALSE` from `SetFilletParameters`).
        reverse: bool,
    },
    /// `SmSurfaceSurfaceFS(ctx, dTolerance, dAngTol, dTanTol, |r1|, |r2|, *pSur1,
    /// *pSur2, bOrient1, bOrient2, sGuess1, sGuess2)`, then `SetFilletTrimType` and
    /// `SetReverseTrim`.
    SurfaceSurface {
        /// `dTolerance`.
        tolerance: f64,
        /// `dAngTol`.
        ang_tol: f64,
        /// `dTanTol`.
        tan_tol: f64,
        /// The two absolute radii.
        radii: [f64; 2],
        /// `*pSur1`, `*pSur2`.
        surfaces: [&'a Surface; 2],
        /// `bOrient1`, `bOrient2`.
        orientations: [bool; 2],
        /// `sGuess1`, `sGuess2`.
        guesses: [Vec2; 2],
        /// `SetFilletTrimType(eFilTrimType)`.
        trim_type: BoundaryTrimmingType,
        /// `SetReverseTrim(bReverseTrim)`.
        reverse_trim: bool,
    },
}

// ---- unported executive members (blend.fillet_executive_p1) --------------------------------

/// `GetFilletErrorInfo()->NoteError(eErrorCode, cpComment)` (unit
/// `blend.fillet_executive_p1`).
fn note_error<O>(executive: &mut FilletExecutive<O>, code: i32, comment: &str) -> Result<()> {
    let _ = (executive, code, comment);
    // UNPORTED(blend.fillet_executive_p1): SmFilletErrorInfo::NoteError
    Err(SolidError::unported(
        "SmFilletErrorInfo::NoteError",
        "blend.fillet_executive_p1",
    ))
}

/// `GetFilletErrorInfo()->NoteError(pEdge, eFilStatus, eErrorCode, cpComment)` (unit
/// `blend.fillet_executive_p1`).
fn note_edge_error<O>(
    executive: &mut FilletExecutive<O>,
    edge: O,
    fillet_status: i32,
    code: i32,
    comment: &str,
) -> Result<()> {
    let _ = (executive, edge, fillet_status, code, comment);
    // UNPORTED(blend.fillet_executive_p1): SmFilletErrorInfo::NoteError
    Err(SolidError::unported(
        "SmFilletErrorInfo::NoteError",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive::NoteFilletError(eErrorCode, pFilletEdge, cpComment)` (unit
/// `blend.fillet_executive_p1`).
fn note_fillet_error<O>(
    executive: &mut FilletExecutive<O>,
    code: i32,
    fillet_edge: O,
    comment: &str,
) -> Result<()> {
    let _ = (executive, code, fillet_edge, comment);
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::NoteFilletError
    Err(SolidError::unported(
        "SmFilletExecutive::NoteFilletError",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive::AddFilletSurfaceGenerator(pFSG)` (unit
/// `blend.fillet_executive_p1`).
fn add_fillet_surface_generator<O>(executive: &mut FilletExecutive<O>, generator: O) -> Result<()> {
    let _ = (executive, generator);
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::AddFilletSurfaceGenerator
    Err(SolidError::unported(
        "SmFilletExecutive::AddFilletSurfaceGenerator",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive::LoadFilletSolver(pFS)` (unit `blend.fillet_executive_p1`).
fn load_fillet_solver<O>(executive: &mut FilletExecutive<O>, solver: O) -> Result<()> {
    let _ = (executive, solver);
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::LoadFilletSolver
    Err(SolidError::unported(
        "SmFilletExecutive::LoadFilletSolver",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive::GetFilletSolverOfEdge(pEdge)` (unit
/// `blend.fillet_executive_p1`).
fn fillet_solver_of_edge<O>(executive: &FilletExecutive<O>, edge: O) -> Result<Option<O>> {
    let _ = (executive, edge);
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::GetFilletSolverOfEdge
    Err(SolidError::unported(
        "SmFilletExecutive::GetFilletSolverOfEdge",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive::CreateSmoothEdgeSequence(rSmoothSequence)` (unit
/// `blend.fillet_executive_p1`).
fn create_smooth_edge_sequence<O>(executive: &mut FilletExecutive<O>, sequence: &[O]) -> Result<()> {
    let _ = (executive, sequence);
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::CreateSmoothEdgeSequence
    Err(SolidError::unported(
        "SmFilletExecutive::CreateSmoothEdgeSequence",
        "blend.fillet_executive_p1",
    ))
}

/// `SmFilletExecutive(crContext)`: a new executive in the context (unit
/// `blend.fillet_executive_p1`).
fn new_executive<O>() -> Result<FilletExecutive<O>> {
    // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::SmFilletExecutive
    Err(SolidError::unported(
        "SmFilletExecutive::SmFilletExecutive",
        "blend.fillet_executive_p1",
    ))
}

// ---- SetFilletParameters -------------------------------------------------------------------

/// The arguments of `SetFilletParameters` (edge pointers, possibly different
/// parameters on each edge). A type or value list holds one entry for all edges or
/// one per edge.
#[derive(Debug, Clone, Copy)]
pub struct FilletParameters<'a, O> {
    /// `pBrep` (kept as the target when not null).
    pub brep: Option<O>,
    /// `rEdgePtrs`.
    pub edges: &'a [O],
    /// `rXSectTypes`.
    pub xsect_types: &'a [FilletSurfaceGeneratorType],
    /// `rRadiusTypes`.
    pub radius_types: &'a [FilletSolverType],
    /// `rRadiusValues`.
    pub radius_values: &'a [f64],
    /// `pOptVarRadLaws` (`None` for the null list; `None` entries are null laws).
    pub var_rad_laws: Option<&'a [Option<O>]>,
    /// `dApproxTol` (default if `<= SM_EFF_ZERO`: 10 times the B-rep tolerance).
    pub approx_tol: f64,
    /// `dAngTolDeg` (default if `<= SM_EFF_ZERO`: 20).
    pub ang_tol_deg: f64,
    /// `dTanTolDeg` (default if `<= SM_EFF_ZERO`: 10).
    pub tan_tol_deg: f64,
    /// `dXSectTol` (default if `<= SM_EFF_ZERO`: 0.05).
    pub xsect_tol: f64,
    /// `lContinuity` (header default 1).
    pub continuity: u32,
    /// `dThumbweight` (header default 1.0).
    pub thumbweight: f64,
}

/// Whether one list entry serves all `n` edges (`TRUE`), one serves each (`FALSE`),
/// or neither (`UNSURE`, here `None`).
fn single(len: usize, n: usize) -> Option<bool> {
    if len == 1 {
        Some(true)
    } else if len == n {
        Some(false)
    } else {
        None
    }
}

impl<O: ClassObject + SolutionObject + Ord> FilletExecutive<O> {
    /// `SetFilletParameters`: resets the five operation switches to their common
    /// values and, for each edge, creates its fillet solver (of the edge's radius type
    /// and value, on its blend edge use) and gives it the surface generator of its
    /// cross-section type; the executive is initialized when every edge succeeded. An
    /// empty edge list succeeds without initializing.
    ///
    /// # Errors
    ///
    /// `SM_ERR_INVALID_INPUT` (after noting the error) for list sizes that fit neither
    /// one nor every edge, an unknown cross-section type, an edge without blend edge
    /// use, a missing variable-radius law or an unsupported radius type;
    /// `SM_ERR_NULL_POINTER` for a null B-rep or law list upstream dereferences; those
    /// of the adapter calls.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn set_fillet_parameters<A>(
        &mut self,
        k: &mut Kernel,
        access: &mut A,
        params: &FilletParameters<'_, O>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        self.initialized = false;

        if let Some(brep) = params.brep {
            self.target = Some(brep);
        }

        // First set some standard flags.
        self.options = ExecutionOptions {
            piecewise_merge: false,
            topology_insertion: true,
            global_merge: false,
            trimming: true,
            classification: true,
        };

        let num_filleted_edges = params.edges.len();
        if num_filleted_edges < 1 {
            return Ok(());
        }

        let single_xsect_type = single(params.xsect_types.len(), num_filleted_edges);
        let single_radius_type = single(params.radius_types.len(), num_filleted_edges);
        let single_radius_value = single(params.radius_values.len(), num_filleted_edges);

        let (Some(single_xsect_type), Some(single_radius_type), Some(single_radius_value)) =
            (single_xsect_type, single_radius_type, single_radius_value)
        else {
            note_error(
                self,
                FILERR_INVALID_INPUT,
                "SetFilletParameters: Input array size mismatch.",
            )?;
            return Err(status(SM_ERR_INVALID_INPUT));
        };

        // pBrep->GetContext()
        let brep = params.brep.ok_or_else(|| null("null B-rep"))?;

        // Note, dApproxTol will be reset for each edge: the default (10 times the B-rep
        // tolerance) is computed but never read.
        if params.approx_tol <= EFF_ZERO {
            let _ = 10.0 * access.tolerance(k, brep, false)?;
        }
        let mut ang_tol_deg = params.ang_tol_deg;
        if ang_tol_deg <= EFF_ZERO {
            ang_tol_deg = 20.0;
        }
        let mut tan_tol_deg = params.tan_tol_deg;
        if tan_tol_deg <= EFF_ZERO {
            tan_tol_deg = 10.0;
        }
        let mut xsect_tol = params.xsect_tol;
        if xsect_tol <= EFF_ZERO {
            xsect_tol = 0.05;
        }
        let ang_tol_rad = deg_to_rad(ang_tol_deg);
        let tan_tol_rad = deg_to_rad(tan_tol_deg);

        // We will use these values throughout, if Single is requested.
        let first = |what| invalid(what);
        let mut radius_type = *params.radius_types.first().ok_or_else(|| first("no radius type"))?;
        let mut xsect_type = *params.xsect_types.first().ok_or_else(|| first("no cross-section type"))?;
        let mut radius = *params.radius_values.first().ok_or_else(|| first("no radius"))?;
        let mut generator: Option<O> = None;

        let generator_spec = |kind: FilletSurfaceGeneratorType, circular_tol: f64| match kind {
            FilletSurfaceGeneratorType::Linear => Some(SurfaceGeneratorSpec::Linear),
            FilletSurfaceGeneratorType::Circular => Some(SurfaceGeneratorSpec::Circular {
                approximate_arcs: true,
                tol_percent_radius: circular_tol,
                mirror: false,
                complement: false,
            }),
            FilletSurfaceGeneratorType::BlendCurve => Some(SurfaceGeneratorSpec::BlendCurve {
                blend_scale: params.thumbweight,
                continuity: params.continuity,
            }),
            FilletSurfaceGeneratorType::Unknown => None,
        };

        if single_xsect_type {
            let Some(spec) = generator_spec(xsect_type, xsect_tol) else {
                note_error(
                    self,
                    FILERR_INVALID_INPUT,
                    "SetFilletParameters: Invalid Fillet Surface Generator type.",
                )?;
                return Err(status(SM_ERR_INVALID_INPUT));
            };
            let fsg = access.new_surface_generator(k, spec)?;
            generator = Some(fsg);
            // Save it, so that we can delete it when filleting is finished.
            add_fillet_surface_generator(self, fsg)?;
        }

        // Now for each Filleted Edge.
        for (ii, &edge) in params.edges.iter().enumerate() {
            // 1. Find the appropriate Edgeuse for the fillet.
            let Some(eu) = access.blend_edgeuse(k, edge)? else {
                // This edge is not filletable.
                note_edge_error(
                    self,
                    edge,
                    FIL_FAILURE,
                    FILERR_INVALID_INPUT,
                    &format!("Edge[ {ii:2} ]: cannot be filleted\n"),
                )?;
                return Err(status(SM_ERR_INVALID_INPUT));
            };

            // 2. Fillet Solver: switch on radius type.
            if !single_radius_type {
                radius_type = *params
                    .radius_types
                    .get(ii)
                    .ok_or_else(|| first("radius type list too short"))?;
            }
            if !single_radius_value {
                radius = *params
                    .radius_values
                    .get(ii)
                    .ok_or_else(|| first("radius list too short"))?;
            }

            // Note, this works well in practice.  [B332]
            const TOL_FACTOR: f64 = 4.0;
            let approx_tol = TOL_FACTOR * access.edge_tolerance(edge)?;

            let spec = match radius_type {
                FilletSolverType::ConstRadius => Some(SolverSpec::ConstantRadius {
                    approx_tol,
                    ang_tol_rad,
                    tan_tol_rad,
                    radius,
                    edgeuse: eu,
                }),
                FilletSolverType::ConstDist => Some(SolverSpec::ConstantDistance {
                    approx_tol,
                    ang_tol_rad,
                    tan_tol_rad,
                    radius,
                    edgeuse: eu,
                }),
                FilletSolverType::VariableRadius => {
                    let laws = params
                        .var_rad_laws
                        .ok_or_else(|| null("null variable-radius law list"))?;
                    match laws.get(ii).copied().flatten() {
                        Some(law) => Some(SolverSpec::VariableRadius {
                            approx_tol,
                            ang_tol_rad,
                            tan_tol_rad,
                            radius,
                            edgeuse: eu,
                            law,
                            reverse: false,
                        }),
                        _ => {
                            note_edge_error(
                                self,
                                edge,
                                FIL_FAILURE,
                                FILERR_INVALID_INPUT,
                                &format!("Edge[ {ii:2} ]: no variable-radius law specified\n"),
                            )?;
                            return Err(status(SM_ERR_INVALID_INPUT));
                        }
                    }
                }
                // Not set up for these yet (they fall through to SM_FS_UNKNOWN).
                FilletSolverType::SurfSurf
                | FilletSolverType::CurveBased
                | FilletSolverType::ConstRadiusAssisted
                | FilletSolverType::Unknown => None,
            };
            let Some(spec) = spec else {
                note_edge_error(
                    self,
                    edge,
                    FIL_FAILURE,
                    FILERR_INVALID_INPUT,
                    &format!("Edge[ {ii:2} ]: invalid radius type (Fillet Solver)\n"),
                )?;
                return Err(status(SM_ERR_INVALID_INPUT));
            };
            let fs = access.new_fillet_solver(k, &spec)?;
            load_fillet_solver(self, fs)?;

            // 3. Fillet Surface Generator: switch on cross-section type.
            if !single_xsect_type {
                xsect_type = *params
                    .xsect_types
                    .get(ii)
                    .ok_or_else(|| first("cross-section type list too short"))?;
                let Some(spec) = generator_spec(xsect_type, 0.1) else {
                    note_error(
                        self,
                        FILERR_INVALID_INPUT,
                        "SetFilletParameters: Invalid Fillet Surface Generator type.",
                    )?;
                    return Err(status(SM_ERR_INVALID_INPUT));
                };
                let fsg = access.new_surface_generator(k, spec)?;
                generator = Some(fsg);
                // Save it, so that we can delete it when filleting is finished.
                add_fillet_surface_generator(self, fsg)?;
            }
            let fsg = generator.ok_or_else(|| null("no surface generator"))?;
            access.set_surface_generator(fs, fsg)?;
        }

        // Give the ok to proceed with the filleting.
        self.initialized = true;
        Ok(())
    }

    // ---- DoSurfaceFilleting --------------------------------------------------------------

    /// `DoSurfaceFilleting`: builds the fillet geometry of every solver into a new
    /// fillet B-rep (one face per fillet surface, stitched), maps the fillet edges to
    /// it, inserts the intersection topology when asked and trims the original
    /// surfaces when asked, with the context's doing-Boolean flag set throughout. The
    /// error info is copied to `error_out`.
    ///
    /// # Errors
    ///
    /// Those of the geometry, topology, stitching and trimming calls upstream checks
    /// (`SER`); a solver whose geometry fails is skipped.
    pub fn do_surface_filleting<A>(
        &mut self,
        k: &mut Kernel,
        access: &mut A,
        error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let doing_boolean = k.context.doing_boolean();
        k.context.set_doing_boolean(true);
        let result = self.do_surface_filleting_body(k, access, error_out);
        k.context.set_doing_boolean(doing_boolean);
        result
    }

    fn do_surface_filleting_body<A>(
        &mut self,
        k: &mut Kernel,
        access: &mut A,
        error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        // create output Fillet Brep to hold all the new Fillet Faces
        let fillet = access.new_brep(k)?;
        self.fillet = Some(fillet);

        // init FilletBrep for editing (SmTemporaryChangeValue)
        let editing = access.editing_enabled(fillet)?;
        access.set_editing_enabled(fillet, true)?;
        let result = self.surface_fillet_body(k, access, fillet, error_out);
        let restore = access.set_editing_enabled(fillet, editing);
        result.and(restore)
    }

    fn surface_fillet_body<A>(
        &mut self,
        k: &mut Kernel,
        access: &mut A,
        fillet: O,
        error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        // for every FilletSolver create a FilletGeom
        let mut tol = 0.0_f64;
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            index += 1;
            if ignored(access.calc_fillet_geom(k, solver, self))?.is_none() {
                continue;
            }
            tol = tol.max(access.solver_approx_tolerance(solver)?);
        }

        // SetTolerance(dTol) with its header defaults (void upstream)
        ignored(access.set_brep_tolerance(k, fillet, tol, true, true))?;

        // for every FilletSolver->FilletGeom->FilletSurface, make a Face in the fillet
        // B-rep
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            index += 1;
            for geom in access.solver_geoms(solver)? {
                if !access.geom_has_surface(geom)? {
                    continue;
                }
                access.make_face_brep(k, geom, fillet, self)?;
            }
        }

        // For every Edge in the fillet B-rep - add attribute(m_pAttributeE)
        for edge in access.edges(fillet)? {
            ignored(access.add_attribute(edge, self.edge_attribute))?;
        }

        // stitch all topology in the fillet B-rep
        access.set_editing_enabled(fillet, true)?;
        let stitch = access.stitch_faces(k, fillet, tol, None, false)?;
        if let Some(error) = stitch.status {
            return Err(error);
        }

        // Set up a map between the fillet curves in the FilletGeoms and corresponding
        // fillet brep edges.
        self.set_fillet_brep_edge_map(k, access)?;

        if self.options.topology_insertion {
            let mut index = 0;
            while let Some(solver) = self.solvers.get(index).copied() {
                index += 1;
                for geom in access.solver_geoms(solver)? {
                    if !access.geom_has_surface(geom)? {
                        continue;
                    }
                    // Merge each rail into originating face and more. . .
                    access.insert_geom_topology(k, geom, self)?;
                }
            }
        }

        // done with fillet B-rep editing
        access.set_editing_enabled(fillet, false)?;

        if self.options.trimming {
            // Mark all common vertices and edges: the original object (in its own
            // context) first, then the fillet object (the executive's context).
            let mut mates = Vec::new();
            for vertex in access.vertices(fillet)? {
                if let Some(original) = self.intersector.brep_mate(Some(vertex)) {
                    mates.push((original, vertex));
                }
            }
            for edge in access.edges(fillet)? {
                if let Some(original) = self.intersector.brep_mate(Some(edge)) {
                    mates.push((original, edge));
                }
            }
            // There is no target B-rep: the original surfaces may come from several
            // B-reps, so the fillet B-rep stands for both and every original object's
            // context is resolved from the object.
            access.with_marks(k, fillet, fillet, &mates, |access, k, marks| {
                self.trim_surface_fillets(k, access, marks)
            })?;
        }

        if let Some(output) = error_out {
            output.assign(&self.error);
        }
        Ok(())
    }

    fn trim_surface_fillets<A>(
        &mut self,
        k: &mut Kernel,
        access: &mut A,
        marks: &[ContextMark],
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let mut kept = Vec::new();
        let mut deleted = Vec::new();
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            index += 1;
            for geom in access.solver_geoms(solver)? {
                if !access.geom_has_surface(geom)? {
                    continue;
                }
                // find faces in the original B-reps replaced by the fillet faces
                access.trim_original_surfaces(k, geom, &mut kept, &mut deleted, marks, self)?;
            }
        }
        access.delete_faces(k, &mut self.intersector, &deleted, None)
    }

    // ---- SetFilletBrepEdgeMap -------------------------------------------------------------

    /// `SetFilletBrepEdgeMap`: for every fillet edge of every solver's fillet
    /// geometry (its rails, then its side edges with an original face) and of every
    /// corner (with an original face or edge), finds the fillet B-rep edge of the same
    /// shape (several for a set-back corner).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without fillet B-rep or for a null rail or side edge;
    /// those of `FindFilletBrepEdge` (a side edge's failure is noted first).
    pub fn set_fillet_brep_edge_map<A>(&mut self, k: &mut Kernel, access: &mut A) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        // get edges from FilletBrep (fillet edges already created)
        let fillet = self.fillet.ok_or_else(|| null("no fillet B-rep"))?;
        let edges = access.edges(fillet)?;

        // for every FilletEdgeSolver
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            index += 1;
            for geom in access.solver_geoms(solver)? {
                // set every rail's fillet B-rep edge
                for rail in access.geom_rails(geom)? {
                    let rail = rail.ok_or_else(|| null("null rail"))?;
                    access.find_fillet_brep_edge(k, rail, &edges, false)?;
                }

                // set every non-rail fillet edge's fillet B-rep edge
                for eu in access.geom_side_edgeuses(geom)? {
                    let fe = access.edgeuse_edge(eu)?.ok_or_else(|| null("null side edge"))?;
                    if access.fillet_edge_original_face(fe)?.is_none() {
                        continue;
                    }
                    if let Err(error) = access.find_fillet_brep_edge(k, fe, &edges, false) {
                        if error.is_unported() {
                            return Err(error);
                        }
                        note_fillet_error(
                            self,
                            FILERR_EDGE_PROBLEM,
                            fe,
                            "Unable to map new Fillet Edge back to Edge in Target Brep",
                        )?;
                        return Err(error);
                    }
                }
            }
        }

        // for every FilletCorner
        let mut index = 0;
        while let Some(corner) = self.corners.get(index).copied() {
            index += 1;
            // Setback corner might have split the edge
            let find_multiple_edges = access.corner_setback_distance(corner)? > EFF_ZERO;
            for fe in access.corner_edges(corner)?.into_iter().flatten() {
                if access.fillet_edge_original_face(fe)?.is_some()
                    || access.fillet_edge_original_edge(fe)?.is_some()
                {
                    access.find_fillet_brep_edge(k, fe, &edges, find_multiple_edges)?;
                }
            }
        }
        Ok(())
    }

    // ---- smooth edge sequences ------------------------------------------------------------

    /// `ProcessSmoothEdgeSequences`: collects the sequences of smoothly meeting
    /// filleted edges one at a time and combines each into a single curve, until at
    /// most one solver is left.
    ///
    /// # Errors
    ///
    /// Those of [`FilletExecutive::collect_tangent_edge_sequence`] and
    /// `CreateSmoothEdgeSequence`.
    pub fn process_smooth_edge_sequences<A>(&mut self, k: &mut Kernel, access: &mut A) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let _ = k;
        // Copy the FilletSolver pointers into a separate array.
        let mut solvers: Vec<Option<O>> = self.solvers.iter().copied().map(Some).collect();
        let mut sequence = Vec::new();

        // Each pass removes at least one solver from the list.
        while solvers.len() > 1 {
            self.collect_tangent_edge_sequence(access, &mut solvers, &mut sequence)?;
            create_smooth_edge_sequence(self, &sequence)?;
        }
        Ok(())
    }

    /// `CollectTangentEdgeSequence`: puts one sequence of smoothly meeting fillet
    /// solvers into `sequence`, in order, and removes it (and the solvers found not to
    /// be in a sequence) from `solvers`, which is then compressed. A sequence starts
    /// at the first edge of a law sequence containing one of the solvers' edges, else
    /// it is traversed backwards from the first remaining solver; then it is traversed
    /// forwards through the corners' tangent solvers.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a null list entry, a solver without corner, or no
    /// solver for the law sequence's first edge; [`SolidError::InvalidInput`] when a
    /// tangent chain is longer than the solver list (HANG-36); those of the adapter
    /// calls.
    pub fn collect_tangent_edge_sequence<A>(
        &self,
        access: &A,
        solvers: &mut Vec<Option<O>>,
        sequence: &mut Vec<O>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        sequence.clear();

        if solvers.len() < 2 {
            return Ok(());
        }

        // Check whether these FilletSolvers are in a Law sequence.
        let mut start_edge: Option<O> = None;
        for list in &self.law_sequence_edges {
            for fs in solvers.iter() {
                let fs = fs.ok_or_else(|| null("null fillet solver"))?;
                let this_edge = access.solver_edge(fs)?;
                if list.contains(&this_edge) {
                    start_edge = list.first().copied(); // First in list.
                    break;
                }
            }
            if start_edge.is_some() {
                break;
            }
        }

        // Each tangent walk visits each solver at most once before it closes or ends.
        let bound = solvers.len() + 1;
        let mut idx = 0usize; // `lIdx`: FindElement leaves it unchanged on a miss
        let mut ii = 0;
        while ii < solvers.len() {
            let Some(mut fs) = solvers.get(ii).copied().flatten() else {
                ii += 1;
                continue;
            };
            if let Some(slot) = solvers.get_mut(ii) {
                *slot = None;
            }
            ii += 1;

            // Traverse backwards, unless the start edge is given.
            if start_edge.is_none() {
                let corner0 = access
                    .solver_corner(fs, 0)?
                    .ok_or_else(|| null("solver without corner"))?;

                let mut curr_fs = fs;
                let mut prev_fs = access.corner_tangent_solver(corner0, fs)?;

                if prev_fs == Some(curr_fs) {
                    continue; // Single closed loop.
                }

                // If there is one, put the current FS into the list.
                if prev_fs.is_some() {
                    sequence.push(fs);
                }

                let mut curr_corner = corner0;
                let mut steps = 0;
                while let Some(prev) = prev_fs.filter(|&p| p != fs) {
                    steps += 1;
                    if steps > bound {
                        return Err(invalid("tangent solver chain longer than the solver list"));
                    }
                    // Add it to the list, and remove it from the main list.
                    sequence.insert(0, prev);
                    if let Some(found) = solvers.iter().position(|s| *s == Some(prev)) {
                        idx = found;
                    }
                    if let Some(slot) = solvers.get_mut(idx) {
                        *slot = None;
                    }

                    let prev_corner = access
                        .solver_other_corner(prev, curr_corner)?
                        .ok_or_else(|| null("solver without other corner"))?;
                    curr_fs = prev;
                    prev_fs = access.corner_tangent_solver(prev_corner, curr_fs)?;
                    curr_corner = prev_corner;
                }
            }

            // Traverse forwards.
            if let Some(edge) = start_edge {
                fs = fillet_solver_of_edge(self, edge)?.ok_or_else(|| null("no solver of edge"))?;
            }

            let corner0 = access.solver_corner(fs, 0)?;
            let corner1 = access
                .solver_corner(fs, 1)?
                .ok_or_else(|| null("solver without end corner"))?;

            if Some(corner1) == corner0 {
                continue; // Single closed loop.
            }

            let curr_fs = fs;
            let mut next_fs = access.corner_tangent_solver(corner1, fs)?;
            if next_fs.is_some() && sequence.is_empty() {
                sequence.push(fs);
            }

            if next_fs == Some(curr_fs) {
                continue;
            }

            if next_fs.is_some_and(|next| sequence.contains(&next)) {
                break; // Already got this one: must be a closed loop.
            }

            let mut curr_corner = corner1;
            let mut steps = 0;
            while let Some(next) = next_fs.filter(|&n| n != fs) {
                steps += 1;
                if steps > bound {
                    return Err(invalid("tangent solver chain longer than the solver list"));
                }
                // Add it to the list, and remove it from the main list.
                sequence.push(next);
                if let Some(found) = solvers.iter().position(|s| *s == Some(next)) {
                    idx = found;
                }
                if let Some(slot) = solvers.get_mut(idx) {
                    *slot = None;
                }

                let next_corner = access
                    .solver_other_corner(next, curr_corner)?
                    .ok_or_else(|| null("solver without other corner"))?;
                let curr_fs = next;
                next_fs = access.corner_tangent_solver(next_corner, curr_fs)?;
                curr_corner = next_corner;
            }

            // If we have a sequence, we're done.
            if sequence.len() > 1 {
                break;
            }
        }

        // CompressZeros
        solvers.retain(Option::is_some);
        Ok(())
    }
}

// ---- SurfaceSurfaceFillet -----------------------------------------------------------------

/// The options of `SurfaceSurfaceFillet` after the two surfaces, radii and tolerance.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceFilletOptions {
    /// `lXSectType`: 0 linear, 1 approximated circular, 2 exact circular (default 2).
    pub xsect_type: u32,
    /// `dXSectAccuracy`: relative accuracy of the approximated circle (default 0.1).
    pub xsect_accuracy: f64,
    /// `eFilTrimType` (default `SM_BT_MAXIMAL`).
    pub trim_type: BoundaryTrimmingType,
    /// `lBaseTrimType`: 0 none, 1 normal, 2 reverse trim (default 0).
    pub base_trim_type: u32,
    /// `bMirror` (default `FALSE`).
    pub mirror: bool,
    /// `bComplement` (default `FALSE`).
    pub complement: bool,
}

impl Default for SurfaceFilletOptions {
    /// The header defaults.
    fn default() -> Self {
        Self {
            xsect_type: 2,
            xsect_accuracy: 0.1,
            trim_type: BoundaryTrimmingType::Maximal,
            base_trim_type: 0,
            mirror: false,
            complement: false,
        }
    }
}

/// `SmFilletExecutive::SurfaceSurfaceFillet`: fillets two surfaces with signed radii
/// (the fillet centre lies on the intersection of the two offset surfaces). Each
/// intersection curve of the offsets seeds one surface/surface fillet solver, rolled
/// from near its middle; the fillet surfaces are built into a new B-rep by a new
/// executive. `None` when the offsets do not intersect.
///
/// # Errors
///
/// `SM_ERR` when the intersection curves intersect each other (self-intersecting
/// fillets are not handled); `SM_ERR_NULL_POINTER` for a null UV curve; those of the
/// offset construction, the intersections, the evaluations, the solver set-up and
/// [`FilletExecutive::do_surface_filleting`].
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn surface_surface_fillet<O, A>(
    k: &mut Kernel,
    access: &mut A,
    sur1: &Surface,
    sur2: &Surface,
    fillet_radius1: f64,
    fillet_radius2: f64,
    tolerance: f64,
    options: &SurfaceFilletOptions,
) -> Result<Option<O>>
where
    O: ClassObject + SolutionObject + Ord,
    A: ExecutiveAccess<O> + ?Sized,
{
    // Create temporary offset surfaces
    let off1 = Surface::from(OffsetSurface::new(k, fillet_radius1, sur1.clone(), false, SS_UNKNOWN)?);
    let off2 = Surface::from(OffsetSurface::new(k, fillet_radius2, sur2.clone(), false, SS_UNKNOWN)?);

    // Intersect the offset surfaces. We will use only the two uv curves.
    let use_surface_edges = [true, true];
    let tol = (fillet_radius1.abs() + fillet_radius2.abs()) / 1000.0;
    let sect = off1.global_surface_intersect(
        k,
        &off1.natural_uv_domain(),
        &off2,
        &off2.natural_uv_domain(),
        use_surface_edges,
        Some(tol),
        None,
    )?;
    let uv_curves1 = sect.surface1_uv_curves;
    let uv_curves2 = sect.surface2_uv_curves;

    // no work - no intersection
    if uv_curves1.is_empty() {
        return Ok(None);
    }

    fn uv(list: &[Option<Curve>], i: usize) -> Result<&Curve> {
        list.get(i)
            .and_then(Option::as_ref)
            .ok_or_else(|| null("null intersection UV curve"))
    }

    // When more than one intersection curve was found, check for self-intersections:
    // we don't do those.
    if uv_curves1.len() > 1 {
        for ii in 0..uv_curves1.len() {
            let uv_curve1 = uv(&uv_curves1, ii)?;
            let ivl1 = uv_curve1.natural_interval();
            for jjj in ii + 1..uv_curves1.len() {
                let uv_curve2 = uv(&uv_curves1, jjj)?;
                let ivl2 = uv_curve2.natural_interval();
                let solutions = uv_curve1.global_curve_intersect(
                    k,
                    &ivl1,
                    uv_curve2,
                    &ivl2,
                    tol,
                    &CurveIntersectOptions::<O>::default(),
                )?;
                // Self-intersecting fillets are not yet handled
                if !solutions.is_empty() {
                    return Err(status(SM_ERR));
                }
            }
        }
    }

    // Got the offset intersection. Create the fillet executive object.
    let mut exec: FilletExecutive<O> = new_executive()?;

    // The fillet surface generator for the cross section (on the stack upstream):
    // 0: linear, 1: approximated circular, 2: exact (rational) circular. All of the
    // user options apply only to the circular one.
    let generator = if options.xsect_type == 0 {
        access.new_surface_generator(k, SurfaceGeneratorSpec::Linear)?
    } else {
        access.new_surface_generator(
            k,
            SurfaceGeneratorSpec::Circular {
                approximate_arcs: options.xsect_type == 1,
                tol_percent_radius: options.xsect_accuracy,
                mirror: options.mirror,
                complement: options.complement,
            },
        )?
    };

    // Info for trimming the base surfaces:
    let reverse_trim = options.base_trim_type == 2;

    // Each curve produced during intersection may produce a separate fillet surface.
    for kk in 0..uv_curves1.len() {
        // The UV parameters near the mid points of the intersection curves seed the
        // surface/surface fillet solver.
        let uv_curve1 = uv(&uv_curves1, kk)?;
        let ivl1 = uv_curve1.natural_interval();
        let uv_curve2 = uv(&uv_curves2, kk)?;
        let ivl2 = uv_curve2.natural_interval();

        let pnt1 = uv_curve1.evaluate_point(ivl1.evaluate(0.4567))?;
        let pnt2 = uv_curve2.evaluate_point(ivl2.evaluate(0.4567))?;
        let guess1 = Vec2::new(pnt1.x, pnt1.y);
        let guess2 = Vec2::new(pnt2.x, pnt2.y);

        // Create the fillet solver for filleting two surfaces.
        let orient1 = fillet_radius1 >= 0.0;
        let orient2 = fillet_radius2 >= 0.0;

        // A couple of default tolerances.
        let ang_tol = deg_to_rad(30.0);
        let tan_tol = deg_to_rad(1.0);

        let fs = access.new_fillet_solver(
            k,
            &SolverSpec::SurfaceSurface {
                tolerance,
                ang_tol,
                tan_tol,
                radii: [fillet_radius1.abs(), fillet_radius2.abs()],
                surfaces: [sur1, sur2],
                orientations: [orient1, orient2],
                guesses: [guess1, guess2],
                trim_type: options.trim_type,
                reverse_trim,
            },
        )?;

        // Attach the fillet surface generator to the fillet solver
        access.set_surface_generator(fs, generator)?;

        // Load the fillet solver into the fillet executive
        load_fillet_solver(&mut exec, fs)?;
    }

    // The following allows us to turn off trimming of the original fillet geometry.
    if options.base_trim_type == 0 {
        exec.options.trimming = false; // SetDoTrimming(FALSE)
    }

    exec.do_surface_filleting(k, access, None)?;

    // Get the resulting Brep that contains the fillet surfaces.
    Ok(exec.fillet)
}

// ---- SmTemporaryRadiusChange --------------------------------------------------------------

/// `SmTemporaryRadiusChange`: while it lives, a variable-radius solver's linear or
/// B-spline law is constant at its value at the end where the side edge meets the
/// filleted edge, so that intersecting the rail with the side edge sees the end
/// radius. [`TemporaryRadiusChange::revert`] puts the law back.
#[derive(Debug, Clone)]
pub struct TemporaryRadiusChange<O> {
    /// `m_pSavedBSp`: the B-spline law's curve, while a constant one is swapped in.
    saved_bsp: Option<BSplineCurve>,
    /// `m_dStartRad`: the linear law's start radius, while it is constant.
    start_rad: f64,
    /// `m_dEndRad`: the linear law's end radius, while it is constant.
    end_rad: f64,
    /// `m_pSideEU`: the edge use of the other filleted edge.
    side_eu: O,
    /// `m_pFS`: this fillet solver.
    fs: O,
    /// `m_lRailIdx`.
    rail_idx: usize,
}

impl<O: ClassObject + SolutionObject + Ord> TemporaryRadiusChange<O> {
    /// The constructor: records the solver, rail and side edge use and runs
    /// [`TemporaryRadiusChange::set_up`], whose status it ignores.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the set-up.
    pub fn new<A>(access: &mut A, fs: O, rail_index: usize, side_eu: O) -> Result<Self>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let mut change = Self {
            saved_bsp: None,
            start_rad: 0.0,
            end_rad: 0.0,
            side_eu,
            fs,
            rail_idx: rail_index,
        };
        ignored(change.set_up(access))?;
        Ok(change)
    }

    /// `SetUp`: for a variable-radius solver whose offset radii can change and whose
    /// law is linear or B-spline, finds the end of the edge the side edge meets (the
    /// start for a closed edge; flipped for a reversed law) and, when the law's edge
    /// map ends there, makes the law constant at its radius there.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a missing edge use or owning edge; those of the law
    /// evaluation (`SmFilletLaw::Evaluate`, unported) and the constant law curve.
    pub fn set_up<A>(&mut self, access: &mut A) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        // Indicate no change, to start.
        self.saved_bsp = None;
        self.start_rad = -1.0;
        self.end_rad = -1.0;

        let Some(vrfs) = access.variable_radius_solver(self.fs)? else {
            return Ok(());
        };
        if !vrfs.offset_radii_can_change()? {
            return Ok(());
        }
        let law = vrfs.fillet_law();

        // Do SmBSplineFilletLaw and SmLinearFilletLaw.
        let is_bspline = matches!(law.kind, FilletLawKind::BSpline { .. });
        if !is_bspline && !matches!(law.kind, FilletLawKind::Linear { .. }) {
            return Ok(());
        }
        let law_orientation = vrfs.fillet_law_orientation();
        let edge_map = law.edge_map;
        let edge_ivl = vrfs.fillet_law_interval();

        // Find which end (an SmBoolean that may stay UNSURE).
        let mut at_start = crate::topology::brep_data::UNSURE;
        let eu = access
            .solver_edgeuse(self.fs, self.rail_idx)?
            .ok_or_else(|| null("no rail edge use"))?;
        let e = access.edgeuse_edge(eu)?.ok_or_else(|| null("edge use without edge"))?;
        let (edge_v1, edge_v2) = access.edge_vertices(e)?;

        let side_edge = access
            .edgeuse_edge(self.side_eu)?
            .ok_or_else(|| null("side edge use without edge"))?;
        let (side_v1, side_v2) = access.edge_vertices(side_edge)?;

        if edge_v1 != edge_v2 {
            // non-closed edge
            if edge_v1 == side_v1 || edge_v1 == side_v2 {
                at_start = 1;
            }
            if edge_v2 == side_v1 || edge_v2 == side_v2 {
                at_start = 0;
            }
        } else {
            // Shouldn't matter: the fillet is closed.
            at_start = 1;
        }

        if law_orientation {
            at_start = i32::from(at_start == 0);
        }

        // Don't do anything if this is an interior location.  [B570]
        let law_t = if at_start != 0 {
            edge_map.min()
        } else {
            edge_map.max()
        };
        let unit_ivl = Extent1d::from_min_max(0.0, 1.0);
        if !unit_ivl.is_value_on_boundary(law_t, EFF_ZERO).0 {
            return Ok(());
        }

        // Don't do anything if this has no range
        if edge_ivl.length() <= 0.0 {
            return Ok(());
        }

        let edge_t = if at_start != 0 {
            edge_ivl.min()
        } else {
            edge_ivl.max()
        };

        // Evaluate the radius at the proper end (no reversal: already checked).
        let law = access
            .variable_radius_solver(self.fs)?
            .ok_or_else(|| null("solver is no longer variable-radius"))?
            .fillet_law();
        let values = law_evaluate(law, edge_t, &edge_ivl, false)?;
        let [end_rad, _, _] = values;

        if is_bspline {
            // Create a law curve that's constant at that value, on [0, 1].
            let mut const_bsp = BSplineCurve::create_line_segment(
                3,
                Point3::new(end_rad, 1.0, 0.0),
                Point3::new(end_rad, 2.0, 0.0),
            )?;
            ignored(const_bsp.edit_parameterization(&unit_ivl))?;

            let vrfs = access
                .variable_radius_solver_mut(self.fs)?
                .ok_or_else(|| null("solver is no longer variable-radius"))?;
            if let FilletLawKind::BSpline { law_curve } = &mut vrfs.law.kind {
                // Swap the new radius function info in.
                self.saved_bsp = Some(core::mem::replace(law_curve, const_bsp));
            }
            vrfs.law.extended_law = None;
        } else {
            let vrfs = access
                .variable_radius_solver_mut(self.fs)?
                .ok_or_else(|| null("solver is no longer variable-radius"))?;
            if let FilletLawKind::Linear {
                start_radius,
                end_radius,
            } = &mut vrfs.law.kind
            {
                self.start_rad = *start_radius;
                self.end_rad = *end_radius;
                *start_radius = end_rad;
                *end_radius = end_rad;
            }
        }
        Ok(())
    }

    /// The destructor (`Revert`): swaps the saved B-spline law curve back in (and
    /// drops the constant one and any extended law), or restores the linear law's
    /// radii when both were recorded (`> -SM_EFF_ZERO`); otherwise nothing changed.
    ///
    /// # Errors
    ///
    /// Those of the adapter's solver lookup.
    pub fn revert<A>(mut self, access: &mut A) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        self.revert_change(access)
    }

    fn revert_change<A>(&mut self, access: &mut A) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let Some(vrfs) = access.variable_radius_solver_mut(self.fs)? else {
            return Ok(());
        };
        if let Some(saved) = self.saved_bsp.take() {
            let law = &mut vrfs.law;
            let FilletLawKind::BSpline { law_curve } = &mut law.kind else {
                return Ok(());
            };
            *law_curve = saved;
            law.extended_law = None;
        } else if self.start_rad > -EFF_ZERO && self.end_rad > -EFF_ZERO {
            // 0.0 is ok...
            let FilletLawKind::Linear {
                start_radius,
                end_radius,
            } = &mut vrfs.law.kind
            else {
                return Ok(());
            };
            *start_radius = self.start_rad;
            *end_radius = self.end_rad;
        }
        Ok(())
    }
}

/// Runs `body` while a [`TemporaryRadiusChange`] of `fs` is in place, reverting it
/// afterwards, also when `body` fails (the C++ scope's destructor).
///
/// # Errors
///
/// Those of the change's construction and of `body`; a failed revert is reported only
/// when `body` succeeded.
pub fn with_temporary_radius_change<O, A, T>(
    access: &mut A,
    fs: O,
    rail_index: usize,
    side_eu: O,
    body: impl FnOnce(&mut A) -> Result<T>,
) -> Result<T>
where
    O: ClassObject + SolutionObject + Ord,
    A: ExecutiveAccess<O> + ?Sized,
{
    let change = TemporaryRadiusChange::new(access, fs, rail_index, side_eu)?;
    let result = body(access);
    let reverted = change.revert(access);
    match result {
        Ok(value) => reverted.map(|()| value),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_law_edge_map_defaults() {
        let law = FilletLaw::linear(1.0, 2.0, None);
        assert_eq!((law.edge_map.min(), law.edge_map.max()), (0.0, 1.0));
        let map = Extent1d::from_min_max(0.25, 0.75);
        let law = FilletLaw::linear(1.0, 2.0, Some(&map));
        assert_eq!((law.edge_map.min(), law.edge_map.max()), (0.25, 0.75));
        let short = Extent1d::from_min_max(0.5, 0.5);
        let law = FilletLaw::linear(1.0, 2.0, Some(&short));
        assert_eq!((law.edge_map.min(), law.edge_map.max()), (0.0, 1.0));
    }

    #[test]
    fn elliptical_law_is_quadratic() {
        let law = FilletLaw::bspline_elliptical(1.0, 2.0, 3.0, PI / 2.0, 0.0, 0.0, None).unwrap();
        let FilletLawKind::BSpline { law_curve } = &law.kind else {
            panic!("not a B-spline law");
        };
        assert_eq!(law_curve.degree().unwrap(), 2);
    }

    #[test]
    fn single_list_classification() {
        assert_eq!(single(1, 3), Some(true));
        assert_eq!(single(3, 3), Some(false));
        assert_eq!(single(2, 3), None);
        assert_eq!(single(1, 1), Some(true));
    }
}
