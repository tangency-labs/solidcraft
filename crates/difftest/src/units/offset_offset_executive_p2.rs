//! Runner for `offset.offset_executive_p2` (PR #201).
//!
//! Upstream: `source/SMLib/src/SmOffsetExecutive.cpp`,
//! `source/SM_API/src/SmApiBrep.cpp`; native adapter:
//! `harness/cpp/shim/offset_offset_executive_p2.cpp` (snapshot protocol v1).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`Runner`] static offset/shell | `SmOffsetExecutive::OffsetBrep`, `ShellBrep` |
//! | [`Runner`] member solid/extended | `DoSolidOffset`, `DoExtendedOffset` |
//! | [`Runner`] API shell/offset | `SmApiShellBrepFull`, `SmApiOffsetBrepFull` |
//!
//! Deviations: independent serde/native DTO conversion and independent Rust input
//! construction, not native operation replay. Ordinary operation statuses are
//! INTEGER observations, not `Output::error` (which would suppress geometry/state
//! comparison). ONLY `ffi::Outcome::Rejected` is a constructor rejection; all FFI
//! guard/transport/layout errors are harness errors. Actual kernel `Unported`
//! errors propagate unchanged, including calls before tolerance guards/copying.
//! No topology clone stands in for the API shell's missing copy constructor.
//!
//! Exact current parity scope: member null (1006), API null (1007), and API shell
//! foreign/null selections (1007), including mixed/duplicate source-face-zero
//! selections and independent arenas whose local face-zero ids collide. Compare
//! all exposed executive scalar getters/flags, Boolean context before/after,
//! ordered selector identities, input/result lifetime/ownership and presence.
//! Input geometry projection compares B-rep tolerance, representable counts and
//! analytic kinds; sheets additionally compare face/natural domains and three
//! evaluated points/normals; wires compare natural/trim intervals, endpoint
//! evaluations and vertex bounds. The constructor's infinite region is explicitly
//! designated void for prefix selection; full region/editing metadata, per-entity
//! effective tolerances and traversal adjacency are not certified by this projection.
//! The COMPLETE native integer/numeric input blocks must remain bitwise unchanged
//! on those early paths; independently, Rust checks its complete Debug-visible
//! arena state and samples before/after the SAME owned input, not a reconstruction.
//!
//! Box inputs are built as natively, with the real primitive constructor
//! (`SmApiCreateBox`, compared in `primitive.api_primitives`) and then the fixture
//! tolerance, so the offset sees the native radial order and face-use orientations.
//! Box input parity remains counts of faces/edges/vertices, analytic kinds, vertex
//! bounds, provenance and purity.
//! Cylinder and sphere inputs are built the same way, with the real
//! `SmApiCreateCylinder` (`SmBrep*&` overload) and `SmApiCreateSphere` and their ports,
//! and projected like the box. Their seam edges are tangent sectors, so they reach the
//! extend-corner pass of `DoExtendedOffset`. Input face and edge kinds use the shim's
//! `IsKindOf` classes on both sides (a cone or sphere is an `SmBSplineSurface`).
//! Sheet boundary scaffolding follows p1's typed rectangular fixture pattern;
//! its Line boundaries are not claimed equivalent to native isocurve kinds.
//! Native sheet/wire construction must preserve the projected counts (1/1/4/8/4
//! and 0/0/1/2/2); otherwise the fixture is rejected before selector resolution.
//! Unprojected shell/region counts and arbitrary size/tolerance cutoffs are not guards.
//! No topology is globally sorted. A returned B-rep and a retained offset temporary
//! are compared as a projection (every topology count, sorted face-surface and
//! edge-curve kind multisets, tolerance and vertex bounds), not by traversal order
//! or region/shell identity. A result that is the consumed source (identified by
//! the source's B-rep key) reports native lifetime/origin 2, and the live source
//! after the call is that result. The Rust purity check ignores the edge uses' UV
//! trim curves, which offset queries may make lazily and the native block omits.
//! Static/member shell lists currently support source faces only; nullable and
//! foreign lists are exercised through the API's origin-qualified interface.
//! Positive B-rep tolerances avoid the unported automatic `SetTolerance` path;
//! member approximation tolerance still covers zero, negative and -1 sentinel.
//! All numeric keys have ZERO absolute floor; signed-zero inputs are retained.
//! A fixture case whose native vertex positions follow heap addresses (upstream
//! `SmRelation` walks in address-hash order) sets `heap_ordered_vertices`, which omits
//! only the result B-reps' vertex bounds on both sides; see [`Inputs::heap_ordered_vertices`].
//!
//! This unit is necessarily PARTIAL: current positive geometry operations reach
//! genuine missing manifold/copy/face-construction/shrink/merge/stitch work.
//! Empty rounded offset runs through `ShrinkGeometry` to its result; static shell queries manifold
//! BEFORE its strict radius guard; valid API shell selections copy BEFORE shelling.
//! Fixtures retain those native paths for future wiring, not manufactured success.
//! Source test references are reductions of tests read at
//! `tests/SM_API_test/src/TestSmBrep.cpp:2544-2758` and
//! `tests/prog_test/src/smOffsetTest.cpp:190-295`, not volume parity assertions.
//! No new input-dependent recursion is introduced.


use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::{FaceKey, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::knots::Side;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::{
    self, OffsetNotifications, OffsetOptions, ShellCopyIdentities, ShellOptions, SolidOffsetOptions,
};
use solidcore::query::api_queries::BrepRef;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::sweep::topology_sweep::store::SweepIdentities;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::topology::Topology;
use solidcore::topology::{
    Brep, BrepId, Edge, EdgeId, Edgeuse, EdgeuseOwner, FaceId, Region, RegionId, Shell, ShellId,
    Vertex, VertexId, Vertexuse, VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::offset_offset_executive_p2 as ffi;

use crate::runner::default_cases;
use crate::util::{p2 as point2, v3};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// Minimum reproducible generated inputs, even when `--seeds 0` is requested.
pub const MIN_GENERATED: usize = 240;
/// The six real native entrypoints and their Rust counterparts.
pub struct Runner;

type HResult<T> = Result<T, HarnessError>;
type RResult<T> = Result<T, RunError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    StaticOffset,
    StaticShell,
    Solid,
    Extended,
    ApiShell,
    ApiOffset,
}
impl Operation {
    const fn native(self) -> ffi::Operation {
        match self {
            Self::StaticOffset => ffi::Operation::StaticOffset,
            Self::StaticShell => ffi::Operation::StaticShell,
            Self::Solid => ffi::Operation::Solid,
            Self::Extended => ffi::Operation::Extended,
            Self::ApiShell => ffi::Operation::ApiShell,
            Self::ApiOffset => ffi::Operation::ApiOffset,
        }
    }
    const fn member(self) -> bool {
        matches!(self, Self::Solid | Self::Extended)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Fixture {
    Empty,
    Wire {
        start: [f64; 3],
        end: [f64; 3],
        domain: [f64; 2],
        interval: [f64; 2],
    },
    PlaneSheet {
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Box {
        origin: [f64; 3],
        size: [f64; 3],
    },
    /// `SmApiCreateCylinder(origin, radius, height)`: base, top and a side face closed
    /// along a seam edge (the edge's two uses form a tangent sector).
    Cylinder {
        origin: [f64; 3],
        radius: f64,
        height: f64,
    },
    /// `SmApiCreateSphere(origin, radius)`.
    Sphere { origin: [f64; 3], radius: f64 },
}
impl Fixture {
    fn native(&self) -> ffi::Fixture {
        match *self {
            Self::Empty => ffi::Fixture::EmptyBrep,
            Self::Wire {
                start,
                end,
                domain,
                interval,
            } => ffi::Fixture::Wire {
                start,
                end,
                domain,
                interval,
            },
            Self::PlaneSheet {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            } => ffi::Fixture::PlaneSheet {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            },
            Self::Box { origin, size } => ffi::Fixture::Box { origin, size },
            Self::Cylinder {
                origin,
                radius,
                height,
            } => ffi::Fixture::Cylinder {
                origin,
                radius,
                height,
            },
            Self::Sphere { origin, radius } => ffi::Fixture::Sphere { origin, radius },
        }
    }
    /// Built by a native primitive constructor (and its Rust port).
    const fn primitive(&self) -> bool {
        matches!(
            self,
            Self::Box { .. } | Self::Cylinder { .. } | Self::Sphere { .. }
        )
    }
    const fn face_count(&self) -> usize {
        match self {
            Self::Empty | Self::Wire { .. } => 0,
            Self::PlaneSheet { .. } | Self::Sphere { .. } => 1,
            Self::Box { .. } => 6,
            Self::Cylinder { .. } => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "origin",
    content = "index",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Selection {
    Source(usize),
    Foreign(usize),
    Null,
}
impl Selection {
    const fn native(self) -> ffi::FaceSelection {
        match self {
            Self::Source(index) => ffi::FaceSelection::Source(index),
            Self::Foreign(index) => ffi::FaceSelection::Foreign(index),
            Self::Null => ffi::FaceSelection::Null,
        }
    }
}

// Five independent native flags, not alternatives in a state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    offset_distance: f64,
    member_distance: f64,
    member_approx_tol: f64,
    member_operation: u32,
    inset: bool,
    extend_convex: bool,
    merge_results: bool,
    create_offset_solid: bool,
    skip_stitch_merge: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            offset_distance: 0.1,
            member_distance: 0.1,
            member_approx_tol: 1e-6,
            member_operation: 0,
            inset: false,
            extend_convex: false,
            merge_results: false,
            create_offset_solid: false,
            skip_stitch_merge: false,
        }
    }
}
impl Config {
    fn native(&self) -> ffi::Config {
        ffi::Config {
            offset_distance: self.offset_distance,
            member_distance: self.member_distance,
            member_approx_tol: self.member_approx_tol,
            member_operation: self.member_operation,
            inset: self.inset,
            extend_convex: self.extend_convex,
            merge_results: self.merge_results,
            create_offset_solid: self.create_offset_solid,
            skip_stitch_merge: self.skip_stitch_merge,
        }
    }
    const fn offset(&self) -> OffsetOptions {
        OffsetOptions {
            distance: self.offset_distance,
            extend_convex_edges: self.extend_convex,
            merge_results: self.merge_results,
        }
    }
    const fn shell(&self) -> ShellOptions {
        ShellOptions {
            offset: self.offset(),
            create_offset_solid: self.create_offset_solid,
            merge_sheet_side_walls: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    operation: Operation,
    source: Option<Fixture>,
    #[serde(default)]
    foreign: Option<Fixture>,
    #[serde(default)]
    faces: Vec<Selection>,
    tolerance: f64,
    #[serde(default)]
    config: Config,
    /// The returned/retained B-reps' vertex bounds are not compared (counts, kinds and
    /// tolerance still are). Set only on fixture cases whose native vertex positions
    /// depend on heap addresses: `DoSolidOffset` walks `SmRelation`s whose
    /// `std::unordered_map` buckets are keyed by object address, so the order of the
    /// tolerance-merge averaging of degenerate side-wall vertices follows the heap
    /// layout of the process (finding
    /// `plan/findings/offset.offset_executive_p2--exact-2tol-shell-heap-layout.md`).
    #[serde(default)]
    heap_ordered_vertices: bool,
}
impl Inputs {
    fn native(&self) -> ffi::Request {
        ffi::Request {
            operation: self.operation.native(),
            source: self.source.as_ref().map(Fixture::native),
            foreign: self.foreign.as_ref().map(Fixture::native),
            faces: self.faces.iter().copied().map(Selection::native).collect(),
            tolerance: self.tolerance,
            config: self.config.native(),
        }
    }
}

fn bad(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}
fn setup(error: impl core::fmt::Display) -> HarnessError {
    bad(format!("Rust input fixture: {error}"))
}
fn at<T>(slice: &[T], index: usize) -> HResult<&T> {
    slice
        .get(index)
        .ok_or_else(|| bad("fixture/snapshot index outside its traversal"))
}
fn finite(values: &[f64]) -> HResult<()> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(bad("nonfinite fixture scalar"))
    }
}
fn interval(range: [f64; 2], strict: bool) -> HResult<()> {
    finite(&range)?;
    let [a, b] = range;
    if a > b || (strict && a == b) || !(b - a).is_finite() {
        return Err(bad("invalid finite fixture interval"));
    }
    Ok(())
}
fn validate(input: &Inputs) -> HResult<()> {
    let cfg = &input.config;
    finite(&[
        input.tolerance,
        cfg.offset_distance,
        cfg.member_distance,
        cfg.member_approx_tol,
    ])?;
    if input.tolerance <= 0.0 {
        return Err(bad(
            "automatic B-rep tolerance is outside this Rust fixture projection",
        ));
    }
    if cfg.member_operation > 4 || (input.operation.member() && cfg.member_distance < 0.0) {
        return Err(bad("invalid native member constructor parameters"));
    }
    if input.source.is_none()
        && matches!(
            input.operation,
            Operation::StaticOffset | Operation::StaticShell
        )
    {
        return Err(bad(
            "static offset/shell dereferences null before validation",
        ));
    }

    if input.faces.len() > ffi::MAX_SELECTIONS {
        return Err(bad("too many selected faces"));
    }
    for selection in &input.faces {
        let (fixture, index) = match *selection {
            Selection::Source(index) => (input.source.as_ref(), index),
            Selection::Foreign(index) => (input.foreign.as_ref(), index),
            Selection::Null => continue,
        };
        if index >= fixture.map_or(0, Fixture::face_count) {
            return Err(bad("selected face outside its origin fixture"));
        }
    }
    // Match the native/FFI guard: null member/API inputs and API-shell invalid
    // origins return before extension arithmetic, but still validate every DTO.
    let invalid_api_selection = input.operation == Operation::ApiShell
        && input
            .faces
            .iter()
            .any(|face| !matches!(face, Selection::Source(_)));
    if input.source.is_some()
        && !invalid_api_selection
        && (input.operation == Operation::Extended || cfg.extend_convex)
    {
        let magnitude = if input.operation.member() {
            cfg.member_distance
        } else {
            cfg.offset_distance.abs()
        };
        finite(&[5.0 * magnitude])?;
    }
    for fixture in input.source.iter().chain(&input.foreign) {
        match *fixture {
            Fixture::Empty => {}
            Fixture::Wire {
                start,
                end,
                domain,
                interval: trim,
            } => {
                finite(&start)?;
                finite(&end)?;
                interval(domain, true)?;
                interval(trim, false)?;
                let [d0, d1] = domain;
                let [t0, t1] = trim;
                if t0 < d0 || t1 > d1 {
                    return Err(bad("wire trim outside curve domain"));
                }
                finite(&[(v3(end) - v3(start)).length_squared()])?;
            }
            Fixture::PlaneSheet {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            } => {
                finite(&origin)?;
                finite(&x_axis)?;
                finite(&y_axis)?;
                finite(&scale)?;
                let [u0, v0, u1, v1] = domain;
                interval([u0, u1], true)?;
                interval([v0, v1], true)?;
                let [su, sv] = scale;
                finite(&[
                    v3(x_axis).length_squared(),
                    v3(y_axis).length_squared(),
                    su * u0,
                    su * u1,
                    sv * v0,
                    sv * v1,
                ])?;
            }
            Fixture::Box { origin, size } => {
                finite(&origin)?;
                finite(&size)?;
                for (a, b) in origin.iter().zip(size) {
                    finite(&[a + b, b * b])?;
                }
            }
            Fixture::Cylinder {
                origin,
                radius,
                height,
            } => {
                primitive_finite(origin, radius)?;
                finite(&[height, origin[2] + height])?;
            }
            Fixture::Sphere { origin, radius } => primitive_finite(origin, radius)?,
        }
    }
    Ok(())
}

#[derive(Debug)]
enum RunError {
    Harness(HarnessError),
    Kernel(SolidError),
}
impl From<HarnessError> for RunError {
    fn from(error: HarnessError) -> Self {
        Self::Harness(error)
    }
}
impl From<SolidError> for RunError {
    fn from(error: SolidError) -> Self {
        Self::Kernel(error)
    }
}
// Preserve all ordinary codes; in particular 1001 != 1006 != 1007. A real
// Unported must escape even when the static offset collapses ordinary errors.
fn operation<T>(result: solidcore::Result<T>) -> RResult<(i32, Option<T>)> {
    match result {
        Ok(value) => Ok((ffi::NATIVE_SUCCESS, Some(value))),
        Err(SolidError::Upstream { code, .. }) => Ok((code, None)),
        Err(error) => Err(RunError::Kernel(error)),
    }
}
fn operation_type(code: u32) -> HResult<OffsetOperation> {
    match code {
        0 => Ok(OffsetOperation::SolidOffset),
        1 => Ok(OffsetOperation::NonManifoldOffset),
        2 => Ok(OffsetOperation::Shell),
        3 => Ok(OffsetOperation::LocalOperation),
        4 => Ok(OffsetOperation::MachineOffset),
        _ => Err(bad("member operation outside native enum")),
    }
}
fn executive(cfg: &Config) -> HResult<OffsetExecutive> {
    let mut executive = OffsetExecutive::new(
        operation_type(cfg.member_operation)?,
        cfg.member_distance,
        cfg.member_approx_tol,
    );
    executive.set_inset(cfg.inset);
    executive.set_extend_convex_edges(cfg.extend_convex);
    executive.set_merge_results(cfg.merge_results);
    executive.set_create_offset_solid(cfg.create_offset_solid);
    Ok(executive)
}
fn executive_state(executive: &OffsetExecutive) -> ffi::ExecutiveState {
    ffi::ExecutiveState {
        operation: executive.operation() as i32,
        inset: executive.inset(),
        extend_convex: executive.extend_convex_edges(),
        merge_results: executive.merge_results(),
        create_offset_solid: executive.create_offset_solid(),
        distance: executive.offset_distance(),
        approx_tol: executive.approximation_tol_3d(),
        tangency_degrees: executive.tangency_tol_degrees(),
    }
}

fn line(start: Vec3, end: Vec3, domain: [f64; 2], dimension: u64) -> HResult<Curve> {
    let domain = Extent1d::from_min_max(domain[0], domain[1]);
    Ok(Curve::from(
        Line::create_line_segment(dimension, start, end, Some(&domain)).map_err(setup)?,
    ))
}
fn add_vertex(brep: &mut Brep, point: Vec3) -> HResult<VertexId> {
    brep.add_vertex_record(Vertex {
        point,
        tolerance: brep.tolerance(),
        ..Vertex::default()
    })
    .map_err(setup)
}
fn add_edge(
    brep: &mut Brep,
    vertices: [VertexId; 2],
    curve: Curve,
    trim: [f64; 2],
) -> HResult<EdgeId> {
    brep.add_edge_record(Edge {
        vertices,
        curve: Some(curve),
        interval: Extent1d::from_min_max(trim[0], trim[1]),
        edgeuses: Vec::new(),
        tolerance: brep.tolerance(),
    })
    .map_err(setup)
}
fn add_use(
    brep: &mut Brep,
    edge: EdgeId,
    vertex: VertexId,
    owner: EdgeuseOwner,
    orientation: OrientType,
    uv_trim_curve: Option<Curve>,
) -> HResult<solidcore::topology::EdgeuseId> {
    let use_id = brep
        .add_edgeuse_record(Edgeuse {
            edge: Some(edge),
            owner,
            orientation,
            uv_trim_curve,
            ..Edgeuse::default()
        })
        .map_err(setup)?;
    let vertexuse = brep
        .add_vertexuse_record(Vertexuse {
            vertex: Some(vertex),
            owner: VertexuseOwner::Edgeuse(Some(use_id)),
            ..Vertexuse::default()
        })
        .map_err(setup)?;
    brep.vertex_mut(vertex)
        .map_err(setup)?
        .vertexuses
        .push(vertexuse);
    brep.edgeuse_mut(use_id).map_err(setup)?.vertexuse = Some(vertexuse);
    brep.edge_mut(edge).map_err(setup)?.edgeuses.push(use_id);
    Ok(use_id)
}
fn add_shell(brep: &mut Brep, region: RegionId) -> HResult<ShellId> {
    let shell = brep
        .add_shell_record(Shell {
            region: Some(region),
            ..Shell::default()
        })
        .map_err(setup)?;
    brep.region_mut(region).map_err(setup)?.shells.push(shell);
    Ok(shell)
}

// Rectangular typed input scaffold, adapted from p1's face_with_edges, with a
// shared shell rather than four extra wire shells. It is NOT an offset result.
/// The FFI's finiteness checks of a cylinder or sphere fixture.
fn primitive_finite(origin: [f64; 3], radius: f64) -> HResult<()> {
    finite(&origin)?;
    finite(&[radius, radius * radius])?;
    for a in origin {
        finite(&[a + radius, a - radius])?;
    }
    Ok(())
}

fn build(kernel: &mut Kernel, fixture: &Fixture, tolerance: f64, namespace: u64) -> HResult<Brep> {
    if let Fixture::Box { origin, size } = *fixture {
        return build_box(kernel, origin, size, tolerance);
    }
    if let Fixture::Cylinder {
        origin,
        radius,
        height,
    } = *fixture
    {
        let mut brep = solidcore::primitive::api_primitives::create_cylinder(
            kernel,
            v3(origin),
            radius,
            height,
        )
        .map_err(setup)?;
        brep.set_tolerance(tolerance);
        return Ok(brep);
    }
    if let Fixture::Sphere { origin, radius } = *fixture {
        let mut brep =
            solidcore::primitive::api_primitives::create_sphere(kernel, v3(origin), radius)
                .map_err(setup)?;
        brep.set_tolerance(tolerance);
        return Ok(brep);
    }
    let mut brep = Brep::new();
    brep.set_tolerance(tolerance);
    // SmBrep.cpp:532–537: the constructor explicitly creates a void infinite region.
    let region = brep
        .add_region_record(Region {
            in_brep: true,
            is_void: 1,
            ..Region::default()
        })
        .map_err(setup)?;
    brep.set_infinite_region(Some(region));
    match *fixture {
        Fixture::Empty => {}
        Fixture::Wire {
            start,
            end,
            domain,
            interval,
        } => {
            let curve = line(v3(start), v3(end), domain, 3)?;
            let [t0, t1] = interval;
            let a = add_vertex(&mut brep, curve.evaluate_point(kernel, t0).map_err(setup)?)?;
            let b = add_vertex(&mut brep, curve.evaluate_point(kernel, t1).map_err(setup)?)?;
            let shell = add_shell(&mut brep, region)?;
            let edge = add_edge(&mut brep, [a, b], curve, interval)?;
            let first = add_use(
                &mut brep,
                edge,
                a,
                EdgeuseOwner::Shell(Some(shell)),
                OrientType::Same,
                None,
            )?;
            let second = add_use(
                &mut brep,
                edge,
                b,
                EdgeuseOwner::Shell(Some(shell)),
                OrientType::Opposite,
                None,
            )?;
            brep.edgeuse_mut(first).map_err(setup)?.ccw = Some(second);
            brep.edgeuse_mut(first).map_err(setup)?.cw = Some(second);
            brep.edgeuse_mut(second).map_err(setup)?.ccw = Some(first);
            brep.edgeuse_mut(second).map_err(setup)?.cw = Some(first);
        }
        Fixture::PlaneSheet {
            origin,
            x_axis,
            y_axis,
            scale,
            domain,
        } => {
            // The native fixture: `CreateFaceFromSurface(new SmPlane(..), domain)` on
            // the new B-rep, through the real constructor (its natural boundary
            // edges, vertices and UV trims), with the face owning its surface
            // under its notification identity.
            let [u0, v0, u1, v1] = domain;
            let domain = Extent2d::from_bounds(u0, v0, u1, v1);
            let surface = Surface::from(Plane::new(
                v3(origin),
                v3(x_axis),
                v3(y_axis),
                point2(scale),
                &domain,
            ));
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attributes = AttributeStore::default();
            let key_of = |entity| notification_key(entity, namespace);
            let mut env = solidcore::topology::edge::NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            let mut marks = solidcore::topology::topology_traverser::Marks::default();
            let this_face = |face: FaceId| key_of(EntityRef::Face(face));
            let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            solidcore::topology::brep::p8::create_face_from_surface(
                kernel,
                &mut brep,
                Some(surface),
                &domain,
                &mut fenv,
            )
            .map_err(setup)?;
        }
        Fixture::Box { .. } | Fixture::Cylinder { .. } | Fixture::Sphere { .. } => {
            return Err(bad("primitive fixtures use the primitive constructors"))
        }
    }
    Ok(brep)
}

/// The native fixture's `SmApiCreateBox(origin, dx, dy, dz)` then
/// `SetTolerance(tolerance, FALSE)`, through the real primitive constructor: the
/// stitched, consistently oriented box an offset needs (a hand-made scaffold would
/// not reproduce the native radial order or face-use orientations).
fn build_box(
    kernel: &mut Kernel,
    origin: [f64; 3],
    size: [f64; 3],
    tolerance: f64,
) -> HResult<Brep> {
    if size.iter().any(|value| *value <= 0.0) {
        return Err(bad("box fixture constructor requires positive dimensions"));
    }
    let [dx, dy, dz] = size;
    let mut brep = solidcore::primitive::api_primitives::create_box(kernel, v3(origin), dx, dy, dz)
        .map_err(setup)?;
    brep.set_tolerance(tolerance);
    Ok(brep)
}

const SOURCE: BrepId = BrepId::from_index(17);
const FOREIGN: BrepId = BrepId::from_index(43);
fn face_at(brep: Option<&Brep>, index: usize) -> HResult<FaceId> {
    brep.and_then(|brep| brep.faces().iter().nth(index).map(|(id, _)| id))
        .ok_or_else(|| bad("selected face absent from Rust origin traversal"))
}
fn selections(
    input: &Inputs,
    source: Option<&Brep>,
    foreign: Option<&Brep>,
) -> HResult<Vec<Option<FaceKey>>> {
    input
        .faces
        .iter()
        .map(|selection| {
            Ok(match *selection {
                Selection::Source(index) => Some((SOURCE, face_at(source, index)?)),
                Selection::Foreign(index) => Some((FOREIGN, face_at(foreign, index)?)),
                Selection::Null => None,
            })
        })
        .collect()
}
fn local_faces(selections: &[Option<FaceKey>]) -> HResult<Vec<FaceId>> {
    selections
        .iter()
        .map(|selection| match selection {
            Some((owner, id)) if *owner == SOURCE => Ok(*id),
            _ => Err(bad(
                "static/member shell lists require source-only faces in this Rust adapter",
            )),
        })
        .collect()
}
fn observe_selections(
    selections: &[Option<FaceKey>],
    source: Option<&Brep>,
    foreign: Option<&Brep>,
    before: Option<&[[i64; 2]]>,
) -> Vec<[i64; 2]> {
    selections
        .iter()
        .enumerate()
        .map(|(i, selection)| {
            if let Some(prev) = before.and_then(|prev| prev.get(i)) {
                if source.is_none() && prev[0] == 0 {
                    return [3, prev[1]];
                }
            }
            let Some((owner, face)) = selection else {
                return [2, -1];
            };
            let (origin, brep) = if *owner == SOURCE {
                (0, source)
            } else {
                (1, foreign)
            };
            let index =
                brep.and_then(|brep| brep.face_list().iter().position(|id| id == face));
            index.map_or([4, -1], |index| [origin, index as i64])
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct Projection {
    counts: Vec<i64>,
    kinds: Vec<i64>,
    statuses: Vec<i64>,
    values: Vec<f64>,
}
fn projected_counts(fixture: &Fixture, counts: [i64; 7]) -> Vec<i64> {
    let [_, _, faces, loops, edges, uses, vertices] = counts;
    if fixture.primitive() {
        vec![faces, edges, vertices]
    } else {
        vec![faces, loops, edges, uses, vertices]
    }
}
fn bounds(points: impl Iterator<Item = [f64; 3]>) -> Vec<f64> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for point in points {
        any = true;
        for ((lo, hi), value) in lo.iter_mut().zip(&mut hi).zip(point) {
            *lo = lo.min(value);
            *hi = hi.max(value);
        }
    }
    if any {
        lo.into_iter().chain(hi).collect()
    } else {
        Vec::new()
    }
}
fn xyz(point: Vec3) -> [f64; 3] {
    [point.x, point.y, point.z]
}
fn domain_values(domain: Extent2d) -> [f64; 4] {
    let lo = domain.min();
    let hi = domain.max();
    [lo.x, lo.y, hi.x, hi.y]
}
fn rust_projection(kernel: &Kernel, fixture: &Fixture, brep: &Brep) -> HResult<Projection> {
    // the shim's GetRegions / GetShells / GetFaces / GetLoops / GetEdges / GetEdgeuses /
    // GetVertices: the live lists (a deleted record stays in its arena, unreachable,
    // `topology.brep_p3`), loops through the faces and edge uses through the edges, as
    // upstream walks them
    let regions = brep_p1::regions(brep, None).map_err(setup)?;
    let mut shells = std::collections::BTreeSet::new();
    for &r in &regions {
        shells.extend(brep.region(r).map_err(setup)?.shells.iter().copied());
    }
    let faces = brep_p1::faces(brep, None).map_err(setup)?;
    let edges = brep_p1::edges(brep, None).map_err(setup)?;
    let vertices = brep_p1::vertices(brep, None).map_err(setup)?;
    let mut loops = 0;
    for &f in &faces {
        let face = brep.face(f).map_err(setup)?;
        loops += if face.faceuse.is_some() {
            solidcore::topology::face::p2::loops(brep, f, false, None)
                .map_err(setup)?
                .len()
        } else {
            face.loops.len()
        };
    }
    let mut uses = 0;
    for &e in &edges {
        uses += brep.edge(e).map_err(setup)?.edgeuses.len();
    }
    let counts = [
        regions.len() as i64,
        shells.len() as i64,
        faces.len() as i64,
        loops as i64,
        edges.len() as i64,
        uses as i64,
        vertices.len() as i64,
    ];
    let mut result = Projection {
        counts: projected_counts(fixture, counts),
        kinds: Vec::new(),
        statuses: Vec::new(),
        values: vec![brep.tolerance()],
    };
    for &face_id in &faces {
        let face = brep.face(face_id).map_err(setup)?;
        let surface = face
            .surface
            .as_ref()
            .ok_or_else(|| bad("fixture face has no real surface"))?;
        // (the shim's `IsKindOf` classes: a cone or sphere is an `SmBSplineSurface`)
        result.kinds.push(surface_kind(Some(surface))?);
        if matches!(fixture, Fixture::PlaneSheet { .. }) {
            result.values.extend(domain_values(face.uv_domain));
            result
                .values
                .extend(domain_values(surface.natural_uv_domain_with_nurb(kernel).map_err(|e| HarnessError::new(e.to_string()))?));
            let lo = face.uv_domain.min();
            let hi = face.uv_domain.max();
            for fraction in [0.0, 0.5, 1.0] {
                let uv = point2([
                    (1.0 - fraction) * lo.x + fraction * hi.x,
                    (1.0 - fraction) * lo.y + fraction * hi.y,
                ]);
                let point = surface.evaluate_point(kernel, uv).map_err(setup)?;
                let normal = surface
                    .evaluate_normal(kernel, uv, Side::Left, Side::Left)
                    .map_err(setup)?;
                result.statuses.extend([i64::from(ffi::NATIVE_SUCCESS); 2]);
                result.values.extend(xyz(point));
                result.values.extend(xyz(normal));
            }
        }
    }
    if matches!(fixture, Fixture::Wire { .. }) {
        for &edge_id in &edges {
            let edge = brep.edge(edge_id).map_err(setup)?;
            let curve = edge
                .curve
                .as_ref()
                .ok_or_else(|| bad("fixture wire has no real curve"))?;
            result.kinds.push(curve_kind(Some(curve)));
            let domain = curve.natural_interval();
            result.values.extend([
                edge.interval.min(),
                edge.interval.max(),
                domain.min(),
                domain.max(),
            ]);
            for parameter in [edge.interval.min(), edge.interval.max()] {
                let point = curve.evaluate_point(kernel, parameter).map_err(setup)?;
                result.statuses.push(i64::from(ffi::NATIVE_SUCCESS));
                result.values.extend(xyz(point));
            }
        }
    }
    if fixture.primitive() || matches!(fixture, Fixture::Wire { .. }) {
        let mut vertex_points = Vec::with_capacity(vertices.len());
        for &vertex_id in &vertices {
            vertex_points.push(xyz(brep.vertex(vertex_id).map_err(setup)?.point));
        }
        result.values.extend(bounds(vertex_points.into_iter()));
    }
    Ok(result)
}

// The FFI decoder separates the protocol's integers and floats. This second,
// bounded decoder validates the ENTIRE topology/geometry layout before selecting
// representable input fields; dropping unsupported fields must not mask truncation.
struct BlockReader<'a> {
    integers: &'a [i64],
    values: &'a [f64],
}
impl BlockReader<'_> {
    fn integer(&mut self, min: i64, max: i64) -> HResult<i64> {
        let (value, rest) = self
            .integers
            .split_first()
            .ok_or_else(|| bad("truncated native topology block"))?;
        if !(min..=max).contains(value) {
            return Err(bad("invalid native topology integer"));
        }
        self.integers = rest;
        Ok(*value)
    }
    fn count(&mut self) -> HResult<usize> {
        Ok(self.integer(0, (ffi::MAX_OUTPUT_VALUES / 64) as i64)? as usize)
    }
    fn id(&mut self, expected: usize) -> HResult<()> {
        self.integer(expected as i64, expected as i64).map(|_| ())
    }
    fn reference(&mut self, count: usize) -> HResult<()> {
        self.integer(-2, count as i64 - 1).map(|_| ())
    }
    fn list(&mut self, count: usize) -> HResult<()> {
        let length = self.count()?;
        if length > self.integers.len() {
            return Err(bad("truncated native adjacency list"));
        }
        for _ in 0..length {
            self.reference(count)?;
        }
        Ok(())
    }
    fn numbers(&mut self, count: usize) -> HResult<Vec<f64>> {
        let values = self
            .values
            .get(..count)
            .ok_or_else(|| bad("truncated native geometry block"))?;
        self.values = self
            .values
            .get(count..)
            .ok_or_else(|| bad("truncated native geometry block"))?;
        Ok(values.to_vec())
    }
    fn status(&mut self) -> HResult<i64> {
        self.integer(i64::from(i32::MIN), i64::from(i32::MAX))
    }
}
#[derive(Debug)]
struct SampleRecord {
    kind: i64,
    statuses: Vec<i64>,
    values: Vec<f64>,
}
#[derive(Debug)]
struct DecodedBrep {
    counts: [i64; 7],
    tolerance: f64,
    faces: Vec<SampleRecord>,
    edges: Vec<SampleRecord>,
    vertices: Vec<[f64; 3]>,
}
fn decode_block(block: &ffi::BrepSnapshot) -> HResult<DecodedBrep> {
    if block.integers.len().saturating_add(block.values.len()) > ffi::MAX_OUTPUT_VALUES {
        return Err(bad("native B-rep block exceeds output budget"));
    }
    let mut rd = BlockReader {
        integers: &block.integers,
        values: &block.values,
    };
    let mut counts = [0; 7];
    for count in &mut counts {
        *count = rd.count()? as i64;
    }
    let [regions, shells, faces, loops, edges, uses, vertices] = counts.map(|count| count as usize);
    if counts.iter().sum::<i64>() > (ffi::MAX_OUTPUT_VALUES / 64) as i64 {
        return Err(bad("native topology aggregate exceeds budget"));
    }
    rd.reference(regions)?; // Native-only infinite region: validated, not fabricated in Rust.
    rd.integer(0, 1)?; // Native-only editing flag.
    rd.integer(0, 1)?; // Validate-only flag; full block is still checked for purity.
    let tolerance = *at(&rd.numbers(1)?, 0)?;
    for index in 0..regions {
        rd.id(index)?;
        rd.integer(0, 1)?;
    }
    for index in 0..shells {
        rd.id(index)?;
        rd.reference(regions)?;
    }
    let mut face_records = Vec::new();
    for index in 0..faces {
        rd.id(index)?;
        let kind = rd.integer(0, 3)?;
        rd.reference(shells)?;
        rd.reference(shells)?;
        rd.integer(-1, 3)?;
        rd.integer(-1, 3)?;
        rd.list(loops)?;
        let statuses = (0..6).map(|_| rd.status()).collect::<HResult<_>>()?;
        face_records.push(SampleRecord {
            kind,
            statuses,
            values: rd.numbers(27)?,
        });
    }
    for index in 0..loops {
        rd.id(index)?;
        rd.reference(faces)?;
        rd.list(uses)?;
    }
    let mut edge_records = Vec::new();
    for index in 0..edges {
        rd.id(index)?;
        rd.reference(vertices)?;
        rd.reference(vertices)?;
        let kind = rd.integer(0, 3)?;
        rd.list(uses)?;
        let statuses = (0..2).map(|_| rd.status()).collect::<HResult<_>>()?;
        edge_records.push(SampleRecord {
            kind,
            statuses,
            values: rd.numbers(11)?,
        });
    }
    for index in 0..uses {
        rd.id(index)?;
        for count in [edges, faces, shells, uses, uses] {
            rd.reference(count)?;
        }
        rd.integer(-1, 3)?;
    }
    let mut vertex_points = Vec::new();
    for index in 0..vertices {
        rd.id(index)?;
        let values = rd.numbers(4)?;
        vertex_points.push([*at(&values, 0)?, *at(&values, 1)?, *at(&values, 2)?]);
    }
    if !rd.integers.is_empty() || !rd.values.is_empty() {
        return Err(bad("trailing native B-rep snapshot fields"));
    }
    Ok(DecodedBrep {
        counts,
        tolerance,
        faces: face_records,
        edges: edge_records,
        vertices: vertex_points,
    })
}
fn native_projection(fixture: &Fixture, decoded: &DecodedBrep) -> HResult<Projection> {
    let mut result = Projection {
        counts: projected_counts(fixture, decoded.counts),
        kinds: Vec::new(),
        statuses: Vec::new(),
        values: vec![decoded.tolerance],
    };
    for face in &decoded.faces {
        result.kinds.push(face.kind);
        if matches!(fixture, Fixture::PlaneSheet { .. }) {
            result.statuses.extend(&face.statuses);
            result.values.extend(
                face.values
                    .get(1..)
                    .ok_or_else(|| bad("missing face geometry"))?,
            );
        }
    }
    if matches!(fixture, Fixture::Wire { .. }) {
        for edge in &decoded.edges {
            result.kinds.push(edge.kind);
            result.statuses.extend(&edge.statuses);
            result.values.extend(
                edge.values
                    .get(1..)
                    .ok_or_else(|| bad("missing edge geometry"))?,
            );
        }
    }
    if fixture.primitive() || matches!(fixture, Fixture::Wire { .. }) {
        result
            .values
            .extend(bounds(decoded.vertices.iter().copied()));
    }
    Ok(result)
}
/// What is compared of a returned B-rep or a retained offset temporary: every
/// topology count, the face surface and edge curve kinds as sorted multisets, the
/// B-rep tolerance and the vertex bounds. Traversal order and region/shell
/// identities are deliberately not compared.
struct ResultProjection {
    counts: Vec<i64>,
    face_kinds: Vec<i64>,
    edge_kinds: Vec<i64>,
    values: Vec<f64>,
}
fn native_result_projection(decoded: &DecodedBrep) -> ResultProjection {
    let mut face_kinds: Vec<i64> = decoded.faces.iter().map(|face| face.kind).collect();
    let mut edge_kinds: Vec<i64> = decoded.edges.iter().map(|edge| edge.kind).collect();
    face_kinds.sort_unstable();
    edge_kinds.sort_unstable();
    let mut values = vec![decoded.tolerance];
    values.extend(bounds(decoded.vertices.iter().copied()));
    ResultProjection {
        counts: decoded.counts.to_vec(),
        face_kinds,
        edge_kinds,
        values,
    }
}
/// The shim's `surface_kind`: 0 null, 1 an `SmPlane`, 2 another `SmBSplineSurface`
/// kind, 3 anything else.
fn surface_kind(surface: Option<&Surface>) -> HResult<i64> {
    use solidcore::surface::surface::p2::{B_SPLINE_SURFACE_TYPE, PLANE_TYPE};
    let Some(surface) = surface else {
        return Ok(0);
    };
    Ok(if surface.is_kind_of(PLANE_TYPE).map_err(setup)? {
        1
    } else if surface.is_kind_of(B_SPLINE_SURFACE_TYPE).map_err(setup)? {
        2
    } else {
        3
    })
}
/// The shim's `curve_kind`: 0 null, 1 an `SmLine`, 2 another `SmBSplineCurve`
/// kind, 3 anything else.
fn curve_kind(curve: Option<&Curve>) -> i64 {
    use solidcore::curve::curve::p2::BSPLINE_CURVE_TYPE;
    match curve {
        None => 0,
        Some(curve) if matches!(curve.kind(), CurveKind::Line(_)) => 1,
        Some(curve) if curve.is_kind_of(BSPLINE_CURVE_TYPE) => 2,
        Some(_) => 3,
    }
}
fn rust_result_projection(brep: &Brep) -> HResult<ResultProjection> {
    let regions = brep_p1::regions(brep, None).map_err(setup)?;
    let mut shells = std::collections::BTreeSet::new();
    for &r in &regions {
        shells.extend(brep.region(r).map_err(setup)?.shells.iter().copied());
    }
    let faces = brep_p1::faces(brep, None).map_err(setup)?;
    let edges = brep_p1::edges(brep, None).map_err(setup)?;
    let vertices = brep_p1::vertices(brep, None).map_err(setup)?;
    let mut loops = 0;
    for &f in &faces {
        let face = brep.face(f).map_err(setup)?;
        loops += if face.faceuse.is_some() {
            solidcore::topology::face::p2::loops(brep, f, false, None)
                .map_err(setup)?
                .len()
        } else {
            face.loops.len()
        };
    }
    let mut edgeuses = 0;
    for &e in &edges {
        edgeuses += brep.edge(e).map_err(setup)?.edgeuses.len();
    }
    let counts = vec![
        regions.len() as i64,
        shells.len() as i64,
        faces.len() as i64,
        loops as i64,
        edges.len() as i64,
        edgeuses as i64,
        vertices.len() as i64,
    ];
    let mut face_kinds = Vec::with_capacity(faces.len());
    for &f in &faces {
        face_kinds.push(surface_kind(brep.face(f).map_err(setup)?.surface.as_ref())?);
    }
    let mut edge_kinds = Vec::with_capacity(edges.len());
    for &e in &edges {
        edge_kinds.push(curve_kind(brep.edge(e).map_err(setup)?.curve.as_ref()));
    }
    face_kinds.sort_unstable();
    edge_kinds.sort_unstable();
    let mut values = vec![brep.tolerance()];
    let mut vertex_points = Vec::with_capacity(vertices.len());
    for &v in &vertices {
        vertex_points.push(xyz(brep.vertex(v).map_err(setup)?.point));
    }
    values.extend(bounds(vertex_points.into_iter()));
    Ok(ResultProjection {
        counts,
        face_kinds,
        edge_kinds,
        values,
    })
}
/// Emits a result projection; with `heap_ordered` (see [`Inputs::heap_ordered_vertices`])
/// only the B-rep tolerance of `values` is kept, not the vertex bounds.
fn project_result(
    output: Output,
    key: &str,
    projection: Option<ResultProjection>,
    heap_ordered: bool,
) -> Output {
    let Some(mut projection) = projection else {
        return output.int(&format!("{key}.present"), 0);
    };
    if heap_ordered {
        projection.values.truncate(1);
    }
    output
        .int(&format!("{key}.present"), 1)
        .ints(&format!("{key}.counts"), projection.counts)
        .ints(&format!("{key}.face_kinds"), projection.face_kinds)
        .ints(&format!("{key}.edge_kinds"), projection.edge_kinds)
        .values(&format!("{key}.geometry"), projection.values)
}
fn project(output: Output, key: &str, projection: Option<Projection>) -> Output {
    let Some(projection) = projection else {
        return output.int(&format!("{key}.present"), 0);
    };
    output
        .int(&format!("{key}.present"), 1)
        .ints(&format!("{key}.counts"), projection.counts)
        .ints(&format!("{key}.kinds"), projection.kinds)
        .ints(&format!("{key}.evaluation_status"), projection.statuses)
        .values(&format!("{key}.geometry"), projection.values)
}
fn scalar_output(
    status: i32,
    input_transferred: bool,
    lifetime: i32,
    result_origin: i32,
    doing_boolean: [bool; 2],
    executive: Option<&ffi::ExecutiveState>,
    selected_before: &[[i64; 2]],
    selected_after: &[[i64; 2]],
) -> Output {
    let mut output = Output::new()
        .int("status", i64::from(status))
        .ints(
            "ownership",
            vec![
                i64::from(input_transferred),
                i64::from(lifetime),
                i64::from(result_origin),
            ],
        )
        .ints("doing_boolean", doing_boolean.map(i64::from).to_vec())
        .ints(
            "selected.before",
            selected_before
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        )
        .ints(
            "selected.after",
            selected_after.iter().flatten().copied().collect::<Vec<_>>(),
        )
        .int("executive.present", i64::from(executive.is_some()));
    if let Some(state) = executive {
        output = output
            .ints(
                "executive.flags",
                vec![
                    i64::from(state.operation),
                    i64::from(state.inset),
                    i64::from(state.extend_convex),
                    i64::from(state.merge_results),
                    i64::from(state.create_offset_solid),
                ],
            )
            .values(
                "executive.geometry_parameters",
                vec![state.distance, state.approx_tol, state.tangency_degrees],
            )
            .ints(
                "executive.signed_zero",
                vec![
                    i64::from(state.distance == 0.0 && state.distance.is_sign_negative()),
                    i64::from(state.approx_tol == 0.0 && state.approx_tol.is_sign_negative()),
                ],
            );
    }
    output
}
fn unchanged(before: Option<&ffi::BrepSnapshot>, after: Option<&ffi::BrepSnapshot>) -> bool {
    match (before, after) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.integers == b.integers
                && a.values.len() == b.values.len()
                && a.values
                    .iter()
                    .zip(&b.values)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
        }
        _ => false,
    }
}
fn native_output(input: &Inputs, snapshot: &ffi::Snapshot) -> HResult<Output> {
    if snapshot.selected_before.len() != input.faces.len()
        || snapshot.selected_after.len() != input.faces.len()
        || snapshot.executive.is_some() != input.operation.member()
    {
        return Err(bad("native executive/selector snapshot shape mismatch"));
    }
    let mut output = scalar_output(
        snapshot.status,
        snapshot.input_transferred,
        snapshot.source_lifetime,
        snapshot.result_origin,
        snapshot.doing_boolean,
        snapshot.executive.as_ref(),
        &snapshot.selected_before,
        &snapshot.selected_after,
    )
    .ints(
        "input.pure",
        vec![
            i64::from(unchanged(
                snapshot.source_before.as_ref(),
                snapshot.source_after.as_ref(),
            )),
            i64::from(unchanged(
                snapshot.foreign_before.as_ref(),
                snapshot.foreign_after.as_ref(),
            )),
        ],
    );
    for (key, fixture, block) in [
        (
            "source.before",
            input.source.as_ref(),
            snapshot.source_before.as_ref(),
        ),
        (
            "source.after",
            input.source.as_ref(),
            snapshot.source_after.as_ref(),
        ),
        (
            "foreign.before",
            input.foreign.as_ref(),
            snapshot.foreign_before.as_ref(),
        ),
        (
            "foreign.after",
            input.foreign.as_ref(),
            snapshot.foreign_after.as_ref(),
        ),
    ] {
        let decoded = block.map(decode_block).transpose()?;
        let projection = match (fixture, decoded.as_ref()) {
            (Some(fixture), Some(decoded)) => Some(native_projection(fixture, decoded)?),
            (_, None) => None,
            (None, Some(_)) => return Err(bad("native snapshot invented an input allocation")),
        };
        output = project(output, key, projection);
    }
    for (key, block) in [
        ("result", &snapshot.result),
        ("executive_offset", &snapshot.executive_offset),
    ] {
        let projection = block
            .as_ref()
            .map(|block| decode_block(block).map(|decoded| native_result_projection(&decoded)))
            .transpose()?;
        output = project_result(output, key, projection, input.heap_ordered_vertices);
    }
    Ok(output)
}
fn native_outcome(input: &Inputs, outcome: ffi::Outcome) -> HResult<Output> {
    match outcome {
        ffi::Outcome::Rejected { status } => Ok(Output::rejected(format!(
            "native fixture constructor status_{status}"
        ))),
        ffi::Outcome::Built(snapshot) => native_output(input, &snapshot),
    }
}

// Notifications use disjoint fixture namespaces, not arena-local IDs or geometry
// cache-owner keys. Every existing native topology record is registered before the
// operation; constructors register new result topology in these same caller stores.
fn notification_key(entity: EntityRef, namespace: u64) -> ObjectRef {
    let (index, type_code) = match entity {
        EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
        EntityRef::Region(id) => (id.index(), Region::type_code()),
        EntityRef::Shell(id) => (id.index(), Shell::type_code()),
        EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
        EntityRef::Faceuse(id) => (id.index(), 16022),
        EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
        EntityRef::Edge(id) => (id.index(), solidcore::topology::edge::TYPE),
        EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
        EntityRef::Vertex(id) => (id.index(), solidcore::topology::vertex::TYPE),
        EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
        // A Rust Loop is a logical boundary record, not a native notification target.
        EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
    };
    ObjectRef {
        key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)),
        type_code,
    }
}

fn original_notification_key(entity: EntityRef) -> ObjectRef {
    notification_key(entity, 1)
}

fn result_notification_key(entity: EntityRef) -> ObjectRef {
    notification_key(entity, 3)
}

fn original_curve_key(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey((5_u64 << 60) | ((type_code as u64) << 32) | u64::from(edge.index())),
        type_code,
    }
}

// Source lists are opaque notification handles, disjoint from topology and curves.
fn boolean_source_lists(result: bool, region: RegionId) -> [ObjectRef; 2] {
    let namespace = if result { 7_u64 } else { 6_u64 };
    [0_u64, 1].map(|list| ObjectRef {
        key: ObjectKey((namespace << 60) | (list << 32) | u64::from(region.index())),
        type_code: 0,
    })
}

fn result_curve_key(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey((4_u64 << 60) | ((type_code as u64) << 32) | u64::from(edge.index())),
        type_code,
    }
}

/// Identities of `SmApiShellBrepFull`'s private working copy, in namespace 0 with
/// class 0 (bits 58-59), disjoint from [`MergeBodies`] (classes 1-3) and every other
/// key here; the handle field (bits 46-57) tells topology (0), curves (1) and
/// source lists (2) apart.
fn copy_notification_key(entity: EntityRef) -> ObjectRef {
    let local = notification_key(entity, 0);
    ObjectRef {
        key: MergeBodies::key(
            0,
            BrepId::from_index(0),
            local.type_code as u64,
            (local.key.0 & 0xffff_ffff) as u32,
        ),
        type_code: local.type_code,
    }
}

fn copy_curve_key(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: MergeBodies::key(0, BrepId::from_index(1), type_code as u64, edge.index()),
        type_code,
    }
}

fn copy_source_lists(region: RegionId) -> [ObjectRef; 2] {
    [0_u64, 1].map(|list| ObjectRef {
        key: MergeBodies::key(0, BrepId::from_index(2), list, region.index()),
        type_code: 0,
    })
}

/// Identities of the B-reps a merging `PiecewiseMerge` creates, in the unused
/// namespace 0 (bits 60-62), disjoint from the fixture, result and curve keys above
/// and below the kernel's reserved bit 63: bits 58-59 tell topology (1), curves (2)
/// and source lists (3) apart, bits 46-57 hold the merge-store handle, bits 32-45 the
/// type code.
struct MergeBodies;
impl MergeBodies {
    fn key(class: u64, brep: BrepId, kind: u64, index: u32) -> ObjectKey {
        ObjectKey(
            (class << 58)
                | ((u64::from(brep.index()) & 0xfff) << 46)
                | ((kind & 0x3fff) << 32)
                | u64::from(index),
        )
    }
}
impl SweepIdentities for MergeBodies {
    fn topology(&self, brep: BrepId, entity: EntityRef) -> ObjectRef {
        let local = notification_key(entity, 0);
        let index = (local.key.0 & 0xffff_ffff) as u32;
        ObjectRef {
            key: Self::key(1, brep, local.type_code as u64, index),
            type_code: local.type_code,
        }
    }
    fn owned_auxiliary_objects(&self, brep: BrepId, model: &Brep) -> Vec<ObjectKey> {
        model
            .edges()
            .iter()
            .filter_map(|(edge, e)| {
                let curve = e.curve.as_ref()?;
                Some(self.curve(brep, edge, curve).key)
            })
            .collect()
    }
    fn curve(&self, brep: BrepId, edge: EdgeId, curve: &Curve) -> ObjectRef {
        let type_code = curve.type_code();
        ObjectRef {
            key: Self::key(2, brep, type_code as u64, edge.index()),
            type_code,
        }
    }
    fn new_curve(
        &self,
        brep: BrepId,
        edge: EdgeId,
        type_code: i64,
    ) -> solidcore::Result<ObjectRef> {
        Ok(ObjectRef {
            key: Self::key(2, brep, type_code as u64, edge.index()),
            type_code,
        })
    }
    fn source_lists(&self, brep: BrepId, region: RegionId) -> solidcore::Result<[ObjectRef; 2]> {
        Ok([0_u64, 1].map(|list| ObjectRef {
            key: Self::key(3, brep, list, region.index()),
            type_code: 0,
        }))
    }
}

/// The notification namespace of the source fixture's identities.
const NAMESPACE_SOURCE: u64 = 1;
/// The notification namespace of the foreign fixture's identities.
const NAMESPACE_FOREIGN: u64 = 2;
fn register_notification_brep(
    records: &mut solidcore::topology::topology::NotifyRecords,
    brep: &mut Brep,
    namespace: u64,
) {
    let mut register = |entity| {
        records.insert(notification_key(entity, namespace).key, Topology::new());
    };
    let root = EntityRef::Brep(BrepId::from_index(0));
    brep.set_key(notification_key(root, namespace).key);
    register(root);
    for (id, _) in brep.regions().iter() {
        register(EntityRef::Region(id));
    }
    for (id, _) in brep.shells().iter() {
        register(EntityRef::Shell(id));
    }
    for (id, _) in brep.faces().iter() {
        register(EntityRef::Face(id));
    }
    for (id, _) in brep.faceuses().iter() {
        register(EntityRef::Faceuse(id));
    }
    for (id, _) in brep.loopuses().iter() {
        register(EntityRef::Loopuse(id));
    }
    for (id, _) in brep.edges().iter() {
        register(EntityRef::Edge(id));
    }
    for (id, _) in brep.edgeuses().iter() {
        register(EntityRef::Edgeuse(id));
    }
    for (id, _) in brep.vertices().iter() {
        register(EntityRef::Vertex(id));
    }
    for (id, _) in brep.vertexuses().iter() {
        register(EntityRef::Vertexuse(id));
    }
}

/// The deferral label of a Rust call that returned a B-rep or kept an offset temporary
/// (reachable since `query.brep`'s box is ported): the scaffolded inputs are not
/// native-equivalent, so the result needs an output adapter before it can be compared.
/// The Debug-visible arena state the purity check compares, without the edge
/// uses' UV trim curves: offset queries may make them lazily in the input (as
/// upstream's `NormalizedEvaluate` / `GetUVTrimCurve` do), and the native
/// snapshot block, which the native purity bit compares, does not record them.
fn purity_state(brep: &Brep) -> String {
    let mut state = brep.clone();
    let uses: Vec<_> = state.edgeuses().ids().collect();
    for id in uses {
        if let Ok(record) = state.edgeuse_mut(id) {
            record.uv_trim_curve = None;
        }
    }
    format!("{state:?}")
}
fn rust_output(input: &Inputs) -> RResult<Output> {
    validate(input)?;
    let mut kernel = Kernel::default();
    let mut source = input
        .source
        .as_ref()
        .map(|fixture| build(&mut kernel, fixture, input.tolerance, NAMESPACE_SOURCE))
        .transpose()?;
    let mut foreign = input
        .foreign
        .as_ref()
        .map(|fixture| build(&mut kernel, fixture, input.tolerance, NAMESPACE_FOREIGN))
        .transpose()?;
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    if let Some(brep) = source.as_mut() {
        register_notification_brep(&mut records, brep, NAMESPACE_SOURCE);
    }
    if let Some(brep) = foreign.as_mut() {
        register_notification_brep(&mut records, brep, NAMESPACE_FOREIGN);
    }
    let result_brep = result_notification_key(EntityRef::Brep(BrepId::from_index(0)));
    records.insert(result_brep.key, Topology::new());
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &original_notification_key,
        result_key_of: &result_notification_key,
        original_curve_ref: &original_curve_key,
        result_curve_ref: &result_curve_key,
        boolean_source_lists: &boolean_source_lists,
        result_brep,
        merge_identities: Some(&MergeBodies),
    };
    let keys = selections(input, source.as_ref(), foreign.as_ref())?;
    let selected_before = observe_selections(&keys, source.as_ref(), foreign.as_ref(), None);
    let source_state_before = source.as_ref().map(purity_state);
    let foreign_state_before = foreign.as_ref().map(purity_state);
    let source_before = input
        .source
        .as_ref()
        .zip(source.as_ref())
        .map(|(fixture, brep)| rust_projection(&kernel, fixture, brep))
        .transpose()?;
    let foreign_before = input
        .foreign
        .as_ref()
        .zip(foreign.as_ref())
        .map(|(fixture, brep)| rust_projection(&kernel, fixture, brep))
        .transpose()?;
    let boolean_before = kernel.context.doing_boolean();
    let mut exec = input
        .operation
        .member()
        .then(|| executive(&input.config))
        .transpose()?;
    if let Some(exec) = &mut exec {
        exec.set_shell_faces(&local_faces(&keys)?);
    }
    let mut transferred = false;
    let result = match input.operation {
        Operation::StaticOffset => p2::offset_brep(
            &mut kernel,
            source
                .as_mut()
                .ok_or_else(|| bad("static offset null source"))?,
            input.config.offset(),
            &mut notifications,
        ),
        Operation::StaticShell => {
            let original = source
                .take()
                .ok_or_else(|| bad("static shell null source"))?;
            let outcome = OffsetExecutive::shell_brep(
                &mut kernel,
                original,
                input.config.shell(),
                &local_faces(&keys)?,
                &mut notifications,
            );
            transferred = outcome.input_ownership_transferred();
            source = outcome.retained_input;
            outcome.result
        }
        Operation::Solid => exec
            .as_mut()
            .ok_or_else(|| bad("missing Rust executive"))?
            .do_solid_offset(
                &mut kernel,
                source.as_mut(),
                SolidOffsetOptions {
                    skip_stitch_merge: input.config.skip_stitch_merge,
                },
                &mut notifications,
            ),
        Operation::Extended => exec
            .as_mut()
            .ok_or_else(|| bad("missing Rust executive"))?
            .do_extended_offset(&mut kernel, source.as_mut(), &mut notifications),
        Operation::ApiShell => p2::api_shell_brep_full(
            &mut kernel,
            source.as_ref().map(|brep| BrepRef {
                brep,
                handle: SOURCE,
            }),
            input.config.shell(),
            &keys,
            &ShellCopyIdentities {
                key_of: &copy_notification_key,
                curve_ref: &copy_curve_key,
                boolean_source_lists: &copy_source_lists,
            },
            &mut notifications,
        ),
        Operation::ApiOffset => p2::api_offset_brep_full(
            &mut kernel,
            source.as_mut(),
            input.config.offset(),
            &mut notifications,
        ),
    };
    let (status, result) = operation(result)?;
    let result_projection = result.as_ref().map(rust_result_projection).transpose()?;
    let offset_projection = exec
        .as_ref()
        .and_then(OffsetExecutive::offset_brep)
        .map(rust_result_projection)
        .transpose()?;
    // A consumed source that comes back as the result (a Difference assigns its
    // result to the inset's primary operand, the original) keeps the source's
    // identity: native lifetime/origin 2 (`result == source`). Any other result is
    // a new allocation (origin 1).
    let source_key = notification_key(EntityRef::Brep(BrepId::from_index(0)), NAMESPACE_SOURCE).key;
    let result_is_source = input.source.is_some()
        && source.is_none()
        && result.as_ref().is_some_and(|brep| brep.key() == source_key);
    let result_origin = match (&result, result_is_source) {
        (None, _) => 0,
        (Some(_), true) => 2,
        (Some(_), false) => 1,
    };
    let state = exec.as_ref().map(executive_state);
    // The native source snapshot and selector observation after the call inspect the
    // live source, which is the result when the result is the source.
    let live_source = source
        .as_ref()
        .or_else(|| result.as_ref().filter(|_| result_is_source));
    let selected_after =
        observe_selections(&keys, live_source, foreign.as_ref(), Some(&selected_before));
    let source_after = input
        .source
        .as_ref()
        .zip(live_source)
        .map(|(fixture, brep)| rust_projection(&kernel, fixture, brep))
        .transpose()?;
    let foreign_after = input
        .foreign
        .as_ref()
        .zip(foreign.as_ref())
        .map(|(fixture, brep)| rust_projection(&kernel, fixture, brep))
        .transpose()?;
    let source_pure = source_state_before == source.as_ref().map(purity_state);
    let foreign_pure = foreign_state_before == foreign.as_ref().map(purity_state);
    let lifetime = if input.source.is_none() {
        0
    } else if source.is_some() {
        1
    } else if result_is_source {
        2
    } else {
        3
    };
    let mut output = scalar_output(
        status,
        transferred,
        lifetime,
        result_origin,
        [boolean_before, kernel.context.doing_boolean()],
        state.as_ref(),
        &selected_before,
        &selected_after,
    )
    .ints(
        "input.pure",
        vec![i64::from(source_pure), i64::from(foreign_pure)],
    )
;
    let heap_ordered = input.heap_ordered_vertices;
    output = project_result(output, "result", result_projection, heap_ordered);
    output = project_result(output, "executive_offset", offset_projection, heap_ordered);
    for (key, projection) in [
        ("source.before", source_before),
        ("source.after", source_after),
        ("foreign.before", foreign_before),
        ("foreign.after", foreign_after),
    ] {
        output = project(output, key, projection);
    }
    Ok(output)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "offset.offset_executive_p2"
    }
    fn run_cpp(&self, case: &Case) -> HResult<Output> {
        let input: Inputs = case.parse()?;
        // FFI argument errors are NEVER constructor rejections or native statuses.
        let outcome = ffi::eval(&input.native())
            .map_err(|error| bad(format!("native p2 adapter: {error}")))?;
        native_outcome(&input, outcome)
    }
    fn run_rust(&self, case: &Case) -> HResult<Output> {
        match rust_output(&case.parse()?) {
            Ok(output) => Ok(output),
            Err(RunError::Harness(error)) => Err(error),
            Err(RunError::Kernel(error)) => Ok(Output::unported(&error)
                .unwrap_or_else(|| Output::err(format!("Rust operation: {error}")))),
        }
    }
    fn cases(&self, context: &CaseContext) -> HResult<Vec<Case>> {
        let mut context = context.clone();
        context.seeds = context.seeds.max(MIN_GENERATED);
        default_cases(self, &context)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(generated(rng)).ok()
    }
}

fn sheet(origin: [f64; 3], scale: f64, axis: u64) -> Fixture {
    let (x_axis, y_axis) = match axis % 3 {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        _ => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    };
    Fixture::PlaneSheet {
        origin,
        x_axis,
        y_axis,
        scale: [scale, 2.0 * scale],
        domain: [-1.0, -0.5, 2.0, 1.5],
    }
}
fn wire(origin: [f64; 3], scale: f64) -> Fixture {
    let [x, y, z] = origin;
    Fixture::Wire {
        start: origin,
        end: [x + 3.0 * scale, y + scale, z - 2.0 * scale],
        domain: [-2.0, 3.0],
        interval: [-1.5, 2.25],
    }
}
fn generated(rng: &mut Rng) -> Inputs {
    // Seven of twelve categories call valid nonempty geometry paths. Early-return
    // cases carry real foreign/source geometry, not an inflated empty/guard corpus.
    let category = rng.below(12);
    let scale = match rng.below(5) {
        0 => 0.01,
        1 => 0.1,
        2 => 1.0,
        3 => 10.0,
        _ => 100.0,
    };
    let origin = rng.point3(-3.0, 3.0).map(|value| value * scale);
    let flags = rng.below(32);
    let tolerance = scale * 1e-7;
    let distance = match rng.below(8) {
        0 => -0.0,
        1 => 0.0,
        2 => 2.0 * tolerance,
        3 => -(2.0 * tolerance),
        _ => scale * rng.uniform(0.02, 0.15) * if rng.chance(0.5) { -1.0 } else { 1.0 },
    };
    let mut input = Inputs {
        operation: Operation::ApiOffset,
        source: Some(sheet(origin, scale, rng.below(3))),
        foreign: None,
        faces: Vec::new(),
        tolerance,
        config: Config {
            offset_distance: distance,
            member_distance: scale * rng.uniform(0.01, 0.2),
            member_approx_tol: match rng.below(7) {
                0 => -1.0,
                1 => -0.0,
                2 => 0.0,
                _ => tolerance,
            },
            member_operation: rng.below(5) as u32,
            inset: flags & 1 != 0,
            extend_convex: flags & 2 != 0,
            merge_results: flags & 4 != 0,
            create_offset_solid: flags & 8 != 0,
            skip_stitch_merge: flags & 16 != 0,
        },
        heap_ordered_vertices: false,
    };
    match category {
        0 | 1 => {
            input.operation = if category == 0 {
                Operation::Solid
            } else {
                Operation::Extended
            };
            input.source = None;
            let scale = match rng.below(4) {
                0 => 1e-9,
                1 => 1e9,
                _ => scale,
            };
            input.foreign = Some(wire([2.0 * scale, -scale, 3.0 * scale], scale));
            input.config.member_distance = match rng.below(5) {
                0 => -0.0,
                1 => 0.0,
                2 => 1e-150,
                3 => 1e100,
                _ => scale * 0.125,
            };
        }
        2 => {
            input.operation = if rng.chance(0.5) {
                Operation::ApiShell
            } else {
                Operation::ApiOffset
            };
            input.source = None;
            input.foreign = Some(sheet(origin, scale, rng.below(3)));
        }
        3 | 4 => {
            input.operation = Operation::ApiShell;
            input.foreign = Some(sheet(
                [origin[0] + 7.0 * scale, origin[1], origin[2]],
                scale,
                rng.below(3),
            ));
            input.faces = match rng.below(4) {
                0 => vec![
                    Selection::Source(0),
                    Selection::Source(0),
                    Selection::Foreign(0),
                ],
                1 => vec![Selection::Foreign(0), Selection::Null, Selection::Source(0)],
                2 => vec![Selection::Null, Selection::Foreign(0), Selection::Source(0)],
                _ => vec![Selection::Source(0), Selection::Null, Selection::Source(0)],
            };
        }
        5 | 6 | 11 => {
            input.operation = if category == 5 {
                Operation::ApiShell
            } else if category == 6 {
                Operation::ApiOffset
            } else {
                Operation::StaticShell
            };
            input.source = Some(Fixture::Box {
                origin,
                size: [2.0 * scale, 3.0 * scale, 4.0 * scale],
            });
            // Constructive boxes stay at moderate scales; extreme getter/guard
            // scalars are covered on genuine early paths rather than assuming
            // offset/stitch convergence over arbitrary native scales.
            input.config.offset_distance = if rng.chance(0.5) {
                0.05 * scale
            } else {
                -0.05 * scale
            };
            if category != 6 && rng.chance(0.7) {
                let index = rng.below(6) as usize;
                input.faces = vec![Selection::Source(index), Selection::Source(index)];
            }
        }
        7 => input.operation = Operation::StaticOffset,
        8 => {
            input.operation = Operation::StaticShell;
            if rng.chance(0.5) {
                input.faces = vec![Selection::Source(0)];
            }
        }
        9 => {
            input.operation = Operation::Solid;
            if rng.chance(0.3) {
                input.faces = vec![Selection::Source(0), Selection::Source(0)];
            }
        }
        _ => {
            input.operation = Operation::Extended;
            if rng.chance(0.5) {
                input.source = Some(wire(origin, scale));
            }
        }
    }
    if input.operation.member() && input.source.is_some() {
        input.config.member_approx_tol = tolerance;
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::case::repo_root;
    use crate::CaseSource;

    fn input(operation: Operation) -> Inputs {
        Inputs {
            operation,
            source: Some(sheet([1.0, -2.0, 3.0], 2.0, 0)),
            foreign: None,
            faces: Vec::new(),
            tolerance: 1e-6,
            config: Config::default(),
            heap_ordered_vertices: false,
        }
    }
    fn case(input: &Inputs) -> Case {
        Case {
            unit: Runner.id().into(),
            name: "unit-test".into(),
            source: CaseSource::Inline,
            inputs: serde_json::to_value(input).unwrap(),
        }
    }
    fn empty_block(tolerance: f64) -> ffi::BrepSnapshot {
        ffi::BrepSnapshot {
            integers: vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            values: vec![tolerance],
        }
    }
    fn empty_snapshot(status: i32) -> ffi::Snapshot {
        ffi::Snapshot {
            status,
            input_transferred: false,
            source_lifetime: 1,
            result_origin: 0,
            doing_boolean: [false, false],
            executive: None,
            selected_before: Vec::new(),
            selected_after: Vec::new(),
            source_before: Some(empty_block(1e-6)),
            source_after: Some(empty_block(1e-6)),
            foreign_before: None,
            foreign_after: None,
            result: None,
            executive_offset: None,
        }
    }

    #[test]
    fn serde_native_dtos_keep_all_operations_origins_and_raw_signed_distances() {
        for operation in [
            Operation::StaticOffset,
            Operation::StaticShell,
            Operation::Solid,
            Operation::Extended,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            for distance in [-1e100, -0.25, -1e-150, -0.0, 0.0, 0.25] {
                let mut original = input(operation);
                original.foreign = original.source.clone(); // DTO only, never a Brep copy.
                original.faces = vec![
                    Selection::Source(0),
                    Selection::Source(0),
                    Selection::Foreign(0),
                    Selection::Null,
                ];
                original.config.offset_distance = distance;
                original.config.member_distance = 0.375;
                original.config.inset = true;
                let decoded: Inputs =
                    serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
                assert_eq!(decoded, original);
                let request = decoded.native();
                assert_eq!(request.operation, operation.native());
                assert_eq!(request.config.offset_distance.to_bits(), distance.to_bits());
                assert_eq!(request.config.member_distance, 0.375);
                assert!(request.config.inset);
                assert_eq!(
                    request.faces,
                    vec![
                        ffi::FaceSelection::Source(0),
                        ffi::FaceSelection::Source(0),
                        ffi::FaceSelection::Foreign(0),
                        ffi::FaceSelection::Null
                    ]
                );
            }
        }
        let mut json = serde_json::to_value(input(Operation::ApiShell)).unwrap();
        json["unexpected"] = Value::Bool(true);
        assert!(serde_json::from_value::<Inputs>(json).is_err());
        assert!(serde_json::from_value::<Selection>(
            serde_json::json!({"origin":"source", "index":-1})
        )
        .is_err());
        assert!(
            serde_json::from_value::<Selection>(serde_json::json!({"origin":"foreign"})).is_err()
        );
        assert!(serde_json::from_value::<Selection>(
            serde_json::json!({"origin":"null", "index":0})
        )
        .is_err());
    }

    #[test]
    fn notification_keys_separate_arenas_entity_kinds_and_curve_records() {
        let edge = EdgeId::from_index(0);
        let vertex = VertexId::from_index(0);
        let source_edge = original_notification_key(EntityRef::Edge(edge));
        let result_edge = result_notification_key(EntityRef::Edge(edge));
        let source_vertex = original_notification_key(EntityRef::Vertex(vertex));
        let curve = result_curve_key(edge, 17001);
        let original_curve = original_curve_key(edge, 17001);
        let region = RegionId::from_index(0);
        let source_lists = boolean_source_lists(false, region);
        let result_lists = boolean_source_lists(true, region);
        let identities = [
            source_edge.key,
            result_edge.key,
            source_vertex.key,
            curve.key,
            original_curve.key,
            source_lists[0].key,
            source_lists[1].key,
            result_lists[0].key,
            result_lists[1].key,
        ];
        assert_eq!(identities.into_iter().collect::<BTreeSet<_>>().len(), 9);
        assert!(source_lists
            .into_iter()
            .chain(result_lists)
            .all(|h| h.type_code == 0));
        let mut source = build(&mut Kernel::default(), &sheet([0.0; 3], 1.0, 0), 1e-6, NAMESPACE_SOURCE).unwrap();
        let mut foreign = build(&mut Kernel::default(), &sheet([0.0; 3], 1.0, 0), 1e-6, NAMESPACE_FOREIGN).unwrap();
        let mut records = solidcore::topology::topology::NotifyRecords::new();
        register_notification_brep(&mut records, &mut source, NAMESPACE_SOURCE);
        let source_count = records.len();
        register_notification_brep(&mut records, &mut foreign, NAMESPACE_FOREIGN);
        assert_eq!(records.len(), 2 * source_count);
        assert_ne!(source.key(), foreign.key());
        assert!(records.contains_key(&source_edge.key));
        assert_eq!(
            original_notification_key(EntityRef::Edge(edge)),
            source_edge
        );
    }

    #[test]
    fn fixtures_designate_native_infinite_region_and_link_shells() {
        for (fixture, regions, shells) in [
            (Fixture::Empty, 1, 0),
            (wire([0.0; 3], 1.0), 1, 1),
            (sheet([0.0; 3], 1.0, 0), 1, 1),
            (
                Fixture::Box {
                    origin: [0.0; 3],
                    size: [10.0; 3],
                },
                2,
                2,
            ),
        ] {
            let brep = build(&mut Kernel::default(), &fixture, 1e-6, NAMESPACE_SOURCE).unwrap();
            assert_eq!(brep.regions().len(), regions);
            assert_eq!(brep.shells().len(), shells);
            let infinite = brep.infinite_region().unwrap();
            assert_eq!(brep.region(infinite).unwrap().is_void(), 1);
            for (region_id, region) in brep.regions().iter() {
                assert!(region.in_brep);
                assert_eq!(region.is_void(), i32::from(region_id == infinite));
                for &shell in &region.shells {
                    assert_eq!(brep.shell(shell).unwrap().region, Some(region_id));
                }
            }
            for (shell_id, shell) in brep.shells().iter() {
                let region = brep.region(shell.region.unwrap()).unwrap();
                assert_eq!(
                    region.shells.iter().filter(|&&id| id == shell_id).count(),
                    1
                );
            }
        }
    }

    #[test]
    fn source_face_origin_cannot_alias_foreign_face_zero_and_order_is_preserved() {
        let mut input = input(Operation::ApiShell);
        input.foreign = Some(sheet([20.0, 3.0, -4.0], 3.0, 1));
        let source = build(
            &mut Kernel::default(),
            input.source.as_ref().unwrap(),
            input.tolerance,
            NAMESPACE_SOURCE,
        )
        .unwrap();
        let foreign = build(
            &mut Kernel::default(),
            input.foreign.as_ref().unwrap(),
            input.tolerance,
            NAMESPACE_FOREIGN,
        )
        .unwrap();
        assert_eq!(
            face_at(Some(&source), 0).unwrap(),
            face_at(Some(&foreign), 0).unwrap()
        );
        for selectors in [
            vec![
                Selection::Source(0),
                Selection::Source(0),
                Selection::Foreign(0),
            ],
            vec![Selection::Source(0), Selection::Null, Selection::Foreign(0)],
            vec![Selection::Null, Selection::Foreign(0), Selection::Source(0)],
        ] {
            input.faces = selectors.clone();
            let keys = selections(&input, Some(&source), Some(&foreign)).unwrap();
            let expected: Vec<_> = selectors
                .iter()
                .map(|selection| match selection {
                    Selection::Source(index) => [0, *index as i64],
                    Selection::Foreign(index) => [1, *index as i64],
                    Selection::Null => [2, -1],
                })
                .collect();
            assert_eq!(
                observe_selections(&keys, Some(&source), Some(&foreign), None),
                expected
            );
            input.config.offset_distance = 0.0;
            let output = Runner.run_rust(&case(&input)).unwrap();
            assert_eq!(output.ints["status"], [1007]);
            assert_eq!(output.ints["input.pure"], [1, 1]);
            assert_eq!(output.ints["ownership"], [0, 1, 0]);
            assert_eq!(
                output.ints["selected.before"],
                output.ints["selected.after"]
            );
            assert_eq!(
                output.values["source.before.geometry"],
                output.values["source.after.geometry"]
            );
            assert_eq!(
                output.values["foreign.before.geometry"],
                output.values["foreign.after.geometry"]
            );
            assert_ne!(
                output.values["source.before.geometry"],
                output.values["foreign.before.geometry"]
            );
            assert!(output.error.is_none() && output.deferred.is_none());
        }
        let mut valid = input;
        valid.faces = vec![Selection::Source(0), Selection::Source(0)];
        // a valid (duplicated) selection is mapped through the working copy
        let test_case = case(&valid);
        let output = Runner.run_rust(&test_case).unwrap();
        assert!(output.deferred.is_none(), "{:?}", output.deferred);
        assert_eq!(output.ints["input.pure"], [1, 1]);
        let native = Runner.run_cpp(&test_case).unwrap();
        assert!(crate::compare::compare(&native, &output, Runner.abs_floors()).pass());
    }

    #[test]
    fn member_null_preserves_every_flag_enum_magnitude_and_constructor_scalar() {
        for operation in [Operation::Solid, Operation::Extended] {
            for bits in 0..32 {
                let mut input = input(operation);
                input.source = None;
                input.foreign = Some(wire([4.0, -2.0, 7.0], 3.0));
                input.config.inset = bits & 1 != 0;
                input.config.extend_convex = bits & 2 != 0;
                input.config.merge_results = bits & 4 != 0;
                input.config.create_offset_solid = bits & 8 != 0;
                input.config.skip_stitch_merge = bits & 16 != 0;
                input.config.member_operation = bits % 5;
                input.config.member_distance = match bits % 4 {
                    0 => -0.0,
                    1 => 1e-150,
                    2 => 1e100,
                    _ => 0.375,
                };
                input.config.member_approx_tol = match bits % 4 {
                    0 => -0.0,
                    1 => -1.0,
                    2 => -0.25,
                    _ => 1e-14,
                };
                let output = Runner.run_rust(&case(&input)).unwrap();
                assert_eq!(output.ints["status"], [1006]);
                assert_eq!(
                    output.ints["executive.flags"],
                    [
                        i64::from(bits % 5),
                        i64::from(bits & 1 != 0),
                        i64::from(bits & 2 != 0),
                        i64::from(bits & 4 != 0),
                        i64::from(bits & 8 != 0)
                    ]
                );
                assert_eq!(
                    output.values["executive.geometry_parameters"][0].to_bits(),
                    input.config.member_distance.to_bits()
                );
                assert_eq!(
                    output.values["executive.geometry_parameters"][1].to_bits(),
                    input.config.member_approx_tol.to_bits()
                );
                assert_eq!(output.values["executive.geometry_parameters"][2], 2.0);
                assert_eq!(output.ints["doing_boolean"], [0, 0]);
                assert_eq!(output.ints["ownership"], [0, 0, 0]);
                assert_eq!(output.ints["input.pure"], [1, 1]);
                assert_eq!(output.ints["executive_offset.present"], [0]);
                assert!(!output.values["foreign.before.geometry"].is_empty());
            }
        }
    }

    #[test]
    fn api_null_is_1007_not_member_ner_or_a_constructor_rejection() {
        for operation in [Operation::ApiShell, Operation::ApiOffset] {
            let mut input = input(operation);
            input.source = None;
            input.foreign = Some(wire([0.0, 3.0, 5.0], 2.0));
            let output = Runner.run_rust(&case(&input)).unwrap();
            assert_eq!(output.ints["status"], [1007]);
            assert_eq!(output.ints["executive.present"], [0]);
            assert_eq!(output.ints["ownership"], [0, 0, 0]);
            assert!(output.rejected.is_none() && output.error.is_none());
            assert!(!output.values["foreign.before.geometry"].is_empty());
        }
    }

    #[test]
    fn ordinary_statuses_remain_integer_observations_and_unported_never_becomes_1001() {
        let mut input = input(Operation::ApiOffset);
        input.source = Some(Fixture::Empty);
        let mut outputs = Vec::new();
        for status in [1001, 1006, 1007] {
            let error = SolidError::Upstream {
                operation: "test",
                code: status,
                entity: None,
            };
            assert_eq!(operation::<Brep>(Err(error)).unwrap().0, status);
            let output = native_outcome(
                &input,
                ffi::Outcome::Built(Box::new(empty_snapshot(status))),
            )
            .unwrap();
            assert!(output.error.is_none() && output.rejected.is_none());
            assert_eq!(output.ints["status"], [i64::from(status)]);
            assert_eq!(output.values["source.before.geometry"], [1e-6]);
            outputs.push(output);
        }
        assert_ne!(outputs[0], outputs[1]);
        assert_ne!(outputs[1], outputs[2]);
        let error = SolidError::unported("SmBrep::ShrinkGeometry", "topology.brep_p4");
        assert!(matches!(
            operation::<Brep>(Err(error)),
            Err(RunError::Kernel(SolidError::Unported { .. }))
        ));
        let rejected = native_outcome(&input, ffi::Outcome::Rejected { status: 1007 }).unwrap();
        assert!(rejected.rejected.is_some());
        assert!(rejected.ints.is_empty() && rejected.values.is_empty());
    }

    #[test]
    fn ffi_argument_error_is_a_harness_error_not_output_rejected() {
        // These fail before native allocation, and do not rely on native geometry
        // convergence or execute one of the positive operation paths.
        let mut input = input(Operation::StaticOffset);
        input.source = None;
        assert!(Runner.run_cpp(&case(&input)).is_err());
        input.operation = Operation::Solid;
        input.config.member_distance = -1.0;
        assert!(Runner.run_cpp(&case(&input)).is_err());
    }

    #[test]
    fn irrelevant_extension_overflow_does_not_hide_native_early_returns() {
        for operation in [
            Operation::Solid,
            Operation::Extended,
            Operation::ApiShell,
            Operation::ApiOffset,
        ] {
            let mut input = input(operation);
            input.source = None;
            input.config.member_distance = f64::MAX;
            input.config.offset_distance = -f64::MAX;
            input.config.extend_convex = true;
            let case = case(&input);
            assert_eq!(
                Runner.run_rust(&case).unwrap(),
                Runner.run_cpp(&case).unwrap()
            );
            input.config.member_distance = -1.0;
            if operation.member() {
                assert!(validate(&input).is_err());
            }
        }
        let mut input = input(Operation::ApiShell);
        input.source = Some(sheet([0.0; 3], 1.0, 0));
        input.foreign = input.source.clone();
        input.faces = vec![Selection::Foreign(0), Selection::Null];
        input.config.extend_convex = true;
        for distance in [-f64::MAX, f64::MAX] {
            input.config.offset_distance = distance;
            let case = case(&input);
            assert_eq!(
                Runner.run_rust(&case).unwrap(),
                Runner.run_cpp(&case).unwrap()
            );
        }
        input.faces.push(Selection::Source(usize::MAX));
        assert!(validate(&input).is_err());
    }

    #[test]
    fn block_decode_validates_even_fields_excluded_from_the_projection() {
        let fixture = Fixture::Empty;
        let block = empty_block(3e-12);
        let projection = native_projection(&fixture, &decode_block(&block).unwrap()).unwrap();
        let brep = build(&mut Kernel::default(), &fixture, 3e-12, NAMESPACE_SOURCE).unwrap();
        assert_eq!(
            projection,
            rust_projection(&Kernel::default(), &fixture, &brep).unwrap()
        );
        let mut broken = block.clone();
        broken.integers.pop(); // Missing native-only region void flag.
        assert!(decode_block(&broken).is_err());
        let mut broken = block.clone();
        broken.integers[11] = 2; // Invalid flag, not silently skipped.
        assert!(decode_block(&broken).is_err());
        let mut broken = block.clone();
        broken.integers[7] = 1; // Invalid infinite-region traversal id.
        assert!(decode_block(&broken).is_err());
        let mut broken = block.clone();
        broken.integers[0] = i64::MAX;
        assert!(decode_block(&broken).is_err());
        let mut broken = block.clone();
        broken.values.push(0.0);
        assert!(decode_block(&broken).is_err());
        let mut broken = block;
        broken.integers.push(0);
        assert!(decode_block(&broken).is_err());
    }

    #[test]
    fn sheet_projection_keeps_domains_points_normals_kinds_and_evaluation_statuses() {
        // A small synthetic decoder record, not a C++ operation result used by
        // run_rust. The count/adjacency framing must be consumed in native order.
        let mut ints = vec![
            1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 2, 0,
        ];
        ints.extend([1000; 6]);
        let mut values = vec![1e-6, 1e-6];
        values.extend([0.0, 0.0, 2.0, 4.0]);
        values.extend([0.0, 0.0, 2.0, 4.0]);
        values.extend([
            0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 0.0, 0.0, 0.0,
            1.0,
        ]);
        let mut block = ffi::BrepSnapshot {
            integers: ints,
            values,
        };
        let decoded = decode_block(&block).unwrap();
        let fixture = sheet([0.0; 3], 1.0, 0);
        let projection = native_projection(&fixture, &decoded).unwrap();
        assert_eq!(projection.kinds, [1]);
        assert_eq!(projection.statuses, [1000; 6]);
        assert_eq!(projection.values.len(), 27);
        assert_eq!(projection.values[1..5], [0.0, 0.0, 2.0, 4.0]);
        assert_eq!(projection.values[21..27], [2.0, 4.0, 0.0, 0.0, 0.0, 1.0]);
        block.integers[20] = 1; // A counted loop list with no valid loop traversal.
        assert!(decode_block(&block).is_err());
        block.integers[20] = 0;
        block.values.pop();
        assert!(decode_block(&block).is_err());
    }

    #[test]
    fn native_only_input_metadata_mutations_are_detected_by_full_local_purity_check() {
        let mut input = input(Operation::ApiOffset);
        input.source = Some(Fixture::Empty);
        let mut snapshot = empty_snapshot(1007);
        let baseline = native_output(&input, &snapshot).unwrap();
        snapshot.source_after.as_mut().unwrap().integers[8] = 1;
        let changed = native_output(&input, &snapshot).unwrap();
        assert_eq!(baseline.values, changed.values);
        assert_eq!(baseline.ints["input.pure"], [1, 1]);
        assert_eq!(changed.ints["input.pure"], [0, 1]);
        snapshot.source_after = snapshot.source_before.clone();
        snapshot.source_after.as_mut().unwrap().values[0] = 2e-6;
        assert_eq!(
            native_output(&input, &snapshot).unwrap().ints["input.pure"],
            [0, 1]
        );
    }

    /// A path that completes and returns a compared result B-rep.
    const RESULT: &str = "result";

    fn assert_reaches(output: &Output, symbol: &str) {
        if symbol == RESULT {
            assert!(output.deferred.is_none(), "{:?}", output.deferred);
            assert!(output.error.is_none(), "{:?}", output.error);
            assert_eq!(output.ints["result.present"], [1]);
        } else {
            assert_eq!(output.deferred.as_ref().unwrap().symbol, symbol);
        }
    }

    #[test]
    fn positive_geometry_and_strict_boundaries_preserve_explicit_partial_paths() {
        for (operation, fixture, extend, symbol) in [
            // An empty rounded offset completes and returns its (empty) result.
            (Operation::StaticOffset, Fixture::Empty, false, RESULT),
            (Operation::Solid, sheet([0.0; 3], 1.0, 0), false, RESULT),
            (Operation::Extended, sheet([0.0; 3], 1.0, 0), true, RESULT),
        ] {
            for distance in [0.0, 1.9999999999999995e-6, 2e-6, 2.0000000000000003e-6] {
                let mut input = input(operation);
                input.source = Some(fixture.clone());
                input.config.offset_distance = distance;
                input.config.member_distance = distance;
                input.config.extend_convex = extend;
                let output = Runner.run_rust(&case(&input)).unwrap();
                assert_reaches(&output, symbol);
            }
        }
        // The pure shell API copies the empty source and shells the copy: the strict
        // guard is 1007 below 2*tolerance, at and above it the empty result returns.
        for distance in [0.0, 1.9999999999999995e-6, 2e-6, 2.0000000000000003e-6] {
            let mut input = input(Operation::ApiShell);
            input.source = Some(Fixture::Empty);
            input.config.offset_distance = distance;
            let test_case = case(&input);
            let output = Runner.run_rust(&test_case).unwrap();
            assert!(output.deferred.is_none(), "{:?}", output.deferred);
            if distance < 2e-6 {
                assert_eq!(output.ints["status"], [1007]);
            } else {
                assert_reaches(&output, RESULT);
            }
            assert_eq!(output.ints["input.pure"], [1, 1]);
            let native = Runner.run_cpp(&test_case).unwrap();
            assert!(crate::compare::compare(&native, &output, Runner.abs_floors()).pass());
        }
        // ShellBrep asks IsManifoldSolid before checking its strict radius guard.
        // Empty breps now have a native IsManifoldSolid implementation, so this
        // path reaches SM_ERR_INVALID_INPUT for |distance| < 2*tolerance instead
        // of deferring at the former topology back-edge. The operation adapter
        // represents this ordinary failure as the scalar status, not Output.error.
        for distance in [0.0, 1.9999999999999995e-6, -1.9999999999999995e-6, -0.0] {
            let mut input = input(Operation::StaticShell);
            input.source = Some(Fixture::Empty);
            input.config.offset_distance = distance;
            let test_case = case(&input);
            let output = Runner.run_rust(&test_case).unwrap();
            assert!(output.deferred.is_none());
            assert!(output.error.is_none());
            assert_eq!(output.ints["status"], [1007]);
            assert_eq!(output.ints["ownership"], [0, 1, 0]);
            assert_eq!(output.ints["doing_boolean"], [0, 0]);
            assert_eq!(output.ints["source.after.present"], [1]);
            let native = Runner.run_cpp(&test_case).unwrap();
            assert!(native.deferred.is_none());
            assert_eq!(native.ints["status"], output.ints["status"]);
            assert_eq!(native.ints["ownership"], output.ints["ownership"]);
            assert_eq!(native.ints["doing_boolean"], output.ints["doing_boolean"]);
            assert!(crate::compare::compare(&native, &output, Runner.abs_floors()).pass());
        }
        // The guard is strict: exactly 2*tolerance and the adjacent value above
        // it proceed into DoSolidOffset, which completes on the empty input.
        for distance in [2e-6, 2.0000000000000003e-6, -2e-6, -2.0000000000000003e-6] {
            let mut input = input(Operation::StaticShell);
            input.source = Some(Fixture::Empty);
            input.config.offset_distance = distance;
            assert_reaches(&Runner.run_rust(&case(&input)).unwrap(), RESULT);
        }
        let box_fixture = Fixture::Box {
            origin: [0.0; 3],
            size: [10.0; 3],
        };
        let brep = build(&mut Kernel::default(), &box_fixture, 1e-6, NAMESPACE_SOURCE).unwrap();
        let projection = rust_projection(&Kernel::default(), &box_fixture, &brep).unwrap();
        assert_eq!(projection.counts, [6, 12, 8]);
        assert_eq!(projection.kinds, [1; 6]);
        assert_eq!(projection.values, [1e-6, 0.0, 0.0, 0.0, 10.0, 10.0, 10.0]);
        let mut input = input(Operation::ApiShell);
        input.source = Some(box_fixture);
        input.faces = vec![Selection::Source(0), Selection::Source(0)];
        // the working copy is made and the shell completes: its faces' surfaces reach
        // their faces' trimmed caches
        let output = Runner.run_rust(&case(&input)).unwrap();
        assert!(output.deferred.is_none());
        assert_eq!(output.ints.get("result.present"), Some(&vec![1]));
    }

    #[test]
    fn face_geometry_paths_reach_distinct_actual_callees() {
        // MakeWireEdgeVertex now runs, and concrete-Surface CrvOnSurf construction
        // lets the rounded and extended paths through DropCurve, ShrinkGeometry and
        // the edge-use offset creator and PiecewiseMerge: they stop at the gap-fill
        // evaluation, or return a result B-rep that is compared.
        let context = CaseContext {
            root: repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        for (name, owner, symbol) in [
            ("static_offset_real_plane_sheet_round", "", RESULT),
            (
                "static_offset_real_plane_sheet_extended",
                "",
                RESULT,
            ),
            (
                "member_sheet_round_skip_true_still_real_geometry",
                "",
                RESULT,
            ),
            // (their face solves reach the faces' surface caches)
            (
                "member_sheet_extended_dispatch_ignores_skip",
                "",
                RESULT,
            ),
            ("extended_sheet_direct_independent_flags", "", RESULT),
        ] {
            let case = fixtures.iter().find(|case| case.name == name).unwrap();
            let output = Runner.run_rust(case).unwrap();
            assert_reaches(&output, symbol);
            if symbol != RESULT {
                assert_eq!(output.deferred.unwrap().unit, owner, "{name}");
            }
        }
    }

    #[test]
    fn empty_native_boxes_are_compared_unless_a_face_is_selected() {
        let context = CaseContext {
            root: repo_root(),
            seeds: 0,
        };
        let mut rejected = 0;
        let mut compared = 0;
        for case in context.fixture_cases(Runner.id()).unwrap() {
            if !case.name.starts_with("review_box_") {
                continue;
            }
            let cpp = Runner.run_cpp(&case).unwrap();
            assert!(cpp.error.is_none() && cpp.deferred.is_none());
            if case.name.ends_with("_control") {
                let rust = Runner.run_rust(&case).unwrap();
                for output in [&cpp, &rust] {
                    assert!(output.rejected.is_none() && output.error.is_none());
                    assert!(output.deferred.is_none());
                    assert_eq!(output.ints["status"], [1007]);
                    assert_eq!(output.ints["input.pure"], [1, 1]);
                    assert!(!output.values.is_empty());
                }
                compared += 1;
            } else if case.name.ends_with("_selected") {
                // The tiny box is an empty B-rep: no face 0 to select.
                assert_eq!(
                    cpp.rejected.as_deref(),
                    Some("native fixture constructor status_1001")
                );
                assert!(cpp.ints.is_empty() && cpp.values.is_empty());
                rejected += 1;
            } else {
                // Null selection: the empty box is built and the API status
                // (1007) is compared against Rust.
                let rust = Runner.run_rust(&case).unwrap();
                for output in [&cpp, &rust] {
                    assert!(output.rejected.is_none() && output.error.is_none());
                    assert_eq!(output.ints["status"], [1007]);
                }
                let comparison = crate::compare::compare(&cpp, &rust, Runner.abs_floors());
                assert!(comparison.pass(), "{}: {comparison:?}", case.name);
                compared += 1;
            }
        }
        assert_eq!((rejected, compared), (4, 6));
    }

    fn assert_native_constructor_control(case: &Case, cpp: &Output, counts: [i64; 5]) {
        let input: Inputs = case.parse().unwrap();
        let rust = Runner.run_rust(case).unwrap();
        for output in [cpp, &rust] {
            assert!(output.rejected.is_none() && output.error.is_none());
            assert!(output.deferred.is_none());
            let status = if input.operation.member() { 1006 } else { 1007 };
            assert_eq!(output.ints["status"], [status]);
            assert_eq!(output.ints["input.pure"], [1, 1]);
            assert_eq!(
                output.ints["ownership"],
                [0, i64::from(input.source.is_some()), 0]
            );
            assert_eq!(
                output.ints["selected.before"],
                output.ints["selected.after"]
            );
            for (key, fixture) in [
                ("source", input.source.as_ref()),
                ("foreign", input.foreign.as_ref()),
            ] {
                if fixture.is_some() {
                    for when in ["before", "after"] {
                        assert_eq!(output.ints[&format!("{key}.{when}.counts")], counts);
                        assert!(!output.values[&format!("{key}.{when}.geometry")].is_empty());
                    }
                }
            }
        }
        let comparison = crate::compare::compare(cpp, &rust, Runner.abs_floors());
        assert!(comparison.pass(), "{}: {comparison:?}", case.name);
        assert!(comparison.values_compared > 0 && comparison.exact_compared > 0);
    }

    #[test]
    fn degenerate_native_sheets_are_compared_alongside_valid_controls() {
        let context = CaseContext {
            root: repo_root(),
            seeds: 0,
        };
        let (rejected, mut compared) = (0, 0);
        for case in context.fixture_cases(Runner.id()).unwrap() {
            if !case.name.starts_with("review_sheet_") {
                continue;
            }
            let cpp = Runner.run_cpp(&case).unwrap();
            assert!(cpp.error.is_none() && cpp.deferred.is_none());
            if case.name.ends_with("_control") {
                assert_native_constructor_control(&case, &cpp, [1, 1, 4, 8, 4]);
                compared += 1;
            } else {
                // Merged/dropped-face sheets are defined native results, built
                // and compared as is (not rejections).
                let rust = Runner.run_rust(&case).unwrap();
                assert!(cpp.rejected.is_none() && rust.rejected.is_none());
                let comparison = crate::compare::compare(&cpp, &rust, Runner.abs_floors());
                assert!(comparison.pass(), "{}: {comparison:?}", case.name);
                assert!(comparison.exact_compared > 0);
                compared += 1;
            }
        }
        assert_eq!((rejected, compared), (0, 13));
    }

    #[test]
    fn stable_native_wire_controls_compare_topology_and_geometry_at_extremes() {
        let context = CaseContext {
            root: repo_root(),
            seeds: 0,
        };
        let mut compared = 0;
        for case in context.fixture_cases(Runner.id()).unwrap() {
            if !case.name.starts_with("review_wire_") {
                continue;
            }
            let cpp = Runner.run_cpp(&case).unwrap();
            assert_native_constructor_control(&case, &cpp, [0, 0, 1, 2, 2]);
            compared += 1;
        }
        assert_eq!(compared, 5);
    }

    #[test]
    fn fixed_fixtures_parse_and_case_budget_is_reproducible_without_running_cpp() {
        let context = CaseContext {
            root: repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        assert!(fixtures.len() >= 40);
        let mut operations = BTreeSet::new();
        for case in &fixtures {
            let input: Inputs = case.parse().unwrap();
            validate(&input).unwrap();
            operations.insert(input.operation.native() as i32);
        }
        assert_eq!(operations.len(), 6);
        let all = Runner.cases(&context).unwrap();
        assert_eq!(all.len(), fixtures.len() + MIN_GENERATED);
        assert_eq!(all, Runner.cases(&context).unwrap());
        assert!(Runner.abs_floors().is_empty());
    }

    #[test]
    fn generated_coverage_is_finite_diverse_nonempty_and_mostly_real_geometry_paths() {
        let mut operations = BTreeSet::new();
        let mut flags = BTreeSet::new();
        let mut enums = BTreeSet::new();
        let (mut positive, mut early, mut boxes, mut wires, mut negatives, mut negative_zeros) =
            (0, 0, 0, 0, 0, 0);
        for index in 0..MIN_GENERATED {
            let seed = CaseContext::seed_for(Runner.id(), index);
            let input = generated(&mut Rng::new(seed));
            validate(&input).unwrap();
            assert_eq!(input, generated(&mut Rng::new(seed)));
            let roundtrip: Inputs =
                serde_json::from_value(serde_json::to_value(&input).unwrap()).unwrap();
            assert_eq!(roundtrip, input);
            operations.insert(input.operation.native() as i32);
            let cfg = &input.config;
            flags.insert((
                cfg.inset,
                cfg.extend_convex,
                cfg.merge_results,
                cfg.create_offset_solid,
                cfg.skip_stitch_merge,
            ));
            enums.insert(cfg.member_operation);
            negatives += usize::from(cfg.offset_distance < 0.0);
            negative_zeros +=
                usize::from(cfg.offset_distance == 0.0 && cfg.offset_distance.is_sign_negative());
            boxes += usize::from(matches!(input.source, Some(Fixture::Box { .. })));
            wires += usize::from(
                input
                    .source
                    .iter()
                    .chain(&input.foreign)
                    .any(|fixture| matches!(fixture, Fixture::Wire { .. })),
            );
            let early_selection = input.operation == Operation::ApiShell
                && input
                    .faces
                    .iter()
                    .any(|face| !matches!(face, Selection::Source(_)));
            let output = Runner.run_rust(&case(&input)).unwrap();
            if input.source.is_none() || early_selection {
                early += 1;
                assert!(output.deferred.is_none());
                assert!(output.error.is_none());
                assert!(!output.values.is_empty());
            } else if input.operation == Operation::StaticShell
                && cfg.offset_distance.abs() < 2.0 * input.tolerance
            {
                // The real manifold query now reaches the native radius guard
                // even for nonempty geometry; this is not a constructed result.
                early += 1;
                assert!(output.deferred.is_none());
                assert!(output.error.is_none());
                assert_eq!(output.ints["status"], [1007]);
                assert_eq!(output.ints["ownership"], [0, 1, 0]);
                assert_eq!(output.ints["result.present"], [0]);
                let native = Runner.run_cpp(&case(&input)).unwrap();
                assert!(crate::compare::compare(&native, &output, Runner.abs_floors()).pass());
            } else {
                positive += 1;
                // Nonempty geometry either reaches a missing dependency or is
                // compared through its real status and result projection.
                assert!(
                    output.deferred.is_some() || output.error.is_none(),
                    "nonempty geometry must not be guessed: {input:?}"
                );
            }
        }
        assert_eq!(operations.len(), 6);
        assert_eq!(enums.len(), 5);
        assert!(flags.len() >= 24);
        assert!(positive > MIN_GENERATED / 2 && early > MIN_GENERATED / 4);
        assert!(boxes > 25 && wires > 25 && negatives > 40 && negative_zeros > 5);
    }
}
