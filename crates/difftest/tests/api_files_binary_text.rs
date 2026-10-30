//! `SmApiReadBrepFromFile` / `SmApiReadPartFromFile` with `bAscii = FALSE` on every
//! upstream test file (all ASCII): the Rust binary readers must return an error for each,
//! never panic or abort on an allocation sized by a count read out of the text
//! (`DatabaseReader::ensure_available` before allocating, no pre-reserve from file
//! counts; AGENTS.md section 1). An over-allocation aborts this test binary, a panic is
//! caught and reported per file.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::api_files::{read_brep_from_file, read_part_from_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::part_file::{Part, PartEnv};
use solidcore::topology::topology::NotifyRecords;
use solidcore::EntityRef;

/// A distinct identity per topology object (the reader only names them).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
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
        key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
    }
}

/// Every file under `root` with extension `ext`, sorted.
fn files(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/TestFiles/pt_TestFiles");
    if root.is_dir() {
        Some(root)
    } else {
        eprintln!("upstream test files missing: {}", root.display());
        None
    }
}

/// What reading `bytes` as binary did: `Ok(true)` an error (as required), `Ok(false)` a
/// success, `Err` a panic.
fn read_brep_binary(bytes: Vec<u8>) -> Result<bool, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut k = Kernel::default();
        let mut attributes = AttributeStore::default();
        let mut records = NotifyRecords::new();
        let mut env = TopologyEnv {
            all_attributes: &[],
            attributes: &mut attributes,
            key_of: &key_of,
            records: &mut records,
            next_key: 1u64 << 50,
        };
        read_brep_from_file(&mut k, Some(bytes), false, false, &mut env).is_err()
    }))
    .map_err(|_| "panicked".to_string())
}

fn read_part_binary(bytes: Vec<u8>) -> Result<bool, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut k = Kernel::default();
        let mut store = AttributeStore::new();
        let mut records = NotifyRecords::new();
        let part_key = |b: usize, e: EntityRef| {
            let r = key_of(e);
            ObjectRef {
                type_code: r.type_code,
                key: ObjectKey(((b as u64 + 1) << 52) | r.key.0),
            }
        };
        let mut env = PartEnv {
            attributes: &mut store,
            key_of: &part_key,
            records: &mut records,
            next_key: 1 << 62,
        };
        let mut out = Part {
            curves: Vec::new(),
            surfaces: Vec::new(),
            boolean_tree_nodes: Vec::new(),
            breps: Vec::new(),
        };
        read_part_from_file(&mut k, Some(bytes), false, &mut out, &mut env).is_err()
    }))
    .map_err(|_| "panicked".to_string())
}

#[test]
fn every_ascii_test_file_read_as_binary_is_an_error() {
    let Some(root) = root() else {
        return;
    };
    let mut bad = Vec::new();
    let mut checked = 0;
    for (ext, read) in [
        (
            "smb",
            read_brep_binary as fn(Vec<u8>) -> Result<bool, String>,
        ),
        ("smp", read_part_binary),
    ] {
        for path in files(&root, ext) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            checked += 1;
            match read(bytes) {
                Ok(true) => {}
                Ok(false) => bad.push(format!("{}: read as binary succeeded", path.display())),
                Err(why) => bad.push(format!("{}: {why}", path.display())),
            }
        }
    }
    assert!(
        checked > 0,
        "no upstream test file found under {}",
        root.display()
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
