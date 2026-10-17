//! C++ parity for merge control, topology cleanup, face coincidence, vertex copies,
//! region attributes and file-local region/translation helpers. Topology fixtures
//! are constructed in C++ and imported as raw arena records before either helper
//! runs, following the topology units. Deferred paths still run the native helper.
use super::topology_edgeuse_p1::{self as spec, BrepIn};
use super::topology_vertexuse::import;
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1 as port;
use solidcore::boolean::topology_intersector::p1::Relations;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{ids, Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::context::{Context, NotifyCallback};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::topology::topology::Topology;
use solidcore::topology::{
    ArenaId, Brep, EdgeId, EdgeuseOwner, FaceId, FaceuseId, LoopuseStart, RegionId, VertexId,
    VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::{boolean_merge_p1 as ffi, topology_vertexuse as fixture};
use std::collections::BTreeMap;

/// Differential runner for `boolean.merge_p1`.
pub struct Runner;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: u8,
    args: [u8; 3],
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    other: Option<BrepIn>,
    #[serde(default)]
    tolerances: Option<[f64; 2]>,
    #[serde(default = "half")]
    parameter: f64,
}
fn half() -> f64 {
    0.5
}
fn harness(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn options(mask: u8) -> port::MergeOptions {
    port::MergeOptions::new(
        mask & 1 != 0,
        mask & 2 != 0,
        mask & 4 != 0,
        mask & 8 != 0,
        mask & 16 != 0,
    )
}
fn flags(o: port::MergeOptions) -> Vec<f64> {
    [
        o.cookie_cutter,
        o.imprinting,
        o.boolean_post_process,
        o.imprint_and_classify_faces,
        o.keep_other_brep,
    ]
    .into_iter()
    .map(|b| f64::from(u8::from(b)))
    .collect()
}
fn control([a, b, c]: [u8; 3]) -> Vec<f64> {
    let mut state = port::MergeState::new(());
    let mut v = flags(state.options);
    v.push(state.tag_id as f64);
    let o = options(a);
    v.extend(flags(o));
    state.set_merge_options((b != 0).then_some(&o));
    v.extend(flags(state.options));
    v.push(state.tag_id as f64);
    state.options = options(c);
    v.extend(flags(state.options));
    v.push(state.tag_id as f64);
    v
}
fn block(i: &Inputs) -> Result<(String, Vec<f64>), HarnessError> {
    if i.op == 0 {
        return Ok((String::new(), Vec::new()));
    }
    Ok((
        spec::path_of(i.brep.as_ref())?,
        spec::brep_block_of(i.brep.as_ref(), false, false)?,
    ))
}
fn other_block(i: &Inputs) -> Result<(String, Vec<f64>), HarnessError> {
    if let Some(other) = &i.other {
        Ok((
            spec::path_of(Some(other))?,
            spec::brep_block_of(Some(other), false, false)?,
        ))
    } else {
        block(i)
    }
}
fn obj(k: u64, t: i64) -> ObjectRef {
    ObjectRef {
        key: ObjectKey(k),
        type_code: t,
    }
}
fn id<I: ArenaId>(v: Option<I>) -> i64 {
    v.map_or(-1, |v| i64::from(v.index()))
}
fn graph(b: &Brep, record: Vec<f64>) -> Output {
    let mut ints = vec![
        b.regions().len() as i64,
        b.shells().len() as i64,
        b.faceuses().len() as i64,
        b.faces().len() as i64,
        b.loopuses().len() as i64,
        b.loops().len() as i64,
        b.edgeuses().len() as i64,
        b.edges().len() as i64,
        b.vertexuses().len() as i64,
        b.vertices().len() as i64,
    ];
    for (_, r) in b.regions().iter() {
        ints.push(i64::from(r.in_brep));
    }
    for (_, f) in b.faces().iter() {
        ints.extend([
            i64::from(f.shell.index()),
            id(f.faceuse),
            i64::from(f.rectangular_trim),
            f.loops.len() as i64,
        ]);
        ints.extend(f.loops.iter().map(|v| i64::from(v.index())));
    }
    for (_, l) in b.loops().iter() {
        ints.extend([
            i64::from(l.face.index()),
            id(l.loopuse),
            l.edges.len() as i64,
        ]);
        for e in &l.edges {
            ints.extend([i64::from(e.edge.index()), i64::from(e.forward)]);
        }
    }
    for (_, l) in b.loopuses().iter() {
        ints.extend(match l.start {
            LoopuseStart::Unknown => [0, -1],
            LoopuseStart::Edgeuse(e) => [1, id(e)],
            LoopuseStart::Vertexuse(v) => [2, id(v)],
        });
    }
    for (_, e) in b.edgeuses().iter() {
        ints.extend(match e.owner {
            EdgeuseOwner::Unknown => [0, -1],
            EdgeuseOwner::Loopuse(l) => [1, id(l)],
            EdgeuseOwner::Shell(sh) => [2, id(sh)],
        });
    }
    for (_, v) in b.vertexuses().iter() {
        ints.push(id(v.vertex));
        ints.extend(match v.owner {
            VertexuseOwner::Unknown => [0, -1],
            VertexuseOwner::Edgeuse(e) => [1, id(e)],
            VertexuseOwner::Loopuse(l) => [2, id(l)],
            VertexuseOwner::Shell(sh) => [3, id(sh)],
        });
    }
    for (_, s) in b.shells().iter() {
        ints.push(id(s.region));
        ints.push(s.faces.len() as i64);
        ints.extend(s.faces.iter().map(|v| i64::from(v.index())));
    }
    for (_, f) in b.faceuses().iter() {
        ints.extend([
            id(f.shell),
            id(f.face),
            id(f.mate),
            f.orientation as i64,
            f.loopuses.len() as i64,
        ]);
        ints.extend(f.loopuses.iter().map(|v| i64::from(v.index())));
    }
    for (_, l) in b.loopuses().iter() {
        ints.extend([id(l.faceuse), id(l.loop_), id(l.mate), l.orientation as i64]);
    }
    for (_, e) in b.edges().iter() {
        ints.push(e.edgeuses.len() as i64);
        ints.extend(e.edgeuses.iter().map(|v| i64::from(v.index())));
    }
    for (_, e) in b.edgeuses().iter() {
        ints.extend([
            id(e.edge),
            id(e.ccw),
            id(e.cw),
            id(e.vertexuse),
            e.orientation as i64,
        ]);
    }
    let mut values = Vec::new();
    for (_, v) in b.vertexuses().iter() {
        values.extend([v.uv_point.x, v.uv_point.y]);
    }
    for (_, v) in b.vertices().iter() {
        values.extend([v.point.x, v.point.y, v.point.z, v.tolerance]);
        ints.push(v.vertexuses.len() as i64);
        ints.extend(v.vertexuses.iter().map(|v| i64::from(v.index())));
    }
    for (_, f) in b.faces().iter() {
        let a = f.uv_domain.min();
        let z = f.uv_domain.max();
        values.extend([a.x, a.y, z.x, z.y, f.tolerance]);
    }
    for (_, e) in b.edges().iter() {
        values.extend([e.interval.min(), e.interval.max(), e.tolerance]);
    }
    Output::new()
        .values("record", record)
        .values("geometry", values)
        .ints("graph", ints)
}
#[derive(Debug, Default)]
struct Recorder {
    events: Vec<[i64; 5]>,
}
impl NotifyCallback for Recorder {
    fn execute(
        &mut self,
        operation: NotifyOperation,
        caller: ObjectRef,
        a: Option<ObjectRef>,
        b: Option<ObjectRef>,
        c: Option<ObjectRef>,
    ) -> solidcore::Result<()> {
        if matches!(
            operation,
            NotifyOperation::Copy | NotifyOperation::RegPropagation
        ) {
            let key = |o: Option<ObjectRef>| o.map_or(-1, |o| o.key.0 as i64);
            self.events.push([
                i64::from(operation as i32),
                caller.key.0 as i64,
                key(a),
                key(b),
                key(c),
            ]);
            return Err(SolidError::sm_err("difftest::notify_callback"));
        }
        Ok(())
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
struct TwoRegions {
    primary: Brep,
    other: Brep,
}
impl solidcore::topology::shell::RegionLookup for TwoRegions {
    fn resolve_region(
        &self,
        object: ObjectKey,
    ) -> solidcore::Result<solidcore::topology::shell::RegionRef<'_>> {
        let (brep, index) = match object.0 {
            1 => (&self.primary, 0),
            2 => (&self.other, 1),
            _ => {
                return Err(SolidError::invalid_input(
                    "difftest::region_lookup",
                    "unknown region object",
                ))
            }
        };
        Ok(solidcore::topology::shell::RegionRef {
            brep,
            brep_id: solidcore::topology::BrepId::from_index(index),
            region_id: RegionId::from_index(0),
        })
    }
}
fn append_events(context: &Context, out: &mut Vec<f64>) -> solidcore::Result<()> {
    let recorder = context
        .user_notify_callback()
        .and_then(NotifyCallback::as_any)
        .and_then(|a| a.downcast_ref::<Recorder>())
        .ok_or_else(|| SolidError::null_pointer("difftest::recorder"))?;
    out.push(recorder.events.len() as f64);
    out.extend(recorder.events.iter().flatten().map(|&v| v as f64));
    Ok(())
}
fn run_rust(i: &Inputs, b: &mut Brep, other: &Brep) -> Result<Vec<f64>, SolidError> {
    let [a, c, d] = i.args;
    let mut other = other.clone();
    let other = &mut other;
    let mut k = Kernel::default();
    let mut out = Vec::new();
    let mut context = Context::default();
    context.set_user_notify_callback(Some(Box::new(Recorder::default())));
    match i.op {
        1 => {
            let mut r = port::CleanupRelations::default();
            if c != 0 {
                r.edges = b.edges().ids().collect();
                r.vertices = b.vertices().ids().collect();
            }
            if c == 1 {
                for (x, y) in b.edges().ids().zip(other.edges().ids()) {
                    r.mates.edges.record(x, y)?;
                }
                for (x, y) in b.vertices().ids().zip(other.vertices().ids()) {
                    r.mates.vertices.record(x, y)?;
                }
            }
            if c == 2 && a == 0 {
                r.edges.push(EdgeId::from_index(u32::MAX));
                r.vertices.push(VertexId::from_index(u32::MAX));
            }
            port::boolean_post_process(&mut k, b, &r, a != 0)?;
        }
        2 => {
            let maps: BTreeMap<_, _> = if c != 0 {
                b.edges()
                    .ids()
                    .zip(other.edges().ids())
                    .enumerate()
                    .filter(|(j, _)| c != 2 || *j != 0)
                    .map(|(_, v)| v)
                    .collect()
            } else {
                BTreeMap::new()
            };
            let reverse = maps.iter().map(|(&x, &y)| (y, x)).collect();
            let f = FaceId::from_index(u32::from(a) % b.faces().len() as u32);
            let g = FaceId::from_index(u32::from(d) % other.faces().len() as u32);
            let (x, y, changed) =
                port::fix_found_face_coincidence(b, &mut other.clone(), f, g, &maps, &reverse)?;
            out.extend([f64::from(u8::from(changed)), x.len() as f64]);
            out.extend(x.iter().map(|f| f64::from(f.index())));
            out.push(y.len() as f64);
            out.extend(y.iter().map(|f| f64::from(f.index())));
        }
        3 => {
            let source = (a != 0)
                .then(|| VertexId::from_index(u32::from(d) % other.vertices().len() as u32));
            let dest = VertexId::from_index(u32::from(d) % b.vertices().len() as u32);
            let mut relations = Relations::default();
            if c != 0 {
                if let Some(s) = source {
                    relations.vertices.record(dest, s)?;
                }
            }
            if let Some([pt, ot]) = i.tolerances {
                solidcore::topology::vertex::set_tolerance(
                    b,
                    dest,
                    solidcore::geom::tol::zone_tol3d(&k, pt),
                    false,
                    true,
                )?;
                if let Some(src) = source {
                    solidcore::topology::vertex::set_tolerance(
                        other,
                        src,
                        solidcore::geom::tol::zone_tol3d(&k, ot),
                        false,
                        true,
                    )?;
                }
            }
            let mut attrs = AttributeStore::new();
            let mut records = BTreeMap::new();
            let mut src = Topology::new();
            src.set_all_flags(16);
            let mut dst = Topology::new();
            dst.set_all_flags(1);
            records.insert(ObjectKey(1), src);
            records.insert(ObjectKey(2), dst);
            let h = attrs.insert(Attribute::long(555, 29, Behavior::Copy))?;
            attrs.add_attribute(ObjectKey(1), h)?;
            let result = port::merge_vertex(
                b,
                other,
                source,
                (a != 2).then_some(RegionId::from_index(0)),
                &mut relations,
                &mut port::VertexCopy {
                    records: &mut records,
                    attributes: &mut attrs,
                    source_key: &|_| obj(1, 16021),
                    destination_key: &|_| obj(2, 16021),
                },
                Some(&mut context),
            )?;
            out.push(result.map_or(-1.0, |v| f64::from(v.index())));
            if result.is_some() {
                out.push(
                    records
                        .get(&ObjectKey(2))
                        .ok_or_else(|| SolidError::null_pointer("difftest::merge_copy"))?
                        .all_flags() as f64,
                );
                out.push(
                    attrs
                        .find_attribute(ObjectKey(2), 555)
                        .and_then(|h| attrs.get(h))
                        .and_then(Attribute::long_value)
                        .unwrap_or(-1) as f64,
                );
            }
            append_events(&context, &mut out)?;
        }
        4 => {
            let mut attrs = AttributeStore::new();
            let r = obj(1, 16012);
            let h = attrs.insert(Attribute::long(555, 29, Behavior::Copy))?;
            attrs.add_attribute(r.key, h)?;
            if c != 0 {
                let h = attrs.insert(Attribute::color(
                    ids::COLOR,
                    Vec3::new(0.2, 0.4, 0.6),
                    Behavior::Copy,
                ))?;
                attrs.add_attribute(r.key, h)?;
            }
            if d != 0 {
                let h = attrs.insert(Attribute::merge_region(r.key, 0, ObjectKey(2), 1))?;
                attrs.add_attribute(r.key, h)?;
            }
            // Object 1 is the primary's first region (with the case's void flag), object 2
            // the other operand's first region, as the shim wires them.
            let mut primary = b.clone();
            primary
                .region_mut(RegionId::from_index(0))?
                .set_is_void(i32::from(a != 0));
            let lookup = TwoRegions {
                primary,
                other: other.clone(),
            };
            port::propagate_other_src_region_attribs(
                &[port::RegionPropagation {
                    region: r,
                    is_void: a != 0,
                    source_lists: [obj(5, 0), obj(6, 0)],
                }],
                &mut attrs,
                Some(&mut context),
                BooleanOperation::Union,
                Some(&lookup),
            )?;
            for id in [555, ids::COLOR, ids::OTHER_REGION_ID] {
                out.push(f64::from(u8::from(
                    attrs.find_attribute(r.key, id).is_some(),
                )));
            }
            append_events(&context, &mut out)?;
        }
        5 => {
            let f = FaceuseId::from_index(u32::from(a) % other.faceuses().len() as u32);
            out.push(
                port::find_brep_region_of_other_face(
                    &k,
                    b,
                    other,
                    f,
                    &if c != 0 {
                        b.edges()
                            .ids()
                            .zip(other.edges().ids())
                            .map(|(a, b)| (b, a))
                            .collect()
                    } else {
                        BTreeMap::new()
                    },
                )?
                .map_or(-1.0, |v| f64::from(v.index())),
            );
        }
        6 => {
            let e = EdgeId::from_index(u32::from(a) % b.edges().len() as u32);
            out.push(port::edge_borders_solid(b, e)?.map_or(-1.0, |v| f64::from(v.index())));
        }
        7 => {
            let mut other = other.clone();
            port::temporary_brep_transform(b, &mut other)?;
        }
        8 | 9 => {
            let sec = solidcore::topology::EdgeuseId::from_index(
                u32::from(a) % b.edgeuses().len() as u32,
            );
            let cls = solidcore::topology::EdgeuseId::from_index(
                u32::from(c) % other.edgeuses().len() as u32,
            );
            let edge = b
                .edgeuse(sec)?
                .edge
                .ok_or_else(|| SolidError::null_pointer("difftest::radial"))?;
            if i.op == 8 {
                out.push(f64::from(
                    solidcore::query::edge::radial_sector_classify_between(
                        &k,
                        b,
                        edge,
                        i.parameter,
                        sec,
                        Some(&mut *other),
                        cls,
                    )?
                    .code(),
                ));
            } else {
                let r = solidcore::topology::edge::find_radial_sector_between(
                    &k,
                    b,
                    edge,
                    Some(&mut *other),
                    cls,
                    None,
                    solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same,
                    None,
                )?;
                out.extend([id(r.radial_edgeuse) as f64, id(r.coincident_faceuse) as f64]);
            }
        }
        _ => return Err(SolidError::invalid_input("difftest", "bad operation")),
    }
    Ok(out)
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p1"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, block) = block(&i)?;
        let (other_path, other_block) = other_block(&i)?;
        let v = ffi::eval(ffi::Request {
            op: i.op,
            args: i.args,
            primary: ffi::Operand {
                path: &path,
                block: &block,
            },
            other: ffi::Operand {
                path: &other_path,
                block: &other_block,
            },
            tolerances: i.tolerances,
            parameter: i.parameter,
        })
        .map_err(|e| harness(e.to_string()))?;
        if i.op == 0 {
            return Ok(Output::new().values("record", v));
        }
        let [status, count, ..] = v.as_slice() else {
            return Err(harness("short merge result"));
        };
        if *status != 0.0 {
            return Ok(Output::err(format!("status_{}", *status as i64)));
        }
        let end = (*count as usize)
            .checked_add(2)
            .ok_or_else(|| harness("record count overflow"))?;
        let raw = v.get(end..).ok_or_else(|| harness("short graph"))?;
        let (b, _) = import(raw)?;
        let record = v.get(2..end).ok_or_else(|| harness("short record"))?;
        Ok(graph(&b, record.to_vec()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if i.op == 0 {
            return Ok(Output::new().values("record", control(i.args)));
        }
        let (path, block) = block(&i)?;
        let raw = fixture::eval(fixture::Op::Export, &path, &block)
            .map_err(|e| harness(e.to_string()))?;
        let (mut b, _) = import(&raw)?;
        let (other_path, other_block) = other_block(&i)?;
        let other_raw = fixture::eval(fixture::Op::Export, &other_path, &other_block)
            .map_err(|e| harness(e.to_string()))?;
        let (other, _) = import(&other_raw)?;
        match run_rust(&i, &mut b, &other) {
            Ok(v) => Ok(graph(&b, v)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| {
                Output::err(format!(
                    "status_{}",
                    match e {
                        SolidError::Upstream { code, .. } => code,
                        _ => -1,
                    }
                ))
            })),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.below(3) == 0 {
            return Some(json!({"op":0,"args":[rng.below(32),rng.below(2),rng.below(32)]}));
        }
        let wire = rng.below(2) == 0;
        let op = if wire {
            *[1, 3, 4, 6, 7].get(rng.below(5) as usize)?
        } else {
            *[1, 2, 4, 5, 6, 7].get(rng.below(6) as usize)?
        };
        // Include both sides of the default 3D zone tolerance as well as tiny/huge geometry.
        let scale = *[1e-6, 0.999999e-5, 1e-5, 1.000001e-5, 1.0, 1e6].get(rng.below(6) as usize)?;
        let p = rng.point3(-10.0 * scale, 10.0 * scale);
        let brep = if wire {
            json!({"kind":"wire","curve":{"dim":3,"cpts":[[p[0],p[1],p[2],1.0],[p[0]+scale,p[1]+scale,p[2],1.0]],"p":1,"u":[0,0,1,1]},"t0":0.125,"t1":0.875})
        } else {
            json!({"kind":"face","surface":{"plane":{"o":p,"n":[0,0,1]}},"dom":[-2,-1,3,4]})
        };
        Some(
            json!({"op":op,"args":[rng.below(2),rng.below(3),rng.below(2)],"brep":brep,
          "tolerances":[*[1e-5,1e-3,1e-1].get(rng.below(3) as usize)?,*[1e-5,1e-3,1e-1].get(rng.below(3) as usize)?]}),
        )
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            root: ctx.root.clone(),
            seeds: ctx.seeds.max(96),
        };
        crate::runner::default_cases(self, &ctx)
    }
}
