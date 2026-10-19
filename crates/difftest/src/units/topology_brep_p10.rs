//! Runner for unit `topology.brep_p10`: building a B-rep from serialized B-rep data
//! (`SmBrep::MakeTopologyFromData`), then `TestClosureSplitRegion` (edge version),
//! `RemoveAllTopology` or `ProjectAndTrim`.
//!
//! C++: `solidcore_ffi::units::topology_brep_p10` (upstream
//! `source/SMLib/src/SmBrep.cpp`). Rust: `solidcore::topology::brep::p10`.
//!
//! A case names an upstream test file (`file`, relative to `TestFiles/pt_TestFiles`, or
//! `fixture:<name>` for a crafted file of this unit: three triangles around one spine
//! edge, and the same with a doubled triangle),
//! the six `MakeTopologyFromData` flags (`options`; the default is `ReadFromFile`'s
//! call), patches to the read B-rep data and an op. Both sides read the file
//! (`SmBrepData::ReadFromFile` / [`BrepData::read_from_file`]), patch it, build the
//! B-rep and dump the result in the layout documented in the FFI module: integers to
//! `ints`, doubles to `dbls` (subnormals as 0) and bit for bit to `dbl_bits`, `kinds`
//! the interleaving. A Rust `InvalidInput` is the guard's `-2`.
//!
//! Generated cases pick a file, flags and patches (zone tolerance, manifold flag,
//! healer version, borrowed geometry, flag stamps, the implied regions of an IGES
//! solid, vertex scaling from 1e-6 to 1e6, dropped UV curves, and index fields set to
//! boundary and out-of-range values) and an op.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector2d::Point2;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p10::{
    make_topology_from_data, project_and_trim, remove_all_topology, test_closure_split_region,
    MakeTopologyOptions, ProjectAndTrimOptions, ProjectEnv, TopologyEnv, TrimType,
};
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::brep_data::{BrepData, TopologyData};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::topology::Topology;
use solidcore::topology::{
    Brep, BrepId, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId,
    LoopuseStart, RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, Kernel, SolidError};
use solidcore_ffi::units::topology_brep_p10::{self as ffi, Item, Op, Patch, Request};

use crate::case::{repo_root, CaseContext};
use crate::fixtures::upstream_test_file;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 300;

/// `topology.brep_p10` runner.
pub struct Runner;

fn default_options() -> [bool; 6] {
    // SmBrep::ReadFromFile: MakeTopologyFromData(data, attrs, FALSE, TRUE, TRUE)
    [false, true, true, false, false, true]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    file: String,
    #[serde(default = "default_options")]
    options: [bool; 6],
    #[serde(default)]
    patches: Vec<PatchIn>,
    #[serde(default)]
    op: OpIn,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PatchIn {
    ZoneTol(f64),
    Manifold(i64),
    Healer(i64),
    Borrowed(bool),
    Stamp(u64),
    Iges,
    Scale(f64),
    DropUv(i64),
    Set { field: i64, index: i64, value: i64 },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OpIn {
    #[default]
    Make,
    Closure {
        do_infinite: bool,
    },
    RemoveAll {
        editing: bool,
    },
    Project {
        trim: i64,
        a: [f64; 3],
        b: [f64; 3],
        dir: [f64; 3],
        reference: [f64; 3],
        #[serde(default)]
        approx_tol: Option<f64>,
        #[serde(default)]
        angle_tol: Option<f64>,
    },
}

fn harness(m: impl Into<String>) -> HarnessError {
    HarnessError::new(m.into())
}

/// The file of a case: under `TestFiles/pt_TestFiles`, or with the prefix `fixture:`
/// one of this unit's crafted files in `fixtures/topology.brep_p10`.
fn path_of(i: &Inputs) -> Result<String, HarnessError> {
    if let Some(name) = i.file.strip_prefix("fixture:") {
        if name.contains(['/', '\\']) {
            return Err(harness("fixture file names have no directories"));
        }
        let p = repo_root().join("fixtures/topology.brep_p10").join(name);
        return Ok(p.to_string_lossy().into_owned());
    }
    Ok(upstream_test_file(&repo_root(), &i.file)?
        .to_string_lossy()
        .into_owned())
}

fn to_output(items: &[Item]) -> Output {
    let ints: Vec<i64> = items
        .iter()
        .filter_map(|x| match x {
            Item::Int(v) => Some(*v),
            Item::Dbl(_) => None,
        })
        .collect();
    let dbls: Vec<f64> = items
        .iter()
        .filter_map(|x| match x {
            Item::Dbl(v) => Some(*v),
            Item::Int(_) => None,
        })
        .collect();
    let kinds: Vec<i64> = items
        .iter()
        .map(|x| i64::from(matches!(x, Item::Dbl(_))))
        .collect();
    let bits: Vec<i64> = dbls.iter().map(|v| v.to_bits() as i64).collect();
    let dbls: Vec<f64> = dbls
        .into_iter()
        .map(|v| if v.is_subnormal() { 0.0 } else { v })
        .collect();
    Output::new()
        .ints("ints", ints)
        .values("dbls", dbls)
        .ints("dbl_bits", bits)
        .ints("kinds", kinds)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let patches = i
        .patches
        .iter()
        .map(|p| match *p {
            PatchIn::ZoneTol(v) => Patch::ZoneTol(v),
            PatchIn::Manifold(v) => Patch::Manifold(v),
            PatchIn::Healer(v) => Patch::Healer(v),
            PatchIn::Borrowed(v) => Patch::Borrowed(v),
            PatchIn::Stamp(v) => Patch::Stamp(v),
            PatchIn::Iges => Patch::Iges,
            PatchIn::Scale(v) => Patch::Scale(v),
            PatchIn::DropUv(v) => Patch::DropUv(v),
            PatchIn::Set {
                field,
                index,
                value,
            } => Patch::Set {
                field,
                index,
                value,
            },
        })
        .collect();
    let op = match i.op {
        OpIn::Make => Op::Make,
        OpIn::Closure { do_infinite } => Op::Closure { do_infinite },
        OpIn::RemoveAll { editing } => Op::RemoveAll { editing },
        OpIn::Project {
            trim,
            a,
            b,
            dir,
            reference,
            approx_tol,
            angle_tol,
        } => Op::Project {
            trim,
            a,
            b,
            dir,
            reference,
            approx_tol,
            angle_tol,
        },
    };
    let req = Request {
        path: path_of(i)?,
        binary: false,
        options: i.options,
        patches,
        op,
    };
    ffi::run(&req)
        .map(|items| to_output(&items))
        .map_err(|e| harness(format!("ffi: {e}")))
}

// ---- the Rust side --------------------------------------------------------------------

/// Upstream status of a Rust error: the guard's `-2` for `InvalidInput`.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::Upstream { code, .. } => i64::from(*code),
        SolidError::InvalidInput { .. } => -2,
        _ => -1,
    }
}

const KIND_SHIFT: u32 = 40;

/// The identity of each entity: its kind and arena index.
fn key_of(e: EntityRef) -> ObjectRef {
    let (t, k, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey((1u64 << KIND_SHIFT) * (k + 1) + u64::from(i)),
    }
}

fn stamp(t: &mut TopologyData, base: u64, i: u64, k: u64) {
    t.flags = base.wrapping_add(7u64.wrapping_mul(i)).wrapping_add(k);
    t.user_index1 = base.wrapping_mul(3).wrapping_add(i);
    t.user_index2 = i.wrapping_mul(11).wrapping_add(k);
}

fn set_field(d: &mut BrepData, field: i64, index: i64, value: i64) -> Result<(), HarnessError> {
    let i = usize::try_from(index).map_err(|_| harness("bad index"))?;
    let u = value as u64;
    // an index past the array is a no-op
    match field {
        0 => d.faces.get_mut(i).into_iter().for_each(|r| r.surface = u),
        1 => d
            .faces
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.start_loop = u),
        2 => d.loops.get_mut(i).into_iter().for_each(|r| r.start_eu = u),
        3 => d.loops.get_mut(i).into_iter().for_each(|r| r.vertex = u),
        4 => d.edgeuses.get_mut(i).into_iter().for_each(|r| r.edge = u),
        5 => d.edgeuses.get_mut(i).into_iter().for_each(|r| r.loop_ = u),
        6 => d
            .edgeuses
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.next_eu = value),
        7 => d.edges.get_mut(i).into_iter().for_each(|r| r.curve = u),
        8 => d
            .edges
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.prim_eu = value),
        9 => d
            .shells
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.shell_type = u),
        10 => d
            .regions
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.start_shell = u),
        11 => d
            .edgeuses
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.uv_curve = u),
        12 => d
            .edgeuses
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.eu_type = u),
        13 => d.loops.get_mut(i).into_iter().for_each(|r| r.num_eu = u),
        14 => d.faceuses.get_mut(i).into_iter().for_each(|r| r.face = u),
        15 => d
            .edges
            .get_mut(i)
            .into_iter()
            .for_each(|r| r.start_vertex = u),
        _ => return Err(harness("bad set field")),
    }
    Ok(())
}

fn apply_patches(d: &mut BrepData, patches: &[PatchIn]) -> Result<(), HarnessError> {
    for p in patches {
        match *p {
            PatchIn::ZoneTol(v) => d.zone_tol_3d = v,
            PatchIn::Manifold(v) => {
                d.manifold = i32::try_from(v).map_err(|_| harness("manifold"))?;
            }
            PatchIn::Healer(v) => d.set_healer_version(v as u64),
            PatchIn::Borrowed(v) => d.is_geom_borrowed = v,
            PatchIn::Stamp(base) => {
                for (k, r) in d.regions.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 0);
                }
                for (k, r) in d.shells.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 1);
                }
                for (k, r) in d.faceuses.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 2);
                }
                for (k, r) in d.faces.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 3);
                }
                for (k, r) in d.loops.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 4);
                }
                for (k, r) in d.edgeuses.iter_mut().enumerate() {
                    let k = k as u64;
                    stamp(&mut r.topology, base, k, 5);
                    r.mate_flags = base.wrapping_add(5u64.wrapping_mul(k));
                    r.mate_user_index1 = k + 1;
                    r.mate_user_index2 = 2 * k;
                }
                for (k, r) in d.edges.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 6);
                }
                for (k, r) in d.vertices.iter_mut().enumerate() {
                    stamp(&mut r.topology, base, k as u64, 7);
                }
            }
            PatchIn::Iges => {
                // a no-op unless there are two regions whose second's shells exist
                if d.regions.len() != 2 {
                    continue;
                }
                let mut r = d.regions.get(1).cloned().ok_or_else(|| harness("region"))?;
                let range = usize::try_from(r.start_shell)
                    .ok()
                    .zip(usize::try_from(r.num_shells).ok())
                    .and_then(|(s0, ns)| Some(s0..s0.checked_add(ns)?));
                let Some(shells) = range.and_then(|rg| d.shells.get(rg)) else {
                    continue;
                };
                let shells = shells.to_vec();
                d.shells = shells;
                r.start_shell = 0;
                d.regions = vec![r];
                d.manifold = 1;
            }
            PatchIn::Scale(s) => {
                for v in &mut d.vertices {
                    v.point *= s;
                }
            }
            PatchIn::DropUv(n) => {
                let n = usize::try_from(n)
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| harness("drop-uv step"))?;
                for r in d.edgeuses.iter_mut().step_by(n) {
                    r.uv_curve = 999_999;
                }
            }
            PatchIn::Set {
                field,
                index,
                value,
            } => set_field(d, field, index, value)?,
        }
    }
    Ok(())
}

/// Items, or the case's deferral.
type Dump = Result<Vec<Item>, SolidError>;

fn rust(i: &Inputs) -> Result<Dump, HarnessError> {
    let path = path_of(i)?;
    let bytes = std::fs::read(&path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut k = Kernel::default();
    let mut data = match BrepData::read_from_file(&k, bytes, FileType::Ascii) {
        Ok(d) => d,
        Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
        Err(e) => return Err(harness(format!("Rust read failed: {e}"))),
    };
    apply_patches(&mut data, &i.patches)?;

    let mut out = Vec::new();
    let mut b = Brep::new_with_infinite_region().map_err(|e| harness(e.to_string()))?;
    let mut store = AttributeStore::new();
    let mut records: BTreeMap<ObjectKey, Topology> = BTreeMap::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    let [make_uv_trim_curves, add_analytics, force_same_tol, check_uv_trim_curves, always_heal, healer_enabled] =
        i.options;
    let options = MakeTopologyOptions {
        make_uv_trim_curves,
        add_analytics,
        force_same_tol,
        check_uv_trim_curves,
        always_heal,
        healer_enabled,
    };
    match make_topology_from_data(&mut k, &mut b, &mut data, &mut env, options) {
        Ok(()) => out.push(Item::Int(0)),
        Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
        Err(e) => {
            out.push(Item::Int(status(&e)));
            return Ok(Ok(out));
        }
    }
    out.push(Item::Int(i64::from(data.manifold)));
    out.push(Item::Int(i64::from(data.is_geom_borrowed)));
    let g = Graph::walk(&b).map_err(|e| harness(e.to_string()))?;
    export(&b, &g, &records, &mut out).map_err(|e| harness(e.to_string()))?;

    match i.op {
        OpIn::Make => {}
        OpIn::Closure { do_infinite } => {
            for &e in &g.edges.items {
                match test_closure_split_region(&mut k, &mut b, do_infinite, e) {
                    Ok(r) => {
                        out.push(Item::Int(0));
                        out.push(Item::Int(r.new_shells.len() as i64));
                        out.push(Item::Int(r.new_regions.len() as i64));
                    }
                    Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                    Err(e) => {
                        out.push(Item::Int(status(&e)));
                        out.push(Item::Int(0));
                        out.push(Item::Int(0));
                    }
                }
            }
            let g2 = Graph::walk(&b).map_err(|e| harness(e.to_string()))?;
            export(&b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
        }
        OpIn::RemoveAll { editing } => {
            b.set_editing_enabled(editing);
            match remove_all_topology(&mut k, &mut b) {
                Ok(()) => out.push(Item::Int(0)),
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                Err(e) => out.push(Item::Int(status(&e))),
            }
            let g2 = Graph::walk(&b).map_err(|e| harness(e.to_string()))?;
            export(&b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
        }
        OpIn::Project {
            trim,
            a,
            b: end,
            dir,
            reference,
            approx_tol,
            angle_tol,
        } => {
            let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
            let curve = match BSplineCurve::create_line_segment(3, v(a), v(end)) {
                Ok(c) => {
                    out.push(Item::Int(0));
                    c
                }
                Err(e) => {
                    out.push(Item::Int(status(&e)));
                    return Ok(Ok(out));
                }
            };
            let trim_type = match trim {
                0 => TrimType::KeepPoint,
                1 => TrimType::DeletePoint,
                _ => TrimType::Split,
            };
            let keys = |o: CacheObject| match o {
                CacheObject::Edge(e) | CacheObject::Curve(e) => key_of(EntityRef::Edge(e)),
                CacheObject::Face(f) | CacheObject::Surface(f) => key_of(EntityRef::Face(f)),
                CacheObject::Vertex(x) => key_of(EntityRef::Vertex(x)),
                CacheObject::Other(r) => r,
                _ => key_of(EntityRef::Brep(BrepId::from_index(0))),
            };
            let owner_of = |_: ObjectRef| None;
            let penv = ProjectEnv {
                brep_id: BrepId::from_index(0),
                keys: &keys,
                owner_of: &owner_of,
            };
            let opts = ProjectAndTrimOptions {
                approximation_tol_3d: approx_tol,
                angle_tol_radians: angle_tol,
                subset_faces: None,
            };
            match project_and_trim(
                &mut k,
                &mut b,
                &penv,
                &curve,
                v(dir),
                v(reference),
                trim_type,
                &opts,
            ) {
                Ok(()) => out.push(Item::Int(0)),
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                Err(e) => out.push(Item::Int(status(&e))),
            }
            let g2 = Graph::walk(&b).map_err(|e| harness(e.to_string()))?;
            export(&b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
        }
    }
    Ok(Ok(out))
}

// ---- traversal-order registry (mirrors the shim's walk) ---------------------------------

struct Reg<I> {
    items: Vec<I>,
    index: BTreeMap<u32, i64>,
}

impl<I: Copy> Reg<I> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            index: BTreeMap::new(),
        }
    }
    fn add(&mut self, id: Option<I>, raw: impl Fn(I) -> u32) {
        let Some(id) = id else {
            return;
        };
        let r = raw(id);
        if self.index.contains_key(&r) {
            return;
        }
        self.index.insert(r, self.items.len() as i64);
        self.items.push(id);
    }
    fn find(&self, id: Option<I>, raw: impl Fn(I) -> u32) -> i64 {
        match id {
            None => -1,
            Some(id) => self.index.get(&raw(id)).copied().unwrap_or(-3),
        }
    }
}

struct Graph {
    regions: Reg<RegionId>,
    shells: Reg<ShellId>,
    faceuses: Reg<FaceuseId>,
    faces: Reg<FaceId>,
    loopuses: Reg<LoopuseId>,
    loops: Reg<LoopId>,
    edgeuses: Reg<EdgeuseId>,
    edges: Reg<EdgeId>,
    vertexuses: Reg<VertexuseId>,
    vertices: Reg<VertexId>,
}

const LOOPUSE_TYPE: i64 = 16004;
const EDGEUSE_TYPE: i64 = 16005;
const VERTEXUSE_TYPE: i64 = 16006;
const SHELL_TYPE: i64 = 16013;
const UNKNOWN_TYPE: i64 = 16999;

fn orient_code(o: solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType) -> i64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

impl Graph {
    #[allow(clippy::too_many_lines)]
    fn walk(b: &Brep) -> Result<Self, SolidError> {
        let mut g = Self {
            regions: Reg::new(),
            shells: Reg::new(),
            faceuses: Reg::new(),
            faces: Reg::new(),
            loopuses: Reg::new(),
            loops: Reg::new(),
            edgeuses: Reg::new(),
            edges: Reg::new(),
            vertexuses: Reg::new(),
            vertices: Reg::new(),
        };
        for (id, r) in b.regions().iter() {
            if r.in_brep {
                g.regions.add(Some(id), RegionId::index);
            }
        }
        let (mut r, mut s, mut fu, mut lu, mut eu, mut vu, mut l, mut e, mut v) =
            (0, 0, 0, 0, 0, 0, 0, 0, 0);
        let mut more = true;
        let bound = b.edgeuses().len() + 1;
        while more {
            more = false;
            while let Some(&id) = g.regions.items.get(r) {
                for &x in &b.region(id)?.shells {
                    g.shells.add(Some(x), ShellId::index);
                }
                r += 1;
                more = true;
            }
            while let Some(&id) = g.shells.items.get(s) {
                match &b.shell(id)?.members {
                    ShellMembers::Faceuses(list) => {
                        for &x in list {
                            g.faceuses.add(Some(x), FaceuseId::index);
                        }
                    }
                    ShellMembers::Edgeuse(x) => g.edgeuses.add(Some(*x), EdgeuseId::index),
                    ShellMembers::Vertexuse(x) => g.vertexuses.add(Some(*x), VertexuseId::index),
                    ShellMembers::Empty => {}
                }
                s += 1;
                more = true;
            }
            while let Some(&id) = g.faceuses.items.get(fu) {
                let rec = b.faceuse(id)?;
                g.faces.add(rec.face, FaceId::index);
                g.faceuses.add(rec.mate, FaceuseId::index);
                for &x in &rec.loopuses {
                    g.loopuses.add(Some(x), LoopuseId::index);
                }
                fu += 1;
                more = true;
            }
            while let Some(&id) = g.loopuses.items.get(lu) {
                let rec = b.loopuse(id)?;
                g.loops.add(rec.loop_, LoopId::index);
                g.loopuses.add(rec.mate, LoopuseId::index);
                match rec.start {
                    LoopuseStart::Edgeuse(start) => {
                        g.edgeuses.add(start, EdgeuseId::index);
                        let mut q = start;
                        let mut k = 0;
                        while let Some(x) = q {
                            if k >= bound {
                                break;
                            }
                            g.edgeuses.add(Some(x), EdgeuseId::index);
                            q = b.edgeuse(x)?.ccw;
                            if q == start {
                                break;
                            }
                            k += 1;
                        }
                    }
                    LoopuseStart::Vertexuse(x) => g.vertexuses.add(x, VertexuseId::index),
                    LoopuseStart::Unknown => {}
                }
                lu += 1;
                more = true;
            }
            while let Some(&id) = g.edgeuses.items.get(eu) {
                let rec = b.edgeuse(id)?;
                g.edges.add(rec.edge, EdgeId::index);
                match rec.owner {
                    EdgeuseOwner::Loopuse(x) => g.loopuses.add(x, LoopuseId::index),
                    EdgeuseOwner::Shell(x) => g.shells.add(x, ShellId::index),
                    EdgeuseOwner::Unknown => {}
                }
                g.edgeuses.add(rec.cw, EdgeuseId::index);
                g.edgeuses.add(rec.ccw, EdgeuseId::index);
                g.vertexuses.add(rec.vertexuse, VertexuseId::index);
                g.edgeuses.add(eu1::edgeuse_next(b, id)?, EdgeuseId::index);
                g.edgeuses.add(eu1::edgeuse_last(b, id)?, EdgeuseId::index);
                eu += 1;
                more = true;
            }
            while let Some(&id) = g.edges.items.get(e) {
                for &x in &b.edge(id)?.edgeuses {
                    g.edgeuses.add(Some(x), EdgeuseId::index);
                }
                e += 1;
                more = true;
            }
            while let Some(&id) = g.vertexuses.items.get(vu) {
                let rec = b.vertexuse(id)?;
                g.vertices.add(rec.vertex, VertexId::index);
                match rec.owner {
                    VertexuseOwner::Edgeuse(x) => g.edgeuses.add(x, EdgeuseId::index),
                    VertexuseOwner::Loopuse(x) => g.loopuses.add(x, LoopuseId::index),
                    VertexuseOwner::Shell(x) => g.shells.add(x, ShellId::index),
                    VertexuseOwner::Unknown => {}
                }
                vu += 1;
                more = true;
            }
            while let Some(&id) = g.vertices.items.get(v) {
                for &x in &b.vertex(id)?.vertexuses {
                    g.vertexuses.add(Some(x), VertexuseId::index);
                }
                v += 1;
                more = true;
            }
            while let Some(&id) = g.loops.items.get(l) {
                g.loopuses.add(b.loop_(id)?.loopuse, LoopuseId::index);
                l += 1;
                more = true;
            }
        }
        Ok(g)
    }
}

struct Out<'a> {
    o: &'a mut Vec<Item>,
    records: &'a BTreeMap<ObjectKey, Topology>,
}

impl Out<'_> {
    fn i(&mut self, v: i64) {
        self.o.push(Item::Int(v));
    }
    fn d(&mut self, v: f64) {
        self.o.push(Item::Dbl(v));
    }
    fn list(&mut self, v: &[i64]) {
        self.i(v.len() as i64);
        for &x in v {
            self.i(x);
        }
    }
    fn topo(&mut self, e: EntityRef) {
        let t = self
            .records
            .get(&key_of(e).key)
            .copied()
            .unwrap_or_else(Topology::new);
        self.i(t.all_flags() as i64);
        self.i(t.user_index1() as i64);
        self.i(t.user_index2() as i64);
        self.i(i64::from(t.user_ptr1() != 0));
    }
    fn point(&mut self, r: Result<Vec3, SolidError>) {
        match r {
            Ok(p) => {
                self.i(0);
                self.d(p.x);
                self.d(p.y);
                self.d(p.z);
            }
            Err(e) => self.i(status(&e)),
        }
    }
    fn curve(&mut self, c: Option<&Curve>, owner: EntityRef) {
        let Some(c) = c else {
            self.i(0);
            return;
        };
        self.i(1);
        self.i(c.type_code());
        self.i(c.dim() as i64);
        let ivl = c.natural_interval();
        self.d(ivl.min());
        self.d(ivl.max());
        for k in 0..=4 {
            let t = ivl.min() + 0.25 * f64::from(k) * (ivl.max() - ivl.min());
            self.point(c.evaluate_point(t));
        }
        self.i(i64::from(c.owner() == Some(key_of(owner))));
    }
    fn surface(&mut self, s: Option<&Surface>, owner: EntityRef) {
        let Some(s) = s else {
            self.i(0);
            return;
        };
        self.i(1);
        self.i(s.type_code());
        let dom: Extent2d = s.natural_uv_domain();
        let (lo, hi) = (dom.min(), dom.max());
        self.d(lo.x);
        self.d(lo.y);
        self.d(hi.x);
        self.d(hi.y);
        for a in 0..=2 {
            for bb in 0..=2 {
                let uv = Point2::new(
                    lo.x + 0.5 * f64::from(a) * (hi.x - lo.x),
                    lo.y + 0.5 * f64::from(bb) * (hi.y - lo.y),
                );
                self.point(s.evaluate_point(uv));
            }
        }
        self.i(i64::from(s.owner() == Some(key_of(owner))));
    }
}

#[allow(clippy::too_many_lines)]
fn export(
    b: &Brep,
    g: &Graph,
    records: &BTreeMap<ObjectKey, Topology>,
    o: &mut Vec<Item>,
) -> Result<(), SolidError> {
    let mut w = Out { o, records };
    w.d(b.tolerance());
    w.i(i64::from(b.editing_enabled()));
    for n in [
        g.regions.items.len(),
        g.shells.items.len(),
        g.faceuses.items.len(),
        g.faces.items.len(),
        g.loopuses.items.len(),
        g.loops.items.len(),
        g.edgeuses.items.len(),
        g.edges.items.len(),
        g.vertexuses.items.len(),
        g.vertices.items.len(),
    ] {
        w.i(n as i64);
    }
    for &id in &g.regions.items {
        let r = b.region(id)?;
        w.i(i64::from(b.infinite_region() == Some(id)));
        w.i(i64::from(r.in_brep));
        w.i(i64::from(r.is_void));
        w.topo(EntityRef::Region(id));
        let v: Vec<i64> = r
            .shells
            .iter()
            .map(|&x| g.shells.find(Some(x), ShellId::index))
            .collect();
        w.list(&v);
    }
    for &id in &g.shells.items {
        let s = b.shell(id)?;
        w.i(g.regions.find(s.region, RegionId::index));
        w.i(s.shell_type.code());
        w.topo(EntityRef::Shell(id));
        let v: Vec<i64> = match (&s.shell_type, &s.members) {
            (ShellType::Faceuse, ShellMembers::Faceuses(list)) => list
                .iter()
                .map(|&x| g.faceuses.find(Some(x), FaceuseId::index))
                .collect(),
            (ShellType::Faceuse, _) => Vec::new(),
            (ShellType::Wire, ShellMembers::Edgeuse(x)) => {
                vec![g.edgeuses.find(Some(*x), EdgeuseId::index)]
            }
            (ShellType::Vertex, ShellMembers::Vertexuse(x)) => {
                vec![g.vertexuses.find(Some(*x), VertexuseId::index)]
            }
            (_, ShellMembers::Empty) => vec![-1],
            _ => vec![-4],
        };
        w.list(&v);
    }
    for &id in &g.faceuses.items {
        let f = b.faceuse(id)?;
        w.i(g.shells.find(f.shell, ShellId::index));
        w.i(g.faces.find(f.face, FaceId::index));
        w.i(g.faceuses.find(f.mate, FaceuseId::index));
        w.i(orient_code(f.orientation));
        w.topo(EntityRef::Faceuse(id));
        let v: Vec<i64> = f
            .loopuses
            .iter()
            .map(|&x| g.loopuses.find(Some(x), LoopuseId::index))
            .collect();
        w.list(&v);
    }
    for &id in &g.faces.items {
        let f = b.face(id)?;
        w.surface(f.surface.as_ref(), EntityRef::Face(id));
        w.i(g.faceuses.find(f.faceuse, FaceuseId::index));
        let (lo, hi) = (f.uv_domain.min(), f.uv_domain.max());
        w.d(lo.x);
        w.d(lo.y);
        w.d(hi.x);
        w.d(hi.y);
        w.i(i64::from(f.rectangular_trim));
        w.d(f.tolerance);
        w.topo(EntityRef::Face(id));
    }
    for &id in &g.loopuses.items {
        let l = b.loopuse(id)?;
        w.i(g.faceuses.find(l.faceuse, FaceuseId::index));
        w.i(g.loops.find(l.loop_, LoopId::index));
        w.i(g.loopuses.find(l.mate, LoopuseId::index));
        let (ty, start) = match l.start {
            LoopuseStart::Edgeuse(x) => (EDGEUSE_TYPE, g.edgeuses.find(x, EdgeuseId::index)),
            LoopuseStart::Vertexuse(x) => {
                (VERTEXUSE_TYPE, g.vertexuses.find(x, VertexuseId::index))
            }
            LoopuseStart::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(start);
        w.i(orient_code(l.orientation));
        w.topo(EntityRef::Loopuse(id));
    }
    for &id in &g.loops.items {
        w.i(g.loopuses.find(b.loop_(id)?.loopuse, LoopuseId::index));
        w.topo(EntityRef::Loop(id));
    }
    for &id in &g.edgeuses.items {
        let e = b.edgeuse(id)?;
        w.i(g.edges.find(e.edge, EdgeId::index));
        let (ty, owner) = match e.owner {
            EdgeuseOwner::Loopuse(x) => (LOOPUSE_TYPE, g.loopuses.find(x, LoopuseId::index)),
            EdgeuseOwner::Shell(x) => (SHELL_TYPE, g.shells.find(x, ShellId::index)),
            EdgeuseOwner::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(owner);
        w.i(g.edgeuses.find(e.cw, EdgeuseId::index));
        w.i(g.edgeuses.find(e.ccw, EdgeuseId::index));
        w.i(g.vertexuses.find(e.vertexuse, VertexuseId::index));
        w.i(orient_code(e.orientation));
        w.i(g.edgeuses.find(eu1::edgeuse_next(b, id)?, EdgeuseId::index));
        w.i(g.edgeuses.find(eu1::edgeuse_last(b, id)?, EdgeuseId::index));
        w.curve(e.uv_trim_curve.as_ref(), EntityRef::Edgeuse(id));
        w.topo(EntityRef::Edgeuse(id));
    }
    for &id in &g.edges.items {
        let e = b.edge(id)?;
        w.d(e.tolerance);
        w.d(e.interval.min());
        w.d(e.interval.max());
        let v: Vec<i64> = e
            .edgeuses
            .iter()
            .map(|&x| g.edgeuses.find(Some(x), EdgeuseId::index))
            .collect();
        w.list(&v);
        w.curve(e.curve.as_ref(), EntityRef::Edge(id));
        w.topo(EntityRef::Edge(id));
    }
    for &id in &g.vertexuses.items {
        let u = b.vertexuse(id)?;
        w.i(g.vertices.find(u.vertex, VertexId::index));
        let (ty, owner) = match u.owner {
            VertexuseOwner::Edgeuse(x) => (EDGEUSE_TYPE, g.edgeuses.find(x, EdgeuseId::index)),
            VertexuseOwner::Loopuse(x) => (LOOPUSE_TYPE, g.loopuses.find(x, LoopuseId::index)),
            VertexuseOwner::Shell(x) => (SHELL_TYPE, g.shells.find(x, ShellId::index)),
            VertexuseOwner::Unknown => (UNKNOWN_TYPE, -1),
        };
        w.i(ty);
        w.i(owner);
        w.topo(EntityRef::Vertexuse(id));
    }
    for &id in &g.vertices.items {
        let v = b.vertex(id)?;
        w.d(v.point.x);
        w.d(v.point.y);
        w.d(v.point.z);
        w.d(v.tolerance);
        let l: Vec<i64> = v
            .vertexuses
            .iter()
            .map(|&x| g.vertexuses.find(Some(x), VertexuseId::index))
            .collect();
        w.list(&l);
        w.topo(EntityRef::Vertex(id));
    }
    Ok(())
}

/// Shared post-state snapshot: topology in traversal order, sampled geometry,
/// and lazy-cache initialization/UV fields not used by the p10 operation runner.
pub(crate) fn snapshot(b: &Brep) -> Result<Output, HarnessError> {
    let g = Graph::walk(b).map_err(|e| harness(e.to_string()))?;
    let mut items = Vec::new();
    export(b, &g, &BTreeMap::new(), &mut items).map_err(|e| harness(e.to_string()))?;
    let mut output = to_output(&items).int("validate_only", i64::from(b.validate_only()));
    // This caller uses the normal relative comparison, not p10's bit audit.
    output.ints.remove("dbl_bits");
    output.values.insert(
        "dbls".into(),
        items
            .iter()
            .filter_map(|x| match x {
                Item::Dbl(v) => Some(*v),
                Item::Int(_) => None,
            })
            .collect(),
    );
    let mut flags = Vec::new();
    let mut uv = Vec::new();
    let mut links = Vec::new();
    for id in &g.faceuses.items {
        let f = b.faceuse(*id).map_err(|e| harness(e.to_string()))?;
        let (next, last) = f.sibling_links.unwrap_or((None, None));
        links.extend([
            g.faceuses.find(next, FaceuseId::index),
            g.faceuses.find(last, FaceuseId::index),
        ]);
    }
    for id in &g.edgeuses.items {
        let e = b.edgeuse(*id).map_err(|e| harness(e.to_string()))?;
        flags.extend([
            i64::from(e.drop_curve_fail.is_some()),
            e.drop_curve_fail
                .as_ref()
                .map_or(0, |f| i64::from(f.fail_status)),
            i64::from(e.drop_curve_fail.as_ref().is_some_and(|f| f.brep.is_some())),
            i64::from(e.max_edge_face_gap.is_init()),
            i64::from(e.edge_ccw_edge_gap.is_init()),
        ]);
    }
    for id in &g.vertexuses.items {
        let v = b.vertexuse(*id).map_err(|e| harness(e.to_string()))?;
        flags.extend([
            i64::from(v.vertex_face_gap.is_init()),
            i64::from(v.vertex_edge_gap.is_init()),
        ]);
        uv.extend([v.uv_point.x, v.uv_point.y]);
    }
    output = output
        .ints("gap_initialized", flags)
        .ints("faceuse_links", links)
        .values("uv", uv);
    Ok(output)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p10"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match rust(&case.parse()?)? {
            Ok(items) => Ok(to_output(&items)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(generate(rng)).ok()
    }
}

// ---- generated cases ----------------------------------------------------------------------

/// The upstream test files both sides read (every B-rep data `.smb` under
/// `pt_TestFiles`), listed once.
fn upstream_files() -> &'static [String] {
    static FILES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FILES.get_or_init(list_upstream_files)
}

fn list_upstream_files() -> Vec<String> {
    let root = repo_root().join(crate::fixtures::UPSTREAM_TEST_FILES);
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "smb") && !is_part_file(&p) {
                if let Ok(rel) = p.strip_prefix(&root) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
    }
    out.sort();
    out
}

/// A part file (several objects behind an output summary), not B-rep data.
fn is_part_file(p: &std::path::Path) -> bool {
    std::fs::read(p).is_ok_and(|b| b.starts_with(b"//[Output Summary]"))
}

fn gen_patch(rng: &mut Rng) -> PatchIn {
    match rng.below(10) {
        0 => PatchIn::ZoneTol(
            *rng.choose(&[0.0, -1.0, 1e-12, 1e-9, 1e-6, 1e-4, 1e-2, 1.0, 1e3])
                .unwrap_or(&0.0),
        ),
        1 => PatchIn::Manifold(*rng.choose(&[0, 1, 2]).unwrap_or(&0)),
        2 => PatchIn::Healer(*rng.choose(&[0, 9, 10, 11]).unwrap_or(&10)),
        3 => PatchIn::Borrowed(true),
        4 => PatchIn::Stamp(rng.below(1 << 20)),
        5 => PatchIn::Scale(
            *rng.choose(&[1e-6, 1e-3, 0.5, 2.0, 1e3, 1e6, -1.0])
                .unwrap_or(&1.0),
        ),
        6 => PatchIn::DropUv(rng.range_i64(1, 5)),
        7 => PatchIn::Set {
            field: rng.range_i64(0, 15),
            index: rng.range_i64(0, 3),
            value: *rng
                .choose(&[0, 1, 2, 3, 999_999, 9_999_999, -1, -2, 1 << 40, i64::MIN])
                .unwrap_or(&0),
        },
        8 => PatchIn::Set {
            field: *rng.choose(&[6, 8]).unwrap_or(&8),
            index: rng.range_i64(0, 3),
            value: rng.range_i64(-6, 6),
        },
        _ => PatchIn::Iges,
    }
}

fn generate(rng: &mut Rng) -> Inputs {
    let file = rng.choose(upstream_files()).cloned().unwrap_or_default();
    let mut options = default_options();
    for o in &mut options {
        if rng.chance(0.3) {
            *o = !*o;
        }
    }
    // mostly compare the full build: no healing, and no analytic surfaces (whose
    // cone recognition is not ported yet)
    if rng.chance(0.7) {
        options[5] = false;
    }
    if rng.chance(0.7) {
        options[1] = false;
    }
    let mut patches = Vec::new();
    for _ in 0..rng.below(3) {
        patches.push(gen_patch(rng));
    }
    // the ops after the build run on well-formed data only: upstream walks a B-rep
    // built from malformed indices without checks
    let malformed = patches.iter().any(|p| matches!(p, PatchIn::Set { .. }));
    let op = match if malformed { 9 } else { rng.below(10) } {
        0 => OpIn::Closure {
            do_infinite: rng.chance(0.5),
        },
        1 => OpIn::RemoveAll {
            editing: rng.chance(0.3),
        },
        _ => OpIn::Make,
    };
    Inputs {
        file,
        options,
        patches,
        op,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_distinct_per_kind() {
        let a = key_of(EntityRef::Edge(EdgeId::from_index(3)));
        let b = key_of(EntityRef::Face(FaceId::from_index(3)));
        assert_ne!(a.key, b.key);
        assert_eq!(a.type_code, 16016);
    }
}
