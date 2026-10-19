//! Runner for `offset.offset_executive_p1` (PR #175).
//!
//! C++: `solidcore_ffi::units::offset_offset_executive_p1`; Rust:
//! `solidcore::offset::offset_executive::p1`. Native statuses remain unmodified
//! (`SM_SUCCESS = 1000`); the snapshot's leading zero is only a fixture tag.
//!
//! Deviations: private `GetSeconds` arrays are parsed for snapshot-layout validation
//! only: neither their membership nor pointer-hashed order is compared. The public
//! Rust executive does not expose relation enumeration; exact-one lookups ARE
//! compared. Following the `SmRelation` convention in `plan/findings.md`, only
//! contiguous groups of exactly equal measured minimum parameters in the four
//! synchronization lists are canonicalized by traversal ID on both engines.
//! Distinct-minimum order, membership, null padding and all geometry stay unchanged.
//! Measured traversal excludes the closing support edges and visits each measured
//! edge's primary then mate use. Exactly coincident measured endpoints use a
//! nonzero closed cubic as unmeasured support: the native closing-segment factory
//! rejects coincident endpoints. Measured zero trims and constant B-splines stay
//! unchanged; coincident analytic-line constructor rejection is not parity.
//! Extension scripts use real Get calls only: the public Rust API has no nullable
//! Get, Forget or Clear. Collapsed two-point isocurves are not parity fixtures:
//! the native safety preflight rejects them as adapter errors before extension.
//! Extension distances are internal nonnegative magnitudes (entrypoints use `fabs`
//! and `SetInset`); signed config/getter coverage remains separate and read-only.
//! Cache metadata tracks identities after successful calls,
//! never substitutes a saved surface for a call. Every numeric key has zero floor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::context::MarkType;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::Kernel;
use solidcore::math::{Vec3, PI};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopEdge, Loopuse, LoopuseStart, Shell, Vertex, VertexId, Vertexuse, VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::offset_offset_executive_p1 as ffi;

use crate::runner::default_cases;
use crate::util::{flag, p2, v3};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// Minimum deterministic generated cases, even with `--seeds 0`.
pub const MIN_GENERATED: usize = 600;
/// Offset executive runner, automatically registered by the build script.
pub struct Runner;

type HResult<T> = Result<T, HarnessError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Inputs {
    Config(Config),
    Synchronize(Synchronize),
    Extend(Extension),
    Probe { branch: Probe, approx_tol: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Independent upstream settings.
#[serde(deny_unknown_fields)]
struct Config {
    offset_distance: f64,
    approx_tol: f64,
    #[serde(default = "default_tangency")]
    tangency_tol_radians: f64,
    operation: u32,
    inset: bool,
    extend_convex: bool,
    merge_results: bool,
    create_solid: bool,
    install_offset_brep: bool,
    original_edges: usize,
    offset_edges: usize,
    face_count: usize,
    shell_face_sets: Vec<Vec<usize>>,
    relations: Vec<[usize; 2]>,
}

fn default_tangency() -> f64 {
    2.0 * PI / 180.0
}

impl Config {
    fn ffi(&self) -> ffi::Config {
        ffi::Config {
            offset_distance: self.offset_distance,
            approx_tol: self.approx_tol,
            tangency_tol_radians: self.tangency_tol_radians,
            operation: self.operation,
            inset: self.inset,
            extend_convex: self.extend_convex,
            merge_results: self.merge_results,
            create_solid: self.create_solid,
            install_offset_brep: self.install_offset_brep,
            original_edges: self.original_edges,
            offset_edges: self.offset_edges,
            face_count: self.face_count,
            shell_face_sets: self.shell_face_sets.clone(),
            relations: self.relations.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CurveIn {
    Line {
        start: [f64; 3],
        end: [f64; 3],
        domain: [f64; 2],
    },
    Bspline {
        degree: usize,
        control_points: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
}

impl CurveIn {
    fn ffi(&self) -> ffi::CurveFixture {
        match self {
            Self::Line { start, end, domain } => ffi::CurveFixture::Line {
                start: *start,
                end: *end,
                domain: *domain,
            },
            Self::Bspline {
                degree,
                control_points,
                knots,
            } => ffi::CurveFixture::BSpline {
                degree: *degree,
                control_points: control_points.clone(),
                knots: knots.clone(),
            },
        }
    }

    fn rust(&self) -> HResult<Curve> {
        match self {
            Self::Line { start, end, domain } => {
                let mut line = Line::from_points(v3(*start), v3(*end), 3);
                line.edit_parameterization(&ivl(*domain)).map_err(setup)?;
                Ok(Curve::from(line))
            }
            Self::Bspline {
                degree,
                control_points,
                knots,
            } => {
                let points: Vec<_> = control_points.iter().copied().map(cpoint).collect();
                let last = points
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| bad("empty curve"))?;
                let last_knot = knots
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| bad("empty knots"))?;
                Ok(Curve::from(
                    BSplineCurve::from_data(3, last, last_knot, *degree, &points, knots)
                        .map_err(setup)?,
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeIn {
    curve: CurveIn,
    interval: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Synchronize {
    first: Vec<EdgeIn>,
    second: Vec<EdgeIn>,
    #[serde(default)]
    null_original: bool,
    #[serde(default)]
    null_other: bool,
}

impl Synchronize {
    fn ffi(&self) -> ffi::Synchronize {
        let edges = |list: &[EdgeIn]| {
            list.iter()
                .map(|edge| ffi::EdgeFixture {
                    curve: edge.curve.ffi(),
                    interval: edge.interval,
                })
                .collect()
        };
        ffi::Synchronize {
            first: edges(&self.first),
            second: edges(&self.second),
            null_original: self.null_original,
            null_other: self.null_other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SurfaceIn {
    Plane {
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Bspline {
        u_degree: usize,
        v_degree: usize,
        u_count: usize,
        v_count: usize,
        control_points: Vec<[f64; 4]>,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
    },
}

impl SurfaceIn {
    fn ffi(&self) -> ffi::SurfaceFixture {
        match self {
            Self::Plane {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            } => ffi::SurfaceFixture::Plane {
                origin: *origin,
                x_axis: *x_axis,
                y_axis: *y_axis,
                scale: *scale,
                domain: *domain,
            },
            Self::Bspline {
                u_degree,
                v_degree,
                u_count,
                v_count,
                control_points,
                u_knots,
                v_knots,
            } => ffi::SurfaceFixture::BSpline {
                u_degree: *u_degree,
                v_degree: *v_degree,
                u_count: *u_count,
                v_count: *v_count,
                control_points: control_points.clone(),
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
            },
        }
    }

    fn rust(&self) -> HResult<Surface> {
        match self {
            Self::Plane {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            } => {
                let [u0, v0, u1, v1] = *domain;
                Ok(Surface::from(Plane::new(
                    v3(*origin),
                    v3(*x_axis),
                    v3(*y_axis),
                    p2(*scale),
                    &Extent2d::from_bounds(u0, v0, u1, v1),
                )))
            }
            Self::Bspline {
                u_degree,
                v_degree,
                u_count,
                v_count,
                control_points,
                u_knots,
                v_knots,
            } => {
                if *v_count == 0 || u_count.checked_mul(*v_count) != Some(control_points.len()) {
                    return Err(bad("invalid surface net"));
                }
                let net = control_points
                    .chunks_exact(*v_count)
                    .map(|row| row.iter().copied().map(cpoint).collect())
                    .collect();
                // Match the shim's owned gw_SURFACE constructor, not from_data:
                // upstream's from_data has a different asymmetric-knot copy bug.
                let raw =
                    NurbSurface::new(net, *u_degree, *v_degree, u_knots.clone(), v_knots.clone())
                        .map_err(setup)?;
                let mut surface = BSplineSurface::from_nurb(raw, false);
                surface.set_b_spline_surface_form(BSplineSurfaceForm::Unspecified);
                Ok(Surface::from(surface))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Extension {
    surfaces: Vec<SurfaceIn>,
    offset_distance: f64,
    approx_tol: f64,
    /// Each key is a Get action, including repeated and distinct identities.
    actions: Vec<usize>,
    samples: Vec<[f64; 2]>,
}

impl Extension {
    fn ffi(&self) -> ffi::Extension {
        ffi::Extension {
            surfaces: self.surfaces.iter().map(SurfaceIn::ffi).collect(),
            offset_distance: self.offset_distance,
            approx_tol: self.approx_tol,
            actions: self
                .actions
                .iter()
                .map(|&key| ffi::CacheAction::Get(Some(key)))
                .collect(),
            samples: self.samples.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Probe {
    ReplaceEmpty,
    RemoveNoMerge,
    RemoveEmpty,
    SideWallNull,
    SideWallUnmapped,
    FillEmpty,
    VertexNull,
    VertexIsolated,
    FaceuseNull,
    FaceuseNoSurface,
    PiecewiseEmpty,
    MapPlane,
    EdgeusesUnmapped,
}

impl Probe {
    fn ffi(self) -> ffi::Probe {
        match self {
            Self::ReplaceEmpty => ffi::Probe::ReplaceEmpty,
            Self::RemoveNoMerge => ffi::Probe::RemoveNoMerge,
            Self::RemoveEmpty => ffi::Probe::RemoveEmpty,
            Self::SideWallNull => ffi::Probe::SideWallNull,
            Self::SideWallUnmapped => ffi::Probe::SideWallUnmapped,
            Self::FillEmpty => ffi::Probe::FillEmpty,
            Self::VertexNull => ffi::Probe::VertexNull,
            Self::VertexIsolated => ffi::Probe::VertexIsolated,
            Self::FaceuseNull => ffi::Probe::FaceuseNull,
            Self::FaceuseNoSurface => ffi::Probe::FaceuseNoSurface,
            Self::PiecewiseEmpty => ffi::Probe::PiecewiseEmpty,
            Self::MapPlane => ffi::Probe::MapPlane,
            Self::EdgeusesUnmapped => ffi::Probe::EdgeusesUnmapped,
        }
    }
}

fn bad(message: impl Into<String>) -> HarnessError {
    HarnessError::new(message)
}
fn setup(error: impl std::fmt::Display) -> HarnessError {
    bad(format!("Rust fixture construction: {error}"))
}
fn ivl([min, max]: [f64; 2]) -> Extent1d {
    Extent1d::from_raw(min, max)
}
fn cpoint([x, y, z, w]: [f64; 4]) -> CPoint {
    CPoint { x, y, z, w }
}
fn at<T>(items: &[T], index: usize) -> HResult<&T> {
    items
        .get(index)
        .ok_or_else(|| bad(format!("fixture index {index} out of range")))
}
fn id<T: PartialEq>(items: &[T], item: Option<T>) -> i64 {
    item.map_or(-1, |item| {
        items
            .iter()
            .position(|other| *other == item)
            .map_or(-2, |n| n as i64)
    })
}

// Snapshot decoder: validate the layout before partitioning semantic integers
// from metrics. In particular a failed operation is not a fixture rejection.
struct Snapshot<'a> {
    rest: &'a [f64],
}
impl<'a> Snapshot<'a> {
    fn take(&mut self, count: usize) -> HResult<&'a [f64]> {
        let result = self
            .rest
            .get(..count)
            .ok_or_else(|| bad("truncated native snapshot"))?;
        self.rest = self
            .rest
            .get(count..)
            .ok_or_else(|| bad("truncated native snapshot"))?;
        Ok(result)
    }
    fn integer(&mut self) -> HResult<i64> {
        let value = *at(self.take(1)?, 0)?;
        if !value.is_finite() || value.fract() != 0.0 || value.abs() > 9_007_199_254_740_991.0 {
            return Err(bad(format!("noninteger native snapshot field {value}")));
        }
        Ok(value as i64)
    }
    fn count(&mut self) -> HResult<usize> {
        let count = usize::try_from(self.integer()?).map_err(|_| bad("negative snapshot count"))?;
        if count > self.rest.len() {
            return Err(bad("snapshot count exceeds remaining buffer"));
        }
        Ok(count)
    }
    fn integers(&mut self, count: usize) -> HResult<Vec<i64>> {
        (0..count).map(|_| self.integer()).collect()
    }
    fn list(&mut self) -> HResult<Vec<i64>> {
        let n = self.count()?;
        self.integers(n)
    }
    fn done(self) -> HResult<()> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(bad("trailing native snapshot fields"))
        }
    }
}

fn state_output(mut output: Output, key: &str, state: &[f64]) -> HResult<Output> {
    let mut metrics = Vec::new();
    let mut integers = Vec::new();
    for (index, &value) in state.iter().enumerate() {
        if [0, 1, 7].contains(&index) {
            metrics.push(value);
        } else {
            integers.push(Snapshot { rest: &[value] }.integer()?);
        }
    }
    output.values.insert(format!("{key}.metrics"), metrics);
    output.ints.insert(format!("{key}.state"), integers);
    Ok(output)
}

fn decode(input: &Inputs, raw: &[f64]) -> HResult<Output> {
    let mut snapshot = Snapshot { rest: raw };
    let tag = snapshot.integer()?;
    if tag == -1 {
        let status = snapshot.integer()?;
        snapshot.done()?;
        if status == i64::from(ffi::NATIVE_SUCCESS) {
            return Err(bad("success tagged as rejected fixture"));
        }
        return Ok(Output::rejected(format!(
            "native fixture constructor status_{status}"
        )));
    }
    if tag != 0 {
        return Err(bad(format!("unknown native fixture tag {tag}")));
    }
    let mut output = Output::new();
    match input {
        Inputs::Config(_) => {
            output = state_output(output, "default", snapshot.take(10)?)?;
            output = state_output(output, "final", snapshot.take(10)?)?;
            let sets = snapshot.count()?;
            output = output.int("shell_set_count", sets as i64);
            for index in 0..sets {
                output = output.ints(&format!("shell_set.{index}"), snapshot.list()?);
            }
            let count = snapshot.count()?;
            output = output.int("original_edgeuse_count", count as i64);
            let mut lookups = Vec::new();
            for _ in 0..count {
                lookups.push(snapshot.integer()?);
                let _private_unordered_seconds = snapshot.list()?;
            }
            output = output
                .ints("lookups", lookups)
                .ints("relate_statuses", snapshot.list()?);
        }
        Inputs::Synchronize(_) => {
            output = output.ints("operation", snapshot.integers(3)?);
            output = output.ints("fixture_edgeuses", snapshot.list()?);
            let _private_first = snapshot.list()?;
            let _private_second = snapshot.list()?;
            let count = snapshot.count()?;
            output = output.int("edgeuse_count", count as i64);
            for index in 0..count {
                let mut ids_statuses = snapshot.integers(2)?;
                let mut metrics = snapshot.take(3)?.to_vec();
                ids_statuses.push(snapshot.integer()?);
                metrics.extend_from_slice(snapshot.take(3)?);
                ids_statuses.push(snapshot.integer()?);
                metrics.extend_from_slice(snapshot.take(3)?);
                output = output
                    .ints(&format!("edgeuse.{index}.ids_statuses"), ids_statuses)
                    .values(&format!("edgeuse.{index}.geometry"), metrics);
            }
            for key in [
                "matched_first",
                "matched_second",
                "gaps_first",
                "gaps_second",
            ] {
                output = output.ints(key, snapshot.list()?);
            }
        }
        Inputs::Extend(_) => {
            let count = snapshot.count()?;
            output = output.int("action_count", count as i64);
            for index in 0..count {
                let kind = snapshot.integer()?;
                let key = format!("action.{index}");
                match kind {
                    0 => {
                        let mut integers = vec![kind];
                        integers.extend(snapshot.integers(4)?);
                        let status = *at(&integers, 1)?;
                        output = output.ints(&format!("{key}.result"), integers);
                        if status == i64::from(ffi::NATIVE_SUCCESS) {
                            output = output
                                .int(&format!("{key}.kind"), snapshot.integer()?)
                                .values(&format!("{key}.domain"), snapshot.take(4)?.to_vec());
                            let n = snapshot.count()?;
                            output = output.int(&format!("{key}.sample_count"), n as i64);
                            let mut statuses = Vec::new();
                            let mut metrics = Vec::new();
                            for _ in 0..n {
                                statuses.push(snapshot.integer()?);
                                metrics.extend_from_slice(snapshot.take(3)?);
                                statuses.push(snapshot.integer()?);
                                metrics.extend_from_slice(snapshot.take(3)?);
                            }
                            output = output
                                .ints(&format!("{key}.sample_statuses"), statuses)
                                .values(&format!("{key}.samples"), metrics);
                        }
                    }
                    1 | 2 => {
                        let mut integers = vec![kind];
                        integers.extend(snapshot.integers(2)?);
                        output = output.ints(&format!("{key}.cache"), integers);
                    }
                    _ => return Err(bad("invalid cache action in native snapshot")),
                }
            }
        }
        Inputs::Probe { .. } => {
            output = output
                .ints("probe", snapshot.integers(6)?)
                .ints("lookups", snapshot.list()?);
        }
    }
    snapshot.done()?;
    normalize_sync_ties(output)
}

// GetSeconds is unordered. Normalize only its unresolved exact-minimum ties in
// comparison snapshots, never the kernel lists or the measured traversal records.
fn normalize_sync_ties(mut output: Output) -> HResult<Output> {
    if output.error.is_some() || output.deferred.is_some() || output.rejected.is_some() {
        return Ok(output);
    }
    let Some(count) = output.ints.get("edgeuse_count") else {
        return Ok(output);
    };
    let count =
        usize::try_from(*at(count, 0)?).map_err(|_| bad("invalid measured edgeuse count"))?;
    let mut minima = BTreeMap::new();
    for index in 0..count {
        let ids_key = format!("edgeuse.{index}.ids_statuses");
        let geometry_key = format!("edgeuse.{index}.geometry");
        let ids = output
            .ints
            .get(&ids_key)
            .ok_or_else(|| bad(format!("missing {ids_key}")))?;
        let geometry = output
            .values
            .get(&geometry_key)
            .ok_or_else(|| bad(format!("missing {geometry_key}")))?;
        let traversal_id = *at(ids, 0)?;
        let minimum = *at(geometry, 0)?;
        if traversal_id < 0 || !minimum.is_finite() {
            return Err(bad(format!("invalid measured edgeuse record {index}")));
        }
        if minima.insert(traversal_id, minimum).is_some() {
            return Err(bad(format!("duplicate measured edgeuse ID {traversal_id}")));
        }
    }
    for key in [
        "matched_first",
        "matched_second",
        "gaps_first",
        "gaps_second",
    ] {
        let ids = output
            .ints
            .get_mut(key)
            .ok_or_else(|| bad(format!("missing synchronization list {key}")))?;
        for &traversal_id in ids.iter() {
            if traversal_id != -1 && !minima.contains_key(&traversal_id) {
                return Err(bad(format!("unknown edgeuse ID {traversal_id} in {key}")));
            }
        }
        for group in ids.chunk_by_mut(|a, b| *a != -1 && *b != -1 && minima.get(a) == minima.get(b))
        {
            group.sort_unstable();
        }
    }
    Ok(output)
}

// An operation error is kept distinct from fixture/adapter errors. Only explicit
// upstream status errors have a numeric native equivalent; other errors fail
// visibly rather than being blanket-normalized to a successful or generic status.
enum RunError {
    Harness(HarnessError),
    Kernel(SolidError),
}
impl From<HarnessError> for RunError {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}
impl From<SolidError> for RunError {
    fn from(e: SolidError) -> Self {
        Self::Kernel(e)
    }
}
type RResult<T> = Result<T, RunError>;
fn operation<T>(result: solidcore::Result<T>) -> RResult<(i64, Option<T>)> {
    match result {
        Ok(value) => Ok((i64::from(ffi::NATIVE_SUCCESS), Some(value))),
        Err(SolidError::Upstream { code, .. }) => Ok((i64::from(code), None)),
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
        _ => Err(bad("operation must be 0..=4")),
    }
}

fn state(executive: &OffsetExecutive) -> Vec<f64> {
    vec![
        executive.offset_distance(),
        executive.approximation_tol_3d(),
        f64::from(executive.operation() as i32),
        flag(executive.inset()),
        flag(executive.extend_convex_edges()),
        flag(executive.merge_results()),
        flag(executive.create_offset_solid()),
        executive.tangency_tol_degrees(),
        flag(executive.offset_brep().is_some()),
        executive.shell_faces().len() as f64,
    ]
}

// Build the shim's primary/mate pair explicitly, rather than assuming arena EU
// ids are traversal ids. The closing edge is real scaffolding, not measured.
fn add_wire(brep: &mut Brep, curve: Curve, interval: Extent1d) -> HResult<EdgeId> {
    add_wire_with_vertices(brep, curve, interval, None)
}
fn add_wire_with_vertices(
    brep: &mut Brep,
    curve: Curve,
    interval: Extent1d,
    vertices: Option<[VertexId; 2]>,
) -> HResult<EdgeId> {
    let start = curve.evaluate_point(interval.min()).map_err(setup)?;
    let end = curve.evaluate_point(interval.max()).map_err(setup)?;
    let shell = brep
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: None,
            ..Shell::default()
        })
        .map_err(setup)?;
    let [a, b] = if let Some(vertices) = vertices {
        vertices
    } else {
        [
            brep.add_vertex_record(Vertex {
                point: start,
                ..Vertex::default()
            })
            .map_err(setup)?,
            brep.add_vertex_record(Vertex {
                point: end,
                ..Vertex::default()
            })
            .map_err(setup)?,
        ]
    };
    let edge = brep
        .add_edge_record(Edge {
            vertices: [a, b],
            edgeuses: Vec::new(),
            interval,
            tolerance: 0.0,
            curve: Some(curve),
        })
        .map_err(setup)?;
    for (vertex, orientation) in [(a, OrientType::Same), (b, OrientType::Opposite)] {
        let use_id = brep
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                owner: EdgeuseOwner::Shell(Some(shell)),
                orientation,
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
    }
    Ok(edge)
}
fn unit_wire(brep: &mut Brep, y: f64) -> HResult<EdgeId> {
    add_wire(
        brep,
        CurveIn::Line {
            start: [0.0, y, 0.0],
            end: [1.0, y, 0.0],
            domain: [0.0, 1.0],
        }
        .rust()?,
        ivl([0.0, 1.0]),
    )
}
fn primary(brep: &Brep, edge: EdgeId) -> HResult<EdgeuseId> {
    at(&brep.edge(edge).map_err(setup)?.edgeuses, 0).copied()
}
fn wire_uses(brep: &Brep) -> Vec<EdgeuseId> {
    brep.edges()
        .iter()
        .flat_map(|(_, edge)| edge.edgeuses.iter().copied())
        .collect()
}
fn plane(z: f64) -> Surface {
    Surface::from(Plane::new(
        Vec3::new(0.0, 0.0, z),
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
    ))
}
fn face_with_edges(
    brep: &mut Brep,
    surface: Surface,
    edges: &[EdgeId],
) -> HResult<(FaceId, FaceuseId)> {
    let shell = brep
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: None,
            ..Shell::default()
        })
        .map_err(setup)?;
    let domain = surface.natural_uv_domain();
    let face = brep
        .add_face_record(Face {
            surface: Some(surface),
            uv_domain: domain,
            ..Face::new(shell)
        })
        .map_err(setup)?;
    brep.shell_mut(shell).map_err(setup)?.faces.push(face);
    let upward = brep
        .add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(face),
            orientation: OrientType::Same,
            ..Faceuse::default()
        })
        .map_err(setup)?;
    let downward = brep
        .add_faceuse_record(Faceuse {
            shell: Some(shell),
            face: Some(face),
            mate: Some(upward),
            orientation: OrientType::Opposite,
            ..Faceuse::default()
        })
        .map_err(setup)?;
    brep.faceuse_mut(upward).map_err(setup)?.mate = Some(downward);
    brep.face_mut(face).map_err(setup)?.faceuse = Some(upward);
    let loop_id = brep
        .add_loop_record(Loop {
            face,
            edges: edges
                .iter()
                .map(|&edge| LoopEdge {
                    edge,
                    forward: true,
                })
                .collect(),
            loopuse: None,
        })
        .map_err(setup)?;
    brep.face_mut(face).map_err(setup)?.loops.push(loop_id);
    let up = brep
        .add_loopuse_record(Loopuse {
            faceuse: Some(upward),
            loop_: Some(loop_id),
            orientation: OrientType::Same,
            ..Loopuse::default()
        })
        .map_err(setup)?;
    let down = brep
        .add_loopuse_record(Loopuse {
            faceuse: Some(downward),
            loop_: Some(loop_id),
            mate: Some(up),
            orientation: OrientType::Same,
            ..Loopuse::default()
        })
        .map_err(setup)?;
    brep.loopuse_mut(up).map_err(setup)?.mate = Some(down);
    brep.loop_mut(loop_id).map_err(setup)?.loopuse = Some(up);
    for (faceuse, loopuse, mate_index) in [(upward, up, 0), (downward, down, 1)] {
        brep.faceuse_mut(faceuse)
            .map_err(setup)?
            .loopuses
            .push(loopuse);
        let mut uses = Vec::new();
        for &edge in edges {
            uses.push(*at(&brep.edge(edge).map_err(setup)?.edgeuses, mate_index)?);
        }
        if mate_index == 1 {
            uses.reverse();
        }
        if let Some(&first) = uses.first() {
            brep.loopuse_mut(loopuse).map_err(setup)?.start = LoopuseStart::Edgeuse(Some(first));
        }
        for (index, &use_id) in uses.iter().enumerate() {
            let ccw = *at(&uses, (index + 1) % uses.len())?;
            let cw = *at(&uses, (index + uses.len() - 1) % uses.len())?;
            let edgeuse = brep.edgeuse_mut(use_id).map_err(setup)?;
            edgeuse.owner = EdgeuseOwner::Loopuse(Some(loopuse));
            edgeuse.ccw = Some(ccw);
            edgeuse.cw = Some(cw);
        }
    }
    Ok((face, upward))
}
fn plane_face(brep: &mut Brep, z: f64) -> HResult<(FaceId, FaceuseId)> {
    let corners = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]];
    let vertices = corners
        .iter()
        .map(|&point| {
            brep.add_vertex_record(Vertex {
                point: v3(point),
                ..Vertex::default()
            })
            .map_err(setup)
        })
        .collect::<HResult<Vec<_>>>()?;
    let mut edges = Vec::new();
    for index in 0..4 {
        let curve = CurveIn::Line {
            start: *at(&corners, index)?,
            end: *at(&corners, (index + 1) % 4)?,
            domain: [0.0, 1.0],
        }
        .rust()?;
        edges.push(add_wire_with_vertices(
            brep,
            curve,
            ivl([0.0, 1.0]),
            Some([*at(&vertices, index)?, *at(&vertices, (index + 1) % 4)?]),
        )?);
    }
    face_with_edges(brep, plane(z), &edges)
}
fn loop_fixture(brep: &mut Brep, input: &EdgeIn) -> HResult<(EdgeId, EdgeId)> {
    let edge = add_wire(brep, input.curve.rust()?, ivl(input.interval))?;
    let [a, b] = brep.edge(edge).map_err(setup)?.vertices;
    let start = brep.vertex(b).map_err(setup)?.point;
    let end = brep.vertex(a).map_err(setup)?.point;
    let closing = if start.x == end.x && start.y == end.y && start.z == end.z {
        // Same nonzero closed cubic support as the shim; the measured curve and
        // interval are untouched, including zero width and constant control nets.
        BSplineCurve::from_data(
            3,
            3,
            7,
            3,
            &[
                cpoint([start.x, start.y, start.z, 1.0]),
                cpoint([1.0, 0.0, 0.0, 1.0]),
                cpoint([0.0, 1.0, 0.0, 1.0]),
                cpoint([end.x, end.y, end.z, 1.0]),
            ],
            &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        )
    } else {
        BSplineCurve::create_line_segment(3, start, end)
    }
    .map_err(setup)?;
    let interval = closing.natural_interval();
    let support = add_wire_with_vertices(brep, Curve::from(closing), interval, Some([b, a]))?;
    face_with_edges(brep, plane(0.0), &[edge, support])?;
    Ok((edge, support))
}

fn rust_config(input: &Config) -> HResult<Output> {
    let mut original = Brep::new();
    let mut offset = Brep::new();
    let mut face_owner = Brep::new();
    for i in 0..input.original_edges {
        unit_wire(&mut original, i as f64)?;
    }
    for i in 0..input.offset_edges {
        unit_wire(&mut offset, i as f64)?;
    }
    let mut faces = Vec::new();
    for i in 0..input.face_count {
        faces.push(plane_face(&mut face_owner, i as f64)?.0);
    }
    let originals = wire_uses(&original);
    let offsets = wire_uses(&offset);
    let mut executive = OffsetExecutive::with_geometry_parameters(
        operation_type(input.operation)?,
        input.offset_distance,
        input.approx_tol,
        input.tangency_tol_radians,
    );
    let mut output = state_output(Output::new(), "default", &state(&executive))?;
    executive.set_inset(input.inset);
    executive.set_extend_convex_edges(input.extend_convex);
    executive.set_merge_results(input.merge_results);
    executive.set_create_offset_solid(input.create_solid);
    if input.install_offset_brep {
        executive.set_offset_brep(offset);
    }
    output = output.int("shell_set_count", input.shell_face_sets.len() as i64);
    for (index, selectors) in input.shell_face_sets.iter().enumerate() {
        let mut selectors = selectors
            .iter()
            .map(|&index| at(&faces, index).copied())
            .collect::<HResult<Vec<_>>>()?;
        executive.set_shell_faces(&selectors);
        selectors.clear();
        output = output.ints(
            &format!("shell_set.{index}"),
            executive
                .shell_faces()
                .iter()
                .map(|&face| id(&faces, Some(face)))
                .collect::<Vec<_>>(),
        );
    }
    output = state_output(output, "final", &state(&executive))?;
    let mut statuses = Vec::new();
    for &[first, second] in &input.relations {
        executive.relate_edgeuses(*at(&originals, first)?, *at(&offsets, second)?);
        statuses.push(i64::from(ffi::NATIVE_SUCCESS));
    }
    Ok(output
        .int("original_edgeuse_count", originals.len() as i64)
        .ints(
            "lookups",
            originals
                .iter()
                .map(|&original| id(&offsets, executive.offset_edgeuse(original)))
                .collect::<Vec<_>>(),
        )
        .ints("relate_statuses", statuses))
}

// Notification identities for the offset B-rep. No runner record observes them;
// they only keep split notifications distinct per entity and curve.
fn notify_key(entity: solidcore::error::EntityRef) -> ObjectRef {
    match entity {
        solidcore::error::EntityRef::Edge(id) => ObjectRef {
            key: ObjectKey(100 + u64::from(id.index())),
            type_code: solidcore::topology::edge::TYPE,
        },
        solidcore::error::EntityRef::Vertex(id) => ObjectRef {
            key: ObjectKey(200 + u64::from(id.index())),
            type_code: solidcore::topology::vertex::TYPE,
        },
        _ => ObjectRef {
            key: ObjectKey(0),
            type_code: 0,
        },
    }
}
fn curve_key(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(300 + u64::from(edge.index())),
        type_code,
    }
}

// The shim's measured_traversal: GetEdges order without the closing support edges,
// each edge's uses in its list order (primary first).
fn measured_traversal(
    brep: &Brep,
    supports: &[EdgeId],
) -> RResult<(Vec<EdgeId>, Vec<EdgeuseId>)> {
    let edges: Vec<EdgeId> = solidcore::topology::brep::p1::edges(brep, None)?
        .into_iter()
        .filter(|edge| !supports.contains(edge))
        .collect();
    let mut uses = Vec::new();
    for &edge in &edges {
        uses.extend_from_slice(&brep.edge(edge)?.edgeuses);
    }
    Ok((edges, uses))
}

fn rust_sync(input: &Synchronize) -> RResult<Output> {
    let mut kernel = Kernel::default();
    let mut original = Brep::new();
    let edge_a = unit_wire(&mut original, 0.0)?;
    let edge_b = unit_wire(&mut original, 1.0)?;
    let a = primary(&original, edge_a)?;
    let b = primary(&original, edge_b)?;
    let mut offset = Brep::new();
    let mut supports = Vec::new();
    let mut fixtures = Vec::new();
    for edge in input.first.iter().chain(&input.second) {
        let (edge, support) = loop_fixture(&mut offset, edge)?;
        supports.push(support);
        fixtures.push(primary(&offset, edge)?);
    }
    let (_, before) = measured_traversal(&offset, &supports)?;
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 1.0, 1e-6);
    for (index, &use_id) in fixtures.iter().enumerate() {
        if index < input.first.len() {
            executive.relate_edgeuses(a, use_id);
        } else if !input.null_other {
            executive.relate_edgeuses(b, use_id);
        }
    }
    executive.set_offset_brep(offset);
    let first = (!input.null_original).then_some(a);
    let second = (!input.null_other).then_some(b);
    let lookup = |value: Option<EdgeuseId>| {
        value.map_or(-1, |value| id(&before, executive.offset_edgeuse(value)))
    };
    let first_lookup = lookup(first);
    let second_lookup = lookup(second);
    let mut records = BTreeMap::new();
    let mut attributes = AttributeStore::default();
    let mut notifications = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &notify_key,
    };
    let (status, synchronized) = operation(executive.synchronize_offset_edgeuses(
        &mut kernel,
        first,
        second,
        &mut notifications,
        &curve_key,
    ))?;
    let offset = executive
        .offset_brep()
        .ok_or_else(|| bad("synchronize lost its offset B-rep"))?;
    // As the shim: the records use a second traversal, which sees split edges.
    let (measured, after) = measured_traversal(offset, &supports)?;
    let mut output = Output::new()
        .ints("operation", vec![status, first_lookup, second_lookup])
        .ints(
            "fixture_edgeuses",
            fixtures
                .iter()
                .map(|&eu| id(&after, Some(eu)))
                .collect::<Vec<_>>(),
        )
        .int("edgeuse_count", after.len() as i64);
    for (index, &use_id) in after.iter().enumerate() {
        let edge_id = offset
            .edgeuse(use_id)
            .map_err(setup)?
            .edge
            .ok_or_else(|| bad("missing measured edge"))?;
        let edge = offset.edge(edge_id).map_err(setup)?;
        let curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| bad("missing measured curve"))?;
        let length = curve.approximate_length(&edge.interval, 10, false)?.0;
        let (start_status, start) = operation(curve.evaluate_point(edge.interval.min()))?;
        let (end_status, end) = operation(curve.evaluate_point(edge.interval.max()))?;
        let start = start.unwrap_or(Vec3::ZERO);
        let end = end.unwrap_or(Vec3::ZERO);
        output = output
            .ints(
                &format!("edgeuse.{index}.ids_statuses"),
                vec![
                    index as i64,
                    id(&measured, Some(edge_id)),
                    start_status,
                    end_status,
                ],
            )
            .values(
                &format!("edgeuse.{index}.geometry"),
                vec![
                    edge.interval.min(),
                    edge.interval.max(),
                    length,
                    start.x,
                    start.y,
                    start.z,
                    end.x,
                    end.y,
                    end.z,
                ],
            );
    }
    let synchronized = synchronized.unwrap_or_default();
    for (key, values) in [
        ("matched_first", synchronized.edgeuses),
        ("matched_second", synchronized.other_edgeuses),
    ] {
        output = output.ints(
            key,
            values
                .into_iter()
                .map(|eu| id(&after, eu))
                .collect::<Vec<_>>(),
        );
    }
    for (key, values) in [
        ("gaps_first", synchronized.gaps),
        ("gaps_second", synchronized.other_gaps),
    ] {
        output = output.ints(
            key,
            values
                .into_iter()
                .map(|eu| id(&after, Some(eu)))
                .collect::<Vec<_>>(),
        );
    }
    Ok(output)
}

fn rust_extend(input: &Extension) -> RResult<Output> {
    let kernel = Kernel::default();
    let bases = input
        .surfaces
        .iter()
        .map(SurfaceIn::rust)
        .collect::<HResult<Vec<_>>>()?;
    let mut executive = OffsetExecutive::new(
        OffsetOperation::SolidOffset,
        input.offset_distance,
        input.approx_tol,
    );
    let mut identities = BTreeMap::new();
    let mut next_id = 0;
    let mut output = Output::new().int("action_count", input.actions.len() as i64);
    for (index, &base_key) in input.actions.iter().enumerate() {
        let base = at(&bases, base_key)?;
        let hit = identities.contains_key(&base_key);
        let (status, surface) =
            operation(executive.get_create_extended_surface(&kernel, base_key as u64, base))?;
        let result_id = if surface.is_some() {
            *identities.entry(base_key).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            })
        } else {
            -1
        };
        let key = format!("action.{index}");
        output = output.ints(
            &format!("{key}.result"),
            vec![
                0,
                status,
                i64::from(hit),
                result_id,
                i64::from(surface.is_some()),
            ],
        );
        let Some(surface) = surface else { continue };
        let domain = surface.natural_uv_domain();
        let kind = match surface.kind() {
            SurfaceKind::Plane(_) => 1,
            SurfaceKind::BSpline(_) => 2,
            _ => 3,
        };
        output = output
            .int(&format!("{key}.kind"), kind)
            .values(
                &format!("{key}.domain"),
                vec![
                    domain.min().x,
                    domain.min().y,
                    domain.max().x,
                    domain.max().y,
                ],
            )
            .int(&format!("{key}.sample_count"), input.samples.len() as i64);
        let mut statuses = Vec::new();
        let mut samples = Vec::new();
        for &[u, v] in &input.samples {
            // Preserve the native sampling expression, not an equivalent lerp.
            let uv = Vec2::new(
                (1.0 - u) * domain.min().x + u * domain.max().x,
                (1.0 - v) * domain.min().y + v * domain.max().y,
            );
            let (status, point) = operation(surface.evaluate_point(uv))?;
            let point = point.unwrap_or(Vec3::ZERO);
            statuses.push(status);
            samples.extend([point.x, point.y, point.z]);
            let (status, normal) =
                operation(surface.evaluate_normal(uv, Side::Right, Side::Right))?;
            let normal = normal.unwrap_or(Vec3::ZERO);
            statuses.push(status);
            samples.extend([normal.x, normal.y, normal.z]);
        }
        output = output
            .ints(&format!("{key}.sample_statuses"), statuses)
            .values(&format!("{key}.samples"), samples);
    }
    Ok(output)
}

fn rust_probe(branch: Probe, tolerance: f64) -> RResult<Output> {
    let mut kernel = Kernel::default();
    let mut original = Brep::new();
    let mut executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.25, tolerance);
    executive.set_offset_brep(Brep::new());
    let mut counts = [0_i64; 4]; // curves, orientations, surface, result B-rep
    let mut originals = Vec::new();
    let mut offsets = Vec::new();
    let result = match branch {
        Probe::ReplaceEmpty => {
            OffsetExecutive::replace_implicit_offsets(&kernel, &mut original, tolerance)
        }
        Probe::RemoveNoMerge | Probe::RemoveEmpty => {
            executive.set_merge_results(matches!(branch, Probe::RemoveEmpty));
            executive.remove_excess_faces(&kernel, &original, &mut Brep::new())
        }
        Probe::SideWallNull => executive.build_side_wall_of_edgeuse(&kernel, &mut original, None),
        Probe::SideWallUnmapped => {
            let edge = unit_wire(&mut original, 0.0)?;
            let eu = primary(&original, edge)?;
            executive.build_side_wall_of_edgeuse(&kernel, &mut original, Some(eu))
        }
        Probe::FillEmpty => {
            executive.fill_gap_edgeuses(&kernel, EdgeId::from_index(u32::MAX), &[], &[])
        }
        Probe::VertexNull | Probe::VertexIsolated => {
            let vertex = if matches!(branch, Probe::VertexIsolated) {
                let shell = original
                    .add_shell_record(Shell {
                        faces: Vec::new(),
                        region: None,
                        ..Shell::default()
                    })
                    .map_err(setup)?;
                let vertex = original
                    .add_vertex_record(Vertex::default())
                    .map_err(setup)?;
                let vu = original
                    .add_vertexuse_record(Vertexuse {
                        vertex: Some(vertex),
                        owner: VertexuseOwner::Shell(Some(shell)),
                        ..Vertexuse::default()
                    })
                    .map_err(setup)?;
                original
                    .vertex_mut(vertex)
                    .map_err(setup)?
                    .vertexuses
                    .push(vu);
                Some(vertex)
            } else {
                None
            };
            executive
                .build_offset_of_vertex(&kernel, &original, vertex)
                .map(|result| {
                    counts = [
                        result.trim_curves.len() as i64,
                        result.curve_orientations.len() as i64,
                        i64::from(result.surface.is_some()),
                        0,
                    ];
                })
        }
        Probe::FaceuseNull | Probe::FaceuseNoSurface => {
            let faceuse = if matches!(branch, Probe::FaceuseNoSurface) {
                Some(plane_face(&mut original, 0.0)?.1)
            } else {
                None
            };
            executive.build_offset_of_faceuse(&kernel, &mut original, faceuse, &mut None)
        }
        Probe::PiecewiseEmpty => executive
            .piecewise_merge(&mut kernel, &mut original)
            .map(|_| {
                counts = [0, 0, 0, 1];
            }),
        Probe::MapPlane => {
            let (face, faceuse) = plane_face(&mut original, 0.0)?;
            originals = wire_uses(&original);
            offsets.clone_from(&originals);
            // Native passes the original face itself as the offset face. The safe
            // API resolves result IDs through its own B-rep, so install its clone.
            executive.set_offset_brep(original.clone());
            executive.map_edgeuses(&kernel, &original, faceuse, face)
        }
        Probe::EdgeusesUnmapped => {
            plane_face(&mut original, 0.0)?;
            let edge = original
                .edges()
                .ids()
                .next()
                .ok_or_else(|| bad("plane has no edge"))?;
            let uses = &original.edge(edge).map_err(setup)?.edgeuses;
            let first = *at(uses, 0)?;
            let other = *at(uses, 1)?;
            let mut records = BTreeMap::new();
            let mut attributes = AttributeStore::default();
            let key_of = |entity| match entity {
                solidcore::error::EntityRef::Edge(id) => ObjectRef {
                    key: ObjectKey(100 + u64::from(id.index())),
                    type_code: solidcore::topology::edge::TYPE,
                },
                solidcore::error::EntityRef::Vertex(id) => ObjectRef {
                    key: ObjectKey(200 + u64::from(id.index())),
                    type_code: solidcore::topology::vertex::TYPE,
                },
                _ => ObjectRef {
                    key: ObjectKey(0),
                    type_code: 0,
                },
            };
            let mut notifications = NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            executive.build_offset_of_edgeuses(
                &mut kernel,
                &original,
                first,
                other,
                MarkType::Mark,
                &mut notifications,
                &key_of,
                &curve_key,
            )
        }
    };
    let (status, _) = operation(result)?;
    let mut fields = vec![status];
    fields.extend(counts);
    fields.push(i64::from(executive.offset_brep().is_some()));
    Ok(Output::new().ints("probe", fields).ints(
        "lookups",
        originals
            .iter()
            .map(|&original| id(&offsets, executive.offset_edgeuse(original)))
            .collect::<Vec<_>>(),
    ))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "offset.offset_executive_p1"
    }
    fn run_cpp(&self, case: &Case) -> HResult<Output> {
        let input: Inputs = case.parse()?;
        let raw = match &input {
            Inputs::Config(input) => ffi::config(&input.ffi()),
            Inputs::Synchronize(input) => ffi::synchronize(&input.ffi()),
            Inputs::Extend(input) => ffi::extend(&input.ffi()),
            Inputs::Probe { branch, approx_tol } => ffi::probe(branch.ffi(), *approx_tol),
        }
        .map_err(|error| bad(format!("native adapter: {error}")))?;
        decode(&input, &raw)
    }
    fn run_rust(&self, case: &Case) -> HResult<Output> {
        let result = match case.parse::<Inputs>()? {
            Inputs::Config(input) => rust_config(&input).map_err(RunError::Harness),
            Inputs::Synchronize(input) => rust_sync(&input),
            Inputs::Extend(input) => rust_extend(&input),
            Inputs::Probe { branch, approx_tol } => rust_probe(branch, approx_tol),
        };
        match result {
            Ok(output) => normalize_sync_ties(output),
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
        let input = match rng.below(20) {
            0 | 1 => Inputs::Config(generated_config(rng)),
            2..=10 => Inputs::Synchronize(generated_sync(rng)),
            _ => Inputs::Extend(generated_extension(rng)),
        };
        serde_json::to_value(input).ok()
    }
}

fn generated_config(rng: &mut Rng) -> Config {
    let original_edges = 1 + rng.below(4) as usize;
    let offset_edges = 1 + rng.below(4) as usize;
    let face_count = 1 + rng.below(5) as usize;
    let mut relations = Vec::new();
    for _ in 0..(2 + rng.below(14)) {
        let pair = [
            rng.below((2 * original_edges) as u64) as usize,
            rng.below((2 * offset_edges) as u64) as usize,
        ];
        relations.push(pair);
        if rng.chance(0.4) {
            relations.push(pair);
        }
    }
    let mut sets = Vec::new();
    for _ in 0..rng.below(5) {
        sets.push(
            (0..rng.below(9))
                .map(|_| rng.below(face_count as u64) as usize)
                .collect(),
        );
    }
    let tangency_tol_radians = if rng.chance(0.5) {
        rng.choose(&[
            0.0,
            -0.0,
            1e-12,
            -1e-12,
            0.1234567890123456,
            -0.1234567890123456,
            PI,
            -PI,
            default_tangency(),
        ])
        .copied()
        .unwrap_or_else(default_tangency)
    } else {
        rng.uniform(-PI, PI)
    };
    Config {
        offset_distance: rng.uniform(-10.0, 10.0),
        approx_tol: rng.uniform(1e-12, 1e-3),
        tangency_tol_radians,
        operation: rng.below(5) as u32,
        inset: rng.chance(0.5),
        extend_convex: rng.chance(0.5),
        merge_results: rng.chance(0.5),
        create_solid: rng.chance(0.5),
        install_offset_brep: rng.chance(0.5),
        original_edges,
        offset_edges,
        face_count,
        shell_face_sets: sets,
        relations,
    }
}
fn clamped(degree: usize, [min, max]: [f64; 2]) -> Vec<f64> {
    std::iter::repeat_n(min, degree + 1)
        .chain(std::iter::repeat_n(max, degree + 1))
        .collect()
}
fn generated_edge(rng: &mut Rng, interval: [f64; 2]) -> EdgeIn {
    let scale = *rng.choose(&[1e-8, 1e-3, 1.0, 1e3, 1e9]).unwrap_or(&1.0);
    let start = rng.point3(-scale, scale);
    let [min, max] = interval;
    let [x, y, z] = start;
    let domain = [min - 0.5, max + 0.5];
    let curve = if rng.chance(0.45) {
        CurveIn::Line {
            start,
            end: [x + scale, y + 0.25 * scale, z - 0.5 * scale],
            domain,
        }
    } else {
        let degree = 1 + rng.below(3) as usize;
        let rational = rng.chance(0.5);
        let points = (0..=degree)
            .map(|i| {
                let t = i as f64 / degree as f64;
                let w = if rational {
                    0.5 + rng.uniform(0.0, 1.0)
                } else {
                    1.0
                };
                [
                    (x + scale * t) * w,
                    (y + scale * 0.25 * t * t) * w,
                    (z + scale * 0.125 * t * (1.0 - t)) * w,
                    w,
                ]
            })
            .collect();
        CurveIn::Bspline {
            degree,
            control_points: points,
            knots: clamped(degree, domain),
        }
    };
    EdgeIn { curve, interval }
}
fn generated_sync(rng: &mut Rng) -> Synchronize {
    let count = 1 + rng.below(6) as usize;
    let shift = rng.range_i64(-8, 8) as f64;
    let mut first = Vec::new();
    let mut second = Vec::new();
    for i in 0..count {
        let min = shift + 2.0 * i as f64;
        first.push(generated_edge(rng, [min, min + 1.0]));
        second.push(generated_edge(rng, [min, min + 1.0]));
    }
    match rng.below(12) {
        0 => first.clear(),
        1 => second.clear(),
        2 => {
            for edge in &mut second {
                let [min, max] = edge.interval;
                edge.interval = [min + 0.25, max];
            }
        } // Real split dependency.
        3 => {
            second.clear();
            second.push(generated_edge(rng, [shift - 3.0, shift - 2.0]));
        }
        4 => {
            first.pop();
        } // One surviving second gap, not an artificial status case.
        5 => {
            second.pop();
        }
        6 => {
            first.clear();
            first.push(generated_edge(rng, [shift, shift]));
        }
        7 => {
            second.clear();
            second.push(generated_edge(rng, [shift, shift]));
        }
        _ => {}
    }
    if rng.chance(0.5) {
        first.reverse();
    }
    if rng.chance(0.5) {
        second.reverse();
    }
    Synchronize {
        first,
        second,
        null_original: false,
        null_other: rng.chance(0.05),
    }
}
fn generated_surface(rng: &mut Rng) -> SurfaceIn {
    let scale = *rng.choose(&[1e-6, 1e-2, 1.0, 1e3, 1e6]).unwrap_or(&1.0);
    let origin = rng.point3(-scale, scale);
    let u0 = rng.range_i64(-4, 2) as f64;
    let v0 = rng.range_i64(-3, 3) as f64;
    let u1 = u0 + 2.0;
    let v1 = v0 + 1.5;
    let domain = [u0, v0, u1, v1];
    let [x, y, z] = origin;
    if rng.chance(0.3) {
        SurfaceIn::Plane {
            origin,
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            scale: [scale, scale * 0.75],
            domain,
        }
    } else {
        let u_degree = 1 + rng.below(2) as usize;
        let v_degree = 1 + rng.below(2) as usize;
        let rational = rng.chance(0.5);
        let mut points = Vec::new();
        for i in 0..=u_degree {
            for j in 0..=v_degree {
                let u = i as f64 / u_degree as f64;
                let v = j as f64 / v_degree as f64;
                let w = if rational {
                    rng.uniform(0.75, 1.25)
                } else {
                    1.0
                };
                points.push([
                    (x + scale * u) * w,
                    (y + scale * v) * w,
                    (z + scale * (0.125 * u * v + 0.0625 * u * u)) * w,
                    w,
                ]);
            }
        }
        SurfaceIn::Bspline {
            u_degree,
            v_degree,
            u_count: u_degree + 1,
            v_count: v_degree + 1,
            control_points: points,
            u_knots: clamped(u_degree, [u0, u1]),
            v_knots: clamped(v_degree, [v0, v1]),
        }
    }
}
fn generated_extension(rng: &mut Rng) -> Extension {
    let surfaces = vec![generated_surface(rng), generated_surface(rng)];
    let distance = match rng.below(10) {
        0 => 0.0,
        1 => -0.0,
        2 => 1e-12,
        3 => -1e-12,
        4 => 1e7,
        5 => -1e7,
        _ => rng.uniform(-1.0, 1.0),
    };
    Extension {
        surfaces,
        // Preserve the RNG draws and geometry mix; only negative magnitudes are
        // outside the internal extension contract. Keep -0.0 as a boundary case.
        offset_distance: if distance < 0.0 {
            distance.abs()
        } else {
            distance
        },
        approx_tol: rng.uniform(1e-10, 1e-3),
        actions: vec![0, 0, 1, 1, 0],
        samples: vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 1.0],
            [0.5, 0.5],
            [0.07, 0.19],
            [0.45, 0.73],
            [rng.unit(), rng.unit()],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compare, CaseSource};

    fn probe_input() -> Inputs {
        Inputs::Probe {
            branch: Probe::SideWallNull,
            approx_tol: 1e-6,
        }
    }
    fn case(input: &Inputs) -> Case {
        Case {
            unit: Runner.id().into(),
            name: "test".into(),
            source: CaseSource::Inline,
            inputs: serde_json::to_value(input).unwrap(),
        }
    }

    #[test]
    fn fixtures_parse_and_minimum_seed_budget_is_enforced() {
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let cases = Runner.cases(&context).unwrap();
        assert_eq!(
            cases
                .iter()
                .filter(|case| matches!(case.source, CaseSource::Generated { .. }))
                .count(),
            MIN_GENERATED
        );
        let fixtures: Vec<_> = cases
            .iter()
            .filter(|case| matches!(case.source, CaseSource::Fixture { .. }))
            .collect();
        assert_eq!(fixtures.len(), 93);
        for case in fixtures {
            case.parse::<Inputs>().unwrap();
        }
    }

    #[test]
    fn omitted_tangency_preserves_existing_config_dtos_and_default_constructor() {
        let config = generated_config(&mut Rng::new(175));
        let mut json = serde_json::to_value(Inputs::Config(config)).unwrap();
        json.as_object_mut().unwrap().remove("tangency_tol_radians");
        let Inputs::Config(config) = serde_json::from_value::<Inputs>(json).unwrap() else {
            unreachable!()
        };
        let radians = default_tangency();
        assert_eq!(config.tangency_tol_radians.to_bits(), radians.to_bits());
        assert_eq!(
            config.ffi().tangency_tol_radians.to_bits(),
            radians.to_bits()
        );
        let executive = OffsetExecutive::new(OffsetOperation::SolidOffset, 0.125, 1e-6);
        assert_eq!(
            executive.tangency_tol_degrees().to_bits(),
            (radians * 180.0 / PI).to_bits()
        );
    }

    #[test]
    fn custom_tangency_reaches_ffi_and_both_scalar_snapshot_phases_in_native_order() {
        let mut config = generated_config(&mut Rng::new(175));
        config.original_edges = 0;
        config.offset_edges = 0;
        config.face_count = 0;
        config.shell_face_sets = vec![Vec::new()];
        config.relations.clear();
        config.inset = true;
        config.extend_convex = true;
        config.merge_results = true;
        config.create_solid = true;
        for radians in [
            0.0,
            -0.0,
            1e-12,
            -1e-12,
            0.1234567890123456,
            -0.1234567890123456,
            PI,
            -PI,
            default_tangency(),
        ] {
            config.tangency_tol_radians = radians;
            let input = Inputs::Config(config.clone());
            let roundtrip: Inputs =
                serde_json::from_value(serde_json::to_value(&input).unwrap()).unwrap();
            let Inputs::Config(roundtrip) = roundtrip else {
                unreachable!()
            };
            assert_eq!(roundtrip.tangency_tol_radians.to_bits(), radians.to_bits());
            assert_eq!(
                roundtrip.ffi().tangency_tol_radians.to_bits(),
                radians.to_bits()
            );
            let output = Runner.run_rust(&case(&input)).unwrap();
            let expected = radians * 180.0 / PI;
            for phase in ["default", "final"] {
                let metrics = &output.values[&format!("{phase}.metrics")];
                assert_eq!(
                    metrics,
                    &[config.offset_distance, config.approx_tol, expected]
                );
                assert_eq!(metrics[2].to_bits(), expected.to_bits());
            }
        }
        // This non-round angle exposes reassociation even below the parity tolerance.
        let radians = 0.1234567890123456;
        assert_ne!(
            (radians * 180.0 / PI).to_bits(),
            (radians * (180.0 / PI)).to_bits()
        );
    }

    #[test]
    fn real_trimmed_line_sync_agrees_with_the_documented_native_record_layout() {
        let input = Inputs::Synchronize(Synchronize {
            first: vec![EdgeIn {
                curve: CurveIn::Line {
                    start: [1.0, 2.0, 3.0],
                    end: [3.0, 2.0, 3.0],
                    domain: [-2.0, 2.0],
                },
                interval: [-1.0, 1.0],
            }],
            second: Vec::new(),
            null_original: false,
            null_other: false,
        });
        let raw = [
            0.0, 1000.0, 0.0, -1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, -1.0, 1.0, 1.0, 1000.0,
            1.5, 2.0, 3.0, 1000.0, 2.5, 2.0, 3.0, 1.0, 0.0, -1.0, 1.0, 1.0, 1000.0, 1.5, 2.0, 3.0,
            1000.0, 2.5, 2.0, 3.0, 1.0, 0.0, 1.0, -1.0, 0.0, 0.0,
        ];
        let expected = decode(&input, &raw).unwrap();
        let actual = Runner.run_rust(&case(&input)).unwrap();
        assert!(compare(&expected, &actual, &[]).pass());
        let Inputs::Synchronize(input) = input else {
            unreachable!()
        };
        let mut brep = Brep::new();
        let (edge, support) = loop_fixture(&mut brep, &input.first[0]).unwrap();
        assert_eq!(brep.vertices().len(), 2);
        assert_eq!(brep.edge(edge).unwrap().edgeuses.len(), 2);
        assert_eq!(
            brep.edge(support).unwrap().vertices,
            [
                brep.edge(edge).unwrap().vertices[1],
                brep.edge(edge).unwrap().vertices[0]
            ]
        );
    }

    #[test]
    fn zero_length_and_near_degenerate_fixtures_keep_their_actual_filter_branches() {
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        for (name, matched_first, matched_second, zero_use) in [
            (
                "zero_trim_first_leaves_second_unfiltered",
                vec![-1, -1],
                vec![4, 2],
                Some(0),
            ),
            (
                "zero_trim_second_filtered_to_null_padding",
                vec![0],
                vec![-1],
                Some(2),
            ),
            (
                "collapsed_quadratic_spline_length_filter",
                vec![-1],
                vec![2],
                Some(0),
            ),
            (
                "near_degenerate_analytic_line_positive_length_survives",
                vec![0],
                vec![-1],
                None,
            ),
            (
                "near_degenerate_quadratic_spline_positive_length_survives",
                vec![0],
                vec![-1],
                None,
            ),
        ] {
            let fixture = fixtures.iter().find(|case| case.name == name).unwrap();
            let actual = Runner.run_rust(fixture).unwrap();
            assert!(actual.error.is_none() && actual.deferred.is_none());
            assert!(actual.rejected.is_none());
            assert_eq!(actual.ints["operation"][0], i64::from(ffi::NATIVE_SUCCESS));
            assert_eq!(actual.ints["matched_first"], matched_first, "{name}");
            assert_eq!(actual.ints["matched_second"], matched_second, "{name}");
            assert!(actual.ints["gaps_first"].is_empty());
            assert!(actual.ints["gaps_second"].is_empty());
            if let Some(index) = zero_use {
                assert_eq!(actual.values[&format!("edgeuse.{index}.geometry")][2], 0.0);
            } else {
                // A positive singleton is kept even though it is very short:
                // length > total / 10, unlike a genuinely zero-length singleton.
                let length = actual.values["edgeuse.0.geometry"][2];
                assert!(length > 0.0 && length < 1e-9, "{name}: {length}");
            }
            let Inputs::Synchronize(input) = fixture.parse().unwrap() else {
                unreachable!()
            };
            let mut brep = Brep::new();
            for edge in input.first.iter().chain(&input.second) {
                let (measured, support) = loop_fixture(&mut brep, edge).unwrap();
                let measured = brep.edge(measured).unwrap();
                let support = brep.edge(support).unwrap();
                let closing = support.curve.as_ref().unwrap();
                assert_eq!(
                    support.vertices,
                    [measured.vertices[1], measured.vertices[0]]
                );
                assert!(
                    closing
                        .approximate_length(&support.interval, 10, false)
                        .unwrap()
                        .0
                        > 0.0
                );
                assert_eq!(
                    closing.evaluate_point(support.interval.min()).unwrap(),
                    brep.vertex(measured.vertices[1]).unwrap().point,
                );
                assert_eq!(
                    closing.evaluate_point(support.interval.max()).unwrap(),
                    brep.vertex(measured.vertices[0]).unwrap().point,
                );
            }
            #[cfg(feature = "cpp")]
            {
                let native = Runner.run_cpp(fixture).unwrap();
                assert!(native.rejected.is_none(), "{name}: {:?}", native.rejected);
                assert!(
                    native.error.is_none() && native.deferred.is_none(),
                    "{name}"
                );
                assert!(compare(&native, &actual, &[]).pass(), "{name}");
            }
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn coincident_analytic_line_remains_a_native_rejection_not_parity() {
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        let fixture = fixtures
            .iter()
            .find(|case| case.name == "coincident_analytic_line_constructor_rejection")
            .unwrap();
        let native = Runner.run_cpp(fixture).unwrap();
        assert_eq!(
            native.rejected.as_deref(),
            Some("native fixture constructor status_1007"),
        );
        assert!(native.ints.is_empty() && native.values.is_empty());
        assert!(native.error.is_none() && native.deferred.is_none());
    }

    #[test]
    fn measured_traversal_excludes_supports_and_uses_even_primary_indices() {
        let edge = EdgeIn {
            curve: CurveIn::Line {
                start: [0.0; 3],
                end: [1.0, 0.0, 0.0],
                domain: [0.0, 1.0],
            },
            interval: [0.0, 1.0],
        };
        let mut second = edge.clone();
        second.interval = [2.0, 3.0];
        second.curve = CurveIn::Line {
            start: [2.0, 0.0, 0.0],
            end: [3.0, 0.0, 0.0],
            domain: [2.0, 3.0],
        };
        let input = Inputs::Synchronize(Synchronize {
            first: vec![second, edge],
            second: Vec::new(),
            null_original: false,
            null_other: false,
        });
        let output = Runner.run_rust(&case(&input)).unwrap();
        assert_eq!(output.ints["fixture_edgeuses"], [0, 2]);
        assert_eq!(output.ints["edgeuse_count"], [4]);
        assert_eq!(output.ints["matched_first"], [2, 0]);
        assert_eq!(output.ints["matched_second"], [-1, -1]);
        assert_eq!(output.ints["edgeuse.2.ids_statuses"], [2, 1, 1000, 1000]);
    }

    #[test]
    fn real_plane_extensions_sample_geometry_and_repeat_genuine_cache_calls() {
        let surface = SurfaceIn::Plane {
            origin: [3.0, 4.0, 5.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
            scale: [2.0, 0.5],
            domain: [-1.0, -2.0, 3.0, 4.0],
        };
        let input = Inputs::Extend(Extension {
            surfaces: vec![surface.clone(), surface],
            offset_distance: 0.125,
            approx_tol: 1e-6,
            actions: vec![0, 0, 1, 1, 0],
            samples: vec![[0.0, 0.0], [0.25, 0.75], [1.0, 1.0]],
        });
        let output = Runner.run_rust(&case(&input)).unwrap();
        assert!(output.error.is_none() && output.deferred.is_none());
        assert_eq!(output.ints["action.0.result"], [0, 1000, 0, 0, 1]);
        assert_eq!(output.ints["action.1.result"], [0, 1000, 1, 0, 1]);
        assert_eq!(output.ints["action.2.result"], [0, 1000, 0, 1, 1]);
        assert_eq!(output.ints["action.3.result"], [0, 1000, 1, 1, 1]);
        assert_eq!(output.values["action.0.samples"].len(), 18);
        assert_eq!(
            output.values["action.0.samples"],
            output.values["action.4.samples"]
        );
        assert_eq!(
            output.values["action.0.domain"],
            output.values["action.2.domain"]
        );
        assert!(Runner.abs_floors().is_empty());
    }

    #[test]
    fn each_highlevel_probe_calls_its_real_method() {
        for (branch, dependency, status) in [
            (Probe::ReplaceEmpty, Some("topology.brep_p4"), 1000),
            (Probe::RemoveNoMerge, None, 1000),
            (Probe::RemoveEmpty, Some("topology.brep_p5"), 1000),
            (Probe::SideWallNull, None, 1000),
            (Probe::SideWallUnmapped, None, 1000),
            (Probe::FillEmpty, None, 1000),
            (Probe::VertexNull, None, 1006),
            (
                Probe::VertexIsolated,
                Some("offset.offset_geometry_creation"),
                1000,
            ),
            (Probe::FaceuseNull, None, 1006),
            (Probe::FaceuseNoSurface, None, 1000),
            (Probe::PiecewiseEmpty, Some("query.brep"), 1000),
            (Probe::MapPlane, Some("topology.edgeuse_p3"), 1000),
            (Probe::EdgeusesUnmapped, None, 1000),
        ] {
            let input = Inputs::Probe {
                branch,
                approx_tol: 1e-6,
            };
            let output = Runner.run_rust(&case(&input)).unwrap();
            if let Some(unit) = dependency {
                assert_eq!(output.deferred.unwrap().unit, unit);
            } else {
                assert_eq!(output.ints["probe"], [status, 0, 0, 0, 0, 1]);
            }
        }
    }

    #[test]
    fn decoder_distinguishes_statuses_rejections_and_bad_layouts() {
        let input = probe_input();
        let output = decode(&input, &[0.0, 1006.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]).unwrap();
        assert_eq!(output.ints["probe"], [1006, 0, 0, 0, 0, 1]);
        assert!(output.values.is_empty() && output.error.is_none() && output.rejected.is_none());
        assert!(decode(&input, &[-1.0, 1007.0]).unwrap().rejected.is_some());
        for raw in [
            vec![],
            vec![0.0],
            vec![-1.0, 1000.0],
            vec![-1.0, 1007.0, 0.0],
            vec![0.0, 1000.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            vec![0.0, 1000.0, 0.0, 0.0, 0.0, 0.0, 1.0, 999.0],
        ] {
            assert!(decode(&input, &raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn extension_decoder_partitions_counts_ids_statuses_and_geometry() {
        let input = Inputs::Extend(Extension {
            surfaces: Vec::new(),
            actions: Vec::new(),
            samples: Vec::new(),
            offset_distance: 0.0,
            approx_tol: 1e-6,
        });
        let raw = [
            0.0, 1.0, 0.0, 1000.0, 0.0, 0.0, 1.0, 2.0, -2.0, -1.0, 4.0, 5.0, 1.0, 1000.0, 1.25,
            2.5, -3.0, 1000.0, 0.0, 0.0, 1.0,
        ];
        let output = decode(&input, &raw).unwrap();
        assert_eq!(output.ints["action.0.result"], [0, 1000, 0, 0, 1]);
        assert_eq!(output.ints["action.0.sample_statuses"], [1000, 1000]);
        assert_eq!(output.values["action.0.domain"], [-2.0, -1.0, 4.0, 5.0]);
        assert_eq!(
            output.values["action.0.samples"],
            [1.25, 2.5, -3.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn extension_fixtures_keep_signed_zero_extremes_and_distinct_boundary_samples() {
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        let mut count = 0;
        let mut magnitudes = Vec::new();
        for fixture in &fixtures {
            if let Inputs::Extend(input) = fixture.parse().unwrap() {
                assert!(input.offset_distance.is_finite() && input.offset_distance >= 0.0);
                magnitudes.push(input.offset_distance.to_bits());
                count += 1;
            }
        }
        assert_eq!(count, 29);
        for distance in [0.0_f64, -0.0, 1e-12, 1e8] {
            assert!(magnitudes.contains(&distance.to_bits()));
        }
        for (kind, distance) in [
            ("plane", 0.375),
            ("bilinear", 0.0625),
            ("quadratic", 0.25),
            ("rational_quadratic", 0.5),
        ] {
            let find = |suffix| {
                fixtures
                    .iter()
                    .find(|case| case.name == format!("extend_{kind}_{suffix}"))
                    .unwrap()
            };
            let Inputs::Extend(original) = find("positive").parse().unwrap() else {
                unreachable!()
            };
            let fixture = find("moderate_boundary_samples");
            let Inputs::Extend(replacement) = fixture.parse().unwrap() else {
                unreachable!()
            };
            assert_eq!(replacement.offset_distance, distance);
            assert_ne!(replacement.offset_distance, original.offset_distance);
            assert_eq!(
                serde_json::to_value(&replacement.surfaces).unwrap(),
                serde_json::to_value(&original.surfaces).unwrap()
            );
            assert!(replacement.samples.starts_with(&original.samples));
            assert_eq!(replacement.samples.len(), original.samples.len() + 6);
            let output = Runner.run_rust(fixture).unwrap();
            assert!(output.error.is_none() && output.deferred.is_none());
            for index in 0..replacement.actions.len() {
                assert_eq!(output.ints[&format!("action.{index}.sample_count")], [15]);
                assert_eq!(output.values[&format!("action.{index}.samples")].len(), 90);
            }
        }
    }

    #[test]
    fn generated_dtos_roundtrip_and_most_cases_do_real_geometry() {
        let mut geometry = 0;
        let mut modes = BTreeMap::new();
        let mut magnitudes = Vec::new();
        let mut negative_configs = 0;
        for index in 0..MIN_GENERATED {
            let seed = CaseContext::seed_for(Runner.id(), index);
            let value = Runner.generate(&mut Rng::new(seed)).unwrap();
            assert_eq!(value, Runner.generate(&mut Rng::new(seed)).unwrap());
            let input: Inputs = serde_json::from_value(value.clone()).unwrap();
            *modes
                .entry(value["op"].as_str().unwrap().to_owned())
                .or_insert(0_usize) += 1;
            assert_eq!(value, serde_json::to_value(&input).unwrap());
            match &input {
                Inputs::Extend(input) => {
                    assert!(input.offset_distance.is_finite() && input.offset_distance >= 0.0);
                    magnitudes.push(input.offset_distance.to_bits());
                    geometry += 1;
                }
                Inputs::Synchronize(_) => geometry += 1,
                Inputs::Config(input) if input.offset_distance < 0.0 => negative_configs += 1,
                _ => {}
            }
            // Full numeric parity belongs to the optimized difftest run; keep
            // this serialization/coverage test cheap in ordinary debug builds.
        }
        assert!(geometry > MIN_GENERATED * 3 / 4);
        assert!(negative_configs > 0);
        for distance in [0.0_f64, -0.0, 1e-12, 1e7] {
            assert!(magnitudes.contains(&distance.to_bits()));
        }
        eprintln!("600 seeded case shapes: {modes:?}");
    }

    #[test]
    fn refused_split_completes_and_probe_propagates_the_actual_dependency() {
        let input = Inputs::Synchronize(Synchronize {
            first: vec![EdgeIn {
                curve: CurveIn::Line {
                    start: [0.0; 3],
                    end: [1.0, 0.0, 0.0],
                    domain: [0.0, 1.0],
                },
                interval: [0.25, 1.0],
            }],
            second: vec![EdgeIn {
                curve: CurveIn::Line {
                    start: [0.0; 3],
                    end: [1.0, 0.0, 0.0],
                    domain: [0.0, 1.0],
                },
                interval: [0.0, 1.0],
            }],
            null_original: false,
            null_other: false,
        });
        // The fixture B-rep keeps the constructor's editing state (FALSE), as the
        // shim's: MakeVertexSplitEdge refuses and synchronization completes.
        let output = Runner.run_rust(&case(&input)).unwrap();
        assert!(output.deferred.is_none() && output.error.is_none());
        let output = Runner
            .run_rust(&case(&Inputs::Probe {
                branch: Probe::ReplaceEmpty,
                approx_tol: 1e-6,
            }))
            .unwrap();
        assert_eq!(output.deferred.unwrap().unit, "topology.brep_p4");
    }

    const SYNC_LISTS: [&str; 4] = [
        "matched_first",
        "matched_second",
        "gaps_first",
        "gaps_second",
    ];

    fn sync_snapshot(minima: &[(i64, f64)], ids: &[i64]) -> Output {
        let mut output = Output::new()
            .ints("operation", vec![1000, -1, -1])
            .ints(
                "fixture_edgeuses",
                minima.iter().map(|&(id, _)| id).collect::<Vec<_>>(),
            )
            .int("edgeuse_count", minima.len() as i64);
        for (index, &(id, min)) in minima.iter().enumerate() {
            output = output
                .ints(
                    &format!("edgeuse.{index}.ids_statuses"),
                    vec![id, index as i64, 1000, 1000],
                )
                .values(
                    &format!("edgeuse.{index}.geometry"),
                    vec![min, min + 1.0, 1.0, -0.0, 2.0, 3.0, 1.0, 2.0, 3.0],
                );
        }
        for key in SYNC_LISTS {
            output = output.ints(key, ids.to_vec());
        }
        output
    }

    #[test]
    fn sync_ties_sort_only_contiguous_exact_minimum_groups_using_record_ids() {
        // Record slots deliberately differ from traversal IDs. Noncontiguous
        // equal minima must not be brought together, nor distinct groups sorted.
        let original = sync_snapshot(
            &[
                (8, 2.0),
                (2, -0.0),
                (6, 2.0),
                (4, 0.0),
                (10, 4.0),
                (12, 2.0),
            ],
            &[8, 6, 4, 2, 10, 12],
        );
        let mut expected = original.clone();
        for key in SYNC_LISTS {
            expected.ints.insert(key.into(), vec![6, 8, 2, 4, 10, 12]);
        }
        let actual = normalize_sync_ties(original.clone()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.ints["fixture_edgeuses"], [8, 2, 6, 4, 10, 12]);
        for (key, values) in &original.values {
            assert_eq!(
                actual.values[key]
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
        assert_eq!(normalize_sync_ties(actual.clone()).unwrap(), actual);
    }

    #[test]
    fn sync_distinct_minimum_order_is_still_compared_even_one_ulp_apart() {
        let original = sync_snapshot(
            &[
                (0, 1.0),
                (2, f64::from_bits(1.0_f64.to_bits() + 1)),
                (4, 3.0),
            ],
            &[4, 2, 0],
        );
        assert_eq!(normalize_sync_ties(original.clone()).unwrap(), original);
        for key in SYNC_LISTS {
            let mut reversed = original.clone();
            reversed.ints.get_mut(key).unwrap().reverse();
            let reversed = normalize_sync_ties(reversed).unwrap();
            assert!(!compare(&original, &reversed, &[]).pass(), "{key}");
        }
    }

    #[test]
    fn sync_equal_minimum_membership_multiplicity_and_lengths_still_mismatch() {
        let original = sync_snapshot(&[(0, 1.0), (2, 1.0), (4, 1.0)], &[0, 2]);
        for key in SYNC_LISTS {
            let mut permuted = original.clone();
            permuted.ints.get_mut(key).unwrap().reverse();
            assert!(compare(&original, &normalize_sync_ties(permuted).unwrap(), &[]).pass());
            for ids in [vec![0, 4], vec![0, 0], vec![0], vec![0, 2, 4]] {
                let mut changed = original.clone();
                changed.ints.insert(key.into(), ids);
                assert!(
                    !compare(&original, &normalize_sync_ties(changed).unwrap(), &[]).pass(),
                    "{key}"
                );
            }
        }
    }

    #[test]
    fn sync_null_padding_stays_in_place_and_separates_equal_minimum_groups() {
        let original = sync_snapshot(
            &[(0, 1.0), (2, 1.0), (4, 1.0), (6, 1.0)],
            &[-1, 6, 4, -1, 2, 0, -1, -1],
        );
        let actual = normalize_sync_ties(original.clone()).unwrap();
        for key in SYNC_LISTS {
            assert_eq!(actual.ints[key], [-1, 4, 6, -1, 0, 2, -1, -1]);
            let mut moved = original.clone();
            moved.ints.get_mut(key).unwrap().swap(0, 1);
            assert!(!compare(&actual, &normalize_sync_ties(moved).unwrap(), &[]).pass());
        }
    }

    #[test]
    fn sync_unknown_ids_and_invalid_measured_records_are_harness_errors() {
        let original = sync_snapshot(&[(0, 1.0), (2, 1.0)], &[2, 0]);
        for key in SYNC_LISTS {
            for invalid in [-2, 1, 99] {
                let mut output = original.clone();
                output.ints.get_mut(key).unwrap().push(invalid);
                assert!(normalize_sync_ties(output).is_err(), "{key}: {invalid}");
            }
        }
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let output = sync_snapshot(&[(0, invalid)], &[0]);
            assert!(normalize_sync_ties(output).is_err());
        }
        assert!(normalize_sync_ties(sync_snapshot(&[(-2, 1.0)], &[])).is_err());
        assert!(normalize_sync_ties(sync_snapshot(&[(0, 1.0), (0, 1.0)], &[])).is_err());
        let mut missing = original.clone();
        missing.values.remove("edgeuse.1.geometry");
        assert!(normalize_sync_ties(missing).is_err());
        for output in [
            Output::err("operation failure"),
            Output::deferred("topology.brep_p2", "SmBrep::Notify"),
            Output::rejected("native fixture constructor status_1007"),
        ] {
            let output = output.int("edgeuse_count", -2);
            assert_eq!(normalize_sync_ties(output.clone()).unwrap(), output);
        }
    }

    #[test]
    fn native_decoder_normalizes_only_measured_ties_and_rejects_unknown_ids() {
        let input = Inputs::Synchronize(Synchronize {
            first: Vec::new(),
            second: Vec::new(),
            null_original: false,
            null_other: false,
        });
        // Two measured records; the private unordered relation list is [2, 0].
        let raw = [
            0.0, 1000.0, -1.0, -1.0, 2.0, 0.0, 2.0, 2.0, 2.0, 0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 2.0,
            1.0, 1000.0, 0.0, 2.0, 3.0, 1000.0, 1.0, 2.0, 3.0, 2.0, 1.0, 1.0, 2.0, 1.0, 1000.0,
            0.0, 2.0, 3.0, 1000.0, 1.0, 2.0, 3.0, 2.0, 2.0, 0.0, 2.0, -1.0, -1.0, 2.0, 2.0, 0.0,
            2.0, 2.0, 0.0,
        ];
        let output = decode(&input, &raw).unwrap();
        for key in ["matched_first", "gaps_first", "gaps_second"] {
            assert_eq!(output.ints[key], [0, 2]);
        }
        assert_eq!(output.ints["matched_second"], [-1, -1]);
        let mut private_order = raw;
        private_order.swap(8, 9);
        assert_eq!(output, decode(&input, &private_order).unwrap());
        let mut distinct = raw;
        distinct[14] = 0.5;
        assert_eq!(
            decode(&input, &distinct).unwrap().ints["matched_first"],
            [2, 0]
        );
        let mut invalid = raw;
        invalid[39] = -2.0;
        assert!(decode(&input, &invalid).is_err());
    }

    #[test]
    fn actual_rust_tie_fixtures_use_the_same_narrow_snapshot_policy() {
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let fixtures = context.fixture_cases(Runner.id()).unwrap();
        for name in [
            "equal_minima_public_order_tie",
            "equal_minima_matched_order_tie",
        ] {
            let fixture = fixtures.iter().find(|case| case.name == name).unwrap();
            let actual = Runner.run_rust(fixture).unwrap();
            assert!(actual.error.is_none() && actual.deferred.is_none());
            assert_eq!(actual.ints["matched_first"], [0, 2]);
            let Inputs::Synchronize(input) = fixture.parse().unwrap() else {
                unreachable!()
            };
            let unnormalized = rust_sync(&input).ok().unwrap();
            assert_eq!(actual, normalize_sync_ties(unnormalized).unwrap());
            // A native snapshot may emit the opposite hash order for these ties.
            let mut opposite = actual.clone();
            opposite.ints.get_mut("matched_first").unwrap().reverse();
            assert!(compare(&actual, &normalize_sync_ties(opposite).unwrap(), &[]).pass());
            assert_eq!(
                actual.ints["fixture_edgeuses"],
                if input.second.is_empty() {
                    vec![0, 2]
                } else {
                    vec![0, 2, 4]
                }
            );
        }
    }

    #[test]
    fn actual_rust_unique_minima_and_null_padding_are_not_reordered() {
        let edge = |min| EdgeIn {
            curve: CurveIn::Line {
                start: [1.0, 2.0, 3.0],
                end: [2.0, 2.0, 3.0],
                domain: [min, min + 1.0],
            },
            interval: [min, min + 1.0],
        };
        let input = Inputs::Synchronize(Synchronize {
            first: vec![edge(2.0), edge(0.0)],
            second: Vec::new(),
            null_original: false,
            null_other: false,
        });
        let output = Runner.run_rust(&case(&input)).unwrap();
        assert_eq!(output.ints["matched_first"], [2, 0]);
        assert_eq!(output.ints["matched_second"], [-1, -1]);
        let mut reversed = output.clone();
        reversed.ints.get_mut("matched_first").unwrap().reverse();
        assert!(!compare(&output, &normalize_sync_ties(reversed).unwrap(), &[]).pass());
    }
}
