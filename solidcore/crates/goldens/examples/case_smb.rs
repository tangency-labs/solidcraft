//! Writes one goldens case's result bodies to `.smb` files, for `solidcore-diag` to draw.
//!
//! ```text
//! cargo run --profile goldens -p goldens --example case_smb -- <unit> <case> <out.smb> [--seeds N]
//! solidcore-diag <out.smb> --out DIR --view -35,30 --size 1200x900
//! ```
//!
//! The case is found by its exact name among the unit's cases (`--seeds` generated ones,
//! 16 by default, as `goldens`). The unit's runner rebuilds the case's bodies as its run
//! does (`result_breps` in `src/units/<unit>.rs`); the first is written to `<out.smb>`,
//! body `n > 1` to `<stem>.<n>.smb` next to it. Each is written as
//! `SmBrep::WriteToFile(name, ASCII, TRUE, FALSE)` writes it (`BrepData::from_brep`, then
//! `write_to_file_with_nurbs`, as `src/units/topology_api_files.rs` does). One line per
//! file: `path<TAB>label<TAB>faces`.
//!
//! A surface result is written as a one-face sheet over its UV domain; a case without a
//! body (a record-only op, a refused or deferred case) is reported as not drawable.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use goldens::units::{
    blend_fillet_corner_p2, intersect_surface_drop_lib_p1, offset_offset_geometry_creation,
    surface_plane_p1, surface_surface_p2, topology_brep_local_operation,
};
use goldens::{Case, CaseContext, HarnessError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::EntityRef;

const USAGE: &str = "usage: case_smb <unit> <case> <out.smb> [--seeds N]";

/// The units that can rebuild a case's bodies, with their `result_breps`.
type ResultBreps = fn(&Case) -> Result<surface_surface_p2::Drawn, HarnessError>;
const UNITS: [(&str, ResultBreps); 6] = [
    (
        "topology.brep_local_operation",
        topology_brep_local_operation::result_breps,
    ),
    (
        "offset.offset_geometry_creation",
        offset_offset_geometry_creation::result_breps,
    ),
    ("surface.plane_p1", surface_plane_p1::result_breps),
    ("surface.surface_p2", surface_surface_p2::result_breps),
    (
        "intersect.surface_drop_lib_p1",
        intersect_surface_drop_lib_p1::result_breps,
    ),
    (
        "blend.fillet_corner_p2",
        blend_fillet_corner_p2::result_breps,
    ),
];

/// A distinct key per topology object (the reader's scheme, as `topology.brep_p7` keys
/// them), for objects whose geometry records no owner.
fn object_key(e: EntityRef) -> ObjectRef {
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

/// The ASCII `.smb` contents of `b`: a face (an edge) is named by the owner its
/// surface (curve) records, every other object by [`object_key`].
fn smb_bytes(k: &Kernel, b: &Brep) -> solidcore::Result<Vec<u8>> {
    let attributes = AttributeStore::default();
    let records = NotifyRecords::new();
    let key_of = |e: EntityRef| -> ObjectRef {
        let owner = match e {
            EntityRef::Face(f) => b.face(f).ok().and_then(|x| x.surface.as_ref()?.owner()),
            EntityRef::Edge(x) => b.edge(x).ok().and_then(|y| y.curve.as_ref()?.owner()),
            _ => None,
        };
        owner.unwrap_or_else(|| object_key(e))
    };
    let env = FromBrepEnv {
        attributes: &attributes,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let mut all = Vec::new();
    let mut data = BrepData::from_brep(b, &env, &mut all)?;
    data.write_to_file_with_nurbs(k, FileType::Ascii)
}

/// `out` for body 1, `<stem>.<n>.smb` beside it for body `n`.
fn body_path(out: &Path, n: usize) -> PathBuf {
    if n == 1 {
        return out.to_path_buf();
    }
    let stem = out
        .file_stem()
        .map_or_else(|| "body".into(), |s| s.to_string_lossy().into_owned());
    out.with_file_name(format!("{stem}.{n}.smb"))
}

fn run(args: &[String]) -> Result<(), String> {
    let mut seeds = 16;
    let mut pos = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--seeds" {
            let v = it.next().ok_or(USAGE)?;
            seeds = v.parse().map_err(|_| format!("bad --seeds `{v}`"))?;
        } else {
            pos.push(a.as_str());
        }
    }
    let &[unit_id, case_name, out] = pos.as_slice() else {
        return Err(USAGE.into());
    };
    let Some(&(_, result_breps)) = UNITS.iter().find(|(id, _)| *id == unit_id) else {
        let ids: Vec<&str> = UNITS.iter().map(|(id, _)| *id).collect();
        return Err(format!(
            "unit `{unit_id}` cannot rebuild its bodies; units: {}",
            ids.join(", ")
        ));
    };
    let unit = goldens::registry()
        .into_iter()
        .find(|u| u.id() == unit_id)
        .ok_or_else(|| format!("unit `{unit_id}` is not registered"))?;
    let ctx = CaseContext {
        root: goldens::case::repo_root(),
        seeds,
    };
    let case = unit
        .cases(&ctx)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.name == case_name)
        .ok_or_else(|| format!("unit `{unit_id}` has no case `{case_name}` (--seeds {seeds})"))?;
    let (k, bodies) = result_breps(&case).map_err(|e| e.to_string())?;
    if bodies.is_empty() {
        return Err(format!("case `{case_name}`: no body to draw"));
    }
    for (n, (label, b)) in bodies.iter().enumerate() {
        let path = body_path(Path::new(out), n + 1);
        let bytes = smb_bytes(&k, b).map_err(|e| format!("{label}: write: {e}"))?;
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}\t{label}\t{}", path.display(), b.faces().len());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("case_smb: {e}");
            ExitCode::FAILURE
        }
    }
}
