//! Part 1 of `SmFilletExecutive`: the fillet laws (the radius and its derivatives
//! along a filleted edge, constant, linear or given by a B-spline), the error info and
//! the executive object.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp` (lines 30-292, 488-523,
//! 1601-1814, 4909-4929, 5126-6132), `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! The law type itself ([`FilletLaw`], [`FilletLawKind`]) was CO-PORTED by unit
//! `blend.fillet_standard_solver_p1` into
//! [`crate::blend::fillet_standard_solver::p1`]; it is extended there (edge map, arc
//! length, cached extension) and re-exported here. This module holds its behaviour.
//!
//! | Rust                                   | Upstream                                              |
//! |----------------------------------------|-------------------------------------------------------|
//! | [`FilletLaw::new`] (co-port)           | `SmFilletLaw::SmFilletLaw`, `~SmFilletLaw`, `~SmBSplineFilletLaw` (`Drop`) |
//! | [`FilletLaw::evaluate`]                | `SmFilletLaw::Evaluate`, `SmLinearFilletLaw::Evaluate`, `SmBSplineFilletLaw::Evaluate` |
//! | [`FilletLaw::is_linear_law`] (co-port) | `SmFilletLaw::IsLinearLaw`, `SmLinearFilletLaw::IsLinearLaw`, `SmBSplineFilletLaw::IsLinearLaw` |
//! | [`FilletLaw::law_curve`]               | `SmBSplineFilletLaw::GetLawCurve`                     |
//! | [`FilletLaw::extended_curve`]          | `SmBSplineFilletLaw::GetExtendedCurve`                |
//! | field `FilletLawKind::BSpline::law_curve` / `extended_law` | `SetLawCurve`, `SetExtendedCurve`     |
//! | field `FilletLawKind::Linear::start_radius` / `end_radius` | `Get/SetStartRadius`, `Get/SetEndRadius` |
//! | field `FilletLaw::edge_arc_length`     | `SmFilletLaw::SetEdgeArcLength`                       |
//! | [`FilletLaw::extended_edge_curve`], [`FilletLaw::set_extended_edge_curve`] (co-port) | `GetExtendedEdgeCurve`, `SetExtendedEdgeCurve` |
//! | [`FilletLaw::type_code`], [`FilletLaw::type_string`] | `GetType`, `GetTypeString` of the three classes |
//! | [`FilletLaw::is_kind_of`]              | `IsKindOf` of the three classes                       |
//! | [`FILLET_LAW_TYPE`], [`LINEAR_FILLET_LAW_TYPE`], [`BSPLINE_FILLET_LAW_TYPE`] | `SmFilletLaw_TYPE`, `SmLinearFilletLaw_TYPE`, `SmBSplineFilletLaw_TYPE` |
//!
//! The `SmLinearFilletLaw` and `SmBSplineFilletLaw` constructors are unit
//! `blend.fillet_executive_p2`'s.
//!
//! The executive's member data ([`FilletExecutive`], [`ErrorInfo`]) was CO-PORTED by
//! unit `blend.fillet_executive_p3` and is extended there; this module holds the
//! constructor and the members of this part.
//!
//! | Rust                                   | Upstream                                              |
//! |----------------------------------------|-------------------------------------------------------|
//! | [`ErrorInfo::new`], `ErrorInfo::default` (co-port) | `SmFilletErrorInfo(pFilletExec)`, `SmFilletErrorInfo()`, `set()` |
//! | [`ErrorInfo::note_error`], [`ErrorInfo::note_edge_error`], [`ErrorInfo::note_merge_error`], [`ErrorInfo::note_corner_error`], [`ErrorInfo::note_fillet_edge_error`], [`ErrorInfo::note_intersection_error`] | the six `SmFilletErrorInfo::NoteError` overloads (cpp:117, 150, 173, 199, 231, 256) |
//! | `ErrorInfo::assign` (co-port)          | `SmFilletErrorInfo::operator=`                        |
//! | [`FilletExecutive::new`], [`ExecutiveObjects`] | `SmFilletExecutive::SmFilletExecutive`        |
//! | [`FilletExecutive::load_fillet_solver`] | `LoadFilletSolver` |
//! | [`FilletExecutive::add_fillet_surface_generator`] | `AddFilletSurfaceGenerator`                |
//! | [`FilletExecutive::remove_from_topo_edges`] | `RemoveFromTopoEdges`                            |
//! | [`FilletExecutive::set_do_global_merge`], [`FilletExecutive::set_do_trimming`], [`FilletExecutive::set_self_intersection_handler`] | `SetDoGlobalMerge`, `SetDoTrimming`, `SetSelfIntersectionHandler` |
//! | fields `fillet`, `pseudo_brep`, `corners`, `solvers`, `error` | `GetFilletBrep`, `GetPseudoBrep`, `GetFilletCorners`, `GetFilletSolvers`, `GetFilletErrorInfo` |
//! | [`FilletExecutive::fillet_corner_of_vertex`] | `GetFilletCornerOfVertex`                       |
//! | [`FilletExecutive::fillet_solver_of_edgeuse`], [`FilletExecutive::fillet_solver_of_edge`] | `GetFilletSolverOfEdgeuse`, `GetFilletSolverOfEdge` |
//! | [`FilletExecutive::note_fillet_error`], [`FilletExecutive::note_fillet_merge_error`], [`FilletExecutive::note_fillet_corner_error`], [`FilletExecutive::note_fillet_intersection_error`] | the four `NoteFilletError` overloads |
//!
//! Deviations:
//! - The class hierarchy is the enum [`FilletLawKind`]; the trivial getters and setters
//!   are public fields.
//! - `Evaluate` is `const` upstream and caches the B-spline extension through a cast
//!   away from `const`; here [`FilletLaw::evaluate`] takes `&mut self` and stores it in
//!   `FilletLawKind::BSpline::extended_law`. The context passed to
//!   `CreateExtendedCurve` only allocates, so no `Kernel` is needed.
//! - The "unable to extend non-B-spline laws" branch is unreachable (both the law curve
//!   and its extension are B-splines) and not ported.
//! - A G1-closed law curve is not extended: `CreateExtendedCurve` hands back an
//!   unextended copy (NLib evaluates such a curve periodically) and the evaluation
//!   clamps, as upstream. `CreateExtendedCurve` returning no curve at all (its
//!   `Ok(None)`: the parameter lies inside the NURB's own bounds, possible only if they
//!   disagreed with `natural_interval`) is a `NULL` dereference upstream and
//!   `SM_ERR_NULL_POINTER` ([`SolidError::null_pointer`]) here.
//! - `GetClassType` / `GetClassTypeString` (the `SM_COMMON` statics) are not ported.
//! - The out-parameter `dValues[3]` is the returned `[f64; 3]`.
//! - Error info: the overloads of `NoteError` and `NoteFilletError` have their own
//!   names. Comments and names are `Option<&str>` (null upstream is `None`); a null one
//!   is printed as glibc's `(null)` (`"%s"` with a null pointer is undefined in C) and
//!   text is cut to 1023 bytes as `snprintf` into the `SM_TBLOCK_SIZE` buffers does, at
//!   a character boundary (upstream may cut inside a UTF-8 sequence; that case is not
//!   compared). The vertex overload of `NoteError` (cpp:131) reads the vertex's number
//!   but stores nothing, and is not ported. The codes are typed
//!   ([`FilletErrorType`], [`FilStatus`], [`FilletEdgeType`]) and stored as their
//!   upstream values. `NoteFilletError` on a fillet edge whose type `GetName` writes
//!   no name for is [`SolidError::InvalidInput`]: upstream prints an uninitialised
//!   buffer.
//! - The executive's objects (pseudo B-rep, the fillet-edges attribute, its own
//!   identity) are handles the owner allocates ([`ExecutiveObjects`]). The constructor
//!   leaves `m_bIsInitialized` unset; it is `false` here, the value the first upstream
//!   write (`SetFilletParameters`) gives it. A null surface generator is not
//!   representable (no upstream caller adds one). `GetContext` is
//!   not ported (no context object). The destructor (deleting the owned objects) and
//!   `SetBevelCorner` wait for the owner's object model.
//! - No recursion.

use std::collections::BTreeMap;

use crate::blend::fillet_executive::p3::{
    ErrorInfo, ExecutionOptions, ExecutiveAccess, FilletExecutive,
};
use crate::blend::fillet_geom::p1::{FilStatus, FilletCornerType, FilletEdgeType, FilletErrorType};
pub use crate::blend::fillet_standard_solver::p1::{FilletLaw, FilletLawKind};
use crate::boolean::topology_intersector::p1::TopologyIntersector;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::object;
use crate::geom::solution_array::SolutionObject;
use crate::intersect::point_class::ClassObject;
use crate::kernel::Kernel;
use crate::math::{Point3, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_curve::BSplineCurve;

const OP_EVAL: &str = "blend::fillet_executive::p1::FilletLaw::evaluate";
const OP_NOTE: &str = "blend::fillet_executive::p1::FilletExecutive::note_fillet_error";

/// `TOPO_BASE_TYPE`.
const TOPO_BASE_TYPE: i64 = 16000;
/// `SmFilletLaw_TYPE` (`TOPO_BASE_TYPE + 39`).
pub const FILLET_LAW_TYPE: i64 = TOPO_BASE_TYPE + 39;
/// `SmLinearFilletLaw_TYPE` (`TOPO_BASE_TYPE + 40`).
pub const LINEAR_FILLET_LAW_TYPE: i64 = TOPO_BASE_TYPE + 40;
/// `SmBSplineFilletLaw_TYPE` (`TOPO_BASE_TYPE + 41`).
pub const BSPLINE_FILLET_LAW_TYPE: i64 = TOPO_BASE_TYPE + 41;

impl FilletLaw {
    /// `GetLawCurve`: the B-spline radius function (`None` for the other kinds).
    #[must_use]
    pub const fn law_curve(&self) -> Option<&BSplineCurve> {
        match &self.kind {
            FilletLawKind::BSpline { law_curve, .. } => Some(law_curve),
            _ => None,
        }
    }

    /// `GetExtendedCurve`: the extension of the law curve the last out-of-domain
    /// evaluation created (`None` before that, and for the other kinds).
    #[must_use]
    pub const fn extended_curve(&self) -> Option<&BSplineCurve> {
        match &self.kind {
            FilletLawKind::BSpline { extended_law, .. } => extended_law.as_ref(),
            _ => None,
        }
    }

    /// `GetType`: the run-time type code of the law's class.
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        match &self.kind {
            FilletLawKind::Base => FILLET_LAW_TYPE,
            FilletLawKind::Linear { .. } => LINEAR_FILLET_LAW_TYPE,
            FilletLawKind::BSpline { .. } => BSPLINE_FILLET_LAW_TYPE,
        }
    }

    /// `GetTypeString`.
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        match &self.kind {
            FilletLawKind::Base => "SmFilletLaw_TYPE",
            FilletLawKind::Linear { .. } => "SmLinearFilletLaw_TYPE",
            FilletLawKind::BSpline { .. } => "SmBSplineFilletLaw_TYPE",
        }
    }

    /// `IsKindOf`: the law's own type, `SmFilletLaw_TYPE` or `SmObject_TYPE`.
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        t == self.type_code() || t == FILLET_LAW_TYPE || object::is_kind_of(t)
    }

    /// `Evaluate(dParameter, crCurveInterval, bReverseOrientation, dValues)`: the
    /// fillet radius at `parameter` of the edge interval `interval` and its first two
    /// derivatives with respect to that parameter (`reverse`: the law runs from the
    /// interval's end to its start).
    ///
    /// A linear law evaluates anywhere. A B-spline law maps the parameter through the
    /// edge map onto the law curve; a parameter within `SM_EFF_ZERO * (1 + |min| +
    /// |max|)` outside the curve's domain snaps to the end, a parameter further out
    /// extends the curve (`C-infinity`, to twice the overshoot and at least a tenth of
    /// the domain) and keeps the extension for later evaluations.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for the base law and for a zero-length `interval` of a B-spline law;
    /// the errors of [`BSplineCurve::create_extended_curve`] and
    /// [`BSplineCurve::evaluate`].
    pub fn evaluate(
        &mut self,
        parameter: f64,
        interval: &Extent1d,
        reverse: bool,
    ) -> Result<[f64; 3]> {
        let edge_map = self.edge_map;
        match &mut self.kind {
            FilletLawKind::Base => Err(SolidError::sm_err(OP_EVAL)),
            FilletLawKind::Linear {
                start_radius,
                end_radius,
            } => Ok(linear_evaluate(
                *start_radius,
                *end_radius,
                &edge_map,
                parameter,
                interval,
                reverse,
            )),
            FilletLawKind::BSpline {
                law_curve,
                extended_law,
            } => bspline_evaluate(
                law_curve,
                extended_law,
                &edge_map,
                parameter,
                interval,
                reverse,
            ),
        }
    }
}

/// `SmLinearFilletLaw::Evaluate`.
fn linear_evaluate(
    start_value: f64,
    end_value: f64,
    edge_map: &Extent1d,
    parameter: f64,
    interval: &Extent1d,
    reverse: bool,
) -> [f64; 3] {
    // Map dParameter into domain of Law which is contained in the edge map.
    let d_min = interval.min();
    let d_max = interval.max();
    let t_norm = if reverse {
        (d_max - parameter) / (d_max - d_min)
    } else {
        (parameter - d_min) / (d_max - d_min)
    };
    let t = edge_map.min() + t_norm * (edge_map.max() - edge_map.min());

    // evaluate function, 1st, and 2nd derivatives at param = dParameter
    let value = start_value + t * (end_value - start_value);
    let end_map_value = start_value + edge_map.max() * (end_value - start_value);
    let start_map_value = start_value + edge_map.min() * (end_value - start_value);
    let mut deriv1 = (end_map_value - start_map_value) / interval.length();
    let deriv2 = 0.0;

    // negate 1st derivatives for reversed orientations
    if reverse {
        deriv1 = -deriv1;
    }

    [value, deriv1, deriv2]
}

/// `SmBSplineFilletLaw::Evaluate`.
fn bspline_evaluate(
    law_curve: &BSplineCurve,
    extended_law: &mut Option<BSplineCurve>,
    edge_map: &Extent1d,
    parameter: f64,
    interval: &Extent1d,
    reverse: bool,
) -> Result<[f64; 3]> {
    if interval.length() == 0.0 {
        return Err(SolidError::sm_err(OP_EVAL));
    }

    // select function to evaluate
    let law = extended_law.as_ref().unwrap_or(law_curve);

    // Compute the scale difference between the parameters of the map and the curve.
    let ivl = law.natural_interval();
    let crv_ivl_size = interval.length();
    let map_ivl_size = edge_map.length();
    let param_scale = crv_ivl_size / map_ivl_size;

    // Map parameter to unitized range accounting for bReverseOrientation
    let d_min = interval.min();
    let d_max = interval.max();

    let t = if reverse {
        (d_max - parameter) / (d_max - d_min)
    } else {
        (parameter - d_min) / (d_max - d_min)
    };

    // Map parameter back to interval of the curve.
    let mut law_t = edge_map.evaluate(t);

    // when parameter is beyond current range
    let mut new_extension = None;
    if !ivl.contains_value(law_t, 0.0) {
        // select param space tolerance
        let scaled_tol = EFF_ZERO * (1.0 + ivl.min().abs() + ivl.max().abs());

        // Try to snap to begin point.
        if law_t < ivl.min() && law_t + scaled_tol > ivl.min() {
            law_t = ivl.min();
        }

        // Try to snap to end point.
        if law_t > ivl.max() && law_t - scaled_tol < ivl.max() {
            law_t = ivl.max();
        }

        // when parameter is still beyond current range
        if !ivl.contains_value(law_t, 0.0) {
            // create an extension of the curve to the requested param point
            // But extend farther than requested, so that we won't have to do this too often.
            // This doubles the extension distance:
            let mut ext_param = if law_t < ivl.min() {
                law_t - (ivl.min() - law_t)
            } else {
                law_t + (law_t - ivl.max())
            };
            // But never do a really tiny extension: it might get snapped to the end,
            // and therefor not extended at all.  [Fillet 308]
            let len = ivl.length();
            if ext_param < ivl.min() {
                if ext_param > ivl.min() - len / 10.0 {
                    ext_param = ivl.min() - len / 10.0;
                }
            } else if ext_param < ivl.max() + len / 10.0 {
                ext_param = ivl.max() + len / 10.0;
            }

            // precise extension (otherwise it adds another length of sIvl);
            // C-infinity works better than G1_G2.  [B551]
            new_extension =
                Some(law.create_extended_curve(ext_param, ContinuityType::CInfinity, true)?);
        } // end need to extend fillet-radius function check after snapping to endPoints
    } // end need to extend fillet-radius function check

    if let Some(extension) = new_extension {
        // save and use the extended curve (the previous one is dropped)
        *extended_law = extension;
        if extended_law.is_none() {
            // create_extended_curve found the parameter inside the NURB's own bounds
            // (Ok(None)), which needs natural_interval and those bounds to disagree;
            // upstream would dereference the NULL curve here
            return Err(SolidError::null_pointer(OP_EVAL));
        }
    }
    let law = extended_law.as_ref().unwrap_or(law_curve);

    let pv = law.evaluate(law_t, 2, true, true)?;
    let x = |i: usize| {
        pv.get(i)
            .map(|v| v.x)
            .ok_or_else(|| SolidError::sm_err(OP_EVAL))
    };

    let value = x(0)?;
    let mut deriv1 = x(1)? / param_scale;
    if reverse {
        deriv1 = -deriv1;
    }
    let deriv2 = x(2)? / (param_scale * param_scale);

    Ok([value, deriv1, deriv2])
}

// ---- SmFilletErrorInfo ---------------------------------------------------------------------

/// `SM_TBLOCK_SIZE`: the size of the error-info text buffers.
const TBLOCK_SIZE: usize = 1024;

/// `smos_sprintf(buffer, "%s", text)`: `snprintf` into an `SM_TBLOCK_SIZE` buffer keeps
/// at most 1023 bytes (cut back to a character boundary here); a null `text` prints
/// glibc's `(null)`.
fn tblock_text(text: Option<&str>) -> String {
    let text = text.unwrap_or("(null)");
    let mut end = text.len().min(TBLOCK_SIZE - 1);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

/// `SmTArray::AddUnique`.
fn add_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

impl<O: Copy + PartialEq> ErrorInfo<O> {
    /// `SmFilletErrorInfo(pFilletExec)`: the `set()` defaults with the executive.
    #[must_use]
    pub fn new(executive: Option<O>) -> Self {
        Self {
            executive,
            ..Self::default()
        }
    }

    /// `NoteError(eErrorCode, cComment)`: a generic error; a null comment keeps the old
    /// one.
    pub fn note_error(&mut self, code: FilletErrorType, comment: Option<&str>) {
        self.code = code as i32;
        if comment.is_some() {
            self.comment = tblock_text(comment);
        }
    }

    /// `NoteError(pBadEdge, eStatus, eErrorCode, cComment)`: an error at a filleted edge;
    /// nothing for a null edge.
    pub fn note_edge_error(
        &mut self,
        edge: Option<O>,
        status: FilStatus,
        code: FilletErrorType,
        comment: Option<&str>,
    ) {
        if edge.is_none() {
            return;
        }
        self.edge = edge;
        self.code = code as i32;
        self.status = status as i32;
        self.comment = tblock_text(comment);
    }

    /// `NoteError(eErrorCode, eEdgeStatus, pFilletedEdge, pSideFace, pFilletEdge,
    /// pFilletSurface, cComment)`: merging a fillet edge into a target face failed.
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_merge_error(
        &mut self,
        code: FilletErrorType,
        status: FilStatus,
        filleted_edge: Option<O>,
        side_face: Option<O>,
        fillet_edge: Option<O>,
        surface: Option<O>,
        comment: Option<&str>,
    ) {
        self.code = code as i32;
        self.status = status as i32;
        self.edge = filleted_edge;
        self.side_face = side_face;
        add_unique(&mut self.edges, fillet_edge);
        add_unique(&mut self.surfaces, surface);
        self.comment = tblock_text(comment);
    }

    /// `NoteError(eErrorCode, eCornerType, eFilletStatus, cCornerName, pFilletedVertex,
    /// pFilletSurface1, pFilletSurface2, pStartFV, pEndFV, cComment)`: an error at a
    /// fillet corner (two fillet surfaces failed to intersect, for one).
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_corner_error(
        &mut self,
        code: FilletErrorType,
        corner_type: FilletCornerType,
        status: FilStatus,
        corner_name: Option<&str>,
        vertex: Option<O>,
        surfaces: [Option<O>; 2],
        fillet_vertices: [Option<O>; 2],
        comment: Option<&str>,
    ) {
        self.code = code as i32;
        self.status = status as i32;
        self.corner_type = corner_type;
        self.vertex = vertex;
        let [surface1, surface2] = surfaces;
        add_unique(&mut self.surfaces, surface1);
        add_unique(&mut self.surfaces, surface2);
        self.fillet_vertices = fillet_vertices;
        self.comment = tblock_text(comment);
        self.entity_name = tblock_text(corner_name);
    }

    /// `NoteError(eErrorCode, eFilletStat, eFEType, pFilletEdge, cEdgeName, cComment)`:
    /// an error at a fillet edge (of the fillet B-rep, not the target).
    pub fn note_fillet_edge_error(
        &mut self,
        code: FilletErrorType,
        status: FilStatus,
        edge_type: FilletEdgeType,
        fillet_edge: Option<O>,
        edge_name: Option<&str>,
        comment: Option<&str>,
    ) {
        self.code = code as i32;
        self.status = status as i32;
        self.edge_type = edge_type as i32;
        add_unique(&mut self.edges, fillet_edge);
        self.comment = tblock_text(comment);
        self.entity_name = tblock_text(edge_name);
    }

    /// `NoteError(eErrorCode, eFilletStatus, pFilletedEdge, rapFilSurfs, rapFilCurvs,
    /// rapFilEdges, rTsectPos, cComment)`: an error along a filleted edge (the base
    /// surfaces failed to intersect, for one).
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_intersection_error(
        &mut self,
        code: FilletErrorType,
        status: FilStatus,
        filleted_edge: Option<O>,
        surfaces: &[Option<O>],
        curves: &[Option<O>],
        fillet_edges: &[Option<O>],
        location: Point3,
        comment: Option<&str>,
    ) {
        self.code = code as i32;
        self.status = status as i32;
        self.edge_type = FilletEdgeType::Unknown as i32; // (it's not a fillet edge)
        self.edge = filleted_edge;
        for &surface in surfaces {
            add_unique(&mut self.surfaces, surface);
        }
        for &curve in curves {
            add_unique(&mut self.curves, curve);
        }
        for &edge in fillet_edges {
            add_unique(&mut self.edges, edge);
        }
        self.location = Some(location);
        self.comment = tblock_text(comment);
    }
}

// ---- SmFilletExecutive: the object ---------------------------------------------------------

/// The objects `SmFilletExecutive(crContext, pTargetBrep)` allocates, made by the owner of
/// the handles ([`ExecutiveAccess::new_executive_objects`]).
#[derive(Debug, Clone, Copy)]
pub struct ExecutiveObjects<O> {
    /// `this`, as `m_sErrorInfo.m_pFilExec` records it.
    pub executive: O,
    /// `m_pPseudoBrep`: a new `SmFilletBrep` with editing enabled.
    pub pseudo_brep: O,
    /// `m_pAttributeE`: a new `SmAttribute(SM_AI_ALL_FILLET_EDGES,
    /// SM_AB_STANDALONE_REFERENCE)`.
    pub edge_attribute: O,
}

impl<O: ClassObject + SolutionObject + Ord> FilletExecutive<O> {
    /// `SmFilletExecutive(crContext, pTargetBrep)`: no fillet B-rep, empty lists and
    /// maps, the upstream switch defaults, the `SmMerge(crContext)` intersector
    /// (`SM_EFF_ZERO_SQ`, 30 degrees) ignoring tangency, and the error info pointing
    /// back at the executive.
    #[must_use]
    pub fn new(target: Option<O>, objects: ExecutiveObjects<O>) -> Self {
        let mut intersector =
            TopologyIntersector::new(None, None, EFF_ZERO_SQ, 30.0 * (PI / 180.0));
        intersector.ignore_tangency = true;
        Self {
            target,
            fillet: None,
            corners: Vec::new(),
            solvers: Vec::new(),
            topo_edges: Vec::new(),
            edge_attribute: Some(objects.edge_attribute),
            initialized: false,
            options: ExecutionOptions::default(),
            error: ErrorInfo::new(Some(objects.executive)),
            intersector,
            intersection_edges: Vec::new(),
            intersection_vertices: Vec::new(),
            law_sequence_edges: Vec::new(),
            pseudo_brep: Some(objects.pseudo_brep),
            surface_generators: Vec::new(),
            filleted_vertices: Vec::new(),
            vertex_corner_map: BTreeMap::new(),
            edgeuse_solver_map: BTreeMap::new(),
            self_intersection_handler: None,
        }
    }

    /// `LoadFilletSolver`: retain the solver, its actual executive binding and the
    /// incident edge uses of its vertices, preserving native insertion order.
    ///
    /// # Errors
    /// Missing offset surfaces, unresolved caller objects, or a native undefined
    /// vertex-array read when an absent rail reaches an existing vertex entry.
    /// Earlier native mutations remain visible on a later error.
    pub fn load_fillet_solver<A: ExecutiveAccess<O> + ?Sized>(
        &mut self,
        k: &Kernel,
        access: &mut A,
        solver: O,
    ) -> Result<()> {
        const OP: &str = "blend::fillet_executive::p1::load_fillet_solver";
        for rail in 0..2 {
            if !access.solver_has_surface(solver, rail)? {
                return Err(SolidError::null_pointer(OP));
            }
        }
        if let Some(edgeuse) = access.solver_edgeuse(solver, 0)? {
            let edge = access
                .edgeuse_edge(edgeuse)?
                .ok_or_else(|| SolidError::invalid_input(OP, "edge use without edge"))?;
            for knot in access.edge_g1_knots(k, edge)? {
                access.solver_base_mut(solver)?.g1_knots.push(knot);
            }
        }
        self.solvers.push(solver);
        access
            .solver_base_mut(solver)?
            .set_fillet_executive(Some(crate::blend::fillet_solver::p1::FilletExecutiveRef));
        let mut edgeuses = [None; 2];
        // Outer None is an unwritten native pVerts entry; inner None is a null vertex.
        let mut vertices = [None; 2];
        for (rail, (edgeuse_slot, vertex_slot)) in
            edgeuses.iter_mut().zip(vertices.iter_mut()).enumerate()
        {
            let edgeuse = access.solver_edgeuse(solver, rail)?;
            if let Some(edgeuse) = edgeuse {
                self.edgeuse_solver_map.insert(edgeuse, solver);
                *vertex_slot = Some(access.edgeuse_vertex(edgeuse)?);
            }
            *edgeuse_slot = edgeuse;
        }
        let first_edgeuse_present = edgeuses.first().is_some_and(Option::is_some);
        for (edgeuse, vertex_entry) in edgeuses.into_iter().zip(vertices) {
            let mut found = false;
            for &vertex in &self.filleted_vertices {
                let current = vertex_entry.ok_or_else(|| {
                    SolidError::invalid_input(
                        OP,
                        "native pVerts entry is uninitialized (absent solver rail)",
                    )
                })?;
                if current == vertex {
                    found = true;
                    let entry = self
                        .vertex_corner_map
                        .get_mut(&vertex)
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let super::p3::VertexEntry::SolverEdgeuses(pending) = entry else {
                        return Err(SolidError::invalid_input(
                            OP,
                            "native pending list has already been replaced by a corner",
                        ));
                    };
                    let edgeuse = edgeuse.ok_or_else(|| {
                        SolidError::invalid_input(
                            OP,
                            "native pending array would read an absent edge use",
                        )
                    })?;
                    let edge = access.edgeuse_edge(edgeuse)?;
                    let mut coincident = false;
                    for &pending_edgeuse in pending.iter() {
                        if edge == access.edgeuse_edge(pending_edgeuse)? {
                            coincident = true;
                            break;
                        }
                    }
                    if !coincident {
                        pending.push(edgeuse);
                    }
                    break;
                }
            }
            if !found && first_edgeuse_present {
                let vertex = vertex_entry.ok_or_else(|| {
                    SolidError::invalid_input(
                        OP,
                        "native pVerts entry is uninitialized (absent solver rail)",
                    )
                })?;
                let edgeuse = edgeuse.ok_or_else(|| {
                    SolidError::invalid_input(
                        OP,
                        "native pending array would store an absent edge use",
                    )
                })?;
                self.filleted_vertices.push(vertex);
                self.vertex_corner_map.insert(
                    vertex,
                    super::p3::VertexEntry::SolverEdgeuses(vec![edgeuse]),
                );
            }
        }
        Ok(())
    }

    /// `AddFilletSurfaceGenerator(pFSG)`: the executive owns the generator from now on.
    pub fn add_fillet_surface_generator(&mut self, generator: O) {
        self.surface_generators.push(generator);
    }

    /// `RemoveFromTopoEdges(pTopoEdge)`: drop the first entry equal to `topo_edge`.
    pub fn remove_from_topo_edges(&mut self, topo_edge: Option<O>) {
        if let Some(index) = self.topo_edges.iter().position(|&e| e == topo_edge) {
            self.topo_edges.remove(index);
        }
    }

    /// `SetDoGlobalMerge`.
    pub fn set_do_global_merge(&mut self, value: bool) {
        self.options.global_merge = value;
    }

    /// `SetDoTrimming`.
    pub fn set_do_trimming(&mut self, value: bool) {
        self.options.trimming = value;
    }

    /// `SetSelfIntersectionHandler`.
    pub fn set_self_intersection_handler(&mut self, handler: Option<O>) {
        self.self_intersection_handler = handler;
    }

    /// `GetFilletCornerOfVertex(pVertex)`: the corner at `vertex`, `None` when there is
    /// none.
    ///
    /// # Errors
    /// A pending solver list is not a completed corner (native UB-209).
    pub fn fillet_corner_of_vertex(&self, vertex: O) -> Result<Option<O>> {
        match self.vertex_corner_map.get(&Some(vertex)) {
            None => Ok(None),
            Some(super::p3::VertexEntry::Corner(corner)) => Ok(Some(*corner)),
            Some(super::p3::VertexEntry::SolverEdgeuses(_)) => Err(SolidError::invalid_input(
                "blend::fillet_executive::p1::fillet_corner_of_vertex",
                "vertex still contains a pending solver list",
            )),
        }
    }

    /// `GetFilletSolverOfEdgeuse(pEdgeuse)`: the solver of `edgeuse`, `None` when there
    /// is none.
    #[must_use]
    pub fn fillet_solver_of_edgeuse(&self, edgeuse: O) -> Option<O> {
        self.edgeuse_solver_map.get(&edgeuse).copied()
    }

    /// `GetFilletSolverOfEdge(pEdge)`: the solver of the first edge use of `edge` that
    /// has one.
    ///
    /// # Errors
    ///
    /// Those of [`ExecutiveAccess::edge_edgeuses`].
    pub fn fillet_solver_of_edge<A>(&self, access: &A, edge: O) -> Result<Option<O>>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        Ok(access
            .edge_edgeuses(edge)?
            .into_iter()
            .find_map(|eu| self.fillet_solver_of_edgeuse(eu)))
    }

    /// `NoteFilletError(eErrorType, pFilletEdge, cComment)`: an error at a fillet edge,
    /// with the edge's name, type and status from the adapter.
    ///
    /// # Errors
    ///
    /// Those of [`ExecutiveAccess::fillet_edge_note`] and
    /// [`FilletExecutive::note_fillet_edge`].
    pub fn note_fillet_error<A>(
        &mut self,
        access: &A,
        code: FilletErrorType,
        fillet_edge: O,
        comment: Option<&str>,
    ) -> Result<()>
    where
        A: ExecutiveAccess<O> + ?Sized,
    {
        let note = access.fillet_edge_note(fillet_edge)?;
        self.note_fillet_edge(code, fillet_edge, note, comment)
    }

    /// The body of [`FilletExecutive::note_fillet_error`] given the fillet edge's
    /// `GetName`, `GetFilletEdgeType()` and `GetStatus()`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a fillet edge type `GetName` writes no name for
    /// (upstream then prints an uninitialised buffer).
    pub fn note_fillet_edge(
        &mut self,
        code: FilletErrorType,
        fillet_edge: O,
        (name, edge_type, status): (Option<&'static str>, FilletEdgeType, FilStatus),
        comment: Option<&str>,
    ) -> Result<()> {
        let name = name.ok_or(SolidError::InvalidInput {
            operation: OP_NOTE,
            reason: "fillet edge type without a name (uninitialised name upstream)",
        })?;
        self.error.note_fillet_edge_error(
            code,
            status,
            edge_type,
            Some(fillet_edge),
            Some(name),
            comment,
        );
        Ok(())
    }

    /// `NoteFilletError(eErrorCode, eFilletStatus, pFilletedEdge, pTargetFace,
    /// pFilletEdge, pFilletSurface, cComment)`: [`ErrorInfo::note_merge_error`].
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_fillet_merge_error(
        &mut self,
        code: FilletErrorType,
        status: FilStatus,
        filleted_edge: Option<O>,
        target_face: Option<O>,
        fillet_edge: Option<O>,
        surface: Option<O>,
        comment: Option<&str>,
    ) {
        self.error.note_merge_error(
            code,
            status,
            filleted_edge,
            target_face,
            fillet_edge,
            surface,
            comment,
        );
    }

    /// `NoteFilletError(eErrorType, eCornerType, eFilletStat, sCornerName, ...)`:
    /// [`ErrorInfo::note_corner_error`].
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_fillet_corner_error(
        &mut self,
        code: FilletErrorType,
        corner_type: FilletCornerType,
        status: FilStatus,
        corner_name: Option<&str>,
        vertex: Option<O>,
        surfaces: [Option<O>; 2],
        fillet_vertices: [Option<O>; 2],
        comment: Option<&str>,
    ) {
        self.error.note_corner_error(
            code,
            corner_type,
            status,
            corner_name,
            vertex,
            surfaces,
            fillet_vertices,
            comment,
        );
    }

    /// `NoteFilletError(eErrorType, eFilletStat, pFilletedEdge, rapFilSurfs, ...)`:
    /// [`ErrorInfo::note_intersection_error`].
    #[allow(clippy::too_many_arguments)] // upstream's overload
    pub fn note_fillet_intersection_error(
        &mut self,
        code: FilletErrorType,
        status: FilStatus,
        filleted_edge: Option<O>,
        surfaces: &[Option<O>],
        curves: &[Option<O>],
        fillet_edges: &[Option<O>],
        location: Point3,
        comment: Option<&str>,
    ) {
        self.error.note_intersection_error(
            code,
            status,
            filleted_edge,
            surfaces,
            curves,
            fillet_edges,
            location,
            comment,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    /// A clamped degree-`p` curve with the radii `xs` as x coordinates over `[a, b]`.
    fn radius_curve(xs: &[f64], p: usize, a: f64, b: f64) -> BSplineCurve {
        let pw = xs.iter().map(|&x| CPoint::new(x, 0.0, 0.0, NOW)).collect();
        let spans = xs.len() - p;
        let mut u = vec![a; p + 1];
        for i in 1..spans {
            u.push(a + (b - a) * i as f64 / spans as f64);
        }
        u.extend(std::iter::repeat_n(b, p + 1));
        BSplineCurve::from_nurb(NurbCurve::new(pw, p, u).expect("curve"), 3, false)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-12 * (1.0 + a.abs().max(b.abs()))
    }

    fn linear(r0: f64, r1: f64) -> FilletLaw {
        FilletLaw::new(FilletLawKind::Linear {
            start_radius: r0,
            end_radius: r1,
        })
    }

    fn bspline(law_curve: BSplineCurve, map: Extent1d) -> FilletLaw {
        let mut law = FilletLaw::new(FilletLawKind::BSpline {
            law_curve,
            extended_law: None,
        });
        law.edge_map = map;
        law
    }

    #[test]
    fn linear_law_values_and_reverse() {
        let mut law = linear(2.0, 0.0);
        assert_eq!(law.edge_map.min(), 0.0);
        assert_eq!(law.edge_map.max(), 1.0);
        assert_eq!(law.edge_arc_length, 0.0);
        let ivl = Extent1d::from_min_max(0.0, 10.0);
        assert_eq!(
            law.evaluate(2.5, &ivl, false).expect("eval"),
            [1.5, -0.2, 0.0]
        );
        assert_eq!(
            law.evaluate(2.5, &ivl, true).expect("eval"),
            [0.5, 0.2, 0.0]
        );
        // a sub-map: only [0.5, 1] of the function maps to the edge
        let mut law = linear(2.0, 0.0);
        law.edge_map = Extent1d::from_min_max(0.5, 1.0);
        assert_eq!(
            law.evaluate(0.0, &ivl, false).expect("eval"),
            [1.0, -0.1, 0.0]
        );
        // a zero-length map means none
        assert!(law.is_linear_law().expect("linear"));
        assert_eq!(law.type_code(), LINEAR_FILLET_LAW_TYPE);
        assert!(law.is_kind_of(FILLET_LAW_TYPE));
        assert!(law.is_kind_of(11001));
        assert!(!law.is_kind_of(BSPLINE_FILLET_LAW_TYPE));
    }

    #[test]
    fn base_law_errors() {
        let mut law = FilletLaw::new(FilletLawKind::Base);
        assert!(law
            .evaluate(0.5, &Extent1d::from_min_max(0.0, 1.0), false)
            .is_err());
        assert_eq!(law.type_string(), "SmFilletLaw_TYPE");
        assert!(law.is_kind_of(FILLET_LAW_TYPE));
        assert!(!law.is_linear_law().expect("linear"));
    }

    #[test]
    fn bspline_law_inside_snap_and_extension() {
        // a degree-1 law from 1 to 3 over [0, 2], mapped onto the edge interval [10, 14]
        let mut law = bspline(
            radius_curve(&[1.0, 3.0], 1, 0.0, 2.0),
            Extent1d::from_min_max(0.0, 2.0),
        );
        assert!(law.is_linear_law().expect("linear"));
        assert_eq!(law.type_code(), BSPLINE_FILLET_LAW_TYPE);
        let ivl = Extent1d::from_min_max(10.0, 14.0);
        let v = law.evaluate(11.0, &ivl, false).expect("eval");
        assert!(
            close(v[0], 1.5) && close(v[1], 0.5) && close(v[2], 0.0),
            "{v:?}"
        );
        let v = law.evaluate(11.0, &ivl, true).expect("eval");
        assert!(close(v[0], 2.5) && close(v[1], -0.5), "{v:?}");
        assert!(law.extended_curve().is_none());
        // just outside: snaps to the end
        let v = law.evaluate(14.0 + 1e-13, &ivl, false).expect("eval");
        assert!(close(v[0], 3.0), "{v:?}");
        assert!(law.extended_curve().is_none());
        // further outside: extends the line (to at least a tenth of the domain)
        let v = law.evaluate(15.0, &ivl, false).expect("eval");
        assert!(close(v[0], 3.5) && close(v[1], 0.5), "{v:?}");
        let ext = law.extended_curve().expect("extended").natural_interval();
        assert!(ext.max() >= 3.0 && ext.min() <= 0.0, "{ext:?}");
        // zero-length interval
        assert!(law
            .evaluate(1.0, &Extent1d::from_min_max(3.0, 3.0), false)
            .is_err());
    }

    #[test]
    fn closed_law_curve_extension_is_a_copy() {
        // smFilletTest.cpp:307: the cubic r, 1.2r, 0.8r, r is G1-closed, so NLib hands
        // back an unextended copy and the evaluation clamps to the end
        let mut law = bspline(
            radius_curve(&[1.0, 1.2, 0.8, 1.0], 3, 0.0, 10.0),
            Extent1d::from_min_max(0.0, 10.0),
        );
        let ivl = Extent1d::from_min_max(0.0, 10.0);
        let v = law.evaluate(5.0, &ivl, false).expect("eval");
        assert!(close(v[0], 1.0), "{v:?}");
        let v = law.evaluate(12.0, &ivl, false).expect("eval");
        assert!(close(v[0], 1.0), "{v:?}");
        let ext = law.extended_curve().expect("copy").natural_interval();
        assert_eq!((ext.min(), ext.max()), (0.0, 10.0));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct H(u32);
    impl ClassObject for H {
        fn class_kind(&self) -> crate::intersect::point_class::ClassObjectKind {
            crate::intersect::point_class::ClassObjectKind::Other
        }
    }
    impl SolutionObject for H {
        fn kind(&self) -> crate::geom::solution_array::ObjectKind {
            crate::geom::solution_array::ObjectKind::Other
        }
    }

    #[test]
    fn error_text_is_snprintf_into_the_tblock_buffer() {
        assert_eq!(tblock_text(None), "(null)");
        assert_eq!(tblock_text(Some(&"x".repeat(2000))).len(), 1023);
        // 'é' is two bytes: byte 1023 falls inside the 512th one.
        assert_eq!(tblock_text(Some(&"é".repeat(600))).len(), 1022);
    }

    #[test]
    fn note_errors_follow_the_overloads() {
        let mut e = ErrorInfo::<H>::new(Some(H(9)));
        e.note_error(FilletErrorType::BadInt, None);
        assert_eq!((e.code, e.comment.as_str()), (5, "<no comment>\n"));
        e.note_edge_error(
            None,
            FilStatus::Failure,
            FilletErrorType::BadMerge,
            Some("ignored"),
        );
        assert_eq!((e.code, e.edge), (5, None));
        e.note_merge_error(
            FilletErrorType::EdgeProblem,
            FilStatus::Failure,
            Some(H(1)),
            Some(H(2)),
            None,
            Some(H(3)),
            None,
        );
        e.note_merge_error(
            FilletErrorType::EdgeProblem,
            FilStatus::Failure,
            Some(H(1)),
            Some(H(2)),
            None,
            Some(H(3)),
            Some("again"),
        );
        assert_eq!(e.edges, vec![None]);
        assert_eq!(e.surfaces, vec![Some(H(3))]);
        assert_eq!(e.comment, "again");
        let objects = ExecutiveObjects {
            executive: H(9),
            pseudo_brep: H(10),
            edge_attribute: H(11),
        };
        let x = FilletExecutive::new(Some(H(1)), objects);
        assert!(x.intersector.ignore_tangency && !x.initialized);
        assert_eq!(
            (x.error.executive, x.pseudo_brep),
            (Some(H(9)), Some(H(10)))
        );
    }
}
