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
//! `ints`, doubles to `dbls` (subnormals as 0; pure reader/build cases also audit raw
//! `dbl_bits`), `kinds` the interleaving. A Rust `InvalidInput` is the guard's `-2`.
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
    make_topology_from_data, project_and_trim, remove_all_topology_notified,
    test_closure_split_region, MakeTopologyOptions, ProjectAndTrimOptions, ProjectEnv, TopologyEnv,
    TrimType, HEALER_VERSION,
};
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::brep_data::{BrepData, TopologyData};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::topology::Topology;
use solidcore::topology::{
    Brep, BrepId, EdgeId, EdgeuseId, EdgeuseOwner, FaceId, FaceuseId, LoopId, LoopuseId,
    LoopuseStart, RegionId, ShellId, VertexId, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, Kernel, SolidError};
use solidcore_ffi::units::topology_brep_p10::{self as ffi, Item, Op, Patch, Request};
use super::topology_vertexuse::CrvIn;

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
    /// Exact borrowed NURB input; unlike the copying constructors, neither engine tunes knots.
    #[serde(default)]
    projection_curve: Option<CrvIn>,
    /// Exercise whole-face classification after an intentionally empty projection subset.
    #[serde(default)]
    empty_projection_subset: bool,
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

/// Whether `i` runs only the `.smb` reader and pointer-wiring build (no analytic
/// surface recognition, healer, closure ray-fire or curve projection), so every float
/// can be audited bit for bit (`dbl_bits`). Once geometry solvers run, 1–3 ULP
/// transcendental (`sin`/`cos`/`atan2`) differences between system `libm` and Rust are
/// compared through `dbls` under the standard `1e-9` relative tolerance (as in
/// [`snapshot`]).
fn bit_audit(i: &Inputs) -> bool {
    let [_, add_analytics, _, _, _, healer_enabled] = i.options;
    !add_analytics && !healer_enabled && matches!(i.op, OpIn::Make | OpIn::RemoveAll { .. })
}

fn to_output(items: &[Item], include_bits: bool) -> Output {
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
    let out = Output::new()
        .ints("ints", ints)
        .values("dbls", dbls)
        .ints("kinds", kinds);
    if include_bits {
        out.ints("dbl_bits", bits)
    } else {
        out
    }
}

/// Exposes stable pre-operation face identities as well as the complete geometry dump.
fn operation_output(items: &[Item], i: &Inputs) -> Result<Output, HarnessError> {
    let out = to_output(items, bit_audit(i));
    if i.projection_curve.is_none() || !matches!(i.op, OpIn::Project { .. }) || items.len() <= 1 {
        return Ok(out);
    }
    let Some(Item::Int(count)) = items.last() else {
        return Err(harness("missing retained-face count"));
    };
    let count = usize::try_from(*count).map_err(|_| harness("negative retained-face count"))?;
    let end = items.len() - 1;
    let start = end.checked_sub(count).ok_or_else(|| harness("short face inventory"))?;
    let original = start.checked_sub(1).and_then(|n| items.get(n));
    let Some(Item::Int(original)) = original else {
        return Err(harness("missing original-face count"));
    };
    let retained = items.get(start..end).ok_or_else(|| harness("short face inventory"))?
        .iter().map(|item| match item {
            Item::Int(id) => Ok(*id),
            Item::Dbl(_) => Err(harness("non-integer face identity")),
        }).collect::<Result<Vec<_>, _>>()?;
    Ok(out.ints("face_counts", vec![*original, count as i64])
        .ints("retained_original_faces", retained))
}

/// Why the C++ side must not build these inputs, if it must not: requests on which
/// `MakeTopologyFromData` runs the healer (`bHealerIsEnabled` and either `bAlwaysHeal`
/// or an old healer version) over a B-rep it builds unchecked from patched records.
///
/// * After `Set` patches that change the data and leave records the build accepts
///   (the Rust build, healing off, does not return `InvalidInput`; data it refuses is
///   the shim's `-2`, and upstream never runs): the build keeps edge uses linked into
///   loops their records do not name, loop rings of the wrong length or edges on
///   another edge's curve, and the healer walks that B-rep unchecked (SIGSEGV and
///   hangs in the C++ reference, UB-164; the ops after the build would too, and the
///   FFI refuses those itself). A `Set` that changes nothing is compared.
/// * After an `Iges` patch whose kept shells leave a face use record in no face use
///   shell (the crafted spine fixtures: region 1 has no shells): that face use is built
///   without a shell and `SmHealData::Fix_BadSheets` hands it to
///   `SmBrep::TestClosureSplitRegion`, which reads `pFUStart->GetShell()->GetRegion()`
///   through the null shell (`SmBrep.cpp:27909`, SIGSEGV).
///
/// The data is read and patched with the Rust reader (the same records upstream reads).
fn heal_guard(i: &Inputs) -> Option<&'static str> {
    let [_, _, _, _, always_heal, healer_enabled] = i.options;
    if !healer_enabled {
        return None;
    }
    let path = path_of(i).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    let k = Kernel::default();
    let mut data = BrepData::read_from_file(&k, bytes.clone(), FileType::Ascii).ok()?;
    apply_patches(&mut data, &i.patches).ok()?;
    if !(always_heal || data.healer_version() < HEALER_VERSION) {
        return None;
    }
    if changes_indices(&k, &bytes, &i.patches) && builds(i, &data) {
        return Some("healing a b-rep a set patch left inconsistent");
    }
    if i.patches.iter().any(|p| matches!(p, PatchIn::Iges)) {
        let in_shell = |f: usize| {
            data.shells.iter().any(|s| {
                s.shell_type == 0
                    && usize::try_from(s.faceuse_start).is_ok_and(|a| {
                        usize::try_from(s.num_faceuses).is_ok_and(|n| f >= a && f - a < n)
                    })
            })
        };
        if !(0..data.faceuses.len()).all(in_shell) {
            return Some("healing a face use the iges patch left without a shell");
        }
    }
    None
}

/// Whether the case's `Set` patches change the patched data: the data with every patch
/// differs from the data with the same patches but its `Set` patches left out. A `Set`
/// past its array, or of a field to the value it holds, changes nothing.
fn changes_indices(k: &Kernel, bytes: &[u8], patches: &[PatchIn]) -> bool {
    let without: Vec<PatchIn> = patches
        .iter()
        .filter(|p| !matches!(p, PatchIn::Set { .. }))
        .copied()
        .collect();
    let patched = |ps: &[PatchIn]| {
        let mut d = BrepData::read_from_file(k, bytes.to_vec(), FileType::Ascii).ok()?;
        apply_patches(&mut d, ps).ok()?;
        Some(format!("{d:?}"))
    };
    match (patched(patches), patched(&without)) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

/// Whether upstream gets past the build to the healer: false only when the Rust build
/// (healing off) returns `InvalidInput`, the data the shim's simulation of the build's
/// record indexing keeps from upstream (`-2`, UB-164), so upstream never runs.
fn builds(i: &Inputs, data: &BrepData) -> bool {
    let mut k = Kernel::default();
    let mut data = data.clone();
    let Ok(mut b) = Brep::new_with_infinite_region() else {
        return true;
    };
    let mut store = AttributeStore::new();
    let mut records: solidcore::topology::topology::NotifyRecords = solidcore::topology::topology::NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    let [make_uv_trim_curves, add_analytics, force_same_tol, check_uv_trim_curves, always_heal, _] =
        i.options;
    let options = MakeTopologyOptions {
        make_uv_trim_curves,
        add_analytics,
        force_same_tol,
        check_uv_trim_curves,
        always_heal,
        healer_enabled: false,
    };
    !matches!(
        make_topology_from_data(&mut k, &mut b, &mut data, &mut env, options),
        Err(SolidError::InvalidInput { .. })
    )
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let Some(why) = heal_guard(i) {
        // C++ undefined behaviour: neither side runs (not a harness error, which a
        // normal run would count as a failure)
        return Ok(Output::rejected(format!("guard: {why}")));
    }
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
        projection_curve: i.projection_curve.as_ref().map(|c| ffi::ProjectionCurve {
            degree: c.p,
            control_points: c.cpts.clone(),
            knots: c.u.clone(),
        }),
        empty_projection_subset: i.empty_projection_subset,
    };
    let items = match ffi::run(&req) {
        Ok(items) => items,
        Err(solidcore_ffi::FfiError::Status(ffi::STATUS_UNDEFINED_PROJECTION_DEGREE)) => {
            return Ok(Output::rejected(
                "projecting a raw curve above NL_MAXDEG into a face overruns native surface scratch arrays",
            ));
        }
        Err(e) => return Err(harness(format!("ffi: {e}"))),
    };
    operation_output(&items, i)
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
            // the shim's `(SmBoolean)rd.i()`: an `int64_t` to `int` conversion, which
            // keeps the low 32 bits (GCC), as `as i32` does
            PatchIn::Manifold(v) => d.manifold = v as i32,
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
    let mut records: solidcore::topology::topology::NotifyRecords = solidcore::topology::topology::NotifyRecords::new();
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
    export(&k, &b, &g, &records, &mut out).map_err(|e| harness(e.to_string()))?;

    match i.op {
        OpIn::Make => {}
        OpIn::Closure { do_infinite } => {
            // the split names the B-rep's objects as the build did (`key_of`, the names
            // its geometry owners carry, which its ray fires look up), over the build's
            // records and attribute store
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut store,
                context: None,
                key_of: &key_of,
            };
            for &e in &g.edges.items {
                match test_closure_split_region(&mut k, &mut b, &mut env, do_infinite, e) {
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
            export(&k, &b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
        }
        OpIn::RemoveAll { editing } => {
            b.set_editing_enabled(editing);
            // the runner's records: the merged and deleted objects propagate their
            // flags and user values into the survivors, as upstream's objects do
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut store,
                context: None,
                key_of: &key_of,
            };
            match remove_all_topology_notified(&mut k, &mut b, &mut env) {
                Ok(()) => out.push(Item::Int(0)),
                Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
                Err(e) => out.push(Item::Int(status(&e))),
            }
            let g2 = Graph::walk(&b).map_err(|e| harness(e.to_string()))?;
            export(&k, &b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
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
            let curve_result = if let Some(c) = &i.projection_curve {
                let points = c.cpts.iter().map(|p| {
                    solidcore::nurbs::math::CPoint::new(p[0], p[1], p[2], p[3])
                }).collect();
                solidcore::nurbs::crv_basic::Curve::new(points, c.p, c.u.clone())
                    .map(|nurb| BSplineCurve::from_nurb(nurb, c.dim, true))
            } else {
                BSplineCurve::create_line_segment(3, v(a), v(end))
            };
            let curve = match curve_result {
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
                subset_faces: i.empty_projection_subset.then_some(&[]),
            };
            // the merges' edits notify the build's records and attribute store under
            // the build's names
            let mut notify = NotifyEnv {
                records: &mut records,
                attributes: &mut store,
                context: None,
                key_of: &key_of,
            };
            match project_and_trim(
                &mut k,
                &mut b,
                &penv,
                &mut notify,
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
            export(&k, &b, &g2, &records, &mut out).map_err(|e| harness(e.to_string()))?;
            if i.projection_curve.is_some() {
                out.push(Item::Int(g.faces.items.len() as i64));
                let mut retained = 0;
                for (index, id) in g.faces.items.iter().enumerate() {
                    if g2.faces.items.contains(id) {
                        out.push(Item::Int(index as i64));
                        retained += 1;
                    }
                }
                out.push(Item::Int(retained));
            }
        }
    }
    Ok(Ok(out))
}

// ---- traversal-order registry (mirrors the shim's walk) ---------------------------------

pub(crate) struct Reg<I> {
    pub(crate) items: Vec<I>,
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
    pub(crate) fn find(&self, id: Option<I>, raw: impl Fn(I) -> u32) -> i64 {
        match id {
            None => -1,
            Some(id) => self.index.get(&raw(id)).copied().unwrap_or(-3),
        }
    }
}

/// The B-rep's objects in the shim's traversal order (shared with `topology.brep_p6`).
pub(crate) struct Graph {
    pub(crate) regions: Reg<RegionId>,
    pub(crate) shells: Reg<ShellId>,
    pub(crate) faceuses: Reg<FaceuseId>,
    pub(crate) faces: Reg<FaceId>,
    pub(crate) loopuses: Reg<LoopuseId>,
    pub(crate) loops: Reg<LoopId>,
    pub(crate) edgeuses: Reg<EdgeuseId>,
    pub(crate) edges: Reg<EdgeId>,
    pub(crate) vertexuses: Reg<VertexuseId>,
    pub(crate) vertices: Reg<VertexId>,
}

const LOOPUSE_TYPE: i64 = 16004;
const EDGEUSE_TYPE: i64 = 16005;
const VERTEXUSE_TYPE: i64 = 16006;
const SHELL_TYPE: i64 = 16013;
const UNKNOWN_TYPE: i64 = 16999;

pub(crate) fn orient_code(o: solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType) -> i64 {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

impl Graph {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn walk(b: &Brep) -> Result<Self, SolidError> {
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
        // GetRegions starts at the owning list head, which ResetInfiniteRegion
        // changes without reallocating the region records.
        for id in solidcore::topology::brep::p1::regions(b, None)? {
            g.regions.add(Some(id), RegionId::index);
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
    key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
    uv_owner_matches: Option<&'a dyn Fn(EdgeuseId, ObjectRef) -> bool>,
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
            .get(&(self.key_of)(e).key)
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
            self.point(c.evaluate_point(&Kernel::default(), t));
        }
        let matches = c.owner().is_some_and(|actual| {
            actual == (self.key_of)(owner)
                || matches!(owner, EntityRef::Edgeuse(holder)
                    if self.uv_owner_matches.is_some_and(|resolve| resolve(holder, actual)))
        });
        self.i(i64::from(matches));
    }
    fn surface(&mut self, k: &Kernel, s: Option<&Surface>, owner: EntityRef) {
        let Some(s) = s else {
            self.i(0);
            return;
        };
        self.i(1);
        self.i(s.type_code());
        // (runner helper that does not return Result)
        let dom: Extent2d = s.natural_uv_domain_if_built();
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
                self.point(s.evaluate_point(k, uv));
            }
        }
        self.i(i64::from(s.owner() == Some((self.key_of)(owner))));
    }
}

#[allow(clippy::too_many_lines)]
fn export(
    k: &Kernel,
    b: &Brep,
    g: &Graph,
    records: &BTreeMap<ObjectKey, Topology>,
    o: &mut Vec<Item>,
) -> Result<(), SolidError> {
    export_with_identity(k, b, g, records, &key_of, None, o)
}

#[allow(clippy::too_many_lines)]
fn export_with_identity(
    k: &Kernel,
    b: &Brep,
    g: &Graph,
    records: &BTreeMap<ObjectKey, Topology>,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
    uv_owner_matches: Option<&dyn Fn(EdgeuseId, ObjectRef) -> bool>,
    o: &mut Vec<Item>,
) -> Result<(), SolidError> {
    let mut w = Out {
        o,
        records,
        key_of,
        uv_owner_matches,
    };
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
        w.surface(k, f.surface.as_ref(), EntityRef::Face(id));
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
pub(crate) fn snapshot(k: &Kernel, b: &Brep) -> Result<Output, HarnessError> {
    snapshot_impl(k, b, &BTreeMap::new(), &key_of, None)
}

/// The same snapshot with the actual records and topology issuer of a constructed body.
pub(crate) fn snapshot_with_identity(
    k: &Kernel,
    b: &Brep,
    records: &BTreeMap<ObjectKey, Topology>,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<Output, HarnessError> {
    // The UV setter takes the body's naming, not this snapshot's topology issuer.
    // Use only the existing public producer callback; these seeded records are not
    // substituted for the actual edit records passed to the snapshot.
    let mut producer = solidcore::topology::brep::p2::ArenaNotifyState::new(b);
    let producer_key_of = producer.env(None).key_of;
    let uv_owner_matches =
        |holder, owner| notify_uv_owner_matches(b, producer_key_of, holder, owner);
    snapshot_impl(k, b, records, key_of, Some(&uv_owner_matches))
}

/// Resolve a Notify UV owner to its exact current holder, not merely its edge.
/// Other geometry keeps the explicit caller issuer; no global owner aliases are inferred.
pub(crate) fn notify_uv_owner_matches(
    body: &Brep,
    producer_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    holder: EdgeuseId,
    owner: ObjectRef,
) -> bool {
    if body.naming() != solidcore::topology::NamingScheme::Notify
        || body
            .edgeuse(holder)
            .ok()
            .and_then(|eu| eu.uv_trim_curve.as_ref())
            .and_then(Curve::owner)
            != Some(owner)
    {
        return false;
    }
    let mut matches = body
        .edgeuses()
        .iter()
        .filter_map(|(id, _)| (producer_key_of(EntityRef::Edgeuse(id)) == owner).then_some(id));
    matches.next() == Some(holder) && matches.next().is_none()
}

fn snapshot_impl(
    k: &Kernel,
    b: &Brep,
    records: &BTreeMap<ObjectKey, Topology>,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
    uv_owner_matches: Option<&dyn Fn(EdgeuseId, ObjectRef) -> bool>,
) -> Result<Output, HarnessError> {
    let g = Graph::walk(b).map_err(|e| harness(e.to_string()))?;
    let mut items = Vec::new();
    export_with_identity(k, b, &g, records, key_of, uv_owner_matches, &mut items)
        .map_err(|e| harness(e.to_string()))?;
    // This caller uses the normal relative comparison, not p10's bit audit.
    let mut output = to_output(&items, false).int("validate_only", i64::from(b.validate_only()));
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
        let (next, last) = solidcore::topology::shell::faceuse_sibling_links(b, *id)
            .map_err(|e| harness(e.to_string()))?;
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
        let inputs: Inputs = case.parse()?;
        match rust(&inputs)? {
            Ok(items) => operation_output(&items, &inputs),
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
        projection_curve: None,
        empty_projection_subset: false,
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
