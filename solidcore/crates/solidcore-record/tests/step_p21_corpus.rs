//! The Part 21 parser over a STEP corpus: every file parses, the units of its representation
//! contexts, and its Part 42 curves and surfaces.
//!
//! Ignored by default; the corpus is not in the repository. Run it with the directory of
//! `.stp` / `.step` files (read in place, recursively) in `SOLIDCORE_STEP_CORPUS`:
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=/path/to/corpus cargo test --release -p solidcore-record \
//!     --test step_p21_corpus -- --ignored --nocapture
//! ```
//!
//! Each file prints one line: its entity count, duplicate ids and schema, or its error. Files
//! are read as bytes and decoded lossily (some writers put Latin-1 bytes in strings), as a
//! caller holding the file's bytes would. The test fails when any file does not parse. The
//! STEP files committed to the repository are parsed by `repository_files_parse`, which always
//! runs.
//!
//! `corpus_units` (ignored too, same variable and command with `corpus_units` as the filter)
//! reads every geometric representation context with assigned units in each file and prints
//! one line per file and a table of how often each length and plane angle unit occurs, and
//! how many contexts state an uncertainty. It fails when a file does not parse. Files with no
//! context whose length unit can be read are listed and counted, not failed: a file written
//! with Part 21 short names (`SUNT`, `LNGUNT`, `GMRPCN`, …) or one without geometry has none.
//!
//! `corpus_geometry` (ignored too, same variable, `corpus_geometry` as the filter) reads every
//! model-space curve entity of each file with `step::read::Reader::curve`, in the units of the
//! file's first geometric context (the curves of a parameter space, read through their pcurves,
//! and the surfaces come with slice 2c). It prints one line per file and, per entity kind, how
//! many were read and how many refused, with the reasons.
//! It also lists the entities where a check the record no longer makes (a direction shorter than
//! 1e-12, unit factors equal within 1e-12) would have decided otherwise. It fails when a file
//! does not parse; refusals are counted, not failed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use solidcore_record::step::p21;
use solidcore_record::step::units::{self, UnitKind};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        paths.push(entry.path());
    }
    paths.sort();
    for path in paths {
        if path.is_dir() {
            files(&path, out);
        } else if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "stp" | "step"))
        {
            out.push(path);
        }
    }
}

/// Parse each file; print one line per file and a total. Returns the failures.
fn parse_all(root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    let mut parsed = 0usize;
    let mut entities = 0usize;
    for path in paths {
        let name = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("{name}: unreadable: {e}");
                failures.push(format!("{name}: unreadable: {e}"));
                continue;
            }
        };
        match p21::parse(&String::from_utf8_lossy(&bytes)) {
            Ok(ex) => {
                parsed += 1;
                entities += ex.entities.len();
                println!(
                    "{name}: {} entities, {} duplicate ids, schema {:?}",
                    ex.entities.len(),
                    ex.duplicates.len(),
                    ex.schemas
                );
            }
            Err(e) => {
                println!("{name}: ERROR {e}");
                failures.push(format!("{name}: {e}"));
            }
        }
    }
    println!(
        "parsed {parsed} of {} files, {entities} entities",
        paths.len()
    );
    failures
}

#[test]
#[ignore = "needs a STEP corpus in SOLIDCORE_STEP_CORPUS"]
fn corpus_parses() {
    let Some(dir) = std::env::var_os("SOLIDCORE_STEP_CORPUS") else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let root = Path::new(&dir);
    let mut paths = Vec::new();
    files(root, &mut paths);
    assert!(!paths.is_empty(), "no STEP files under {dir:?}");
    let failures = parse_all(root, &paths);
    assert!(
        failures.is_empty(),
        "files that did not parse:\n{}",
        failures.join("\n")
    );
}

/// The STEP files in the repository parse to every instance they hold, each id once: a parser
/// that dropped or merged instances would change the counts (the instance lines in the files).
/// A length factor's name (millimetres per unit).
fn length_name(f: f64) -> String {
    match f {
        1.0 => "mm".into(),
        25.4 => "inch".into(),
        1000.0 => "m".into(),
        10.0 => "cm".into(),
        304.8 => "foot".into(),
        _ => format!("{f} mm"),
    }
}

/// A plane angle factor's name (radians per unit). A degree is the number the file writes:
/// `degree` when it is π/180 to 12 digits or more, else the written factor, so a file that
/// rounds π/180 shorter (`0.0174532925`) shows in the table as what it is.
fn angle_name(f: f64) -> String {
    if f == 1.0 {
        "rad".into()
    } else if (f - std::f64::consts::PI / 180.0).abs() <= 1e-14 {
        "degree".into()
    } else {
        format!("{f} rad")
    }
}

#[test]
#[ignore = "needs a STEP corpus in SOLIDCORE_STEP_CORPUS"]
fn corpus_units() {
    let Some(dir) = std::env::var_os("SOLIDCORE_STEP_CORPUS") else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let root = Path::new(&dir);
    let mut paths = Vec::new();
    files(root, &mut paths);
    assert!(!paths.is_empty(), "no STEP files under {dir:?}");
    let mut failures = Vec::new();
    let mut unread = Vec::new();
    let mut lengths: BTreeMap<String, usize> = BTreeMap::new();
    let mut angles: BTreeMap<String, usize> = BTreeMap::new();
    let (mut contexts, mut with_uncertainty, mut files_read) = (0usize, 0usize, 0usize);
    for path in &paths {
        let name = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string();
        let ex = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| p21::parse(&String::from_utf8_lossy(&b)))
        {
            Ok(ex) => ex,
            Err(e) => {
                println!("{name}: ERROR {e}");
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut here = Vec::new();
        let mut readable = false;
        for (&id, e) in &ex.entities {
            if !(e.has("GEOMETRIC_REPRESENTATION_CONTEXT") && e.has("GLOBAL_UNIT_ASSIGNED_CONTEXT"))
            {
                continue;
            }
            // Whether the context names a length unit that reads as a factor, rather than
            // falling back to millimetres.
            readable |= e
                .record("GLOBAL_UNIT_ASSIGNED_CONTEXT")
                .and_then(<[p21::Param]>::first)
                .and_then(p21::Param::as_list)
                .is_some_and(|l| {
                    l.iter().filter_map(p21::Param::as_ref_id).any(|u| {
                        matches!(units::unit_factor(&ex, u),
                            Some((UnitKind::Length, f)) if f.is_finite() && f > 0.0)
                    })
                });
            let c = units::context(&ex, id);
            contexts += 1;
            with_uncertainty += usize::from(c.uncertainty.is_some());
            let (l, a) = (length_name(c.units.len), angle_name(c.units.ang));
            *lengths.entry(l.clone()).or_default() += 1;
            *angles.entry(a.clone()).or_default() += 1;
            here.push(format!("#{id} {l}/{a} uncertainty {:?}", c.uncertainty));
        }
        println!("{name}: {} contexts: {}", here.len(), here.join(", "));
        if readable {
            files_read += 1;
        } else {
            println!("{name}: no context with a readable length unit");
            unread.push(name);
        }
    }
    println!(
        "{files_read} of {} files have a context with a readable length unit; {contexts} contexts, {with_uncertainty} with an uncertainty",
        paths.len()
    );
    println!("length units (contexts): {lengths:?}");
    println!("plane angle units (contexts): {angles:?}");
    println!(
        "no context with a readable length unit: {} files {unread:?}",
        unread.len()
    );
    assert!(
        failures.is_empty(),
        "files that did not parse:\n{}",
        failures.join("\n")
    );
}

/// The STEP files in the repository parse to every instance they hold, each id once: a parser
/// that dropped or merged instances would change the counts (the instance lines in the files).
#[test]
fn repository_files_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, entities) in [
        (
            "crates/goldens/src/units/topology_brep_constructor_tetra.stp",
            86,
        ),
        (
            "crates/solidcore-record/tests/fixtures/discord-2026-10-30-fillet.step",
            571,
        ),
    ] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        let ex = p21::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(ex.entities.len(), entities, "{path}");
        assert!(ex.duplicates.is_empty(), "{path}: {:?}", ex.duplicates);
    }
}

/// Curve entity names (simple instances) the corpus count looks at: every one the reader maps,
/// and the other ISO 10303-42 curves a file may hold, which it refuses.
const CURVES: &[&str] = &[
    "B_SPLINE_CURVE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "BEZIER_CURVE",
    "BOUNDED_SURFACE_CURVE",
    "CIRCLE",
    "CLOTHOID",
    "COMPOSITE_CURVE",
    "COMPOSITE_CURVE_ON_SURFACE",
    "CURVE_REPLICA",
    "ELLIPSE",
    "HYPERBOLA",
    "INTERSECTION_CURVE",
    "LINE",
    "OFFSET_CURVE_2D",
    "OFFSET_CURVE_3D",
    "PARABOLA",
    "POLYLINE",
    "QUASI_UNIFORM_CURVE",
    "SEAM_CURVE",
    "SURFACE_CURVE",
    "TRIMMED_CURVE",
    "UNIFORM_CURVE",
];

/// Surface entity names, likewise.
const SURFACES: &[&str] = &[
    "B_SPLINE_SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "BEZIER_SURFACE",
    "CONICAL_SURFACE",
    "CURVE_BOUNDED_SURFACE",
    "CYLINDRICAL_SURFACE",
    "DEGENERATE_TOROIDAL_SURFACE",
    "OFFSET_SURFACE",
    "ORIENTED_SURFACE",
    "PLANE",
    "QUASI_UNIFORM_SURFACE",
    "RECTANGULAR_COMPOSITE_SURFACE",
    "RECTANGULAR_TRIMMED_SURFACE",
    "SPHERICAL_SURFACE",
    "SURFACE_OF_LINEAR_EXTRUSION",
    "SURFACE_OF_REVOLUTION",
    "SURFACE_REPLICA",
    "TOROIDAL_SURFACE",
    "UNIFORM_SURFACE",
];

/// Whether `e` is a curve or a surface for the count, and its kind: the entity name, or for a
/// complex instance `(B_SPLINE_CURVE …)` with `rational` when it is.
fn geometry_kind(e: &p21::Entity) -> Option<(bool, String)> {
    if e.is_complex() {
        let rational = if e.has("RATIONAL_B_SPLINE_CURVE") || e.has("RATIONAL_B_SPLINE_SURFACE") {
            " rational"
        } else {
            ""
        };
        let is_curve =
            e.has("B_SPLINE_CURVE") || e.records.iter().any(|r| CURVES.contains(&r.name.as_str()));
        let is_surface = e.has("B_SPLINE_SURFACE")
            || e.records
                .iter()
                .any(|r| SURFACES.contains(&r.name.as_str()));
        let names: Vec<&str> = e
            .records
            .iter()
            .map(|r| r.name.as_str())
            .filter(|n| CURVES.contains(n) || SURFACES.contains(n))
            .collect();
        let kind = format!("({}){rational}", names.join(" "));
        return match (is_curve, is_surface) {
            (true, false) => Some((true, kind)),
            (false, true) => Some((false, kind)),
            _ => None,
        };
    }
    let n = e.name();
    if CURVES.contains(&n) {
        Some((true, n.to_string()))
    } else if SURFACES.contains(&n) {
        Some((false, n.to_string()))
    } else {
        None
    }
}

/// An error message with its entity ids replaced by `#n`, so equal reasons count together.
fn reason(e: &str) -> String {
    let mut out = String::new();
    let mut chars = e.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '#' && chars.peek().is_some_and(char::is_ascii_digit) {
            out.push('n');
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
        }
    }
    out
}

/// The curves of a file that lie in a parameter space: the items of its
/// `DEFINITIONAL_REPRESENTATION`s and the curves they refer to (a 2D trimmed curve's basis …).
fn curves_2d(ex: &p21::Exchange) -> std::collections::BTreeSet<u64> {
    fn refs(p: &p21::Param, out: &mut Vec<u64>) {
        match p {
            p21::Param::Ref(r) => out.push(*r),
            p21::Param::List(l) | p21::Param::Typed(_, l) => l.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }
    let mut stack = Vec::new();
    for e in ex.entities.values() {
        if e.name() == "DEFINITIONAL_REPRESENTATION"
            && let Some(items) = e.params().get(1)
        {
            refs(items, &mut stack);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = stack.pop() {
        let Some(e) = ex.get(id) else { continue };
        if !matches!(geometry_kind(e), Some((true, _))) || !seen.insert(id) {
            continue;
        }
        for r in &e.records {
            r.params.iter().for_each(|p| refs(p, &mut stack));
        }
    }
    seen
}

/// Read or refused, by reason.
#[derive(Default)]
struct Tally {
    read: usize,
    refused: BTreeMap<String, usize>,
}

impl Tally {
    fn add<T>(&mut self, r: &Result<T, String>) {
        match r {
            Ok(_) => self.read += 1,
            Err(e) => *self.refused.entry(reason(e)).or_default() += 1,
        }
    }
    fn refused(&self) -> usize {
        self.refused.values().sum()
    }
}

#[test]
#[ignore = "needs a STEP corpus in SOLIDCORE_STEP_CORPUS"]
fn corpus_geometry() {
    use solidcore_record::step::read::Reader;
    let Some(dir) = std::env::var_os("SOLIDCORE_STEP_CORPUS") else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let root = Path::new(&dir);
    let mut paths = Vec::new();
    files(root, &mut paths);
    assert!(!paths.is_empty(), "no STEP files under {dir:?}");
    let mut failures = Vec::new();
    // (group, kind) -> tally; the group is "curve" (surfaces and pcurves come with slice 2c).
    let mut kinds: BTreeMap<(&str, String), Tally> = BTreeMap::new();
    let mut mixed_units = Vec::new();
    let mut removed = Vec::new();
    for path in &paths {
        let name = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string();
        let ex = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| p21::parse(&String::from_utf8_lossy(&b)))
        {
            Ok(ex) => ex,
            Err(e) => {
                println!("{name}: ERROR {e}");
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        // The units of the file's first geometric context (the topology reader, which comes
        // later, reads each representation in its own context).
        let contexts: Vec<units::Units> = ex
            .entities
            .iter()
            .filter(|(_, e)| {
                e.has("GEOMETRIC_REPRESENTATION_CONTEXT") && e.has("GLOBAL_UNIT_ASSIGNED_CONTEXT")
            })
            .map(|(&id, _)| units::context(&ex, id).units)
            .collect();
        let u = contexts.first().copied().unwrap_or_default();
        if contexts.iter().any(|c| *c != u) {
            mixed_units.push(name.clone());
        }
        // Where a removed check would have decided otherwise: SolidCraft refused a direction
        // whose length is at most 1e-12 (here: only one that is exactly zero), and took the two
        // unit factors of a conic's parameter space as equal within 1e-12 relative (here: only
        // when equal).
        for (&id, e) in &ex.entities {
            if e.name() == "DIRECTION"
                && let Some(r) = e.params().get(1).and_then(p21::Param::as_list)
            {
                let sq: f64 = r.iter().filter_map(p21::Param::as_f64).map(|x| x * x).sum();
                if sq > 0.0 && sq <= 1e-24 {
                    removed.push(format!("{name}: #{id} a direction of length at most 1e-12"));
                }
            }
        }
        if u.len != u.ang && (u.len - u.ang).abs() <= 1e-12 * u.len.abs().max(u.ang.abs()) {
            removed.push(format!(
                "{name}: length and angle factors equal within 1e-12"
            ));
        }
        let flat = curves_2d(&ex);
        let mut reader = Reader::new(&ex, u);
        let mut here = Tally::default();
        for (&id, e) in &ex.entities {
            let Some((is_curve, kind)) = geometry_kind(e) else {
                continue;
            };
            // Surfaces, and the curves of a parameter space (read through their pcurves), come
            // with slice 2c.
            if !is_curve || flat.contains(&id) {
                continue;
            }
            let r = reader.curve(id);
            here.add(&r);
            kinds.entry(("curve", kind)).or_default().add(&r);
        }
        println!(
            "{name}: curves {} read, {} refused",
            here.read,
            here.refused()
        );
    }
    println!("by kind (group, kind: read / refused, with reasons):");
    let mut totals: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for ((group, kind), t) in &kinds {
        println!("  {group} {kind}: {} / {}", t.read, t.refused());
        for (why, n) in &t.refused {
            println!("      {n} x {why}");
        }
        let g = totals.entry(group).or_default();
        g.0 += t.read;
        g.1 += t.refused();
    }
    println!("totals (read / refused): {totals:?}");
    println!(
        "files whose contexts have different units (read in the first): {} {mixed_units:?}",
        mixed_units.len()
    );
    println!(
        "entities a removed check would have decided otherwise: {} {removed:?}",
        removed.len()
    );
    assert!(
        failures.is_empty(),
        "files that did not parse:\n{}",
        failures.join("\n")
    );
}
