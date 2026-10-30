//! Runner for unit `corpus.parity`: the Rust port is at least as good as upstream on
//! every real model file under `upstream/TestFiles/pt_TestFiles` (188 `.smb` B-rep
//! files and 74 `.smp` part files). C++: `solidcore_ffi::units::corpus_parity`. Rust:
//! the readers (`SmBrep::ReadFromFile`, `SmBrepData::ReadPartFromFile`) and the
//! measures of `crates/solidcore/examples/corpus`.
//!
//! One case per file. Each side reads the file and measures every B-rep: the kind
//! (`IsManifoldSolid`, else `IsSheet`, else other; empty without faces, edges and
//! vertices), the face, edge, vertex and loop counts, the area (`SmApiFaceComputeArea`'s
//! per-face integration without its accuracy clamp) and, for a solid, the volume
//! (`SmApiBrepComputeVolume`), at integration accuracy [`ACCURACY`]. Both sides run in
//! child processes (`difftest --corpus-parity-child <cpp|rust> <path> <accuracy>`), so a
//! crash or hang on one file fails only that file's side (timeout [`TIMEOUT`]).
//!
//! The file's class, decided on the Rust side (`run_cpp` only records the expected
//! verdict):
//! * `pass`: both succeed, with equal B-rep counts, kinds and counts, and areas and
//!   volumes within `10 * ACCURACY` relative (the integration accuracy, not the 1e-9
//!   of the ported units: the two integrations are separate); also when both fail.
//! * `rust-better`: upstream fails (error status, crash or timeout) and the port
//!   succeeds. Not a failure: recorded as the diagnostic `rust_better` and printed as
//!   `corpus.parity: rust-better <file>`. So is a disagreement on a file listed in
//!   [`KNOWN_CPP_DEFECTS`] (upstream's value there is a recorded C++ defect) when every
//!   quantity outside the defect agrees; it also gets the diagnostic `cpp_defect`.
//! * `REGRESSION`: upstream succeeds and the port fails or disagrees. The only
//!   failing class: the Rust output is the error `REGRESSION: <why>`.
//! * deferred: upstream succeeds and the port stops at an `Unported` callee (the unit
//!   is `partial` until the callee is ported).
//!
//! A file whose read passes is then round-tripped on both sides: every B-rep written as
//! an ASCII `.smb` (`SmBrep::WriteToFile`: `SmBrepData::FromBrep`, `WriteToFile`), read
//! back into a new B-rep and measured again, and the read-back measures are compared
//! the same way (children `cpp-roundtrip` / `rust-roundtrip`). Upstream's writer prints
//! fixed-format doubles, so a read-back volume can drift from the read one by more than
//! the integration accuracy; the two sides write the same bytes and must drift alike.

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
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::{self as fl, PPF_AREA};
use solidcore::topology::part_file::{read_part_from_file, PartEnv};
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::corpus_parity as ffi;

use crate::case::{CaseContext, CaseSource};
use crate::{Case, HarnessError, Output, UnitDiff};

/// Integration accuracy of the areas and volumes (relative).
pub const ACCURACY: f64 = 1e-6;
/// Longest a side may take on one file.
pub const TIMEOUT: Duration = Duration::from_secs(300);
/// The child-process flag of `difftest`.
pub const CHILD_FLAG: &str = "--corpus-parity-child";
/// Deepest directory level searched below the test files.
const MAX_DEPTH: usize = 32;
/// Values per B-rep in a measurement (as [`ffi::PER_BREP`]).
const PER_BREP: usize = ffi::PER_BREP;
/// First identity of the geometry the readers make (topology identities stay below).
const GEOMETRY_KEYS: u64 = 1 << 62;

/// `corpus.parity` runner.
pub struct Runner;

#[derive(Serialize, Deserialize)]
struct Inputs {
    /// The model file, relative to `upstream/TestFiles/pt_TestFiles`.
    file: String,
    /// Its absolute path.
    path: String,
}

// ---- the measurements -----------------------------------------------------------------

/// What one side made of a file.
#[derive(Debug, Clone, PartialEq)]
enum Measured {
    /// `read_status n` then [`PER_BREP`] values per B-rep (`kind faces edges vertices
    /// loops status area volume`; volume NaN unless a solid).
    Values(Vec<f64>),
    /// The port stopped at an unported callee.
    Unported { upstream: String, unit: String },
    /// The side failed: an error, a crash or a timeout (the reason).
    Failed(String),
}

impl Measured {
    /// The B-reps' records when the side read the file and measured every B-rep.
    fn breps(&self) -> Option<Vec<&[f64]>> {
        let Self::Values(v) = self else { return None };
        let (&status, rest) = v.split_first()?;
        let (&n, rest) = rest.split_first()?;
        if status != 0.0 || rest.len() != n as usize * PER_BREP {
            return None;
        }
        let breps: Vec<&[f64]> = rest.chunks(PER_BREP).collect();
        breps
            .iter()
            .all(|b| b.get(5) == Some(&0.0))
            .then_some(breps)
    }

    /// Why the side did not succeed (for the report).
    fn why(&self) -> String {
        match self {
            Self::Values(v) => match v.split_first() {
                Some((&s, _)) if s != 0.0 => format!("read status {s}"),
                _ => "a B-rep's measurement failed".to_string(),
            },
            Self::Unported { upstream, unit } => format!("unported {upstream} ({unit})"),
            Self::Failed(why) => why.clone(),
        }
    }
}

/// `|a - b| <= 10 * ACCURACY * max(|a|, |b|)` (both NaN: no volume on either side).
fn close(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    (a - b).abs() <= 10.0 * ACCURACY * a.abs().max(b.abs())
}

/// The first disagreement between two successful measurements, if any.
fn disagreement(cpp: &[&[f64]], rust: &[&[f64]]) -> Option<String> {
    disagreement_except(cpp, rust, &[])
}

/// [`disagreement`] ignoring the quantities named in `skip`.
fn disagreement_except(cpp: &[&[f64]], rust: &[&[f64]], skip: &[&str]) -> Option<String> {
    const NAMES: [&str; 5] = ["kind", "faces", "edges", "vertices", "loops"];
    if cpp.len() != rust.len() {
        return Some(format!("{} B-reps, upstream {}", rust.len(), cpp.len()));
    }
    for (i, (c, r)) in cpp.iter().zip(rust).enumerate() {
        for (j, name) in NAMES.iter().enumerate() {
            if !skip.contains(name) && c.get(j) != r.get(j) {
                return Some(format!(
                    "B-rep {i}: {name} {:?}, upstream {:?}",
                    r.get(j),
                    c.get(j)
                ));
            }
        }
        for (j, name) in [(6, "area"), (7, "volume")] {
            if skip.contains(&name) {
                continue;
            }
            let (a, b) = (c.get(j).copied(), r.get(j).copied());
            match (a, b) {
                (Some(a), Some(b)) if close(a, b) => {}
                _ => return Some(format!("B-rep {i}: {name} {b:?}, upstream {a:?}")),
            }
        }
    }
    None
}

/// Files where upstream's result is a known C++ defect and the port's is right: the file,
/// the quantities the defect affects (empty: the whole result) and the evidence.
const KNOWN_CPP_DEFECTS: [(&str, &[&str], &str); 1] = [
    (
        "Solids/fillet_ts.smb",
        &[],
        "plan/upstream-issues.md ND-257 (UB-254): upstream's B-rep depends on the allocator; \
         without address reuse it is the port's",
    ),
];

/// The evidence when `file`'s disagreement is a known C++ defect: everything outside the
/// defect's quantities agrees.
fn known_cpp_defect(file: &str, cpp: &[&[f64]], rust: &[&[f64]]) -> Option<&'static str> {
    KNOWN_CPP_DEFECTS
        .iter()
        .find(|(f, _, _)| *f == file)
        .and_then(|(_, quantities, why)| {
            (quantities.is_empty() || disagreement_except(cpp, rust, quantities).is_none())
                .then_some(*why)
        })
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

/// One B-rep's record (as the shim's `measure`).
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
        0.0,
        area,
        volume,
    ]);
    Ok(())
}

/// The Rust side on one file.
fn measure_rust(path: &Path, accuracy: f64) -> Measured {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Measured::Failed(format!("read: {e}")),
    };
    let fail = |stage: &str, e: SolidError| match e {
        SolidError::Unported { upstream, unit } => Measured::Unported {
            upstream: upstream.to_string(),
            unit: unit.to_string(),
        },
        e => Measured::Failed(format!("{stage}: {e}")),
    };
    let mut k = Kernel::default();
    let mut read = match read_breps(&mut k, bytes) {
        Ok(r) => r,
        Err(e) => return fail("read", e),
    };
    let mut out = vec![0.0, read.breps.len() as f64];
    for b in &mut read.breps {
        if let Err(e) = measure(&mut k, b, accuracy, &mut out) {
            return fail("measure", e);
        }
    }
    Measured::Values(out)
}

/// The Rust side's round trip of one file, as the shim's `sc_corpus_parity_roundtrip`:
/// every B-rep flattened (`SmBrepData::FromBrep`), written as ASCII (`WriteToFile`, which
/// makes a lazy surface's NURB as upstream's `GetOrCreateGwNurbPointer` does), read back
/// into a new B-rep and measured.
fn measure_rust_roundtrip(path: &Path, accuracy: f64) -> Measured {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Measured::Failed(format!("read: {e}")),
    };
    let fail = |stage: &str, e: SolidError| match e {
        SolidError::Unported { upstream, unit } => Measured::Unported {
            upstream: upstream.to_string(),
            unit: unit.to_string(),
        },
        e => Measured::Failed(format!("{stage}: {e}")),
    };
    let mut k = Kernel::default();
    let read = match read_breps(&mut k, bytes) {
        Ok(r) => r,
        Err(e) => return fail("read", e),
    };
    let mut out = vec![0.0, read.breps.len() as f64];
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
            Err(e) => return fail("write", e),
        };
        let mut back = match read_breps(&mut k, bytes) {
            Ok(r) => r,
            Err(e) => return fail("read back", e),
        };
        let Some(b2) = back.breps.first_mut() else {
            return Measured::Failed("read back: no B-rep".to_string());
        };
        if let Err(e) = measure(&mut k, b2, accuracy, &mut out) {
            return fail("measure read back", e);
        }
    }
    Measured::Values(out)
}

// ---- the child processes --------------------------------------------------------------

/// `difftest --corpus-parity-child <cpp|rust|cpp-roundtrip|rust-roundtrip> <path>
/// <accuracy>`: measures one file (or its ASCII round trip) on one side and prints one line (`CP <values..>`, `CP-UNPORTED <upstream> <unit>` or
/// `CP-ERROR <why>`, tab separated).
pub fn run_child(args: &[String]) -> ExitCode {
    let (Some(side), Some(path), Some(accuracy)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!(
            "usage: difftest {CHILD_FLAG} <cpp|rust|cpp-roundtrip|rust-roundtrip> <path> <accuracy>"
        );
        return ExitCode::from(2);
    };
    let Ok(accuracy) = accuracy.parse::<f64>() else {
        eprintln!("{CHILD_FLAG}: bad accuracy {accuracy:?}");
        return ExitCode::from(2);
    };
    let m = match side.as_str() {
        "cpp" => match ffi::measure(path, accuracy) {
            Ok(v) => Measured::Values(v),
            Err(e) => Measured::Failed(format!("shim: {e}")),
        },
        "rust" => measure_rust(Path::new(path), accuracy),
        "cpp-roundtrip" => match PrivateDir::new() {
            // (the shim writes and reads the file by name: only inside a directory of
            // our own, never at a guessable path another user could have planted)
            Ok(dir) => {
                let tmp = dir.0.join("roundtrip.smb");
                match ffi::roundtrip(path, &tmp.to_string_lossy(), accuracy) {
                    Ok(v) => Measured::Values(v),
                    Err(e) => Measured::Failed(format!("shim: {e}")),
                }
            }
            Err(e) => Measured::Failed(format!("private temp dir: {e}")),
        },
        "rust-roundtrip" => measure_rust_roundtrip(Path::new(path), accuracy),
        _ => {
            eprintln!("{CHILD_FLAG}: side must be cpp, rust, cpp-roundtrip or rust-roundtrip");
            return ExitCode::from(2);
        }
    };
    match m {
        Measured::Values(v) => {
            let s: Vec<String> = v.iter().map(|x| format!("{x:e}")).collect();
            println!("CP\t{}", s.join("\t"));
        }
        Measured::Unported { upstream, unit } => println!("CP-UNPORTED\t{upstream}\t{unit}"),
        Measured::Failed(why) => println!("CP-ERROR\t{}", why.replace(['\n', '\t'], " ")),
    }
    ExitCode::SUCCESS
}

/// A directory only this user can reach (mode 0700 on Unix), made fresh under the
/// system temp directory for the C++ round trip's file and removed with everything in it
/// when dropped (on every return path). `create_dir` fails on any existing entry,
/// a planted symlink included, and the next name is tried.
pub(super) struct PrivateDir(pub(super) PathBuf);

impl PrivateDir {
    pub(super) fn new() -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let mut last = None;
        for attempt in 0..64_u32 {
            let dir = base.join(format!(
                "corpus-parity-{}-{nanos:09}-{attempt}",
                std::process::id()
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs one side on `path` in a child process, with [`TIMEOUT`].
fn measure_isolated(side: &str, path: &str) -> Measured {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return Measured::Failed(format!("current_exe: {e}")),
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
        Err(e) => return Measured::Failed(format!("spawn: {e}")),
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
            Err(e) => return Measured::Failed(format!("wait: {e}")),
        }
    };
    let stdout = reader.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err_reader.and_then(|h| h.join().ok()).unwrap_or_default();
    let Some(status) = status else {
        return Measured::Failed(with_stderr(
            format!("timeout after {}s", TIMEOUT.as_secs()),
            &stderr,
        ));
    };
    if !status.success() {
        return Measured::Failed(with_stderr(format!("crash ({status})"), &stderr));
    }
    match parse_child(&stdout) {
        Measured::Failed(why) if why == NO_RESULT => Measured::Failed(with_stderr(why, &stderr)),
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
    for line in stdout.lines() {
        let mut fields = line.split('\t');
        match fields.next() {
            Some("CP") => {
                let v: Result<Vec<f64>, _> = fields.map(str::parse::<f64>).collect();
                return match v {
                    Ok(v) => Measured::Values(v),
                    Err(e) => Measured::Failed(format!("bad child output: {e}")),
                };
            }
            Some("CP-UNPORTED") => {
                return Measured::Unported {
                    upstream: fields.next().unwrap_or_default().to_string(),
                    unit: fields.next().unwrap_or_default().to_string(),
                };
            }
            Some("CP-ERROR") => {
                return Measured::Failed(fields.next().unwrap_or_default().to_string());
            }
            _ => {}
        }
    }
    Measured::Failed(NO_RESULT.to_string())
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

/// The round trip's verdict on a file whose read passed (`read`, kept when the round trip
/// passes too): each side writes every B-rep as ASCII and reads it back
/// (`cpp-roundtrip` / `rust-roundtrip` children), and the read-back measures are compared
/// as the read ones are. A disagreement is a `REGRESSION` (upstream's ASCII writer drops
/// digits, so the read-back volume can move from the read one; both sides must move
/// alike), unless it is a known C++ defect of the file.
fn roundtrip_verdict(i: &Inputs, read: Output) -> Output {
    let cpp = measure_isolated("cpp-roundtrip", &i.path);
    let rust = measure_isolated("rust-roundtrip", &i.path);
    match (cpp.breps(), rust.breps()) {
        (Some(c), Some(r)) => match disagreement(&c, &r) {
            None => read.diagnostic("roundtrip_pass", 1),
            Some(why) => match known_cpp_defect(&i.file, &c, &r) {
                Some(_) => read.diagnostic("roundtrip_cpp_defect", 1),
                None => Output::err(format!("REGRESSION: round trip: {why}")),
            },
        },
        (Some(_), None) => match rust {
            Measured::Unported { upstream, unit } => Output::deferred(unit, upstream),
            _ => Output::err(format!("REGRESSION: round trip: {}", rust.why())),
        },
        (None, Some(_)) => {
            eprintln!(
                "corpus.parity: rust-better {} (upstream round trip: {})",
                i.file,
                cpp.why()
            );
            read.diagnostic("roundtrip_rust_better", 1)
        }
        (None, None) => read.diagnostic("roundtrip_both_fail", 1),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "corpus.parity"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let dir = ctx.root.join(crate::fixtures::UPSTREAM_TEST_FILES);
        let files = model_files(&dir);
        if files.is_empty() {
            return Err(HarnessError::new(format!(
                "no .smb/.smp files under {}",
                dir.display()
            )));
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

    /// The verdict expected of every file: no regression (the measurements run on the
    /// Rust side, which classifies the file).
    fn run_cpp(&self, _case: &Case) -> Result<Output, HarnessError> {
        Ok(Output::new().int("regression", 0))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let cpp = measure_isolated("cpp", &i.path);
        let rust = measure_isolated("rust", &i.path);
        let ok = Output::new().int("regression", 0);
        let read = match (cpp.breps(), rust.breps()) {
            (Some(c), Some(r)) => match disagreement(&c, &r) {
                None => ok.diagnostic("pass", 1),
                Some(why) => match known_cpp_defect(&i.file, &c, &r) {
                    Some(defect) => {
                        eprintln!(
                            "corpus.parity: rust-better {} (known C++ defect: {why}; {defect})",
                            i.file
                        );
                        ok.diagnostic("rust_better", 1).diagnostic("cpp_defect", 1)
                    }
                    None => Output::err(format!("REGRESSION: {why}")),
                },
            },
            (Some(_), None) => match rust {
                Measured::Unported { upstream, unit } => Output::deferred(unit, upstream),
                _ => Output::err(format!("REGRESSION: {}", rust.why())),
            },
            (None, Some(_)) => {
                eprintln!(
                    "corpus.parity: rust-better {} (upstream: {})",
                    i.file,
                    cpp.why()
                );
                ok.diagnostic("rust_better", 1)
            }
            (None, None) => ok.diagnostic("both_fail", 1),
        };
        if read.error.is_some() || read.deferred.is_some() {
            return Ok(read);
        }
        Ok(roundtrip_verdict(&i, read))
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
