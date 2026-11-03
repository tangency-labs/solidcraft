//! The rig corpus's operand bodies (`crates/solidcore/tests/kernel_cases/`, #2549) through the
//! direct conversion: each record a case gives goes to the kernel as SolidCraft read it, back to
//! the record (which validates), to the kernel again (the same counts) and to the
//! same record again, to the rounding of a crossing at part scale. A sample of the entries, every `STRIDE`th in name order; a body the
//! conversion does not carry yet must be refused with its kind, never with a wrong record.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::Brep;
use solidcore_convert::brep::{to_kernel, to_record, Unsupported};
use solidcore_record::brep::Brep as Record;

/// Every this many entries with operand bodies, in name order (`CORPUS_STRIDE` overrides it:
/// 1 runs them all).
const STRIDE: usize = 7;

fn stride() -> usize {
    std::env::var("CORPUS_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(STRIDE)
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

fn entries(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("the rig corpus (#2549) at {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            entries(&p, out);
        } else if p.extension().is_some_and(|x| x == "json")
            && !p.to_string_lossy().ends_with(".bodies.json")
            && p.parent() != Some(corpus().as_path())
        {
            out.push(p);
        }
    }
}

fn counts(b: &Brep) -> [usize; 3] {
    [
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    ]
}

#[derive(Default, Debug)]
struct Tally {
    bodies: usize,
    round: usize,
    refused_in: Vec<String>,
    refused_out: Vec<String>,
}

/// One body: in as SolidCraft read it, then round. A refusal going in is the kernel's or the
/// record's (not this test's); going out, it must be a typed refusal, and is listed.
fn one(name: &str, record: &Record, trusted: bool, fixes: UpstreamFixes, t: &mut Tally) {
    t.bodies += 1;
    if std::env::var_os("CORPUS_TRACE").is_some() {
        eprintln!("{name}");
    }
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    let mut b = match to_kernel(&mut k, record, trusted) {
        Ok((b, _)) => b,
        Err(e) => {
            t.refused_in.push(format!("{name}: {e}"));
            return;
        }
    };
    let r1 = match to_record(&mut k, &mut b) {
        Ok((r, _)) => r,
        Err(e @ (Unsupported::NotYet(_) | Unsupported::Kernel(_))) => {
            t.refused_out.push(format!("{name}: {e}"));
            return;
        }
        Err(e) => panic!("{name}: the kernel's body is no record: {e}"),
    };
    r1.validate_built()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let (mut b2, _) =
        to_kernel(&mut k, &r1, true).unwrap_or_else(|e| panic!("{name}: back in: {e}"));
    assert_eq!(counts(&b2), counts(&b), "{name}: counts");
    let (r2, _) = to_record(&mut k, &mut b2).unwrap_or_else(|e| panic!("{name}: back out: {e}"));
    // The same record to the rounding of a crossing at part scale: a kernel line holds its
    // unit direction and length, so the end it gives back is the record's to an ulp or two.
    let mut out = Vec::new();
    differences(
        "",
        &serde_json::to_value(&r1).unwrap(),
        &serde_json::to_value(&r2).unwrap(),
        &mut out,
    );
    assert!(
        out.is_empty(),
        "{name}: the record changed on a round trip: {out:#?}"
    );
    t.round += 1;
}

/// The first few places two records' serde forms differ past the rounding of a number at part
/// scale (10⁻¹² of it, at least 10⁻¹²), with both values.
fn differences(at: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if out.len() >= 8 || a == b {
        return;
    }
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        if (x - y).abs() <= 1e-12 * x.abs().max(1.0) {
            return;
        }
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                differences(
                    &format!("{at}.{k}"),
                    v,
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                differences(&format!("{at}[{i}]"), v, w, out);
            }
        }
        _ => out.push(format!("{at}: {a} -> {b}")),
    }
}

#[test]
fn a_sample_of_the_corpus_bodies_goes_round_unchanged() {
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    let fixes_json: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    let mut fixes = UpstreamFixes::default();
    for flag in fixes_json.as_array().expect("fixes.json: flag names") {
        let name = flag.as_str().unwrap();
        *fixes
            .flag_mut(name)
            .unwrap_or_else(|| panic!("fixes.json: no fix {name}")) = true;
    }
    let mut t = Tally::default();
    let with_bodies = files.iter().filter_map(|f| {
        let e: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let bodies = e["bodies"].as_object()?.clone();
        (!bodies.is_empty()).then_some((f.clone(), bodies))
    });
    for (f, bodies) in with_bodies.step_by(stride()) {
        let gz = f.with_extension("bodies.json.gz");
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
            .read_to_string(&mut text)
            .unwrap();
        let records: Value = serde_json::from_str(&text).unwrap();
        for (id, how) in bodies {
            let record: Record = serde_json::from_value(records[&id].clone()).unwrap();
            let name = format!("{}:{id}", f.display());
            one(
                &name,
                &record,
                how["trusted"].as_bool().unwrap(),
                fixes,
                &mut t,
            );
        }
    }
    eprintln!("{t:#?}");
    assert!(t.round > 0, "{t:#?}");
}
