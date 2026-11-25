//! The tolerance-substitution replay of the golden rows the edge/face gap cover moved.
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
//!
//! The WR-493 retirement (found spheres keep the NURB they tested) moves some of these
//! rows too: [`WR493_REFIT`] lists those replayed with the fitted NURB put back as well
//! (step 1 then reproduces main's row bit for bit). Two of them it moves by less than the
//! goldens' comparator ([`WITHIN_COMPARATOR`]: `corpus.parity`'s
//! `Solids__revolution_mold.smb`, `topology.brep_p10`'s `read_Fillet_sphere_plane`): their
//! committed line stays main's, so for them step 2 asserts the run agrees with it under the
//! comparator the goldens gate with (`golden::diff`, the unit's floors), not to the bit.
//!
//! **When a later change moves one of these rows** other than through the read
//! tolerances, the substitution can no longer give main's row; such a row's entry is
//! deleted and the unit's row count lowered. #17896 (exact shared coordinates,
//! `CPoint::affine2`) moves the geometry of the `Fillet/bottle.smb` read itself; it deletes
//! the 19 `blend.fillet_executive_p3` bottle rows whose fillet that read reaches. The read's
//! moved values and those fillets' results are classified in `solidcore`'s
//! `tests/heal_edge_tolerance_covers_edge_face_gap.rs`.
//!
//! #18162's lazy UV trim raise re-blesses `blend.fillet_executive_p3`
//! `stubs4-delete-edge-bottle-e8-r0.1` and `topology.brep_p10` `gen0020` (their edge
//! tolerances, each certified in `certify::trim_tolerance`), so their entries are deleted too.
//!
//! #20939's `edge::set_interval` clears the gaps cached over the old interval, so the UV trim
//! recovery of `create_uv_trim_curves` leaves the gap-cache flags of the
//! `own_reader_open_shell_uv_trim_f<k>_rebuild` rows of `topology.face_p3` at 0 where main's
//! row has 1; their 12 entries are deleted.
//!
//! The kept-trim cover (tangency-labs/solidcraft#19295) raises, after the healer, the
//! tolerance of an edge whose kept UV trim runs farther from it than its tolerance, to a
//! proved bound on that distance, and records each raise it makes with the value before
//! (`Brep::trim_cover_raises`). Every read of a replayed row first has those raises put
//! back. [`TRIM_COVER`] lists the rows only the cover moved, each with the hash of main's
//! encoded row from main's own run of the case ([`record`] on main; a committed golden line
//! can differ from main's run in the last bits, within the goldens' tolerance): with the
//! raises put back (at least one) the row is main's row, and as is it is the committed
//! golden line.
//!
//! The WR-493 retirement moves some of those rows as well. [`TRIM_COVER_REFIT`] lists the
//! ones whose read it moves: they are replayed with the found spheres' fitted NURB put
//! back too. [`TRIM_COVER_WITHIN_COMPARATOR`] lists the ones where it moves only the cover's
//! raises (the cover measures the kept trims on the file's NURB here, on the fitted NURB on
//! main) by less than the goldens' comparator: the committed line stays main's, the run
//! agrees with it under the comparator, and with the fitted NURB put back the run is that
//! line to the bit.
//!
//! `corpus.parity`'s five part-file reads (`Solids/boolean_trees.smp`,
//! `ss_plane_sph1..4.smp`) are not replayed: the part read (`part_file::read_part_from_db`)
//! has no refit seam, so main's read cannot be rebuilt. `ss_plane_sph1..4` move beyond the
//! comparator and are proved with the WR-493 rows (their area enclosure, `solidcore`'s
//! `tests/wr493_class_proofs.rs`); the cover raises nothing on them here, and
//! `boolean_trees.smp` agrees with its committed line (main's) under the comparator
//! ([`part_reads_the_retirement_moves`]).
//!
//! #18166 (the stitch's end-of-operation cover, raised to proved bounds, and the lazy UV
//! trim raise) re-blesses 27 of these rows, each certified in `certify::trim_tolerance`:
//! `topology.brep_mirror`'s 18 `nurb_fillet` mirrors (with the four whose put-back row was
//! off main's run, and the rows only the cover's put-back step reached), `topology.face_p3`'s
//! five `own_reader_sphere_diff_3cylinders_uv_trim` faces, `topology.brep_p2`'s
//! `refine_cci_f6_plane_c_uv` and `refine_cci_f7_plane_b_uv`, `topology.brep_local_operation`'s
//! `fc0006` and `blend.fillet_executive_p3`'s `stubs4-shell_fillet2-e5-r0.5`; their entries
//! are deleted. Four more, `blend.fillet_executive_p3`'s
//! `stubs4-delete-edge-sphere_diff_3cylinders-e1/e4-r0.1/r0.5`, keep main's committed line
//! to the bit, but with the cover's raises put back their edges hold #18166's lazily raised
//! tolerances (lower, each a proved bound) instead of main's, so the put-back row is no
//! longer main's: they are not replayed with the cover's raises put back. They are checked
//! instead as rows that did not move ([`KEPT_MAIN_ROW`]): run as is, each is main's committed
//! line to the bit (its hash, taken from main's expected file, which holds these four lines
//! unchanged) and this branch's committed line. (The hashes their [`TRIM_COVER`] entries held
//! were of an older main's run, not of the line main now commits.)
//!
//! #20179 (the point test asks the face whether to test) adds five UV trims to the read of
//! `Fillet/sphere_diff_3cylinders.smb`, and six `blend.fillet_executive_p3` rows keep them
//! in their result body. Since WR-732's retirement (#18934) the read makes them by
//! implanting the cone face's current trims while it is edited. The put-back runs of every
//! row that reads the file run on main's read of it ([`run_without_classified_trims`]:
//! `state::strip_classified_trims`; the trims are proved in `wr493_rows`'
//! `sphere_diff_3cylinders_rows_move_only_by_the_classified_trims`).
//!
//! #17739's vertex step raises a vertex whose gap is over half its new tolerance to
//! `zone_tol3d(gain * 2 * gap)`. Its raises on these reads were recorded the same way
//! against #18230's healer and carried into [`DATA`]: a listed vertex holds #17739's bits,
//! and a vertex only #17739 moved is listed with #18230's (main's) bits.
//!
//! Twenty rows the cover moved are moved by #17739 too: the 19 `blend.fillet_executive_p3`
//! bottle rows (vertex 22) and `corpus.parity`'s `Fillet__bolt.smb` (vertices 8, 9, 11, 12).
//! They are in [`DATA`] with the cover's main hash: the cover's raises go back first, then
//! those vertices take main's bits, each recorded on both sides with the raises put back.

use std::collections::BTreeMap;

use serde_json::Value;

use super::state::{self, Mode, Sub};
use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, diff, encode, read};
use crate::rng::fnv1a64;
use crate::units::corpus_parity::run_in_process;
use crate::UnitDiff;

mod data;
use data::DATA;
mod trim_cover;
use trim_cover::TRIM_COVER;

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
    encode(&case.name, &run_raw(u, case))
}

/// The case's output before it is encoded.
fn run_raw(u: &dyn UnitDiff, case: &Case) -> crate::Output {
    let out = if u.id() == "corpus.parity" {
        run_in_process(case)
    } else {
        u.run_rust(case)
    };
    out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name))
}

/// [`run`] without the UV trims the read of `Fillet/sphere_diff_3cylinders.smb` makes on
/// its cone face (#20179, made by the implant since #18934): every row with main's read of
/// that file put back (`state::strip_classified_trims`; rows of other files as [`run`]).
/// The row main's run gives, for the put-back runs.
fn run_without_classified_trims(u: &dyn UnitDiff, case: &Case) -> Value {
    state::strip_classified_trims(|| run(u, case)).0
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
            undone: 0,
            failures: Vec::new(),
        }));
        // (a row the WR-493 retirement moves too: before it, found spheres had the fitted
        // NURB, so the replay puts that back as well)
        let refit = WR493_REFIT.contains(&(unit_id, name));
        if refit {
            crate::refit::state::set(Some(crate::refit::state::Refit::default()));
        }
        // (a row the WR-450 retirement moves too: main's run had main's choices at its seams)
        let line =
            crate::wr450_rows::with_mains_choices_for(unit_id, name, || run(u.as_ref(), case));
        if refit {
            let r = crate::refit::state::take().expect("refit lost");
            if r.replaced == 0 || !r.failures.is_empty() {
                failures.push(format!("{name}: refit {r:?}"));
            }
        }
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
            failures.push(format!(
                "{name}: substituted row {:#018x} is not main's row",
                hash(&line)
            ));
        }
        let line = run(u.as_ref(), case);
        let committed = golden.get(name);
        if crate::wr450_rows::below_comparator(unit_id, name)
            && !WITHIN_COMPARATOR.contains(&(unit_id, name))
        {
            // (the WR-450 retirement moves it below the comparator: the committed line is
            // main's, which `wr450_rows` shows the run is with main's choices put back)
            match committed {
                Some(g) if g != &line && diff(g, &line, u.abs_floors()).is_none() => {}
                _ => failures.push(format!(
                    "{name}: the row does not agree with the committed golden line"
                )),
            }
        } else if WITHIN_COMPARATOR.contains(&(unit_id, name)) {
            // (the committed line is main's: the run agrees with it, not to the bit)
            match committed {
                Some(g) if g == &line => {
                    failures.push(format!("{name}: the run is the committed line to the bit"));
                }
                Some(g) if diff(g, &line, u.abs_floors()).is_none() => {}
                _ => failures.push(format!(
                    "{name}: the row does not agree with the committed golden line"
                )),
            }
        } else if committed != Some(&line) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(failures.is_empty(), "{unit_id}:\n{}", failures.join("\n"));
    n
}

/// The rows here that the WR-493 retirement moves as well: replayed with the found
/// spheres' fitted NURB put back too (`crate::refit`).
const WR493_REFIT: &[(&str, &str)] = &[
    ("heal.stitch_p1", "file_revolution_mold.smb_advanced"),
    (
        "heal.stitch_p2",
        "file_revolution_mold.smb_stitchfaces_0.001",
    ),
    (
        "heal.stitch_p2",
        "file_revolution_mold.smb_stitchfaces_0.005",
    ),
    ("corpus.parity", "Solids__revolution_mold.smb"),
    ("topology.brep_p10", "read_Fillet_sphere_plane"),
    ("topology.brep_p10", "read_Solids_revolution_mold"),
    ("topology.edgeuse_p3", "own_reader_sphere_plane_create_eu21"),
    ("topology.edgeuse_p3", "own_reader_sphere_plane_create_eu33"),
    ("topology.edgeuse_p3", "own_reader_sphere_plane_create_eu45"),
    ("topology.edgeuse_p3", "own_reader_sphere_plane_create_eu9"),
];

/// The [`WR493_REFIT`] rows the retirement moves by less than the goldens' comparator:
/// their committed line is main's, which the run agrees with but is not to the bit.
const WITHIN_COMPARATOR: &[(&str, &str)] = &[
    ("corpus.parity", "Solids__revolution_mold.smb"),
    ("topology.brep_p10", "read_Fillet_sphere_plane"),
];

/// Whether `case` of `unit_id` is a [`WITHIN_COMPARATOR`] row.
pub(crate) fn within_comparator(unit_id: &str, case: &str) -> bool {
    WITHIN_COMPARATOR.contains(&(unit_id, case))
}

/// Runs every row of `unit_id` in [`TRIM_COVER`] with the reads' kept-trim cover raises put
/// back (at least one per row) and as is; returns the row count. A row in
/// [`TRIM_COVER_REFIT`] has the found spheres' fitted NURB put back as well for the first
/// run. A row in [`TRIM_COVER_WITHIN_COMPARATOR`] agrees with its committed line under the
/// goldens' comparator but not to the bit, and with the fitted NURB put back (the cover's
/// raises kept) it is that line to the bit.
fn replay_trim_cover(unit_id: &str) -> usize {
    let u = unit(unit_id);
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), unit_id).unwrap();
    let mut failures = Vec::new();
    let mut n = 0;
    let refit_run = |case: &Case, failures: &mut Vec<String>| {
        let (line, replaced) =
            crate::refit::with_fitted_spheres(|| run_without_classified_trims(u.as_ref(), case));
        if replaced == 0 {
            failures.push(format!("{}: no found sphere refit", case.name));
        }
        line
    };
    for &(_, name, main_hash) in TRIM_COVER.iter().filter(|r| r.0 == unit_id) {
        n += 1;
        assert!(
            !DATA.iter().any(|r| r.0 == unit_id && r.1 == name),
            "{unit_id} {name}: replayed with #18230's rows"
        );
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        state::set(Some(Mode::Substitute {
            subs: Vec::new(),
            reads: 0,
            applied: 0,
            already: 0,
            undone: 0,
            failures: Vec::new(),
        }));
        // (a row whose put-back run the joined-trims cover raises further: each raised value
        // put back, it is main's row; see [`TRIM_COVER_JOINED`])
        let joined = TRIM_COVER_JOINED
            .iter()
            .find(|r| r.0 == unit_id && r.1 == name)
            .map(|r| r.2);
        // (a row the WR-493 retirement moves too: main had the fitted NURB; one the WR-450
        // retirement moves too: main's choices at its seams)
        let (line, turned) =
            crate::wr450_rows::with_mains_choices_counting_gate(unit_id, name, || {
                if let Some(raised) = joined {
                    assert!(
                        !TRIM_COVER_REFIT.contains(&(unit_id, name)),
                        "{unit_id} {name}: joined and refit"
                    );
                    let mut out = run_raw(u.as_ref(), case);
                    for (key, count) in put_back_joined(&mut out, raised) {
                        failures.push(format!(
                        "{name}: {count} values {key:e} raised by the joined cover, not as listed"
                    ));
                    }
                    encode(&case.name, &out)
                } else if TRIM_COVER_REFIT.contains(&(unit_id, name)) {
                    refit_run(case, &mut failures)
                } else {
                    run_without_classified_trims(u.as_ref(), case)
                }
            });
        // (a row the carry gate reaches only in this put-back run: it turned a trim away there)
        if crate::wr450_rows::GATE_REPLAY_ONLY.contains(&(unit_id, name)) && turned == 0 {
            failures.push(format!("{name}: the carry gate turned no trim away"));
        }
        let Some(Mode::Substitute {
            reads,
            undone,
            failures: f,
            ..
        }) = state::take()
        else {
            panic!("mode lost");
        };
        if reads == 0 || undone == 0 || !f.is_empty() {
            failures.push(format!("{name}: {reads} reads, {undone} put back, {f:?}"));
        }
        let committed = golden.get(name);
        if hash(&line) != main_hash {
            failures.push(format!(
                "{name}: the row put back is not main's row ({:#018x})",
                hash(&line)
            ));
        }
        let line = run(u.as_ref(), case);
        // (a row the WR-450 retirement moved past the comparator was re-blessed: its
        // committed line is this branch's run, and main's is the put-back above)
        if TRIM_COVER_WITHIN_COMPARATOR.contains(&(unit_id, name))
            && !crate::wr450_rows::moved(unit_id, name)
        {
            // (the committed line is main's: the run agrees with it, not to the bit, and
            // with the fitted NURB put back it is the line)
            match committed {
                Some(g) if g == &line => {
                    failures.push(format!("{name}: the run is the committed line to the bit"));
                }
                Some(g) if diff(g, &line, u.abs_floors()).is_none() => {
                    let refit = refit_run(case, &mut failures);
                    if &refit != g {
                        failures.push(format!(
                            "{name}: with the fitted NURB the row is not the committed line"
                        ));
                    }
                }
                _ => failures.push(format!(
                    "{name}: the row does not agree with the committed golden line"
                )),
            }
        } else if committed != Some(&line) {
            failures.push(format!("{name}: the row is not the committed golden line"));
        }
    }
    assert!(failures.is_empty(), "{unit_id}:\n{}", failures.join("\n"));
    n
}

/// The [`TRIM_COVER`] rows the WR-493 retirement moves too: replayed with the found
/// spheres' fitted NURB put back as well, as main built them.
const TRIM_COVER_REFIT: &[(&str, &str)] = &[
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
    ),
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
    ),
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
    ),
    (
        "blend.fillet_executive_p3",
        "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
    ),
    (
        "blend.fillet_executive_p3",
        "sweep4-sphere_diff_3cylinders-all-r0.1",
    ),
    (
        "blend.fillet_executive_p3",
        "sweep5-sphere_diff_3cylinders-e29-r1.0",
    ),
    ("corpus.parity", "Fillet__sphere_diff_3cylinders.smb"),
    ("corpus.parity", "Fillet__sphere_plane.smb"),
    ("topology.api_files", "write_bytes0085"),
    ("topology.api_files", "write_bytes0115"),
    ("topology.api_files", "write_bytes0179"),
    ("topology.api_files", "write_bytes_file_nurb_fillet_ascii"),
    ("topology.api_files", "write_bytes_file_nurb_fillet_binary"),
    ("topology.brep_p2", "refine_cci_f6_plane_b_nouv"),
    ("topology.brep_p2", "refine_cci_f6_plane_b_uv"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f0"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f10"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f2"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f4"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f6"),
    ("topology.face_p3", "own_reader_sphere_plane_uv_trim_f8"),
];

/// The [`TRIM_COVER`] rows whose cover raises the WR-493 retirement moves by less than the
/// goldens' comparator (the cover measures the kept trims on the file's NURB here, on the
/// fitted NURB on main): their committed line stays main's. (#20179 re-blessed
/// `corpus.parity`'s `Fillet__sphere_diff_3cylinders.smb` and `topology.face_p3`'s
/// `uv_trim_f0`/`f1`/`f4`/`f5`/`f8`/`f9`, which its classified trims move, and #18934
/// re-blessed `uv_trim_f6`, whose read now holds four of the classified read's vertex/face
/// gaps again, computed after the implant set the trims: their put-back above is main's
/// hash, and their committed line is now this branch's run. The WR-450 retirement, #18923,
/// re-blessed those of them in `wr450_rows`' `DATA` the same way: their put-back, with
/// main's choices at its seams, is main's hash, and their committed line is this branch's
/// run. #18934 also moved `topology.brep_cutting`'s `gen0050` apart from the cover, so its
/// [`TRIM_COVER`] entry is deleted; its raises are measured, not certified
/// (`trim_cover.rs`).)
const TRIM_COVER_WITHIN_COMPARATOR: &[(&str, &str)] = &[
    ("blend.fillet_removal", "nurb_fillet_face18"),
    ("heal.stitch_p1", "file_boolean_trees_result.smb_advanced"),
    ("heal.stitch_p1", "file_nurb_fillet.smb_advanced"),
    (
        "heal.stitch_p2",
        "file_boolean_trees_result.smb_stitchfaces_0.001",
    ),
    (
        "heal.stitch_p2",
        "file_boolean_trees_result.smb_stitchfaces_0.005",
    ),
    ("heal.stitch_p2", "file_nurb_fillet.smb_stitchfaces_0.001"),
    ("heal.stitch_p2", "file_nurb_fillet.smb_stitchfaces_0.005"),
    ("topology.brep_mirror", "mat0058"),
    ("topology.brep_p2", "refine_cci_f0_plane_a_nouv"),
    ("topology.brep_p2", "refine_cci_f0_plane_a_uv"),
    ("topology.brep_p2", "refine_cci_f0_plane_b_uv"),
    ("topology.brep_p2", "refine_cci_f4_plane_a_nouv"),
    ("topology.brep_p2", "refine_cci_f4_plane_a_uv"),
    ("topology.brep_p2", "refine_cci_f4_plane_b_uv"),
    ("topology.brep_p2", "refine_cci_f6_plane_b_nouv"),
    ("topology.brep_p2", "refine_cci_f6_plane_b_uv"),
    ("topology.brep_p2", "refine_cci_f8_plane_a_uv"),
    (
        "topology.edgeuse_p3",
        "own_reader_sphere_diff_3cylinders_get_or_create_eu13",
    ),
    (
        "topology.edgeuse_p3",
        "own_reader_sphere_diff_3cylinders_get_or_create_eu19",
    ),
];

/// `corpus.parity`'s part-file reads the retirement moves, which have no refit seam (see
/// the module docs): `ss_plane_sph1..4` read with nothing for the cover to raise and are
/// their committed lines (proved in `solidcore`'s `tests/wr493_class_proofs.rs`), and
/// `Solids__boolean_trees.smp` agrees with its committed line, main's, under the comparator.
#[test]
fn part_reads_the_retirement_moves() {
    let u = unit("corpus.parity");
    let all = cases(u.as_ref());
    let golden = read(&default_dir(), "corpus.parity").unwrap();
    for name in [
        "ss_plane_sph1.smp",
        "ss_plane_sph2.smp",
        "ss_plane_sph3.smp",
        "ss_plane_sph4.smp",
        "Solids__boolean_trees.smp",
    ] {
        let case = &all[name];
        state::set(Some(Mode::Substitute {
            subs: Vec::new(),
            reads: 0,
            applied: 0,
            already: 0,
            undone: 0,
            failures: Vec::new(),
        }));
        let put_back = run(u.as_ref(), case);
        let Some(Mode::Substitute {
            reads,
            undone,
            failures,
            ..
        }) = state::take()
        else {
            panic!("mode lost");
        };
        assert!(
            reads > 0 && failures.is_empty(),
            "{name}: {reads} reads, {failures:?}"
        );
        let line = run(u.as_ref(), case);
        let committed = golden.get(name).expect("a committed line");
        if name.starts_with("ss_plane_sph") {
            assert_eq!(undone, 0, "{name}: the cover raised {undone} tolerances");
            assert_eq!(put_back, line, "{name}");
            assert_eq!(committed, &line, "{name}: not the committed line");
        } else {
            assert!(
                diff(committed, &line, u.abs_floors()).is_none(),
                "{name}: does not agree with the committed line"
            );
        }
    }
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
    fillet_executive_p3_rows_move_only_through_the_read_tolerances: "blend.fillet_executive_p3", 29;
    fillet_removal_rows_move_only_through_the_read_tolerances: "blend.fillet_removal", 4;
    corpus_parity_rows_move_only_through_the_read_tolerances: "corpus.parity", 27;
    stitch_p1_rows_move_only_through_the_read_tolerances: "heal.stitch_p1", 2;
    stitch_p2_rows_move_only_through_the_read_tolerances: "heal.stitch_p2", 4;
    brep_p10_rows_move_only_through_the_read_tolerances: "topology.brep_p10", 38;
    edgeuse_p3_rows_move_only_through_the_read_tolerances: "topology.edgeuse_p3", 14;
    face_p3_rows_move_only_through_the_read_tolerances: "topology.face_p3", 36;
}

macro_rules! replay_trim_cover_tests {
    ($($test:ident: $unit:literal, $rows:literal;)*) => {$(
        #[test]
        fn $test() {
            assert_eq!(replay_trim_cover($unit), $rows);
        }
    )*};
}

replay_trim_cover_tests! {
    blend_feature_executive_rows_move_only_through_the_trim_cover: "blend.feature_executive", 3;
    blend_fillet_executive_p3_rows_move_only_through_the_trim_cover: "blend.fillet_executive_p3", 8;
    blend_fillet_removal_rows_move_only_through_the_trim_cover: "blend.fillet_removal", 20;
    corpus_parity_rows_move_only_through_the_trim_cover: "corpus.parity", 2;
    heal_stitch_p1_rows_move_only_through_the_trim_cover: "heal.stitch_p1", 4;
    heal_stitch_p2_rows_move_only_through_the_trim_cover: "heal.stitch_p2", 8;
    topology_api_files_rows_move_only_through_the_trim_cover: "topology.api_files", 12;
    topology_brep_cutting_rows_move_only_through_the_trim_cover: "topology.brep_cutting", 1;
    topology_brep_local_operation_rows_move_only_through_the_trim_cover: "topology.brep_local_operation", 27;
    topology_brep_mirror_rows_move_only_through_the_trim_cover: "topology.brep_mirror", 2;
    topology_brep_p2_rows_move_only_through_the_trim_cover: "topology.brep_p2", 9;
    topology_brep_p9_rows_move_only_through_the_trim_cover: "topology.brep_p9", 4;
    topology_edgeuse_p3_rows_move_only_through_the_trim_cover: "topology.edgeuse_p3", 16;
    topology_face_p3_rows_move_only_through_the_trim_cover: "topology.face_p3", 13;
}

/// A value the joined-trims cover raises: `(raised, main's, count)` ([`TRIM_COVER_JOINED`]).
type Raised = (f64, f64, usize);

/// [`TRIM_COVER`] rows whose put-back run moves further through the joined-trims cover
/// (tangency-labs/solidcraft#20373, `cover_joined_trims` at the end of the stitch): unit,
/// case, and each value the joined cover raises, as `(raised, main's, count)`: the value it
/// raises to, the value main's row holds there, and how many values of the put-back run's
/// output are the raised one to the bit. The put-back run must hold exactly those counts
/// of each raised value, and with main's values put back in their place encode to the
/// row's [`TRIM_COVER`] main hash. Run as is, each is still the committed golden line.
///
/// `extended_surface_9_Platte`: the put-back takes the read's cover raises off e145 and
/// e151 (1.4508e-5 to 1e-5) and e217 and e219 (4.9059e-5 to 1e-5). The blend's edges e19
/// and e33 then carry trims those uncovered edges leave farther from them than their
/// tolerance (1e-5), and the joined cover raises both to their proved trim bound:
/// e19 to 4.898398292695583e-5 and e33 to 4.898398232973159e-5, and each edge's two
/// vertices with it (`cover_vertex_ends`: a vertex at least its edges' tolerance). In
/// `feature0_geometry` that is six values, each 1e-5 in main's row: entries 235 and 239
/// (e19's vertices) and 390 (e19), 227 and 231 (e33's vertices) and 417 (e33); measured
/// by dumping the raw output with `cover_joined_trims` on and off (off, the row is main's
/// to the bit). As is, the read's cover already covers those trims and the joined cover
/// raises nothing.
const TRIM_COVER_JOINED: &[(&str, &str, &[Raised])] = &[(
    "blend.feature_executive",
    "extended_surface_9_Platte",
    &[
        (4.898398292695583e-5, 1e-5, 3),
        (4.898398232973159e-5, 1e-5, 3),
    ],
)];

/// Puts main's values back in `out` for each raised value of a [`TRIM_COVER_JOINED`] row,
/// matched to the bit in every float array; returns each raised value whose count is not
/// the listed one, with the count found.
fn put_back_joined(out: &mut crate::Output, raised: &[Raised]) -> Vec<(f64, usize)> {
    let mut wrong = Vec::new();
    for &(after, before, count) in raised {
        let mut found = 0;
        for x in out.values.values_mut().flatten() {
            if x.to_bits() == after.to_bits() {
                *x = before;
                found += 1;
            }
        }
        if found != count {
            wrong.push((after, found));
        }
    }
    wrong
}

/// Rows #18166 took out of [`TRIM_COVER`] that it does not move (see the module docs): unit,
/// case, and the hash of main's committed line (`origin/main` ee673a19bf's expected file).
/// #18490 moves these four rows (the round sits on the sphere drop's finer trims), so each
/// hash is of the row with that change, from Linux CI.
/// Each hash is of the line without the `fillet_point_step` key, which recorded the retired
/// step latch (#18916). Two changes move the four rows' `mass_properties` only: the drop
/// tracer's clamped-sliver rule (#21353: a span whose clamped UV image is one point within
/// the drop's tolerances is left out) and WR-732's retirement (#18934: the fillet's own
/// classifications while it edits, by up to 1.7e-10 of the area). So each hash is of main's
/// line without its `mass_properties` key (main's expected file at e97d2814d7; #21353 leaves
/// it unchanged); the moved mass entries are classified against a certified enclosure
/// (`tests/wr493_moved_rows_are_classified`, `tests/drop_clamped_sliver_moved_mass_rows.rs`).
const KEPT_MAIN_ROW: &[(&str, &str, u64)] = &[
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        KEPT_E1_R01,
    ),
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        KEPT_E1_R05,
    ),
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        KEPT_E4_R01,
    ),
    (
        "blend.fillet_executive_p3",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        KEPT_E4_R05,
    ),
];

/// The hashes of [`KEPT_MAIN_ROW`]: main's committed line of each row (e97d2814d7) without
/// its `mass_properties` key.
const KEPT_E1_R01: u64 = 0x977c_63d7_b621_5306;
const KEPT_E1_R05: u64 = 0x2c5d_ffd0_302a_cccf;
const KEPT_E4_R01: u64 = 0x6d3c_42d8_6b25_81f2;
const KEPT_E4_R05: u64 = 0x8557_8f8e_7925_20db;

/// Each [`KEPT_MAIN_ROW`] row, run as is, is this branch's committed line, and without its
/// `mass_properties` key it is main's committed line to the bit.
#[test]
fn rows_the_lazy_raise_keeps_are_main_s() {
    let mut failures = Vec::new();
    for &(unit_id, name, main_hash) in KEPT_MAIN_ROW {
        assert!(
            !TRIM_COVER.iter().any(|r| r.0 == unit_id && r.1 == name)
                && !DATA.iter().any(|r| r.0 == unit_id && r.1 == name),
            "{unit_id} {name}: replayed as well"
        );
        let u = unit(unit_id);
        let all = cases(u.as_ref());
        let golden = read(&default_dir(), unit_id).unwrap();
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let line = run(u.as_ref(), case);
        if golden.get(name) != Some(&line) {
            failures.push(format!("{unit_id} {name}: not the committed line"));
        }
        let mut without_mass = line.clone();
        let removed = without_mass["values"]
            .as_object_mut()
            .and_then(|v| v.remove("mass_properties"));
        if removed.is_none() {
            failures.push(format!("{unit_id} {name}: no mass_properties"));
        }
        if hash(&without_mass) != main_hash {
            failures.push(format!(
                "{unit_id} {name}: not main's line without its mass ({:#018x})",
                hash(&without_mass)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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

/// The rows that read `Fillet/sphere_diff_3cylinders.smb` and move with #20179 (besides
/// `blend.fillet_executive_p3`'s, which `wr493_rows` proves), with main's row hash
/// (`goldens` at the PR's base, 4c275ffb7d, MEASURED). The twelve `topology.face_p3`
/// `_rebuild` rows hold main's row after #18490 moved them (main at 0cfdad69ff: the hash of
/// main's blessed line, MEASURED equal to this branch's run with the trims put back); the eight
/// `uv_trim_f2`, `f3`, `f10` and `f11` rows (each with its `_rebuild`) hold main's blessed line
/// after #20939 cleared their gap-cache flags (main at 1562e5f516). WR-732's retirement
/// (#18934) moves `topology.brep_p10`'s `gen0026` (`drop_uv` 2, healer on) apart from the
/// classified trims (62 of its doubles by up to 1.7e-8 against main's put-back run,
/// MEASURED at e97d2814d7, within the comparator), so its entry is deleted; its moved
/// tolerances stay certified in `certify::trim_tolerance`. The two
/// `uv_trim_f6` rows read the file too but do not move with #20179; they are listed so that
/// the put-back is shown to keep them. `uv_trim_f7_rebuild` holds the run with the drop
/// tracer's clamped-sliver rule, which moves that row's rebuilt face-7 trim alone
/// (`certify::trim_tolerance` certifies its moved tolerance).
const CLASSIFIED_READ_ROWS: &[(&str, &str, u64)] = &[
    (
        "corpus.parity",
        "Fillet__sphere_diff_3cylinders.smb",
        0x0fe4ffda86f0023a,
    ),
    (
        "topology.brep_p10",
        "read_Fillet_sphere_diff_3cylinders",
        0xd5b48ce479ae2618,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f0",
        0x7743ee2b9d99803b,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f0_rebuild",
        0xc4a920b322329987,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f1",
        0xb10d395946aaa1a5,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f10",
        0x913d9dc3e316de70,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f10_rebuild",
        0x42934528e6779089,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f11",
        0x234465c42f5694be,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f11_rebuild",
        0xdec945d4e624bdce,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f1_rebuild",
        0x1e49694bff2783c8,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f2",
        0x287ded99c94c1089,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f2_rebuild",
        0x5d8eae594ea92b9c,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f3",
        0xd02ee7b36ac19415,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f3_rebuild",
        0xcc8e58869406a559,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f4",
        0x8fcb5e3dee081b8f,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f4_rebuild",
        0x5bca12f6769919d2,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f5",
        0x66b68d6a73c6eda2,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f5_rebuild",
        0x09f860420df241b3,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f6",
        0x8212305e228c5d3b,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f6_rebuild",
        0x8e918074b11ab263,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f7",
        0x467d1e66deace095,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f7_rebuild",
        0x50fca59ea3c3d0cc,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f8",
        0xfebe25a2bec0a563,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f8_rebuild",
        0x9857e670ca50f05f,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f9",
        0x78eb4a9ce31b574d,
    ),
    (
        "topology.face_p3",
        "own_reader_sphere_diff_3cylinders_uv_trim_f9_rebuild",
        0xd8bd3415a29dbaa0,
    ),
];

/// #20179: each [`CLASSIFIED_READ_ROWS`] row, run with main's read of the file put back
/// (`state::strip_classified_trims`: the five UV trims the read's face classification now
/// makes taken off, the vertex gaps setting them cleared computed again, and in a healed
/// read the healer's raises on those trims' edges and vertices, each first checked to
/// cover its entity's gaps, back at the zone tolerance), is main's row to the bit: everything else in these
/// rows is the same code giving the same bits, and the rows move only through those trims
/// (which `wr493_rows::sphere_diff_3cylinders_rows_move_only_by_the_classified_trims`
/// certifies within their edges' tolerances).
#[test]
fn classified_read_rows_are_mains_without_the_trims() {
    let mut failures = Vec::new();
    let mut by_unit: BTreeMap<&str, BTreeMap<String, Case>> = BTreeMap::new();
    for &(unit_id, name, main_hash) in CLASSIFIED_READ_ROWS {
        let u = unit(unit_id);
        let all = by_unit.entry(unit_id).or_insert_with(|| cases(u.as_ref()));
        let case = all
            .get(name)
            .unwrap_or_else(|| panic!("{unit_id}: no case {name}"));
        let (line, reads) = state::strip_classified_trims(|| run(u.as_ref(), case));
        if reads == 0 {
            failures.push(format!("{unit_id} {name}: no read put back"));
        }
        if hash(&line) != main_hash {
            failures.push(format!(
                "{unit_id} {name}: not main's row ({:#018x})",
                hash(&line)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
