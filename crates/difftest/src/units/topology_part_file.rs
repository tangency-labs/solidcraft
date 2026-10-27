//! Difftest runner for the part reader (`topology::part_file`, outside the plan:
//! `SmBrepData::ReadPartFromFile` is not a plan unit, see `plan/findings.md`). Run it
//! with `--unit topology.part_file`; `--green` does not include it.
//!
//! One case per upstream part file under `upstream/TestFiles/pt_TestFiles` (the `.smp`
//! files and the `.smb` files that start with `//[Output Summary]`). Both sides read the
//! file and dump, as `out`: the stand-alone curves (type, dimension, natural interval),
//! the surfaces (type, natural UV domain), the Boolean tree nodes and per B-rep its
//! tolerance, manifold-solid flag and region, shell, face, loop, edge and vertex counts
//! (layout in `solidcore_ffi::units::topology_part_file`). A read failure is compared
//! as a status only. The B-reps go through `MakeTopologyFromData`, so a case defers
//! wherever that path still reaches an unported call.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as fl;
use solidcore::topology::part_file::{read_part_from_file, Part, PartEnv};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_part_file as ffi;

use crate::case::{repo_root, CaseContext};
use crate::fixtures::UPSTREAM_TEST_FILES;
use crate::{Case, CaseSource, HarnessError, Output, UnitDiff};

/// Status output of a failed read (compared on both sides).
const READ_FAILED: &str = "read failed";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// The part file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
}

/// Differential runner for `SmBrepData::ReadPartFromFile`.
pub struct Runner;

fn path_of(rel: &str) -> String {
    repo_root()
        .join(UPSTREAM_TEST_FILES)
        .join(rel)
        .to_string_lossy()
        .into_owned()
}

/// Every part file under the upstream test files, sorted.
fn part_files() -> Vec<String> {
    let root = repo_root().join(UPSTREAM_TEST_FILES);
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
            } else if is_part_file(&p) {
                if let Ok(rel) = p.strip_prefix(&root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out.sort();
    out
}

fn is_part_file(p: &Path) -> bool {
    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or_default();
    (ext == "smp" || ext == "smb")
        && std::fs::read(p).is_ok_and(|b| b.starts_with(b"//[Output Summary]"))
}

/// The identity of topology entity `e` of B-rep `brep` (geometry keys start at `1 << 62`).
fn key_of(brep: usize, e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
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
        type_code,
        key: ObjectKey(((brep as u64 + 1) << 36) | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// The dump of a part read on the Rust side (the shim's layout).
fn dump(k: &mut Kernel, part: &Part) -> Result<Vec<f64>, SolidError> {
    let mut o = vec![0.0, part.curves.len() as f64];
    for c in &part.curves {
        let ivl = c.object.natural_interval();
        let (ty, dim) = (c.object.type_code() as f64, c.object.dim() as f64);
        o.extend([ty, dim, ivl.min(), ivl.max()]);
    }
    o.push(part.surfaces.len() as f64);
    for s in &part.surfaces {
        let d = s.object.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (d.min(), d.max());
        o.extend([s.object.type_code() as f64, lo.x, lo.y, hi.x, hi.y]);
    }
    o.push(part.boolean_tree_nodes.len() as f64);
    o.extend(part.boolean_tree_nodes.iter().map(|&n| n as f64));
    o.push(part.breps.len() as f64);
    for b in &part.breps {
        let faces = bl::faces(b, None)?;
        let mut loops = 0usize;
        for &f in &faces {
            loops += fl::loops(b, f, false, None)?.len();
        }
        o.push(b.tolerance());
        o.push(f64::from(u8::from(is_manifold_solid(b)?)));
        o.push(bl::regions(b, None)?.len() as f64);
        o.push(bl::shells(k, b, None)?.len() as f64);
        o.push(faces.len() as f64);
        o.push(loops as f64);
        o.push(bl::edges(b, None)?.len() as f64);
        o.push(bl::vertices(b, None)?.len() as f64);
    }
    Ok(o)
}

fn rust(rel: &str) -> Result<Output, HarnessError> {
    let bytes =
        std::fs::read(path_of(rel)).map_err(|e| HarnessError::new(format!("{rel}: {e}")))?;
    let mut k = Kernel::default();
    let mut store = AttributeStore::new();
    let mut records = BTreeMap::new();
    let mut env = PartEnv {
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 62,
    };
    let result = read_part_from_file(&mut k, bytes, FileType::Ascii, &mut env)
        .and_then(|part| dump(&mut k, &part));
    Ok(match result {
        Ok(v) => Output::new().values("out", v),
        Err(e) => Output::unported(&e).unwrap_or_else(|| Output::err(READ_FAILED)),
    })
}

fn cpp(rel: &str) -> Result<Output, HarnessError> {
    let v = ffi::run(&path_of(rel)).map_err(|e| HarnessError::new(format!("c++: {e}")))?;
    Ok(match v.first() {
        Some(&0.0) => Output::new().values("out", v),
        st => {
            // (the status only goes to stderr: both sides' errors compare as READ_FAILED)
            eprintln!(
                "topology.part_file: c++ {rel}: status {st:?}, {} values",
                v.len()
            );
            Output::err(READ_FAILED)
        }
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.part_file"
    }

    fn cases(&self, _ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        Ok(part_files()
            .into_iter()
            .map(|rel| Case {
                unit: self.id().to_string(),
                name: rel.clone(),
                source: CaseSource::Inline,
                inputs: serde_json::to_value(Inputs { file: rel }).unwrap_or(Value::Null),
            })
            .collect())
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        cpp(&i.file)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        rust(&i.file)
    }
}
