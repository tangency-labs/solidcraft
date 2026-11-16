//! Runner for unit `corpus.parity`: every real model file under
//! `upstream/TestFiles/pt_TestFiles` (188 `.smb` B-rep files and 74 `.smp` part files) and
//! the generated case inputs of `REPO_MODEL_DIRS`, read and measured by the port and
//! checked against the unit's golden. Ported from the readers `SmBrep::ReadFromFile` and
//! `SmBrepData::ReadPartFromFile` and the measures of `crates/solidcore/examples/corpus`;
//! it compared them with upstream's until the goldens replaced that comparison.
//!
//! One case per file. The file is read and every B-rep measured: the kind
//! (`IsManifoldSolid`, else `IsSheet`, else other; empty without faces, edges and
//! vertices), the face, edge, vertex, loop and shell counts, the validity check's result
//! (`check_brep` at level 2, its geometry owners taken as the geometry records them, as
//! [`crate::soundness`] does: the number of failure lines, `-1` when the check itself
//! errs), the area (`SmApiFaceComputeArea`'s per-face integration without its accuracy
//! clamp) and, for a solid, the volume (`SmApiBrepComputeVolume`), at integration accuracy
//! [`ACCURACY`]. The counts are `ints`, one entry per B-rep; area and volume are `values`
//! under the 1e-9 relative rule (volume NaN unless a solid).
//!
//! A file that reads is then round-tripped: every B-rep written as an ASCII `.smb`
//! (`SmBrep::WriteToFile`: `SmBrepData::FromBrep`, `WriteToFile`), read back into a new
//! B-rep and measured again, recorded under the same names with the prefix `rt_`.
//!
//! An error of the kernel (reading, measuring, writing) is recorded as the case's error,
//! named by its stage (`round trip: ` before a round trip's), with what was measured
//! before it; an `Unported` callee defers the case. Each side runs in a child process
//! (`goldens --corpus-parity-child <rust|rust-roundtrip> <path> <accuracy>`), so a crash
//! or a hang (timeout [`TIMEOUT`]) fails only that file, as a harness error: never
//! blessed.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::{is_manifold_solid, is_sheet};
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep_with_keys, CheckLevel};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::{self as fl, PPF_AREA};
use solidcore::topology::part_file::{read_part_from_file, PartEnv};
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, SolidError};

use crate::case::{CaseContext, CaseSource};
use crate::{Case, HarnessError, Output, UnitDiff};

/// Integration accuracy of the areas and volumes (relative).
pub const ACCURACY: f64 = 1e-6;
/// Longest a side may take on one file.
pub const TIMEOUT: Duration = Duration::from_secs(300);
/// The child-process flag of `goldens`.
pub const CHILD_FLAG: &str = "--corpus-parity-child";
/// Deepest directory level searched below the test files.
const MAX_DEPTH: usize = 32;
/// Repository directories of generated `.smb` case inputs other runners read, measured
/// here too so a change in those runners' results that comes from the reader shows here
/// (`offset.offset_executive_p2`'s `issue1528_plate_api_shell_2` shells
/// `issue1528_plate.smb`). Their case names keep the repository-relative path.
const REPO_MODEL_DIRS: [&str; 1] = ["fixtures/offset.offset_executive_p2/data"];
/// The integer quantities of one B-rep's record, in order (the area and volume follow).
const COUNTS: [&str; 7] = ["kind", "faces", "edges", "vertices", "loops", "shells", "check"];
/// Values per B-rep in a measurement: [`COUNTS`], area, volume.
const PER_BREP: usize = COUNTS.len() + 2;
/// First identity of the geometry the readers make (topology identities stay below).
const GEOMETRY_KEYS: u64 = 1 << 62;

/// `corpus.parity` runner.
pub struct Runner;

#[derive(Serialize, Deserialize)]
struct Inputs {
    /// The model file, relative to `upstream/TestFiles/pt_TestFiles` (a
    /// `REPO_MODEL_DIRS` file: relative to the repository root).
    file: String,
    /// Its absolute path.
    path: String,
}

// ---- the measurements -----------------------------------------------------------------

/// What one side made of a file.
#[derive(Debug, Clone, PartialEq)]
enum Measured {
    /// [`PER_BREP`] values per B-rep measured, and the kernel's error that stopped the
    /// side, if one did (its stage and message).
    Values {
        records: Vec<f64>,
        error: Option<String>,
    },
    /// The port stopped at an unported callee.
    Unported { upstream: String, unit: String },
    /// The side did not run to an answer: a crash, a timeout, bad child output.
    Broken(String),
}

// ---- the Rust side --------------------------------------------------------------------

/// A distinct identity for every topology object of B-rep `brep` the readers build
/// (as the corpus example gives them).
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
    let brep = (brep as u64 + 1) << 36;
    ObjectRef {
        type_code,
        key: ObjectKey(brep | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// A model file as read: its B-reps and the records and attributes the readers made.
struct Read {
    breps: Vec<Brep>,
    attributes: AttributeStore,
    records: solidcore::topology::topology::NotifyRecords,
}

/// The B-reps of a model file: one from a B-rep file, any number from a part file.
fn read_breps(k: &mut Kernel, bytes: Vec<u8>) -> solidcore::Result<Read> {
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    if bytes.starts_with(b"//[Output Summary]") {
        let mut env = PartEnv {
            attributes: &mut attributes,
            key_of: &key_of,
            records: &mut records,
            next_key: GEOMETRY_KEYS,
        };
        let breps = read_part_from_file(k, bytes, FileType::Ascii, &mut env)?.breps;
        return Ok(Read {
            breps,
            attributes,
            records,
        });
    }
    let mut b = Brep::new_with_infinite_region()?;
    let one = |e| key_of(0, e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &one,
        records: &mut records,
        next_key: GEOMETRY_KEYS,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env)?;
    Ok(Read {
        breps: vec![b],
        attributes,
        records,
    })
}

/// The area at `accuracy`: `SmApiFaceComputeArea`'s per-face integration without its
/// accuracy clamp.
fn integrated_area(
    k: &Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    accuracy: f64,
) -> solidcore::Result<f64> {
    let mut area = 0.0;
    for f in faces {
        let bbox = solidcore::query::face::p1::calculate_bounding_box(k, &*b, *f, false)?;
        let s = bbox.size();
        let d = s.x.max(s.y).max(s.z);
        let p = face_properties(
            k,
            b,
            *f,
            OrientType::Same,
            accuracy,
            bbox.mid(),
            d * d / 100.0,
            0.0,
            PPF_AREA,
        )?;
        area += p.area.abs();
    }
    Ok(area)
}

/// The validity check's result: the number of `check_brep` level-2 failure lines, the
/// geometry owners taken as the geometry records them (an edge's curve's, a face's
/// surface's, an edge use's trim curve's), as [`crate::soundness`] checks a body; `-1`
/// when the check errs.
fn check_lines(k: &mut Kernel, b: &mut Brep) -> f64 {
    let mut owners = std::collections::BTreeMap::new();
    for (id, e) in b.edges().iter() {
        if let Some(o) = e.curve.as_ref().and_then(solidcore::curve::curve::Curve::owner) {
            owners.insert(EntityRef::Edge(id), o);
        }
    }
    for (id, f) in b.faces().iter() {
        if let Some(o) = f
            .surface
            .as_ref()
            .and_then(solidcore::surface::surface::Surface::owner)
        {
            owners.insert(EntityRef::Face(id), o);
        }
    }
    for (id, u) in b.edgeuses().iter() {
        if let Some(o) = u
            .uv_trim_curve
            .as_ref()
            .and_then(solidcore::curve::curve::Curve::owner)
        {
            owners.insert(EntityRef::Edgeuse(id), o);
        }
    }
    let keys = |e: EntityRef| {
        owners.get(&e).copied().unwrap_or(ObjectRef {
            type_code: 0,
            key: ObjectKey(0),
        })
    };
    check_brep_with_keys(k, b, CheckLevel::Level2, &keys)
        .map_or(-1.0, |c| c.failure_count() as f64)
}

/// One B-rep's record: [`COUNTS`], area, volume.
fn measure(
    k: &mut Kernel,
    b: &mut Brep,
    accuracy: f64,
    out: &mut Vec<f64>,
) -> solidcore::Result<()> {
    let faces = bl::faces(b, None)?;
    let edges = bl::edges(b, None)?;
    let vertices = bl::vertices(b, None)?;
    let mut loops = 0;
    for f in &faces {
        loops += fl::loops(b, *f, false, None)?.len();
    }
    let kind = if faces.is_empty() && edges.is_empty() && vertices.is_empty() {
        0.0
    } else if is_manifold_solid(b)? {
        1.0
    } else if is_sheet(k, b)? {
        2.0
    } else {
        3.0
    };
    let shells = bl::shells(k, b, None)?.len();
    let area = integrated_area(k, b, &faces, accuracy)?;
    let mut volume = f64::NAN;
    if kind == 1.0 {
        let store = AttributeStore::new();
        let key = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key,
        };
        volume = brep_volume(k, b, accuracy, &attributes)?;
    }
    out.extend([
        kind,
        faces.len() as f64,
        edges.len() as f64,
        vertices.len() as f64,
        loops as f64,
        shells as f64,
    ]);
    // the check builds caches and names geometry owners: it runs after the measures
    let check = check_lines(k, b);
    out.extend([check, area, volume]);
    Ok(())
}

/// A kernel error as a side's result: an unported callee defers; any other error stops
/// the side at `stage`, keeping the records measured before it.
fn stopped(records: Vec<f64>, stage: &str, e: SolidError) -> Measured {
    match e {
        SolidError::Unported { upstream, unit } => Measured::Unported {
            upstream: upstream.to_string(),
            unit: unit.to_string(),
        },
        e => Measured::Values {
            records,
            error: Some(format!("{stage}: {e}")),
        },
    }
}

/// The Rust side on one file.
fn measure_rust(path: &Path, accuracy: f64) -> Measured {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Measured::Broken(format!("read {}: {e}", path.display())),
    };
    let mut k = Kernel::default();
    let mut read = match read_breps(&mut k, bytes) {
        Ok(r) => r,
        Err(e) => return stopped(Vec::new(), "read", e),
    };
    let mut out = Vec::new();
    for b in &mut read.breps {
        if let Err(e) = measure(&mut k, b, accuracy, &mut out) {
            return stopped(out, "measure", e);
        }
    }
    Measured::Values {
        records: out,
        error: None,
    }
}

/// The round trip of one file: every B-rep flattened (`SmBrepData::FromBrep`), written as
/// ASCII (`WriteToFile`, which makes a lazy surface's NURB as upstream's
/// `GetOrCreateGwNurbPointer` does), read back into a new B-rep and measured.
fn measure_rust_roundtrip(path: &Path, accuracy: f64) -> Measured {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Measured::Broken(format!("read {}: {e}", path.display())),
    };
    let mut k = Kernel::default();
    let read = match read_breps(&mut k, bytes) {
        Ok(r) => r,
        Err(e) => return stopped(Vec::new(), "read", e),
    };
    let mut out = Vec::new();
    for (i, b) in read.breps.iter().enumerate() {
        let key = |e| key_of(i, e);
        let env = FromBrepEnv {
            attributes: &read.attributes,
            key_of: &key,
            records: &read.records,
            curve_key: &|_: EdgeId| ObjectKey(0),
            uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
            surface_key: &|_: FaceId| ObjectKey(0),
        };
        let mut all = Vec::new();
        let written = BrepData::from_brep(b, &env, &mut all)
            .and_then(|mut data| data.write_to_file_with_nurbs(&k, FileType::Ascii));
        let bytes = match written {
            Ok(bytes) => bytes,
            Err(e) => return stopped(out, "write", e),
        };
        let mut back = match read_breps(&mut k, bytes) {
            Ok(r) => r,
            Err(e) => return stopped(out, "read back", e),
        };
        let Some(b2) = back.breps.first_mut() else {
            return Measured::Broken("read back: no B-rep".to_string());
        };
        if let Err(e) = measure(&mut k, b2, accuracy, &mut out) {
            return stopped(out, "measure read back", e);
        }
    }
    Measured::Values {
        records: out,
        error: None,
    }
}

// ---- the child processes --------------------------------------------------------------

/// `goldens --corpus-parity-child <rust|rust-roundtrip> <path> <accuracy>`: measures one
/// file (or its ASCII round trip) and prints one line, tab separated: `CP <values..>`
/// (a complete measurement), `CP-ERROR <why> <values..>` (stopped by the kernel's error
/// after the values), `CP-UNPORTED <upstream> <unit>` or `CP-BROKEN <why>`.
pub fn run_child(args: &[String]) -> ExitCode {
    let (Some(side), Some(path), Some(accuracy)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("usage: goldens {CHILD_FLAG} <rust|rust-roundtrip> <path> <accuracy>");
        return ExitCode::from(2);
    };
    let Ok(accuracy) = accuracy.parse::<f64>() else {
        eprintln!("{CHILD_FLAG}: bad accuracy {accuracy:?}");
        return ExitCode::from(2);
    };
    let m = match side.as_str() {
        "rust" => measure_rust(Path::new(path), accuracy),
        "rust-roundtrip" => measure_rust_roundtrip(Path::new(path), accuracy),
        _ => {
            eprintln!("{CHILD_FLAG}: side must be rust or rust-roundtrip");
            return ExitCode::from(2);
        }
    };
    let flat = |v: &[f64]| -> String {
        v.iter().fold(String::new(), |mut s, x| {
            let _ = write!(s, "\t{x:e}");
            s
        })
    };
    let one_line = |s: &str| s.replace(['\n', '\t'], " ");
    match m {
        Measured::Values {
            records,
            error: None,
        } => println!("CP{}", flat(&records)),
        Measured::Values {
            records,
            error: Some(why),
        } => println!("CP-ERROR\t{}{}", one_line(&why), flat(&records)),
        Measured::Unported { upstream, unit } => println!("CP-UNPORTED\t{upstream}\t{unit}"),
        Measured::Broken(why) => println!("CP-BROKEN\t{}", one_line(&why)),
    }
    ExitCode::SUCCESS
}

/// Runs one side on `path` in a child process, with [`TIMEOUT`].
fn measure_isolated(side: &str, path: &str) -> Measured {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return Measured::Broken(format!("current_exe: {e}")),
    };
    let child = Command::new(exe)
        .arg(CHILD_FLAG)
        .arg(side)
        .arg(path)
        .arg(ACCURACY.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Measured::Broken(format!("spawn: {e}")),
    };
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = out.read_to_string(&mut s);
            s
        })
    });
    // stderr is drained concurrently (a full pipe would stall the child) and only its tail
    // is kept, for the reason of a side that crashed, timed out or printed no result
    let err_reader = child
        .stderr
        .take()
        .map(|err| std::thread::spawn(move || stderr_tail(err)));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Measured::Broken(format!("wait: {e}")),
        }
    };
    let stdout = reader.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err_reader.and_then(|h| h.join().ok()).unwrap_or_default();
    let Some(status) = status else {
        return Measured::Broken(with_stderr(
            format!("timeout after {}s", TIMEOUT.as_secs()),
            &stderr,
        ));
    };
    if !status.success() {
        return Measured::Broken(with_stderr(format!("crash ({status})"), &stderr));
    }
    match parse_child(&stdout) {
        Measured::Broken(why) if why == NO_RESULT => Measured::Broken(with_stderr(why, &stderr)),
        m => m,
    }
}

/// Most trailing lines of a failed child's stderr kept in its reason.
const STDERR_TAIL_LINES: usize = 40;
/// Most trailing bytes of a child's stderr held while it runs.
const STDERR_TAIL_BYTES: usize = 64 * 1024;
/// The reason of a child that exited without printing a result.
const NO_RESULT: &str = "no result from the child";

/// Reads `err` to its end, keeping its last [`STDERR_TAIL_BYTES`] bytes.
fn stderr_tail(mut err: impl std::io::Read) -> String {
    let mut tail: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match err.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                tail.extend_from_slice(buf.get(..n).unwrap_or_default());
                if tail.len() > 2 * STDERR_TAIL_BYTES {
                    tail.drain(..tail.len() - STDERR_TAIL_BYTES);
                }
            }
        }
    }
    if tail.len() > STDERR_TAIL_BYTES {
        tail.drain(..tail.len() - STDERR_TAIL_BYTES);
    }
    String::from_utf8_lossy(&tail).into_owned()
}

/// `why` followed by the last [`STDERR_TAIL_LINES`] non-empty lines of the child's
/// `stderr` (a panic message, an allocation failure, a thread or stack limit), if any.
fn with_stderr(why: String, stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return why;
    }
    let tail = lines
        .get(lines.len().saturating_sub(STDERR_TAIL_LINES)..)
        .unwrap_or_default();
    format!(
        "{why}; child stderr (last {} lines):\n{}",
        tail.len(),
        tail.join("\n")
    )
}

/// The measurement a child printed.
fn parse_child(stdout: &str) -> Measured {
    let numbers = |fields: std::str::Split<'_, char>| -> Result<Vec<f64>, String> {
        fields
            .map(str::parse::<f64>)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("bad child output: {e}"))
    };
    for line in stdout.lines() {
        let mut fields = line.split('\t');
        match fields.next() {
            Some("CP") => {
                return match numbers(fields) {
                    Ok(records) => Measured::Values {
                        records,
                        error: None,
                    },
                    Err(why) => Measured::Broken(why),
                };
            }
            Some("CP-ERROR") => {
                let why = fields.next().unwrap_or_default().to_string();
                return match numbers(fields) {
                    Ok(records) => Measured::Values {
                        records,
                        error: Some(why),
                    },
                    Err(why) => Measured::Broken(why),
                };
            }
            Some("CP-UNPORTED") => {
                return Measured::Unported {
                    upstream: fields.next().unwrap_or_default().to_string(),
                    unit: fields.next().unwrap_or_default().to_string(),
                };
            }
            Some("CP-BROKEN") => {
                return Measured::Broken(fields.next().unwrap_or_default().to_string());
            }
            _ => {}
        }
    }
    Measured::Broken(NO_RESULT.to_string())
}

// ---- the cases ------------------------------------------------------------------------

/// The `.smb` and `.smp` files below `dir` (relative paths, sorted).
fn model_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .map(str::to_ascii_lowercase);
            if matches!(ext.as_deref(), Some("smb" | "smp")) {
                if let Ok(rel) = p.strip_prefix(dir) {
                    out.push((p.clone(), rel.to_string_lossy().replace('\\', "/")));
                }
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Adds one side's records to `out`, each quantity under `prefix` (one entry per B-rep).
fn put_records(mut out: Output, prefix: &str, records: &[f64]) -> Result<Output, HarnessError> {
    if !records.len().is_multiple_of(PER_BREP) {
        return Err(HarnessError::new(format!(
            "{} values, not a whole number of {PER_BREP}-value B-rep records",
            records.len()
        )));
    }
    let column = |j: usize| -> Vec<f64> {
        records
            .chunks(PER_BREP)
            .filter_map(|r| r.get(j).copied())
            .collect()
    };
    for (j, name) in COUNTS.iter().enumerate() {
        let ints: Vec<i64> = column(j).iter().map(|&x| x as i64).collect();
        out = out.ints(&format!("{prefix}{name}"), ints);
    }
    Ok(out
        .values(&format!("{prefix}area"), column(COUNTS.len()))
        .values(&format!("{prefix}volume"), column(COUNTS.len() + 1)))
}

/// One side, run in a child, added to `out` under `prefix`; `Ok(false)` when the side
/// stopped (an error recorded, or a deferral), so no later side runs.
fn run_side(
    out: &mut Output,
    side: &str,
    prefix: &str,
    stage: &str,
    path: &str,
) -> Result<bool, HarnessError> {
    match measure_isolated(side, path) {
        Measured::Values { records, error } => {
            *out = put_records(std::mem::take(out), prefix, &records)?;
            let more = error.is_none();
            if let Some(why) = error {
                out.error = Some(format!("{stage}{why}"));
            }
            Ok(more)
        }
        Measured::Unported { upstream, unit } => {
            *out = Output::deferred(unit, upstream);
            Ok(false)
        }
        Measured::Broken(why) => Err(HarnessError::new(format!("{side}: {why}"))),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "corpus.parity"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let dir = ctx.root.join(crate::fixtures::UPSTREAM_TEST_FILES);
        let mut files = model_files(&dir);
        if files.is_empty() {
            return Err(HarnessError::new(format!(
                "no .smb/.smp files under {}",
                dir.display()
            )));
        }
        for repo_dir in REPO_MODEL_DIRS {
            files.extend(
                model_files(&ctx.root.join(repo_dir))
                    .into_iter()
                    .map(|(path, file)| (path, format!("{repo_dir}/{file}"))),
            );
        }
        files
            .into_iter()
            .map(|(path, file)| {
                let name = file.replace('/', "__");
                let inputs = Inputs {
                    file,
                    path: path.to_string_lossy().into_owned(),
                };
                Ok(Case {
                    unit: self.id().to_string(),
                    name,
                    source: CaseSource::Inline,
                    inputs: serde_json::to_value(inputs)
                        .map_err(|e| HarnessError::new(e.to_string()))?,
                })
            })
            .collect()
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut out = Output::new();
        if run_side(&mut out, "rust", "", "", &i.path)? {
            run_side(&mut out, "rust-roundtrip", "rt_", "round trip: ", &i.path)?;
        }
        Ok(out)
    }

    fn generate(&self, _rng: &mut crate::Rng) -> Option<Value> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_reason_keeps_the_stderr_tail() {
        assert_eq!(
            with_stderr("crash (exit status: 3)".into(), "\n \n"),
            "crash (exit status: 3)"
        );
        let err: String = (0..50).fold(String::new(), |mut s, i| {
            s.push_str("line ");
            s.push_str(&i.to_string());
            s.push('\n');
            s
        });
        let why = with_stderr("crash (exit status: 3)".into(), &err);
        assert!(why.starts_with("crash (exit status: 3); child stderr (last 40 lines):\nline 10\n"));
        assert!(why.ends_with("line 49"));
    }

    #[test]
    fn stderr_tail_keeps_the_last_bytes() {
        let mut input = vec![b'x'; 3 * STDERR_TAIL_BYTES];
        input.extend_from_slice(b"\npanicked at end");
        let tail = stderr_tail(&input[..]);
        assert_eq!(tail.len(), STDERR_TAIL_BYTES);
        assert!(tail.ends_with("panicked at end"));
    }
}
