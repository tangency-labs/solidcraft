//! Differential checks of B-rep pointer reports and coincidence pairs against the
//! unchanged c9c5979 C++ reference. The Rust side builds a [`BrepIn`] source itself
//! ([`crate::own_builder`], and the shim's `SmBrepConstructor` sheet grids through
//! [`sheet_grid`], in the shim's call order) and reads an `.smb` file with the Rust reader
//! ([`crate::own_reader::brep_in_order`]), both under this runner's conventions. Post-call curve samples compare the
//! reversal repairs and lazily created UV curves made by the checks themselves.
use super::topology_edgeuse_p1::{brep_block_of, import_raw_with_types, path_of, BrepIn};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::assert_array::{AssertArray, ObjectKey as ReportKey};
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::validation;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::edgeuse::p1 as eu1p;
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, NamingScheme, Shell, ShellId, Vertex, VertexId,
    Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, Kernel, SolidError};
use solidcore_ffi::units::topology_brep_validation as ffi;

/// Native B-rep validation runner.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default = "empty_brep")]
    brep: BrepIn,
    #[serde(default)]
    grid_faces: Option<usize>,
    #[serde(default)]
    shared_boundaries: Option<u8>,
    #[serde(default)]
    coincidence: bool,
    #[serde(default = "level_two")]
    level: u8,
    #[serde(default = "yes")]
    walk: bool,
    #[serde(default = "default_tol")]
    tolerance: f64,
    #[serde(default = "yes")]
    find_all: bool,
    #[serde(default)]
    mutation: u8,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    value: f64,
}
fn empty_brep() -> BrepIn {
    BrepIn::Vertices { points: Vec::new() }
}
fn yes() -> bool {
    true
}
fn level_two() -> u8 {
    2
}
fn default_tol() -> f64 {
    -1.0
}
fn herr(e: impl Into<String>) -> HarnessError {
    HarnessError::new(e)
}
fn info(e: EntityRef) -> (i64, u32) {
    match e {
        EntityRef::Brep(x) => (16011, x.index()),
        EntityRef::Region(x) => (16012, x.index()),
        EntityRef::Shell(x) => (16013, x.index()),
        EntityRef::Face(x) => (16002, x.index()),
        EntityRef::Faceuse(x) => (16022, x.index()),
        EntityRef::Loop(x) => (16003, x.index()),
        EntityRef::Loopuse(x) => (16004, x.index()),
        EntityRef::Edge(x) => (16016, x.index()),
        EntityRef::Edgeuse(x) => (16005, x.index()),
        EntityRef::Vertex(x) => (16021, x.index()),
        EntityRef::Vertexuse(x) => (16006, x.index()),
    }
}
fn notify_key(t: i64, i: u32) -> ObjectRef {
    let kind = match t {
        16011 => 0,
        16012 => 1,
        16013 => 2,
        16022 => 3,
        16002 => 4,
        16004 => 5,
        16003 => 6,
        16005 => 7,
        16016 => 8,
        16006 => 9,
        16021 => 10,
        _ => 11,
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}
fn key(e: EntityRef) -> ObjectRef {
    let (t, i) = info(e);
    notify_key(t, i)
}
struct Reader<'a> {
    data: &'a [f64],
    pos: usize,
}
impl Reader<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = *self
            .data
            .get(self.pos)
            .ok_or_else(|| herr("short validation protocol"))?;
        self.pos += 1;
        Ok(x)
    }
    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.num()?;
        if !x.is_finite() || x < 0.0 || x.fract() != 0.0 || x >= usize::MAX as f64 {
            return Err(herr("bad validation count"));
        }
        Ok(x as usize)
    }
    fn slice(&mut self, n: usize) -> Result<&[f64], HarnessError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| herr("protocol overflow"))?;
        let x = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| herr("short validation protocol"))?;
        self.pos = end;
        Ok(x)
    }
    fn list(&mut self) -> Result<Vec<usize>, HarnessError> {
        let n = self.count()?;
        let values = self.slice(n)?;
        values
            .iter()
            .map(|&x| {
                if x.is_finite() && x >= 0.0 && x.fract() == 0.0 && x < usize::MAX as f64 {
                    Ok(x as usize)
                } else {
                    Err(herr("bad list index"))
                }
            })
            .collect()
    }
    fn owner(&mut self) -> Result<Option<ObjectRef>, HarnessError> {
        let t = self.num()? as i64;
        let i = self.num()? as i64;
        if t < 0 {
            return Ok(None);
        }
        Ok(Some(notify_key(t, i as u32)))
    }
}
fn args(i: &Inputs) -> [f64; 7] {
    [
        f64::from(i.level),
        f64::from(i.walk),
        i.tolerance,
        f64::from(i.find_all),
        f64::from(i.mutation),
        i.index as f64,
        i.value,
    ]
}
/// The fixture path and block the shim builds the B-rep from: a sheet grid
/// (`shared_boundaries`, then `grid_faces`, as the shim's dispatch reads them) or a file.
fn native_input(i: &Inputs) -> Result<(String, Vec<f64>), HarnessError> {
    if let Some(mode) = i.shared_boundaries {
        return Ok((String::new(), vec![101.0, f64::from(mode)]));
    }
    if let Some(count) = i.grid_faces {
        return Ok((String::new(), vec![100.0, count as f64]));
    }
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), false, false)?;
    Ok((path, block))
}
fn exported(i: &Inputs) -> Result<(String, Vec<f64>, Vec<f64>), HarnessError> {
    let (path, block) = native_input(i)?;
    let export = ffi::eval(0, &path, &block, args(i)).map_err(|e| herr(e.to_string()))?;
    Ok((path, block, export))
}
/// The C++ side's operation: pointer validation (1) or the coincidence search (2).
fn op(i: &Inputs) -> u8 {
    if i.coincidence {
        2
    } else {
        1
    }
}
fn imported(export: &[f64]) -> Result<Brep, HarnessError> {
    let mut rd = Reader {
        data: export,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Err(herr("native fixture failed"));
    }
    let n = rd.count()?;
    let graph = rd.slice(n)?;
    let (mut b, types) = import_raw_with_types(graph)?;
    if (0..b.edges().len()).any(|e| types.inexact_curve(e).is_some())
        || (0..b.faces().len()).any(|f| types.inexact_surface(f).is_some())
    {
        return Err(herr("validation fixture needs exact geometry import"));
    }
    b.set_face_list(
        rd.list()?
            .into_iter()
            .map(|i| FaceId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    b.set_edge_list(
        rd.list()?
            .into_iter()
            .map(|i| EdgeId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    b.set_vertex_list(
        rd.list()?
            .into_iter()
            .map(|i| VertexId::from_index(i as u32))
            .collect(),
    )
    .map_err(|e| herr(e.to_string()))?;
    for index in 0..b.faces().len() {
        let f = FaceId::from_index(index as u32);
        let fu = rd.num()? as i64;
        b.face_mut(f).map_err(|e| herr(e.to_string()))?.faceuse =
            u32::try_from(fu).ok().map(FaceuseId::from_index);
        let owner = rd.owner()?;
        if let Some(s) = b
            .face_mut(f)
            .map_err(|e| herr(e.to_string()))?
            .surface
            .as_mut()
        {
            s.set_owner(owner);
        }
        b.sync_face_surface_tolerance(f)
            .map_err(|e| herr(e.to_string()))?;
    }
    for index in 0..b.edges().len() {
        let owner = rd.owner()?;
        if let Some(c) = b
            .edge_mut(EdgeId::from_index(index as u32))
            .map_err(|e| herr(e.to_string()))?
            .curve
            .as_mut()
        {
            c.set_owner(owner, None);
        }
    }
    b.set_naming(NamingScheme::Notify);
    if rd.pos != export.len() {
        return Err(herr("trailing export metadata"));
    }
    Ok(b)
}
fn own_face_owner(f: FaceId) -> ObjectRef {
    key(EntityRef::Face(f))
}
fn own_edgeuse_owner(eu: EdgeuseId) -> ObjectRef {
    key(EntityRef::Edgeuse(eu))
}
fn own_edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    (
        key(EntityRef::Edge(e)),
        key(EntityRef::Brep(solidcore::topology::BrepId::from_index(0))),
    )
}
/// The shim's build conventions (`sc_edgeuse_graph.h`), with the geometry owned under
/// this runner's notification identities ([`key`]), as [`imported`] names it.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    prefix_lists: false,
    vertex_edge_gaps: false,
    vertex_lists_last: false,
    skip_edge_list: false,
    face_owner: &own_face_owner,
    edge_owner: &own_edge_owner,
    edgeuse_owner: &own_edgeuse_owner,
    // the runner records `Notify` itself, after the build
    naming: NamingScheme::Unknown,
};
/// The Rust B-rep of a [`BrepIn`] source built by the Rust kernel
/// ([`crate::own_builder`], and the `SmBrepConstructor` sheet grids `grid_faces` and
/// `shared_boundaries` through [`sheet_grid`], in the shim's call order) or, for an `.smb`
/// file, read by the Rust reader and renumbered into the shim's traversal order
/// ([`crate::own_reader::brep_in_order`]): native lists, face uses and geometry owners
/// under [`CONVENTIONS`], `Notify` naming as [`imported`] sets it. A failed build is the
/// shim's failed construction; a failed read is a Rust divergence, reported as an error.
fn own_brep(i: &Inputs) -> Result<Option<Result<Brep, Output>>, HarnessError> {
    use crate::own_builder::{finish, in_new_brep, Built};
    let mut k = Kernel::default();
    // the sheet grids (`shared_boundaries` first, as the shim's dispatch reads it)
    let grid = match (i.shared_boundaries, i.grid_faces) {
        (Some(mode), _) => Some((2, mode)),
        (None, Some(count)) => Some((count, 0)),
        (None, None) => None,
    };
    let file = matches!(i.brep, BrepIn::File { .. });
    let built = match grid {
        Some((count, shared)) => {
            let raw = in_new_brep(&mut k, |k, b| sheet_grid(k, b, count, shared))?;
            finish(&mut k, raw, false, false, &CONVENTIONS)?
        }
        None => crate::own_reader::brep_in_order(&mut k, &i.brep, false, false, &CONVENTIONS)?,
    };
    Ok(Some(match built {
        Built::Brep(b) => {
            let mut b = *b;
            b.set_naming(NamingScheme::Notify);
            Ok(b)
        }
        Built::Failed(e) => Err(Output::unported(&e).unwrap_or_else(|| {
            if file {
                Output::err(format!("rust read failed: {e}"))
            } else {
                Output::rejected("native fixture construction failed")
            }
        })),
    }))
}
fn orient(v: f64) -> OrientType {
    match v as u8 {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}
fn mutate(b: &mut Brep, i: &Inputs) -> Result<(), HarnessError> {
    let n = match i.mutation {
        1 | 6 => b.vertices().len(),
        2 | 7 | 9 => b.edges().len(),
        3 | 8 => b.faces().len(),
        4 => b.faceuses().len(),
        5 => b.loopuses().len(),
        10 => b.edgeuses().len(),
        _ => 0,
    };
    if n == 0 {
        return Ok(());
    }
    let index = (i.index % n) as u32;
    match i.mutation {
        1 => {
            b.vertex_mut(VertexId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .tolerance = i.value;
        }
        2 => {
            b.edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .tolerance = i.value;
        }
        3 => {
            let f = FaceId::from_index(index);
            b.face_mut(f).map_err(|e| herr(e.to_string()))?.tolerance = i.value;
            b.sync_face_surface_tolerance(f)
                .map_err(|e| herr(e.to_string()))?;
        }
        4 => {
            b.faceuse_mut(FaceuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .orientation = orient(i.value);
        }
        5 => {
            b.loopuse_mut(LoopuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .orientation = orient(i.value);
        }
        6 => {
            b.vertex_mut(VertexId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .point
                .x += i.value;
        }
        7 => {
            if let Some(c) = b
                .edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .curve
                .as_mut()
            {
                c.set_owner(None, None);
            }
        }
        8 => {
            b.face_mut(FaceId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .surface = None;
        }
        9 => {
            b.edge_mut(EdgeId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .curve = None;
        }
        10 => {
            b.edgeuse_mut(EdgeuseId::from_index(index))
                .map_err(|e| herr(e.to_string()))?
                .ccw = None;
        }
        _ => {}
    }
    Ok(())
}
fn text(out: &mut Vec<f64>, s: Option<&str>) {
    out.push(s.map_or(0, str::len) as f64);
    if let Some(s) = s {
        out.extend(s.bytes().map(f64::from));
    }
}
fn report_key(out: &mut Vec<f64>, k: Option<ReportKey>) {
    if let Some(k) = k {
        out.extend([(k.0 >> 32) as f64, (k.0 & 0xffff_ffff) as f64]);
    } else {
        out.extend([-1.0, -1.0]);
    }
}
fn rust_protocol(
    b: &Brep,
    status: i32,
    pairs: &[(EntityRef, EntityRef)],
    reports: &AssertArray,
) -> Result<Vec<f64>, HarnessError> {
    let mut out = vec![0.0, f64::from(status), pairs.len() as f64];
    for &(a, c) in pairs {
        let (t, i) = info(a);
        let (u, j) = info(c);
        out.extend([t as f64, f64::from(i), u as f64, f64::from(j)]);
    }
    out.push(reports.len() as f64);
    for r in reports.reports() {
        out.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            f64::from(r.ok),
        ]);
        report_key(&mut out, r.reporter);
        report_key(&mut out, r.owner);
        report_key(&mut out, r.other);
        out.extend([r.owner_type as f64, r.other_type as f64, r.tol3d, r.value]);
        text(&mut out, Some(r.name));
        text(&mut out, r.message.as_deref());
        text(&mut out, r.message_for_file);
        text(&mut out, r.owner_type_string);
        text(&mut out, r.other_type_string);
    }
    out.push(f64::from(reports.last_add()));
    out.push((b.edges().len() + b.edgeuses().len()) as f64);
    let kernel = Kernel::default();
    for curve in b
        .edges()
        .iter()
        .map(|(_, e)| e.curve.as_ref())
        .chain(b.edgeuses().iter().map(|(_, eu)| eu.uv_trim_curve.as_ref()))
    {
        let Some(curve) = curve else {
            out.push(-1.0);
            continue;
        };
        out.push(curve.type_code() as f64);
        let domain = curve.natural_interval();
        out.extend([domain.min(), domain.max()]);
        for i in 0..5 {
            match curve.evaluate_point(&kernel, domain.evaluate(f64::from(i) / 4.0)) {
                Ok(p) => out.extend([1000.0, p.x, p.y, p.z]),
                Err(e) if e.is_unported() => return Err(herr(e.to_string())),
                Err(_) => out.push(1001.0),
            }
        }
    }
    Ok(out)
}
fn output(protocol: &[f64]) -> Result<Output, HarnessError> {
    let mut rd = Reader {
        data: protocol,
        pos: 0,
    };
    if rd.num()? != 0.0 {
        return Ok(Output::rejected("native fixture construction failed"));
    }
    let status = rd.num()? as i64;
    let n = rd.count()?;
    let pairs: Vec<i64> = rd
        .slice(n.checked_mul(4).ok_or_else(|| herr("pair overflow"))?)?
        .iter()
        .map(|&x| x as i64)
        .collect();
    let n = rd.count()?;
    let mut ints = vec![n as i64];
    let mut numbers = Vec::new();
    for _ in 0..n {
        ints.extend(rd.slice(13)?.iter().map(|&x| x as i64));
        numbers.extend_from_slice(rd.slice(2)?);
        for _ in 0..5 {
            let n = rd.count()?;
            ints.push(n as i64);
            ints.extend(rd.slice(n)?.iter().map(|&x| x as i64));
        }
    }
    ints.push(rd.num()? as i64);
    let n = rd.count()?;
    let mut geometry = Vec::new();
    let mut values = Vec::new();
    geometry.push(n as i64);
    for _ in 0..n {
        let ty = rd.num()? as i64;
        geometry.push(ty);
        if ty < 0 {
            continue;
        }
        values.extend_from_slice(rd.slice(2)?);
        for _ in 0..5 {
            let s = rd.num()? as i64;
            geometry.push(s);
            if s == 1000 {
                values.extend_from_slice(rd.slice(3)?);
            }
        }
    }
    if rd.pos != protocol.len() {
        return Err(herr("trailing validation output"));
    }
    Ok(Output::new()
        .int("status", status)
        .ints("pairs", pairs)
        .ints("reports", ints)
        .values("report_values", numbers)
        .ints("geometry", geometry)
        .values("curve_samples", values))
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_validation"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let (path, block, _) = exported(&i)?;
        output(&ffi::eval(op(&i), &path, &block, args(&i)).map_err(|e| herr(e.to_string()))?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        // an input the FFI refuses is a harness error on both sides (PR #502's
        // convention): the shim never runs it, so the Rust side must not either. Fuzz
        // case fuzz00074 (`--seeds 24`) mutated `grid_faces` to 2^32: the FFI refuses a
        // sheet grid of more than 512 faces for the C++ side, while this side's
        // `sheet_grid` built faces until the 4 GiB fuzz address-space limit aborted the
        // process. The checks see the same operation, block and arguments as `run_cpp`.
        let (path, block) = native_input(&i)?;
        ffi::validate(op(&i), &path, &block, args(&i)).map_err(|e| herr(e.to_string()))?;
        let mut b = match own_brep(&i)? {
            Some(Ok(b)) => b,
            Some(Err(o)) => return Ok(o),
            None => {
                let (_, _, export) = exported(&i)?;
                if export.first() != Some(&0.0) {
                    return Ok(Output::rejected("native fixture construction failed"));
                }
                imported(&export)?
            }
        };
        mutate(&mut b, &i)?;
        let kernel = Kernel::default();
        let mut reports = AssertArray::new();
        let run = if i.coincidence {
            validation::contains_coincident_topology(
                &kernel,
                &mut b,
                i.tolerance,
                i.find_all,
                Some(&mut reports),
                i.level,
                &key,
            )
            .map(|r| (i32::from(r.found), r.pairs))
        } else {
            validation::validate_pointers(
                &kernel,
                &mut b,
                Some(&mut reports),
                i.level,
                i.walk,
                &key,
            )
            .map(|s| (s, Vec::new()))
        };
        let (status, pairs) = match run {
            Ok(x) => x,
            Err(e) => return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        };
        output(&rust_protocol(&b, status, &pairs, &reports)?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let coincidence = rng.below(3) == 0;
        let brep = match rng.below(4) {
            0 => BrepIn::Box {
                origin: rng.point3(-10.0, 10.0),
                size: [
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                    0.5 + rng.unit() * 4.0,
                ],
            },
            1 => BrepIn::Network,
            2 => BrepIn::Vertices {
                points: (0..(2 + rng.below(12)))
                    .map(|_| rng.point3(-2.0, 2.0))
                    .collect(),
            },
            _ => BrepIn::Points {
                vertices: vec![[0.0, 0.0, 0.0, 1e-6], [0.5, 0.0, 0.0, 1e-6]],
                lines: vec![[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1e-6]],
            },
        };
        let mutation = if coincidence { 0 } else { rng.below(11) as u8 };
        let value = if matches!(mutation, 4 | 5) {
            rng.below(3) as f64
        } else {
            [0.0, 1e-12, 1e-6, 0.1, 1.0, 1e3]
                .get(rng.below(6) as usize)
                .copied()?
        };
        serde_json::to_value(Inputs {
            brep,
            grid_faces: None,
            shared_boundaries: None,
            coincidence,
            level: rng.below(5) as u8,
            walk: rng.below(2) == 0,
            tolerance: 1e-6,
            find_all: rng.below(2) == 0,
            mutation,
            index: rng.below(100) as usize,
            value,
        })
        .ok()
    }
}

// ---- the sheet grids: the shim's `sheet_grid` through `SmBrepConstructor` ----------------------

/// `SmBrepConstructor`'s state between calls (`SmBrepData.cpp`), for the calls the shim's
/// `sheet_grid` makes on an open shell in the infinite region (no `StartRegion`, so no
/// inner shell): the records each call makes, linked as upstream links them.
struct Constructor {
    outer_shell: Option<ShellId>,
    face: Option<FaceId>,
    loop_: Option<LoopId>,
    loop_surface_orientation: OrientType,
    edge: Option<EdgeId>,
    edgeuse: Option<EdgeuseId>,
}

fn sm_err(op: &'static str) -> SolidError {
    SolidError::sm_err(op)
}

fn opt_err<T>(x: Option<T>, op: &'static str) -> Result<T, SolidError> {
    x.ok_or_else(|| sm_err(op))
}

impl Constructor {
    /// `StartBrep` (the B-rep is the caller's new one).
    fn new() -> Self {
        Self {
            outer_shell: None,
            face: None,
            loop_: None,
            loop_surface_orientation: OrientType::Unknown,
            edge: None,
            edgeuse: None,
        }
    }

    /// `StartShell` without a region: one face-use shell posted into the infinite region.
    fn start_shell(&mut self, b: &mut Brep) -> Result<ShellId, SolidError> {
        const OP: &str = "difftest::brep_validation::start_shell";
        if self.outer_shell.is_some() {
            return Err(sm_err(OP));
        }
        let region = opt_err(b.infinite_region(), OP)?;
        let shell = b.add_shell_record(Shell {
            region: Some(region),
            shell_type: ShellType::Faceuse,
            members: ShellMembers::Faceuses(Vec::new()),
            ..Shell::default()
        })?;
        b.region_mut(region)?.shells.push(shell);
        self.outer_shell = Some(shell);
        Ok(shell)
    }

    /// `StartFace(SM_OT_SAME)`: the face (B-rep tolerance) and its up (`SAME`) and down
    /// (`OPPOSITE`) face uses, both posted into the outer shell.
    fn start_face(&mut self, b: &mut Brep) -> Result<FaceId, SolidError> {
        const OP: &str = "difftest::brep_validation::start_face";
        let shell = opt_err(self.outer_shell, OP)?;
        let tolerance = b.tolerance();
        let face = b.add_face_record(Face {
            tolerance,
            ..Face::new(shell)
        })?;
        let fu1 = b.add_faceuse_record(Faceuse {
            face: Some(face),
            orientation: OrientType::Same,
            ..Faceuse::default()
        })?;
        let fu2 = b.add_faceuse_record(Faceuse {
            face: Some(face),
            mate: Some(fu1),
            orientation: OrientType::Opposite,
            ..Faceuse::default()
        })?;
        b.faceuse_mut(fu1)?.mate = Some(fu2);
        b.face_mut(face)?.faceuse = Some(fu1);
        for fu in [fu1, fu2] {
            b.faceuse_mut(fu)?.shell = Some(shell);
            match &mut b.shell_mut(shell)?.members {
                ShellMembers::Faceuses(v) => v.push(fu),
                _ => return Err(sm_err(OP)),
            }
        }
        b.shell_mut(shell)?.faces.push(face);
        self.face = Some(face);
        Ok(face)
    }

    /// `SetFaceSurface(surface, surface->GetNaturalUVDomain())` of a surface without
    /// owner: the face owns it.
    fn set_face_surface(
        &mut self,
        b: &mut Brep,
        mut surface: Surface,
        uv_domain: Extent2d,
    ) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::set_face_surface";
        let face = opt_err(self.face, OP)?;
        surface.set_owner(Some(arena_identity(CacheObject::Face(face))));
        let f = b.face_mut(face)?;
        f.surface = Some(surface);
        f.uv_domain = uv_domain;
        b.sync_face_surface_tolerance(face)
    }

    /// `StartLoop(SM_OT_SAME)`: the loop and its loop uses on the up and down face uses
    /// (the first loop of a face use outer, `SAME`).
    fn start_loop(&mut self, b: &mut Brep) -> Result<LoopId, SolidError> {
        const OP: &str = "difftest::brep_validation::start_loop";
        let face = opt_err(self.face, OP)?;
        self.edge = None;
        let fu1 = opt_err(b.face(face)?.faceuse, OP)?;
        let fu2 = opt_err(b.faceuse(fu1)?.mate, OP)?;
        let l = b.add_loop_record(Loop {
            face,
            edges: Vec::new(),
            loopuse: None,
        })?;
        let lu1 = b.add_loopuse_record(Loopuse {
            faceuse: Some(fu1),
            loop_: Some(l),
            ..Loopuse::default()
        })?;
        let lu2 = b.add_loopuse_record(Loopuse {
            faceuse: Some(fu2),
            loop_: Some(l),
            mate: Some(lu1),
            ..Loopuse::default()
        })?;
        b.faceuse_mut(fu1)?.loopuses.push(lu1);
        b.faceuse_mut(fu2)?.loopuses.push(lu2);
        b.loop_mut(l)?.loopuse = Some(lu1);
        b.face_mut(face)?.loops.push(l);
        self.loop_surface_orientation = OrientType::Same;
        let orientation = if b.faceuse(fu1)?.loopuses.len() == 1 {
            OrientType::Same
        } else {
            OrientType::Opposite
        };
        b.loopuse_mut(lu1)?.orientation = orientation;
        b.loopuse_mut(lu2)?.orientation = orientation;
        b.loopuse_mut(lu1)?.mate = Some(lu2);
        self.loop_ = Some(l);
        Ok(l)
    }

    /// `StartEdge(SM_OT_SAME, existing_edge, start_vertex, end_vertex)`.
    fn start_edge(
        &mut self,
        b: &mut Brep,
        existing_edge: Option<EdgeId>,
        mut start_vertex: Option<VertexId>,
        mut end_vertex: Option<VertexId>,
    ) -> Result<EdgeId, SolidError> {
        const OP: &str = "difftest::brep_validation::start_edge";
        let l = opt_err(self.loop_, OP)?;
        let edge = match existing_edge {
            None => {
                let tolerance = b.tolerance();
                b.add_edge_record(Edge {
                    vertices: [VertexId::from_index(0), VertexId::from_index(0)],
                    edgeuses: Vec::new(),
                    interval: Extent1d::new(),
                    tolerance,
                    curve: None,
                })?
            }
            Some(e) => e,
        };
        let eu1 = b.add_edgeuse_record(Edgeuse::default())?;
        let eu2 = b.add_edgeuse_record(Edgeuse::default())?;
        let vu1 = b.add_vertexuse_record(Vertexuse {
            owner: VertexuseOwner::Edgeuse(Some(eu1)),
            ..Vertexuse::default()
        })?;
        let vu2 = b.add_vertexuse_record(Vertexuse {
            owner: VertexuseOwner::Edgeuse(Some(eu2)),
            ..Vertexuse::default()
        })?;
        b.edgeuse_mut(eu1)?.vertexuse = Some(vu1);
        b.edgeuse_mut(eu2)?.vertexuse = Some(vu2);
        // the shim's edges are SAME in a SAME loop
        if OrientType::Same == self.loop_surface_orientation {
            b.edgeuse_mut(eu1)?.orientation = OrientType::Same;
            b.edgeuse_mut(eu2)?.orientation = OrientType::Opposite;
        } else {
            b.edgeuse_mut(eu1)?.orientation = OrientType::Opposite;
            b.edgeuse_mut(eu2)?.orientation = OrientType::Same;
            std::mem::swap(&mut start_vertex, &mut end_vertex);
        }
        // the loop uses
        let lu1 = opt_err(b.loop_(l)?.loopuse, OP)?;
        let lu2 = opt_err(b.loopuse(lu1)?.mate, OP)?;
        b.edgeuse_mut(eu1)?.owner = EdgeuseOwner::Loopuse(Some(lu1));
        b.loopuse_mut(lu1)?.start = LoopuseStart::Edgeuse(Some(eu1));
        b.edgeuse_mut(eu2)?.owner = EdgeuseOwner::Loopuse(Some(lu2));
        b.loopuse_mut(lu2)?.start = LoopuseStart::Edgeuse(Some(eu2));
        // CCW and CW
        if self.edge.is_none() {
            for eu in [eu1, eu2] {
                let r = b.edgeuse_mut(eu)?;
                r.ccw = Some(eu);
                r.cw = Some(eu);
            }
        } else if self.loop_surface_orientation == OrientType::Same {
            let prev1 = opt_err(self.edgeuse, OP)?;
            let prev2 = opt_err(eu1p::mate(b, prev1)?, OP)?;
            let prev1_ccw = opt_err(b.edgeuse(prev1)?.ccw, OP)?;
            b.edgeuse_mut(eu1)?.ccw = Some(prev1_ccw);
            b.edgeuse_mut(prev1_ccw)?.cw = Some(eu1);
            b.edgeuse_mut(prev1)?.ccw = Some(eu1);
            b.edgeuse_mut(eu1)?.cw = Some(prev1);
            let prev2_cw = opt_err(b.edgeuse(prev2)?.cw, OP)?;
            b.edgeuse_mut(eu2)?.cw = Some(prev2_cw);
            b.edgeuse_mut(prev2_cw)?.ccw = Some(eu2);
            b.edgeuse_mut(prev2)?.cw = Some(eu2);
            b.edgeuse_mut(eu2)?.ccw = Some(prev2);
        } else {
            // (the shim's loops are all SAME)
            return Err(sm_err(OP));
        }
        // connect edge and edge uses: the first edge use has the edge's orientation
        if existing_edge.is_none() {
                if let Some(v) = start_vertex {
                    post_insert_vertexuse(b, v, vu1)?;
                }
                if let Some(v) = end_vertex {
                    post_insert_vertexuse(b, v, vu2)?;
                }
                let order = if b.edgeuse(eu1)?.orientation == OrientType::Same {
                    [eu1, eu2]
                } else {
                    [eu2, eu1]
                };
                for eu in order {
                    b.edge_mut(edge)?.edgeuses.push(eu);
                    b.edgeuse_mut(eu)?.edge = Some(edge);
                }
        } else {
                let eu = opt_err(eu1p::primary_edgeuse(b, edge)?, OP)?;
                let eu_mate = opt_err(eu1p::mate(b, eu)?, OP)?;
                let eu_vu = opt_err(b.edgeuse(eu)?.vertexuse, OP)?;
                let mate_vu = opt_err(b.edgeuse(eu_mate)?.vertexuse, OP)?;
                let (to_eu_vertex, to_mate_vertex) =
                    if b.edgeuse(eu)?.orientation == b.edgeuse(eu1)?.orientation {
                        (vu1, vu2)
                    } else {
                        (vu2, vu1)
                    };
                if let Some(v) = b.vertexuse(eu_vu)?.vertex {
                    post_insert_vertexuse(b, v, to_eu_vertex)?;
                }
                if let Some(v) = b.vertexuse(mate_vu)?.vertex {
                    post_insert_vertexuse(b, v, to_mate_vertex)?;
                }
                // the radial sector: the primary's mate of a lamina edge (the shim's
                // shared edges join one face to another, never a third)
                if !solidcore::topology::edge::is_lamina(b, edge)? {
                    return Err(sm_err(OP));
                }
                solidcore::topology::edge::add_oriented_eu_pair(b, edge, eu1, eu2, Some(eu_mate))?;
        }
        self.edge = Some(edge);
        self.edgeuse = Some(eu1);
        Ok(edge)
    }

    /// `SetEdgeCurve(curve, FALSE)`: the 3D curve of an edge without one, owned by it.
    fn set_edge_curve(&mut self, b: &mut Brep, mut curve: Curve) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::set_edge_curve";
        let edge = opt_err(self.edge, OP)?;
        opt_err(self.edgeuse, OP)?;
        if b.edge(edge)?.curve.is_some() {
            return Err(sm_err(OP));
        }
        let _ = curve.set_owner(
            Some(arena_identity(CacheObject::Edge(edge))),
            Some(arena_identity(CacheObject::Brep)),
        );
        b.edge_mut(edge)?.curve = Some(curve);
        Ok(())
    }

    /// `SetLimits(SmVertexNode(min), SmVertexNode(max))`: parameter nodes, so the
    /// interval is set directly.
    fn set_limits(&mut self, b: &mut Brep, min: f64, max: f64) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::set_limits";
        let edge = opt_err(self.edge, OP)?;
        if b.edge(edge)?.curve.is_none() {
            return Err(sm_err(OP));
        }
        b.edge_mut(edge)?.interval.set_min_max(min, max)
    }

    /// `EndEdge`.
    fn end_edge(&mut self) -> Result<(), SolidError> {
        opt_err(self.edge, "difftest::brep_validation::end_edge").map(|_| ())
    }

    /// `EndLoop`: a vertex for every vertex use of the up loop use (shared through the
    /// mate's CCW neighbour, the radial's CCW neighbour, or a closed edge's radial),
    /// and the down loop use's vertex uses hooked to the same vertices.
    fn end_loop(&mut self, k: &Kernel, b: &mut Brep) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::end_loop";
        let l = opt_err(self.loop_, OP)?;
        let lu = opt_err(b.loop_(l)?.loopuse, OP)?;
        let vertexuses = solidcore::topology::loopuse::vertexuses(b, lu, None)?;
        for vu in vertexuses {
            let vu = opt_err(vu, OP)?;
            let VertexuseOwner::Edgeuse(Some(eu)) = b.vertexuse(vu)?.owner else {
                return Err(sm_err(OP));
            };
            let e = opt_err(b.edgeuse(eu)?.edge, OP)?;
            let mate = opt_err(eu1p::mate(b, eu)?, OP)?;
            let mate_ccw = opt_err(b.edgeuse(mate)?.ccw, OP)?;
            let vu_mate_ccw = opt_err(b.edgeuse(mate_ccw)?.vertexuse, OP)?;
            let radial = eu1p::radial(b, eu)?;
            let vu_radial_ccw = match radial {
                Some(r) if r != mate => {
                    let r_ccw = opt_err(b.edgeuse(r)?.ccw, OP)?;
                    b.edgeuse(r_ccw)?.vertexuse
                }
                _ => None,
            };
            let v = if let Some(v) = b.vertexuse(vu)?.vertex {
                v
            } else {
                let mut v = b.vertexuse(vu_mate_ccw)?.vertex;
                if v.is_none() {
                    if let Some(x) = vu_radial_ccw {
                        v = b.vertexuse(x)?.vertex;
                    }
                }
                if v.is_none() {
                    if let Some(r) = radial {
                        let r_vu = opt_err(b.edgeuse(r)?.vertexuse, OP)?;
                        if let Some(rv) = b.vertexuse(r_vu)?.vertex {
                            let edge = b.edge(e)?;
                            let curve = opt_err(edge.curve.as_ref(), OP)?;
                            if curve.is_closed(k, &edge.interval, edge.tolerance)? {
                                v = Some(rv);
                            }
                        }
                    }
                }
                let v = if let Some(v) = v {
                    v
                } else {
                        let tolerance = b.tolerance();
                        let v = b.add_vertex_record(Vertex {
                            tolerance,
                            ..Vertex::default()
                        })?;
                        let edge = b.edge(e)?;
                        let t = if b.edgeuse(eu)?.orientation == OrientType::Opposite {
                            edge.interval.max()
                        } else {
                            edge.interval.min()
                        };
                        let p = opt_err(edge.curve.as_ref(), OP)?.evaluate_point(k, t)?;
                        b.vertex_mut(v)?.point = p;
                        v
                };
                post_insert_vertexuse(b, v, vu)?;
                v
            };
            // the vertex use on the bottom loop use
            if b.vertexuse(vu_mate_ccw)?.vertex.is_none() {
                post_insert_vertexuse(b, v, vu_mate_ccw)?;
            }
            let cw = opt_err(b.edgeuse(eu)?.cw, OP)?;
            if eu1p::mate(b, cw)? != Some(mate_ccw) {
                return Err(sm_err(OP));
            }
        }
        self.loop_ = None;
        self.edge = None;
        self.loop_surface_orientation = OrientType::Unknown;
        Ok(())
    }

    /// `EndFace`.
    fn end_face(&mut self) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::end_face";
        if self.face.is_none() || self.loop_.is_some() {
            return Err(sm_err(OP));
        }
        self.face = None;
        Ok(())
    }

    /// `EndShell(FALSE)` (no stitching).
    fn end_shell(&mut self) -> Result<(), SolidError> {
        const OP: &str = "difftest::brep_validation::end_shell";
        if self.outer_shell.is_none() || self.face.is_some() {
            return Err(sm_err(OP));
        }
        self.outer_shell = None;
        Ok(())
    }

    /// `EndBrep`.
    fn end_brep(&self) -> Result<(), SolidError> {
        if self.outer_shell.is_some() || self.face.is_some() || self.loop_.is_some() {
            return Err(sm_err("difftest::brep_validation::end_brep"));
        }
        Ok(())
    }
}

/// `pVertex->PostInsert(pVU)`: `vu` appended to the vertex's use list.
fn post_insert_vertexuse(b: &mut Brep, v: VertexId, vu: VertexuseId) -> Result<(), SolidError> {
    b.vertex_mut(v)?.vertexuses.push(vu);
    b.vertexuse_mut(vu)?.vertex = Some(v);
    Ok(())
}

/// The shim's `sheet_grid(count, status, shared)`: `count` unit-square plane faces at
/// `x = 4 i` (`shared` 0), or two at `x = 0` whose second face reuses the first's edges
/// (`shared` 1) or new edges between the first's vertices (`shared` 2), in the shim's
/// `SmBrepConstructor` call order.
fn sheet_grid(k: &mut Kernel, b: &mut Brep, count: usize, shared: u8) -> Result<(), SolidError> {
    let mut builder = Constructor::new();
    builder.start_shell(b)?;
    let mut first_edges: [Option<EdgeId>; 4] = [None; 4];
    for i in 0..count {
        let x = if shared != 0 { 0.0 } else { i as f64 * 4.0 };
        builder.start_face(b)?;
        let surface = Surface::from(Plane::new(
            Point3::new(x, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
        ));
        let dom = surface.natural_uv_domain_with_nurb(k)?;
        builder.set_face_surface(b, surface, dom)?;
        builder.start_loop(b)?;
        let p = [
            Point3::new(x, 0.0, 0.0),
            Point3::new(x + 1.0, 0.0, 0.0),
            Point3::new(x + 1.0, 1.0, 0.0),
            Point3::new(x, 1.0, 0.0),
        ];
        for j in 0..4 {
            let kk = (j + 1) % 4;
            let original = if i != 0 && shared != 0 {
                first_edges.get(j).copied().flatten()
            } else {
                None
            };
            let (start, end) = match original {
                Some(o) if shared == 2 => {
                    let v = solidcore::topology::edge::vertex(b, o)?;
                    (v, solidcore::topology::edge::other_vertex(b, o, v)?)
                }
                _ => (None, None),
            };
            let edge = builder.start_edge(b, if shared == 1 { original } else { None }, start, end)?;
            if i == 0 {
                if let Some(slot) = first_edges.get_mut(j) {
                    *slot = Some(edge);
                }
            }
            if original.is_some() && shared == 1 {
                builder.end_edge()?;
                continue;
            }
            let a = *p.get(kk).ok_or_else(|| sm_err("sheet_grid"))?;
            let c = *p.get((kk + 1) % 4).ok_or_else(|| sm_err("sheet_grid"))?;
            let curve = Curve::from(Line::from_points(a, c, 3));
            let ivl = curve.natural_interval();
            builder.set_edge_curve(b, curve)?;
            builder.set_limits(b, ivl.min(), ivl.max())?;
            builder.end_edge()?;
        }
        builder.end_loop(k, b)?;
        builder.end_face()?;
    }
    builder.end_shell()?;
    builder.end_brep()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_case(grid_faces: u64, coincidence: bool) -> Case {
        Case {
            unit: "topology.brep_validation".into(),
            name: "t".into(),
            source: crate::CaseSource::Inline,
            inputs: serde_json::json!({"grid_faces": grid_faces, "coincidence": coincidence}),
        }
    }

    /// Fuzz case `topology.brep_validation/fuzz00074` (`--fuzz 300 --seeds 24`): a sheet
    /// grid of 2^32 faces. The FFI refuses more than 512 faces for the C++ side, and the
    /// Rust side must be refused the same way instead of building faces until the
    /// allocator aborts.
    #[test]
    fn huge_sheet_grid_is_refused_on_both_sides() {
        for coincidence in [false, true] {
            let case = grid_case(4_294_967_296, coincidence);
            let cpp = Runner.run_cpp(&case).unwrap_err().to_string();
            let rust = Runner.run_rust(&case).unwrap_err().to_string();
            assert_eq!(cpp, "invalid argument: invalid B-rep validation arguments");
            assert_eq!(rust, cpp);
        }
    }

    /// The largest sheet grid the FFI accepts still runs on both sides.
    #[test]
    fn max_sheet_grid_runs_on_both_sides() {
        let case = grid_case(512, false);
        let cpp = Runner.run_cpp(&case).unwrap();
        let rust = Runner.run_rust(&case).unwrap();
        assert!(crate::compare(&cpp, &rust, &[]).mismatches.is_empty());
    }

    #[test]
    fn shared_boundaries_reach_positive_edge_and_face_comparisons() {
        for (mode, expected_type) in [(1, 16002), (2, 16016)] {
            let case = Case {
                unit: "topology.brep_validation".into(),
                name: format!("positive_shared_boundaries_{mode}"),
                source: crate::CaseSource::Inline,
                inputs: serde_json::json!({"shared_boundaries": mode, "coincidence": true}),
            };
            let cpp = Runner
                .run_cpp(&case)
                .expect("native shared-boundary fixture");
            let rust = Runner
                .run_rust(&case)
                .expect("Rust shared-boundary fixture");
            for result in [&cpp, &rust] {
                assert!(result.error.is_none());
                assert!(result.rejected.is_none());
                assert!(result.deferred.is_none());
                assert!(result.ints["pairs"]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] == expected_type && p[2] == expected_type));
            }
            assert_eq!(cpp, rust);
        }
    }
}
