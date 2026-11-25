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
//! - `copy+trims`: both seams' choices differ somewhere in the row's run (the class is
//!   per row: the hook does not tie a copy to a split, so a copy and a kept trim may be on
//!   different faces); each kept trim that differs from main's, or whose face's surface
//!   differs from main's (an exact copy where main had the analytic), is certified against
//!   the edge's curve on the face's surface as committed ([`kept_trims_are_certified`]), so
//!   the certificate does not rest on the grid's sampled same-parameterisation check;
//! - `trims`: the analytic is kept (it gives the source's points within the tolerance at
//!   every grid point) and the new face keeps the split face's trims instead of dropping
//!   and rebuilding them; each kept trim that differs from main's is certified over its
//!   whole edge ([`kept_trims_are_certified`]), not on the grid.
//!
//! The certificate is the interval bound of `certify::trim_tolerance` on any surface with an
//! exact net: an edge curve on a surface is enclosed through its own UV curve and surface
//! (`crv_on_surf_trim_gap_bound`), and a B-spline trim whose domain ends a few ulps inside
//! the edge's interval (evaluated clamped there) through `clamped_trim_gap_bound`.
//!
//! [`DATA`] was made by [`record`] with main's expected files (`WR450_MAIN`, a directory
//! of main's `expected/*.jsonl` at [`MAIN_COMMIT`]). **When a later change moves one of
//! these rows**, either delete it everywhere it is pinned (its `DATA` entry, its
//! `main.jsonl` line at the same position, its unit's count in `UNITS`, the total in
//! [`data_covers_the_moved_rows`] and any [`WITHIN_RULE`] entry), or record the data
//! again on the main it merges into: `WR450_MAIN=<main's expected/> cargo test -p goldens
//! --lib -- wr450_rows::record --ignored` rewrites `DATA` and `main.jsonl`; then set
//! [`MAIN_COMMIT`] (the recorder does not write it), `UNITS`, the total and
//! [`WITHIN_RULE`] by hand from its output.
//!
//! Retiring WR-495's flag (tangency-labs/solidcraft#21301) moved seven of these rows again
//! (`topology.brep_local_operation`'s fc0000, fc0008, fc0025, gen0011, gen0030, gen0041 and
//! gen0070; the first three were [`UNGATED_LINES`] rows too): they are deleted here by the
//! first rule, and `wr495_rows` holds each to main's facts.
//!
//! The WR-255 retirement (#21272: `sm_FixUVTrimCurves` moves only the trims' end control
//! points) moves `blend.feature_executive`'s `single_face_fillet1019_6` and `_8` under main's
//! choices too, in their UV trims alone: `operand_trim_rows`' replay of both keeps every
//! other value (its blinded digests, which leave out the trims, are unchanged) and certifies
//! each trim on its edge. Their `main.jsonl` lines are recorded again as step 1's run on
//! that retirement, so step 1 holds them bit for bit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use solidcore::surface::surface::replay::{Call, Seam};

use crate::golden::encode;
use crate::golden::{default_dir, diff, read};
use crate::replay_seam::{cases, run, unit, with_hook};
use crate::UnitDiff;

mod data;
use data::DATA;

/// The main commit [`DATA`] was recorded against.
const MAIN_COMMIT: &str = "97c3343fb9";

/// Main's golden line of each row of [`DATA`], in its order (`{"unit": .., "line": ..}`).
const MAIN_LINES: &str = include_str!("wr450_rows/main.jsonl");

/// Main's golden line of `case` of `unit_id`.
fn main_line(unit_id: &str, case: &str) -> Value {
    MAIN_LINES
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["unit"] == unit_id && v["line"]["case"] == case)
        .map_or_else(
            || panic!("{unit_id}: no main line of {case}"),
            |v| v["line"].clone(),
        )
}

/// `text`, an expected file of `unit_id`, with the line of each of its rows of [`DATA`] put
/// back to main's ([`MAIN_LINES`]): for the other ledgers that rebuild main's file from this
/// branch's. [`data_covers_the_moved_rows`] shows those rows are the only lines this retirement
/// moved.
pub(crate) fn with_mains_lines(unit_id: &str, text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let case = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v["case"].as_str().map(str::to_string));
        match case {
            Some(c) if moved(unit_id, &c) => out.push_str(&main_line(unit_id, &c).to_string()),
            // (a row only the carry gate moved: its line before the gate, which is main's
            // since the retirement did not move it)
            Some(c) if carry_gated(unit_id, &c) => {
                out.push_str(&ungated_line(unit_id, &c).to_string());
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
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
                // (main had no carry gate: put back to a carry, uncounted, so a DATA row the
                // gate also moves within the comparator is listed by its class flips only;
                // the rows the gate alone moved are `UNGATED_LINES`)
                Seam::CarryGate => return Ok(vec![0]),
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

/// Whether `case` of `unit_id` is a row of [`DATA`]: a row this retirement moved, whose
/// committed line is this branch's output (re-blessed) unless [`WITHIN_RULE`] lists it.
pub(crate) fn moved(unit_id: &str, case: &str) -> bool {
    DATA.iter().any(|r| r.0 == unit_id && r.1 == case)
}

/// Rows of other ledgers this retirement moves by less than the goldens' comparator, unit
/// and case: the bless keeps their committed line, main's, which the run agrees with but not
/// to the bit (so they are not in [`DATA`]). Each keeps an exact copy where main had the
/// analytic, and with main's choices put back it is its committed line to the bit, or, for a
/// row whose committed line the WR-493 retirement already left within the comparator
/// (`tolsub::replay`'s `WITHIN_COMPARATOR`), agrees with it and is the run
/// `wr493_rows::corpus_parity_rows_are_closer` pins to the bit
/// ([`below_comparator_rows_are_mains_with_mains_choices`]). Listed are those another
/// replay rebuilds main's run of (main's row hash or bits); a row no replay rebuilds needs no
/// entry, since its committed line already holds it.
pub(crate) const BELOW_COMPARATOR: &[(&str, &str)] = &[
    ("corpus.parity", "Fillet__blade_hub_3.smb"),
    ("corpus.parity", "Solids__revolution_mold.smb"),
    // `merged_vertex_rows` (#21340) holds their moved vertex tolerances to the bit: the
    // exact copies move them in the last digits (gen0012's 12.803875079777875 to ...878)
    ("boolean.topology_intersector_p3", "gen0012"),
    ("boolean.topology_intersector_p3", "gen0017"),
    ("boolean.topology_intersector_p3", "gen0165"),
];

/// Whether `case` of `unit_id` is a [`BELOW_COMPARATOR`] row.
pub(crate) fn below_comparator(unit_id: &str, case: &str) -> bool {
    BELOW_COMPARATOR.contains(&(unit_id, case))
}

/// Rows whose run as is this retirement leaves at their committed line to the bit, but whose
/// substituted run, which another replay (`operand_trim_rows`) compares with main's row hash,
/// reaches the two seams: those replays rebuild main's run with main's choices put back, and
/// at least one call's choice differs from this branch's
/// ([`replay_only_rows_reach_the_seams`]).
pub(crate) const REPLAY_ONLY: &[(&str, &str)] = &[
    ("blend.feature_executive", "extended_surface_7_LShape_I"),
    ("topology.brep_p10", "subdivide_cylinder_face0_cinf"),
];

/// Runs `f` with main's choices at the WR-450 seams (the two choices and the carry gate,
/// which main did not have) when `case` of `unit_id` is a row of [`DATA`],
/// [`BELOW_COMPARATOR`], [`REPLAY_ONLY`], [`GATE_REPLAY_ONLY`] or [`UNGATED_LINES`] (a row this retirement moved,
/// or whose replayed run it moved), else as is: for the other replays that rebuild main's
/// run of a row (main's row hash), which since this retirement differs from this branch's
/// through these choices as well (the WR-493 retirement's
/// `wr493_rows::without_classified_trims` precedent). [`replay_unit`],
/// [`below_comparator_rows_are_mains_with_mains_choices`] and
/// [`carry_gate_rows_move_only_through_the_gate`] prove those rows move only through these
/// choices.
pub(crate) fn with_mains_choices_for<R>(unit_id: &str, case: &str, f: impl FnOnce() -> R) -> R {
    with_mains_choices_counting_gate(unit_id, case, f).0
}

/// [`with_mains_choices_for`], also returning how many calls the carry gate turned a trim
/// away in that put back to a carry (0 when the row is run as is).
pub(crate) fn with_mains_choices_counting_gate<R>(
    unit_id: &str,
    case: &str,
    f: impl FnOnce() -> R,
) -> (R, usize) {
    if !moved(unit_id, case)
        && !below_comparator(unit_id, case)
        && !REPLAY_ONLY.contains(&(unit_id, case))
        && !GATE_REPLAY_ONLY.contains(&(unit_id, case))
        && !carry_gated(unit_id, case)
    {
        return (f(), 0);
    }
    let turned = Arc::new(Mutex::new(0_usize));
    let t = Arc::clone(&turned);
    let r = with_hook(
        move |c: Call<'_>| match c.seam {
            Seam::CarryGate => {
                if c.own.as_ref().ok() == Some(&vec![1]) {
                    *t.lock().unwrap() += 1;
                }
                Ok(vec![0])
            }
            Seam::AnalyticCopy => Ok(vec![0]),
            Seam::SplitTrims => Ok(c.input.to_vec()),
        },
        f,
    );
    let turned = *turned.lock().unwrap();
    (r, turned)
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
    brep_local_operation: "topology.brep_local_operation", 29;
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

/// The lines of the rows the carry gate moved ([`Seam::CarryGate`],
/// `copy_uv_trim_curves_of_other_face` turning away the other operand's trim of a copied
/// edge whose same-parameter gap is not proved within the edge's tolerance), as committed
/// before the gate (`{"unit": .., "line": ..}`, the branch's lines at 498fdc76fb).
const UNGATED_LINES: &str = include_str!("wr450_rows/ungated.jsonl");

/// The line before the carry gate of `case` of `unit_id`, a row of [`UNGATED_LINES`].
fn ungated_line(unit_id: &str, case: &str) -> Value {
    UNGATED_LINES
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["unit"] == unit_id && v["line"]["case"] == case)
        .map_or_else(
            || panic!("{unit_id}: no line before the gate of {case}"),
            |v| v["line"].clone(),
        )
}

/// Rows whose run as is the carry gate leaves at their committed line, but whose substituted
/// run in another replay it reaches: `tolsub::replay`'s trim-cover put-back of
/// `extended_surface_9_Platte` takes the read's cover raises off, and the gate then turns away
/// a trim main carried, so the joined cover raises nothing. That replay runs it with main's
/// carry put back ([`with_mains_choices_counting_gate`]) and checks the gate turned at least
/// one trim away there and that the row put back is main's row hash.
pub(crate) const GATE_REPLAY_ONLY: &[(&str, &str)] =
    &[("blend.feature_executive", "extended_surface_9_Platte")];

/// Whether `case` of `unit_id` is a row the carry gate moved ([`UNGATED_LINES`]): the other
/// replays rebuild main's run of it with the gate put back too
/// ([`with_mains_choices_for`]), and rebuild main's file with its line before the gate where
/// the retirement did not move it ([`with_mains_lines`]).
pub(crate) fn carry_gated(unit_id: &str, case: &str) -> bool {
    UNGATED_LINES.lines().any(|l| {
        let v: Value = serde_json::from_str(l).unwrap();
        v["unit"] == unit_id && v["line"]["case"] == case
    })
}

/// Each row the carry gate moved: run with only the gate's choice put back (every trim
/// carried, as before the gate), it is its line before the gate under the goldens' rule
/// ([`diff`] with the unit's floors) and at least one call turned a trim away; run as is,
/// it is its committed golden line. So the gate's choices are the rows' only move, and each
/// trim it turned away was one the gate refused (by the gate's code in
/// `copy_uv_trim_curves_of_other_face`: a same-parameter bound past its edge's tolerance, or
/// none enclosed; the hook sees the choice, not the bound).
#[test]
fn carry_gate_rows_move_only_through_the_gate() {
    let mut failures = Vec::new();
    let mut rows = 0;
    for l in UNGATED_LINES.lines() {
        let v: Value = serde_json::from_str(l).unwrap();
        let unit_id = v["unit"].as_str().unwrap();
        let name = v["line"]["case"].as_str().unwrap();
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let turned = Arc::new(Mutex::new(0_usize));
        let t = Arc::clone(&turned);
        let line = with_hook(
            move |c: Call<'_>| match c.seam {
                Seam::CarryGate => {
                    if c.own.as_ref().ok() == Some(&vec![1]) {
                        *t.lock().unwrap() += 1;
                    }
                    Ok(vec![0])
                }
                _ => c.own,
            },
            || row(u.as_ref(), case),
        );
        if let Some(change) = diff(&v["line"], &line, u.abs_floors()) {
            failures.push(format!(
                "{unit_id} {name}: ungated, the row is not its line before the gate: {change:?}"
            ));
        }
        let turned = *turned.lock().unwrap();
        if turned == 0 {
            failures.push(format!("{unit_id} {name}: the gate turned no trim away"));
        }
        let golden = read(&default_dir(), unit_id).unwrap();
        if golden.get(name) != Some(&row(u.as_ref(), case)) {
            failures.push(format!(
                "{unit_id} {name}: the row is not the committed golden line"
            ));
        }
        eprintln!("{unit_id} {name}: {turned} trims turned away");
        rows += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(rows, 3, "the rows the carry gate moved");
}

/// Each [`BELOW_COMPARATOR`] row: not in [`DATA`]; as run, it agrees with its committed
/// line under the goldens' comparator but not to the bit; with main's choices put back at
/// both seams, at least one call's choice differing from this branch's, it is that line to
/// the bit.
#[test]
fn below_comparator_rows_are_mains_with_mains_choices() {
    let mut failures = Vec::new();
    for &(unit_id, name) in BELOW_COMPARATOR {
        assert!(!moved(unit_id, name), "{unit_id} {name}: in DATA");
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let golden = read(&default_dir(), unit_id).unwrap();
        let committed = golden
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no committed line of {name}"));
        let line = row(u.as_ref(), case);
        if &line == committed {
            failures.push(format!("{unit_id} {name}: the run is the committed line"));
        } else if let Some(change) = diff(committed, &line, u.abs_floors()) {
            failures.push(format!(
                "{unit_id} {name}: beyond the comparator: {change:?}"
            ));
        }
        let (mains, flips) = with_mains_choices(u.as_ref(), case);
        if flips.class() == "none" {
            failures.push(format!(
                "{unit_id} {name}: no call's choice differs from main's"
            ));
        }
        if crate::tolsub::replay::within_comparator(unit_id, name) {
            // (the WR-493 retirement left this committed line within the comparator: with
            // main's choices the run is the one `wr493_rows` pins, agreeing with the line)
            if let Some(change) = diff(committed, &mains, u.abs_floors()) {
                failures.push(format!(
                    "{unit_id} {name}: with main's choices beyond the comparator: {change:?}"
                ));
            }
        } else if &mains != committed {
            failures.push(format!(
                "{unit_id} {name}: with main's choices not the committed line"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Each [`REPLAY_ONLY`] row: not in [`DATA`] or [`BELOW_COMPARATOR`]; as run, it is its
/// committed line to the bit; with main's choices put back, at least one call's choice
/// differs from this branch's (the replays that compare its substituted run with main's
/// row hash then pass only through those choices).
#[test]
fn replay_only_rows_reach_the_seams() {
    let mut failures = Vec::new();
    for &(unit_id, name) in REPLAY_ONLY {
        assert!(!moved(unit_id, name), "{unit_id} {name}: in DATA");
        assert!(
            !below_comparator(unit_id, name),
            "{unit_id} {name}: listed twice"
        );
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let golden = read(&default_dir(), unit_id).unwrap();
        let committed = golden
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no committed line of {name}"));
        if &row(u.as_ref(), case) != committed {
            failures.push(format!(
                "{unit_id} {name}: the run is not the committed line"
            ));
        }
        let (_, flips) = with_mains_choices(u.as_ref(), case);
        if flips.class() == "none" {
            failures.push(format!(
                "{unit_id} {name}: no call's choice differs from main's"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
    assert_eq!(DATA.len(), 395);
    // each (unit, case) once
    let keys: std::collections::BTreeSet<(&str, &str)> = DATA.iter().map(|r| (r.0, r.1)).collect();
    assert_eq!(keys.len(), DATA.len(), "a (unit, case) listed twice");
    // main.jsonl holds main's line for exactly these rows, in order, and every listed
    // row's committed line is there and its text differs from main's. Under the goldens'
    // rule (`diff` with the unit's floors) each moves too, except [`WITHIN_RULE`]: rows
    // whose text the bless rewrote although every value stayed within the rule (replayed
    // like the others).
    let mains: Vec<Value> = MAIN_LINES
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(mains.len(), DATA.len());
    let mut within = Vec::new();
    for (&(unit_id, name, _), m) in DATA.iter().zip(&mains) {
        assert_eq!(
            (m["unit"].as_str(), m["line"]["case"].as_str()),
            (Some(unit_id), Some(name))
        );
        let golden = read(&default_dir(), unit_id).unwrap();
        let now = golden
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id} {name}: no committed line"));
        assert_ne!(now, &m["line"], "{unit_id} {name}: its text did not move");
        if diff(&m["line"], now, unit(unit_id).abs_floors()).is_none() {
            within.push((unit_id, name));
        }
    }
    assert_eq!(within, WITHIN_RULE, "rows within the goldens' rule");
}

/// The listed rows whose text moved but whose values all stay within the goldens' rule
/// (`diff` with the unit's floors), unit and case: [`data_covers_the_moved_rows`] requires
/// exactly these.
const WITHIN_RULE: &[(&str, &str)] = &[];

/// The UV trims of `b`'s edge `e`, one per edge use, as their debug text (`None` where the
/// use keeps none), each with the debug text of the surface of the use's face (`None`
/// where it has none): a trim is this change's where either differs from main's.
fn use_trims(
    b: &solidcore::topology::Brep,
    e: solidcore::topology::EdgeId,
) -> Vec<(Option<String>, Option<String>)> {
    b.edge(e)
        .expect("edge")
        .edgeuses
        .iter()
        .map(|&eu| {
            let trim = b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref();
            let face = solidcore::topology::edgeuse::p1::face(b, eu).expect("face");
            let surface = face.and_then(|f| b.face(f).expect("face").surface.as_ref());
            (
                trim.map(|t| format!("{t:?}")),
                surface.map(|s| format!("{s:?}")),
            )
        })
        .collect()
}

/// Regression (#18923): splitting a cone face at its seam moves edge uses whose trims run
/// past the source surface's natural domain (to `u = 1.125` on `[0, 1]`) onto the new
/// face, whose analytic copy keeps the source's parameterisation on that domain only (it
/// clamps there; 0.765 off the edge). The split drops such trims, so each trim the new
/// face's edges 3 and 4 hold when the healer writes its B-rep is one made again on the
/// copy, certified within the edge's tolerance over the whole edge.
#[test]
fn seam_cone_split_drops_trims_off_the_domain() {
    let k = solidcore::kernel::Kernel::default();
    let u = unit("heal.heal_data_p3");
    let all = cases(u.as_ref());
    let case = all
        .get("split_face_seam_cone_r1_1_h1_s90_q0")
        .expect("the seam cone case");
    crate::certify::watch_written();
    let _ = row(u.as_ref(), case);
    let written = crate::certify::take_written();
    let b = written.first().expect("the healed B-rep");
    let edges = solidcore::topology::brep::p1::edges(b, None).expect("edges");
    let mut checked = 0;
    for &e in edges.iter().filter(|e| [3, 4].contains(&e.index())) {
        let tol = b.edge(e).expect("edge").tolerance;
        // Without the fix the kept trim is 0.765 off its edge: a tolerance below that is
        // what makes this test fail then, so a later raise of it cannot hide the trim.
        assert!(
            tol < 0.765,
            "edge {}: tolerance {tol:e} would cover the kept trim's gap",
            e.index()
        );
        for (eu, bound) in crate::certify::trim_tolerance::trim_bounds_any_surface(&k, b, e) {
            let bound = bound
                .unwrap_or_else(|| panic!("edge {} use {}: not enclosed", e.index(), eu.index()));
            assert!(
                bound <= tol,
                "edge {} use {}: B {bound:e} > tolerance {tol:e}",
                e.index(),
                eu.index()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "edges 3 and 4 of the healed B-rep hold no trim"
    );
    eprintln!("seam_cone_split_drops_trims_off_the_domain: {checked} trims certified");
}

/// The rows whose new face keeps its trims (`trims`, `copy+trims`): each UV trim that this
/// branch's B-reps hold and main's do not (the B-reps each runner writes, run as is and with
/// main's choices, matched by body, edge and edge use: the topology is the same in both)
/// is certified on its face's surface as kept. Its proved bound `B` on the Hausdorff
/// distance between the edge curve and the trim's image
/// ([`crate::certify::trim_tolerance::trim_bounds`]: the kernel's `trim_image_gap_bound`,
/// bisected interval enclosures, rounding carried) is within the edge's tolerance, over the
/// whole edge interval. No point sample decides it. A trim both runs hold the same is not
/// this change's.
#[test]
fn kept_trims_are_certified() {
    let k = solidcore::kernel::Kernel::default();
    let mut failures = Vec::new();
    let mut certified = 0_usize;
    for &(unit_id, name, class) in DATA.iter().filter(|r| r.2 != "copy") {
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        crate::certify::watch_written();
        let _ = row(u.as_ref(), case);
        let ours = crate::certify::take_written();
        crate::certify::watch_written();
        let _ = with_mains_choices(u.as_ref(), case);
        let mains = crate::certify::take_written();
        let at = format!("{unit_id} {name} ({class})");
        if ours.is_empty() || ours.len() != mains.len() {
            failures.push(format!(
                "{at}: B-reps written {} and main's {}",
                ours.len(),
                mains.len()
            ));
            continue;
        }
        let mut row_trims = 0_usize;
        for (bi, (b, m)) in ours.iter().zip(&mains).enumerate() {
            let edges = solidcore::topology::brep::p1::edges(b, None).expect("edges");
            let main_edges = solidcore::topology::brep::p1::edges(m, None).expect("edges");
            if edges.len() != main_edges.len() {
                failures.push(format!(
                    "{at} body {bi}: {} edges, main's {}",
                    edges.len(),
                    main_edges.len()
                ));
                continue;
            }
            for (&e, &me) in edges.iter().zip(&main_edges) {
                let (now, then) = (use_trims(b, e), use_trims(m, me));
                if now.len() != then.len() {
                    failures.push(format!(
                        "{at} body {bi} edge {}: edge uses differ",
                        e.index()
                    ));
                    continue;
                }
                if b.edge(e).expect("edge").curve.is_none() {
                    if now != then {
                        failures.push(format!("{at} body {bi} edge {}: no edge curve", e.index()));
                    }
                    continue;
                }
                let tol = b.edge(e).expect("edge").tolerance;
                let bounds = crate::certify::trim_tolerance::trim_bounds_any_surface(&k, b, e);
                let kept = now.iter().zip(&then).filter(|(n, _)| n.0.is_some());
                for ((n, t), (eu, bound)) in kept.zip(bounds) {
                    if n == t {
                        continue;
                    }
                    row_trims += 1;
                    match bound {
                        Some(x) if x <= tol => certified += 1,
                        Some(x) => failures.push(format!(
                            "{at} body {bi} edge {} use {}: B {x:e} > tolerance {tol:e}",
                            e.index(),
                            eu.index()
                        )),
                        None => failures.push(format!(
                            "{at} body {bi} edge {} use {}: not enclosed",
                            e.index(),
                            eu.index()
                        )),
                    }
                }
            }
        }
        if row_trims == 0 {
            failures.push(format!("{at}: no kept trim differs from main's"));
        }
    }
    eprintln!("kept_trims_are_certified: {certified} trims certified");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
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
        let added_or_removed: Vec<&String> = main
            .keys()
            .filter(|n| !now.contains_key(*n))
            .chain(now.keys().filter(|n| !main.contains_key(*n)))
            .collect();
        assert!(
            added_or_removed.is_empty(),
            "{id}: cases added or removed since main, not rows to replay: {added_or_removed:?}"
        );
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
            writeln!(out, "    ({id:?}, {name:?}, {:?}),", flips.class()).unwrap();
            lines += &serde_json::json!({ "unit": id, "line": line }).to_string();
            lines.push('\n');
        }
    }
    out += "];\n";
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/wr450_rows");
    std::fs::write(dir.join("data.rs"), out).unwrap();
    std::fs::write(dir.join("main.jsonl"), lines).unwrap();
}
