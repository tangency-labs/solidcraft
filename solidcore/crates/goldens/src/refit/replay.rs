//! The refit replay of the golden rows the WR-493 retirement moves.
//!
//! A sphere found in a file NURB now keeps that NURB, so the file's UV trims stay on it.
//! Main replaces it by upstream's NURB made from the fitted circles, on which the read
//! drops the file's trims and rebuilds them. For each moved row, [`DATA`] holds the FNV-1a
//! hash of main's committed row.
//!
//! The tests run each case twice:
//! 1. with every found sphere's file NURB replaced by the fitted one before the topology
//!    is made ([`super::before_topology`]; at least one replaced, on a read the seam sees),
//!    the encoded row hashes to main's row;
//! 2. as is, the encoded row equals the committed golden line.
//!
//! So each committed row is this branch's output, and it differs from main's only through
//! the NURB the found spheres keep.
//!
//! #20179 (the point test asks the face whether to test) adds five UV trims to the result
//! body of six `sphere_diff_3cylinders` rows; step 1 hashes those rows without them
//! ([`run_without_classified_trims`]: the five edges' trim counts lowered, which
//! `wr493_rows`' `sphere_diff_3cylinders_rows_move_only_by_the_classified_trims` proves is
//! the body's record without those trims, each certified on its edge). `corpus.parity` runs its two sides in this process
//! (`run_in_process`) so the seam sees their reads.
//!
//! [`DATA`] was made by [`record`] from main's expected rows (`REFIT_OLD`, its `expected`
//! directory): every case whose committed row differs from main's. A row the retirement
//! moves by less than the goldens' comparator keeps main's committed line, so it is not
//! listed (its run is not that line to the bit). Of `blend.fillet_executive_p3`'s moved rows
//! the ten `sphere_diff_3cylinders` rows and `stubs4-sphere_plane-e3-r0.5` are listed
//! (`REFIT_UNITS` that unit alone, these rows kept). Its other moved row,
//! `source-file-box_w_slot-all-edges`, is not listed: there the seam rebuilds main's run of
//! the case bit for bit (measured on main at 0b321cbaa4), but main's committed line is not
//! main's own run to the bit (`mass_properties` up to 1.7e-10 relative off, within the
//! goldens' comparator). It is held entity by entity against the seam build instead, its
//! record checked against main's (`wr493_rows`'
//! `box_w_slot_geometry_moves_only_on_the_found_spheres`, `wr493_certificates`).
//!
//! Tori: the read's recognisers (`CopyAndAddAnalytics`) find no torus, so the seam has none
//! to replace. Tori (and spheres) are found later only by the fillet's exact-round
//! recognisers (`blend::fillet_exact`) and the implicit support recognisers
//! (`surface::implicit`); a probe on main with those keeping the tested NURB left every
//! `blend.fillet_executive_p3` row within the comparator and the records of
//! `source-file-box_w_slot-all-edges` and `stubs4-sphere_plane-e3-r0.5` bit for bit
//! (measured at 0b321cbaa4, not replayed here).

use std::collections::BTreeMap;

use serde_json::Value;

use super::state::{self, Refit};
use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::output::Output;
use crate::rng::fnv1a64;
use crate::units::corpus_parity::run_in_process;
use crate::UnitDiff;

mod data;
use data::DATA;

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// One moved row: unit, case and main's row hash.
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
    encode(&case.name, &output(u, case))
}

fn output(u: &dyn UnitDiff, case: &Case) -> Output {
    let out = if u.id() == "corpus.parity" {
        run_in_process(case)
    } else {
        u.run_rust(case)
    };
    out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name))
}

/// [`run`] without the UV trims #20179's face classification adds to a row's result body
/// (`wr493_rows::without_classified_trims`, which proves them; other rows as [`run`]): the
/// row main's run gives, for the runs held to main's.
pub(crate) fn run_without_classified_trims(u: &dyn UnitDiff, case: &Case) -> Value {
    let mut out = output(u, case);
    crate::wr493_rows::without_classified_trims(u.id(), &case.name, &mut out);
    encode(&case.name, &out)
}

fn hash(v: &Value) -> u64 {
    fnv1a64(&serde_json::to_string(v).unwrap())
}

/// `run` with the refit installed: the row and what the seam saw.
pub(crate) fn run_refit(u: &dyn UnitDiff, case: &Case) -> (Value, Refit) {
    state::set(Some(Refit::default()));
    let line = run_without_classified_trims(u, case);
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
            failures.push(format!(
                "{name}: refit row is not main's row ({:#018x})",
                hash(&line)
            ));
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
    brep_p10_rows_move_only_through_the_kept_nurb: "topology.brep_p10", 5;
    brep_p4_rows_move_only_through_the_kept_nurb: "topology.brep_p4", 5;
    edge_rows_move_only_through_the_kept_nurb: "topology.edge", 32;
    edgeuse_p1_rows_move_only_through_the_kept_nurb: "topology.edgeuse_p1", 49;
    edgeuse_p3_rows_move_only_through_the_kept_nurb: "topology.edgeuse_p3", 31;
    face_p2_rows_move_only_through_the_kept_nurb: "topology.face_p2", 4;
    face_p3_rows_move_only_through_the_kept_nurb: "topology.face_p3", 24;
    face_p4_rows_move_only_through_the_kept_nurb: "topology.face_p4", 30;
    vertexuse_rows_move_only_through_the_kept_nurb: "topology.vertexuse", 4;
    edgeuse_p2_rows_move_only_through_the_kept_nurb: "topology.edgeuse_p2", 8;
    vertex_rows_move_only_through_the_kept_nurb: "topology.vertex", 1;
    standard_solver_p1_rows_move_only_through_the_kept_nurb: "blend.fillet_standard_solver_p1", 1;
    fillet_executive_p3_rows_move_only_through_the_kept_nurb: "blend.fillet_executive_p3", 11;
}

/// `topology.vertex` `surf_sphere150_rebuild_v3_gaps` (`max_up_dim_gap_3d` with its list):
/// main's row and this branch's list the same gaps, every one of length 0, and differ only
/// in which of them is reported as the largest (main a face gap held by vertex use 1, now
/// an edge gap held by vertex use 2). Both are a largest gap of that list; this asserts the
/// reported gap is one the list holds and that no listed gap is longer, so the row's answer
/// is a maximum by construction.
#[test]
fn surf_sphere150_rebuild_v3_gaps_reports_a_largest_listed_gap() {
    let u = unit("topology.vertex");
    let all = cases(u.as_ref());
    let case = &all["surf_sphere150_rebuild_v3_gaps"];
    let out = u.run_rust(case).unwrap();
    let s = &out.values["state"];
    // [status, max kind, max holder, max length, list length, (kind, holder, length)...]
    assert_eq!(s[0], 0.0);
    let max = (s[1], s[2], s[3]);
    let n = s[4] as usize;
    let list: Vec<(f64, f64, f64)> = (0..n)
        .map(|g| (s[5 + 3 * g], s[6 + 3 * g], s[7 + 3 * g]))
        .collect();
    assert_eq!(max, (0.0, 2.0, 0.0), "{s:?}");
    assert!(list.contains(&max), "{max:?} not in {list:?}");
    assert!(list.iter().all(|g| g.2 <= max.2), "{list:?}");
    // main's answer, the face gap held by vertex use 1, is listed with the same length
    assert!(list.contains(&(1.0, 1.0, 0.0)), "{list:?}");
}

/// Writes `$REFIT_OUT`, the source of [`DATA`]: per unit below, every case whose
/// committed row differs from its row in `$REFIT_OLD` (main's `expected` directory),
/// with the hash of that old row; and a line per case of what the refit run gives.
#[test]
#[ignore = "makes the replay's data from main's expected rows"]
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
