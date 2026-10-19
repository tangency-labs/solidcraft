//! Runner for unit `blend.hcr`. C++: `solidcore_ffi::units::blend_hcr`. Rust:
//! `solidcore::blend::hcr`.
//!
//! The C++ side builds a real `SmBrep` (the B-rep blocks of unit `topology.vertexuse`:
//! upstream test files, a wire edge, a face from a B-spline or plane surface, the wire
//! network), optionally adds a second wire edge, constructs an `SmHCR` on it from a view
//! vector or a view placement, seeds silhouette edges (the silhouette branch of
//! `ComputeSilhouettes`' edge loop) and back-facing edges, and runs one member. It
//! exports the scene B-rep (`GetSceneBrep()`, the copy transformed by the unported
//! `SmBrep` copy constructor and `Transform`) with its `CalculateBoundingBox` (unit
//! `query.brep`, unported); the Rust side imports that scene record by record (the
//! `topology.vertexuse` importer), builds its HCR with [`Hcr::from_scene`], seeds the same
//! edges and runs the same member. Both sides first build the scene (shim op 8) and check
//! the seeds and op arguments against it with the same `ffi::validate`.
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
//! shim does not run it and the port returns `InvalidInput`. Ops whose edge curves (or,
//! for op 5, face surfaces) the importer cannot hold exactly (an `SmEllipse` edge curve
//! is imported as its B-spline part) report `inexact_geometry` on both sides, decided
//! from the same export.
//!
//! The importer leaves the surfaces' owners unset; the runner sets each face surface's
//! owner to its face, as the upstream B-rep build does, so `SmBrep::GetSurfaces`
//! (`IsPointVisible`) selects the same surfaces on both sides.
//!
//! `ComputeSilhouettes` and `ComputeGlobalVisibility` are not run: their C++ path reaches
//! UB-170 on open silhouette chains.

use super::topology_vertexuse::{gen_brep, import, path_of, push_curve, push_surface, sel, BrepIn, CrvIn};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::blend::hcr::{new_curve_classification, view_transform_for_vector, Hcr, HcrObject, HcrOptions};
use solidcore::curve::curve::p2::{BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::PointClassification;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::{edge, Brep, EdgeId, FaceId, VertexId};
use solidcore::SolidError;
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
    if n_e == 0 && !matches!(i.op, OpIn::Construct | OpIn::SilhouetteEdges | OpIn::PointVisible { .. }) {
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

/// What both sides compute before running: the scene (shim op 8), the resolved
/// selectors and the op block, validated against the scene.
struct Prepared {
    path: String,
    brep: Vec<f64>,
    hcr: Vec<f64>,
    scene: Vec<f64>,
    r: Resolved,
    op: u8,
    ops: Vec<f64>,
}

/// [`Prepared`], or the label of a rejection both sides report identically.
fn prepare(i: &Inputs) -> Result<Result<Prepared, String>, HarnessError> {
    let path = path_of(&i.brep)?;
    let brep = brep_block(&i.brep)?;
    let hcr = hcr_block(i)?;
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
        scene,
        r,
        op,
        ops,
    }))
}

fn exact_curve(types: &[f64], e: i64) -> bool {
    types
        .get(e as usize)
        .is_some_and(|&t| t == BSPLINE_CURVE_TYPE as f64 || t == LINE_TYPE as f64)
}

/// Whether the op depends on geometry the import does not hold exactly.
fn inexact(r: &Resolved, types: &[f64], exact_faces: &[bool]) -> bool {
    let curves = |e: i64| !exact_curve(types, e) || r.sil.iter().any(|&s| !exact_curve(types, s));
    match r.op {
        OpIn::Intersect { edge, .. } | OpIn::CurveVisibility { edge, .. } => curves(edge),
        OpIn::EdgeVisibility { edge, .. } => curves(edge) || exact_faces.iter().any(|x| !x),
        _ => false,
    }
}

fn output(status: f64, record: Vec<f64>) -> Output {
    Output::new().values("status", vec![status]).values("record", record)
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
        let s = ffi::split(&p.scene).ok_or_else(|| harness("short scene output"))?;
        let (_, exact_faces) = import(s.export)?;
        if inexact(&p.r, s.curve_types, &exact_faces) {
            return Ok(Output::err("inexact_geometry"));
        }
        let v = match ffi::eval(p.op, &p.path, &p.brep, &p.hcr, &p.ops) {
            Ok(v) => v,
            Err(e) => return Ok(Output::err(format!("rejected: {e}"))),
        };
        let o = ffi::split(&v).ok_or_else(|| harness("short output"))?;
        Ok(output(o.status, o.record.to_vec()))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let p = match prepare(&i)? {
            Ok(p) => p,
            Err(msg) => return Ok(Output::err(msg)),
        };
        let s = ffi::split(&p.scene).ok_or_else(|| harness("short scene output"))?;
        let (mut scene, exact_faces) = import(s.export)?;
        if inexact(&p.r, s.curve_types, &exact_faces) {
            return Ok(Output::err("inexact_geometry"));
        }
        // the surfaces' owners (`SmSurface::GetOwner`, set by the B-rep build upstream and
        // read by `SmBrep::GetSurfaces`): the kernel's arena identities
        let face_ids: Vec<FaceId> = scene.faces().ids().collect();
        for id in face_ids {
            if let Some(surface) = scene
                .face_mut(id)
                .map_err(|e| harness(e.to_string()))?
                .surface
                .as_mut()
            {
                let _ = surface.set_owner(Some(arena_identity(CacheObject::Face(id))));
            }
        }
        let [x0, y0, z0, x1, y1, z1] = s.scene_box;
        let (lo, hi) = (Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1));
        let scene_box = if lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z {
            Extent3d::from_points(lo, hi)
        } else if Extent3d::new().min() == lo && Extent3d::new().max() == hi {
            Extent3d::new()
        } else {
            return Err(harness("scene box neither ordered nor empty"));
        };
        match run_rust(&i, &p.r, scene, scene_box) {
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

fn put_piece(o: &mut Vec<f64>, c: &Curve) {
    let n = c.natural_interval();
    o.extend([n.min(), n.max()]);
    for i in 0..5 {
        match c.evaluate_point(n.evaluate(f64::from(i) / 4.0)) {
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

fn run_rust(i: &Inputs, r: &Resolved, scene: Brep, scene_box: Extent3d) -> Result<(f64, Vec<f64>), SolidError> {
    let k = Kernel::default();
    let transform = match i.placement {
        None => view_transform_for_vector(Vec3::new(i.view[0], i.view[1], i.view[2]))?,
        Some([z, x]) => {
            let mut a = Axis2Placement::default();
            let _ = a.set_step_canonical(
                Vec3::new(i.view[0], i.view[1], i.view[2]),
                Vec3::new(z[0], z[1], z[2]),
                Vec3::new(x[0], x[1], x[2]),
            );
            a
        }
    };
    let opts = HcrOptions {
        approx_tol_3d: i.approx_tol,
        ang_tol_rad: i.ang_tol,
        display_seams: i.seams,
        display_smooth_edges: i.smooth,
    };
    let mut h = Hcr::from_scene(&k, scene, &transform, scene_box, opts);
    for &e in &r.sil {
        h.add_silhouette_edge(&k, eid(e))?;
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
        OpIn::IsSilhouette { edge } => rec.push(flag(h.is_silhouette_edge(&k, eid(edge))?)),
        OpIn::IsBackFacing { edge } => rec.push(flag(h.is_back_facing_edge(&k, eid(edge))?)),
        OpIn::Intersect { edge, fa, fb } | OpIn::CurveVisibility { edge, fa, fb, .. } => {
            let e = eid(edge);
            let ivl = edge::interval(h.scene_brep(), e)?;
            let ivl = Extent1d::from_min_max(ivl.evaluate(fa), ivl.evaluate(fb));
            let tol = edge::tolerance(h.scene_brep(), e)?;
            let mut cc = new_curve_classification(&k, HcrObject::Curve(e), ivl, tol);
            let res = match r.op {
                OpIn::CurveVisibility { start_hint, end_hint, .. } => {
                    h.compute_curve_visibility(&k, &mut cc, hint_ulong(start_hint), hint_ulong(end_hint))
                }
                _ => h.intersect_curve_with_silhouettes(&k, &mut cc),
            };
            match res {
                Ok(()) => {}
                Err(e) if e.is_unported() || matches!(e, SolidError::InvalidInput { .. }) => return Err(e),
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
            match h.compute_edge_visibility(&k, eid(edge), v, visible, &mut marked) {
                Ok(()) => {}
                Err(SolidError::Upstream { code, .. }) => status = f64::from(code),
                Err(e) => return Err(e),
            }
            rec.push(h.visible_curves().len() as f64);
            for c in h.visible_curves() {
                put_piece(&mut rec, c);
            }
            rec.push(h.invisible_curves().len() as f64);
            for c in h.invisible_curves() {
                put_piece(&mut rec, c);
            }
        }
        OpIn::SilhouetteEdges => {
            let edges = h.silhouette_edges();
            rec.push(edges.len() as f64);
            rec.extend(edges.iter().map(|e| f64::from(e.index())));
        }
        OpIn::PointVisible { p } => rec.push(flag(h.is_point_visible(&k, Vec3::new(p[0], p[1], p[2]))?)),
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
