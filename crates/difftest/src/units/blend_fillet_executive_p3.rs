//! Executive p3 parity for explicit states over empty native B-reps. All passing
//! cases exercise precondition failures and compare status/state values. Actual
//! fillet construction defers at executive p2 `ProcessSmoothEdgeSequences` (or p1
//! `CreateFilletCorners`), so no geometry-success coverage is claimed. Seven real
//! upstream API box examples are also run natively and explicitly deferred at
//! their unported executive-p1 API/setup entrypoints on Rust.

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::blend::fillet_executive::p3::{
    ContextMark, CornerView, ErrorInfo, ExecutionOptions, ExecutiveAccess, FilletExecutive,
    StitchAttempt, StitchSettings, TopoEdgeView,
};
use solidcore::boolean::topology_intersector::p1::TopologyIntersector;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

/// Parity runner for the three executive orchestration methods.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    record: Vec<f64>,
    #[serde(default)]
    upstream_case: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Handle(usize);
impl ClassObject for Handle {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Other
    }
}
impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Other
    }
}

// This fixture owns B-reps only. Reject nonexistent handles instead of creating
// fabricated solvers, corners, surfaces or topology. No metadata method is reached
// before the correctly labelled executive p1/p2 deferral.
struct Access {
    breps: Vec<Option<solidcore::topology::Brep>>,
}
fn missing_error() -> SolidError {
    SolidError::InvalidInput {
        operation: "harness::fillet_executive_p3",
        reason: "fixture has no such object",
    }
}
fn missing<T>() -> SolidResult<T> {
    Err(missing_error())
}
impl ExecutiveAccess<Handle> for Access {
    // This empty-B-rep fixture has no face handles or intersector world. These
    // required adapters reject nonexistent objects; they are not UNPORTED calls
    // to the now-ported face p3, topology intersector p2, stitch p2, fillet geom p4,
    // fillet corner p3 or fillet solver p1 implementations.
    fn notify_face(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: solidcore::geom::object::NotifyOperation,
        _: Option<Handle>,
        _: Option<Handle>,
        _: Option<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn intersect_insert_relate(
        &mut self,
        _: &mut Kernel,
        _: &mut TopologyIntersector<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn delete_faces(
        &mut self,
        _: &mut Kernel,
        _: &mut TopologyIntersector<Handle>,
        _: &[Handle],
        _: Option<&[Handle]>,
    ) -> SolidResult<()> {
        missing()
    }
    fn do_stitching(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: f64,
        _: Option<&[Handle]>,
        _: Option<&[Handle]>,
        _: StitchSettings,
        _: Option<&mut f64>,
        _: Option<&mut f64>,
    ) -> SolidResult<StitchAttempt> {
        missing()
    }
    fn make_face_brep(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn insert_geom_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn calc_corner_geom(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn trim_rails(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn make_corner_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn adjust_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        // No concrete object is constructed here. Future fixtures must dispatch
        // derived types to their owners, as documented on ExecutiveAccess.
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::AdjustTopology
        Err(SolidError::unported(
            "SmFilletCorner::AdjustTopology",
            "blend.fillet_corner_p1",
        ))
    }
    fn adjust_tangent_corner(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        // No concrete object is constructed here. Future fixtures must dispatch
        // derived types to their owners, as documented on ExecutiveAccess.
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::AdjustTangentCorner
        Err(SolidError::unported(
            "SmFilletCorner::AdjustTangentCorner",
            "blend.fillet_corner_p1",
        ))
    }
    fn calc_fillet_geom(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        // No concrete object is constructed here. Future fixtures must dispatch
        // derived types to their owners, as documented on ExecutiveAccess.
        // UNPORTED(blend.fillet_solver_p4): SmFilletSolver::CalcFilletGeom
        Err(SolidError::unported(
            "SmFilletSolver::CalcFilletGeom",
            "blend.fillet_solver_p4",
        ))
    }
    fn corner_error_info(
        &self,
        _: Handle,
        _: &FilletExecutive<Handle>,
    ) -> SolidResult<CornerView<Handle>> {
        missing()
    }
    fn is_blending_corner(&self, _: Handle) -> SolidResult<bool> {
        missing()
    }
    fn is_tangent_corner(&self, _: Handle) -> SolidResult<bool> {
        missing()
    }
    fn corner_vertex(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn corner_approx_tolerance(&self, _: Handle) -> SolidResult<f64> {
        missing()
    }
    fn solver_geoms(&self, _: Handle) -> SolidResult<Vec<Handle>> {
        missing()
    }
    fn solver_geom(&self, _: Handle, _: usize) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn solver_edge(&self, _: Handle) -> SolidResult<Handle> {
        missing()
    }
    fn solver_approx_tolerance(&self, _: Handle) -> SolidResult<f64> {
        missing()
    }
    fn geom_face(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn topo_edge(&self, _: Handle) -> SolidResult<TopoEdgeView<Handle>> {
        missing()
    }
    fn vertex_point(&self, _: Handle) -> SolidResult<Point3> {
        missing()
    }
    fn edge_curve_ends(&self, _: &Kernel, _: Handle) -> SolidResult<(Point3, Point3)> {
        missing()
    }
    fn set_calc_as_const_rad(&mut self, _: Handle, _: bool) -> SolidResult<()> {
        missing()
    }
    fn new_brep(&mut self, _: &Kernel) -> SolidResult<Handle> {
        let handle = Handle(self.breps.len());
        self.breps.push(Some(solidcore::topology::Brep::new()));
        Ok(handle)
    }
    fn delete_brep(&mut self, brep: Handle) -> SolidResult<()> {
        if let Some(slot) = self.breps.get_mut(brep.0) {
            *slot = None;
            Ok(())
        } else {
            missing()
        }
    }
    fn tolerance(&self, k: &Kernel, brep: Handle, approx: bool) -> SolidResult<f64> {
        let tolerance = self
            .breps
            .get(brep.0)
            .and_then(Option::as_ref)
            .map(solidcore::topology::Brep::tolerance)
            .ok_or_else(missing_error)?;
        // Fixtures have finite nonnegative stored zone tolerances, normal size.
        // Preserve the distinct approximation-tolerance query as well.
        Ok(if approx {
            solidcore::geom::tol::approx_tol3d(k, 0.5 * tolerance)
        } else {
            tolerance
        })
    }
    fn editing_enabled(&self, brep: Handle) -> SolidResult<bool> {
        self.breps
            .get(brep.0)
            .and_then(Option::as_ref)
            .map(solidcore::topology::Brep::editing_enabled)
            .ok_or_else(missing_error)
    }
    fn set_editing_enabled(&mut self, brep: Handle, value: bool) -> SolidResult<()> {
        match self.breps.get_mut(brep.0).and_then(Option::as_mut) {
            Some(b) => {
                b.set_editing_enabled(value);
                Ok(())
            }
            None => missing(),
        }
    }
    fn edges(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        let brep = self
            .breps
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or_else(missing_error)?;
        if !brep.edges().is_empty() {
            return missing();
        }
        Ok(Vec::new())
    }
    fn vertices(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        let brep = self
            .breps
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or_else(missing_error)?;
        if !brep.vertices().is_empty() {
            return missing();
        }
        Ok(Vec::new())
    }
    fn faces(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        let brep = self
            .breps
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or_else(missing_error)?;
        if !brep.faces().is_empty() {
            return missing();
        }
        Ok(Vec::new())
    }
    fn add_attribute(&mut self, _: Handle, _: Option<Handle>) -> SolidResult<()> {
        missing()
    }
    fn new_keep_attribute(&mut self, _: &Kernel) -> SolidResult<Handle> {
        missing()
    }
    fn attribute_users(&self, _: Handle) -> SolidResult<Vec<Handle>> {
        missing()
    }
    fn delete_attribute(&mut self, _: Handle) -> SolidResult<()> {
        missing()
    }
    fn with_marks<T>(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: Handle,
        _: &[(Handle, Handle)],
        _: impl FnOnce(&mut Self, &mut Kernel, &[ContextMark]) -> SolidResult<T>,
    ) -> SolidResult<T> {
        missing()
    }
    fn shrink_face(&mut self, _: &mut Kernel, _: Handle) -> SolidResult<()> {
        missing()
    }
    fn boolean_post_process(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
        _: bool,
    ) -> SolidResult<()> {
        missing()
    }
    fn geom_has_surface(&self, _: Handle) -> SolidResult<bool> {
        missing()
    }
    fn geom_rails(&self, _: Handle) -> SolidResult<Vec<Option<Handle>>> {
        missing()
    }
    fn geom_side_edgeuses(&self, _: Handle) -> SolidResult<Vec<Handle>> {
        missing()
    }
    fn edgeuse_edge(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn fillet_edge_original_face(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn fillet_edge_original_edge(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn find_fillet_brep_edge(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &[Handle],
        _: bool,
    ) -> SolidResult<()> {
        missing()
    }
    fn corner_edges(&self, _: Handle) -> SolidResult<Vec<Option<Handle>>> {
        missing()
    }
    fn blend_edgeuse(&mut self, _: &mut Kernel, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn edge_tolerance(&self, _: Handle) -> SolidResult<f64> {
        missing()
    }
    fn edge_vertices(&self, _: Handle) -> SolidResult<(Option<Handle>, Option<Handle>)> {
        missing()
    }
    fn variable_radius_solver(
        &self,
        _: Handle,
    ) -> SolidResult<Option<&solidcore::blend::fillet_standard_solver::p1::VariableRadiusFs>> {
        missing()
    }
    fn variable_radius_solver_mut(
        &mut self,
        _: Handle,
    ) -> SolidResult<Option<&mut solidcore::blend::fillet_standard_solver::p1::VariableRadiusFs>>
    {
        missing()
    }
    fn solver_edgeuse(&self, _: Handle, _: usize) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn corner_setback_distance(&self, _: Handle) -> SolidResult<f64> {
        missing()
    }
    fn new_surface_generator(
        &mut self,
        _: &mut Kernel,
        _: solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator,
    ) -> SolidResult<Handle> {
        missing()
    }
    fn new_constant_radius_solver(
        &mut self,
        _: &mut Kernel,
        _: f64,
        _: f64,
        _: f64,
        _: f64,
        _: Handle,
    ) -> SolidResult<Handle> {
        missing()
    }
    fn set_surface_generator(&mut self, _: Handle, _: Handle) -> SolidResult<()> {
        missing()
    }
}

fn flag(value: bool) -> f64 {
    f64::from(u8::from(value))
}

fn output(values: &[f64]) -> Result<Output, HarnessError> {
    if values.len() < 15 {
        return Err(HarnessError::new("bad executive output length"));
    }
    let mut out = Output::new();
    out.ints.insert(
        "status_error_flags".into(),
        values
            .get(..9)
            .ok_or_else(|| HarnessError::new("short state"))?
            .iter()
            .map(|v| *v as i64)
            .collect(),
    );
    out.values.insert(
        "tolerances".into(),
        values
            .get(9..11)
            .ok_or_else(|| HarnessError::new("short tolerances"))?
            .to_vec(),
    );
    out.ints.insert(
        "comment_lengths".into(),
        values
            .get(11..13)
            .ok_or_else(|| HarnessError::new("short comments"))?
            .iter()
            .map(|v| *v as i64)
            .collect(),
    );
    out.ints.insert(
        "corner_types".into(),
        values
            .get(13..15)
            .ok_or_else(|| HarnessError::new("short corner types"))?
            .iter()
            .map(|v| *v as i64)
            .collect(),
    );
    out.ints.insert(
        "comment_bytes".into(),
        values
            .get(15..)
            .ok_or_else(|| HarnessError::new("short comment bytes"))?
            .iter()
            .map(|v| *v as i64)
            .collect(),
    );
    Ok(out)
}

fn rust(input: &Inputs) -> Result<Output, HarnessError> {
    if let Some(example) = input.upstream_case {
        ffi::validate_upstream_fixture(example).map_err(|e| HarnessError::new(e.to_string()))?;
        if !input.record.is_empty() {
            return Err(HarnessError::new("mixed fixture forms"));
        }
        // These fixture constructors are the actual missing API entrypoints, not
        // geometric passes or a replacement for direct p3 parity.
        let symbol = match example {
            // UNPORTED(blend.fillet_executive_p1): SmApiCircularFillet
            0 => "SmApiCircularFillet",
            // UNPORTED(blend.fillet_executive_p1): SmApiChamferFillet
            1 => "SmApiChamferFillet",
            // UNPORTED(blend.fillet_executive_p1): SmApiFilletEdges
            2 => "SmApiFilletEdges",
            // UNPORTED(blend.fillet_executive_p1): SmApiFilletEdgesPerEdge
            3 => "SmApiFilletEdgesPerEdge",
            // UNPORTED(blend.fillet_executive_p1): SmApiVariableRadiusFillet
            4 => "SmApiVariableRadiusFillet",
            // UNPORTED(blend.fillet_executive_p1): SmApiSetBevelCorners
            5 | 6 => "SmApiSetBevelCorners",
            _ => return Err(HarnessError::new("unknown upstream fixture")),
        };
        return Ok(Output::deferred("blend.fillet_executive_p1", symbol));
    }
    ffi::validate(&input.record).map_err(|e| HarnessError::new(e.to_string()))?;
    let [op, has_target, initialized, prior_boolean, editing, want_error, seed_code, tolerance, piecewise, insertion, global, trimming, classification] =
        input.record.as_slice()
    else {
        return Err(HarnessError::new("short executive record"));
    };
    let mut k = Kernel::default();
    k.context.set_doing_boolean(*prior_boolean != 0.0);
    let mut b = solidcore::topology::Brep::new();
    // Apply SmZoneTol3d's default-sentinel/fabs conversion, then write the member.
    // SmBrep::SetTolerance belongs to brep p2 and is not exercised by this setup.
    b.set_tolerance(solidcore::geom::tol::zone_tol3d(&k, *tolerance));
    b.set_editing_enabled(*editing != 0.0);
    let mut access = Access {
        breps: vec![Some(b)],
    };
    let target = (*has_target != 0.0).then_some(Handle(0));
    let mut executive = FilletExecutive {
        target,
        fillet: None,
        corners: vec![],
        solvers: vec![],
        topo_edges: vec![],
        edge_attribute: None,
        initialized: *initialized != 0.0,
        options: ExecutionOptions {
            piecewise_merge: *piecewise != 0.0,
            topology_insertion: *insertion != 0.0,
            global_merge: *global != 0.0,
            trimming: *trimming != 0.0,
            classification: *classification != 0.0,
        },
        error: ErrorInfo {
            code: *seed_code as i32,
            ..ErrorInfo::default()
        },
        intersector: TopologyIntersector::new(
            None,
            None,
            solidcore::math::EFF_ZERO_SQ,
            30.0 * (solidcore::math::PI / 180.0),
        ),
        intersection_edges: vec![],
        intersection_vertices: vec![],
        law_sequence_edges: vec![],
    };
    // The executive constructor sets this after constructing its SmMerge base.
    executive.intersector.ignore_tangency = true;
    let mut error_out = ErrorInfo {
        code: 5,
        comment: "sentinel".into(),
        ..ErrorInfo::default()
    };
    let optional = (*want_error != 0.0).then_some(&mut error_out);
    let result = match *op as i32 {
        0 => executive.do_fillet(&mut k, &mut access),
        1 => executive.create_fillet_brep(&mut k, &mut access, optional),
        2 => executive.do_filleting(&mut k, &mut access, optional),
        _ => return Err(HarnessError::new("invalid executive opcode")),
    };
    let status = match result {
        Ok(()) => 0,
        Err(error) => {
            if let Some(deferred) = Output::unported(&error) {
                return Ok(deferred);
            }
            match error {
                SolidError::InvalidInput { .. } => 1007,
                SolidError::Upstream { code, .. } => code,
                other => return Err(HarnessError::new(other.to_string())),
            }
        }
    };
    let editing = if let Some(target) = target {
        flag(
            access
                .editing_enabled(target)
                .map_err(|e| HarnessError::new(e.to_string()))?,
        )
    } else {
        -1.0
    };
    let target_tol = if let Some(target) = target {
        access
            .tolerance(&k, target, false)
            .map_err(|e| HarnessError::new(e.to_string()))?
    } else {
        -1.0
    };
    let fillet_tol = if let Some(fillet) = executive.fillet {
        access
            .tolerance(&k, fillet, false)
            .map_err(|e| HarnessError::new(e.to_string()))?
    } else {
        -1.0
    };
    let mut values = vec![
        f64::from(status),
        f64::from(executive.error.code),
        f64::from(executive.error.status),
        f64::from(error_out.code),
        f64::from(error_out.status),
        flag(k.context.doing_boolean()),
        editing,
        flag(executive.intersector.doing_fillets),
        flag(executive.fillet.is_some()),
        target_tol,
        fillet_tol,
        executive.error.comment.len() as f64,
        error_out.comment.len() as f64,
        // Encode upstream discriminants only at the runner boundary.
        f64::from(executive.error.corner_type as i32),
        f64::from(error_out.corner_type as i32),
    ];
    values.extend(
        executive
            .error
            .comment
            .bytes()
            .chain(error_out.comment.bytes())
            .map(f64::from),
    );
    output(&values)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_executive_p3"
    }
    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(256),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let input = case.parse::<Inputs>()?;
        if let Some(example) = input.upstream_case {
            if !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            let values =
                ffi::upstream_fixture(example).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut out = Output::new();
            out.ints.insert(
                "api_status_counts".into(),
                values.into_iter().map(|v| v as i64).collect(),
            );
            return Ok(out);
        }
        output(&ffi::eval(&input.record).map_err(|e| HarnessError::new(e.to_string()))?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse::<Inputs>()?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let tolerance = *rng.choose(&[0.0, 1e-300, 1e-9, 1e-5, 1.0, 1e100])?;
        Some(serde_json::json!({"record": [rng.below(3) as f64,
            flag(rng.chance(0.7)), flag(rng.chance(0.5)), flag(rng.chance(0.5)),
            flag(rng.chance(0.5)), flag(rng.chance(0.5)), rng.below(8) as f64, tolerance,
            flag(rng.chance(0.5)), flag(rng.chance(0.5)), flag(rng.chance(0.5)),
            flag(rng.chance(0.5)), flag(rng.chance(0.5))]}))
    }
}
