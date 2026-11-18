//! #18230's tolerance-substitution replay of the golden rows the edge/face gap cover moved.
//!
//! The healer's `Fix_TolSizes` now raises an edge tolerance that is under the edge's own
//! edge/face gap to `zone_tol3d(gain * gap)`, and the vertex step then raises its vertices.
//! Every moved row of the nine units below reads a file (or a patched generated body)
//! through a [`super::after_read`] seam. For each moved row, [`DATA`] holds:
//! - every edge and vertex tolerance that differs between main's read and this branch's
//!   read, read by read, as `(this branch's bits, main's bits)`;
//! - the FNV-1a hash of main's encoded row, from main's own run of the same case.
//!
//! The tests run each case twice:
//! 1. with main's bits put back on exactly those entities (each must hold this branch's
//!    bits first, except on `corpus.parity`'s read back of the body its round trip writes
//!    from the substituted read, which must hold main's already; every read must
//!    happen), the encoded row hashes to main's row;
//! 2. as is, the encoded row equals the committed golden line.
//!
//! So each committed row is this branch's output, and it differs from main's run only
//! through the listed tolerances. `corpus.parity` runs its two sides in this process
//! ([`run_in_process`]) so the seam sees their reads.
//!
//! [`DATA`] was made by [`record`] on main and on this branch (`TOLSUB_CASES`, a
//! `unit<TAB>case` list, and `TOLSUB_OUT`), diffed read by read.

use std::collections::BTreeMap;

use serde_json::Value;

use super::state::{self, Mode, Sub};
use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::rng::fnv1a64;
use crate::units::corpus_parity::run_in_process;
use crate::UnitDiff;

mod data;
use data::DATA;

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// One moved row: unit, case, main's row hash, this case's read count and its
/// substitutions `(read, vertex, index, this branch's bits, main's bits)`.
pub(super) type Row = (
    &'static str,
    &'static str,
    u64,
    u32,
    &'static [(u32, bool, u32, u64, u64)],
);

fn unit(id: &str) -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

fn cases(u: &dyn UnitDiff) -> BTreeMap<String, Case> {
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

fn run(u: &dyn UnitDiff, case: &Case) -> Value {
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

/// Runs every row of `unit_id` in [`DATA`] substituted and as is; returns the row count.
fn replay(unit_id: &str) -> usize {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut n = 0;
    for &(_, name, main_hash, reads, subs) in DATA.iter().filter(|r| r.0 == unit_id) {
        n += 1;
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let subs: Vec<Sub> = subs
            .iter()
            .map(|&(read, vertex, index, now, main)| Sub {
                read,
                vertex,
                index,
                now,
                main,
            })
            .collect();
        let count = subs.len();
        // (only `corpus.parity`'s read back, read 2 on, of the body its round trip writes
        // from the substituted read holds main's bits already)
        let read_back = if unit_id == "corpus.parity" {
            subs.iter().filter(|s| s.read >= 2).count()
        } else {
            0
        };
        assert!(count > 0, "{unit_id} {name}: no substitution");
        state::set(Some(Mode::Substitute {
            subs,
            reads: 0,
            applied: 0,
            already: 0,
            failures: Vec::new(),
        }));
        let line = run(u.as_ref(), case);
        let Some(Mode::Substitute {
            reads: seen,
            applied,
            already,
            failures: f,
            ..
        }) = state::take()
        else {
            panic!("mode lost");
        };
        if seen != reads || applied != count || already != read_back || !f.is_empty() {
            failures.push(format!(
                "{name}: {seen} of {reads} reads, {applied} of {count} substitutions \
                 ({already} held main's, {read_back} read back), {f:?}"
            ));
        }
        if hash(&line) != main_hash {
            failures.push(format!("{name}: substituted row is not main's row"));
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
    feature_executive_rows_move_only_through_the_read_tolerances: "blend.feature_executive", 1;
    fillet_executive_p3_rows_move_only_through_the_read_tolerances: "blend.fillet_executive_p3", 30;
    fillet_removal_rows_move_only_through_the_read_tolerances: "blend.fillet_removal", 4;
    corpus_parity_rows_move_only_through_the_read_tolerances: "corpus.parity", 26;
    stitch_p1_rows_move_only_through_the_read_tolerances: "heal.stitch_p1", 2;
    stitch_p2_rows_move_only_through_the_read_tolerances: "heal.stitch_p2", 4;
    brep_p10_rows_move_only_through_the_read_tolerances: "topology.brep_p10", 39;
    edgeuse_p3_rows_move_only_through_the_read_tolerances: "topology.edgeuse_p3", 14;
    face_p3_rows_move_only_through_the_read_tolerances: "topology.face_p3", 48;
}

/// Writes, per `unit<TAB>case` line of `$TOLSUB_CASES`, one JSON line to `$TOLSUB_OUT`:
/// the row hash, the encoded row and every read's tolerances.
#[test]
#[ignore = "makes the replay's data; run on main and on this branch"]
fn record() {
    use std::fmt::Write as _;
    let list = std::fs::read_to_string(std::env::var("TOLSUB_CASES").unwrap()).unwrap();
    let mut by_unit: BTreeMap<String, BTreeMap<String, Case>> = BTreeMap::new();
    let mut out = String::new();
    for l in list.lines() {
        let (id, name) = l.split_once('\t').unwrap();
        let u = unit(id);
        let all = by_unit
            .entry(id.to_string())
            .or_insert_with(|| cases(u.as_ref()));
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{id}: no case {name}"));
        state::set(Some(Mode::Record(Vec::new())));
        let line = run(u.as_ref(), case);
        let Some(Mode::Record(snaps)) = state::take() else {
            panic!("mode lost");
        };
        let rec = serde_json::json!({
            "unit": id, "case": name, "hash": hash(&line).to_string(),
            "line": line, "reads": snaps,
        });
        let _ = writeln!(out, "{rec}");
    }
    std::fs::write(std::env::var("TOLSUB_OUT").unwrap(), out).unwrap();
}
