//! `cargo run --release --example corpus -- [<dir>] [--json <path>]`: reads every real
//! B-rep model under `<dir>` (default: upstream's `TestFiles/pt_TestFiles`, 188 `.smb`
//! files and 74 `.smp` part files) with the ported reader (`SmBrep::ReadFromFile`), checks
//! each one in pure Rust and prints `corpus: X/Y pass`.
//!
//! Per model:
//! 1. read: [`read_from_file`] on the file's bytes (ASCII); a part file (`.smp`, or an
//!    `.smb` that starts with `//[Output Summary]`) with [`read_part_from_file`]
//!    (`SmBrepData::ReadPartFromFile`), which gives any number of B-reps.
//! 2. classify each B-rep: `IsManifoldSolid` gives a solid, otherwise `IsSheet` gives a
//!    sheet, otherwise it is `other` (wire, non-manifold or general). The model's kind
//!    is its B-reps' kind when they agree, else `mixed` (`empty` without a B-rep).
//! 3. measure each B-rep: face, edge, vertex and loop counts; the face components (faces joined
//!    through shared edges); the tight bounding box; the area, integrated per face as the
//!    `primitives` example's checks do (`SmApiFaceComputeArea` without its accuracy clamp);
//!    and, for a solid, `SmApiBrepComputeVolume`.
//! 4. check: a model has a B-rep, and every B-rep has a face and a positive finite area.
//!    A solid also needs an Euler-Poincare characteristic `V - E + 2F - L = 2 (S - G)`
//!    that is even and gives a genus `G >= 0` (`S` the face components) and a volume in
//!    `(0, bbox volume]`. A check of B-rep `i` of a part names it in its message.
//!
//! 5. round trip (does not change the class): each measured B-rep is flattened
//!    (`SmBrepData::FromBrep`), written as an ASCII `.smb` (`WriteToFile`, after making a lazy surface's NURB as upstream's `GetOrCreateGwNurbPointer` does), read back
//!    and measured again; it fails when the kind or counts differ. An area or volume
//!    that moves by more than ten times the integration accuracy (relative) is
//!    reported as `drift (upstream writer): ...` with the values, not as a failure:
//!    upstream's writer prints doubles in fixed format (`%.16f`) and its own round
//!    trip moves them alike (bit for bit on the corpus; `plan/findings/corpus.parity--ascii-round-trip-volume-drift-is-upstreams-own-2026-10-23.md`, and the
//!    `corpus.parity` difftest compares the C++ and Rust round trips). The model's
//!    `roundtrip` is `pass`, that drift, or the first B-rep's failure
//!    (`unported(<symbol>)` for a writer not ported yet).
//!
//! Each model ends in one class: `pass`, `read-fail` (the reader returned an error),
//! `check-fail` (a measurement errored or a check failed), `unported` (an
//! `Unported { upstream, unit }` stopped it; the symbol is reported) or `crash` (the
//! model's child process panicked, aborted, overflowed its stack or ran past
//! `--timeout`).
//!
//! Every model runs in its own child process (this executable with `--one`), so a crash
//! or hang on one file cannot stop the run; `--no-isolate` runs them in-process.
//!
//! Options: `--json <path>` writes the results (schema below); `--accuracy <a>`
//! (default `1e-6`) is the integration accuracy; `--timeout <s>` (default 300) bounds
//! each model; `--jobs <n>` (default 1) runs that many models at once; `--filter <s>`
//! keeps the files whose relative path contains `s`. The exit status is non-zero only
//! for bad arguments or I/O errors, not for failing models. Schema 1:
//!
//! ```text
//! { "schema": 1, "source": "crates/solidcore/examples/corpus", "dir", "accuracy",
//!   "roundtrip": "<what the round trip does>",
//!   "summary": { "total", "pass", "read_fail", "check_fail", "unported", "crash",
//!                "roundtrip": { "tried", "pass", "drift" },
//!                "kinds": { "<kind>": { "total", "pass" } } },
//!   "blockers": [ { "upstream", "unit", "count", "files": [..] } ],   (most files first)
//!   "models": [ { "file", "class", "kind", "stage": null | "read" | "measure" | "check"
//!                 | "run", "blocker": null | { "upstream", "unit" }, "error",
//!                 "breps": [ { "kind", "counts": { "faces", "edges", "vertices",
//!                              "loops", "components" }, "genus", "volume", "area",
//!                              "bbox": { "min", "max" } } ],
//!                 "checks": [ { "name", "pass", "message" } ],
//!                 "roundtrip": null | "pass" | "drift (upstream writer): ..." | "<failure>",
//!                 "seconds" } ] }
//! ```

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::{is_manifold_solid, is_sheet};
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge as ed;
use solidcore::topology::face::p2::{self as fl, PPF_AREA};
use solidcore::topology::part_file::{read_part_from_file, PartEnv};
use solidcore::topology::topology::Topology;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, SolidError};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Upstream's test models, relative to this crate.
const DEFAULT_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../upstream/TestFiles/pt_TestFiles"
);
/// Largest model file read (bytes).
const MAX_FILE_BYTES: u64 = 256 << 20;
/// Deepest directory level searched below `<dir>`.
const MAX_DEPTH: usize = 32;
/// Most model files run.
const MAX_FILES: usize = 100_000;
/// Most bytes of a crashed child's stderr kept for the report.
const STDERR_TAIL: usize = 800;
/// What the round trip does.
const ROUNDTRIP: &str = "FromBrep, WriteToFile (ASCII), ReadFromFile: equal kind and counts; \
                         area or volume beyond 10 x accuracy is upstream-writer drift";

/// The prefix of a round trip whose area or volume moved (upstream's writer moves it
/// alike).
const DRIFT: &str = "drift (upstream writer)";

/// Command-line options.
struct Opts {
    dir: PathBuf,
    json: Option<PathBuf>,
    accuracy: f64,
    timeout: Duration,
    jobs: usize,
    isolate: bool,
    filter: Option<String>,
    /// Child mode: run this one model and print its result.
    one: Option<PathBuf>,
    /// Child mode: the model's path relative to `dir`.
    rel: Option<String>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        dir: PathBuf::from(DEFAULT_DIR),
        json: None,
        accuracy: 1e-6,
        timeout: Duration::from_secs(300),
        jobs: 1,
        isolate: true,
        filter: None,
        one: None,
        rel: None,
    };
    let usage = "usage: [<dir>] [--json <path>] [--accuracy <a>] [--timeout <secs>] \
                 [--jobs <n>] [--filter <substring>] [--no-isolate]";
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            let v = args.next();
            v.ok_or_else(|| format!("{name} needs a value; {usage}"))
        };
        match a.as_str() {
            "--json" => o.json = Some(PathBuf::from(value("--json")?)),
            "--accuracy" => {
                let v = value("--accuracy")?;
                o.accuracy = v.parse().map_err(|e| format!("--accuracy {v:?}: {e}"))?;
                if !(o.accuracy.is_finite() && o.accuracy > 0.0) {
                    return Err(format!("--accuracy {v:?}: must be positive"));
                }
            }
            "--timeout" => {
                let v = value("--timeout")?;
                let s: u64 = v.parse().map_err(|e| format!("--timeout {v:?}: {e}"))?;
                o.timeout = Duration::from_secs(s.max(1));
            }
            "--jobs" => {
                let v = value("--jobs")?;
                let n: usize = v.parse().map_err(|e| format!("--jobs {v:?}: {e}"))?;
                o.jobs = n.clamp(1, 64);
            }
            "--filter" => o.filter = Some(value("--filter")?),
            "--no-isolate" => o.isolate = false,
            "--one" => o.one = Some(PathBuf::from(value("--one")?)),
            "--rel" => o.rel = Some(value("--rel")?),
            s if s.starts_with("--") => return Err(format!("unknown option {s:?}; {usage}")),
            _ => o.dir = PathBuf::from(&a),
        }
    }
    Ok(o)
}

fn main() -> Result<(), String> {
    let opts = parse_args()?;
    if let Some(file) = &opts.one {
        let rel = match &opts.rel {
            Some(r) => r.clone(),
            None => file.display().to_string(),
        };
        let e = Entry::of(&evaluate(file, &rel, opts.accuracy));
        let upstream = e.blocker.as_ref().map_or("", |b| b.0.as_str());
        let unit = e.blocker.as_ref().map_or("", |b| b.1.as_str());
        let rt = match e.roundtrip {
            None => "-",
            Some(true) if e.roundtrip_drift => "drift",
            Some(true) => "pass",
            Some(false) => "fail",
        };
        let (class, kind) = (&e.class, &e.kind);
        println!("CORPUS-CLASS\t{class}\t{kind}\t{upstream}\t{unit}\t{rt}");
        println!("CORPUS-LINE\t{}", e.line.replace(['\n', '\r'], " "));
        println!("CORPUS-JSON\t{}", e.json);
        return Ok(());
    }

    let files = model_files(&opts.dir, opts.filter.as_deref());
    if files.is_empty() {
        return Err(format!("no .smb/.smp files under {}", opts.dir.display()));
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    println!(
        "corpus: {} model files under {}",
        files.len(),
        opts.dir.display()
    );

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Entry>>> = Mutex::new(files.iter().map(|_| None).collect());
    thread::scope(|s| {
        for _ in 0..opts.jobs.min(files.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((path, rel)) = files.get(i) else {
                    break;
                };
                let entry = if opts.isolate {
                    run_isolated(&exe, path, rel, &opts)
                } else {
                    Entry::of(&evaluate(path, rel, opts.accuracy))
                };
                println!("{}", entry.line);
                let mut r = results.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(slot) = r.get_mut(i) {
                    *slot = Some(entry);
                }
            });
        }
    });
    let entries: Vec<Entry> = results
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .flatten()
        .collect();

    let summary = Summary::of(&entries);
    summary.print();
    if let Some(path) = &opts.json {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let text = report_json(&opts, &summary, &entries);
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("JSON: {}", path.display());
    }
    Ok(())
}

// ---- files ----------------------------------------------------------------------------------

/// Every `.smb`/`.smp` file under `dir` (bounded depth and count), sorted by relative path,
/// as `(path, relative path)`.
fn model_files(dir: &Path, filter: Option<&str>) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or_default();
            if !(ext.eq_ignore_ascii_case("smb") || ext.eq_ignore_ascii_case("smp")) {
                continue;
            }
            let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy();
            let rel = rel.replace('\\', "/");
            if filter.is_some_and(|f| !rel.contains(f)) {
                continue;
            }
            out.push((p, rel));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out.truncate(MAX_FILES);
    out
}

// ---- one model ------------------------------------------------------------------------------

/// How a model ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Class {
    Pass,
    ReadFail,
    CheckFail,
    Unported { upstream: String, unit: String },
}

impl Class {
    fn label(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::ReadFail => "read-fail",
            Self::CheckFail => "check-fail",
            Self::Unported { .. } => "unported",
        }
    }
}

/// One named check of a model.
struct Check {
    name: &'static str,
    pass: bool,
    message: String,
}

/// Faces, edges, vertices, loops and face components.
#[derive(Clone, Copy)]
struct Counts {
    faces: usize,
    edges: usize,
    vertices: usize,
    loops: usize,
    components: usize,
}

/// What was measured of one B-rep.
struct Body {
    kind: &'static str,
    counts: Counts,
    genus: Option<i64>,
    volume: Option<f64>,
    area: f64,
    bbox: (Point3, Point3),
}

/// Everything found out about one model.
struct Outcome {
    file: String,
    class: Class,
    kind: &'static str,
    stage: Option<&'static str>,
    error: Option<String>,
    bodies: Vec<Body>,
    checks: Vec<Check>,
    /// `None` when not tried; `pass` or the first failure.
    roundtrip: Option<String>,
    seconds: f64,
}

impl Outcome {
    /// Stops the model at `stage` on `e`: unported, or a read or check failure.
    fn fail(&mut self, stage: &'static str, e: &SolidError) {
        self.stage = Some(stage);
        self.error = Some(format!("{e:?}"));
        self.class = match e {
            SolidError::Unported { upstream, unit } => Class::Unported {
                upstream: (*upstream).to_string(),
                unit: (*unit).to_string(),
            },
            _ if stage == "read" => Class::ReadFail,
            _ => Class::CheckFail,
        };
    }
}

fn evaluate(path: &Path, rel: &str, accuracy: f64) -> Outcome {
    let start = Instant::now();
    let mut o = Outcome {
        file: rel.to_string(),
        class: Class::Pass,
        kind: "unknown",
        stage: None,
        error: None,
        bodies: Vec::new(),
        checks: Vec::new(),
        roundtrip: None,
        seconds: 0.0,
    };
    run(path, accuracy, &mut o);
    o.seconds = start.elapsed().as_secs_f64();
    o
}

/// Reads, measures and checks one model into `o`.
fn run(path: &Path, accuracy: f64, o: &mut Outcome) {
    let bytes = match read_bytes(path) {
        Ok(b) => b,
        Err(msg) => {
            o.stage = Some("read");
            o.error = Some(msg);
            o.class = Class::ReadFail;
            return;
        }
    };
    let mut k = Kernel::default();
    let mut read = match read_breps(&mut k, bytes) {
        Ok(r) => r,
        Err(e) => return o.fail("read", &e),
    };
    let n_breps = read.breps.len();

    for (i, b) in read.breps.iter_mut().enumerate() {
        match measure(&mut k, b, accuracy) {
            Ok(body) => o.bodies.push(body),
            Err(e) => return o.fail("measure", &e),
        }
        if let Some(body) = o.bodies.last() {
            let name = if n_breps > 1 {
                format!("B-rep {i}: ")
            } else {
                String::new()
            };
            check(body, &name, accuracy, &mut o.checks);
        }
    }
    let mut rt = String::from("pass");
    let mut drift = Vec::new();
    for (i, body) in o.bodies.iter().enumerate() {
        let name = if n_breps > 1 {
            format!("B-rep {i}: ")
        } else {
            String::new()
        };
        match round_trip(&mut k, &read, i, body, accuracy) {
            Ok(None) => {}
            Ok(Some(moved)) => drift.push(format!("{name}{moved}")),
            Err(why) => {
                rt = format!("{name}{why}");
                break;
            }
        }
    }
    if rt == "pass" && !drift.is_empty() {
        rt = format!("{DRIFT}: {}", drift.join("; "));
    }
    o.roundtrip = Some(rt);
    o.kind = match o.bodies.split_first() {
        None => "empty",
        Some((first, rest)) if rest.iter().all(|b| b.kind == first.kind) => first.kind,
        Some(_) => "mixed",
    };
    o.checks.push(Check {
        name: "breps",
        pass: n_breps != 0,
        message: if n_breps == 0 {
            "no B-rep in the file".to_string()
        } else {
            String::new()
        },
    });
    if o.checks.iter().any(|c| !c.pass) {
        o.stage = Some("check");
        o.class = Class::CheckFail;
    }
}

/// The B-reps of a model file with their objects' records and attributes.
struct Read {
    breps: Vec<Brep>,
    attributes: AttributeStore,
    records: BTreeMap<ObjectKey, Topology>,
}

/// The B-reps of a model file: one from a B-rep file (`SmBrep::ReadFromFile`), any
/// number from a part file (`SmBrepData::ReadPartFromFile`).
fn read_breps(k: &mut Kernel, bytes: Vec<u8>) -> solidcore::Result<Read> {
    let mut records = BTreeMap::new();
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
    // a B-rep as `new SmBrep(ctx)` makes it, read as `SmBrep::ReadFromFile` does
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

/// Writes B-rep `i` of `read` to an ASCII file and reads it back, then compares its
/// measures with `body`. `Err` is the failure (kind or counts); `Ok(Some(..))` the
/// area or volume drift beyond ten times `accuracy`, which upstream's writer makes too.
fn round_trip(
    k: &mut Kernel,
    read: &Read,
    i: usize,
    body: &Body,
    accuracy: f64,
) -> Result<Option<String>, String> {
    let describe = |e: SolidError| match e {
        SolidError::Unported { upstream, .. } => format!("unported({upstream})"),
        e => format!("{e:?}"),
    };
    let b = read.breps.get(i).ok_or("no such B-rep")?;
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
    let mut data = BrepData::from_brep(b, &env, &mut all).map_err(describe)?;
    let bytes = data
        .write_to_file_with_nurbs(k, FileType::Ascii)
        .map_err(describe)?;
    let mut back = read_breps(k, bytes).map_err(|e| format!("read back: {}", describe(e)))?;
    let b2 = back.breps.first_mut().ok_or("read back no B-rep")?;
    let again = measure(k, b2, accuracy).map_err(|e| format!("measure: {}", describe(e)))?;

    let (c, d) = (body.counts, again.counts);
    let counts = (c.faces, c.edges, c.vertices, c.loops);
    let counts2 = (d.faces, d.edges, d.vertices, d.loops);
    if body.kind != again.kind || counts != counts2 {
        return Err(format!(
            "{} F E V L {counts:?} read back as {} {counts2:?}",
            body.kind, again.kind
        ));
    }
    let tol = 10.0 * accuracy;
    let close = |a: f64, b: f64| (a - b).abs() <= tol * a.abs().max(b.abs());
    let mut moved = Vec::new();
    if !close(body.area, again.area) {
        moved.push(format!("area {} read back as {}", body.area, again.area));
    }
    if let (Some(v), Some(w)) = (body.volume, again.volume) {
        if !close(v, w) {
            moved.push(format!("volume {v} read back as {w}"));
        }
    }
    Ok((!moved.is_empty()).then(|| moved.join(", ")))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("metadata: {e}"))?
        .len();
    if len > MAX_FILE_BYTES {
        return Err(format!(
            "file of {len} bytes is over the {MAX_FILE_BYTES}-byte limit"
        ));
    }
    std::fs::read(path).map_err(|e| format!("read: {e}"))
}

/// First identity of the geometry the readers make (topology identities stay below).
const GEOMETRY_KEYS: u64 = 1 << 62;

/// A distinct identity for every topology object of B-rep `brep` the readers build,
/// as the `topology.brep_p7` difftest runner gives them, with the B-rep index above.
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

/// Measures one B-rep: counts, kind, bounding box, area and (solids) volume.
fn measure(k: &mut Kernel, b: &mut Brep, accuracy: f64) -> solidcore::Result<Body> {
    let faces = bl::faces(b, None)?;
    let edges = bl::edges(b, None)?;
    let vertices = bl::vertices(b, None)?;
    let mut loops = 0;
    for f in &faces {
        loops += fl::loops(b, *f, false, None)?.len();
    }
    let counts = Counts {
        faces: faces.len(),
        edges: edges.len(),
        vertices: vertices.len(),
        loops,
        components: components(b, &faces)?,
    };

    let kind = if is_manifold_solid(b)? {
        "solid"
    } else if is_sheet(k, b)? {
        "sheet"
    } else {
        "other"
    };

    let opts = BoundingBoxOptions {
        approx: false,
        tight: true,
        expand_by_zone_tol_3d: false,
    };
    let ext = calculate_bounding_box(k, &*b, opts)?;
    let bbox = (
        Point3::new(ext.min_at(0), ext.min_at(1), ext.min_at(2)),
        Point3::new(ext.max_at(0), ext.max_at(1), ext.max_at(2)),
    );

    let area = integrated_area(k, b, &faces, accuracy)?;

    let mut volume = None;
    if kind == "solid" {
        let store = AttributeStore::new();
        let key = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key,
        };
        volume = Some(brep_volume(k, b, accuracy, &attributes)?);
    }

    // V - E + F - (L - F) = 2 (S - G)
    let c = counts;
    let chi = c.vertices as i64 - c.edges as i64 + 2 * c.faces as i64 - c.loops as i64;
    let genus = if chi.rem_euclid(2) == 0 {
        Some(c.components as i64 - chi.div_euclid(2))
    } else {
        None
    };
    Ok(Body {
        kind,
        counts,
        genus,
        volume,
        area,
        bbox,
    })
}

/// The surface area at `accuracy`: `SmApiBrepComputeArea`'s per-face integration
/// (`SmApiFaceComputeArea`: face box midpoint as origin, area estimate from the box, area
/// only) without that entry point's `[1e-4, 1e-1]` accuracy clamp, as the `primitives`
/// example's checks integrate it.
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

/// The number of connected components of `faces`, two faces being connected when they
/// share an edge.
fn components(b: &Brep, faces: &[FaceId]) -> solidcore::Result<usize> {
    let index: BTreeMap<FaceId, usize> = faces.iter().enumerate().map(|(i, f)| (*f, i)).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    for e in bl::edges(b, None)? {
        let on = ed::faces(b, e, None)?;
        let mut ids = on.iter().filter_map(|f| index.get(f).copied());
        if let Some(first) = ids.next() {
            for other in ids {
                let (x, y) = (root(&mut parent, first), root(&mut parent, other));
                if let Some(slot) = parent.get_mut(x.max(y)) {
                    *slot = x.min(y);
                }
            }
        }
    }
    Ok((0..faces.len())
        .filter(|&i| root(&mut parent, i) == i)
        .count())
}

/// The union-find root of `i` (path halving).
fn root(parent: &mut [usize], mut i: usize) -> usize {
    loop {
        let p = parent.get(i).copied().unwrap_or(i);
        if p == i {
            return i;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = gp;
    }
}

/// Runs the checks of one measured B-rep (`name` prefixes the messages).
fn check(body: &Body, name: &str, accuracy: f64, checks: &mut Vec<Check>) {
    let c = body.counts;
    checks.push(Check {
        name: "faces",
        pass: c.faces > 0,
        message: if c.faces > 0 {
            String::new()
        } else {
            format!("{name}no faces")
        },
    });
    let area = body.area;
    let area_ok = area.is_finite() && area > 0.0;
    checks.push(Check {
        name: "area",
        pass: area_ok,
        message: if area_ok {
            String::new()
        } else {
            format!("{name}area {area} is not positive and finite")
        },
    });
    if body.kind != "solid" {
        return;
    }

    let euler_ok = body.genus.is_some_and(|g| g >= 0);
    checks.push(Check {
        name: "euler",
        pass: euler_ok,
        message: if euler_ok {
            String::new()
        } else {
            let (v, e, f, l, s) = (c.vertices, c.edges, c.faces, c.loops, c.components);
            format!("{name}V {v} E {e} F {f} L {l} with {s} components: no genus >= 0")
        },
    });

    let volume = body.volume.unwrap_or(f64::NAN);
    let (lo, hi) = body.bbox;
    let box_volume = (hi.x - lo.x) * (hi.y - lo.y) * (hi.z - lo.z);
    // the integration is accurate to about ten times the requested accuracy
    let volume_ok =
        volume.is_finite() && volume > 0.0 && volume <= box_volume * (1.0 + 10.0 * accuracy);
    checks.push(Check {
        name: "volume",
        pass: volume_ok,
        message: if volume_ok {
            String::new()
        } else {
            format!("{name}volume {volume} is not in (0, bounding-box volume {box_volume}]")
        },
    });
}

// ---- isolation ------------------------------------------------------------------------------

/// A finished model: its class, kind, blocker, report line and JSON object.
struct Entry {
    file: String,
    class: String,
    kind: String,
    blocker: Option<(String, String)>,
    /// The round trip: `None` when not tried, else whether it passed.
    roundtrip: Option<bool>,
    /// A passed round trip moved the area or volume (upstream-writer drift).
    roundtrip_drift: bool,
    line: String,
    json: String,
}

impl Entry {
    fn of(o: &Outcome) -> Self {
        let blocker = match &o.class {
            Class::Unported { upstream, unit } => Some((upstream.clone(), unit.clone())),
            _ => None,
        };
        let mut detail = String::new();
        if o.bodies.len() > 1 {
            let _ = write!(detail, " {} B-reps:", o.bodies.len());
        }
        for b in &o.bodies {
            let c = b.counts;
            let (f, e, v, l) = (c.faces, c.edges, c.vertices, c.loops);
            if o.bodies.len() > 1 {
                let _ = write!(detail, " [{}", b.kind);
            }
            let _ = write!(detail, " F {f} E {e} V {v} L {l}");
            if let Some(g) = b.genus {
                let _ = write!(detail, " genus {g}");
            }
            if let Some(v) = b.volume {
                let _ = write!(detail, " volume {v:.9e}");
            }
            let _ = write!(detail, " area {:.9e}", b.area);
            if o.bodies.len() > 1 {
                detail.push(']');
            }
        }
        if let Some((upstream, unit)) = &blocker {
            let _ = write!(detail, " {upstream} ({unit})");
        } else if let Some(e) = &o.error {
            let _ = write!(detail, " {}", short(e, 160));
        }
        for c in o.checks.iter().filter(|c| !c.pass) {
            let _ = write!(detail, " [{}: {}]", c.name, c.message);
        }
        if let Some(rt) = &o.roundtrip {
            let _ = write!(detail, " roundtrip: {}", short(rt, 160));
        }
        let (class, kind, file, secs) = (o.class.label(), o.kind, &o.file, o.seconds);
        Self {
            file: o.file.clone(),
            class: class.to_string(),
            kind: kind.to_string(),
            blocker,
            roundtrip: o
                .roundtrip
                .as_ref()
                .map(|rt| rt == "pass" || rt.starts_with(DRIFT)),
            roundtrip_drift: o.roundtrip.as_ref().is_some_and(|rt| rt.starts_with(DRIFT)),
            line: format!("{class:<10} {kind:<7} {file}{detail} ({secs:.2}s)"),
            json: outcome_json(o),
        }
    }

    /// A model whose child process did not report.
    fn crash(rel: &str, error: &str, seconds: f64) -> Self {
        let line = format!(
            "crash      unknown {rel} {} ({seconds:.2}s)",
            short(error, 300)
        );
        let (file, error, seconds) = (string(rel), string(error), num(seconds));
        let json = format!(
            "{{\"file\": {file}, \"class\": \"crash\", \"kind\": \"unknown\", \
             \"stage\": \"run\", \"blocker\": null, \"error\": {error}, \"breps\": [], \
             \"checks\": [], \"roundtrip\": null, \"seconds\": {seconds}}}"
        );
        Self {
            file: rel.to_string(),
            class: "crash".to_string(),
            kind: "unknown".to_string(),
            blocker: None,
            roundtrip: None,
            roundtrip_drift: false,
            line,
            json,
        }
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n).collect();
        t.push_str("...");
        t
    }
}

/// Runs one model in a child process (`--one`), bounded by `--timeout`.
fn run_isolated(exe: &Path, path: &Path, rel: &str, opts: &Opts) -> Entry {
    let start = Instant::now();
    let child = Command::new(exe)
        .arg("--one")
        .arg(path)
        .arg("--rel")
        .arg(rel)
        .arg("--accuracy")
        .arg(format!("{:e}", opts.accuracy))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Entry::crash(rel, &format!("spawn: {e}"), 0.0),
    };
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if start.elapsed() >= opts.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("timeout after {} s", opts.timeout.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("wait: {e}"));
            }
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = err.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr);
    let seconds = start.elapsed().as_secs_f64();
    let reported = parse_child(&stdout, rel);
    match (status, reported) {
        (Ok(st), Some(entry)) if st.success() => entry,
        (status, _) => {
            let why = match status {
                Ok(st) => format!("child exited with {st}"),
                Err(e) => e,
            };
            let tail: String = {
                let t = stderr.trim();
                let skip = t.chars().count().saturating_sub(STDERR_TAIL);
                t.chars().skip(skip).collect()
            };
            let error = if tail.is_empty() {
                why
            } else {
                format!("{why}: {tail}")
            };
            Entry::crash(rel, &error, seconds)
        }
    }
}

/// Reads all of a child's pipe on its own thread (so a full pipe cannot stall the child).
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut v = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut v);
        }
        v
    })
}

/// The entry a `--one` child printed for the model at `rel`.
fn parse_child(stdout: &str, rel: &str) -> Option<Entry> {
    let mut head = None;
    let mut line = None;
    let mut json = None;
    for l in stdout.lines() {
        if let Some(rest) = l.strip_prefix("CORPUS-CLASS\t") {
            head = Some(rest);
        } else if let Some(rest) = l.strip_prefix("CORPUS-LINE\t") {
            line = Some(rest);
        } else if let Some(rest) = l.strip_prefix("CORPUS-JSON\t") {
            json = Some(rest);
        }
    }
    let mut f = head?.split('\t');
    let class = f.next()?.to_string();
    let kind = f.next()?.to_string();
    let upstream = f.next().unwrap_or_default();
    let unit = f.next().unwrap_or_default();
    let rt = f.next();
    let roundtrip = match rt {
        Some("pass" | "drift") => Some(true),
        Some("fail") => Some(false),
        _ => None,
    };
    let blocker = (!upstream.is_empty()).then(|| (upstream.to_string(), unit.to_string()));
    Some(Entry {
        file: rel.to_string(),
        class,
        kind,
        blocker,
        roundtrip,
        roundtrip_drift: rt == Some("drift"),
        line: line?.to_string(),
        json: json?.to_string(),
    })
}

// ---- summary and JSON -----------------------------------------------------------------------

/// Totals over all models.
struct Summary {
    total: usize,
    by_class: BTreeMap<String, usize>,
    /// kind -> (total, pass)
    kinds: BTreeMap<String, (usize, usize)>,
    /// (upstream, unit) -> files, most files first
    blockers: Vec<((String, String), Vec<String>)>,
    /// Models whose round trip was tried, passed, and passed with upstream-writer drift.
    roundtrip: (usize, usize, usize),
}

impl Summary {
    fn of(entries: &[Entry]) -> Self {
        let mut by_class = BTreeMap::new();
        let mut kinds: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut blockers: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        let mut roundtrip = (0, 0, 0);
        for e in entries {
            *by_class.entry(e.class.clone()).or_insert(0) += 1;
            let k = kinds.entry(e.kind.clone()).or_insert((0, 0));
            k.0 += 1;
            if e.class == "pass" {
                k.1 += 1;
            }
            if let Some(b) = &e.blocker {
                blockers.entry(b.clone()).or_default().push(e.file.clone());
            }
            if let Some(pass) = e.roundtrip {
                roundtrip.0 += 1;
                roundtrip.1 += usize::from(pass);
                roundtrip.2 += usize::from(e.roundtrip_drift);
            }
        }
        let mut blockers: Vec<_> = blockers.into_iter().collect();
        blockers.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        Self {
            total: entries.len(),
            by_class,
            kinds,
            blockers,
            roundtrip,
        }
    }

    fn count(&self, class: &str) -> usize {
        self.by_class.get(class).copied().unwrap_or(0)
    }

    fn print(&self) {
        if !self.blockers.is_empty() {
            println!("blockers (unported symbol, models stopped):");
            for ((upstream, unit), files) in &self.blockers {
                println!("  {:>4}  {upstream} ({unit})", files.len());
            }
        }
        let kinds: Vec<String> = self
            .kinds
            .iter()
            .map(|(k, (t, p))| format!("{k} {p}/{t}"))
            .collect();
        println!(
            "corpus: {}/{} pass ({}; read-fail {}, check-fail {}, unported {}, crash {})",
            self.count("pass"),
            self.total,
            kinds.join(", "),
            self.count("read-fail"),
            self.count("check-fail"),
            self.count("unported"),
            self.count("crash")
        );
        let (tried, pass, drift) = self.roundtrip;
        println!(
            "roundtrip: {pass}/{tried} pass, {drift} with upstream-writer drift \
             (FromBrep, WriteToFile, ReadFromFile)"
        );
    }
}

fn num(x: f64) -> String {
    if x.is_finite() {
        format!("{x:?}")
    } else {
        "null".to_string()
    }
}

fn string(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn opt<T>(v: Option<T>, f: impl Fn(T) -> String) -> String {
    v.map_or_else(|| "null".to_string(), f)
}

fn point(p: Point3) -> String {
    let (x, y, z) = (num(p.x), num(p.y), num(p.z));
    format!("[{x}, {y}, {z}]")
}

fn counts_json(c: Counts) -> String {
    let (f, e, v, l, s) = (c.faces, c.edges, c.vertices, c.loops, c.components);
    format!(
        "{{\"faces\": {f}, \"edges\": {e}, \"vertices\": {v}, \"loops\": {l}, \
         \"components\": {s}}}"
    )
}

fn bbox_json((lo, hi): (Point3, Point3)) -> String {
    let (lo, hi) = (point(lo), point(hi));
    format!("{{\"min\": {lo}, \"max\": {hi}}}")
}

fn check_json(c: &Check) -> String {
    let (name, pass, message) = (string(c.name), c.pass, string(&c.message));
    format!("{{\"name\": {name}, \"pass\": {pass}, \"message\": {message}}}")
}

fn blocker_json(upstream: &str, unit: &str) -> String {
    let (upstream, unit) = (string(upstream), string(unit));
    format!("{{\"upstream\": {upstream}, \"unit\": {unit}}}")
}

fn body_json(b: &Body) -> String {
    let kind = string(b.kind);
    let counts = counts_json(b.counts);
    let genus = opt(b.genus, |g| format!("{g}"));
    let volume = opt(b.volume, num);
    let area = num(b.area);
    let bbox = bbox_json(b.bbox);
    format!(
        "{{\"kind\": {kind}, \"counts\": {counts}, \"genus\": {genus}, \"volume\": {volume}, \
         \"area\": {area}, \"bbox\": {bbox}}}"
    )
}

fn outcome_json(o: &Outcome) -> String {
    let blocker = match &o.class {
        Class::Unported { upstream, unit } => blocker_json(upstream, unit),
        _ => "null".to_string(),
    };
    let file = string(&o.file);
    let class = string(o.class.label());
    let kind = string(o.kind);
    let stage = opt(o.stage, string);
    let error = opt(o.error.as_deref(), string);
    let breps: Vec<String> = o.bodies.iter().map(body_json).collect();
    let breps = breps.join(", ");
    let checks: Vec<String> = o.checks.iter().map(check_json).collect();
    let checks = checks.join(", ");
    let roundtrip = opt(o.roundtrip.as_deref(), string);
    let seconds = num(o.seconds);
    format!(
        "{{\"file\": {file}, \"class\": {class}, \"kind\": {kind}, \"stage\": {stage}, \
         \"blocker\": {blocker}, \"error\": {error}, \"breps\": [{breps}], \
         \"checks\": [{checks}], \"roundtrip\": {roundtrip}, \"seconds\": {seconds}}}"
    )
}

fn report_json(opts: &Opts, s: &Summary, entries: &[Entry]) -> String {
    let mut kinds = Vec::new();
    for (k, (total, pass)) in &s.kinds {
        let k = string(k);
        kinds.push(format!("{k}: {{\"total\": {total}, \"pass\": {pass}}}"));
    }
    let mut blockers = Vec::new();
    for ((upstream, unit), files) in &s.blockers {
        let (upstream, unit, count) = (string(upstream), string(unit), files.len());
        let files: Vec<String> = files.iter().map(|f| string(f)).collect();
        let files = files.join(", ");
        blockers.push(format!(
            "{{\"upstream\": {upstream}, \"unit\": {unit}, \"count\": {count}, \
             \"files\": [{files}]}}"
        ));
    }
    let models: Vec<&str> = entries.iter().map(|e| e.json.as_str()).collect();
    let dir = string(&opts.dir.display().to_string());
    let accuracy = num(opts.accuracy);
    let roundtrip = string(ROUNDTRIP);
    let total = s.total;
    let pass = s.count("pass");
    let read_fail = s.count("read-fail");
    let check_fail = s.count("check-fail");
    let unported = s.count("unported");
    let crash = s.count("crash");
    let (rt_tried, rt_pass, rt_drift) = s.roundtrip;
    let kinds = kinds.join(", ");
    let blockers = blockers.join(",\n");
    let models = models.join(",\n");
    format!(
        "{{\"schema\": 1, \"source\": \"crates/solidcore/examples/corpus\", \"dir\": {dir}, \
         \"accuracy\": {accuracy}, \"roundtrip\": {roundtrip},\n\"summary\": {{\"total\": {total}, \
         \"pass\": {pass}, \"read_fail\": {read_fail}, \"check_fail\": {check_fail}, \
         \"unported\": {unported}, \"crash\": {crash}, \
         \"roundtrip\": {{\"tried\": {rt_tried}, \"pass\": {rt_pass}, \"drift\": {rt_drift}}}, \
         \"kinds\": {{{kinds}}}}},\n\
         \"blockers\": [{blockers}],\n\"models\": [\n{models}\n]}}\n"
    )
}
