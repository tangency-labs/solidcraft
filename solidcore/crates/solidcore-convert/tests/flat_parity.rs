//! Parity with SolidCraft's flat-array hop (#3137): every operand record of the rig corpus
//! (`crates/solidcore/tests/kernel_cases/`) is what SolidCraft's flat export wrote (record →
//! arrays → kernel → arrays → record), or, for a body SolidCraft made or read itself (not
//! `trusted`), what it handed the kernel. Each goes to the kernel ([`to_kernel`]) and back
//! ([`to_record`]); the record that comes back is compared with the one that went in, field by
//! field, bit for bit. A difference is classed by where it is (indices dropped) and, for a
//! kind, by what it became; a number that differs within 10⁻¹² of its size is its own class
//! (rounding), with its count and the largest.
//!
//! On the whole corpus (1,332 bodies, at #3137): 1,256 come back bit for bit, 37 within 10⁻¹⁵
//! of their size, and 39 differ only in the named classes ([`NAMED`]) or on a sheet face used on
//! its reverse side.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr
)] // test

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore_convert::brep::{to_kernel, to_kernel_body, to_record, Unsupported};
use solidcore_record::brep::Brep as Record;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

fn entries(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
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

#[derive(Default)]
struct Class {
    bodies: usize,
    places: usize,
    examples: Vec<String>,
}

#[derive(Default)]
struct Tally {
    bodies: usize,
    identical: usize,
    rounding_only: usize,
    rounding_places: usize,
    rounding_largest: f64,
    refused_in: Vec<String>,
    /// Sheets with a face used on its reverse side (every difference follows from that).
    sheet_side: Vec<String>,
    refused_out: Vec<String>,
    classes: BTreeMap<String, Class>,
    /// (body, face, the record's kind, the kind that came back, trusted)
    relabelled: Vec<(String, usize, String, String, bool)>,
}

fn normal(path: &str) -> String {
    let mut out = String::new();
    let mut in_index = false;
    for c in path.chars() {
        match c {
            '[' => {
                in_index = true;
                out.push_str("[*]");
            }
            ']' => in_index = false,
            _ if in_index => {}
            _ => out.push(c),
        }
    }
    out
}

/// Every place `a` and `b` differ: (class, place); numbers within 10⁻¹² of their size counted
/// apart, with the largest relative difference.
fn walk(
    at: &str,
    a: &Value,
    b: &Value,
    diffs: &mut Vec<(String, String)>,
    rounding: &mut (usize, f64),
) {
    if a == b {
        return;
    }
    if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
        let rel = (x - y).abs() / x.abs().max(y.abs()).max(1.0);
        if rel <= 1e-12 {
            rounding.0 += 1;
            rounding.1 = rounding.1.max(rel);
            return;
        }
        diffs.push((normal(at), format!("{at}: {x} -> {y}")));
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                walk(
                    &format!("{at}.{k}"),
                    v,
                    y.get(k).unwrap_or(&Value::Null),
                    diffs,
                    rounding,
                );
            }
            for (k, w) in y {
                if !x.contains_key(k) {
                    walk(&format!("{at}.{k}"), &Value::Null, w, diffs, rounding);
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                walk(&format!("{at}[{i}]"), v, w, diffs, rounding);
            }
        }
        (Value::Array(x), Value::Array(y)) => diffs.push((
            format!("{}: length", normal(at)),
            format!("{at}: {} -> {}", x.len(), y.len()),
        )),
        (Value::String(x), Value::String(y)) => {
            diffs.push((format!("{}: {x} -> {y}", normal(at)), at.to_owned()));
        }
        _ => {
            let short = |v: &Value| {
                let s = v.to_string();
                s.chars().take(60).collect::<String>()
            };
            diffs.push((normal(at), format!("{at}: {} -> {}", short(a), short(b))));
        }
    }
}

fn kind(face: &Value) -> String {
    face["surface"]["kind"].as_str().unwrap_or("?").to_owned()
}

fn one(name: &str, record: &Record, trusted: bool, fixes: &UpstreamFixes, t: &mut Tally) {
    t.bodies += 1;
    // a sheet face used on its reverse side: the kernel keeps both uses of a sheet's face and
    // the record comes back with the `same` one, as SolidCraft's flat export wrote it
    // (a sheet's shell is listed by one region; a closed shell by the two it separates)
    let listed = |i: usize| {
        record
            .regions
            .iter()
            .filter(|r| r.shells.iter().any(|&s| s as usize == i))
            .count()
    };
    let sheet_side = record.shells.iter().enumerate().any(|(i, s)| match s {
        solidcore_record::brep::Shell::Faces { faces, .. } => {
            listed(i) == 1 && faces.iter().any(|f| !f.forward)
        }
        _ => false,
    });
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = *fixes;
    let built = match to_kernel(&mut k, record, trusted) {
        Ok((b, _)) => Ok(b),
        Err(Unsupported::NotYet(w)) if w.contains("healer") => {
            to_kernel_body(&mut k, record, trusted)
        }
        Err(e) => Err(e),
    };
    let mut b = match built {
        Ok(b) => b,
        Err(e) => {
            t.refused_in.push(format!("{name}: {e}"));
            return;
        }
    };
    let back = match to_record(&mut k, &mut b) {
        Ok((r, _)) => r,
        Err(e) => {
            t.refused_out.push(format!("{name}: {e}"));
            return;
        }
    };
    let (a, z) = (
        serde_json::to_value(record).unwrap(),
        serde_json::to_value(&back).unwrap(),
    );
    // `PARITY_DUMP=<dir>` and `PARITY_ONLY=<part of a name>`: both records written there
    if let (Ok(dir), Ok(only)) = (std::env::var("PARITY_DUMP"), std::env::var("PARITY_ONLY")) {
        if name.contains(&only) {
            let stem = name.replace(['/', ':'], "_");
            std::fs::write(format!("{dir}/{stem}.in.json"), a.to_string()).unwrap();
            std::fs::write(format!("{dir}/{stem}.out.json"), z.to_string()).unwrap();
        }
    }
    let mut diffs = Vec::new();
    let mut rounding = (0, 0.0_f64);
    walk("", &a, &z, &mut diffs, &mut rounding);
    t.rounding_places += rounding.0;
    t.rounding_largest = t.rounding_largest.max(rounding.1);
    if let (Some(fa), Some(fz)) = (a["faces"].as_array(), z["faces"].as_array()) {
        for (i, (x, y)) in fa.iter().zip(fz).enumerate() {
            if kind(x) != kind(y) {
                t.relabelled
                    .push((name.to_owned(), i, kind(x), kind(y), trusted));
            }
        }
    }
    if diffs.is_empty() {
        if rounding.0 == 0 {
            t.identical += 1;
        } else {
            t.rounding_only += 1;
        }
        return;
    }
    if sheet_side {
        t.sheet_side.push(name.to_owned());
        return;
    }
    let mut seen = BTreeMap::new();
    for (class, place) in diffs {
        seen.entry(class).or_insert((0, place)).0 += 1;
    }
    for (class, (n, place)) in seen {
        let tag = if trusted { "" } else { "(untrusted) " };
        let c = t.classes.entry(format!("{tag}{class}")).or_default();
        c.bodies += 1;
        c.places += n;
        if c.examples.len() < 3 {
            c.examples.push(format!("{name} {place}"));
        }
    }
}

fn fixes() -> UpstreamFixes {
    let fixes_json: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    let mut fixes = UpstreamFixes::default();
    for flag in fixes_json.as_array().unwrap() {
        *fixes.flag_mut(flag.as_str().unwrap()).unwrap() = true;
    }
    fixes
}

/// The differences that are intended, by class (indices dropped): what the flat hop does too,
/// or where the kernel's proof is stricter than the hop's sampled check (#3137's tables).
const NAMED: &[&str] = &[
    // a record whose body tolerance is above its least entity's: the import lowers it, as the
    // flat import's `own_tolerances` does
    ".tolerance",
    // a plane through three points: the kernel's plane holds no record of which kind it came
    // from, and the flat export never writes this kind
    ".faces[*].surface.kind: plane_through -> plane",
    ".faces[*].surface.origin",
    ".faces[*].surface.u_axis",
    ".faces[*].surface.v_axis",
    ".faces[*].surface.u_point",
    ".faces[*].surface.v_point",
    // a label its form is not proved on goes as the form alone (#2540): cylinders 1e-13 to
    // 1e-12 off their forms; tori within ulps of theirs (a proof gap, filed); and an extruded
    // circle the flat hop left a spline, proved a cylinder
    ".faces[*].surface.kind: cylinder -> rational_spline",
    ".faces[*].surface.kind: torus -> rational_spline",
    ".faces[*].surface.kind: rational_spline -> cylinder",
    ".faces[*].surface.frame",
    ".faces[*].surface.radius",
    ".faces[*].surface.major_radius",
    ".faces[*].surface.minor_radius",
    ".faces[*].surface.homogeneous",
    ".faces[*].surface.u_knots",
    ".faces[*].surface.v_knots",
    ".faces[*].nurbs",
];

/// Every `stride`th entry; the report is written to `$PARITY_OUT` when set. No body is refused
/// either way, and every difference is a named one.
fn parity(stride: usize) {
    let mut files = Vec::new();
    entries(&corpus(), &mut files);
    let fixes = fixes();
    let mut t = Tally::default();
    let with_bodies = files.iter().filter_map(|f| {
        let e: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let bodies = e["bodies"].as_object()?.clone();
        (!bodies.is_empty()).then_some((f.clone(), bodies))
    });
    for (f, bodies) in with_bodies.step_by(stride) {
        let gz = f.with_extension("bodies.json.gz");
        let mut text = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
            .read_to_string(&mut text)
            .unwrap();
        let records: Value = serde_json::from_str(&text).unwrap();
        let short = f.strip_prefix(corpus()).unwrap_or(&f).display().to_string();
        for (id, how) in bodies {
            let record: Record = serde_json::from_value(records[&id].clone()).unwrap();
            one(
                &format!("{short}:{id}"),
                &record,
                how["trusted"].as_bool().unwrap_or(true),
                &fixes,
                &mut t,
            );
        }
    }
    let mut report = String::new();
    let _ = writeln!(
        report,
        "bodies {} identical {} rounding-only {} (places {}, largest {:e}) differing {} refused in {} refused out {}",
        t.bodies,
        t.identical,
        t.rounding_only,
        t.rounding_places,
        t.rounding_largest,
        t.bodies - t.identical - t.rounding_only - t.refused_in.len() - t.refused_out.len(),
        t.refused_in.len(),
        t.refused_out.len()
    );
    for (class, c) in &t.classes {
        let _ = writeln!(
            report,
            "CLASS {class} | bodies {} places {}",
            c.bodies, c.places
        );
        for e in &c.examples {
            let _ = writeln!(report, "    {e}");
        }
    }
    for n in &t.sheet_side {
        let _ = writeln!(report, "SHEET-SIDE {n}");
    }
    for (n, f, a, b, trusted) in &t.relabelled {
        let _ = writeln!(report, "RELABEL {n} face {f}: {a} -> {b} trusted {trusted}");
    }
    for r in &t.refused_in {
        let _ = writeln!(report, "REFUSED-IN {r}");
    }
    for r in &t.refused_out {
        let _ = writeln!(report, "REFUSED-OUT {r}");
    }
    if let Ok(p) = std::env::var("PARITY_OUT") {
        std::fs::write(p, &report).unwrap();
    }
    eprintln!("{}", report.lines().next().unwrap_or(""));
    assert!(t.bodies > 0);
    assert!(t.refused_in.is_empty(), "{:#?}", t.refused_in);
    assert!(t.refused_out.is_empty(), "{:#?}", t.refused_out);
    let unnamed: Vec<&String> = t
        .classes
        .keys()
        .filter(|c| !NAMED.contains(&c.trim_start_matches("(untrusted) ")))
        .collect();
    assert!(unnamed.is_empty(), "unnamed differences: {unnamed:#?}");
}

/// The whole corpus (`PARITY_STRIDE` takes every nth entry; the run takes seconds in release).
#[test]
fn every_corpus_body_comes_back_as_the_flat_export_wrote_it() {
    parity(
        std::env::var("PARITY_STRIDE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
    );
}
