//! Runner for unit `query.brep`.
//!
//! C++: `solidcore_ffi::units::query_brep` (the public `SM_API` entry points). Rust:
//! `solidcore::query::brep`.
//!
//! An `evaluate` case is an entry point (`point`, `normal` or `derivatives`), an optional
//! surface (`null` for a `NULL` surface), a parameter `uv` (entries may be `"NaN"`, `"inf"`
//! or `"-inf"`) and the values the outputs hold before the call. Both sides produce the
//! status (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`) and
//! `P Du Dv` as the entry point leaves them: the sentinels where it does not write them
//! (upstream evaluates into locals and writes the outputs only on success).
//!
//! An `attribute` case is a decimation attribute class (`decimate_vertex`,
//! `quadric_vertex` or `quadric_edge`), its id, an optional behaviour (`null` for the
//! constructor's default) and optional data written before the copy, laid out as the
//! ffi module documents. Both sides produce `id behavior type data` of the constructed
//! attribute, then of its `MakeCopy` (the quadric edge's sort index NaN while unset).
//!
//! A `box` case is a B-rep (unit `topology.edgeuse_p1`'s B-rep block, as in the
//! `query.face_p1` runner: the C++ side builds a real `SmBrep`, the Rust side imports its
//! export) and up to [`MAX_BOX_STEPS`] steps run on it in order: `box` (the arguments of
//! `CalculateBoundingBox`, or its defaults), `tight`, `lock` and `unlock`. Both sides
//! produce every step's status and box (seeded, so what a failing call leaves is
//! compared). A B-rep with a face surface or edge curve the export carries only as its
//! B-spline part is deferred, as in that runner.
//!
//! A `classify` case is such a B-rep, a point (a vertex's point, a face surface point at
//! fractions of the face's UV domain, or given coordinates; plus an offset, computed once
//! from the import for both sides), a tolerance and `do_bnd`. Both sides produce the
//! status of `SmBrep::Point3DClassify` and the classification (class, object kind and
//! index, gap, source tolerance, pre-snap parameter, curve and surface parameters,
//! parameters-set flag, face kind and index); on failure the classification as the
//! call starts it. The Rust side builds the B-rep cache with
//! `topology::brep_cache::make_brep_cache` over the import's arena identities.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::kernel::Kernel;
use solidcore::topology::{Brep, BrepId, FaceId, VertexId};
use solidcore::{EntityRef, SolidError};

use solidcore::geom::attribute::{Attribute, AttributeValue, Behavior};
use solidcore::math::{Point2, Vec3};
use solidcore::query::brep::{self as qb, PolyVertexClass};
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::query_brep::{
    self as ffi, AttrKind, BoxStep, EvalOp, CLASSIFICATION_LEN, MAX_BOX_STEPS, SEED_BOX,
};

use super::surface_surface_cache_p1::harness;
use super::surface_surface_cache_p3::{gen_surf, push_surf, surf_domain, surface_in, SurfIn};
use super::query_face_p1::{everything, gen_face_brep, inexact, model, Model, NoGeometry};
use super::surface_surface_p7::{st, Fail};
use super::topology_edgeuse_p1::{fixture_input, gen_brep, BrepIn};
use crate::case::CaseContext;
use crate::util::flag;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `query.brep` runner.
pub struct Runner;

/// `SM_SUCCESS` as an output status.
const SUCCESS: f64 = 0.0;
/// The output of a case whose selected entity kind the B-rep does not have.
const NO_ENTITY: f64 = -2.0;
/// Where `P`, `Du` and `Dv` start in an evaluation output (after the status).
const P_AT: usize = 0;
const DU_AT: usize = 3;
const DV_AT: usize = 6;

/// The object kinds of a classification output (as the shim's `put_object`) and the index
/// of no object.
mod object_kind {
    pub const NONE: f64 = 0.0;
    pub const FACE: f64 = 1.0;
    pub const EDGE: f64 = 2.0;
    pub const VERTEX: f64 = 3.0;
    pub const REGION: f64 = 4.0;
    pub const OTHER: f64 = -3.0;
    pub const NO_INDEX: f64 = -1.0;
}

// ---- inputs -----------------------------------------------------------------------

/// The evaluation entry point of an `evaluate` case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum What {
    Point,
    Normal,
    Derivatives,
}

impl What {
    fn ffi(self) -> EvalOp {
        match self {
            Self::Point => EvalOp::Point,
            Self::Normal => EvalOp::Normal,
            Self::Derivatives => EvalOp::Derivatives,
        }
    }
}

/// An `evaluate` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalIn {
    what: What,
    surf: Option<SurfIn>,
    #[serde(with = "non_finite2")]
    uv: [f64; 2],
    /// `P Du Dv` before the call.
    sentinel: [f64; 9],
}

/// The class of an `attribute` case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    DecimateVertex,
    QuadricVertex,
    QuadricEdge,
}

impl Kind {
    fn ffi(self) -> AttrKind {
        match self {
            Self::DecimateVertex => AttrKind::DecimateVertex,
            Self::QuadricVertex => AttrKind::QuadricVertex,
            Self::QuadricEdge => AttrKind::QuadricEdge,
        }
    }
}

/// An `attribute` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AttrIn {
    kind: Kind,
    id: u64,
    behavior: Option<u8>,
    /// The data written before the copy (see the ffi module, without the sort index):
    /// `class distance error N`, `Q0 Q1 Q2 b c` or `P error`.
    data: Option<Vec<f64>>,
    /// The sort index written with the data (decimate vertex and quadric edge).
    #[serde(default)]
    sort: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Evaluate(EvalIn),
    Attribute(AttrIn),
    Box(BoxIn),
    Classify(ClassifyIn),
}

/// Where the point of a `classify` case is.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
enum At {
    /// The point of vertex `index` (modulo the vertex count).
    Vertex { index: usize },
    /// The surface point of face `index` (modulo the face count) at fractions `u`, `v`
    /// of its UV domain.
    Face { index: usize, u: f64, v: f64 },
    /// Given coordinates.
    Point { p: [f64; 3] },
}

/// A `classify` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassifyIn {
    brep: Value,
    #[serde(flatten)]
    at: At,
    #[serde(default)]
    offset: [f64; 3],
    tol: f64,
    do_bnd: bool,
}

/// A step of a `box` case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
enum StepIn {
    Box {
        approx: bool,
        tight: bool,
        expand: bool,
        #[serde(default)]
        defaults: bool,
    },
    Tight,
    Lock {
        tight: bool,
        #[serde(default)]
        defaults: bool,
    },
    Unlock,
}

impl StepIn {
    fn ffi(self) -> BoxStep {
        match self {
            Self::Box {
                approx,
                tight,
                expand,
                defaults,
            } => BoxStep::Box {
                approx,
                tight,
                expand,
                defaults,
            },
            Self::Tight => BoxStep::Tight,
            Self::Lock { tight, defaults } => BoxStep::Lock { tight, defaults },
            Self::Unlock => BoxStep::Unlock,
        }
    }
}

/// A `box` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BoxIn {
    brep: Value,
    steps: Vec<StepIn>,
}

/// A `[f64; 2]` whose entries may be `"NaN"`, `"inf"` or `"-inf"` (JSON has no
/// non-finite numbers).
mod non_finite2 {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    use crate::json::num;

    pub fn serialize<S: Serializer>(v: &[f64; 2], s: S) -> Result<S::Ok, S::Error> {
        v.map(num).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f64; 2], D::Error> {
        let float = |v: &Value| match v {
            Value::Number(n) => n.as_f64().ok_or_else(|| D::Error::custom(format!("bad number {n}"))),
            Value::String(s) if s == "NaN" => Ok(f64::NAN),
            Value::String(s) if s == "inf" => Ok(f64::INFINITY),
            Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
            other => Err(D::Error::custom(format!("bad float {other}"))),
        };
        let [a, b] = <[Value; 2]>::deserialize(d)?;
        Ok([float(&a)?, float(&b)?])
    }
}

// ---- C++ side -----------------------------------------------------------------------

fn eval_input(i: &EvalIn) -> Result<Vec<f64>, HarnessError> {
    let mut o = vec![flag(i.surf.is_some())];
    if let Some(s) = &i.surf {
        push_surf(&mut o, s)?;
    }
    o.extend(i.uv);
    o.extend(i.sentinel);
    Ok(o)
}

fn eval_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("p_du_dv", rest.to_vec())
}

fn cpp_evaluate(i: &EvalIn) -> Result<Output, HarnessError> {
    let flat = ffi::evaluate(i.what.ffi(), &eval_input(i)?).map_err(|e| harness(e.to_string()))?;
    Ok(eval_output(&flat))
}

/// An `ULONG` as its exact 32-bit halves `hi lo`.
fn halves(v: u64) -> [f64; 2] {
    [(v >> 32) as f64, (v & 0xffff_ffff) as f64]
}

fn attr_input(i: &AttrIn) -> Vec<f64> {
    let mut o = halves(i.id).to_vec();
    o.extend([
        flag(i.behavior.is_some()),
        f64::from(i.behavior.unwrap_or(0)),
        flag(i.data.is_some()),
    ]);
    if let Some(d) = &i.data {
        o.extend(d);
        if !matches!(i.kind, Kind::QuadricVertex) {
            o.push(flag(i.sort.is_some()));
            o.extend(halves(i.sort.unwrap_or(0)));
        }
    }
    o
}

fn cpp_attribute(i: &AttrIn) -> Result<Output, HarnessError> {
    let flat = ffi::attribute(i.kind.ffi(), &attr_input(i)).map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("attributes", flat))
}

fn cpp_box(i: &BoxIn) -> Result<Output, HarnessError> {
    let (path, block) = fixture_input(i.brep.clone())?;
    let steps: Vec<BoxStep> = i.steps.iter().map(|s| s.ffi()).collect();
    let flat = ffi::box_steps(&path, &block, &steps).map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("steps", flat))
}

/// The classified point of a `classify` case on the import; `None` when the selected
/// kind has no entity.
fn classify_point(m: &Model, i: &ClassifyIn) -> Result<Option<[f64; 3]>, HarnessError> {
    let b = &m.brep;
    let p = match &i.at {
        At::Vertex { index } => {
            let n = b.vertices().len();
            if n == 0 {
                return Ok(None);
            }
            let v = VertexId::from_index((index % n) as u32);
            b.vertex(v).map_err(|e| harness(e.to_string()))?.point
        }
        At::Face { index, u, v } => {
            let n = b.faces().len();
            if n == 0 {
                return Ok(None);
            }
            let f = b
                .face(FaceId::from_index((index % n) as u32))
                .map_err(|e| harness(e.to_string()))?;
            let (lo, size) = (f.uv_domain.min(), f.uv_domain.size());
            let uv = Point2::new(lo.x + u * size.x, lo.y + v * size.y);
            f.surface
                .as_ref()
                .ok_or_else(|| harness("face without surface"))?
                .evaluate_point(&Kernel::default(), uv)
                .map_err(|e| harness(format!("surface point: {e}")))?
        }
        At::Point { p } => Vec3::new(p[0], p[1], p[2]),
    };
    let [dx, dy, dz] = i.offset;
    Ok(Some([p.x + dx, p.y + dy, p.z + dz]))
}

fn cpp_classify(i: &ClassifyIn) -> Result<Output, HarnessError> {
    let m = model(&i.brep)?;
    let Some(p) = classify_point(&m, i)? else {
        return Ok(Output::new().values("classify", vec![NO_ENTITY]));
    };
    let flat = ffi::classify(&m.path, &m.block, p, i.tol, i.do_bnd)
        .map_err(|e| harness(e.to_string()))?;
    Ok(Output::new().values("classify", flat))
}

// ---- Rust side ----------------------------------------------------------------------

fn rust_evaluate(i: &EvalIn) -> Result<Output, Fail> {
    let surface: Option<Surface> = i.surf.as_ref().map(surface_in).transpose()?;
    let uv = Point2::new(i.uv[0], i.uv[1]);
    let k = Kernel::default();
    let mut out = i.sentinel.to_vec();
    let mut put = |at: usize, v: Vec3| {
        if let Some(slot) = out.get_mut(at..at + 3) {
            slot.copy_from_slice(&[v.x, v.y, v.z]);
        }
    };
    let code = match i.what {
        What::Point => {
            st(qb::evaluate_surface_point(&k, surface.as_ref(), uv))?.map(|p| put(P_AT, p))
        }
        What::Normal => {
            st(qb::evaluate_surface_normal(&k, surface.as_ref(), uv))?.map(|n| put(P_AT, n))
        }
        What::Derivatives => st(qb::evaluate_surface_derivatives(&k, surface.as_ref(), uv))?.map(
            |(p, du, dv)| {
                put(P_AT, p);
                put(DU_AT, du);
                put(DV_AT, dv);
            },
        ),
    }
    .map_or_else(|code| code, |()| SUCCESS);
    let mut flat = vec![code];
    flat.extend(out);
    Ok(eval_output(&flat))
}

/// Reads the `at`th entry of a data record.
fn datum(d: &[f64], at: usize) -> Result<f64, HarnessError> {
    d.get(at)
        .copied()
        .ok_or_else(|| harness("attribute data too short"))
}

fn vec_at(d: &[f64], at: usize) -> Result<Vec3, HarnessError> {
    Ok(Vec3::new(datum(d, at)?, datum(d, at + 1)?, datum(d, at + 2)?))
}

/// Writes a data record and sort index into the attribute (as the shim's `set_data`).
fn set_data(a: &mut Attribute, d: &[f64], sort: Option<u64>) -> Result<(), HarnessError> {
    match a.value_mut() {
        AttributeValue::DecimateVertex(v) => {
            let class = datum(d, 0)?;
            v.vertex_class = PolyVertexClass::ALL
                .get(class as usize)
                .copied()
                .ok_or_else(|| harness(format!("bad vertex class {class}")))?;
            v.vertex_distance = datum(d, 1)?;
            v.max_face_error = datum(d, 2)?;
            v.average_plane_normal = vec_at(d, 3)?;
            if let Some(s) = sort {
                v.sort_index = s;
            }
        }
        AttributeValue::QuadricVertex(v) => {
            v.q = [vec_at(d, 0)?, vec_at(d, 3)?, vec_at(d, 6)?];
            v.b = vec_at(d, 9)?;
            v.c = datum(d, 12)?;
        }
        AttributeValue::QuadricEdge(v) => {
            v.new_point = vec_at(d, 0)?;
            v.error = datum(d, 3)?;
            if sort.is_some() {
                v.sort_index = sort;
            }
        }
        _ => return Err(harness("not a decimation attribute")),
    }
    Ok(())
}

fn push_attribute(o: &mut Vec<f64>, a: &Attribute) -> Result<(), HarnessError> {
    let pv = |o: &mut Vec<f64>, v: Vec3| o.extend([v.x, v.y, v.z]);
    o.extend(halves(a.attribute_id()));
    o.extend([f64::from(a.behavior().code()), a.type_code() as f64]);
    match a.value() {
        AttributeValue::DecimateVertex(v) => {
            o.extend(halves(v.sort_index));
            o.extend([
                f64::from(v.vertex_class as u8),
                v.vertex_distance,
                v.max_face_error,
            ]);
            pv(o, v.average_plane_normal);
        }
        AttributeValue::QuadricVertex(v) => {
            for r in v.q {
                pv(o, r);
            }
            pv(o, v.b);
            o.push(v.c);
        }
        AttributeValue::QuadricEdge(v) => {
            pv(o, v.new_point);
            o.push(v.error);
            o.extend(v.sort_index.map_or([f64::NAN; 2], halves));
        }
        _ => return Err(harness("not a decimation attribute")),
    }
    Ok(())
}

fn rust_attribute(i: &AttrIn) -> Result<Output, HarnessError> {
    let behavior = match i.behavior {
        Some(b) => Behavior::from_code(i32::from(b)).map_err(|e| harness(e.to_string()))?,
        // the constructors' default argument
        None => Behavior::Copy,
    };
    let mut a = match i.kind {
        Kind::DecimateVertex => qb::decimate_vertex_attribute(i.id, behavior),
        Kind::QuadricVertex => qb::quadric_vertex_attribute(i.id, behavior),
        Kind::QuadricEdge => qb::quadric_edge_attribute(i.id, behavior),
    };
    if let Some(d) = &i.data {
        set_data(&mut a, d, i.sort)?;
    }
    let c = a.make_copy();
    let mut o = Vec::new();
    push_attribute(&mut o, &a)?;
    push_attribute(&mut o, &c)?;
    Ok(Output::new().values("attributes", o))
}

/// The sentinel box the shim seeds every box with.
fn seed_box() -> Extent3d {
    let [x0, y0, z0, x1, y1, z1] = SEED_BOX;
    Extent3d::from_points(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1))
}

/// The `SmStatus` of a Rust result; `None` for an error upstream has no status for.
fn status_code(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
        _ => None,
    }
}

/// Appends a box step's status and box (the seed when it failed).
fn push_box(o: &mut Vec<f64>, r: solidcore::Result<Extent3d>) -> Result<(), Fail> {
    let (status, bx) = match r {
        Ok(bx) => (SUCCESS, bx),
        Err(e) => match Output::unported(&e) {
            Some(d) => return Err(Fail::Deferred(Box::new(d))),
            None => (
                status_code(&e).ok_or_else(|| harness(format!("rust error: {e}")))?,
                seed_box(),
            ),
        },
    };
    let (lo, hi) = (bx.min(), bx.max());
    o.extend([status, lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]);
    Ok(())
}

fn run_steps(k: &Kernel, b: &mut Brep, steps: &[StepIn]) -> Result<Vec<f64>, Fail> {
    let mut o = Vec::new();
    for s in steps {
        match *s {
            StepIn::Box {
                approx,
                tight,
                expand,
                defaults,
            } => {
                let opts = if defaults {
                    qb::BoundingBoxOptions::default()
                } else {
                    qb::BoundingBoxOptions {
                        approx,
                        tight,
                        expand_by_zone_tol_3d: expand,
                    }
                };
                push_box(&mut o, qb::calculate_bounding_box(k, &mut *b, opts))?;
            }
            StepIn::Tight => push_box(&mut o, qb::calculate_tight_bounding_box(k, &mut *b))?,
            StepIn::Lock { tight, defaults } => {
                // the default argument is `FALSE`
                let r = qb::lock_bounding_box(k, b, tight && !defaults);
                o.push(match st(r)? {
                    Ok(()) => SUCCESS,
                    Err(code) => code,
                });
            }
            StepIn::Unlock => {
                qb::unlock_bounding_box(b);
                o.push(SUCCESS);
            }
        }
    }
    Ok(o)
}

fn rust_box(i: &BoxIn) -> Result<Output, Fail> {
    if i.steps.is_empty() || i.steps.len() > MAX_BOX_STEPS {
        return Err(Fail::Harness(harness("box steps: count")));
    }
    let mut m = model(&i.brep)?;
    let (faces, edges) = everything(&m.brep);
    if let Some(o) = inexact(&m, &faces, &edges) {
        return Ok(o);
    }
    let k = Kernel::default();
    let flat = run_steps(&k, &mut m.brep, &i.steps)?;
    Ok(Output::new().values("steps", flat))
}

/// `class kind index gap src_tol presnap tparam u v param_set face_kind face_index` (as
/// the shim's `put_pc`).
fn pc_block(pc: &PointClassification<EntityRef>) -> Vec<f64> {
    let object = |o: Option<EntityRef>| match o {
        None => (object_kind::NONE, object_kind::NO_INDEX),
        Some(EntityRef::Face(f)) => (object_kind::FACE, f64::from(f.index())),
        Some(EntityRef::Edge(e)) => (object_kind::EDGE, f64::from(e.index())),
        Some(EntityRef::Vertex(v)) => (object_kind::VERTEX, f64::from(v.index())),
        Some(EntityRef::Region(r)) => (object_kind::REGION, f64::from(r.index())),
        Some(_) => (object_kind::OTHER, object_kind::NO_INDEX),
    };
    let (kind, index) = object(pc.object());
    let (face_kind, face_index) = object(pc.face());
    let uv = pc.uv_param();
    vec![
        f64::from(pc.point_class().code()),
        kind,
        index,
        pc.gap_3d(&Kernel::default(), &NoGeometry, None)
            .unwrap_or(f64::NAN),
        pc.src_zone_tol_3d(),
        pc.pre_snap_param(),
        pc.t_param(),
        uv.x,
        uv.y,
        flag(pc.are_parameters_set()),
        face_kind,
        face_index,
    ]
}

fn rust_classify(i: &ClassifyIn) -> Result<Output, Fail> {
    let m = model(&i.brep)?;
    let Some(p) = classify_point(&m, i)? else {
        return Ok(Output::new().values("classify", vec![NO_ENTITY]));
    };
    let (faces, edges) = everything(&m.brep);
    if let Some(o) = inexact(&m, &faces, &edges) {
        return Ok(o);
    }
    let k = Kernel::default();
    let mut b = m.brep;
    crate::util::own_arena_geometry(&mut b);
    let query = qb::BrepQuery::new(qb::BrepIdentity::arena(BrepId::from_index(0)));
    let tol = zone_tol3d(&k, i.tol);
    let r = query.point_3d_classify(&k, &mut b, Vec3::new(p[0], p[1], p[2]), tol, i.do_bnd);
    let (status, pc) = match r {
        Ok(pc) => (SUCCESS, pc),
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            let Some(code) = status_code(&e) else {
                return Ok(Output::err(format!("rust error: {e}")));
            };
            // as the call starts the classification
            let mut pc = PointClassification::new(tol);
            pc.set_class_object(PointClassType::Unknown, None);
            (code, pc)
        }
    };
    let mut flat = vec![status];
    flat.extend(pc_block(&pc));
    debug_assert_eq!(flat.len(), 1 + CLASSIFICATION_LEN);
    Ok(Output::new().values("classify", flat))
}

// ---- generation ---------------------------------------------------------------------

/// A value for a data field: mostly moderate, sometimes zero, huge, tiny or non-finite
/// (the attributes copy whatever they hold).
fn gen_value(rng: &mut Rng) -> f64 {
    match rng.below(16) {
        0 => 0.0,
        1 => -1e300,
        2 => 1e-300,
        3 => f64::MIN_POSITIVE / 4.0,
        _ => rng.uniform(-1e3, 1e3),
    }
}

/// An `ULONG`: the ends of the range, the `f64` precision limits, or a random one.
fn gen_ulong(rng: &mut Rng) -> u64 {
    match rng.below(8) {
        0 => 0,
        1 => u64::from(u32::MAX),
        2 => (1 << 53) + 1,
        3 => u64::MAX,
        4 => rng.below(1 << 20),
        _ => (rng.below(1 << 32) << 32) | rng.below(1 << 32),
    }
}

fn gen_attr(rng: &mut Rng) -> AttrIn {
    let kind = match rng.below(3) {
        0 => Kind::DecimateVertex,
        1 => Kind::QuadricVertex,
        _ => Kind::QuadricEdge,
    };
    let id = gen_ulong(rng);
    let behavior = rng.chance(0.7).then(|| rng.below(5) as u8);
    let data = rng.chance(0.75).then(|| match kind {
        Kind::DecimateVertex => {
            let mut d = vec![rng.below(6) as f64];
            d.extend((0..5).map(|_| gen_value(rng)));
            d
        }
        Kind::QuadricVertex => (0..13).map(|_| gen_value(rng)).collect(),
        Kind::QuadricEdge => (0..4).map(|_| gen_value(rng)).collect(),
    });
    let sort = rng.chance(0.5).then(|| gen_ulong(rng));
    AttrIn {
        kind,
        id,
        behavior,
        data,
        sort,
    }
}

fn gen_what(rng: &mut Rng) -> What {
    match rng.below(3) {
        0 => What::Point,
        1 => What::Normal,
        _ => What::Derivatives,
    }
}

/// A parameter for a surface over `d`: mostly inside, else on a corner or side, just
/// outside, or non-finite.
fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [u0, v0, u1, v1] = d;
    let inside = |rng: &mut Rng| [rng.uniform(u0, u1), rng.uniform(v0, v1)];
    match rng.below(12) {
        0 => [
            if rng.chance(0.5) { u0 } else { u1 },
            if rng.chance(0.5) { v0 } else { v1 },
        ],
        1 => {
            let [u, v] = inside(rng);
            if rng.chance(0.5) {
                [if rng.chance(0.5) { u0 } else { u1 }, v]
            } else {
                [u, if rng.chance(0.5) { v0 } else { v1 }]
            }
        }
        2 => {
            // just outside one bound
            let [u, v] = inside(rng);
            let du = (u1 - u0).abs().max(1.0) * 1e-9;
            let dv = (v1 - v0).abs().max(1.0) * 1e-9;
            match rng.below(4) {
                0 => [u0 - du, v],
                1 => [u1 + du, v],
                2 => [u, v0 - dv],
                _ => [u, v1 + dv],
            }
        }
        3 => {
            let bad = match rng.below(3) {
                0 => f64::NAN,
                1 => f64::INFINITY,
                _ => f64::NEG_INFINITY,
            };
            let [u, v] = inside(rng);
            if rng.chance(0.5) {
                [bad, v]
            } else {
                [u, bad]
            }
        }
        _ => inside(rng),
    }
}

fn gen_step(rng: &mut Rng) -> StepIn {
    match rng.below(10) {
        0..=4 => StepIn::Box {
            approx: rng.chance(0.3),
            tight: rng.chance(0.2),
            expand: rng.chance(0.7),
            defaults: rng.chance(0.3),
        },
        5 | 6 => StepIn::Tight,
        7 | 8 => StepIn::Lock {
            tight: rng.chance(0.3),
            defaults: rng.chance(0.3),
        },
        _ => StepIn::Unlock,
    }
}

fn gen_box(rng: &mut Rng) -> Option<Value> {
    let brep = gen_case_brep(rng);
    let n = 1 + rng.below(MAX_BOX_STEPS as u64) as usize;
    let steps = (0..n).map(|_| gen_step(rng)).collect();
    serde_json::to_value(Inputs::Box(BoxIn {
        brep: serde_json::to_value(&brep).ok()?,
        steps,
    }))
    .ok()
}

/// A closed box solid (two regions) with a random corner and size.
fn gen_box_solid(rng: &mut Rng) -> BrepIn {
    let size = *rng.choose(&[1e-2, 1.0, 1.0, 10.0, 1e3]).unwrap_or(&1.0);
    BrepIn::Box {
        origin: rng.point3(-10.0, 10.0),
        size: [
            size * rng.uniform(0.1, 1.0),
            size * rng.uniform(0.1, 1.0),
            size * rng.uniform(0.1, 1.0),
        ],
    }
}

/// Isolated vertices and wire line segments with their own tolerances; `large` is past
/// the approximation threshold of `SmBrep::CalculateBoundingBox` (500 vertices, 100
/// edges).
fn gen_points_brep(rng: &mut Rng, large: bool) -> BrepIn {
    let tol = |rng: &mut Rng| *rng.choose(&[0.0, 1e-7, 1e-5, 1e-3, 0.1]).unwrap_or(&1e-5);
    let (nv, ne) = if large {
        (300 + rng.below(250) as usize, 101 + rng.below(49) as usize)
    } else {
        (rng.below(6) as usize, 1 + rng.below(5) as usize)
    };
    let scale = *rng.choose(&[1e-3, 1.0, 10.0, 1e3]).unwrap_or(&1.0);
    let vertices = (0..nv)
        .map(|_| {
            let p = rng.point3(-scale, scale);
            [p[0], p[1], p[2], tol(rng)]
        })
        .collect();
    let lines = (0..ne)
        .map(|_| {
            let a = rng.point3(-scale, scale);
            let b = rng.point3(-scale, scale);
            [a[0], a[1], a[2], b[0], b[1], b[2], tol(rng)]
        })
        .collect();
    BrepIn::Points { vertices, lines }
}

/// A B-rep for a generated `box` or `classify` case.
fn gen_case_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(12) {
        0..=4 => gen_face_brep(rng),
        5..=7 => gen_brep(rng),
        8 | 9 => gen_box_solid(rng),
        10 => gen_points_brep(rng, false),
        _ => {
            let large = rng.chance(0.5);
            gen_points_brep(rng, large)
        }
    }
}

fn gen_classify(rng: &mut Rng) -> Option<Value> {
    let brep = gen_case_brep(rng);
    let frac = |rng: &mut Rng| match rng.below(6) {
        0 => 0.0,
        1 => 1.0,
        2 => 0.5,
        _ => rng.unit(),
    };
    let at = match rng.below(3) {
        0 => At::Vertex {
            index: rng.below(64) as usize,
        },
        1 => At::Face {
            index: rng.below(64) as usize,
            u: frac(rng),
            v: frac(rng),
        },
        _ if rng.chance(0.1) => At::Point {
            p: *rng
                .choose(&[[1e100, -1e100, 3e100], [1e-200, -1e-200, 3e-200], [1e6, -1e6, 3e6]])
                .unwrap_or(&[0.0; 3]),
        },
        _ => At::Point {
            p: match &brep {
                BrepIn::Box { origin, size } => {
                    let mut p = *origin;
                    for (x, s) in p.iter_mut().zip(size) {
                        *x += s * rng.uniform(-0.5, 1.5);
                    }
                    p
                }
                _ => rng.point3(-20.0, 20.0),
            },
        },
    };
    let scale = *rng
        .choose(&[0.0, 0.0, 1e-9, 1e-7, 1e-5, 1e-3, 0.1, 5.0])
        .unwrap_or(&0.0);
    let offset = [
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
    ];
    let tol = *rng
        .choose(&[0.0, 1e-10, 1e-8, 1e-6, 1e-4, 1e-2, 1e-6, 1e-6, -1e-12, -1.0, 100.0])
        .unwrap_or(&1e-6);
    serde_json::to_value(Inputs::Classify(ClassifyIn {
        brep: serde_json::to_value(&brep).ok()?,
        at,
        offset,
        tol,
        do_bnd: rng.chance(0.6),
    }))
    .ok()
}

fn gen_eval(rng: &mut Rng) -> Result<EvalIn, HarnessError> {
    let what = gen_what(rng);
    let mut sentinel = [0.0; 9];
    for s in &mut sentinel {
        *s = rng.uniform(-1e3, 1e3);
    }
    if rng.chance(0.03) {
        return Ok(EvalIn {
            what,
            surf: None,
            uv: [rng.unit(), rng.unit()],
            sentinel,
        });
    }
    let surf = gen_surf(rng);
    let uv = gen_uv(rng, surf_domain(&surf)?);
    Ok(EvalIn {
        what,
        surf: Some(surf),
        uv,
        sentinel,
    })
}

// ---- runner -------------------------------------------------------------------------

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.brep"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.chance(0.2) {
            let i = gen_attr(rng);
            return serde_json::to_value(Inputs::Attribute(i)).ok();
        }
        if rng.chance(0.4) {
            return if rng.chance(0.5) {
                gen_box(rng)
            } else {
                gen_classify(rng)
            };
        }
        for _ in 0..64 {
            let Ok(i) = gen_eval(rng) else {
                continue;
            };
            let Ok(input) = eval_input(&i) else {
                continue;
            };
            if ffi::validate_evaluate(&input).is_ok() {
                return serde_json::to_value(Inputs::Evaluate(i)).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        match case.parse::<Inputs>()? {
            Inputs::Evaluate(i) => cpp_evaluate(&i),
            Inputs::Attribute(i) => cpp_attribute(&i),
            Inputs::Box(i) => cpp_box(&i),
            Inputs::Classify(i) => cpp_classify(&i),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let r = match case.parse::<Inputs>()? {
            Inputs::Evaluate(i) => rust_evaluate(&i),
            Inputs::Attribute(i) => return rust_attribute(&i),
            Inputs::Box(i) => rust_box(&i),
            Inputs::Classify(i) => rust_classify(&i),
        };
        match r {
            Ok(o) => Ok(o),
            Err(Fail::Deferred(o)) => Ok(*o),
            Err(Fail::Harness(e)) => Err(e),
        }
    }
}

