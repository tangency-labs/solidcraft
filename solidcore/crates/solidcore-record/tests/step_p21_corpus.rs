//! The Part 21 parser over a STEP corpus: every file parses, and the units of its representation
//! contexts.
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
