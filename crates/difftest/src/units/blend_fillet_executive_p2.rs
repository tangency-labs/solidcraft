//! Runner for unit `blend.fillet_executive_p2`. C++:
//! `solidcore_ffi::units::blend_fillet_executive_p2`. Rust:
//! `solidcore::blend::fillet_executive::p2`.
//!
//! The fillet-law constructors are compared on every value (edge map, radii and arc
//! length of a linear law; the degree, control points and knots of the elliptical
//! B-spline law curve). The executive members run on an `SmApiCreateBox` box built by
//! the C++ side and imported into a Rust [`Brep`] through `heal.stitch_p2`'s export
//! (`topology.brep_p1`'s import), with the executive state the C++ constructor leaves
//! (the executive constructor belongs to `blend.fillet_executive_p1`). Their output is
//! one float array (`out`): the status, then the switches, solver count and error code
//! (`SetFilletParameters`) or the sequence and list lengths. `-2` in the status slot
//! (upstream would dereference null) and `-4` (a box with fewer than 12 edges) are
//! guards: an error on both sides. An elliptical
//! law whose interpolation failed (no such input is known) would have a null curve
//! upstream and an error here: also an error on both sides.
//!
//! Whatever reaches `AddFilletSurfaceGenerator`, `LoadFilletSolver`, `NoteError` or
//! `CreateFilletCorners` defers to `blend.fillet_executive_p1`, so `SmTemporaryRadiusChange`
//! (which needs a loaded solver) is not compared yet.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_custom_solver::ConstantRadiusAssistedFs;
use solidcore::blend::fillet_executive::p2::FilletParameters;
use solidcore::blend::fillet_executive::p3::{
    ContextMark, CornerView, ErrorInfo, ExecutionOptions, ExecutiveAccess, FilletExecutive,
    StitchAttempt, StitchSettings, TopoEdgeView,
};
use solidcore::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
use solidcore::blend::fillet_standard_solver::p1::{
    constant_radius_from_edgeuse, FilletLaw, FilletLawKind, FilletSolverType,
    FilletSurfaceGeneratorType,
};
use solidcore::boolean::topology_intersector::p1::TopologyIntersector;
use solidcore::error::SM_ERR_NULL_POINTER;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p2::{self as ffi, Op};
use solidcore_ffi::units::heal_stitch_p2 as stitch_ffi;
use solidcore_ffi::units::topology_brep_p1::counts;

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in a status slot.
const NULL_DEREF: f64 = -2.0;

/// The box has fewer than 12 edges (`-4 n_edges`): no op runs on either side.
const SHORT_BOX: f64 = -4.0;

/// `SmPlane_TYPE`; `SmLine_TYPE`: the geometry of an `SmApiCreateBox` box, which the
/// export carries exactly.
const BOX_SURFACE: i64 = 15011;
const BOX_CURVE: i64 = 14010;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct BoxIn {
    origin: [f64; 3],
    size: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// `SmLinearFilletLaw(start, end, map)`.
    LawLinear {
        start: f64,
        end: f64,
        map: Option<[f64; 2]>,
    },
    /// The elliptical `SmBSplineFilletLaw`.
    LawElliptic {
        start: f64,
        end: f64,
        outer: f64,
        span: f64,
        start_deriv: f64,
        end_deriv: f64,
        map: Option<[f64; 2]>,
    },
    /// `SetFilletParameters` (edge pointers); `laws` entries are `[0]` (null) or
    /// `[1, start, end]` (`SmLinearFilletLaw`).
    SetParams {
        has_brep: bool,
        #[serde(rename = "box")]
        bx: BoxIn,
        edges: Vec<usize>,
        xsect_types: Vec<usize>,
        radius_types: Vec<usize>,
        radius_values: Vec<f64>,
        laws: Option<Vec<Vec<f64>>>,
        approx: f64,
        ang: f64,
        tan: f64,
        xsect: f64,
        continuity: u32,
        thumbweight: f64,
    },
    /// `sub` 0 `SetFilletBrepEdgeMap`, 1 `ProcessSmoothEdgeSequences`, 2
    /// `CollectTangentEdgeSequence` on `solvers` constant-radius solvers.
    Exec {
        sub: usize,
        solvers: usize,
        #[serde(rename = "box")]
        bx: BoxIn,
    },
    /// `SmTemporaryRadiusChange` on the box's first edge.
    TempRadius {
        variable: bool,
        start: f64,
        end: f64,
        #[serde(rename = "box")]
        bx: BoxIn,
    },
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push_map(v: &mut Vec<f64>, map: Option<[f64; 2]>) {
    v.push(flag(map.is_some()));
    v.extend(map.unwrap_or([0.0, 0.0]));
}

fn push_box(v: &mut Vec<f64>, bx: &BoxIn) {
    v.extend(bx.origin);
    v.extend(bx.size);
}

/// The shim record of the case.
fn record(i: &Inputs) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    let op = match i {
        Inputs::LawLinear { start, end, map } => {
            v.extend([*start, *end]);
            push_map(&mut v, *map);
            Op::LawLinear
        }
        Inputs::LawElliptic {
            start,
            end,
            outer,
            span,
            start_deriv,
            end_deriv,
            map,
        } => {
            v.extend([*start, *end, *outer, *span, *start_deriv, *end_deriv]);
            push_map(&mut v, *map);
            Op::LawElliptic
        }
        Inputs::SetParams {
            has_brep,
            bx,
            edges,
            xsect_types,
            radius_types,
            radius_values,
            laws,
            approx,
            ang,
            tan,
            xsect,
            continuity,
            thumbweight,
        } => {
            v.push(flag(*has_brep));
            push_box(&mut v, bx);
            v.push(edges.len() as f64);
            v.extend(edges.iter().map(|&e| e as f64));
            v.push(xsect_types.len() as f64);
            v.extend(xsect_types.iter().map(|&t| t as f64));
            v.push(radius_types.len() as f64);
            v.extend(radius_types.iter().map(|&t| t as f64));
            v.push(radius_values.len() as f64);
            v.extend(radius_values);
            v.push(flag(laws.is_some()));
            let laws = laws.as_deref().unwrap_or(&[]);
            v.push(laws.len() as f64);
            for law in laws {
                v.extend(law);
            }
            v.extend([
                *approx,
                *ang,
                *tan,
                *xsect,
                f64::from(*continuity),
                *thumbweight,
            ]);
            Op::SetParams
        }
        Inputs::Exec { sub, solvers, bx } => {
            v.extend([*sub as f64, *solvers as f64]);
            push_box(&mut v, bx);
            Op::Exec
        }
        Inputs::TempRadius {
            variable,
            start,
            end,
            bx,
        } => {
            v.extend([flag(*variable), *start, *end]);
            push_box(&mut v, bx);
            Op::TempRadius
        }
    };
    (op, v)
}

/// The status slot as a guard error, or the values.
fn values_output(o: Vec<f64>) -> Output {
    match o.first() {
        Some(&s) if s == NULL_DEREF => Output::err("null_deref"),
        Some(&s) if s == SHORT_BOX => Output::err("box_edge_count"),
        _ => Output::new().values("out", o),
    }
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, rec) = record(i);
    let o = ffi::eval(op, &rec).map_err(|e| harness(format!("c++: {e}")))?;
    if let Inputs::LawElliptic { .. } = i {
        // has_curve after the status and the edge map
        if o.get(3) == Some(&0.0) {
            return Ok(Output::err("law_without_curve"));
        }
    }
    Ok(values_output(o))
}

// ---- Rust side --------------------------------------------------------------------------

/// The objects of a case: B-rep 0 is the box (its edges and edge uses are those ids),
/// later B-reps are fillet B-reps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Handle {
    Brep(usize),
    Edge(EdgeId),
    Edgeuse(EdgeuseId),
    Generator(usize),
    Solver(usize),
    Law(usize),
}
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

struct Access {
    breps: Vec<Brep>,
    /// The executive's pseudo B-rep (`m_pPseudoBrep`): the solvers' fillet geometries.
    pseudo: FilletBrep,
    generators: Vec<FilletSurfaceGenerator>,
    solvers: Vec<ConstantRadiusAssistedFs>,
    laws: Vec<FilletLaw>,
}
fn missing_error() -> SolidError {
    SolidError::InvalidInput {
        operation: "harness::fillet_executive_p2",
        reason: "fixture has no such object",
    }
}
fn missing<T>() -> SolidResult<T> {
    Err(missing_error())
}
impl Access {
    fn brep(&self, h: Handle) -> SolidResult<&Brep> {
        match h {
            Handle::Brep(i) => self.breps.get(i).ok_or_else(missing_error),
            _ => missing(),
        }
    }
    fn brep_mut(&mut self, h: Handle) -> SolidResult<&mut Brep> {
        match h {
            Handle::Brep(i) => self.breps.get_mut(i).ok_or_else(missing_error),
            _ => missing(),
        }
    }
}

/// `target->MergeBrep(other, NULL, NULL, bAddAnalytics)` for this fixture, which keeps no
/// notification state: `topology::brep::p3::merge_brep` under the kernel's identities
/// (`with_kernel_callbacks`), `other` enrolled in the same identity domain first so its
/// objects never share a key with `target`'s.
fn merge_breps(
    k: &mut Kernel,
    target: &mut Brep,
    other: &mut Brep,
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
    b: &mut Brep,
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
    // The adapters SetFilletParameters and the empty-list executive members reach
    // before their executive p1 deferrals are real (below). The others reject
    // objects this fixture does not build; they are not UNPORTED calls.
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
        // (the fixture's handles name no topology objects: no copy maps of them)
        if copy_map.is_some() || reverse_map.is_some() || target == other {
            return missing();
        }
        let mut src = std::mem::take(self.brep_mut(other)?);
        let r = match self.brep_mut(target) {
            Ok(dst) => merge_breps(k, dst, &mut src, add_analytics),
            Err(e) => Err(e),
        };
        if let Ok(slot) = self.brep_mut(other) {
            *slot = src;
        }
        r
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
        let b = self.brep_mut(brep)?;
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
    fn corner_setback_distance(&self, _: Handle) -> SolidResult<f64> {
        missing()
    }
    fn new_brep(&mut self, _: &Kernel) -> SolidResult<Handle> {
        let handle = Handle::Brep(self.breps.len());
        self.breps.push(Brep::new());
        Ok(handle)
    }
    fn delete_brep(&mut self, _: Handle) -> SolidResult<()> {
        missing()
    }
    fn set_brep_tolerance(
        &mut self,
        k: &mut Kernel,
        brep: Handle,
        tolerance: f64,
        update_only_if_larger: bool,
        cascade_to_boundaries: bool,
    ) -> SolidResult<()> {
        solidcore::topology::brep::p2::set_tolerance(
            k,
            self.brep_mut(brep)?,
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
        // both executive call sites stitch every face (pOptFaces = NULL)
        if faces.is_some() {
            return missing();
        }
        StitchAttempt::from_stitch(solidcore::topology::brep::p4::stitch_faces(
            k,
            self.brep_mut(brep)?,
            tolerance,
            None,
            allow_spine_edges,
        ))
    }
    fn tolerance(&self, k: &Kernel, brep: Handle, approx: bool) -> SolidResult<f64> {
        let tolerance = self.brep(brep)?.tolerance();
        Ok(if approx {
            solidcore::geom::tol::approx_tol3d(k, 0.5 * tolerance)
        } else {
            tolerance
        })
    }
    fn editing_enabled(&self, brep: Handle) -> SolidResult<bool> {
        Ok(self.brep(brep)?.editing_enabled())
    }
    fn set_editing_enabled(&mut self, brep: Handle, value: bool) -> SolidResult<()> {
        self.brep_mut(brep)?.set_editing_enabled(value);
        Ok(())
    }
    fn edges(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        let b = self.brep(handle)?;
        if handle != Handle::Brep(0) && !b.edges().is_empty() {
            return missing();
        }
        Ok(b.edges().ids().map(Handle::Edge).collect())
    }
    fn edge_is_manifold(&self, _: Handle) -> SolidResult<bool> {
        missing()
    }
    fn vertices(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        if !self.brep(handle)?.vertices().is_empty() {
            return missing();
        }
        Ok(Vec::new())
    }
    fn faces(&self, handle: Handle) -> SolidResult<Vec<Handle>> {
        if !self.brep(handle)?.faces().is_empty() {
            return missing();
        }
        Ok(Vec::new())
    }
    fn blend_edgeuse(&mut self, k: &mut Kernel, edge: Handle) -> SolidResult<Option<Handle>> {
        let Handle::Edge(e) = edge else {
            return missing();
        };
        let b = self.brep_mut(Handle::Brep(0))?;
        Ok(edge::blend_edgeuse(k, b, e)?.map(Handle::Edgeuse))
    }
    fn edge_tolerance(&self, edge: Handle) -> SolidResult<f64> {
        let Handle::Edge(e) = edge else {
            return missing();
        };
        edge::tolerance(self.brep(Handle::Brep(0))?, e)
    }
    fn new_surface_generator(
        &mut self,
        _: &mut Kernel,
        generator: FilletSurfaceGenerator,
    ) -> SolidResult<Handle> {
        self.generators.push(generator);
        Ok(Handle::Generator(self.generators.len() - 1))
    }
    fn new_constant_radius_solver(
        &mut self,
        k: &mut Kernel,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
        radius: f64,
        edgeuse: Handle,
    ) -> SolidResult<Handle> {
        let Handle::Edgeuse(eu) = edgeuse else {
            return missing();
        };
        let id = FilletSolverId(
            u32::try_from(self.solvers.len()).map_err(|_| missing_error())?,
        );
        let b = self.breps.first_mut().ok_or_else(missing_error)?;
        let fs = constant_radius_from_edgeuse(
            k,
            b,
            &mut self.pseudo,
            id,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
            radius,
            eu,
            None,
        )?;
        self.solvers.push(fs);
        Ok(Handle::Solver(self.solvers.len() - 1))
    }
    fn set_surface_generator(&mut self, solver: Handle, generator: Handle) -> SolidResult<()> {
        let (Handle::Solver(s), Handle::Generator(g)) = (solver, generator) else {
            return missing();
        };
        let fsg = *self.generators.get(g).ok_or_else(missing_error)?;
        let fs = self.solvers.get_mut(s).ok_or_else(missing_error)?;
        fs.solver.set_fillet_surface_generator(Some(fsg));
        Ok(())
    }
}

/// The prefix of the p2 module's null-object operations (`SM_ERR_NULL_POINTER` raised
/// there, `blend::fillet_executive::p2: <what>`, marks a null dereference upstream,
/// `-2`).
const OP_P2: &str = "blend::fillet_executive::p2: ";

/// The `SmStatus` of a Rust error.
fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream {
            operation, code, ..
        } if *code == SM_ERR_NULL_POINTER && operation.starts_with(OP_P2) => NULL_DEREF,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -9.0,
    }
}

/// The shim's `SmExtent1d(mn, mx)`, which swaps reversed ends.
fn map_of(map: Option<[f64; 2]>) -> Option<Extent1d> {
    map.map(|[a, b]| Extent1d::from_min_max(a, b))
}

fn law_linear(start: f64, end: f64, map: Option<[f64; 2]>) -> Output {
    let law = FilletLaw::linear(start, end, map_of(map).as_ref());
    let FilletLawKind::Linear {
        start_radius: s,
        end_radius: e,
    } = law.kind
    else {
        return Output::err("not a linear law");
    };
    values_output(vec![
        0.0,
        law.edge_map.min(),
        law.edge_map.max(),
        s,
        e,
        law.edge_arc_length,
    ])
}

#[allow(clippy::too_many_arguments)]
fn law_elliptic(
    start: f64,
    end: f64,
    outer: f64,
    span: f64,
    start_deriv: f64,
    end_deriv: f64,
    map: Option<[f64; 2]>,
) -> Output {
    let law = match FilletLaw::bspline_elliptical(
        start,
        end,
        outer,
        span,
        start_deriv,
        end_deriv,
        map_of(map).as_ref(),
    ) {
        Ok(law) => law,
        Err(e) => {
            return Output::unported(&e).unwrap_or_else(|| Output::err("law_without_curve"));
        }
    };
    let FilletLawKind::BSpline {
        law_curve,
        extended_law,
    } = &law.kind
    else {
        return Output::err("not a B-spline law");
    };
    let mut o = vec![0.0, law.edge_map.min(), law.edge_map.max()];
    let Some(nurb) = law_curve.gw_nurb() else {
        return Output::err("law_without_curve");
    };
    o.push(1.0);
    let cpts = nurb.cpts();
    let knots = nurb.knots();
    o.push(cpts.len() as f64 - 1.0);
    o.push(nurb.degree() as f64);
    o.push(knots.len() as f64 - 1.0);
    for p in cpts {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(knots);
    o.push(flag(extended_law.is_some()));
    values_output(o)
}

/// The box B-rep (index 0), or the deferral of an inexact import.
fn box_access(bx: &BoxIn) -> Result<Result<Access, Output>, HarnessError> {
    let mut block = vec![2.0];
    push_box(&mut block, bx);
    let ex = stitch_ffi::eval(stitch_ffi::Op::Export, "", &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some((_, graph_at)) = counts(&ex) else {
        return Ok(Err(Output::err(format!(
            "brep_build_status_{}",
            ex.get(1).copied().unwrap_or(0.0)
        ))));
    };
    let (b, types) = super::topology_brep_p1::import(&ex, graph_at)?;
    if b.edges().len() != ffi::BOX_EDGES {
        // The shim reports the same box as `-4 n_edges`.
        return Ok(Err(Output::err("box_edge_count")));
    }
    if types
        .face_surface_types
        .iter()
        .any(|&t| t >= 0 && t != BOX_SURFACE)
        || types
            .edge_curve_types
            .iter()
            .any(|&t| t >= 0 && t != BOX_CURVE)
    {
        return Err(harness("box geometry is not planes and lines"));
    }
    Ok(Ok(Access {
        breps: vec![b],
        pseudo: FilletBrep::new(),
        generators: Vec::new(),
        solvers: Vec::new(),
        laws: Vec::new(),
    }))
}

/// The executive state the C++ constructor leaves (`SmFilletExecutive(ctx, target)`).
fn executive(target: Option<Handle>) -> FilletExecutive<Handle> {
    let mut executive = FilletExecutive {
        target,
        fillet: None,
        corners: vec![],
        solvers: vec![],
        topo_edges: vec![],
        edge_attribute: None,
        initialized: false,
        options: ExecutionOptions {
            piecewise_merge: false,
            topology_insertion: true,
            global_merge: false,
            trimming: true,
            classification: true,
        },
        error: ErrorInfo::default(),
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
    executive.intersector.ignore_tangency = true;
    executive
}

fn xsect_type(code: usize) -> Result<FilletSurfaceGeneratorType, HarnessError> {
    Ok(match code {
        0 => FilletSurfaceGeneratorType::Unknown,
        1 => FilletSurfaceGeneratorType::Linear,
        2 => FilletSurfaceGeneratorType::Circular,
        3 => FilletSurfaceGeneratorType::BlendCurve,
        _ => return Err(harness("cross-section type code out of range")),
    })
}

fn radius_type(code: usize) -> Result<FilletSolverType, HarnessError> {
    Ok(match code {
        0 => FilletSolverType::Unknown,
        1 => FilletSolverType::ConstRadius,
        2 => FilletSolverType::ConstDist,
        3 => FilletSolverType::VariableRadius,
        4 => FilletSolverType::SurfSurf,
        5 => FilletSolverType::CurveBased,
        6 => FilletSolverType::ConstRadiusAssisted,
        _ => return Err(harness("radius type code out of range")),
    })
}

/// A Rust error as the case's output: a deferral, or the status alone (as the shim
/// writes it for a failed set-up).
fn error_output(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| values_output(vec![status_of(e)]))
}

#[allow(clippy::too_many_arguments)]
fn set_params(
    has_brep: bool,
    bx: &BoxIn,
    edges: &[usize],
    xsect_types: &[usize],
    radius_types: &[usize],
    radius_values: &[f64],
    laws: Option<&[Vec<f64>]>,
    tolerances: [f64; 4],
    continuity: u32,
    thumbweight: f64,
) -> Result<Output, HarnessError> {
    let mut access = match box_access(bx)? {
        Ok(a) => a,
        Err(o) => return Ok(o),
    };
    let edges: Vec<Handle> = edges
        .iter()
        .map(|&e| Handle::Edge(EdgeId::from_index(e as u32)))
        .collect();
    let xsect_types = xsect_types
        .iter()
        .map(|&t| xsect_type(t))
        .collect::<Result<Vec<_>, _>>()?;
    let radius_types = radius_types
        .iter()
        .map(|&t| radius_type(t))
        .collect::<Result<Vec<_>, _>>()?;
    let mut law_handles = Vec::new();
    for law in laws.unwrap_or(&[]) {
        match law.as_slice() {
            [k, a, b] if *k == 1.0 => {
                access.laws.push(FilletLaw::linear(*a, *b, None));
                law_handles.push(Some(Handle::Law(access.laws.len() - 1)));
            }
            [k] if *k == 0.0 => law_handles.push(None),
            _ => return Err(harness("bad law entry")),
        }
    }
    let [approx_tol, ang_tol_deg, tan_tol_deg, xsect_tol] = tolerances;
    let params = FilletParameters {
        brep: has_brep.then_some(Handle::Brep(0)),
        edges: &edges,
        xsect_types: &xsect_types,
        radius_types: &radius_types,
        radius_values,
        var_rad_laws: laws.map(|_| law_handles.as_slice()),
        approx_tol,
        ang_tol_deg,
        tan_tol_deg,
        xsect_tol,
        continuity,
        thumbweight,
    };
    let mut k = Kernel::default();
    let mut ex = executive(None);
    let status = match ex.set_fillet_parameters(&mut k, &mut access, &params) {
        Ok(()) => 0.0,
        Err(e) if e.is_unported() => return Ok(error_output(&e)),
        Err(e) => status_of(&e),
    };
    Ok(values_output(vec![
        status,
        flag(ex.initialized),
        flag(ex.options.piecewise_merge),
        flag(ex.options.topology_insertion),
        flag(ex.options.global_merge),
        flag(ex.options.trimming),
        flag(ex.options.classification),
        flag(ex.target == Some(Handle::Brep(0))),
        ex.solvers.len() as f64,
        f64::from(ex.error.code),
    ]))
}

/// The shim's `load_solvers`: the box's first `n` edges as constant-radius solvers
/// (linear cross section, radius 1).
fn load_solvers(
    k: &mut Kernel,
    access: &mut Access,
    ex: &mut FilletExecutive<Handle>,
    n: usize,
) -> SolidResult<()> {
    let edges: Vec<Handle> = (0..n)
        .map(|e| Handle::Edge(EdgeId::from_index(e as u32)))
        .collect();
    let params = FilletParameters {
        brep: Some(Handle::Brep(0)),
        edges: &edges,
        xsect_types: &[FilletSurfaceGeneratorType::Linear],
        radius_types: &[FilletSolverType::ConstRadius],
        radius_values: &[1.0],
        var_rad_laws: None,
        approx_tol: 0.0,
        ang_tol_deg: 0.0,
        tan_tol_deg: 0.0,
        xsect_tol: 0.0,
        continuity: 1,
        thumbweight: 1.0,
    };
    ex.set_fillet_parameters(k, access, &params)
}

fn exec(sub: usize, n: usize, bx: &BoxIn) -> Result<Output, HarnessError> {
    let mut access = match box_access(bx)? {
        Ok(a) => a,
        Err(o) => return Ok(o),
    };
    let mut k = Kernel::default();
    let mut ex = executive(Some(Handle::Brep(0)));
    if n > 0 {
        if let Err(e) = load_solvers(&mut k, &mut access, &mut ex, n) {
            return Ok(error_output(&e));
        }
    }
    if sub > 0 && n > 1 {
        if let Err(e) = access.create_corners(&mut k, &mut ex, None) {
            return Ok(error_output(&e));
        }
    }
    let n_solvers = ex.solvers.len() as f64;
    let out = match sub {
        0 => {
            let fillet = access
                .new_brep(&k)
                .map_err(|e| harness(format!("new_brep: {e}")))?;
            ex.fillet = Some(fillet);
            ex.set_fillet_brep_edge_map(&mut k, &mut access)
                .map(|()| vec![0.0, 0.0, n_solvers])
        }
        1 => ex
            .process_smooth_edge_sequences(&mut k, &mut access)
            .map(|()| vec![0.0, 0.0, n_solvers]),
        _ => {
            let mut list: Vec<Option<Handle>> = ex.solvers.iter().copied().map(Some).collect();
            let mut seq = Vec::new();
            ex.collect_tangent_edge_sequence(&access, &mut list, &mut seq)
                .map(|()| vec![0.0, seq.len() as f64, list.len() as f64])
        }
    };
    Ok(match out {
        Ok(o) => values_output(o),
        Err(e) => error_output(&e),
    })
}

fn temp_radius(variable: bool, start: f64, end: f64, bx: &BoxIn) -> Result<Output, HarnessError> {
    let mut access = match box_access(bx)? {
        Ok(a) => a,
        Err(o) => return Ok(o),
    };
    access.laws.push(FilletLaw::linear(start, end, None));
    let law = [Some(Handle::Law(0))];
    let edges = [Handle::Edge(EdgeId::from_index(0))];
    let radius_types = [if variable {
        FilletSolverType::VariableRadius
    } else {
        FilletSolverType::ConstRadius
    }];
    let params = FilletParameters {
        brep: Some(Handle::Brep(0)),
        edges: &edges,
        xsect_types: &[FilletSurfaceGeneratorType::Linear],
        radius_types: &radius_types,
        radius_values: &[1.0],
        var_rad_laws: Some(&law),
        approx_tol: 0.0,
        ang_tol_deg: 0.0,
        tan_tol_deg: 0.0,
        xsect_tol: 0.0,
        continuity: 1,
        thumbweight: 1.0,
    };
    let mut k = Kernel::default();
    let mut ex = executive(Some(Handle::Brep(0)));
    match ex.set_fillet_parameters(&mut k, &mut access, &params) {
        Err(e) => Ok(error_output(&e)),
        // Reached once executive p1 lands: the side edge use and the change are not
        // modelled yet, so this is a harness error rather than a pass.
        Ok(()) => Err(harness(
            "temp_radius past SetFilletParameters is not modelled",
        )),
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    // Only what the shim may see runs on either side (counts and codes bounded).
    let (op, rec) = record(i);
    ffi::validate(op, &rec).map_err(|e| harness(format!("rejected: {e}")))?;
    match i {
        Inputs::LawLinear { start, end, map } => Ok(law_linear(*start, *end, *map)),
        Inputs::LawElliptic {
            start,
            end,
            outer,
            span,
            start_deriv,
            end_deriv,
            map,
        } => Ok(law_elliptic(
            *start,
            *end,
            *outer,
            *span,
            *start_deriv,
            *end_deriv,
            *map,
        )),
        Inputs::SetParams {
            has_brep,
            bx,
            edges,
            xsect_types,
            radius_types,
            radius_values,
            laws,
            approx,
            ang,
            tan,
            xsect,
            continuity,
            thumbweight,
        } => set_params(
            *has_brep,
            bx,
            edges,
            xsect_types,
            radius_types,
            radius_values,
            laws.as_deref(),
            [*approx, *ang, *tan, *xsect],
            *continuity,
            *thumbweight,
        ),
        Inputs::Exec { sub, solvers, bx } => exec(*sub, *solvers, bx),
        Inputs::TempRadius {
            variable,
            start,
            end,
            bx,
        } => temp_radius(*variable, *start, *end, bx),
    }
}

// ---- generated cases --------------------------------------------------------------------

fn gen_map(rng: &mut Rng) -> Option<[f64; 2]> {
    match rng.below(5) {
        0 => None,
        1 => Some([0.0, 1.0]),
        2 => {
            let a = rng.uniform(0.0, 1.0);
            Some([a, rng.uniform(a, 1.0)])
        }
        3 => {
            // reversed ends: SmExtent1d(mn, mx) swaps them
            let a = rng.uniform(-1.0, 1.0);
            Some([a, a - rng.uniform(0.0, 1.0)])
        }
        _ => {
            let a = rng.uniform(-1.0, 2.0);
            Some([a, a + rng.uniform(0.0, 1e-12)])
        }
    }
}

fn gen_radius(rng: &mut Rng) -> f64 {
    let scale = *rng.choose(&[1e-3, 1.0, 10.0, 1e4]).unwrap_or(&1.0);
    scale * rng.uniform(0.01, 3.0)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_executive_p2"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let map = gen_map(rng);
        let start = gen_radius(rng);
        let end = if rng.chance(0.2) {
            start
        } else {
            gen_radius(rng)
        };
        Some(if rng.chance(0.3) {
            json!({"op": "law_linear", "start": start, "end": end, "map": map})
        } else {
            let outer = if rng.chance(0.3) {
                rng.uniform(0.0, start.max(end))
            } else {
                start.max(end) * rng.uniform(1.0, 4.0)
            };
            let span = rng.uniform(0.05, 1.5);
            json!({"op": "law_elliptic", "start": start, "end": end, "outer": outer,
                   "span": span, "start_deriv": 0.0, "end_deriv": 0.0, "map": map})
        })
    }
}
