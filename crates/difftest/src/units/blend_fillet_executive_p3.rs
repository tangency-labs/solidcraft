//! Executive p3 state/error parity and real-solid fillet session checkpoints.
//! Setup, corner creation and direct first-Nx1 topology use actual bodies and
//! owned production solvers. Full fillet construction remains deferred at the
//! missing public API; native result records do not claim Rust geometry success.
//! `fillet_preview` cases run `SmApiFilletPreview` (module `preview`) on their own seed
//! stream (`preview0000`, ...), so each `gen` case stays the one the baseline has.

use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::blend::fillet_executive::p1::ExecutiveObjects;
use solidcore::blend::fillet_executive::p3::{
    ContextMark, CornerView, ErrorInfo, ExecutionOptions, ExecutiveAccess, FilletExecutive,
    StitchAttempt, StitchSettings, TopoEdgeView,
};
use solidcore::blend::fillet_geom::p1::{FilStatus, FilletEdgeType};
use solidcore::boolean::topology_intersector::p1::TopologyIntersector;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;
use std::collections::BTreeMap;

#[path = "blend_fillet_executive_p3/end_to_end.rs"]
mod end_to_end;
#[path = "blend_fillet_executive_p3/preview.rs"]
mod preview;
#[path = "blend_fillet_executive_p3/surface_bodies.rs"]
mod surface_bodies;

/// Parity runner for the three executive orchestration methods.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    record: Vec<f64>,
    #[serde(default)]
    upstream_case: Option<usize>,
    #[serde(default)]
    real_solid: Option<end_to_end::RealSolid>,
    #[serde(default)]
    surface_surface: Option<end_to_end::SurfaceSurface>,
    #[serde(default)]
    surface_surface_bodies: Option<surface_bodies::SurfaceSurfaceBodies>,
    #[serde(default)]
    fillet_preview: Option<preview::FilletPreview>,
}

impl Inputs {
    /// Whether any other fixture form accompanies a `fillet_preview` case.
    fn mixed_with_preview(&self) -> bool {
        self.surface_surface_bodies.is_some()
            || self.surface_surface.is_some()
            || self.real_solid.is_some()
            || self.upstream_case.is_some()
            || !self.record.is_empty()
    }
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
/// `target->MergeBrep(other, NULL, NULL, bAddAnalytics)` for this fixture, which keeps no
/// notification state: `topology::brep::p3::merge_brep` under the kernel's identities
/// (`with_kernel_callbacks`), `other` enrolled in the same identity domain first so its
/// objects never share a key with `target`'s.
fn merge_breps(
    k: &mut Kernel,
    target: &mut solidcore::topology::Brep,
    other: &mut solidcore::topology::Brep,
    add_analytics: bool,
) -> SolidResult<()> {
    use solidcore::topology::brep::p3 as bp3;
    let serial = bp3::notify_serial(&mut k.context, other)?;
    let source_key = move |e| bp3::topology_ref(serial, e);
    let other = &*other;
    bp3::with_kernel_callbacks(k, target, |k, b, env| {
        bp3::merge_brep(k, env, b, other, &source_key, None, None, add_analytics)
    })
}

/// `brep->OrientTrimmedSurfaces(..)` for this fixture: `topology::brep::p3`'s under the
/// kernel's identities. The fixture has no face handles, so only an empty
/// `pOptFacesToSwapUV` list is accepted.
fn orient_brep(
    k: &mut Kernel,
    b: &mut solidcore::topology::Brep,
    outward: bool,
    maybe_not_solid: &mut bool,
    just_unify: bool,
    faces_to_swap_uv: Option<&mut Vec<Handle>>,
) -> SolidResult<()> {
    use solidcore::topology::brep::p3 as bp3;
    if faces_to_swap_uv.as_ref().is_some_and(|f| !f.is_empty()) {
        return missing();
    }
    let swap: Option<&[solidcore::topology::FaceId]> = faces_to_swap_uv.map(|_| &[][..]);
    let r = bp3::with_kernel_callbacks(k, b, |k, b, env| {
        let mut marks = solidcore::topology::topology_traverser::Marks::default();
        bp3::orient_trimmed_surfaces(k, b, env, &mut marks, outward, just_unify, swap)
    })?;
    *maybe_not_solid = r.maybe_not_closed_solid;
    Ok(())
}

impl ExecutiveAccess<Handle> for Access {
    fn solver_base(
        &self,
        _: Handle,
    ) -> SolidResult<&solidcore::blend::fillet_solver::p1::FilletSolver> {
        missing()
    }
    fn solver_base_mut(
        &mut self,
        _: Handle,
    ) -> SolidResult<&mut solidcore::blend::fillet_solver::p1::FilletSolver> {
        missing()
    }
    fn solver_has_surface(&self, _: Handle, _: usize) -> SolidResult<bool> {
        missing()
    }
    fn edge_g1_knots(&mut self, _: &Kernel, _: Handle) -> SolidResult<Vec<f64>> {
        missing()
    }
    fn edgeuse_vertex(&self, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    // (the fixture's handles name no corners)
    fn corner_solvers(&self, _: Handle, _: &FilletExecutive<Handle>) -> SolidResult<Vec<Handle>> {
        missing()
    }
    fn calc_corner_vertices(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn check_degeneracy(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<bool> {
        missing()
    }
    fn calc_corner_edges(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn reinitialize_corner_edges(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn insert_corner_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    fn corner_tangent_solver(&self, _: Handle, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    /// `SmFilletExecutive::CreateFilletCorners`, the ported
    /// `FilletExecutive::create_fillet_corners`. The fixture names no vertices, so
    /// its corner factory (`SmFilletCorner::Create`) rejects any it is asked for.
    fn create_corners(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<Handle>,
        error: Option<&mut ErrorInfo<Handle>>,
    ) -> SolidResult<()> {
        executive.create_fillet_corners(k, |_, _, _, _| missing(), error)
    }
    /// `SmFilletExecutive::DoPiecewiseMerge`: this empty-B-rep fixture never builds a
    /// fillet B-rep, so there is nothing to merge piecewise.
    fn piecewise_merge(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        missing()
    }
    // This empty-B-rep fixture has no face handles or intersector world. These
    // required adapters reject nonexistent objects; they are not UNPORTED calls
    // to the now-ported face p3, topology intersector p2, stitch p2, fillet geom p4
    // and p6, fillet corner p3 or fillet solver p1 implementations.
    // `set_brep_tolerance` runs the brep p2 port for real.
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
    fn merge_brep(
        &mut self,
        k: &mut Kernel,
        target: Handle,
        other: Handle,
        copy_map: Option<&mut CopyBrepMap<Handle>>,
        reverse_map: Option<&mut CopyBrepMap<Handle>>,
        add_analytics: bool,
    ) -> SolidResult<()> {
        // (the fixture's handles name B-reps only: no copy maps of their objects)
        if copy_map.is_some() || reverse_map.is_some() {
            return missing();
        }
        let mut src = self
            .breps
            .get_mut(other.0)
            .and_then(Option::take)
            .ok_or_else(missing_error)?;
        let r = match self.breps.get_mut(target.0).and_then(Option::as_mut) {
            Some(dst) => merge_breps(k, dst, &mut src, add_analytics),
            None => missing(),
        };
        if let Some(slot) = self.breps.get_mut(other.0) {
            *slot = Some(src);
        }
        r
    }
    /// The fixture's B-reps carry no edge objects, so only an empty topo-edge list
    /// (`remove_topo_edges` over no edges, a no-op) can run.
    fn remove_topo_edges(
        &mut self,
        _: &mut Kernel,
        executive: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
        if executive.topo_edges.is_empty() {
            Ok(())
        } else {
            missing()
        }
    }
    fn orient_trimmed_surfaces(
        &mut self,
        k: &mut Kernel,
        brep: Handle,
        outward: bool,
        maybe_not_solid: &mut bool,
        just_unify: bool,
        faces_to_swap_uv: Option<&mut Vec<Handle>>,
    ) -> SolidResult<()> {
        let b = self
            .breps
            .get_mut(brep.0)
            .and_then(Option::as_mut)
            .ok_or_else(missing_error)?;
        orient_brep(k, b, outward, maybe_not_solid, just_unify, faces_to_swap_uv)
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
    fn trim_original_surfaces(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut Vec<Handle>,
        _: &mut Vec<Handle>,
        _: &[ContextMark],
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
    fn test_rollover(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut Vec<Handle>,
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
        // This fixture builds no solver, so there is nothing to dispatch to: the
        // base `CalcFilletGeom` is ported (`blend::fillet_solver::p4`) and
        // `SmSurfaceSurfaceFS`'s override is blend.fillet_standard_solver_p2's.
        missing()
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
    fn set_brep_tolerance(
        &mut self,
        k: &mut Kernel,
        brep: Handle,
        tolerance: f64,
        update_only_if_larger: bool,
        cascade_to_boundaries: bool,
    ) -> SolidResult<()> {
        let b = self
            .breps
            .get_mut(brep.0)
            .and_then(Option::as_mut)
            .ok_or_else(missing_error)?;
        solidcore::topology::brep::p2::set_tolerance(
            k,
            b,
            tolerance,
            update_only_if_larger,
            cascade_to_boundaries,
        )
    }
    fn stitch_faces(
        &mut self,
        k: &mut Kernel,
        brep: Handle,
        tolerance: f64,
        faces: Option<&[Handle]>,
        allow_spine_edges: bool,
    ) -> SolidResult<StitchAttempt> {
        // the fixture has no face handles; both executive call sites stitch every face
        if faces.is_some() {
            return missing();
        }
        let b = self
            .breps
            .get_mut(brep.0)
            .and_then(Option::as_mut)
            .ok_or_else(missing_error)?;
        StitchAttempt::from_stitch(solidcore::topology::brep::p4::stitch_faces(
            k,
            b,
            tolerance,
            None,
            allow_spine_edges,
        ))
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
    fn edge_is_manifold(&self, _: Handle) -> SolidResult<bool> {
        missing()
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
    fn edge_edgeuses(&self, _: Handle) -> SolidResult<Vec<Handle>> {
        missing()
    }
    fn fillet_edge_note(
        &self,
        _: Handle,
    ) -> SolidResult<(Option<&'static str>, FilletEdgeType, FilStatus)> {
        missing()
    }
    fn new_executive_objects(&mut self, _: &mut Kernel) -> SolidResult<ExecutiveObjects<Handle>> {
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
    fn solver_corner(&self, _: Handle, _: usize) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn solver_other_corner(&self, _: Handle, _: Handle) -> SolidResult<Option<Handle>> {
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
    fn new_fillet_solver(
        &mut self,
        _: &mut Kernel,
        _: &solidcore::blend::fillet_executive::p2::SolverSpec<'_, Handle>,
    ) -> SolidResult<Handle> {
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
        return end_to_end::upstream_example(example)?
            .ok_or_else(|| HarnessError::new("unknown upstream fixture"));
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
        pseudo_brep: None,
        surface_generators: vec![],
        filleted_vertices: Vec::new(),
        vertex_corner_map: BTreeMap::new(),
        edgeuse_solver_map: BTreeMap::new(),
        self_intersection_handler: None,
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
                SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007,
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
    Ok(with_point_step(output(&values)?, rust_point_step(&k)))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_executive_p3"
    }
    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<Case>, HarnessError> {
        let gen_ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(256),
            ..ctx.clone()
        };
        let mut v = crate::runner::default_cases(self, &gen_ctx)?;
        // the SmApiFilletPreview cases under their own names and seeds, so that each
        // `gen` case stays the one the baseline has
        let preview_ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(preview::MIN_GENERATED),
            ..ctx.clone()
        };
        for mut c in preview_ctx.generated_cases("blend.fillet_executive_p3:preview", |rng| {
            Some(serde_json::json!({ "fillet_preview": preview::generate(rng) }))
        }) {
            c.unit = self.id().to_string();
            c.name = c.name.replacen("gen", "preview", 1);
            v.push(c);
        }
        Ok(v)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let input = case.parse::<Inputs>()?;
        if let Some(preview) = &input.fillet_preview {
            if input.mixed_with_preview() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return preview::cpp(preview);
        }
        if let Some(pair) = &input.surface_surface_bodies {
            if input.surface_surface.is_some()
                || input.real_solid.is_some()
                || input.upstream_case.is_some()
                || !input.record.is_empty()
            {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return surface_bodies::cpp(pair);
        }
        if let Some(pair) = &input.surface_surface {
            if input.real_solid.is_some() || input.upstream_case.is_some() || !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return end_to_end::surface_surface_cpp(pair);
        }
        if let Some(real) = &input.real_solid {
            if input.upstream_case.is_some() || !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return end_to_end::cpp(real);
        }
        if let Some(example) = input.upstream_case {
            if !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            let (values, step) = fresh_cpp(|| ffi::upstream_fixture(example))
                .map_err(|e| HarnessError::new(e.to_string()))?;
            let mut out = Output::new();
            out.ints.insert(
                "api_status_counts".into(),
                values.into_iter().map(|v| v as i64).collect(),
            );
            return Ok(with_point_step(out, step));
        }
        let (values, step) =
            fresh_cpp(|| ffi::eval(&input.record)).map_err(|e| HarnessError::new(e.to_string()))?;
        Ok(with_point_step(output(&values)?, step))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let input = case.parse::<Inputs>()?;
        if let Some(preview) = &input.fillet_preview {
            if input.mixed_with_preview() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return preview::rust(preview);
        }
        if let Some(pair) = &input.surface_surface_bodies {
            if input.surface_surface.is_some()
                || input.real_solid.is_some()
                || input.upstream_case.is_some()
                || !input.record.is_empty()
            {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return surface_bodies::rust(pair);
        }
        if let Some(pair) = &input.surface_surface {
            if input.real_solid.is_some() || input.upstream_case.is_some() || !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return end_to_end::surface_surface_rust(pair);
        }
        if let Some(real) = &input.real_solid {
            if input.upstream_case.is_some() || !input.record.is_empty() {
                return Err(HarnessError::new("mixed fixture forms"));
            }
            return end_to_end::rust(real);
        }
        rust(&input)
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

/// Compared key: the `sdStaticStep` latch (ND-3: `SmFilletSolver::ComputePointValues`'
/// function static, fixed by the first call in a process) after the case.
pub(super) const POINT_STEP: &str = "fillet_point_step";

/// Runs a case's C++ calls in a fresh child process, so `sdStaticStep` starts unlatched
/// as on a first call whatever ran before, and returns the record with the step the case
/// left latched there (`1e-5` when it latched nothing). The step is a compared output
/// only ([`POINT_STEP`]); the Rust side never sees it.
pub(super) fn fresh_cpp(
    case: impl FnOnce() -> Result<Vec<f64>, solidcore_ffi::FfiError>,
) -> Result<(Vec<f64>, f64), solidcore_ffi::FfiError> {
    solidcore_ffi::units::blend_fillet_solver_p4::with_fresh_static_step(case)
}

/// The Rust side's [`Kernel::fillet_point_step`] after a case run on a fresh kernel,
/// read as the C++ probe reads `sdStaticStep`: the probe's finite-difference crossing
/// point latches `1e-5` when nothing has.
pub(super) fn rust_point_step(k: &Kernel) -> f64 {
    k.fillet_point_step.get().unwrap_or(1.0e-5)
}

/// `out` with the case's latched step ([`POINT_STEP`]); a deferred or rejected output is
/// returned unchanged.
pub(super) fn with_point_step(mut out: Output, step: f64) -> Output {
    if out.deferred.is_none() && out.rejected.is_none() {
        out.values.insert(POINT_STEP.into(), vec![step]);
    }
    out
}
