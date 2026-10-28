//! Rust-side reader for the `.smb` file sources the shims read natively.
//!
//! A shim reads a file source with `SmBrep::ReadFromFile` into `new SmBrep(ctx)`.
//! Runners used to rebuild their Rust B-rep from the shim's graph export of that
//! B-rep, so the Rust side's input was C++ output. [`read_file`] instead reads the
//! same file with the Rust reader (`topology::brep::p7::read_from_file`), so no
//! C++-built geometry or topology reaches the Rust side
//! (`crates/difftest/README.md`, No C++ state reaches the Rust side).
//!
//! The reader builds its arenas in its own construction order, which is not the shim's
//! export traversal (`walk` in `sc_edgeuse_graph.h`): on the 15 `topology.edgeuse_p1`
//! files the edge-use arena differs from the export order in 13 and the vertex arena in
//! 2. A runner may therefore use [`read_file`] directly only where its selectors go
//! through the B-rep's own lists (`GetEdges`, `GetFaces`, ...), as geom.tools and the
//! fillet end-to-end cases do. Runners whose selectors are export-traversal indices
//! need the read B-rep renumbered into that order first. After the read the B-rep
//! records its geometry owners under arena identities ([`Brep::use_arena_naming`]),
//! as a kernel-built B-rep does. Runners whose own `key_of` identities name the
//! geometry (the naming audit, `plan/findings/general--difftest-import-naming-audit.md`)
//! keep their naming.
//!
//! This is the file-source counterpart of `own_builder` (the shim-built sources).

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::{p10::TopologyEnv, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Result as SolidResult};

use crate::HarnessError;

/// The topology identities [`read_file`]'s reader gives the objects it builds, as
/// the `topology.brep_p7` runner keys them: one distinct key per object.
pub(crate) fn read_key_of(e: EntityRef) -> ObjectRef {
    use EntityRef as E;
    let (type_code, kind, i) = match e {
        E::Brep(x) => (16011, 0u64, x.index()),
        E::Region(x) => (16012, 1, x.index()),
        E::Shell(x) => (16013, 2, x.index()),
        E::Faceuse(x) => (16022, 3, x.index()),
        E::Face(x) => (16002, 4, x.index()),
        E::Loopuse(x) => (16004, 5, x.index()),
        E::Loop(x) => (16003, 6, x.index()),
        E::Edgeuse(x) => (16005, 7, x.index()),
        E::Edge(x) => (16016, 8, x.index()),
        E::Vertexuse(x) => (16006, 9, x.index()),
        E::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
    }
}

/// The Rust B-rep of the ASCII `.smb` file at `path`, read by the Rust reader alone
/// (`SmBrep::ReadFromFile` into `new SmBrep(ctx)`, no rebuild), as the C++ side
/// reads it. The outer error is a harness failure (the file cannot be opened); the
/// inner one is the Rust reader's own result.
pub(crate) fn read_file(k: &mut Kernel, path: &str) -> Result<SolidResult<Brep>, HarnessError> {
    read_file_rebuild(k, path, false)
}

/// [`read_file`] with `ReadFromFile`'s `bRebuildUVTrimCurves` flag: `rebuild` rebuilds
/// the UV trim curves instead of checking and fixing them.
pub(crate) fn read_file_rebuild(
    k: &mut Kernel,
    path: &str,
    rebuild: bool,
) -> Result<SolidResult<Brep>, HarnessError> {
    let bytes = std::fs::read(path).map_err(|e| HarnessError::new(format!("{path}: {e}")))?;
    Ok(read_bytes(k, bytes, rebuild))
}

/// [`read_file_rebuild`] on the file's bytes.
fn read_bytes(k: &mut Kernel, bytes: Vec<u8>, rebuild: bool) -> SolidResult<Brep> {
    let mut body = Brep::new_with_infinite_region()?;
    let mut attributes = AttributeStore::default();
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    p7::read_from_file(k, &mut body, bytes, FileType::Ascii, rebuild, &mut env)?;
    // Record the geometry owners under arena identities, as a kernel-built B-rep
    // does, so `GetFace` / `GetEdge` on that geometry resolve to the owning topology.
    body.use_arena_naming()?;
    Ok(body)
}

/// [`read_file`] where a Rust read failure is reported as a harness failure: for
/// runners whose file sources the C++ side always reads (a failed Rust read is then
/// a Rust divergence the case reports, never a deferral).
pub(crate) fn read_file_or_fail(k: &mut Kernel, path: &str) -> Result<Brep, HarnessError> {
    read_file(k, path)?.map_err(|e| HarnessError::new(format!("rust read {path}: {e}")))
}
