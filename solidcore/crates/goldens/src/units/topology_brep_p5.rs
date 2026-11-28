//! Runner for unit `topology.brep_p5`: `CombineCoincidentVertices`, the coincident vertex
//! target, `DeleteTopologicalVertex`, `FindAndSetInfiniteRegion`, `GlueVertices`,
//! `MakeEdgeInFace`, `MakeManifold`, `RemoveTopologicalEdgesAndVertices`, `RemoveFaces`,
//! `StitchAndOrient`, `SqueezeFace`, `Transform`, `CreateWireEdgeFromCurve` and the topology
//! change notifications. Ported from upstream `SmBrep` (`SmBrep.cpp`); checked against its
//! golden.
//!
//! The case builds its fixture itself ([`own_fixture`], the upstream test fixture's
//! `p5_build` with the Rust kernel). Each op records its status and outputs, then the
//! semantic graph of the B-rep reachable after it (`combine_target` records only the
//! target stage, the notifications their lists).

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
use solidcore::topology::brep::p1::SetRegionVoidType;
use solidcore::topology::brep::p5;
use solidcore::topology::{Brep, EdgeId, FaceId, LoopId, RegionId, VertexId};
use solidcore::{Result as SolidResult, SolidError};

use super::boolean_merge_p1::graph_owned;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use solidcore::math::det;

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
        Err(e) if solidcore::error::upstream_status(&e).is_some() => {
            Ok(f64::from(solidcore::error::upstream_status(&e).unwrap_or(-1)))
        }
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(1007.),
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

/// The op's record. In test builds `log` is the case's certificate log (see
/// [`transform_certificate::Fired`]), which the transform (op 12) counts into.
fn rust_record(
    k: &mut Kernel,
    b: &mut Brep,
    i: &Inputs,
    #[cfg(test)] log: &mut transform_certificate::Fired,
) -> SolidResult<Vec<f64>> {
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
        4 => {
            // the shim casts the mode to SmSetRegionVoidType; only its three values exist
            let void_setting = match i.mode {
                0 => SetRegionVoidType::NestedSolid,
                1 => SetRegionVoidType::InfiniteRegionOnly,
                2 => SetRegionVoidType::InfiniteRegionSwap,
                _ => {
                    return Err(SolidError::invalid_input(
                        "difftest::topology_brep_p5",
                        "void setting outside SmSetRegionVoidType",
                    ))
                }
            };
            vec![status(p5::find_and_set_infinite_region(
                k,
                b,
                &solidcore::query::brep::BrepQuery::default(),
                None,
                void_setting,
                None,
            ))?]
        }
        5 => vec![status(p5::glue_vertices(k, b, v1, v2, None))?],
        6 => {
            let old = p5::EdgeInFace {
                edge: e,
                loop_: pick(i.second, b.loops().len()).map(LoopId::from_index),
                face,
            };
            let mut out = old;
            let st = status(p5::make_edge_in_face(
                k,
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
                None,
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
        12 => {
            #[cfg(test)]
            let before = transform_certificate::Snapshot::before_transform(k, b, i.scale.map(v))
                .expect("certificate snapshot");
            let r = p5::transform(
                k,
                b,
                &Axis2Placement::new(v(i.origin), v(i.x), v(i.y)),
                i.scale.map(v),
                i.scale_tolerances,
            );
            #[cfg(test)]
            if r.is_ok() {
                let s = i.scale.map(v);
                before
                    .check(k, b, s, i.scale_tolerances, "brep_p5 op 12", log)
                    .expect("certificate check");
            }
            vec![status(r)?]
        }
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
        // StopTracking records the dependencies of the completed edit (1.0.1
        // `RecordDependencies`), under the shim's keys: vertex, edge and face
        // `1/2/4_000_000 + index`
        let entity_of = |key: ObjectKey| {
            let i = u32::try_from(key.0 % 1_000_000).ok()?;
            match key.0 / 1_000_000 {
                1 => Some(solidcore::EntityRef::Vertex(VertexId::from_index(i))),
                2 => Some(solidcore::EntityRef::Edge(EdgeId::from_index(i))),
                4 => Some(solidcore::EntityRef::Face(FaceId::from_index(i))),
                _ => None,
            }
        };
        let key_of = |e: solidcore::EntityRef| {
            ObjectKey(match e {
                solidcore::EntityRef::Vertex(v) => 1_000_000 + u64::from(v.index()),
                solidcore::EntityRef::Edge(x) => 2_000_000 + u64::from(x.index()),
                solidcore::EntityRef::Face(x) => 4_000_000 + u64::from(x.index()),
                _ => 999,
            })
        };
        tracker.stop_tracking_with_brep(Some(&mut context), b, &entity_of, &key_of)?;
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

// The native fixture's geometry retains its actual edge/face owners. The Rust
// build owns it under the fixture identities; a plain kernel edit notifies under
// kernel-issued topology identities. Bind owned geometry into that same context
// before transforming it.
fn bind_transform_owners(k: &mut Kernel, b: &mut Brep) -> SolidResult<()> {
    use solidcore::topology::brep::p3::{notify_serial, topology_ref};
    use solidcore::EntityRef;
    let serial = notify_serial(&mut k.context, b)?;
    let edges: Vec<_> = b.edges().ids().collect();
    for edge in edges {
        if let Some(curve) = b.edge_mut(edge)?.curve.as_mut() {
            // Every fixture edge owns the distinct curve constructed for it.
            curve.set_owner(Some(topology_ref(serial, EntityRef::Edge(edge))), None);
        }
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for face in faces {
        if let Some(surface) = b.face_mut(face)?.surface.as_mut() {
            if surface.owner().is_some() {
                surface.set_owner(Some(topology_ref(serial, EntityRef::Face(face))));
            }
        }
    }
    Ok(())
}

/// The shim's `p5_build` with the Rust kernel, renumbered into the shim's traversal
/// order ([`crate::own_builder`], `sc_topology_fixture.h`'s `walk`): kind 1 the
/// `prog_test` wire network, kind 2 the shared fixture's plane face (`SmPlane` at the
/// origin with normal `+z` over `[-2, 4] x [-3, 5]`), kind 0 two shell vertices
/// (`MakeShellVertex`, then `SetTolerance(SmZoneTol3d(tol), FALSE, FALSE)`), each with its count of
/// `MakeWireEdgeVertex` wire edges over `CreateLineSegment` lines to
/// `point + (2 + j, 3 + j^2, 5 + j)`, `m_bEditingEnabled` set. A failed build is
/// rejected with its status, as the C++ side's.
fn own_fixture(f: &[f64; 11]) -> Result<Result<Brep, Output>, HarnessError> {
    use crate::own_builder::{build_status, build_with, finish, in_new_brep, line, Built};
    use crate::own_builder::Conventions;
    use crate::units::topology_edgeuse_p1::{BrepIn, SrfIn};
    let mut k = Kernel::default();
    let conv = &Conventions::TOPOLOGY_FIXTURE;
    let built = match f[0] as i64 {
        1 => build_with(&mut k, &BrepIn::Network, false, false, conv)?,
        2 => build_with(
            &mut k,
            &BrepIn::Face {
                surface: SrfIn::Plane {
                    o: [0.0, 0.0, 0.0],
                    n: [0.0, 0.0, 1.0],
                },
                dom: [-2.0, -3.0, 4.0, 5.0],
            },
            false,
            false,
            conv,
        )?,
        _ => {
            let raw = in_new_brep(&mut k, |k, b| {
                use solidcore::topology::brep::p2::ArenaNotifyState;
                b.set_editing_enabled(true);
                let mut state = ArenaNotifyState::new(b);
                let region = b.infinite_region();
                // (point, raw tolerance, incident edge count) of each vertex
                let stars = [
                    ([f[1], f[2], f[3]], f[7], f[9]),
                    ([f[4], f[5], f[6]], f[8], f[10]),
                ];
                for (point, raw_tol, count) in stars {
                    let (_, v) = {
                        let mut env = state.env(None);
                        solidcore::topology::brep::p9::make_shell_vertex(
                            b,
                            region,
                            Vec3::from_array(point),
                            &mut env,
                        )?
                    };
                    state.seed(b);
                    // `SetTolerance(SmZoneTol3d(raw), FALSE, FALSE)`
                    let tol = solidcore::geom::tol::zone_tol3d(k, raw_tol);
                    solidcore::topology::vertex::set_tolerance(b, v, tol, false, false)?;
                    for j in 0..(count as i64) {
                        let jf = j as f64;
                        let start = b.vertex(v)?.point;
                        let end = start + Vec3::new(2.0 + jf, 3.0 + jf * jf, 5.0 + jf);
                        let c = line(start, end)?;
                        let natural = c.natural_interval();
                        {
                            let mut env = state.env(None);
                            solidcore::topology::brep::p3::make_wire_edge_vertex(
                                k,
                                b,
                                &mut env,
                                region,
                                v,
                                c,
                                &natural,
                                OrientType::Same,
                                end,
                            )?;
                        }
                        state.seed(b);
                    }
                }
                Ok(())
            })?;
            Some(finish(&mut k, raw, false, false, conv)?)
        }
    };
    match built {
        Some(Built::Brep(b)) => Ok(Ok(*b)),
        Some(Built::Failed(e)) => Ok(Err(Output::unported(&e).unwrap_or_else(|| {
            Output::rejected(format!("fixture constructor status {}", build_status(&e)))
        }))),
        None => Err(HarnessError::new("fixture kind without a build")),
    }
}

impl Runner {
    /// [`UnitDiff::run_rust`], with the case's certificate log in test builds.
    fn run(
        case: &Case,
        #[cfg(test)] log: &mut transform_certificate::Fired,
    ) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut b = match own_fixture(&i.fixture)? {
            Ok(b) => b,
            Err(o) => return Ok(o),
        };
        // The shim sets `m_bEditingEnabled` from the editing argument for every op.
        b.set_editing_enabled(i.editing);
        if i.op == 15 {
            return Ok(Output::new().values(
                "notification_stage",
                notification_record(&b, &i).map_err(|e| HarnessError::new(e.to_string()))?,
            ));
        }
        let mut kernel = Kernel::default();
        if i.op == 12 {
            bind_transform_owners(&mut kernel, &mut b)
                .map_err(|e| HarnessError::new(e.to_string()))?;
        }
        match rust_record(&mut kernel, &mut b, &i, #[cfg(test)] log) {
            Ok(record) if i.op == 2 => Ok(Output::new().values("target_stage", record)),
            // a use or shell the edit unlinked stays in its arena (append-only records)
            // where upstream deletes it; [`graph`] compares only the records reachable
            // from the B-rep, as the native export walk emits only those
            Ok(record) => {
                // (the serial plain edits and `bind_transform_owners` name geometry under)
                let serial =
                    solidcore::topology::brep::p3::notify_serial(&mut kernel.context, &mut b)
                        .map_err(|e| HarnessError::new(e.to_string()))?;
                Ok(graph_owned(&b, record, Some(serial)))
            }
            Err(e) => Output::unported(&e).ok_or_else(|| HarnessError::new(e.to_string())),
        }
    }
}

#[cfg(test)]
impl transform_certificate::Certified for Runner {
    fn run_certified(
        &self,
        case: &Case,
        log: &mut transform_certificate::Fired,
    ) -> Result<Output, HarnessError> {
        Self::run(case, log)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p5"
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        Self::run(case, #[cfg(test)] &mut transform_certificate::Fired::default())
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = det::powi(10f64, rng.range_i64(-6, 6) as i32);
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

#[cfg(test)]
mod transform_certificate_tests {
    use super::*;

    /// Every case replays against its blessed golden line, and each `Transform` (op 12)
    /// goes through [`transform_certificate`], `identity_scale_delta_1e-11` (the one row
    /// that moved when the WR-507 / WR-508 flags were retired) among them.
    #[test]
    fn every_transform_meets_the_wr507_wr508_certificate() {
        let fired = transform_certificate::replay_matches_golden(&Runner);
        let f = fired["identity_scale_delta_1e-11"];
        assert!(f.scaled > 0, "{f:?}");
    }
}

/// The WR-507 / WR-508 certificate of `SmBrep::Transform`'s tolerances, checked after each
/// transform a runner makes when the goldens crate's tests run (the binary skips it):
/// - a scale whose largest absolute component `s` is 1 within `SM_EFF_ZERO` (a rotation,
///   reflection or move), or tolerance scaling off, changes no vertex, edge or face
///   tolerance and not the body's model size estimate;
/// - any other scale sets the model size estimate to the length of the scaled body's
///   bounding box diagonal (vertices, edges and faces, not grown by tolerances: WR-508)
///   and each tolerance `t` to `max(zone, t * s)`, `zone` the body tolerance that size
///   gives (WR-507), bit for bit as the closed form computes it.
///
/// Every case of `topology.brep_p5`, `topology.brep_mirror` and
/// `topology.proto_topology_p1` replays through it in those units' tests.
#[cfg(test)]
pub(super) mod transform_certificate {
    use solidcore::kernel::Kernel;
    use solidcore::math::{max2, Vec3, EFF_ZERO};
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    use solidcore::geom::tol::are_same;
    use solidcore::topology::brep::p1;
    use solidcore::topology::brep::p5::turn_to_nurbs_plain;
    use solidcore::topology::{edge, face, vertex, Brep};
    use solidcore::Result;

    /// `max(|x|, |y|, |z|)` of a transform's scale, as `SmBrep::Transform` takes it.
    pub(crate) fn tol_scale(s: Vec3) -> f64 {
        max2(s.x.abs(), max2(s.y.abs(), s.z.abs()))
    }

    /// The tolerances and model size estimate of a body before a transform.
    pub(crate) struct Snapshot {
        vertices: Vec<f64>,
        edges: Vec<f64>,
        faces: Vec<f64>,
        size: f64,
        /// Each vertex's point, bit for bit.
        points: Vec<[u64; 3]>,
    }

    /// The certified checks one case went through: the case's certificate log, owned by
    /// the replay ([`replay_matches_golden`]) and passed down to each checked transform.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Fired {
        /// WR-507 / WR-508: transforms that scaled the tolerances (closed form checked).
        pub(crate) scaled: usize,
        /// Transforms with a scale of 1 or no tolerance scaling (nothing changed).
        pub(crate) unscaled: usize,
        /// Transforms that took `SmBrep::Transform`'s identity exit.
        pub(crate) identity: usize,
    }

    impl Fired {
        fn add(&mut self, other: Self) {
            self.scaled += other.scaled;
            self.unscaled += other.unscaled;
            self.identity += other.identity;
        }
    }

    /// A runner whose transforms the certificate checks: its [`crate::UnitDiff::run_rust`]
    /// with the case's log passed down to every transform.
    pub(crate) trait Certified: crate::UnitDiff {
        /// Runs `case` as `run_rust` does, counting each checked transform into `log`.
        fn run_certified(
            &self,
            case: &crate::Case,
            log: &mut Fired,
        ) -> Result<crate::Output, crate::HarnessError>;
    }

    /// `unit` as the goldens check runs it during one replay, which owns the logs of
    /// every case it ran, by case name.
    struct Replay<'a, U: ?Sized> {
        unit: &'a U,
        logs: std::cell::RefCell<std::collections::BTreeMap<String, Fired>>,
    }

    impl<U: Certified + ?Sized> crate::UnitDiff for Replay<'_, U> {
        fn id(&self) -> &'static str {
            self.unit.id()
        }
        fn run_rust(&self, case: &crate::Case) -> Result<crate::Output, crate::HarnessError> {
            self.logs.borrow_mut().entry(case.name.clone()).or_default();
            let mut log = Fired::default();
            let out = self.unit.run_certified(case, &mut log);
            if let Some(f) = self.logs.borrow_mut().get_mut(&case.name) {
                f.add(log);
            }
            out
        }
        fn generate(&self, rng: &mut crate::Rng) -> Option<serde_json::Value> {
            self.unit.generate(rng)
        }
        fn abs_floors(&self) -> &'static [crate::AbsFloor] {
            self.unit.abs_floors()
        }
        fn cases(
            &self,
            ctx: &crate::CaseContext,
        ) -> Result<Vec<crate::Case>, crate::HarnessError> {
            self.unit.cases(ctx)
        }
    }

    /// Runs every case of `unit` through the goldens check, in this test build (so every
    /// certificate assertion runs), and panics unless each case's output agrees with its
    /// blessed golden line: the certificates then checked the very computation the
    /// blessed rows record, and their side work (copies, a kernel shared with the run)
    /// changed none of them. Returns the checks each case went through.
    pub(crate) fn replay_matches_golden<U: Certified + ?Sized>(
        unit: &U,
    ) -> std::collections::BTreeMap<String, Fired> {
        let ctx = crate::CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let dir = crate::golden::default_dir();
        let golden = crate::golden::read(&dir, unit.id()).unwrap();
        let known = crate::golden::known_failing(&dir, unit.id()).unwrap();
        let replay = Replay {
            unit,
            logs: std::cell::RefCell::default(),
        };
        let res = crate::golden::check_unit(
            &replay,
            &ctx,
            &crate::runner::RunOptions::default(),
            &golden,
            &known,
            false,
        );
        assert!(res.harness_error.is_none(), "{}: {:?}", unit.id(), res.harness_error);
        assert!(res.broken.is_empty(), "{}: broken {:?}", unit.id(), res.broken);
        assert!(
            res.changed.is_empty() && res.added.is_empty() && res.removed.is_empty(),
            "{}: changed {:?} added {:?} removed {:?}",
            unit.id(),
            res.changed.iter().map(|c| &c.0).collect::<Vec<_>>(),
            res.added,
            res.removed
        );
        replay.logs.into_inner()
    }

    impl Snapshot {
        /// The body's tolerances in list order, and its own model size estimate.
        pub(crate) fn of(k: &Kernel, b: &Brep) -> Result<Self> {
            Ok(Self {
                vertices: p1::vertices(b, None)?
                    .into_iter()
                    .map(|v| vertex::tolerance(b, v))
                    .collect::<Result<_>>()?,
                edges: p1::edges(b, None)?
                    .into_iter()
                    .map(|e| edge::tolerance(b, e))
                    .collect::<Result<_>>()?,
                faces: p1::faces(b, None)?
                    .into_iter()
                    .map(|f| face::p2::tolerance(b, f))
                    .collect::<Result<_>>()?,
                size: b.this_model_size_estimate(k, false),
                points: p1::vertices(b, None)?
                    .into_iter()
                    .map(|v| {
                        b.vertex(v)
                            .map(|x| [x.point.x.to_bits(), x.point.y.to_bits(), x.point.z.to_bits()])
                    })
                    .collect::<Result<_>>()?,
            })
        }

        /// [`Self::of`] the body as `SmBrep::Transform` with scale `scale` has it when it
        /// scales the tolerances: a non-uniform or tiny scale first turns the body to
        /// NURBS (`TurnToNURBS`, flag-independent, its status ignored), which may grow
        /// vertex tolerances, so those are read from a copy so converted.
        pub(crate) fn before_transform(
            k: &mut Kernel,
            b: &Brep,
            scale: Option<Vec3>,
        ) -> Result<Self> {
            let converts = scale.is_some_and(|s| {
                !are_same(s.x, s.y) || !are_same(s.x, s.z) || s.x < EFF_ZERO
            });
            if !converts {
                return Self::of(k, b);
            }
            let mut copy = b.clone_standing_for_self()?;
            let _ = turn_to_nurbs_plain(k, &mut copy);
            let mut snapshot = Self::of(k, &copy)?;
            // (the size is the body's own, which the conversion leaves alone)
            snapshot.size = b.this_model_size_estimate(k, false);
            Ok(snapshot)
        }

        /// Whether nothing the certificate records changed.
        pub(crate) fn unchanged(&self, after: &Self) -> bool {
            after.vertices == self.vertices
                && after.edges == self.edges
                && after.faces == self.faces
                && after.size.to_bits() == self.size.to_bits()
                && after.points == self.points
        }

        /// Panics unless the body's tolerances after a successful transform whose
        /// tolerance-scaling step has scale `scale` (`None`: no scale) are the closed
        /// form's. A scale `approx_eq` to `(1, 1, 1)` may also take `SmBrep::Transform`'s
        /// identity exit, which changes nothing. The model size is checked bit for bit.
        /// Counts the check that passed into the case's `log`.
        pub(crate) fn check(
            &self,
            k: &mut Kernel,
            b: &mut Brep,
            scale: Option<Vec3>,
            scale_tolerances: bool,
            what: &str,
            log: &mut Fired,
        ) -> Result<()> {
            let after = Self::of(k, b)?;
            assert_eq!(
                (after.vertices.len(), after.edges.len(), after.faces.len()),
                (self.vertices.len(), self.edges.len(), self.faces.len()),
                "{what}: a transform changed the topology"
            );
            let s = scale.map(tol_scale);
            let scaled = s.filter(|s| scale_tolerances && (s - 1.0).abs() > EFF_ZERO);
            let Some(s) = scaled else {
                assert_eq!(after.vertices, self.vertices, "{what}: vertex tolerances");
                assert_eq!(after.edges, self.edges, "{what}: edge tolerances");
                assert_eq!(after.faces, self.faces, "{what}: face tolerances");
                assert_eq!(
                    after.size.to_bits(),
                    self.size.to_bits(),
                    "{what}: model size estimate"
                );
                log.unscaled += 1;
                return Ok(());
            };
            // `SmBrep::Transform`'s identity exit (an identity placement and a scale
            // `approx_eq` to 1) changes nothing at all: no vertex point moved either, which
            // a non-identity placement or a scale past `SM_EFF_ZERO` would do.
            if scale.is_some_and(|v| v.approx_eq(Vec3::new(1.0, 1.0, 1.0)))
                && self.unchanged(&after)
            {
                log.identity += 1;
                return Ok(());
            }
            // The size is taken from the transformed body before its tolerances are
            // scaled (the edge and face boxes depend on them): the box of a copy given
            // back its old tolerances.
            let mut copy = b.clone_standing_for_self()?;
            for (v, t) in p1::vertices(&copy, None)?.into_iter().zip(&self.vertices) {
                vertex::set_tolerance(&mut copy, v, *t, false, false)?;
            }
            for (e, t) in p1::edges(&copy, None)?.into_iter().zip(&self.edges) {
                edge::set_tolerance(&mut copy, e, *t, false, false)?;
            }
            for (f, t) in p1::faces(&copy, None)?.into_iter().zip(&self.faces) {
                face::p3::set_tolerance(&mut copy, f, *t, false, false)?;
            }
            let model_box = calculate_bounding_box(
                k,
                &mut copy,
                BoundingBoxOptions {
                    approx: false,
                    tight: false,
                    expand_by_zone_tol_3d: false,
                },
            )?;
            let size = model_box.size().length();
            assert_eq!(
                after.size.to_bits(),
                size.to_bits(),
                "{what}: model size {} against the box diagonal {size}",
                after.size
            );
            // The zone is derived from the certified size, not read back from the body, and
            // the body's own tolerance must be that zone.
            let zone = solidcore::geom::tol::size_estimate_to_zone_tol_3d(size);
            assert_eq!(
                b.tolerance().to_bits(),
                zone.to_bits(),
                "{what}: body tolerance {} against the zone {zone} of size {size}",
                b.tolerance()
            );
            for (name, old, new) in [
                ("vertex", &self.vertices, &after.vertices),
                ("edge", &self.edges, &after.edges),
                ("face", &self.faces, &after.faces),
            ] {
                for (i, (t, got)) in old.iter().zip(new).enumerate() {
                    let want = max2(zone, t * s);
                    assert_eq!(
                        got.to_bits(),
                        want.to_bits(),
                        "{what}: {name} {i} tolerance {got:e}, closed form max({zone:e}, {t:e} * {s}) = {want:e}"
                    );
                }
            }
            log.scaled += 1;
            Ok(())
        }
    }
}
