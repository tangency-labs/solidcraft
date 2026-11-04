//! The kernel rig's corpus (`crates/solidcore/tests/kernel_cases/`, its README), replayed
//! through this crate's public operations and graded against each case's `rig` values: what
//! SolidCraft's kernel rig measured at the corpus's pin.
//!
//! Each case is one of:
//! - **pass**: every operation refused or ran as the rig's trace says, and the final bodies'
//!   count is the rig's, their face, edge and vertex counts are the records' (`outcome.raw_counts`:
//!   the rig's own merge faces across analytic surfaces), and their volume is within the case's
//!   tolerance (`expected.tolerance`, else the rig's relative 1e-3);
//! - **fail**: otherwise, with what differs;
//! - **unsupported**: the kernel alone does not account for the result (`operation_expressible`
//!   false), the rig itself rated it unsupported, or the conversion does not carry a record yet;
//! - **unported**: an operation this crate does not have yet, named, so the case is not run;
//! - **known-failing**: listed in `known_failing.json` beside the corpus, the one list, each entry
//!   with its reason and issue. A listed case whose result moves from the rig's is reported, so
//!   the list can shrink.
//!
//! The corpus's rules: `transform` is a copy, then the copy moved in place; `tagged` changes no
//! geometry and its history is not graded; picks are record indices, passed to the operations
//! with the body's record [`Order`] (the one face index convention).
//!
//! The test is `#[ignore]`d, out of the workspace run: run it with `--ignored`.
//!
//! Environment:
//! - `KERNEL_CASES_AREA`: comma-separated areas (a prefix each: `ladder` takes every tier) to run
//!   only those, the CI shards (#2394);
//! - `KERNEL_CASES_CASE`: run only cases whose name contains it;
//! - `KERNEL_CASES_TIMEOUT_S`: a case's time limit, 180 s by default; one past it is a hang, and
//!   the cases after it are not run (it holds the process kernel);
//! - `KERNEL_CASES_REPORT`: a file to write the per-area table and the lists to;
//! - `KERNEL_CASES_REPORT_ONLY`: set, the test reports without failing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss
)] // test

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use serde_json::Value;
use solidcore_ops::{make_sphere, offset, session, Body, Order};
use solidcore_record::brep::Brep as Record;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// The operations this crate runs; any other is unported. A family stacked onto ops adds its
/// operations here and to [`call`].
const PORTED: &[&str] = &[
    "copy",
    "tagged",
    "transform",
    "make_sphere",
    "shell",
    "offset_faces",
    "thicken",
];

/// The relative tolerance the rig grades volume and area with when a case states none.
const DEFAULT_REL: f64 = 1e-3;

/// The volume's relative accuracy asked of the kernel.
const VOLUME_ACCURACY: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Pass,
    Fail,
    Unsupported,
    Unported,
    KnownFailing,
}

/// One case's grade, and what to say about it.
#[derive(Debug)]
struct Graded {
    area: String,
    case: String,
    kind: Kind,
    note: String,
    /// The operations not ported, for an unported case (a listed one included).
    unported: Vec<String>,
    /// The largest relative volume difference from the rig, for a run case.
    deviation: Option<f64>,
    /// For a listed case: whether its result moved from the rig's.
    moved: bool,
    /// Whether it ran past its time limit.
    hang: bool,
}

/// An entry of the known-failing list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    case: String,
    issue: String,
    reason: String,
}

fn known_failing() -> BTreeMap<String, Listed> {
    let text = std::fs::read_to_string(corpus().join("known_failing.json")).unwrap();
    let list: Vec<Listed> = serde_json::from_str(&text).unwrap();
    let mut out = BTreeMap::new();
    for l in list {
        assert!(!l.reason.is_empty() && !l.issue.is_empty(), "{}", l.case);
        let case = l.case.clone();
        assert!(out.insert(case.clone(), l).is_none(), "{case} listed twice");
    }
    out
}

fn entries(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            entries(&p, out);
        } else if p.extension().is_some_and(|x| x == "json") && p.parent() != Some(&corpus()) {
            out.push(p);
        }
    }
}

fn records(entry: &Path, e: &Value) -> serde_json::Map<String, Value> {
    let Some(name) = e["records"].as_str() else {
        return serde_json::Map::new();
    };
    let gz = std::fs::File::open(entry.parent().unwrap().join(name)).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A body of the run, and the record order its picks are taken through, once known.
#[derive(Clone)]
struct Slot {
    body: Arc<Body>,
    order: Option<Order>,
}

/// Why a case stopped before grading.
enum Stop {
    Unsupported(String),
    Fail(String),
}

fn f64s<const N: usize>(v: &Value) -> Option<[f64; N]> {
    let a = v.as_array()?;
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64()?;
    }
    (a.len() == N).then_some(out)
}

/// A pick's record indices.
fn picked(p: &Value) -> Result<Vec<usize>, Stop> {
    let mut out = Vec::new();
    for x in p["picked"].as_array().into_iter().flatten() {
        match x["index"].as_u64() {
            Some(i) => out.push(usize::try_from(i).unwrap()),
            None => return Err(Stop::Unsupported(format!("a pick missed: {x}"))),
        }
    }
    Ok(out)
}

/// The picked faces' record indices, and the record order they are taken through: an operation
/// takes record indices with the body's [`Order`].
fn faces(slot: &mut Slot, pick: &Value) -> Result<(Vec<usize>, Order), Stop> {
    let record = picked(pick)?;
    if slot.order.is_none() {
        // a body an operation made: its record order is the one its record is written in
        let (_, order) = slot
            .body
            .to_record()
            .map_err(|e| Stop::Unsupported(format!("to_record for a pick: {e}")))?;
        slot.order = Some(order);
    }
    let order = slot
        .order
        .clone()
        .ok_or_else(|| Stop::Fail("no record order".to_owned()))?;
    Ok((record, order))
}

/// Runs one traced call: its output, or the operation's error as text.
fn call(c: &Value, ins: &mut [Slot]) -> Result<Result<Slot, String>, Stop> {
    let p = &c["params"];
    let bad = |what: &str| Stop::Fail(format!("{}: parameter {what}", c["op"]));
    let op = c["op"].as_str().unwrap();
    let first = || ins.first().cloned().ok_or_else(|| bad("input"));
    let in_place =
        |slot: Slot, r: solidcore_ops::Result<()>| r.map(|()| slot).map_err(|e| e.to_string());
    Ok(match op {
        "copy" => {
            let s = first()?;
            s.body
                .copy()
                .map(|b| Slot {
                    body: Arc::new(b),
                    order: s.order,
                })
                .map_err(|e| e.to_string())
        }
        "tagged" => Ok(first()?),
        "transform" => {
            let s = first()?;
            let m = p["matrix"].as_array().ok_or_else(|| bad("matrix"))?;
            let mut matrix = [[0.0; 4]; 4];
            for (row, v) in matrix.iter_mut().zip(m) {
                *row = f64s::<4>(v).ok_or_else(|| bad("matrix"))?;
            }
            match s.body.copy() {
                Ok(b) => {
                    let out = Slot {
                        body: Arc::new(b),
                        order: s.order,
                    };
                    let r = out.body.transform(&matrix);
                    in_place(out, r)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        "make_sphere" => {
            let center = f64s::<3>(&p["center"]).ok_or_else(|| bad("center"))?;
            let radius = p["radius"].as_f64().ok_or_else(|| bad("radius"))?;
            make_sphere(center, radius)
                .map(|b| Slot {
                    body: Arc::new(b),
                    order: None,
                })
                .map_err(|e| e.to_string())
        }
        "shell" => {
            let s = ins.first_mut().ok_or_else(|| bad("input"))?;
            let (open, order) = faces(s, &p["open_faces"])?;
            let t = p["thickness"].as_f64().ok_or_else(|| bad("thickness"))?;
            let s = s.clone();
            let r = offset::shell(&s.body, &order, &open, t);
            in_place(Slot { order: None, ..s }, r)
        }
        "offset_faces" => {
            let s = ins.first_mut().ok_or_else(|| bad("input"))?;
            let (picked, order) = faces(s, &p["faces"])?;
            let d = p["distance"].as_f64().ok_or_else(|| bad("distance"))?;
            let s = s.clone();
            let r = offset::offset_faces(&s.body, &order, &picked, d);
            in_place(Slot { order: None, ..s }, r)
        }
        "thicken" => {
            let s = first()?;
            let t = p["thickness"].as_f64().ok_or_else(|| bad("thickness"))?;
            let sym = p["symmetric"].as_bool().ok_or_else(|| bad("symmetric"))?;
            let r = offset::thicken(&s.body, t, sym);
            in_place(Slot { order: None, ..s }, r)
        }
        other => {
            return Err(Stop::Fail(format!(
                "{other} is listed as ported but not run"
            )))
        }
    })
}

/// What the case's final bodies measure.
#[derive(Debug, Default)]
struct Measured {
    bodies: usize,
    counts: [usize; 3],
    volume: Option<f64>,
}

fn measure(result: &[Slot]) -> Result<Measured, String> {
    let mut m = Measured {
        bodies: result.len(),
        ..Measured::default()
    };
    let mut volume = Some(0.0);
    for s in result {
        let c = s.body.counts().map_err(|e| format!("counts: {e}"))?;
        for (a, b) in m.counts.iter_mut().zip(c) {
            *a += b;
        }
        volume = match (volume, s.body.volume(VOLUME_ACCURACY)) {
            (Some(v), Ok(x)) => Some(v + x),
            _ => None,
        };
    }
    m.volume = volume;
    Ok(m)
}

/// The case's tolerance on a volume: relative, or absolute.
fn within(got: f64, want: f64, tol: &Value) -> bool {
    let d = (got - want).abs();
    if let Some(a) = tol["abs_mm"].as_f64() {
        return d <= a;
    }
    d <= tol["rel"].as_f64().unwrap_or(DEFAULT_REL) * want.abs()
}

/// Runs a case's calls and grades the result against `rig` (or, for a hand-written case
/// without one, `expected`): `Ok(deviation)` for a match.
fn run_case(entry: &Path, e: &Value) -> Result<Option<f64>, Stop> {
    let recs = records(entry, e);
    let mut bodies: HashMap<String, Slot> = HashMap::new();
    for (name, how) in e["bodies"].as_object().into_iter().flatten() {
        let record: Record = serde_json::from_value(recs[name].clone())
            .map_err(|x| Stop::Fail(format!("{name}: record: {x}")))?;
        let trusted = how["trusted"].as_bool().unwrap_or(true);
        match Body::from_record(&record, trusted) {
            Ok((b, order)) => {
                bodies.insert(
                    name.clone(),
                    Slot {
                        body: Arc::new(b),
                        order: Some(order),
                    },
                );
            }
            Err(solidcore_ops::Error::Unsupported(w)) => {
                return Err(Stop::Unsupported(format!("{name}: the conversion: {w}")))
            }
            Err(x) => return Err(Stop::Fail(format!("{name}: from_record: {x}"))),
        }
    }
    for (i, c) in e["calls"].as_array().unwrap().iter().enumerate() {
        let op = c["op"].as_str().unwrap();
        let mut ins = Vec::new();
        for n in c["inputs"].as_array().unwrap() {
            let n = n.as_str().unwrap();
            ins.push(
                bodies
                    .get(n)
                    .cloned()
                    .ok_or_else(|| Stop::Fail(format!("call {i} ({op}): no body {n}")))?,
            );
        }
        let traced_error = c.get("error").and_then(Value::as_str);
        match (call(c, &mut ins)?, traced_error) {
            (Ok(out), None) => {
                for n in c["outputs"].as_array().into_iter().flatten() {
                    bodies.insert(n.as_str().unwrap().to_owned(), out.clone());
                }
            }
            // refused where the rig's kernel refused: the case ends there, as it did
            (Err(_), Some(_)) => return Ok(None),
            (Err(x), None) => return Err(Stop::Fail(format!("call {i} ({op}) refused: {x}"))),
            (Ok(_), Some(want)) => {
                return Err(Stop::Fail(format!(
                    "call {i} ({op}) ran where the rig's refused: {want}"
                )))
            }
        }
    }
    let mut result = Vec::new();
    for r in e["result"].as_array().unwrap() {
        let n = r
            .get("made")
            .or_else(|| r.get("operand"))
            .and_then(Value::as_str)
            .unwrap();
        result.push(
            bodies
                .get(n)
                .cloned()
                .ok_or_else(|| Stop::Fail(format!("no final body {n}")))?,
        );
    }
    let got = measure(&result).map_err(Stop::Fail)?;
    let tol = &e["expected"]["tolerance"];
    let want = match e.get("rig") {
        Some(r) if r["measured"].is_object() => &r["measured"],
        Some(r) if !r["status"].is_null() => {
            return Err(Stop::Fail(format!(
                "ran where the rig stopped: {}",
                r["error"]
            )))
        }
        // a hand-written case: its expected values
        _ => &e["expected"],
    };
    let mut diffs = Vec::new();
    if let Some(n) = want["body_count"].as_u64() {
        if n != got.bodies as u64 {
            diffs.push(format!("bodies {} (rig {n})", got.bodies));
        }
    }
    // the rig's own counts merge faces across analytic surfaces; the records' are the kernel's
    let counts = match &e["outcome"]["raw_counts"] {
        r if r.is_object() => r,
        _ => want,
    };
    for (k, g) in ["faces", "edges", "vertices"].iter().zip(got.counts) {
        if let Some(n) = counts[*k].as_u64() {
            if n != g as u64 {
                diffs.push(format!("{k} {g} (rig {n})"));
            }
        }
    }
    let mut deviation = None;
    if let Some(v) = want["volume_mm3"].as_f64() {
        match got.volume {
            Some(g) => {
                if v != 0.0 {
                    deviation = Some((g - v).abs() / v.abs());
                }
                if !within(g, v, tol) {
                    diffs.push(format!("volume {g} (rig {v})"));
                }
            }
            None => diffs.push(format!("volume not measured (rig {v})")),
        }
    }
    if diffs.is_empty() {
        Ok(deviation)
    } else {
        Err(Stop::Fail(diffs.join(", ")))
    }
}

fn grade(entry: &Path, listed: &BTreeMap<String, Listed>, timeout: Duration) -> Graded {
    let e: Value = serde_json::from_str(&std::fs::read_to_string(entry).unwrap()).unwrap();
    let area = e["area"].as_str().unwrap().to_owned();
    let case = e["case"].as_str().unwrap().to_owned();
    let key = format!("{area}/{case}");
    let mut g = Graded {
        area,
        case,
        kind: Kind::Pass,
        note: String::new(),
        unported: Vec::new(),
        deviation: None,
        moved: false,
        hang: false,
    };
    let is_listed = listed.contains_key(&key);
    let rig_status = e["rig"]["status"].as_str().unwrap_or("").to_owned();
    if !e["operation_expressible"].as_bool().unwrap_or(false) {
        g.kind = if is_listed {
            Kind::KnownFailing
        } else {
            Kind::Unsupported
        };
        g.note = format!("not the kernel's alone: {}", e["why_not"]);
        return g;
    }
    let mut unported: BTreeSet<String> = BTreeSet::new();
    for c in e["calls"].as_array().unwrap() {
        let op = c["op"].as_str().unwrap();
        if !PORTED.contains(&op) {
            unported.insert(op.to_owned());
        }
    }
    if !unported.is_empty() {
        g.unported = unported.into_iter().collect();
        g.kind = if is_listed {
            Kind::KnownFailing
        } else {
            Kind::Unported
        };
        g.note = format!("unported: {}", g.unported.join(", "));
        return g;
    }
    let (tx, rx) = mpsc::channel();
    let path = entry.to_path_buf();
    let started = Instant::now();
    std::thread::spawn(move || {
        let r = std::panic::catch_unwind(|| run_case(&path, &e));
        let _ = tx.send(r);
    });
    let r = match rx.recv_timeout(timeout) {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err(Stop::Fail("panicked".to_owned())),
        Err(_) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Fail
            };
            g.note = format!("hang: past {} s", started.elapsed().as_secs());
            g.moved = is_listed;
            g.hang = true;
            g.note
                .push_str(" (the run stops here: the case holds the process kernel)");
            return g;
        }
    };
    match r {
        Ok(dev) => {
            g.deviation = dev;
            if is_listed {
                g.kind = Kind::KnownFailing;
                "the rig's result (still failing as listed)".clone_into(&mut g.note);
            } else if rig_status == "unsupported" {
                g.kind = Kind::Unsupported;
                "rated unsupported by the rig".clone_into(&mut g.note);
            }
        }
        Err(Stop::Unsupported(w)) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Unsupported
            };
            g.note = w;
        }
        Err(Stop::Fail(w)) => {
            g.kind = if is_listed {
                Kind::KnownFailing
            } else {
                Kind::Fail
            };
            g.moved = is_listed;
            g.note = w;
        }
    }
    g
}

#[test]
#[ignore = "the whole corpus is slow; run it with --ignored, KERNEL_CASES_AREA shards it"]
fn kernel_cases() {
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let listed = known_failing();
    let areas: Option<Vec<String>> = std::env::var("KERNEL_CASES_AREA").ok().map(|a| {
        a.split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    });
    let only = std::env::var("KERNEL_CASES_CASE").ok();
    let timeout = Duration::from_secs(
        std::env::var("KERNEL_CASES_TIMEOUT_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(180),
    );
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    files.sort();
    let root = corpus();
    let selected: Vec<PathBuf> = files
        .into_iter()
        .filter(|f| {
            let rel = f
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let area_ok = areas.as_ref().is_none_or(|a| {
                a.iter()
                    .any(|x| rel == *x || rel.starts_with(&format!("{x}/")))
            });
            area_ok && only.as_ref().is_none_or(|c| rel.contains(c.as_str()))
        })
        .collect();
    // every listed case exists
    let all: BTreeSet<String> = {
        let mut f = Vec::new();
        entries(&root, &mut f);
        f.iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };
    for k in listed.keys() {
        assert!(
            all.contains(k),
            "known_failing.json lists {k}, which the corpus does not have"
        );
    }

    let mut graded = Vec::new();
    let mut not_run = 0;
    for f in &selected {
        if graded.last().is_some_and(|g: &Graded| g.hang) {
            not_run += 1;
            continue;
        }
        graded.push(grade(f, &listed, timeout));
    }

    let report = report(&graded, &listed, not_run);
    println!("{report}");
    if let Ok(p) = std::env::var("KERNEL_CASES_REPORT") {
        std::fs::write(p, &report).unwrap();
    }
    let failed: Vec<_> = graded.iter().filter(|g| g.kind == Kind::Fail).collect();
    let moved: Vec<_> = graded.iter().filter(|g| g.moved).collect();
    if std::env::var_os("KERNEL_CASES_REPORT_ONLY").is_none() {
        assert!(
            failed.is_empty() && moved.is_empty() && not_run == 0,
            "{} failed, {} listed moved, {not_run} not run",
            failed.len(),
            moved.len()
        );
    }
}

fn report(graded: &[Graded], listed: &BTreeMap<String, Listed>, not_run: usize) -> String {
    let mut by_area: BTreeMap<&str, [usize; 5]> = BTreeMap::new();
    let mut dev: BTreeMap<&str, f64> = BTreeMap::new();
    let mut ops: BTreeMap<&str, usize> = BTreeMap::new();
    for g in graded {
        let row = by_area.entry(&g.area).or_default();
        row[g.kind as usize] += 1;
        if let Some(d) = g.deviation {
            let m = dev.entry(&g.area).or_default();
            *m = m.max(d);
        }
        for o in &g.unported {
            *ops.entry(o).or_default() += 1;
        }
    }
    let mut s = String::new();
    writeln!(s, "| Area | Cases | pass | fail | unsupported | unported | known-failing | max rel. volume dev. |").unwrap();
    writeln!(s, "|---|---|---|---|---|---|---|---|").unwrap();
    let mut total = [0; 5];
    for (a, row) in &by_area {
        let n: usize = row.iter().sum();
        for (t, r) in total.iter_mut().zip(row) {
            *t += r;
        }
        let d = dev
            .get(a)
            .map_or_else(|| "-".to_owned(), |d| format!("{d:.1e}"));
        writeln!(
            s,
            "| {a} | {n} | {} | {} | {} | {} | {} | {d} |",
            row[0], row[1], row[2], row[3], row[4]
        )
        .unwrap();
    }
    let n: usize = total.iter().sum();
    writeln!(
        s,
        "| **all** | {n} | {} | {} | {} | {} | {} | |",
        total[0], total[1], total[2], total[3], total[4]
    )
    .unwrap();
    if not_run > 0 {
        writeln!(s, "\n{not_run} case(s) not run after a hang.").unwrap();
    }
    writeln!(s, "\nUnported operations (cases needing each):").unwrap();
    for (o, n) in &ops {
        writeln!(s, "- {o}: {n}").unwrap();
    }
    let mut section = |title: &str, f: &dyn Fn(&Graded) -> bool| {
        let v: Vec<_> = graded.iter().filter(|g| f(g)).collect();
        if !v.is_empty() {
            writeln!(s, "\n{title} ({}):", v.len()).unwrap();
            for g in v {
                writeln!(s, "- {}/{}: {}", g.area, g.case, g.note).unwrap();
            }
        }
    };
    section("Fail", &|g| g.kind == Kind::Fail);
    section(
        "Known-failing whose result moved from the rig's (check: fixed, or a new failure)",
        &|g| g.moved,
    );
    section("Unsupported, run", &|g| {
        g.kind == Kind::Unsupported && !g.note.starts_with("not the kernel")
    });
    let ran: Vec<_> = graded
        .iter()
        .filter(|g| {
            g.kind == Kind::KnownFailing && listed.contains_key(&format!("{}/{}", g.area, g.case))
        })
        .collect();
    writeln!(s, "\nKnown-failing ({}):", ran.len()).unwrap();
    for g in ran {
        let l = &listed[&format!("{}/{}", g.area, g.case)];
        writeln!(s, "- {}/{} [{}]: {}", g.area, g.case, l.issue, g.note).unwrap();
    }
    s
}
