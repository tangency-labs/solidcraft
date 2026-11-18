//! The `blend.fillet_executive_p3` rows whose `mass_properties` the WR-493 retirement moves
//! (the found sphere keeps the NURB it tested, not upstream's fitted one), each moved entry
//! classified against a certified enclosure of the body's true value.
//!
//! The old rows are main's; the new are this branch's. Both are estimates (`brep_mass_properties(k, b, 1e-4, density 1, origin
//! 0)`), and exact floats, so which is nearer the true value depends only on which side of
//! their midpoint the true value lies (`harness::side`). For each row the test runs the
//! case with `goldens::dump::capture` on, checks that the run reproduces the new row (to
//! `values_agree`'s `1e-9`, as a bless keeps a line that agrees), encloses the captured body
//! (`enclose_mass_properties`, asking [`ENCLOSURE_WIDTH`] of each quantity's scale; one that
//! spends its cell budget stays a valid, wider enclosure) and classifies each moved entry.
//! The classes are pinned in [`EXPECTED`]; a FARTHER entry would be pinned as FARTHER and
//! listed as open, never relabelled. [`WIDE`] lists the entries whose enclosure is too wide
//! for EQUAL to bound the new value's error within the rows' accuracy.
//!
//! Run (each body's enclosure is a full sweep, so `#[ignore]`d; the goldens job checks the
//! rows themselves):
//! `cargo test --profile goldens -p goldens --test wr493_moved_rows_are_classified -- --include-ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::float_cmp
)] // test

#[path = "moved_mass_rows/harness.rs"]
mod harness;
use harness::{Row, Side, Table};

include!("wr493_moved_rows/rows.rs");
include!("wr493_moved_rows/expected.rs");
include!("wr493_moved_rows/beyond_rounding.rs");

/// The width asked of each enclosure, relative to its value's scale.
const ENCLOSURE_WIDTH: f64 = 1e-5;
/// The tests the rows are split into, by body.
const PARTS: usize = 3;

/// The moved rows and how they are classified.
const TABLE: Table = Table {
    unit: "blend.fillet_executive_p3",
    tag: "WR493",
    rows: &ROWS,
    parts: PARTS,
    width: ENCLOSURE_WIDTH,
    // (`brep_mass_properties` at `1e-4`, relative to each value's scale)
    accuracy: 1e-4,
};

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_0() {
    TABLE.check(0, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_1() {
    TABLE.check(1, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_2() {
    TABLE.check(2, &EXPECTED, &WIDE);
}

/// The pinned table is the moved entries of [`ROWS`], in order, and every row is in a part.
#[test]
fn expected_lists_every_moved_entry() {
    let pinned: Vec<(&str, usize)> = EXPECTED.iter().map(|e| (e.0, e.1)).collect();
    assert_eq!(TABLE.moved(), pinned);
    assert!((0..ROWS.len()).all(|i| TABLE.part_of(i) < PARTS));
}

/// Every entry main's row lies beyond this branch's summation-rounding bands for
/// (`goldens::wr493_rows`) is pinned EQUAL here and is not [`WIDE`]: its new value is within
/// the rows' accuracy of the certified enclosure, which the ignored parts check.
#[test]
fn beyond_rounding_entries_are_equal_within_accuracy() {
    for &(case, i) in &BEYOND_ROUNDING {
        assert!(
            EXPECTED.contains(&(case, i, Side::Equal)),
            "{case} {i}: not pinned EQUAL"
        );
        assert!(!WIDE.contains(&(case, i)), "{case} {i}: WIDE");
    }
}
