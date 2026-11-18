//! #18195's refit replay of the golden rows the WR-493 retirement moved.
//!
//! A sphere found in a file NURB now keeps that NURB, so the file's UV trims stay on it.
//! #18230's tree replaced it by upstream's NURB made from the fitted circles, on which the
//! read dropped the file's trims and rebuilt them. For each moved row, [`DATA`] holds the
//! FNV-1a hash of #18230's committed row.
//!
//! The tests run each case twice:
//! 1. with every found sphere's file NURB replaced by the fitted one before the topology
//!    is made ([`super::before_topology`]; at least one replaced, on a read the seam sees),
//!    the encoded row hashes to #18230's row;
//! 2. as is, the encoded row equals the committed golden line.
//!
//! So each committed row is this branch's output, and it differs from #18230's only
//! through the NURB the found spheres keep. `corpus.parity` runs its two sides in this
//! process (`run_in_process`) so the seam sees their reads.
//!
//! [`DATA`] was made by [`record`] from #18230's expected rows (`REFIT_OLD`, its
//! `expected` directory): every case whose committed row differs from #18230's.

use std::collections::BTreeMap;

use serde_json::Value;

use super::state::{self, Refit};
use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::rng::fnv1a64;
use crate::units::corpus_parity::run_in_process;
use crate::UnitDiff;

mod data;
use data::DATA;

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// One moved row: unit, case and #18230's row hash.
pub(super) type Row = (&'static str, &'static str, u64);

pub(crate) fn unit(id: &str) -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

pub(crate) fn cases(u: &dyn UnitDiff) -> BTreeMap<String, Case> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    u.cases(&ctx)
        .unwrap()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}

pub(crate) fn run(u: &dyn UnitDiff, case: &Case) -> Value {
    let out = if u.id() == "corpus.parity" {
        run_in_process(case)
    } else {
        u.run_rust(case)
    };
    encode(
        &case.name,
        &out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name)),
    )
}

fn hash(v: &Value) -> u64 {
    fnv1a64(&serde_json::to_string(v).unwrap())
}

/// `run` with the refit installed: the row and what the seam saw.
pub(crate) fn run_refit(u: &dyn UnitDiff, case: &Case) -> (Value, Refit) {
    state::set(Some(Refit::default()));
    let line = run(u, case);
    let refit = state::take().expect("refit lost");
    (line, refit)
}

/// Runs every row of `unit_id` in [`DATA`] refit and as is; returns the row count.
fn replay(unit_id: &str) -> usize {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut n = 0;
    for &(_, name, old_hash) in DATA.iter().filter(|r| r.0 == unit_id) {
        n += 1;
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let (line, refit) = run_refit(u.as_ref(), case);
        if refit.reads == 0 || refit.replaced == 0 || !refit.failures.is_empty() {
            failures.push(format!("{name}: {refit:?}"));
        }
        if hash(&line) != old_hash {
            failures.push(format!("{name}: refit row is not #18230's row"));
        }
        let line = run(u.as_ref(), case);
        if golden.get(name) != Some(&line) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(failures.is_empty(), "{unit_id}:\n{}", failures.join("\n"));
    n
}

macro_rules! replay_tests {
    ($($test:ident: $unit:literal, $rows:literal;)*) => {$(
        #[test]
        fn $test() {
            assert_eq!(replay($unit), $rows);
        }
    )*};
}

replay_tests! {
    api_files_rows_move_only_through_the_kept_nurb: "topology.api_files", 14;
    assert_valid_rows_move_only_through_the_kept_nurb: "topology.assert_valid", 2;
    brep_p10_rows_move_only_through_the_kept_nurb: "topology.brep_p10", 6;
    brep_p4_rows_move_only_through_the_kept_nurb: "topology.brep_p4", 5;
    edge_rows_move_only_through_the_kept_nurb: "topology.edge", 32;
    edgeuse_p1_rows_move_only_through_the_kept_nurb: "topology.edgeuse_p1", 48;
    edgeuse_p3_rows_move_only_through_the_kept_nurb: "topology.edgeuse_p3", 16;
    face_p2_rows_move_only_through_the_kept_nurb: "topology.face_p2", 4;
    face_p3_rows_move_only_through_the_kept_nurb: "topology.face_p3", 24;
    face_p4_rows_move_only_through_the_kept_nurb: "topology.face_p4", 30;
    vertexuse_rows_move_only_through_the_kept_nurb: "topology.vertexuse", 4;
}

/// Writes `$REFIT_OUT`, the source of [`DATA`]: per unit below, every case whose
/// committed row differs from its row in `$REFIT_OLD` (#18230's `expected` directory),
/// with the hash of that old row; and a line per case of what the refit run gives.
#[test]
#[ignore = "makes the replay's data from #18230's expected rows"]
fn record() {
    use std::fmt::Write as _;
    let old_dir = std::path::PathBuf::from(std::env::var("REFIT_OLD").unwrap());
    let units = std::env::var("REFIT_UNITS").unwrap();
    let mut out = String::new();
    let mut log = String::new();
    for id in units.split(',') {
        let u = unit(id);
        let all = cases(u.as_ref());
        let now = read(&default_dir(), id).unwrap();
        let old = read(&old_dir, id).unwrap();
        for (name, line) in &now {
            let Some(before) = old.get(name) else {
                continue;
            };
            if before == line {
                continue;
            }
            let case = all
                .get(name)
                .unwrap_or_else(|| panic!("{id}: no case {name}"));
            let (refit_line, refit) = run_refit(u.as_ref(), case);
            let _ = writeln!(
                log,
                "{id}\t{name}\treplaced {}\treads {}\tfailures {:?}\tmatches {}",
                refit.replaced,
                refit.reads,
                refit.failures,
                &refit_line == before
            );
            let _ = writeln!(out, "    (\"{id}\", \"{name}\", {:#018x}),", hash(before));
        }
    }
    let path = std::env::var("REFIT_OUT").unwrap();
    std::fs::write(&path, out).unwrap();
    std::fs::write(format!("{path}.log"), log).unwrap();
}
