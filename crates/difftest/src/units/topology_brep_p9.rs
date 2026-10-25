//! Native B-rep construction calls and dependency-limited Rust entrypoints.
//!
//! Initial graphs are exported before either operation. Split refusals compare
//! their retained endpoint as well as the complete semantic topology/geometry
//! snapshot. Successful native mutations on deferred paths are not parity passes.
use super::boolean_merge_p1::{graph_reachable, Canon};
use super::topology_edgeuse_p1::{self as spec, BrepIn};
use super::topology_vertexuse::{import, import_raw};
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p9 as port;
use solidcore::topology::{ArenaId, Brep, EdgeId, FaceId, RegionId, VertexuseId};
use solidcore::SolidError;
use solidcore_ffi::units::topology_brep_p9 as ffi;

/// Differential runner for `topology.brep_p9`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: u8,
    brep: BrepIn,
    #[serde(default)]
    selector: u8,
    #[serde(default)]
    variant: u8,
    #[serde(default = "yes")]
    editing: bool,
    #[serde(default = "yes")]
    same: bool,
    #[serde(default, with = "nf")]
    parameter: f64,
    #[serde(default, with = "nf_array")]
    point: [f64; 3],
    #[serde(default, with = "nf_array")]
    tolerances: [f64; 2],
}
// JSON numbers or the explicit strings "NaN", "inf" and "-inf".
mod nf {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `with` signature
    pub fn serialize<S: Serializer>(x: &f64, s: S) -> Result<S::Ok, S::Error> {
        if x.is_nan() {
            s.serialize_str("NaN")
        } else if x.is_infinite() {
            s.serialize_str(if *x > 0.0 { "inf" } else { "-inf" })
        } else {
            s.serialize_f64(*x)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom("bad number")),
            Value::String(s) if s == "NaN" => Ok(f64::NAN),
            Value::String(s) if s == "inf" => Ok(f64::INFINITY),
            Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
            other => Err(D::Error::custom(format!("bad float {other}"))),
        }
    }
}
// Arrays of `nf` values.
mod nf_array {
    use serde::de::Error as _;
    use serde::ser::SerializeTuple;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde_json::Value;

    struct One(f64);
    impl serde::Serialize for One {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            super::nf::serialize(&self.0, s)
        }
    }

    pub fn serialize<S: Serializer, const N: usize>(x: &[f64; N], s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(N)?;
        for v in x {
            t.serialize_element(&One(*v))?;
        }
        t.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[f64; N], D::Error> {
        let values = Vec::<Value>::deserialize(d)?;
        let values = values
            .into_iter()
            .map(|v| super::nf::deserialize(v).map_err(D::Error::custom))
            .collect::<Result<Vec<f64>, D::Error>>()?;
        values
            .try_into()
            .map_err(|_| D::Error::custom(format!("expected {N} floats")))
    }
}

fn yes() -> bool {
    true
}
fn error(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}
fn id<I: ArenaId>(v: Option<I>) -> f64 {
    v.map_or(-1.0, |v| f64::from(v.index()))
}
fn status(e: &SolidError) -> Result<Output, HarnessError> {
    if let Some(out) = Output::unported(e) {
        return Ok(out);
    }
    Err(error(e))
}
fn ordinary<T>(r: solidcore::Result<T>) -> Result<i32, SolidError> {
    match r {
        Ok(_) => Ok(0),
        Err(SolidError::Upstream { code, .. }) => Ok(code),
        Err(e) => Err(e),
    }
}
fn request<'a>(i: &Inputs, path: &'a str, block: &'a [f64]) -> ffi::Request<'a> {
    ffi::Request {
        op: i.op,
        selector: i.selector,
        variant: i.variant,
        editing: i.editing,
        same: i.same,
        path,
        block,
        parameter: i.parameter,
        point: i.point,
        tolerances: i.tolerances,
    }
}
fn target<I: ArenaId>(count: usize, selector: u8) -> Option<I> {
    (count > 0).then(|| I::from_index(u32::from(selector) % count as u32))
}
fn rust(i: &Inputs, initial: &[f64]) -> Result<Output, HarnessError> {
    let (mut b, _) = import(
        initial
            .get(2..)
            .ok_or_else(|| error("missing initial export"))?,
    )?;
    // As the native B-rep: each edge owns its curve and each face its surface, under the
    // arena keys op 0's `BrepQuery::default()` names them by (the import leaves curves
    // unowned and owns face f's surface as `1 + f`), and a face-owned surface carries the
    // face's tolerance. Without it the B-rep cache trees hold no edges or faces.
    crate::util::own_arena_geometry(&mut b);
    for f in b.faces().ids().collect::<Vec<_>>() {
        b.sync_face_surface_tolerance(f).map_err(|e| error(&e))?;
    }
    // As the shim, after setup and export: `b->m_bEditingEnabled = editing`.
    b.set_editing_enabled(i.editing);
    let mut k = Kernel::default();
    let mut record = Vec::new();
    let mut records = std::collections::BTreeMap::new();
    let mut attributes = solidcore::geom::attribute::AttributeStore::default();
    let mut env = solidcore::topology::edge::NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &object_ref,
    };
    let result: solidcore::Result<i32> = (|| {
        match i.op {
            0 => {
                record.push(id(port::find_infinite_region(
                    &mut k,
                    &mut b,
                    &solidcore::query::brep::BrepQuery::default(),
                    None,
                    None,
                )?));
                Ok(0)
            }
            1 => {
                let face = if i.variant == 1 {
                    None
                } else {
                    target::<FaceId>(b.faces().len(), i.selector)
                };
                ordinary(port::orient_sheet_surfaces(&mut k, &mut b, face, i.same))
            }
            2 => {
                let region = if i.variant == 1 {
                    None
                } else {
                    target::<RegionId>(b.regions().len(), i.selector)
                };
                let (shell, vertex) =
                    port::make_shell_vertex(&mut b, region, Vec3::from(i.point), &mut env)?;
                record.extend([id(Some(shell)), id(Some(vertex))]);
                Ok(0)
            }
            3 => {
                let face = target::<FaceId>(b.faces().len(), i.selector)
                    .ok_or_else(|| SolidError::invalid_input("difftest::brep_p9", "no face"))?;
                let (loop_, vertex) = port::make_vertex_loop(
                    &mut b,
                    face,
                    Vec3::from(i.point),
                    if i.same {
                        OrientType::Same
                    } else {
                        OrientType::Opposite
                    },
                    &mut env,
                )?;
                record.extend([id(Some(loop_)), id(Some(vertex))]);
                Ok(0)
            }
            4 => {
                let edge = target::<EdgeId>(b.edges().len(), i.selector)
                    .ok_or_else(|| SolidError::invalid_input("difftest::brep_p9", "no edge"))?;
                // Native tolerances were assigned before export. They have already
                // undergone SmZoneTol3d conversion and are imported verbatim.
                let split = port::make_vertex_split_edge(
                    &mut k,
                    &mut b,
                    edge,
                    i.parameter,
                    &mut env,
                    &curve_ref,
                )?;
                record.extend([
                    id(split.edges.map(|v| v[0])),
                    id(split.edges.map(|v| v[1])),
                    id(split.vertex),
                ]);
                Ok(split.status())
            }
            5 => {
                let face = if i.variant == 1 {
                    None
                } else {
                    target::<FaceId>(b.faces().len(), i.selector)
                };
                let vu = |slot: usize| {
                    initial
                        .get(slot)
                        .copied()
                        .filter(|v| *v >= 0.0)
                        .map(|v| VertexuseId::from_index(v as u32))
                };
                let start = vu(0);
                let end = vu(1);
                let point = |v: Option<VertexuseId>, default| -> solidcore::Result<Vec3> {
                    if let Some(v) = v {
                        let vertex = b
                            .vertexuse(v)?
                            .vertex
                            .ok_or_else(|| SolidError::null_pointer("difftest::brep_p9"))?;
                        Ok(b.vertex(vertex)?.point)
                    } else {
                        Ok(default)
                    }
                };
                let p = point(start, Vec3::new(0.0, 0.0, 0.0))?;
                let q = point(end, Vec3::new(1.0, 1.0, 0.0))?;
                let curve = (i.variant != 2).then(|| Curve::from(Line::from_points(p, q, 3)));
                let output = port::make_edge_in_face_with_vu(
                    &k,
                    &mut b,
                    port::EdgeInFaceInput {
                        face,
                        vertices: [
                            if i.variant == 3 { None } else { start },
                            if i.variant == 4 { None } else { end },
                        ],
                        curve,
                        uv_trim: None,
                        drop_fail: None,
                        interval: Extent1d::from_min_max(0.0, 1.0),
                        orientation: OrientType::Same,
                        curve_zone_tol_3d: zone_tol3d(&k, 1e-7),
                    },
                    &mut env,
                    &curve_ref,
                );
                match &output {
                    Ok(out) => record.extend([id(Some(out.edge)), id(out.loop_), id(out.face)]),
                    Err(_) => record.extend([-1.0, -1.0, -1.0]),
                }
                ordinary(output)
            }
            6 | 7 => {
                use solidcore::intersect::point_class::{
                    BrepMerge, PointClassType, PointClassification,
                };
                use solidcore::EntityRef;
                let edge = target::<EdgeId>(b.edges().len(), i.selector)
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?;
                let target = if i.op == 6 {
                    EntityRef::Edge(edge)
                } else {
                    EntityRef::Vertex(
                        solidcore::topology::edge::start_vertex(&b, edge)?
                            .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?,
                    )
                };
                let mut pc = PointClassification::new(zone_tol3d(&k, 1e-7));
                pc.set_class_object(
                    if i.op == 6 {
                        PointClassType::Edge
                    } else {
                        PointClassType::Vertex
                    },
                    Some(Handle(target)),
                );
                pc.set_t_param(i.parameter, false);
                if i.variant == 4 {
                    pc.unset_parameters();
                }
                if !i.same {
                    pc.set_sss_point(Vec3::from(i.point));
                }
                let mut records = std::collections::BTreeMap::new();
                let mut attributes = solidcore::geom::attribute::AttributeStore::default();
                let mut env = solidcore::topology::edge::NotifyEnv {
                    records: &mut records,
                    attributes: &mut attributes,
                    context: None,
                    key_of: &object_ref,
                };
                let result = pc.merge_into_brep(
                    &mut k,
                    Vec3::from(i.point),
                    BrepMerge {
                        brep: &mut b,
                        target,
                        handle: &Handle,
                        notifications: &mut env,
                        curve_ref: &curve_ref,
                    },
                );
                let vertex = match pc.object() {
                    Some(Handle(EntityRef::Vertex(v))) => Some(v),
                    _ => None,
                };
                let sizes = match &result {
                    Ok(out) => (out.new_edges.len(), out.new_vertices.len()),
                    Err(_) => (0, 0),
                };
                record.extend([
                    f64::from(pc.point_class().code()),
                    id(vertex),
                    sizes.0 as f64,
                    sizes.1 as f64,
                    f64::from(u8::from(pc.are_parameters_set())),
                    pc.t_param(),
                ]);
                ordinary(result)
            }
            8 => {
                let face = target::<FaceId>(b.faces().len(), i.selector)
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?;
                let f = b.face(face)?;
                let surface = f
                    .surface
                    .as_ref()
                    .ok_or_else(|| SolidError::sm_err("difftest::brep_p9"))?
                    .clone();
                let y = if i.variant == 1 { 2.0 } else { 0.5 };
                let (x0, x1) = if i.variant == 2 {
                    (0.25, 0.75)
                } else {
                    (-1.0, 2.0)
                };
                let interval = Extent1d::from_min_max(-3.0, 7.0);
                let mut line = Line::from_points(
                    surface.evaluate_point(&k, Point2::new(x0, y))?,
                    surface.evaluate_point(&k, Point2::new(x1, y))?,
                    3,
                );
                line.edit_parameterization(&interval)?;
                // the face-owned surface: the drop reaches its face
                let dropped =
                    port::drop_edge_curve_on_face(&k, &mut b, face, &Curve::from(line), &interval)?;
                record.extend([
                    dropped.max_drop_to_surf,
                    dropped.max_approx_dev,
                    dropped.uv_curves.len() as f64,
                ]);
                for curve in dropped.uv_curves {
                    let d = curve.natural_interval();
                    record.extend([d.min(), d.max()]);
                    for t in [d.min(), d.min() + 0.5 * (d.max() - d.min()), d.max()] {
                        let p = curve.evaluate_point(&k, t)?;
                        record.extend([p.x, p.y, p.z]);
                    }
                }
                Ok(0)
            }
            _ => Err(SolidError::invalid_input("difftest::brep_p9", "bad op")),
        }
    })();
    match result {
        Ok(code) => {
            record.insert(0, f64::from(code));
            canon_record(&b, i.op, &mut record);
            skeleton_caches(&mut b);
            Ok(graph_reachable(&b, record))
        }
        Err(SolidError::Upstream { code, .. }) if i.op <= 4 => {
            // These APIs clear their pointer outputs before the first fallible
            // operation. Compare defined native failures as statuses, not harness
            // errors; any partial graph changes are still compared normally.
            let slots = match i.op {
                0 => 1,
                2 | 3 => 2,
                4 => 3,
                _ => 0,
            };
            record.resize(slots, -1.0);
            record.insert(0, f64::from(code));
            canon_record(&b, i.op, &mut record);
            skeleton_caches(&mut b);
            Ok(graph_reachable(&b, record))
        }
        Err(e) => status(&e),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Handle(solidcore::EntityRef);
impl solidcore::intersect::point_class::ClassObject for Handle {
    fn class_kind(&self) -> solidcore::intersect::point_class::ClassObjectKind {
        use solidcore::intersect::point_class::ClassObjectKind as K;
        use solidcore::EntityRef as E;
        match self.0 {
            E::Edge(_) => K::Edge,
            E::Vertex(_) => K::Vertex,
            E::Face(_) => K::Face,
            E::Region(_) => K::Region,
            _ => K::Other,
        }
    }
}
/// Empties the Rust-only face loop and loop edge lists (`Face::loops`, `Loop::edges`,
/// which `make_edge_in_face_with_vu` and friends keep in step) as the shared importer
/// leaves them on the C++ side, which has no such lists: the graph compares the
/// upstream topology links.
fn skeleton_caches(b: &mut Brep) {
    let faces: Vec<FaceId> = b.faces().ids().collect();
    for f in faces {
        if let Ok(r) = b.face_mut(f) {
            r.loops.clear();
        }
    }
    let loops: Vec<solidcore::topology::LoopId> = b.loops().ids().collect();
    for l in loops {
        if let Ok(r) = b.loop_mut(l) {
            r.edges.clear();
        }
    }
}

/// Puts the B-rep ids of `record` (status first) in [`Canon`] numbering, the shim's
/// `fresh.find` of a walk after the op.
fn canon_record(b: &Brep, op: u8, record: &mut [f64]) {
    let c = Canon::reachable(b);
    let mut map = |at: usize, f: &dyn Fn(f64) -> f64| {
        if let Some(x) = record.get_mut(at) {
            *x = f(*x);
        }
    };
    match op {
        0 => map(1, &|x| c.regions.of_slot(x)),
        2 => {
            map(1, &|x| c.shells.of_slot(x));
            map(2, &|x| c.vertices.of_slot(x));
        }
        3 => {
            map(1, &|x| c.loops.of_slot(x));
            map(2, &|x| c.vertices.of_slot(x));
        }
        4 => {
            map(1, &|x| c.edges.of_slot(x));
            map(2, &|x| c.edges.of_slot(x));
            map(3, &|x| c.vertices.of_slot(x));
        }
        5 => {
            map(1, &|x| c.edges.of_slot(x));
            map(2, &|x| c.loops.of_slot(x));
            map(3, &|x| c.faces.of_slot(x));
        }
        6 | 7 => map(2, &|x| c.vertices.of_slot(x)),
        _ => {}
    }
}

fn object_ref(e: solidcore::EntityRef) -> solidcore::geom::object::ObjectRef {
    super::boolean_topology_intersector_p1::p9_entity_ref(e)
}

fn curve_ref(edge: EdgeId, kind: i64) -> solidcore::geom::object::ObjectRef {
    solidcore::geom::object::ObjectRef {
        key: solidcore::geom::attribute::ObjectKey(((kind as u64) << 32) | u64::from(edge.index())),
        type_code: kind,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p9"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let path = spec::path_of(Some(&i.brep))?;
        let block = spec::brep_block_of(Some(&i.brep), false, false)?;
        let raw = ffi::eval(request(&i, &path, &block), false).map_err(|e| error(&e))?;
        let n = *raw.get(1).ok_or_else(|| error("short native output"))? as usize;
        let mut record = vec![*raw.first().ok_or_else(|| error("missing native status"))?];
        record.extend(
            raw.get(2..2 + n)
                .ok_or_else(|| error("short native record"))?,
        );
        let (b, _) = import_raw(
            raw.get(2 + n..)
                .ok_or_else(|| error("short native graph"))?,
        )?;
        canon_record(&b, i.op, &mut record);
        Ok(graph_reachable(&b, record))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let path = spec::path_of(Some(&i.brep))?;
        let block = spec::brep_block_of(Some(&i.brep), false, false)?;
        let initial = ffi::eval(request(&i, &path, &block), true).map_err(|e| error(&e))?;
        rust(&i, &initial)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = *[1e-6, 1.0, 1e6].get(rng.below(3) as usize)?;
        let origin = rng.point3(-scale, scale);
        let a: f64 = *[0.0, -7.0, 1000.0].get(rng.below(3) as usize)?;
        let width: f64 = *[1.0, 2.5, 10.0].get(rng.below(3) as usize)?;
        let t0 = a + 0.125 * width;
        let t1 = a + 0.875 * width;
        let p = *[
            t0 - 0.1 * width,
            t0,
            t0 + 1e-13 * (1.0 + t0.abs()),
            t0 + 0.001 * width,
            t0 + 0.5 * (t1 - t0),
            t1 - 0.001 * width,
            t1,
            t1 + 0.1 * width,
        ]
        .get(rng.below(8) as usize)?;
        let wire = json!({"kind":"wire","curve":{"dim":3,"cpts":[[origin[0],origin[1],origin[2],1.0],
            [origin[0]+scale,origin[1]+0.5*scale,origin[2],1.0]],"p":1,"u":[a,a,a+width,a+width]},"t0":t0,"t1":t1});
        Some(
            json!({"op":4,"brep":wire,"variant":rng.below(4),"parameter":p,
            "editing":rng.below(10)!=0,"tolerances":[scale* *[0.0,0.0001,0.1,2.0].get(rng.below(4) as usize)?,scale* *[0.0,0.0001,0.1,2.0].get(rng.below(4) as usize)?]}),
        )
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &CaseContext {
                root: ctx.root.clone(),
                seeds: ctx.seeds.max(512),
            },
        )
    }
}
