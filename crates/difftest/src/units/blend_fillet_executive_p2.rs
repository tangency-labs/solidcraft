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
//! Constant-radius solvers are retained and loaded into actual registered executives.
//! Remaining corner/sequence operations still report their missing implementation.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_custom_solver::ConstantRadiusAssistedFs;
use solidcore::blend::fillet_executive::p2::FilletParameters;
use solidcore::blend::fillet_executive::p1::ExecutiveObjects;
use solidcore::blend::fillet_geom::p1::{FilStatus, FilletEdgeType};
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
    Vertex(solidcore::topology::VertexId),
    Edgeuse(EdgeuseId),
    Generator(usize),
    Solver(usize),
    Corner(usize),
    Law(usize),
    Geom(solidcore::blend::fillet_geom::p1::FilletGeomId),
    Pseudo,
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

enum SolverEntry {
    ConstantRadius(ConstantRadiusAssistedFs),
    ConstantDistance(solidcore::blend::fillet_standard_solver::p1::ConstantDistanceFs),
    VariableRadius(Box<solidcore::blend::fillet_standard_solver::p1::VariableRadiusFs>),
    SurfaceSurface(solidcore::blend::fillet_standard_solver::p1::SurfaceSurfaceFs),
}

impl SolverEntry {
    fn base(&self) -> &solidcore::blend::fillet_solver::p1::FilletSolver {
        match self {
            Self::ConstantRadius(s) => &s.solver,
            Self::ConstantDistance(s) => &s.solver,
            Self::VariableRadius(s) => &s.curve_based.solver,
            Self::SurfaceSurface(s) => &s.base.solver,
        }
    }

    fn base_mut(&mut self) -> &mut solidcore::blend::fillet_solver::p1::FilletSolver {
        match self {
            Self::ConstantRadius(s) => &mut s.solver,
            Self::ConstantDistance(s) => &mut s.solver,
            Self::VariableRadius(s) => &mut s.curve_based.solver,
            Self::SurfaceSurface(s) => &mut s.base.solver,
        }
    }
}

struct Access {
    breps: Vec<Brep>,
    /// The executive's pseudo B-rep (`m_pPseudoBrep`): the solvers' fillet geometries.
    pseudo: FilletBrep,
    generators: Vec<FilletSurfaceGenerator>,
    solvers: Vec<SolverEntry>,
    corners: Vec<solidcore::blend::fillet_corner::p3::FilletCorner>,
    laws: Vec<FilletLaw>,
    edge_caches: BTreeMap<EdgeId, solidcore::curve::curve_cache::CurveCache<Handle>>,
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
    fn solver_surface(
        &self,
        solver: Handle,
        rail: usize,
    ) -> SolidResult<Option<&solidcore::blend::fillet_custom_solver::OffsetSurface>> {
        let solver = self.solver_base(solver)?;
        if let Some(geom) = solver.fg_for_surfaces {
            self.pseudo.geom(geom)?.offset_surface(rail, Some(solver))
        } else {
            Ok(solver.surface(rail)?.map(|slot| &slot.surface))
        }
    }
    fn brep(&self, h: Handle) -> SolidResult<&Brep> {
        match h {
            Handle::Brep(i) => self.breps.get(i).ok_or_else(missing_error),
            Handle::Pseudo => Ok(self.pseudo.brep()),
            _ => missing(),
        }
    }
    fn brep_mut(&mut self, h: Handle) -> SolidResult<&mut Brep> {
        match h {
            Handle::Brep(i) => self.breps.get_mut(i).ok_or_else(missing_error),
            Handle::Pseudo => Ok(self.pseudo.brep_mut()),
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
    fn solver_base(
        &self,
        solver: Handle,
    ) -> SolidResult<&solidcore::blend::fillet_solver::p1::FilletSolver> {
        let Handle::Solver(index) = solver else {
            return missing();
        };
        self.solvers
            .get(index)
            .map(SolverEntry::base)
            .ok_or_else(missing_error)
    }
    fn solver_base_mut(
        &mut self,
        solver: Handle,
    ) -> SolidResult<&mut solidcore::blend::fillet_solver::p1::FilletSolver> {
        let Handle::Solver(index) = solver else {
            return missing();
        };
        self.solvers
            .get_mut(index)
            .map(SolverEntry::base_mut)
            .ok_or_else(missing_error)
    }
    fn solver_has_surface(&self, solver: Handle, rail: usize) -> SolidResult<bool> {
        Ok(self.solver_surface(solver, rail)?.is_some())
    }
    fn edge_g1_knots(&mut self, k: &Kernel, edge: Handle) -> SolidResult<Vec<f64>> {
        const OP: &str = "SmFilletExecutive::LoadFilletSolver";
        let Handle::Edge(id) = edge else {
            return missing();
        };
        let curve = self
            .breps
            .first()
            .ok_or_else(missing_error)?
            .edge(id)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let knots = curve.knots(k, None)?.0;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.edge_caches.entry(id) {
            // The cache manager returns null on construction failure; LoadFilletSolver
            // applies NER to that pointer after GetKnots has succeeded.
            // the cache is the edge curve's own: its tolerance is the edge's
            let zone_tol_3d = edge::tolerance(
                self.breps.first().ok_or_else(missing_error)?,
                id,
            )?;
            let cache = solidcore::curve::curve::p5::make_curve_cache_with_zone_tol(
                k,
                curve,
                Some(edge),
                Some(zone_tol_3d),
            )
            .map_err(|error| {
                    if error.is_unported() {
                        error
                    } else {
                        SolidError::null_pointer(OP)
                    }
                })?;
            entry.insert(cache);
        }
        let cache = self.edge_caches.get(&id).ok_or_else(missing_error)?;
        cache
            .continuities()
            .iter()
            .enumerate()
            .filter(|(_, continuity)| {
                **continuity == solidcore::geom::geom_utility::p2::ContinuityType::G1
            })
            .map(|(index, _)| knots.get(index).copied().ok_or_else(missing_error))
            .collect()
    }
    fn edgeuse_vertex(&self, edgeuse: Handle) -> SolidResult<Option<Handle>> {
        let Handle::Edgeuse(id) = edgeuse else {
            return missing();
        };
        let b = self.brep(Handle::Brep(0))?;
        let vertexuse = b.edgeuse(id)?.vertexuse.ok_or_else(missing_error)?;
        Ok(b.vertexuse(vertexuse)?.vertex.map(Handle::Vertex))
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
    fn solver_geoms(&self, solver: Handle) -> SolidResult<Vec<Handle>> {
        Ok(self.solver_base(solver)?.fillet_geoms().into_iter().map(Handle::Geom).collect())
    }
    fn solver_geom(&self, solver: Handle, index: usize) -> SolidResult<Option<Handle>> {
        Ok(self.solver_base(solver)?.fillet_geoms.get(index).copied().map(Handle::Geom))
    }
    fn solver_edge(&self, solver: Handle) -> SolidResult<Handle> {
        const OP: &str = "blend::fillet_executive::p2: solver_edge";
        let eu = self
            .solver_base(solver)?
            .edgeuse(0)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        self.brep(Handle::Brep(0))?
            .edgeuse(eu)?
            .edge
            .map(Handle::Edge)
            .ok_or_else(|| SolidError::null_pointer(OP))
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
    fn edge_edgeuses(&self, edge: Handle) -> SolidResult<Vec<Handle>> {
        let Handle::Edge(e) = edge else {
            return missing();
        };
        let b = self.brep(Handle::Brep(0))?;
        Ok(edge::edgeuses(b, e, None, None)?.into_iter().map(Handle::Edgeuse).collect())
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
    fn edgeuse_edge(&self, edgeuse: Handle) -> SolidResult<Option<Handle>> {
        let Handle::Edgeuse(id) = edgeuse else { return missing(); };
        Ok(self.brep(Handle::Brep(0))?.edgeuse(id)?.edge.map(Handle::Edge))
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
    fn edge_vertices(&self, edge_handle: Handle) -> SolidResult<(Option<Handle>, Option<Handle>)> {
        let Handle::Edge(e) = edge_handle else {
            return missing();
        };
        let b = self.brep(Handle::Brep(0))?;
        let v1 = edge::start_vertex(b, e)?;
        let v2 = match v1 {
            Some(s) => edge::other_vertex(b, e, Some(s))?,
            None => None,
        };
        Ok((v1.map(Handle::Vertex), v2.map(Handle::Vertex)))
    }
    fn variable_radius_solver(
        &self,
        solver: Handle,
    ) -> SolidResult<Option<&solidcore::blend::fillet_standard_solver::p1::VariableRadiusFs>> {
        let Handle::Solver(idx) = solver else {
            return missing();
        };
        match self.solvers.get(idx).ok_or_else(missing_error)? {
            SolverEntry::VariableRadius(vr) => Ok(Some(vr.as_ref())),
            _ => Ok(None),
        }
    }
    fn variable_radius_solver_mut(
        &mut self,
        solver: Handle,
    ) -> SolidResult<Option<&mut solidcore::blend::fillet_standard_solver::p1::VariableRadiusFs>>
    {
        let Handle::Solver(idx) = solver else {
            return missing();
        };
        match self.solvers.get_mut(idx).ok_or_else(missing_error)? {
            SolverEntry::VariableRadius(vr) => Ok(Some(vr.as_mut())),
            _ => Ok(None),
        }
    }
    fn solver_edgeuse(&self, solver: Handle, rail: usize) -> SolidResult<Option<Handle>> {
        self.solver_base(solver)?
            .edgeuse(rail)
            .map(|edgeuse| edgeuse.map(Handle::Edgeuse))
    }
    fn solver_corner(&self, _: Handle, _: usize) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn solver_other_corner(&self, _: Handle, _: Handle) -> SolidResult<Option<Handle>> {
        missing()
    }
    fn solver_corner_in(
        &self,
        executive: &FilletExecutive<Handle>,
        solver: Handle,
        index: usize,
    ) -> SolidResult<Option<Handle>> {
        let b = self.brep(Handle::Brep(0))?;
        let Some(v) = self.solver_base(solver)?.vertex(b, index)? else {
            return Ok(None);
        };
        executive.fillet_corner_of_vertex(Handle::Vertex(v))
    }
    fn solver_other_corner_in(
        &self,
        executive: &FilletExecutive<Handle>,
        solver: Handle,
        corner: Handle,
    ) -> SolidResult<Option<Handle>> {
        let c0 = self.solver_corner_in(executive, solver, 0)?;
        let c1 = self.solver_corner_in(executive, solver, 1)?;
        if c0 == Some(corner) {
            Ok(c1)
        } else if c1 == Some(corner) {
            Ok(c0)
        } else {
            Ok(None)
        }
    }
    fn corner_tangent_solver_in(
        &self,
        k: &Kernel,
        executive: &FilletExecutive<Handle>,
        corner: Handle,
        solver: Handle,
    ) -> SolidResult<Option<Handle>> {
        let Handle::Corner(cid) = corner else {
            return missing();
        };
        let c = self.corners.get(cid).ok_or_else(missing_error)?;
        let b = self.brep(Handle::Brep(0))?;
        c.tangent_fillet_solver_with_lookup(k, b, solver, |eu| {
            executive.fillet_solver_of_edgeuse(Handle::Edgeuse(eu))
        })
    }
    fn smooth_solver_edge0_id(&self, solver: Handle) -> SolidResult<EdgeId> {
        let Handle::Edge(e) = self.solver_edge(solver)? else {
            return missing();
        };
        Ok(e)
    }
    fn smooth_target_brep(&self) -> SolidResult<&Brep> {
        self.brep(Handle::Brep(0))
    }
    fn smooth_with_target_mut<T>(
        &mut self,
        edges: &[EdgeId],
        f: impl FnOnce(&mut Brep) -> SolidResult<T>,
    ) -> SolidResult<T> {
        for &e in edges {
            self.edge_caches.remove(&e);
        }
        let b = self.breps.first_mut().ok_or_else(missing_error)?;
        f(b)
    }
    fn smooth_set_edge_curve(
        &mut self,
        k: &mut Kernel,
        edge_id: EdgeId,
        curve: solidcore::curve::curve::Curve,
    ) -> SolidResult<()> {
        use solidcore::topology::brep::p3 as bp3;
        self.edge_caches.remove(&edge_id);
        let b = self.breps.first_mut().ok_or_else(missing_error)?;
        bp3::with_kernel_callbacks(k, b, |_k, b, env| {
            let _ = edge::set_curve(
                b,
                edge_id,
                Some(curve),
                edge::SetCurveOptions {
                    delete_old_curve: true,
                    dbg_warn_leaks: true,
                    call_notify: true,
                },
                env,
                [None, None],
            )?;
            Ok(())
        })
    }
    fn smooth_solver_original_curve(
        &self,
        solver: Handle,
    ) -> SolidResult<Option<&solidcore::curve::curve::Curve>> {
        let Handle::Solver(idx) = solver else {
            return missing();
        };
        Ok(match self.solvers.get(idx).ok_or_else(missing_error)? {
            SolverEntry::VariableRadius(vr) => vr.curve_based.original_curve(),
            _ => None,
        })
    }
    fn smooth_set_solver_original_curve(
        &mut self,
        solver: Handle,
        curve: solidcore::curve::curve::Curve,
    ) -> SolidResult<()> {
        let Handle::Solver(idx) = solver else {
            return missing();
        };
        if let SolverEntry::VariableRadius(vr) =
            self.solvers.get_mut(idx).ok_or_else(missing_error)?
        {
            vr.curve_based.set_original_curve(Some(curve));
        }
        Ok(())
    }
    fn smooth_set_solver_law_interval(
        &mut self,
        solver: Handle,
        interval: Extent1d,
    ) -> SolidResult<()> {
        let Handle::Solver(idx) = solver else {
            return missing();
        };
        if let SolverEntry::VariableRadius(vr) =
            self.solvers.get_mut(idx).ok_or_else(missing_error)?
        {
            vr.set_fillet_law_interval(interval);
        }
        Ok(())
    }
    fn create_corners(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<Handle>,
        error: Option<&mut ErrorInfo<Handle>>,
    ) -> SolidResult<()> {
        let b = self.breps.first_mut().ok_or_else(missing_error)?;
        let solvers = &self.solvers;
        let corners = &mut self.corners;
        executive.create_fillet_corners(
            k,
            |k, executive, vertex, edgeuses| {
                let Some(Handle::Vertex(v)) = vertex else {
                    return missing();
                };
                let eus = edgeuses
                    .iter()
                    .map(|h| match *h {
                        Handle::Edgeuse(eu) => Ok(eu),
                        _ => missing(),
                    })
                    .collect::<SolidResult<Vec<_>>>()?;
                let made =
                    solidcore::blend::fillet_corner::p3::FilletCorner::create_with_solver_lookup(
                        k,
                        b,
                        v,
                        &eus,
                        |eu| {
                            let Some(Handle::Solver(idx)) =
                                executive.fillet_solver_of_edgeuse(Handle::Edgeuse(eu))
                            else {
                                return missing();
                            };
                            solvers
                                .get(idx)
                                .map(SolverEntry::base)
                                .ok_or_else(missing_error)
                        },
                    )?;
                let Some(corner) = made else {
                    return Ok(None);
                };
                let id = corners.len();
                corners.push(corner);
                Ok(Some(Handle::Corner(id)))
            },
            error,
        )
    }
    /// `SmFilletExecutive::DoPiecewiseMerge`: this runner drives the executive's set-up
    /// stages only and never builds a fillet B-rep to merge piecewise.
    fn piecewise_merge(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
    ) -> SolidResult<()> {
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
        self.solvers.push(SolverEntry::ConstantRadius(fs));
        Ok(Handle::Solver(self.solvers.len() - 1))
    }
    fn new_fillet_solver(
        &mut self,
        k: &mut Kernel,
        spec: &solidcore::blend::fillet_executive::p2::SolverSpec<'_, Handle>,
    ) -> SolidResult<Handle> {
        use solidcore::blend::fillet_custom_solver::SurfaceId;
        use solidcore::blend::fillet_executive::p2::SolverSpec;
        use solidcore::blend::fillet_standard_solver::p1::{
            ConstantDistanceFs, SurfaceSurfaceFs, VariableRadiusFs,
        };

        match *spec {
            SolverSpec::ConstantRadius {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            } => self.new_constant_radius_solver(
                k,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            ),
            SolverSpec::ConstantDistance {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            } => {
                let Handle::Edgeuse(eu) = edgeuse else {
                    return missing();
                };
                let id = FilletSolverId(
                    u32::try_from(self.solvers.len()).map_err(|_| missing_error())?,
                );
                let b = self.breps.first_mut().ok_or_else(missing_error)?;
                let fs = ConstantDistanceFs::from_edgeuse(
                    k,
                    b,
                    &mut self.pseudo,
                    id,
                    approx_tol,
                    ang_tol_rad,
                    tan_tol_rad,
                    radius,
                    eu,
                )?;
                self.solvers.push(SolverEntry::ConstantDistance(fs));
                Ok(Handle::Solver(self.solvers.len() - 1))
            }
            SolverSpec::VariableRadius {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
                law,
                reverse,
            } => {
                let (Handle::Edgeuse(eu), Handle::Law(law_idx)) = (edgeuse, law) else {
                    return missing();
                };
                let law = self.laws.get(law_idx).ok_or_else(missing_error)?.clone();
                let id = FilletSolverId(
                    u32::try_from(self.solvers.len()).map_err(|_| missing_error())?,
                );
                let b = self.breps.first_mut().ok_or_else(missing_error)?;
                let fs = VariableRadiusFs::from_edgeuse(
                    k,
                    b,
                    &mut self.pseudo,
                    id,
                    approx_tol,
                    ang_tol_rad,
                    tan_tol_rad,
                    radius,
                    eu,
                    law,
                    reverse,
                )?;
                self.solvers.push(SolverEntry::VariableRadius(Box::new(fs)));
                Ok(Handle::Solver(self.solvers.len() - 1))
            }
            SolverSpec::SurfaceSurface {
                tolerance,
                ang_tol,
                tan_tol,
                radii,
                surfaces,
                orientations,
                guesses,
                trim_type,
                reverse_trim,
            } => {
                let id = FilletSolverId(
                    u32::try_from(self.solvers.len()).map_err(|_| missing_error())?,
                );
                let pair = [
                    (surfaces[0].clone(), SurfaceId::Free(0)),
                    (surfaces[1].clone(), SurfaceId::Free(1)),
                ];
                let mut fs = SurfaceSurfaceFs::new(
                    k,
                    &mut self.pseudo,
                    id,
                    tolerance,
                    ang_tol,
                    tan_tol,
                    radii,
                    pair,
                    orientations,
                    guesses,
                )?;
                fs.set_fillet_trim_type(trim_type);
                fs.base.solver.set_reverse_trim(reverse_trim);
                self.solvers.push(SolverEntry::SurfaceSurface(fs));
                Ok(Handle::Solver(self.solvers.len() - 1))
            }
        }
    }
    fn set_surface_generator(&mut self, solver: Handle, generator: Handle) -> SolidResult<()> {
        let (Handle::Solver(s), Handle::Generator(g)) = (solver, generator) else {
            return missing();
        };
        let fsg = *self.generators.get(g).ok_or_else(missing_error)?;
        let fs = self.solvers.get_mut(s).ok_or_else(missing_error)?;
        fs.base_mut().set_fillet_surface_generator(Some(fsg));
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

/// The box B-rep (index 0): `SmApiCreateBox(P, l, w, h)` (the `heal.stitch_p2` shim's
/// box block) made by the Rust kernel in the shim's traversal order
/// ([`crate::own_builder`]), or the output of a failed build. No C++ state reaches the
/// Rust side.
fn box_access(bx: &BoxIn) -> Result<Result<Access, Output>, HarnessError> {
    use crate::own_builder::{build, Built};
    let src = super::topology_edgeuse_p1::BrepIn::Box {
        origin: bx.origin,
        size: bx.size,
    };
    let b = match build(&mut Kernel::default(), &src, false, false)? {
        Some(Built::Brep(b)) => *b,
        Some(Built::Failed(e)) => return Ok(Err(crate::own_builder::failed_output(&e))),
        None => return Err(harness("box source not built")),
    };
    if b.edges().len() != ffi::BOX_EDGES {
        // The shim reports the same box as `-4 n_edges`.
        return Ok(Err(Output::err("box_edge_count")));
    }
    Ok(Ok(Access {
        breps: vec![b],
        pseudo: FilletBrep::new(),
        generators: Vec::new(),
        solvers: Vec::new(),
        corners: Vec::new(),
        laws: Vec::new(),
        edge_caches: BTreeMap::new(),
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
        pseudo_brep: None,
        surface_generators: vec![],
        filleted_vertices: Vec::new(),
        vertex_corner_map: BTreeMap::new(),
        edgeuse_solver_map: BTreeMap::new(),
        self_intersection_handler: None,
        blend_curve_at_thirds: false,
        mark_corner_patches: false,
        cliff_overflow: false,
        open_ends: Vec::new(),
        overflow_first: false,
        classify_only: false,
        overflow_edges: Vec::new(),
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

/// `SmFilletSolverType` (`SmFilletSolver.h:47`) from its code.
fn radius_type(code: usize) -> Result<FilletSolverType, HarnessError> {
    Ok(match code {
        0 => FilletSolverType::Unknown,
        1 => FilletSolverType::SurfSurf,
        2 => FilletSolverType::ConstRadius,
        3 => FilletSolverType::VariableRadius,
        4 => FilletSolverType::ConstDist,
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
    // the shim drops an index past the box's edges
    let edge_count = access
        .brep(Handle::Brep(0))
        .map_err(|e| harness(format!("box: {e}")))?
        .edges()
        .len();
    let edges: Vec<Handle> = edges
        .iter()
        .filter(|&&e| e < edge_count)
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
    let mut executive = executive(None);
    let ex = &mut executive;
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

fn push_bspline_curve(o: &mut Vec<f64>, crv: Option<&solidcore::curve::curve::Curve>) {
    use solidcore::curve::curve::CurveKind;
    let bsc = crv.and_then(|c| match c.kind() {
        CurveKind::BSpline(b) => Some(std::borrow::Cow::Borrowed(b)),
        CurveKind::Line(l) => l.base_with_nurb().ok(),
        CurveKind::Ellipse(e) => Some(std::borrow::Cow::Borrowed(e.base())),
        CurveKind::Parabola(p) => Some(std::borrow::Cow::Borrowed(p.base())),
        CurveKind::Hyperbola(h) => Some(std::borrow::Cow::Borrowed(h.base())),
        _ => None,
    });
    let Some(nurb) = bsc.as_deref().and_then(|b| b.gw_nurb()) else {
        o.push(0.0);
        return;
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
}

fn exec(sub: usize, n: usize, bx: &BoxIn) -> Result<Output, HarnessError> {
    let mut access = match box_access(bx)? {
        Ok(a) => a,
        Err(o) => return Ok(o),
    };
    let mut k = Kernel::default();
    let mut executive = executive(Some(Handle::Brep(0)));
    let ex = &mut executive;
    if sub >= 3 {
        use solidcore::geom::attribute::ObjectKey;
        use solidcore::geom::object::ObjectRef;
        use solidcore::topology::brep::{p3 as bp3, p9 as bp9};

        let all_edges: Vec<EdgeId> = access
            .brep(Handle::Brep(0))
            .map_err(|e| harness(e.to_string()))?
            .edges()
            .ids()
            .collect();
        let e0 = *all_edges.first().ok_or_else(|| harness("empty box edges"))?;
        let ivl = access
            .brep(Handle::Brep(0))
            .and_then(|b| edge::interval(b, e0))
            .map_err(|e| harness(e.to_string()))?;
        let split_t = 0.35 * ivl.min() + 0.65 * ivl.max();
        let split_res = {
            let b = access
                .brep_mut(Handle::Brep(0))
                .map_err(|e| harness(e.to_string()))?;
            bp3::with_kernel_callbacks(&mut k, b, |k, b, env| {
                bp9::make_vertex_split_edge(k, b, e0, split_t, env, &|eid, kind| ObjectRef {
                    key: ObjectKey(((kind as u64) << 32) | u64::from(eid.index())),
                    type_code: kind,
                })
            })
        };
        let split = match split_res {
            Ok(s) => s,
            Err(e) => return Ok(error_output(&e)),
        };
        let Some([e1, e2]) = split.edges else {
            return Ok(values_output(vec![f64::from(split.status())]));
        };
        {
            let b = access
                .brep_mut(Handle::Brep(0))
                .map_err(|e| harness(e.to_string()))?;
            if let Err(e) = edge::reverse_orientation(&k, b, e2) {
                return Ok(error_output(&e));
            }
            let e2_ivl = Extent1d::from_min_max(0.0, 2.0);
            if let Some(crv) = b
                .edge_mut(e2)
                .map_err(|e| harness(e.to_string()))?
                .curve
                .as_mut()
            {
                let _ = crv.edit_parameterization(&k, &e2_ivl);
            }
            b.edge_mut(e2)
                .map_err(|e| harness(e.to_string()))?
                .interval = e2_ivl;
        }
        let mut edges = vec![Handle::Edge(e2), Handle::Edge(e1)];
        for &eid in all_edges.iter().skip(1).take(n.saturating_sub(2)) {
            edges.push(Handle::Edge(eid));
        }
        let mut law_handles = Vec::new();
        if sub == 5 {
            for i in 0..edges.len() {
                access.laws.push(FilletLaw::linear(1.0, 2.0, None));
                law_handles.push(Some(Handle::Law(i)));
            }
        }
        let radius_types = [if sub == 5 {
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
            var_rad_laws: if sub == 5 { Some(&law_handles) } else { None },
            approx_tol: 0.0,
            ang_tol_deg: 0.0,
            tan_tol_deg: 0.0,
            xsect_tol: 0.0,
            continuity: 1,
            thumbweight: 1.0,
        };
        if let Err(e) = ex.set_fillet_parameters(&mut k, &mut access, &params) {
            return Ok(if e.is_unported() {
                error_output(&e)
            } else {
                values_output(vec![status_of(&e)])
            });
        }
        if let Err(e) = access.create_corners(&mut k, ex, None) {
            return Ok(if e.is_unported() {
                error_output(&e)
            } else {
                values_output(vec![status_of(&e)])
            });
        }
        if sub == 3 {
            let mut list: Vec<Option<Handle>> = ex.solvers.iter().copied().map(Some).collect();
            let mut seq = Vec::new();
            return match ex.collect_tangent_edge_sequence_narrow(&k, &access, &mut list, &mut seq) {
                Ok(()) => Ok(values_output(vec![0.0, seq.len() as f64, list.len() as f64])),
                Err(e) if e.is_unported() => Ok(error_output(&e)),
                Err(e) => Ok(values_output(vec![
                    status_of(&e),
                    seq.len() as f64,
                    list.len() as f64,
                ])),
            };
        }
        if let Err(e) = ex.process_smooth_edge_sequences(&mut k, &mut access) {
            return Ok(if e.is_unported() {
                error_output(&e)
            } else {
                values_output(vec![status_of(&e)])
            });
        }
        let b = access
            .brep(Handle::Brep(0))
            .map_err(|e| harness(e.to_string()))?;
        let ivl1 = edge::interval(b, e1).map_err(|e| harness(e.to_string()))?;
        let ivl2 = edge::interval(b, e2).map_err(|e| harness(e.to_string()))?;
        let mut o = vec![0.0, ivl1.min(), ivl1.max(), ivl2.min(), ivl2.max()];
        if sub == 5 {
            let vr0 = access
                .variable_radius_solver(Handle::Solver(0))
                .map_err(|e| harness(e.to_string()))?;
            let vr1 = access
                .variable_radius_solver(Handle::Solver(1))
                .map_err(|e| harness(e.to_string()))?;
            o.push(vr0.map_or(-1.0, |v| v.fillet_law_interval().min()));
            o.push(vr0.map_or(-1.0, |v| v.fillet_law_interval().max()));
            o.push(vr1.map_or(-1.0, |v| v.fillet_law_interval().min()));
            o.push(vr1.map_or(-1.0, |v| v.fillet_law_interval().max()));
            push_bspline_curve(
                &mut o,
                b.edge(e1)
                    .map_err(|e| harness(e.to_string()))?
                    .curve
                    .as_ref(),
            );
            push_bspline_curve(&mut o, vr0.and_then(|v| v.curve_based.original_curve()));
        } else {
            push_bspline_curve(
                &mut o,
                b.edge(e1)
                    .map_err(|e| harness(e.to_string()))?
                    .curve
                    .as_ref(),
            );
            push_bspline_curve(
                &mut o,
                b.edge(e2)
                    .map_err(|e| harness(e.to_string()))?
                    .curve
                    .as_ref(),
            );
        }
        return Ok(values_output(o));
    }
    if n > 0 {
        if let Err(e) = load_solvers(&mut k, &mut access, ex, n) {
            return Ok(error_output(&e));
        }
    }
    if sub > 0 && n > 1 {
        if let Err(e) = access.create_corners(&mut k, ex, None) {
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
            ex.collect_tangent_edge_sequence_narrow(&k, &access, &mut list, &mut seq)
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
    let mut executive = executive(Some(Handle::Brep(0)));
    let ex = &mut executive;
    if let Err(e) = ex.set_fillet_parameters(&mut k, &mut access, &params) {
        Ok(error_output(&e))
    } else {
        let solver = ex
            .solvers
            .first()
            .copied()
            .ok_or_else(|| harness("no retained solver"))?;
        let b = access
            .brep(Handle::Brep(0))
            .map_err(|e| harness(e.to_string()))?;
        let first =
            edge::start_vertex(b, EdgeId::from_index(0)).map_err(|e| harness(e.to_string()))?;
        let mut side = None;
        for (id, _) in b.edges().iter().skip(1) {
            let start = edge::start_vertex(b, id).map_err(|e| harness(e.to_string()))?;
            let end = edge::other_vertex(b, id, start).map_err(|e| harness(e.to_string()))?;
            if start == first || end == first {
                side = Some(Handle::Edge(id));
                break;
            }
        }
        let side = match side {
            Some(side) => access
                .blend_edgeuse(&mut k, side)
                .map_err(|e| harness(e.to_string()))?,
            None => None,
        };
        let Some(side) = side else {
            return Ok(error_output(&SolidError::sm_err(
                "harness::fillet_executive_p2::temporary_radius_change",
            )));
        };
        let read_linear_radii = |acc: &Access| -> SolidResult<(f64, f64)> {
            let vrfs = acc
                .variable_radius_solver(solver)?
                .ok_or_else(missing_error)?;
            match vrfs.fillet_law().kind {
                FilletLawKind::Linear {
                    start_radius,
                    end_radius,
                } => Ok((start_radius, end_radius)),
                _ => missing(),
            }
        };
        if variable {
            let during = match solidcore::blend::fillet_executive::p2::with_temporary_radius_change(
                &mut access,
                solver,
                0,
                side,
                |acc| read_linear_radii(acc),
            ) {
                Ok(pair) => pair,
                Err(e) => return Ok(error_output(&e)),
            };
            let after = read_linear_radii(&access).map_err(|e| harness(e.to_string()))?;
            Ok(values_output(vec![
                0.0, during.0, during.1, after.0, after.1,
            ]))
        } else {
            match solidcore::blend::fillet_executive::p2::with_temporary_radius_change(
                &mut access,
                solver,
                0,
                side,
                |_| Ok(()),
            ) {
                Ok(()) => Ok(values_output(vec![0.0])),
                Err(e) => Ok(error_output(&e)),
            }
        }
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

#[path = "blend_fillet_executive_p2/loader.rs"]
mod loader;
pub(crate) use loader::{loader_cpp, loader_rust, LoaderScript};

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::blend::fillet_executive::p2::{
        with_temporary_radius_change, with_temporary_radius_change_with_fixes, TemporaryRadiusChange,
    };

    #[test]
    fn temporary_radius_setup_error_never_enters_the_filleting_body() {
        let mut access = Access {
            breps: vec![Brep::new()],
            pseudo: FilletBrep::new(),
            generators: vec![],
            solvers: vec![],
            corners: vec![],
            laws: vec![FilletLaw::linear(1.0, 2.0, None)],
            edge_caches: BTreeMap::new(),
        };
        let solver = Handle::Solver(0);
        let side = Handle::Edgeuse(EdgeuseId::from_index(0));
        let expected = missing_error();
        // The additive API keeps the existing constructor's pinned default.
        assert!(TemporaryRadiusChange::new(&mut access, solver, 0, side).is_ok());
        let mut default_body_called = false;
        // Revert can fail for this intentionally absent solver; the body still ran.
        let _ = with_temporary_radius_change(&mut access, solver, 0, side, |_| {
            default_body_called = true;
            Ok(())
        });
        assert!(default_body_called);
        let mut fixes = solidcore::kernel::UpstreamFixes::default();
        fixes.temporary_radius_setup_errors = true;
        let error =
            TemporaryRadiusChange::new_with_fixes(&mut access, solver, 0, side, &fixes).unwrap_err();
        assert_eq!(error, expected);

        let mut body_called = false;
        let error = with_temporary_radius_change_with_fixes(
            &mut access,
            solver,
            0,
            side,
            &fixes,
            |_| {
                body_called = true;
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error, expected);
        assert!(!body_called);
        assert_eq!(access.laws.len(), 1);
        assert!(matches!(
            access.laws[0].kind,
            FilletLawKind::Linear {
                start_radius: 1.0,
                end_radius: 2.0
            }
        ));
        assert!(access.solvers.is_empty());
    }
}
