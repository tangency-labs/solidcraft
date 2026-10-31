//! Runner for unit `blend.hcr`. C++: `solidcore_ffi::units::blend_hcr`. Rust:
//! `solidcore::blend::hcr`.
//!
//! The C++ side builds a real `SmBrep` (the B-rep blocks of unit `topology.vertexuse`:
//! upstream test files, a wire edge, a face from a B-spline or plane surface, the wire
//! network), optionally adds a second wire edge, constructs an `SmHCR` on it from a view
//! vector or a view placement, seeds silhouette edges (the silhouette branch of
//! `ComputeSilhouettes`' edge loop) and back-facing edges, and runs one member. It
//! exports the scene B-rep (`GetSceneBrep()`, the copy transformed by the unported
//! `SmBrep` copy constructor and `Transform`) with its `CalculateBoundingBox`. The Rust
//! side makes everything itself, in the shim's call order, so no C++ state reaches it:
//! the B-rep block built by the Rust kernel ([`crate::own_builder`], `.smb` files by the
//! Rust reader), the extra wire edge (`create_wire_edge_from_curve`), the HCR from
//! [`Hcr::new`] / [`Hcr::with_transform`] (its own scene copy, transform, infinite
//! region and bounding box), the scene renumbered into the shim's traversal order
//! ([`crate::own_builder::renumber_with`], so edge and vertex selectors name the same
//! entities), then the same seeds and member. The C++ side checks the seeds and op
//! arguments against its scene with `ffi::validate` (shim op 8); the Rust side makes the
//! same checks against its own scene ([`validate_rust`]).
//!
//! Compared: the construction state (transforms, projection box, ray offset tolerance,
//! view vector, options), `IsSilhouetteEdge` / `IsBackFacingEdge`, the intervals and
//! point classes of `IntersectCurveWithSilhouettes` / `ComputeCurveVisibility`, the
//! visible and hidden curve pieces of `ComputeEdgeVisibility` (natural interval and 5
//! points each), `GetSilhouetteEdges` (sorted: upstream iterates a pointer hash) and
//! `IsPointVisible`. The classifications of ops 3 and 4 are built with
//! [`new_curve_classification`], the co-port of the constructor `ComputeEdgeVisibility`
//! uses, against the C++ `SmCurveClassification(curve, ivl, NULL, tol, 64, data)`.
//!
//! The crossing fixtures are two closed wire loops (degree 1 or 3 B-splines) at
//! different heights, seen along +Z or a slight tilt: the projection of the second loop
//! crosses the first in front of or behind it, overlaps it along a side (range
//! solutions), or puts a vertex over a side's interior, so the classifications get two or
//! more intervals and `ComputeEdgeVisibility` several pieces. Generated B-rep blocks are
//! also scaled by 1e-4 and 1e4, and a face on a surface closed in u (seam edge) is
//! generated.
//!
//! Status `-2` marks an upstream path that is undefined (UB-170, null topology): the
//! shim does not run it. An empty record with that sentinel is reported as a native
//! rejection, never as a successful comparison. Rust-only regressions exercise the
//! safe UB-170 continuation independently. The Rust scene holds its geometry natively,
//! so no case is skipped as inexact.
//!
//! The renumbered scene's geometry is owned under the kernel's arena identities (and the
//! B-rep records that naming), so `SmBrep::GetSurfaces` (`IsPointVisible`) finds each face
//! surface's owner, as upstream's copy constructor sets it.
//!
//! `ComputeSilhouettes` (op `silhouettes`: the silhouette edges, sorted, each with its
//! scene edge index or `-1` for a new edge, its interval and 5 points) and
//! `ComputeGlobalVisibility` (op `global_visibility`: the visible and hidden pieces) run
//! on the upstream solid bodies (`solid_bodies.json`), each C++ case in a fresh process.
//! Their C++ path reaches UB-170 on open silhouette chains, so op `global_visibility`
//! first runs `ComputeSilhouettes` on a copy of the HCR (the C++ side: a second `SmHCR`
//! of the same B-rep) and outputs status `-2` when some scene edge's classification
//! there has more silhouette vertices than edges in its box: the port's
//! `IntersectCurveWithSilhouettes` reports that as `InvalidInput` ("UB-170").

use super::topology_vertexuse::{gen_brep, path_of, push_curve, push_surface, sel, BrepIn, CrvIn};
use crate::{own_reader, Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::blend::hcr::{new_curve_classification, Hcr, HcrObject, HcrOptions};
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::PointClassification;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::{edge, EdgeId, VertexId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::{blend_hcr as ffi, topology_vertexuse as fixture};
use std::collections::BTreeSet;

/// The unit runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Construct,
    IsSilhouette { edge: i64 },
    IsBackFacing { edge: i64 },
    Intersect { edge: i64, fa: f64, fb: f64 },
    CurveVisibility { edge: i64, fa: f64, fb: f64, start_hint: f64, end_hint: f64 },
    EdgeVisibility { edge: i64, vertex: i64, visible: bool, premark: bool },
    SilhouetteEdges,
    PointVisible { p: [f64; 3] },
    GlobalVisibility,
    Silhouettes,
}

/// A wire edge added to the B-rep (`CreateWireEdgeFromCurve`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtraIn {
    curve: CrvIn,
    t0: f64,
    t1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    extra: Option<ExtraIn>,
    /// View vector (mode 0) or the placement origin (mode 1).
    view: [f64; 3],
    /// Placement `Z`, `Xref` (mode 1).
    #[serde(default)]
    placement: Option<[[f64; 3]; 2]>,
    approx_tol: f64,
    ang_tol: f64,
    #[serde(default = "yes")]
    seams: bool,
    #[serde(default = "yes")]
    smooth: bool,
    /// Seeded silhouette edges (selectors); `[-1]`: every edge.
    #[serde(default)]
    sil: Vec<i64>,
    /// Seeded back-facing edges (selectors).
    #[serde(default)]
    back: Vec<i64>,
    #[serde(flatten)]
    op: OpIn,
}

const fn yes() -> bool {
    true
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The B-rep block (as unit `topology.vertexuse`, without gap initialisation).
fn brep_block(b: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match b {
        BrepIn::File { rebuild, .. } => v.extend([0.0, flag(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
    }
    v.extend([0.0, 0.0]);
    Ok(v)
}

/// The extra wire and view part of the input.
fn hcr_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match &i.extra {
        None => v.push(0.0),
        Some(x) => {
            v.push(1.0);
            push_curve(&mut v, &x.curve)?;
            v.extend([x.t0, x.t1]);
        }
    }
    v.push(if i.placement.is_some() { 1.0 } else { 0.0 });
    v.extend(i.view);
    if let Some([z, x]) = i.placement {
        v.extend(z);
        v.extend(x);
    }
    v.extend([i.approx_tol, i.ang_tol, flag(i.seams), flag(i.smooth)]);
    Ok(v)
}

/// Selectors resolved against the scene's edge and vertex counts.
struct Resolved {
    sil: Vec<i64>,
    back: Vec<i64>,
    op: OpIn,
}

fn resolve(i: &Inputs, n_e: usize, n_v: usize) -> Option<Resolved> {
    if n_e == 0 && !matches!(
            i.op,
            OpIn::Construct
                | OpIn::SilhouetteEdges
                | OpIn::PointVisible { .. }
                | OpIn::GlobalVisibility
                | OpIn::Silhouettes
        ) {
        return None;
    }
    let e = |s: i64| sel(s, n_e).max(0);
    let sil = if i.sil == [-1] {
        (0..n_e.min(64) as i64).collect()
    } else if n_e == 0 {
        Vec::new()
    } else {
        i.sil.iter().map(|&s| e(s)).collect()
    };
    let back = if n_e == 0 { Vec::new() } else { i.back.iter().map(|&s| e(s)).collect() };
    let op = match i.op.clone() {
        OpIn::IsSilhouette { edge } => OpIn::IsSilhouette { edge: e(edge) },
        OpIn::IsBackFacing { edge } => OpIn::IsBackFacing { edge: e(edge) },
        OpIn::Intersect { edge, fa, fb } => OpIn::Intersect { edge: e(edge), fa, fb },
        OpIn::CurveVisibility { edge, fa, fb, start_hint, end_hint } => {
            OpIn::CurveVisibility { edge: e(edge), fa, fb, start_hint, end_hint }
        }
        OpIn::EdgeVisibility { edge, vertex, visible, premark } => OpIn::EdgeVisibility {
            edge: e(edge),
            vertex: if vertex < 0 || n_v == 0 { -1 } else { sel(vertex, n_v) },
            visible,
            premark,
        },
        other => other,
    };
    Some(Resolved { sil, back, op })
}

fn op_args(op: &OpIn) -> (u8, Vec<f64>) {
    match *op {
        OpIn::Construct => (0, vec![]),
        OpIn::IsSilhouette { edge } => (1, vec![edge as f64]),
        OpIn::IsBackFacing { edge } => (2, vec![edge as f64]),
        OpIn::Intersect { edge, fa, fb } => (3, vec![edge as f64, fa, fb, 0.0, 0.0]),
        OpIn::CurveVisibility { edge, fa, fb, start_hint, end_hint } => {
            (4, vec![edge as f64, fa, fb, start_hint, end_hint])
        }
        OpIn::EdgeVisibility { edge, vertex, visible, premark } => {
            (5, vec![edge as f64, vertex as f64, flag(visible), flag(premark)])
        }
        OpIn::SilhouetteEdges => (6, vec![]),
        OpIn::PointVisible { p } => (7, p.to_vec()),
        OpIn::GlobalVisibility => (9, vec![]),
        OpIn::Silhouettes => (10, vec![]),
    }
}

/// The seeds and op arguments.
fn ops_block(r: &Resolved) -> (u8, Vec<f64>) {
    let (op, args) = op_args(&r.op);
    let mut v = vec![r.sil.len() as f64];
    v.extend(r.sil.iter().map(|&x| x as f64));
    v.push(r.back.len() as f64);
    v.extend(r.back.iter().map(|&x| x as f64));
    v.extend(args);
    (op, v)
}

/// What the C++ side computes before running: the scene (shim op 8), the resolved
/// selectors and the op block, validated against that scene.
struct Prepared {
    path: String,
    brep: Vec<f64>,
    hcr: Vec<f64>,
    op: u8,
    ops: Vec<f64>,
}

/// [`Prepared`], or the label of a rejection.
fn prepare(i: &Inputs) -> Result<Result<Prepared, String>, HarnessError> {
    let path = path_of(&i.brep)?;
    let brep = brep_block(&i.brep)?;
    let hcr = hcr_block(i)?;
    // a failed build of the B-rep block (the shim's `build` returns NULL), reported with
    // its status as the Rust side reports its own failed build
    if let Ok(ex) = fixture::eval(fixture::Op::Export, &path, &brep) {
        if ex.first() == Some(&-1.0) {
            let status = ex.get(1).copied().unwrap_or(0.0);
            return Ok(Err(format!("brep_build_status_{status}")));
        }
    }
    let scene = match ffi::scene(&path, &brep, &hcr) {
        Ok(s) => s,
        Err(e) => return Ok(Err(format!("rejected: {e}"))),
    };
    let s = ffi::split(&scene).ok_or_else(|| harness("short scene output"))?;
    let c = fixture::counts(s.export).ok_or_else(|| harness("scene export"))?;
    let Some(r) = resolve(i, c[7], c[9]) else {
        return Ok(Err("no_edge".into()));
    };
    let (op, ops) = ops_block(&r);
    if let Err(e) = ffi::validate(op, &scene, &ops) {
        return Ok(Err(format!("rejected: {e}")));
    }
    Ok(Ok(Prepared {
        path,
        brep,
        hcr,
        op,
        ops,
    }))
}

// ---- the Rust side's own scene and checks ---------------------------------------------------

/// The label of an input rejection (`ffi::scene` / `ffi::validate`'s `bad`).
fn bad(msg: &str) -> String {
    format!("rejected: invalid argument: blend.hcr: {msg}")
}

/// `ffi`'s `MAX_SCENE_MAGNITUDE` check on the inputs (the `±f64::MAX` markers aside).
fn check_magnitudes(v: &[f64]) -> Result<(), String> {
    if v.iter().any(|x| {
        let a = x.abs();
        a > ffi::MAX_SCENE_MAGNITUDE && a < f64::MAX
    }) {
        return Err(bad("scene value above 1e15 (grid walk does not end)"));
    }
    Ok(())
}

/// `ffi`'s view checks on the inputs (UB-169: a view vector `SmVector3d::Unitize`
/// cannot unitize; the placement, tolerances and flags are typed inputs here).
fn check_view(i: &Inputs) -> Result<(), String> {
    if i.placement.is_none() {
        let [x, y, z] = i.view;
        let l = x * x + y * y + z * z;
        if !(l >= 1.0e-24 && l.is_finite()) {
            return Err(bad("UB-169: view vector cannot be unitized"));
        }
    }
    Ok(())
}

/// `ffi::validate`'s checks of the seeds and op arguments (`ops`, as [`ops_block`]
/// makes them) against the Rust scene `h` (its edge and vertex counts and which edges
/// have a curve).
fn validate_rust(op: u8, h: &Hcr, ops: &[f64]) -> Result<(), String> {
    if op == 7 {
        check_magnitudes(ops)?;
    }
    let b = h.scene_brep();
    let (n_e, n_v) = (b.edges().len(), b.vertices().len());
    let mut at = 0usize;
    let mut num = || -> Result<f64, String> {
        let x = *ops.get(at).ok_or_else(|| bad("short input"))?;
        at += 1;
        if !x.is_finite() {
            return Err(bad("non-finite number"));
        }
        Ok(x)
    };
    let count = |x: f64| -> Result<usize, String> {
        if x.fract() != 0.0 || !(0.0..=64.0).contains(&x) {
            return Err(bad("count out of range"));
        }
        Ok(x as usize)
    };
    let index = |x: f64, n: usize, allow_none: bool| -> Result<i64, String> {
        if x.fract() != 0.0 || x < if allow_none { -1.0 } else { 0.0 } || x >= n as f64 {
            return Err(bad("index out of range"));
        }
        Ok(x as i64)
    };
    let flag = |x: f64| -> Result<(), String> {
        if x != 0.0 && x != 1.0 {
            return Err(bad("flag"));
        }
        Ok(())
    };
    let n_sil = count(num()?)?;
    for _ in 0..n_sil {
        let e = index(num()?, n_e, false)?;
        let has_curve = b.edge(eid(e)).is_ok_and(|x| x.curve.is_some());
        if !has_curve {
            return Err(bad("seeded silhouette edge without curve"));
        }
    }
    let n_back = count(num()?)?;
    for _ in 0..n_back {
        index(num()?, n_e, false)?;
    }
    if op >= 9 && n_sil + n_back > 0 {
        return Err(bad("seeds for a whole-scene op"));
    }
    match op {
        1 | 2 => {
            index(num()?, n_e, false)?;
        }
        3 | 4 => {
            index(num()?, n_e, false)?;
            let (fa, fb) = (num()?, num()?);
            if !(0.0..=1.0).contains(&fa) || !(0.0..=1.0).contains(&fb) {
                return Err(bad("interval fractions"));
            }
            for _ in 0..2 {
                let h = num()?;
                if !(-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&h) {
                    return Err(bad("hint"));
                }
            }
        }
        5 => {
            index(num()?, n_e, false)?;
            index(num()?, n_v, true)?;
            flag(num()?)?;
            flag(num()?)?;
        }
        7 => {
            for _ in 0..3 {
                num()?;
            }
        }
        _ => {}
    }
    if at != ops.len() {
        return Err(bad("trailing op input"));
    }
    Ok(())
}

/// The identity of an original B-rep object the scene copy notifies under: the
/// notification naming in a key namespace of its own (as the sweep's HCR scene copy).
fn source_key(e: EntityRef) -> ObjectRef {
    let r = own_reader::read_key_of(e);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(r.key.0 | 1 << 50),
    }
}

/// The Rust side's HCR, as the shim makes it: the B-rep block built by the Rust kernel,
/// the extra wire edge, the HCR constructor (scene copy, transform, infinite region,
/// bounding box), the scene renumbered into the shim's `walk` order; or the output of a
/// rejected or failed build.
fn rust_hcr(k: &mut Kernel, i: &Inputs) -> Result<Result<Hcr, Output>, HarnessError> {
    use crate::own_builder::{build_as, failed_output, renumber_with, Built, Conventions};
    let conv = &Conventions::EDGEUSE_GRAPH;
    // `ffi::scene`'s input checks
    let (brep, hcr) = (brep_block(&i.brep)?, hcr_block(i)?);
    if let Err(m) = check_magnitudes(&brep).and_then(|()| check_magnitudes(&hcr)) {
        return Ok(Err(Output::err(m)));
    }
    // the fixture export's block checks (`fixture::eval`; pure Rust, no shim call)
    if let Err(e) = fixture::validate_block(&path_of(&i.brep)?, &brep) {
        return Ok(Err(Output::err(format!("rejected: {e}"))));
    }
    // `ffi::validate_hcr` admits the extra curve through a wire fixture too.
    if let Some(x) = &i.extra {
        let wire = brep_block(&BrepIn::Wire { curve: x.curve.clone(), t0: x.t0, t1: x.t1 })?;
        if let Err(e) = fixture::validate_block("", &wire) {
            return Ok(Err(Output::err(format!("rejected: {e}"))));
        }
    }
    if let Err(m) = check_view(i) {
        return Ok(Err(Output::err(m)));
    }
    // the shim's `build` (`sc_topology_fixture.h`, no validate-only, no gap walk)
    let built = match &i.brep {
        BrepIn::File { rebuild, .. } => {
            own_reader::file_in_order(k, &path_of(&i.brep)?, *rebuild, false, false, conv)?
        }
        src => build_as(k, src, false, false, conv)?
            .ok_or_else(|| harness("own builder: no build for the source"))?,
    };
    let mut b = match built {
        Built::Brep(b) => *b,
        Built::Failed(e) => return Ok(Err(failed_output(&e))),
    };
    // the extra wire edge (`CreateWireEdgeFromCurve`)
    if let Some(x) = &i.extra {
        let curve: super::topology_edgeuse_p1::CrvIn = serde_json::to_value(&x.curve)
            .and_then(serde_json::from_value)
            .map_err(|e| harness(format!("extra curve: {e}")))?;
        let c = Curve::from(super::topology_edgeuse_p1::bspline_of(&curve)?);
        if let Err(e) = solidcore::topology::brep::p5::create_wire_edge_from_curve(
            k,
            &mut b,
            c,
            Extent1d::from_min_max(x.t0, x.t1),
        ) {
            return Ok(Err(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("extra_wire_failed: {e}")))));
        }
    }
    let opts = HcrOptions {
        approx_tol_3d: i.approx_tol,
        ang_tol_rad: i.ang_tol,
        display_seams: i.seams,
        display_smooth_edges: i.smooth,
    };
    let mut state = ArenaNotifyState::default();
    let made = match i.placement {
        None => Hcr::new(
            k,
            &mut state.env(None),
            &b,
            &source_key,
            Vec3::new(i.view[0], i.view[1], i.view[2]),
            opts,
        ),
        Some([z, x]) => {
            let mut a = Axis2Placement::default();
            a.set_step_canonical(
                Vec3::new(i.view[0], i.view[1], i.view[2]),
                Vec3::new(z[0], z[1], z[2]),
                Vec3::new(x[0], x[1], x[2]),
            )
            .and_then(|()| Hcr::with_transform(k, &mut state.env(None), &b, &source_key, &a, opts))
        }
    };
    let mut h = match made {
        Ok(h) => h,
        Err(e) => {
            return Ok(Err(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("hcr_construct_failed: {e}")))))
        }
    };
    // the shim's `walk(scene, g)` order: the scene's arena ids renumbered to it (nothing
    // in the freshly constructed HCR refers to a scene id yet), its geometry owned under
    // the arena identities the renumbering gives it
    let mut scene = renumber_with(h.scene_brep(), conv)?;
    scene
        .use_arena_naming()
        .map_err(|e| harness(format!("scene naming: {e}")))?;
    *h.scene_brep_mut() = scene;
    Ok(Ok(h))
}

fn output(status: f64, record: Vec<f64>) -> Output {
    Output::new().values("status", vec![status]).values("record", record)
}

// The shim emits an empty -2 record only when its existing UB/null-topology
// guards refuse the call. An operation that actually ran emits its record even
// on failure; do not turn a real status -2 with a record into a rejection.
fn native_output(status: f64, record: Vec<f64>) -> Output {
    if status == -2.0 && record.is_empty() {
        Output::rejected("blend.hcr native UB-170/null-topology guard refused undefined input")
    } else {
        output(status, record)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.hcr"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let p = match prepare(&i)? {
            Ok(p) => p,
            Err(msg) => return Ok(Output::err(msg)),
        };
        let v = match ffi::eval(p.op, &p.path, &p.brep, &p.hcr, &p.ops) {
            Ok(v) => v,
            Err(e) => return Ok(Output::err(format!("rejected: {e}"))),
        };
        let o = ffi::split(&v).ok_or_else(|| harness("short output"))?;
        Ok(native_output(o.status, o.record.to_vec()))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        let h = match rust_hcr(&mut k, &i)? {
            Ok(h) => h,
            Err(o) => return Ok(o),
        };
        let b = h.scene_brep();
        let Some(r) = resolve(&i, b.edges().len(), b.vertices().len()) else {
            return Ok(Output::err("no_edge"));
        };
        let (op, ops) = ops_block(&r);
        if let Err(m) = validate_rust(op, &h, &ops) {
            return Ok(Output::err(m));
        }
        match run_rust(&mut k, h, &r) {
            Ok((status, record)) => Ok(output(status, record)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| match e {
                SolidError::InvalidInput {
                    operation: "blend::hcr",
                    ..
                } => output(-2.0, vec![]),
                SolidError::Upstream { code, .. } => output(f64::from(code), vec![]),
                other => Output::err(format!("rust_error: {other}")),
            })),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        Some(gen_case(rng))
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            root: ctx.root.clone(),
            seeds: ctx.seeds.max(400),
        };
        crate::runner::default_cases(self, &ctx)
    }
}

/// `(ULONG)(int64_t)hint`: the shim's conversion of a hint (truncates toward zero, wraps
/// negative values).
fn hint_ulong(h: f64) -> u64 {
    h as i64 as u64
}

fn eid(i: i64) -> EdgeId {
    EdgeId::from_index(i as u32)
}

fn put_axis(o: &mut Vec<f64>, a: &Axis2Placement) {
    for v in [a.origin(), a.x_axis(), a.y_axis(), a.z_axis()] {
        o.extend([v.x, v.y, v.z]);
    }
}

fn put_box(o: &mut Vec<f64>, b: &Extent3d) {
    let (lo, hi) = (b.min(), b.max());
    o.extend([lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]);
}

fn put_pc(o: &mut Vec<f64>, pc: &PointClassification<HcrObject>) {
    o.push(f64::from(pc.point_class().code()));
    let (kind, idx) = match pc.object() {
        None => (0.0, -1.0),
        Some(HcrObject::Vertex(v)) => (1.0, f64::from(v.index())),
        Some(HcrObject::Edge(e)) => (2.0, f64::from(e.index())),
        Some(HcrObject::Region(r)) => (3.0, f64::from(r.index())),
        Some(HcrObject::Curve(_)) => (4.0, -1.0),
    };
    o.extend([kind, idx, pc.pre_snap_param(), pc.t_param()]);
}

fn put_cc(o: &mut Vec<f64>, cc: &CurveClassification<HcrObject>) {
    o.push(cc.size() as f64);
    for c in cc.intervals() {
        o.extend([c.interval.min(), c.interval.max()]);
        put_pc(o, &c.start);
        put_pc(o, &c.mid);
        put_pc(o, &c.end);
    }
}

fn put_piece(k: &Kernel, o: &mut Vec<f64>, c: &Curve) {
    let n = c.natural_interval();
    o.extend([n.min(), n.max()]);
    for i in 0..5 {
        match c.evaluate_point(k, n.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
            Err(e) => o.extend([
                match e {
                    SolidError::Upstream { code, .. } => f64::from(code),
                    _ => -1.0,
                },
                0.0,
                0.0,
                0.0,
            ]),
        }
    }
}

/// The shim's `global_undefined`: after `ComputeSilhouettes` on a copy of `h`, some scene
/// edge's classification (as `ComputeEdgeVisibility` makes it) reaches UB-170 in
/// `IntersectCurveWithSilhouettes`.
fn global_undefined(k: &mut Kernel, h: &Hcr) -> Result<bool, SolidError> {
    let mut h2 = h.clone();
    // its status is ignored, as `ComputeGlobalVisibility` does
    if let Err(e) = h2.compute_silhouettes(k) {
        if e.is_unported() {
            return Err(e);
        }
    }
    let scene = h2.scene_brep();
    for e in solidcore::topology::brep::p1::edges(scene, None)? {
        if edge::curve(scene, e)?.is_none() {
            continue;
        }
        let ivl = edge::interval(scene, e)?;
        let tol = edge::tolerance(scene, e)?;
        let mut cc = new_curve_classification(k, HcrObject::Curve(e), ivl, tol);
        match h2.intersect_curve_with_silhouettes(k, &mut cc) {
            Err(SolidError::InvalidInput { reason, .. }) if reason.starts_with("UB-170") => {
                return Ok(true)
            }
            Err(e) if e.is_unported() => return Err(e),
            _ => {}
        }
    }
    Ok(false)
}

/// The shim's `put_silhouettes`: per silhouette edge its index in the scene as
/// constructed (`-1` for an edge `ComputeSilhouettes` made), its interval and 5 points
/// of its curve over that interval, sorted.
fn put_silhouettes(k: &Kernel, o: &mut Vec<f64>, h: &Hcr, n_e: usize) -> Result<(), SolidError> {
    let scene = h.scene_brep();
    let mut recs: Vec<Vec<f64>> = Vec::new();
    for e in h.silhouette_edges() {
        let id = if (e.index() as usize) < n_e { f64::from(e.index()) } else { -1.0 };
        let ivl = edge::interval(scene, e)?;
        let mut r = vec![id, ivl.min(), ivl.max()];
        let curve = edge::curve(scene, e)?;
        for j in 0..5 {
            let t = ivl.evaluate(f64::from(j) / 4.0);
            match curve.map(|c| c.evaluate_point(k, t)) {
                Some(Ok(p)) => r.extend([0.0, p.x, p.y, p.z]),
                Some(Err(SolidError::Upstream { code, .. })) => {
                    r.extend([f64::from(code), 0.0, 0.0, 0.0]);
                }
                Some(Err(_)) => r.extend([-1.0, 0.0, 0.0, 0.0]),
                // `SM_ERR`
                None => r.extend([1001.0, 0.0, 0.0, 0.0]),
            }
        }
        recs.push(r);
    }
    recs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    o.push(recs.len() as f64);
    for r in recs {
        o.extend(r);
    }
    Ok(())
}

fn run_rust(k: &mut Kernel, mut h: Hcr, r: &Resolved) -> Result<(f64, Vec<f64>), SolidError> {
    for &e in &r.sil {
        h.add_silhouette_edge(k, eid(e))?;
    }
    for &e in &r.back {
        h.add_back_facing_edge(eid(e));
    }
    let mut rec = Vec::new();
    let mut status = 0.0;
    match r.op {
        OpIn::Construct => {
            put_axis(&mut rec, h.view_transform());
            put_axis(&mut rec, h.inv_view_transform());
            put_box(&mut rec, h.proj_bbox());
            rec.push(h.ray_offset_tolerance());
            let v = h.view_vector();
            rec.extend([v.x, v.y, v.z, h.approx_tol_3d(), h.ang_tol_rad()]);
            rec.extend([flag(h.display_seams()), flag(h.display_smooth_edges())]);
        }
        OpIn::IsSilhouette { edge } => rec.push(flag(h.is_silhouette_edge(k, eid(edge))?)),
        OpIn::IsBackFacing { edge } => rec.push(flag(h.is_back_facing_edge(k, eid(edge))?)),
        OpIn::Intersect { edge, fa, fb } | OpIn::CurveVisibility { edge, fa, fb, .. } => {
            let e = eid(edge);
            let ivl = edge::interval(h.scene_brep(), e)?;
            let ivl = Extent1d::from_min_max(ivl.evaluate(fa), ivl.evaluate(fb));
            let tol = edge::tolerance(h.scene_brep(), e)?;
            let mut cc = new_curve_classification(k, HcrObject::Curve(e), ivl, tol);
            let res = match r.op {
                OpIn::CurveVisibility { start_hint, end_hint, .. } => {
                    h.compute_curve_visibility(k, &mut cc, hint_ulong(start_hint), hint_ulong(end_hint))
                }
                _ => h.intersect_curve_with_silhouettes(k, &mut cc),
            };
            match res {
                Ok(()) => {}
                Err(e) if e.is_unported() || matches!(e, SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => return Err(e),
                Err(SolidError::Upstream { code, .. }) => status = f64::from(code),
                Err(e) => return Err(e),
            }
            put_cc(&mut rec, &cc);
        }
        OpIn::EdgeVisibility { edge, vertex, visible, premark } => {
            let mut marked = BTreeSet::new();
            if premark {
                marked.insert(eid(edge));
            }
            let v = (vertex >= 0).then(|| VertexId::from_index(vertex as u32));
            match h.compute_edge_visibility(k, eid(edge), v, visible, &mut marked) {
                Ok(()) => {}
                Err(SolidError::Upstream { code, .. }) => status = f64::from(code),
                Err(e) => return Err(e),
            }
            rec.push(h.visible_curves().len() as f64);
            for c in h.visible_curves() {
                put_piece(k, &mut rec, c);
            }
            rec.push(h.invisible_curves().len() as f64);
            for c in h.invisible_curves() {
                put_piece(k, &mut rec, c);
            }
        }
        OpIn::SilhouetteEdges => {
            // sorted, as the shim's put_sorted (the pointer-hash order is not compared)
            let mut edges = h.silhouette_edges();
            edges.sort_unstable();
            rec.push(edges.len() as f64);
            rec.extend(edges.iter().map(|e| f64::from(e.index())));
        }
        OpIn::PointVisible { p } => rec.push(flag(h.is_point_visible(k, Vec3::new(p[0], p[1], p[2]))?)),
        OpIn::GlobalVisibility => {
            if global_undefined(k, &h)? {
                return Ok((-2.0, Vec::new()));
            }
            match h.compute_global_visibility(k) {
                Ok(()) => {}
                Err(SolidError::Upstream { code, .. }) => status = f64::from(code),
                Err(e) => return Err(e),
            }
            rec.push(h.visible_curves().len() as f64);
            for c in h.visible_curves() {
                put_piece(k, &mut rec, c);
            }
            rec.push(h.invisible_curves().len() as f64);
            for c in h.invisible_curves() {
                put_piece(k, &mut rec, c);
            }
        }
        OpIn::Silhouettes => {
            let n_e = h.scene_brep().edges().len();
            match h.compute_silhouettes(k) {
                Ok(()) => {}
                Err(SolidError::Upstream { code, .. }) => status = f64::from(code),
                Err(e) => return Err(e),
            }
            put_silhouettes(k, &mut rec, &h, n_e)?;
        }
    }
    Ok((status, rec))
}

// ---- generation -------------------------------------------------------------------------

/// A closed loop through the corners `pts` (the first repeated at the end): a clamped
/// degree-1 B-spline, or a degree-3 one on the same control polygon.
fn loop_curve(pts: &[[f64; 3]], cubic: bool) -> Value {
    let mut cpts: Vec<[f64; 4]> = pts.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect();
    if let Some(&f) = cpts.first() {
        cpts.push(f);
    }
    let p: usize = if cubic { 3 } else { 1 };
    let interior = cpts.len().saturating_sub(p + 1);
    let mut u = vec![0.0; p + 1];
    u.extend((1..=interior).map(|j| j as f64 / (interior + 1) as f64));
    u.extend(vec![1.0; p + 1]);
    json!({"dim": 3, "cpts": cpts, "p": p, "u": u})
}

/// Scales the points of a generated B-rep block (curve control points, surface net,
/// plane origin) by `s`.
fn scale_brep(b: &mut Value, s: f64) {
    let scale_pt = |p: &mut Value| {
        if let Some(a) = p.as_array_mut() {
            for c in a.iter_mut().take(3) {
                if let Some(x) = c.as_f64() {
                    if x.abs() < 1e300 {
                        *c = json!(x * s);
                    }
                }
            }
        }
    };
    if let Some(pts) = b.pointer_mut("/curve/cpts").and_then(Value::as_array_mut) {
        pts.iter_mut().for_each(scale_pt);
    }
    if let Some(rows) = b.pointer_mut("/surface/bspline/net").and_then(Value::as_array_mut) {
        for row in rows.iter_mut() {
            if let Some(r) = row.as_array_mut() {
                r.iter_mut().for_each(scale_pt);
            }
        }
    }
    if let Some(o) = b.pointer_mut("/surface/plane/o") {
        scale_pt(o);
    }
}

/// A face on a B-spline surface closed in u (a square tube, degree 1): its loop has a
/// seam edge.
fn periodic_face(s: f64) -> Value {
    let ring = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0], [1.0, 0.0]];
    let net: Vec<Vec<[f64; 4]>> = ring
        .iter()
        .map(|[x, y]| vec![[x * s, y * s, 0.0, 1.0], [x * s, y * s, s, 1.0]])
        .collect();
    json!({"kind": "face", "surface": {"bspline": {"net": net, "p": 1, "q": 1,
        "u": [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0], "v": [0.0, 0.0, 1.0, 1.0]}},
        "dom": [0.0, 0.0, 1.0, 1.0]})
}

fn gen_view(rng: &mut Rng) -> (Value, Option<Value>) {
    let view = match rng.below(16) {
        0 => json!([0.0, 0.0, 1.0]),
        1 => json!([0.0, 0.0, -1.0]),
        2 => json!([1.0, 0.0, 0.0]),
        3 => json!([0.0, 1e-13, 0.0]),
        4 => json!([1e200, 0.0, 0.0]), // UB-169 (overflowing squared length): rejected
        _ => {
            let p = rng.point3(-1.0, 1.0);
            let s = *rng.choose(&[1e-4, 1.0, 1e4]).unwrap_or(&1.0);
            json!([p[0] * s, p[1] * s, p[2] * s + 1e-3])
        }
    };
    let placement = rng.chance(0.2).then(|| {
        let z = rng.point3(-1.0, 1.0);
        let x = if rng.chance(0.2) { z } else { rng.point3(-1.0, 1.0) };
        json!([[z[0], z[1], z[2] + 0.01], [x[0], x[1], x[2]]])
    });
    (view, placement)
}

fn gen_op(rng: &mut Rng, edge: i64, crossing: bool) -> Value {
    let (fa, fb) = if rng.chance(0.6) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(0.0, 0.6);
        (a, a + rng.uniform(0.1, 0.4))
    };
    let hint = |rng: &mut Rng| *rng.choose(&[0u8, 1, 1, 1, 2]).unwrap_or(&1);
    let vertex = if rng.chance(0.3) { -1 } else { rng.below(1 << 20) as i64 };
    let pick = if crossing { 2 + rng.below(12) } else { rng.below(16) };
    match pick {
        0 | 1 => json!({"op": "construct"}),
        2 => json!({"op": "is_silhouette", "edge": edge}),
        3 => json!({"op": "is_back_facing", "edge": edge}),
        4..=7 => json!({"op": "intersect", "edge": edge, "fa": fa, "fb": fb}),
        8 | 9 => json!({"op": "curve_visibility", "edge": edge, "fa": fa, "fb": fb,
                         "start_hint": hint(rng), "end_hint": hint(rng)}),
        10..=13 => json!({"op": "edge_visibility", "edge": edge, "vertex": vertex,
                           "visible": rng.chance(0.8), "premark": rng.chance(0.1)}),
        14 => json!({"op": "silhouette_edges"}),
        _ => {
            let p = rng.point3(-2.0, 2.0);
            json!({"op": "point_visible", "p": p})
        }
    }
}

/// Two closed wire loops: the B-rep's (z = 0) and an extra one whose projection along
/// +Z crosses it in front of or behind it, overlaps a side, or has a vertex over a
/// side's interior.
fn gen_crossing(rng: &mut Rng) -> Value {
    let s = *rng.choose(&[1e-4, 1.0, 1.0, 1e4]).unwrap_or(&1.0);
    let sc = |p: [f64; 3]| [p[0] * s, p[1] * s, p[2] * s];
    let base: Vec<[f64; 3]> = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]]
        .into_iter()
        .map(sc)
        .collect();
    let variant = rng.below(3);
    let mut z = || *rng.choose(&[-1.0, -0.5, 0.5, 1.0, 2.0]).unwrap_or(&1.0);
    let other: Vec<[f64; 3]> = match variant {
        0 => [[1.0, 1.0, z()], [3.0, 1.0, z()], [3.0, 3.0, z()], [1.0, 3.0, z()]].to_vec(),
        1 => {
            let h = z();
            [[1.0, 0.0, h], [3.0, 0.0, h], [3.0, 2.0, h], [1.0, 2.0, h]].to_vec()
        }
        _ => [[1.5, 0.5, z()], [3.0, 0.5, z()], [3.0, 3.0, z()], [1.5, 3.0, z()]].to_vec(),
    }
    .into_iter()
    .map(sc)
    .collect();
    let cubic = rng.chance(0.3);
    let brep = json!({"kind": "wire", "curve": loop_curve(&base, cubic), "t0": 0.0, "t1": 1.0});
    let extra = json!({"curve": loop_curve(&other, rng.chance(0.3)), "t0": 0.0, "t1": 1.0});
    let view = if rng.chance(0.8) {
        json!([0.0, 0.0, 1.0])
    } else {
        json!([rng.uniform(-0.05, 0.05), rng.uniform(-0.05, 0.05), 1.0])
    };
    let sil: Vec<i64> = match rng.below(3) {
        0 => vec![-1],
        1 => vec![0],
        _ => vec![1],
    };
    let edge = rng.below(2) as i64;
    let mut v = json!({
        "brep": brep, "extra": extra, "view": view,
        "approx_tol": *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-3), "ang_tol": 0.2,
        "seams": true, "smooth": true, "sil": sil, "back": [],
    });
    merge(&mut v, &gen_op(rng, edge, true));
    v
}

fn merge(v: &mut Value, extra: &Value) {
    if let (Some(o), Some(map)) = (extra.as_object(), v.as_object_mut()) {
        for (key, val) in o {
            map.insert(key.clone(), val.clone());
        }
    }
}

fn gen_case(rng: &mut Rng) -> Value {
    if rng.chance(0.45) {
        return gen_crossing(rng);
    }
    let scale = *rng.choose(&[1e-4, 1.0, 1.0, 1.0, 1e4]).unwrap_or(&1.0);
    let mut brep = if rng.chance(0.15) {
        periodic_face(1.0)
    } else {
        json!(gen_brep(rng))
    };
    if brep.get("kind").and_then(Value::as_str) != Some("file") {
        scale_brep(&mut brep, scale);
    }
    let (view, placement) = gen_view(rng);
    let sil: Vec<i64> = match rng.below(4) {
        0 => vec![],
        1 => vec![-1],
        _ => (0..rng.below(4)).map(|_| rng.below(1 << 20) as i64).collect(),
    };
    let back: Vec<i64> = (0..rng.below(3)).map(|_| rng.below(1 << 20) as i64).collect();
    let edge = rng.below(1 << 20) as i64;
    let mut v = json!({
        "brep": brep,
        "view": view,
        "approx_tol": *rng.choose(&[1e-6, 1e-3, 0.1]).unwrap_or(&1e-3),
        "ang_tol": *rng.choose(&[0.01, 0.2]).unwrap_or(&0.2),
        "seams": rng.chance(0.7),
        "smooth": rng.chance(0.7),
        "sil": sil,
        "back": back,
    });
    if let (Some(p), Some(map)) = (placement, v.as_object_mut()) {
        map.insert("placement".into(), p);
    }
    merge(&mut v, &gen_op(rng, edge, false));
    v
}

#[cfg(test)]
mod ub170_tests {
    use super::*;

    #[test]
    fn native_guard_is_rejected_without_hiding_operation_results() {
        assert!(native_output(-2.0, vec![]).rejected.is_some());
        for (status, record) in [(0.0, vec![]), (-2.0, vec![1.0]), (3.0, vec![])] {
            let o = native_output(status, record.clone());
            assert!(o.rejected.is_none());
            assert_eq!(o.values.get("status"), Some(&vec![status]));
            assert_eq!(o.values.get("record"), Some(&record));
        }
    }

    #[test]
    fn guarded_ub170_cases_complete_independently_in_rust() {
        // These unchanged generated cases formerly returned UB-170 InvalidInput.
        // Native rejection is checked separately; no native output is supplied
        // to the Rust scene builder or operation, and these are not parity passes.
        let expected = [
            (14, 35),
            (62, 24),
            (114, 18),
            (128, 18),
            (163, 18),
            (175, 18),
            (183, 18),
            (197, 24),
            (237, 24),
            (251, 18),
            (256, 18),
            (279, 18),
            (289, 24),
            (294, 18),
            (306, 18),
            (318, 24),
            (367, 18),
        ];
        for (index, record_len) in expected {
            let seed = CaseContext::seed_for("blend.hcr", index);
            let mut rng = Rng::new(seed);
            let case = Case {
                unit: "blend.hcr".into(),
                name: format!("gen{index:04}"),
                source: crate::case::CaseSource::Generated { seed },
                inputs: Runner.generate(&mut rng).unwrap(),
            };
            let cpp = Runner.run_cpp(&case).unwrap();
            assert!(cpp.rejected.is_some(), "{}: {cpp:?}", case.name);
            let rust = Runner.run_rust(&case).unwrap();
            assert!(rust.error.is_none(), "{}: {rust:?}", case.name);
            assert!(rust.deferred.is_none(), "{}: {rust:?}", case.name);
            assert_eq!(rust.values.get("status"), Some(&vec![0.0]), "{}", case.name);
            let record = rust.values.get("record").unwrap();
            assert_eq!(record.len(), record_len, "{}", case.name);
            assert!(record.iter().all(|v| v.is_finite()), "{}", case.name);
        }
    }
}
