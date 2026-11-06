//! The Part 21 parser over a STEP corpus: every file parses.
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

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test

use std::path::{Path, PathBuf};

use solidcore_record::step::p21;

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
#[test]
fn repository_files_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (path, entities) in [
        (
            "crates/difftest/src/units/topology_brep_constructor_tetra.stp",
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
