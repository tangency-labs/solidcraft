//! The golden rows the WR-450 retirement (#18923) moved, each run again with main's two
//! choices put back (tests only).
//!
//! The retirement changes two choices, both seen by the `wr57-replay` seam
//! (`solidcore`'s `surface::surface::replay`, a feature this crate turns on for its
//! tests only; the runners of the units below make their kernels with
//! [`crate::runner_kernel`], which carries the seam a test installs on its thread):
//!
//! - [`Seam::AnalyticCopy`]: `BSplineSurface::copy_and_add_analytics` keeps an exact copy
//!   of the B-spline (`[1]`) where its recognised analytic surface does not give the
//!   B-spline's points at the same parameters (7 x 7 grid); main kept the analytic (`[0]`)
//!   always.
//! - [`Seam::SplitTrims`]: `MakeEdgeInFaceWithVU` keeps the UV trims of the new face's
//!   edge uses (`[0]`); main dropped them (WR-361's restaging) wherever the copy of the
//!   split face's surface was of another kind (the seam's input `[1]`).
//!
//! Each test runs every moved row of its unit twice:
//! 1. with main's choice put in place at every call of both seams, the encoded row is
//!    main's golden line ([`MAIN_COMMIT`], `wr450_rows/main.jsonl`) under the goldens'
//!    own rule ([`diff`] with the unit's floors: what main's goldens check accepts; a bless
//!    keeps a line's old text while the output stays within that rule, so main's line need
//!    not be main's output bit for bit), and at least one call's choice differs from this
//!    branch's; the row's class (which seams differ) is the one [`DATA`] records;
//! 2. as is, the encoded row is the committed golden line.
//!
//! So each committed row is this branch's output and differs from main's only through
//! those two choices. The classes:
//! - `copy`: only exact copies; the kept copy is the source B-spline's net bit for bit
//!   (`BSplineSurface::copy`), where main's analytic was off it at the same parameters by
//!   more than the 3D tolerance at a grid point (a witnessed distance, so main's surface
//!   is not the source's there);
//! - `copy+trims`: a split face that main gave the analytic copy is now the exact copy,
//!   so main's restaging also dropped the new face's trims; with the exact copy the trims
//!   are the split face's own, on the same surface at the same parameters;
//! - `trims`: the analytic is kept (it gives the source's points within the tolerance at
//!   every grid point) and the new face keeps the split face's trims instead of dropping
//!   and rebuilding them.
//!
//! [`DATA`] was made by [`record`] with main's expected files (`WR450_MAIN`, a directory
//! of main's `expected/*.jsonl` at [`MAIN_COMMIT`]). **When a later change moves one of
//! these rows**, delete its entry (the tape is the retirement's proof against
//! [`MAIN_COMMIT`]) or record the data again on the main it merges into.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use solidcore::surface::surface::replay::{Call, Seam};

use crate::golden::encode;
use crate::golden::{default_dir, diff, read};
use crate::wr57_replay::{cases, run, unit, with_hook};
use crate::UnitDiff;

mod data;
use data::DATA;

/// The main commit [`DATA`] was recorded against.
const MAIN_COMMIT: &str = "1b210ef4b0";

/// Main's golden line of each row of [`DATA`], in its order (`{"unit": .., "line": ..}`).
const MAIN_LINES: &str = include_str!("wr450_rows/main.jsonl");

/// Main's golden line of `case` of `unit_id`.
fn main_line(unit_id: &str, case: &str) -> Value {
    MAIN_LINES
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["unit"] == unit_id && v["line"]["case"] == case)
        .map(|v| v["line"].clone())
        .unwrap_or_else(|| panic!("{unit_id}: no main line of {case}"))
}

/// How many calls of each seam the substituted run changed.
#[derive(Default, Clone, Copy)]
struct Flips {
    copies: usize,
    trims: usize,
}

impl Flips {
    fn class(self) -> &'static str {
        match (self.copies > 0, self.trims > 0) {
            (true, false) => "copy",
            (true, true) => "copy+trims",
            (false, true) => "trims",
            (false, false) => "none",
        }
    }
}

/// The encoded row of `case`: `corpus.parity` measured in this process (its runner
/// measures in a child process, which the seam does not reach), else [`run`].
fn row(u: &dyn UnitDiff, case: &crate::Case) -> Value {
    if u.id() == "corpus.parity" {
        let out = crate::units::corpus_parity::run_in_process(case);
        return encode(
            &case.name,
            &out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name)),
        );
    }
    run(u, case)
}

/// Runs `case` with main's choices at both WR-450 seams; returns the encoded row and how
/// many calls' choices differ from this branch's.
fn with_mains_choices(u: &dyn UnitDiff, case: &crate::Case) -> (Value, Flips) {
    let flips = Arc::new(Mutex::new(Flips::default()));
    let f = Arc::clone(&flips);
    let line = with_hook(
        move |c: Call<'_>| {
            let main = match c.seam {
                Seam::AnalyticCopy => vec![0],
                Seam::SplitTrims => c.input.to_vec(),
                _ => return c.own,
            };
            if c.own.as_ref().ok() != Some(&main) {
                let mut f = f.lock().unwrap();
                if c.seam == Seam::AnalyticCopy {
                    f.copies += 1;
                } else {
                    f.trims += 1;
                }
            }
            Ok(main)
        },
        || row(u, case),
    );
    let flips = *flips.lock().unwrap();
    (line, flips)
}

/// Replays every row of `unit_id` in [`DATA`]; returns the row count per class.
fn replay_unit(unit_id: &str) -> BTreeMap<&'static str, usize> {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut classes = BTreeMap::new();
    for &(_, name, class) in DATA.iter().filter(|r| r.0 == unit_id) {
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let (line, flips) = with_mains_choices(u.as_ref(), case);
        if let Some(change) = diff(&main_line(unit_id, name), &line, u.abs_floors()) {
            failures.push(format!(
                "{name}: with main's choices the row is not main's: {change:?}"
            ));
        }
        if flips.class() == "none" {
            failures.push(format!("{name}: no call's choice differs from main's"));
        }
        if flips.class() != class {
            failures.push(format!(
                "{name}: class {} (recorded {class})",
                flips.class()
            ));
        }
        *classes.entry(flips.class()).or_default() += 1;
        if golden.get(name) != Some(&row(u.as_ref(), case)) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(
        failures.is_empty(),
        "{unit_id} (main {MAIN_COMMIT}; see the module doc when a change moves these \
         rows):\n{}",
        failures.join("\n")
    );
    classes
}

macro_rules! replay_tests {
    ($($test:ident: $unit:literal, $rows:literal;)*) => {
        /// The units with moved rows and their row counts.
        const UNITS: &[(&str, usize)] = &[$(($unit, $rows)),*];
        $(
            #[test]
            fn $test() {
                let classes = replay_unit($unit);
                eprintln!("{}: classes {classes:?}", $unit);
                assert_eq!(classes.values().sum::<usize>(), $rows, "the rows of {}", $unit);
            }
        )*
    };
}

replay_tests! {
    feature_executive: "blend.feature_executive", 26;
    fillet_executive_p1: "blend.fillet_executive_p1", 1;
    fillet_executive_p3: "blend.fillet_executive_p3", 8;
    fillet_removal: "blend.fillet_removal", 26;
    fillet_standard_solver_p1: "blend.fillet_standard_solver_p1", 2;
    fillet_standard_solver_p2: "blend.fillet_standard_solver_p2", 4;
    topology_intersector_p1: "boolean.topology_intersector_p1", 1;
    topology_intersector_p2: "boolean.topology_intersector_p2", 2;
    corpus_parity: "corpus.parity", 6;
    heal_data_p1: "heal.heal_data_p1", 2;
    heal_data_p2: "heal.heal_data_p2", 8;
    heal_data_p3: "heal.heal_data_p3", 26;
    stitch_p1: "heal.stitch_p1", 2;
    stitch_p2: "heal.stitch_p2", 4;
    offset_geometry_creation: "offset.offset_geometry_creation", 3;
    api_trim_ops: "primitive.api_trim_ops", 5;
    query_brep: "query.brep", 7;
    query_face_p1: "query.face_p1", 17;
    query_face_p2: "query.face_p2", 14;
    query_face_p3: "query.face_p3", 1;
    api_files: "topology.api_files", 18;
    api_projection: "topology.api_projection", 1;
    assert_valid: "topology.assert_valid", 3;
    brep_cutting: "topology.brep_cutting", 6;
    brep_local_operation: "topology.brep_local_operation", 36;
    brep_mirror: "topology.brep_mirror", 51;
    brep_p1: "topology.brep_p1", 4;
    brep_p10: "topology.brep_p10", 30;
    brep_p2: "topology.brep_p2", 13;
    brep_p3: "topology.brep_p3", 2;
    brep_p4: "topology.brep_p4", 3;
    brep_p6: "topology.brep_p6", 24;
    brep_planar_section: "topology.brep_planar_section", 1;
    edge: "topology.edge", 17;
    edgeuse_p3: "topology.edgeuse_p3", 12;
    topology_face_p2: "topology.face_p2", 1;
    topology_face_p3: "topology.face_p3", 14;
    vertex: "topology.vertex", 1;
}

/// [`DATA`] lists exactly the rows the units above moved, with the counts above.
#[test]
fn data_covers_the_moved_rows() {
    let mut per_unit: BTreeMap<&str, usize> = BTreeMap::new();
    for row in DATA {
        *per_unit.entry(row.0).or_default() += 1;
    }
    let want: BTreeMap<&str, usize> = UNITS.iter().copied().collect();
    assert_eq!(per_unit, want);
    assert_eq!(DATA.len(), 402);
}

/// Writes [`DATA`] (`src/wr450_rows/data.rs`) and main's lines (`src/wr450_rows/main.jsonl`):
/// every row of every unit whose line differs from main's (`WR450_MAIN`, a directory of
/// main's expected files), with main's line and the row's class from a substituted run.
#[test]
#[ignore = "records the data; run with WR450_MAIN set"]
fn record() {
    let main_dir = std::path::PathBuf::from(std::env::var("WR450_MAIN").unwrap());
    let mut out = String::from(
        "//! The rows the WR-450 retirement moved (generated by `wr450_rows::record`, with\n\
         //! `main.jsonl`, main's golden line of each in the same order): unit, case and the\n\
         //! row's class.\n\n\
         /// One moved row.\n\
         pub(super) type Row = (&'static str, &'static str, &'static str);\n\n\
         /// The moved rows.\n\
         pub(super) const DATA: &[Row] = &[\n",
    );
    let mut lines = String::new();
    for u in crate::registry() {
        let id = u.id();
        let now = read(&default_dir(), id).unwrap();
        let main = read(&main_dir, id).unwrap();
        let moved: Vec<(&String, &Value)> = main
            .iter()
            .filter(|(n, v)| now.get(*n) != Some(*v))
            .collect();
        if moved.is_empty() {
            continue;
        }
        let all = cases(u.as_ref());
        for (name, line) in moved {
            let (_, flips) = with_mains_choices(u.as_ref(), &all[name.as_str()]);
            out += &format!("    ({id:?}, {name:?}, {:?}),\n", flips.class());
            lines += &serde_json::json!({ "unit": id, "line": line }).to_string();
            lines.push('\n');
        }
    }
    out += "];\n";
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/wr450_rows");
    std::fs::write(dir.join("data.rs"), out).unwrap();
    std::fs::write(dir.join("main.jsonl"), lines).unwrap();
}
