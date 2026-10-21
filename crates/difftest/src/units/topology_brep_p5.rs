//! Differential coverage for `topology.brep_p5`.
//!
//! `combine_target` compares only the old-tol target stage against the surviving
//! vertex after real native `CombineCoincidentVertices`. Other operations compare
//! status/output lifecycle and the semantic graph or explicitly defer. Successful
//! topology mutation and nonidentity transformation are not certified here.
//! Inputs the FFI guards refuse (undefined natively) are `rejected`, not failed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::context::{MarkLock, MarkType, ALL_MARKS};
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p5;
use solidcore::topology::{Brep, EdgeId, FaceId, LoopId, RegionId, VertexId};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::topology_brep_p5::{self as ffi, Op};
use solidcore_ffi::FfiError;

use super::boolean_merge_p1::graph;
use super::topology_vertexuse::import;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// Part 5's runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Independent native optional flags.
#[serde(default)]
struct Inputs {
    // Raw fixed-size fixture, described in the FFI module.
    fixture: [f64; 11],
    op: u8,
    editing: bool,
    first: i64,
    second: i64,
    aux: i64,
    mode: u8,
    tolerance: f64,
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    scale: Option<[f64; 3]>,
    scale_tolerances: bool,
    flag: bool,
    allow_spine_edges: bool,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            fixture: [0., 0., 0., 0., 0.001, 0., 0., 1e-5, 2e-5, 0., 0.],
            op: 12,
            editing: false,
            first: 0,
            second: 1,
            aux: 0,
            mode: 0,
            tolerance: -0.1,
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [0., 1., 0.],
            scale: None,
            scale_tolerances: false,
            flag: false,
            allow_spine_edges: false,
        }
    }
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn f(b: bool) -> f64 {
    f64::from(u8::from(b))
}
fn status(r: SolidResult<()>) -> SolidResult<f64> {
    match r {
        Ok(()) => Ok(0.),
        Err(e) if e.is_unported() => Err(e),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
        Err(SolidError::InvalidInput { .. }) => Ok(1007.),
        Err(e) => Err(e),
    }
}
fn pick(i: i64, n: usize) -> Option<u32> {
    (i >= 0 && n != 0).then(|| (i as usize % n) as u32)
}
fn vertex(b: &Brep, i: i64) -> Option<VertexId> {
    pick(i, b.vertices().len()).map(VertexId::from_index)
}
fn edge(b: &Brep, i: i64) -> Option<EdgeId> {
    pick(i, b.edges().len()).map(EdgeId::from_index)
}
fn face(b: &Brep, i: i64) -> Option<FaceId> {
    pick(i, b.faces().len()).map(FaceId::from_index)
}
fn curve() -> Curve {
    Curve::from(Line::bounded(
        Vec3::ZERO,
        Vec3::X,
        &Extent1d::from_min_max(-2., 3.),
        &BoundedLineOptions::default(),
    ))
}
fn args(i: &Inputs) -> [f64; 22] {
    let s = i.scale.unwrap_or([1.; 3]);
    [
        f(i.editing),
        i.first as f64,
        i.second as f64,
        i.aux as f64,
        f64::from(i.mode),
        i.tolerance,
        i.origin[0],
        i.origin[1],
        i.origin[2],
        i.x[0],
        i.x[1],
        i.x[2],
        i.y[0],
        i.y[1],
        i.y[2],
        f(i.scale.is_some()),
        s[0],
        s[1],
        s[2],
        f(i.scale_tolerances),
        f(i.flag),
        f(i.allow_spine_edges),
    ]
}
fn op(i: &Inputs) -> Result<Op, HarnessError> {
    Op::from_code(i.op)
        .filter(|op| *op != Op::Export)
        .ok_or_else(|| HarnessError::new("unknown op"))
}
fn refused_fixture(raw: &[f64]) -> Option<Output> {
    (raw.first() == Some(&-999.)).then(|| {
        Output::rejected(format!(
            "fixture constructor status {}",
            raw.get(1).copied().unwrap_or(-1.)
        ))
    })
}
/// The native record, or `Err(output)` when the FFI guard refuses an input that is
/// undefined natively. Nothing runs on the native side, so the case is rejected
/// (as `topology.brep_p1`'s runner does), neither passed nor failed.
fn native(i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    match ffi::eval(op(i)?, &i.fixture, &args(i)) {
        Ok(raw) => Ok(Ok(raw)),
        Err(e @ FfiError::InvalidArgument(_)) => {
            Ok(Err(Output::rejected(format!("ffi guard: {e}"))))
        }
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

fn rust_record(k: &mut Kernel, b: &mut Brep, i: &Inputs) -> SolidResult<Vec<f64>> {
    let v1 = vertex(b, i.first);
    let v2 = vertex(b, i.second);
    let e = edge(b, i.aux);
    let face = face(b, i.first);
    let null = || SolidError::null_pointer("difftest::topology_brep_p5");
    let r = match i.op {
        1 => {
            let mut deleted = e;
            let st = status(p5::combine_coincident_vertices(
                k,
                b,
                v1.ok_or_else(null)?,
                v2.ok_or_else(null)?,
                &mut deleted,
            ))?;
            vec![st, f(deleted.is_some())]
        }
        2 => {
            let t = p5::coincident_vertex_target(b, v1.ok_or_else(null)?, v2.ok_or_else(null)?)?;
            vec![0., t.point.x, t.point.y, t.point.z, t.tolerance]
        }
        3 => vec![status(p5::delete_topological_vertex(
            k, b, i.editing, v1, e,
        ))?],
        4 => vec![status(p5::find_and_set_infinite_region(
            b,
            None,
            i32::from(i.mode),
            None,
        ))?],
        5 => vec![status(p5::glue_vertices(k, b, v1, v2, None))?],
        6 => {
            let old = p5::EdgeInFace {
                edge: e,
                loop_: pick(i.second, b.loops().len()).map(LoopId::from_index),
                face,
            };
            let mut out = old;
            let st = status(p5::make_edge_in_face(
                b,
                i.editing,
                face,
                v1,
                v2,
                curve(),
                None,
                Extent1d::from_min_max(0., 1.),
                OrientType::Same,
                0.,
                &mut out,
            ))?;
            vec![
                st,
                f(out.edge == old.edge),
                f(out.loop_ == old.loop_),
                f(out.face == old.face),
            ]
        }
        7 => {
            let regions: Vec<RegionId> = if i.mode == 2 {
                b.regions().ids().collect()
            } else {
                Vec::new()
            };
            vec![status(p5::make_manifold(
                k,
                b,
                if i.mode == 0 { None } else { Some(&regions) },
                i.flag,
            ))?]
        }
        8 => vec![status(p5::remove_topological_edges_and_vertices(
            k, b, None, None,
        ))?],
        9 => {
            let faces: Vec<_> = if i.mode != 0 {
                face.into_iter().collect()
            } else {
                Vec::new()
            };
            let mut outer = if i.flag {
                Some(MarkLock::new(
                    core::slice::from_mut(&mut k.context),
                    0,
                    ALL_MARKS,
                )?)
            } else {
                None
            };
            let marks = [
                MarkType::Mark,
                MarkType::Mark2,
                MarkType::Mark3,
                MarkType::MarkIo,
                MarkType::MarkAssert,
            ];
            let before = marks.map(|m| k.context.current_mark(m));
            let result = p5::remove_faces(k, b, i.editing, &faces);
            let mut changes = Vec::new();
            for (mark, old) in marks.into_iter().zip(before) {
                changes.extend([
                    k.context.current_mark(mark).wrapping_sub(old) as f64,
                    f(k.context.is_mark_locked(mark)),
                ]);
            }
            if let Some(outer) = outer.as_mut() {
                outer.release(core::slice::from_mut(&mut k.context))?;
            }
            let mut out = vec![status(result)?];
            out.extend(changes);
            out
        }
        10 => vec![status(p5::stitch_and_orient(
            k,
            b,
            &p5::StitchAndOrientOptions {
                stitch_tolerance: i.tolerance,
                allow_spine_edges: i.allow_spine_edges,
                faces_to_swap: None,
                just_unify: (i.mode != 0).then_some(i.flag),
            },
        ))?],
        11 => {
            let mut out = p5::SqueezedFace {
                deleted_vertices: v1.into_iter().collect(),
                new_edges: e.into_iter().collect(),
                deleted_edges: e.into_iter().collect(),
            };
            let st = status(p5::squeeze_face(
                k,
                b,
                i.editing,
                face.unwrap_or(FaceId::from_index(0)),
                (i.mode != 0).then_some(i.tolerance),
                &mut out,
            ))?;
            vec![
                st,
                out.deleted_vertices.len() as f64,
                out.new_edges.len() as f64,
                out.deleted_edges.len() as f64,
            ]
        }
        12 => vec![status(p5::transform(
            k,
            b,
            &Axis2Placement::new(v(i.origin), v(i.x), v(i.y)),
            i.scale.map(v),
            i.scale_tolerances,
        ))?],
        13 => vec![status(
            p5::create_wire_edge_from_curve(k, b, curve(), Extent1d::from_min_max(-2., 3.))
                .map(|_| ()),
        )?],
        _ => {
            return Err(SolidError::invalid_input(
                "difftest::topology_brep_p5",
                "op",
            ))
        }
    };
    Ok(r)
}

fn notification_record(b: &Brep, i: &Inputs) -> SolidResult<Vec<f64>> {
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::geom::context::NotifyCallback;
    use solidcore::geom::object::{
        NotifyOperation::{
            AddToBrep, ChangeGeometry, ChangeOwner, Coincident, Construction, Copy, Destruction,
            Merge, MergeInBrep, PostEdit, PreEdit, RegPropagation, RmFromBrep, Split, SplitInBrep,
            TrimNoSplitInBrep, Unknown,
        },
        ObjectRef,
    };
    use solidcore::topology::topology::TopologyChangeCallback;
    let object = |type_code, key| ObjectRef {
        type_code,
        key: ObjectKey(key),
    };
    let a = vertex(b, i.first).ok_or_else(|| SolidError::null_pointer("notification fixture"))?;
    let z = vertex(b, i.second).ok_or_else(|| SolidError::null_pointer("notification fixture"))?;
    let v1 = object(16021, 1_000_000 + u64::from(a.index()));
    let v2 = object(16021, 1_000_000 + u64::from(z.index()));
    let brep = object(16011, 0);
    if i.flag {
        use solidcore::topology::{topology::TrackTopologyChanges, BrepId};
        let mut context = solidcore::geom::context::Context::default();
        if i.mode == 1 {
            context.set_sys_notify_callback(Some(Box::new(TopologyChangeCallback::new(2))));
        }
        // The fixture contains one B-rep at traversal index zero.
        let mut tracker =
            TrackTopologyChanges::new(Some(BrepId::from_index(0)), 1, true, Some(&mut context));
        context
            .sys_notify_callback_mut()
            .ok_or_else(|| SolidError::sm_err("notification tracker"))?
            .execute(PostEdit, v1, Some(v1), Some(brep), None)?;
        let mut out = notification_values(&tracker.lists());
        tracker.reset_arrays();
        out.extend(notification_values(&tracker.lists()));
        context
            .sys_notify_callback_mut()
            .ok_or_else(|| SolidError::sm_err("notification tracker"))?
            .execute(PostEdit, v1, Some(v1), Some(brep), None)?;
        if i.mode != 1 {
            context.set_sys_notify_callback(Some(Box::new(TopologyChangeCallback::new(2))));
        }
        tracker.stop_tracking(Some(&mut context));
        out.extend(notification_values(&tracker.lists()));
        out.push(f(context.sys_notify_callback().is_none()));
        context.set_sys_notify_callback(None);
        return Ok(out);
    }
    let mut owners = std::collections::BTreeMap::new();
    let mut callback = TopologyChangeCallback::new(1);
    let mut send = |op, caller, x, y, z| callback.execute(op, caller, x, y, z);
    if i.mode == 0 {
        send(AddToBrep, brep, Some(v1), Some(brep), None)?;
        send(AddToBrep, brep, Some(v1), Some(brep), None)?;
        send(ChangeGeometry, v1, None, Some(brep), None)?;
        send(MergeInBrep, brep, Some(v1), Some(v2), Some(brep))?;
        send(PostEdit, v1, Some(v1), Some(brep), None)?;
        send(Destruction, v1, Some(v1), None, None)?;
        send(RegPropagation, brep, None, None, None)?;
        send(Unknown, brep, None, None, None)?;
    } else if i.mode == 1 {
        send(SplitInBrep, brep, Some(v1), Some(v2), None)?;
        send(TrimNoSplitInBrep, brep, Some(v2), Some(v1), None)?;
        send(RmFromBrep, brep, Some(v2), Some(brep), None)?;
        send(AddToBrep, brep, Some(v1), Some(brep), None)?;
        send(Copy, v1, Some(v2), Some(brep), Some(brep))?;
        send(Coincident, brep, Some(v1), Some(v2), Some(brep))?;
        send(PreEdit, v1, Some(v1), Some(brep), None)?;
        send(ChangeOwner, v1, None, None, None)?;
        send(Construction, v1, Some(v1), None, None)?;
        send(Destruction, v2, Some(v2), None, None)?;
    } else {
        let e = edge(b, i.aux).ok_or_else(|| SolidError::null_pointer("notification edge"))?;
        let edge_ref = object(16016, 2_000_000 + u64::from(e.index()));
        let curve = b
            .edge(e)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer("notification curve"))?;
        let curve_ref = object(curve.type_code(), 3_000_000);
        // The native fixture constructs this curve owned by this actual edge.
        owners.insert(curve_ref.key, Some(edge_ref));
        callback = TopologyChangeCallback::with_owners(1, owners);
        callback.execute(PostEdit, curve_ref, Some(curve_ref), Some(edge_ref), None)?;
        callback.execute(Split, curve_ref, Some(curve_ref), None, Some(edge_ref))?;
        callback.execute(
            Merge,
            curve_ref,
            Some(curve_ref),
            Some(curve_ref),
            Some(edge_ref),
        )?;
        if let Some(face) = face(b, i.first) {
            let face_ref = object(16002, 4_000_000 + u64::from(face.index()));
            callback.execute(PostEdit, face_ref, Some(face_ref), Some(brep), None)?;
        }
    }
    Ok(notification_values(&callback.lists()))
}

fn notification_values(lists: &solidcore::topology::topology::TopologyChangeLists) -> Vec<f64> {
    use solidcore::geom::object::ObjectRef;
    let mut out = Vec::new();
    for list in [&lists.rm_list, &lists.add_list, &lists.chg_list] {
        out.push(list.len() as f64);
        out.extend(list.iter().map(|key| key.0 as f64));
    }
    out.push(lists.notify_events.len() as f64);
    let key = |object: Option<ObjectRef>| object.map_or(-1., |o| o.key.0 as f64);
    for event in &lists.notify_events {
        out.extend([
            f64::from(event.operation as i32),
            key(event.caller),
            key(event.data1),
            key(event.data2),
            key(event.data3),
        ]);
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p5"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = match native(&i)? {
            Ok(raw) => raw,
            Err(refused) => return Ok(refused),
        };
        if let Some(out) = refused_fixture(&raw) {
            return Ok(out);
        }
        if matches!(i.op, 2 | 15) {
            return Ok(Output::new().values(
                if i.op == 2 {
                    "target_stage"
                } else {
                    "notification_stage"
                },
                raw,
            ));
        }
        let n = *raw
            .first()
            .ok_or_else(|| HarnessError::new("short record"))? as usize;
        let record = raw
            .get(1..=n)
            .ok_or_else(|| HarnessError::new("short record"))?
            .to_vec();
        let (b, _) = import(
            raw.get(n + 1..)
                .ok_or_else(|| HarnessError::new("short graph"))?,
        )?;
        Ok(graph(&b, record))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let raw = ffi::eval(Op::Export, &i.fixture, &args(&i))
            .map_err(|e| HarnessError::new(e.to_string()))?;
        let (mut b, _) = import(&raw)?;
        if i.op == 15 {
            return Ok(Output::new().values(
                "notification_stage",
                notification_record(&b, &i).map_err(|e| HarnessError::new(e.to_string()))?,
            ));
        }
        match rust_record(&mut Kernel::default(), &mut b, &i) {
            Ok(record) if i.op == 2 => Ok(Output::new().values("target_stage", record)),
            Ok(record) => Ok(graph(&b, record)),
            Err(e) => Output::unported(&e).ok_or_else(|| HarnessError::new(e.to_string())),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = 10f64.powi(rng.range_i64(-6, 6) as i32);
        let mut i = Inputs {
            fixture: [
                0.,
                rng.uniform(-10., 10.) * scale,
                rng.uniform(-10., 10.) * scale,
                0.,
                rng.uniform(-10., 10.) * scale,
                rng.uniform(-10., 10.) * scale,
                0.,
                rng.uniform(0., 0.1) * scale,
                rng.uniform(0., 0.1) * scale,
                0.,
                0.,
            ],
            ..Inputs::default()
        };
        i.op = match rng.below(8) {
            0..=3 => 2,
            4 => 12,
            5 => 3,
            6 => 9,
            _ => 11,
        };
        if i.op == 2 {
            i.editing = true;
            let side = rng.below(2) as usize;
            if let Some(count) = i.fixture.get_mut(9 + side) {
                *count = rng.below(5) as f64;
            }
            if rng.chance(0.5) {
                i.first = 1;
                i.second = 0;
            }
        } else if i.op == 12 {
            i.fixture[0] = if rng.chance(0.5) { 1. } else { 2. };
            if rng.chance(0.25) {
                i.origin = [1., 2., 3.];
            }
            if rng.chance(0.5) {
                i.scale = Some([1.; 3]);
            }
            i.scale_tolerances = rng.chance(0.5);
        }
        serde_json::to_value(i).ok()
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(512);
        crate::runner::default_cases(self, &ctx)
    }
}
