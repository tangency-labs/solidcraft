//! The `blend.fillet_executive_p3` rows whose `mass_properties` the WR-493 retirement moved
//! (the found sphere keeps the NURB it tested, not upstream's fitted one): each entry is
//! within the rows' accuracy of a certified enclosure of the body's true value.
//!
//! Each row is an estimate (`brep_mass_properties(k, b, 1e-4, density 1, origin 0)`). For
//! each row the test runs the case with `goldens::dump::capture` on, checks that the run
//! reproduces the row (to `values_agree`'s `1e-9`, as a bless keeps a line that agrees),
//! encloses the captured body (`enclose_mass_properties`, asking [`ENCLOSURE_WIDTH`] of each
//! quantity's scale; one that spends its cell budget stays a valid, wider enclosure) and
//! checks each entry `n` against its enclosure `[lo, hi]`: the farthest the true value can
//! be, `max(n - lo, hi - n)`, is at most the rows' accuracy times the entry's scale (the
//! area, the volume, `R` the box's half diagonal for a centroid coordinate, `V R^2` for a
//! moment). [`WIDE`] pins the entries where it is not, as measured.
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

use goldens::case::repo_root;
use goldens::compare::values_agree;
use goldens::{registry, CaseContext};
use solidcore::mass::enclosure::enclose_mass_properties;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::Kernel;

/// A row: its case and its `mass_properties`.
type Row = (&'static str, [f64; 12]);

include!("wr493_moved_rows/rows.rs");

/// The entries whose enclosure leaves the farthest the true value can be above the rows'
/// accuracy: (case, entry), as measured: the inertia y and z of `box_w_slot` (farthest
/// 2.1e-4 and 2.5e-4 of `V R^2`; its volume enclosure spends its cell budget).
static WIDE: [(&str, usize); 2] = [
    ("source-file-box_w_slot-all-edges", 7),
    ("source-file-box_w_slot-all-edges", 8),
];

/// The unit whose rows moved.
const UNIT: &str = "blend.fillet_executive_p3";
/// The width asked of each enclosure, relative to its value's scale.
const ENCLOSURE_WIDTH: f64 = 1e-5;
/// The rows' integration accuracy, relative to each value's scale.
const ACCURACY: f64 = 1e-4;
/// The goldens' default `--seeds`.
const SEEDS: usize = 16;
/// The tests the rows are split into, by body.
const PARTS: usize = 3;

/// The names of a record's entries.
const NAMES: [&str; 12] = [
    "area",
    "volume",
    "mass",
    "centroid x",
    "centroid y",
    "centroid z",
    "inertia x",
    "inertia y",
    "inertia z",
    "product yz",
    "product zx",
    "product xy",
];

/// The part row `index` is checked in: rows with the same values share one, so a body met
/// twice is enclosed once.
fn part_of(index: usize) -> usize {
    let row = ROWS[index].1;
    ROWS.iter().position(|r| r.1 == row).unwrap() % PARTS
}

/// Checks every entry of the rows in `part` against a certified enclosure of the body's
/// true value; the entries over the rows' accuracy must be those of [`WIDE`] in the part.
fn check(part: usize) {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let reg = registry();
    let unit = reg.iter().find(|u| u.id() == UNIT).unwrap();
    let cases = unit.cases(&ctx).unwrap();
    // (row values, result geometry, each entry's farthest error over its scale)
    let mut enclosed: Vec<([f64; 12], Vec<f64>, Vec<f64>)> = Vec::new();
    let mut wide = Vec::new();
    let mut entries = 0;
    for (index, &(name, row)) in ROWS.iter().enumerate() {
        if part_of(index) != part {
            continue;
        }
        let case = cases.iter().find(|c| c.name == name).unwrap();
        goldens::dump::capture();
        let out = unit.run_rust(case);
        let captured = goldens::dump::take();
        let out = out.unwrap();
        let run: Vec<f64> = out.values.get("mass_properties").unwrap().clone();
        assert_eq!(run.len(), 12, "{name}");
        for (i, (&x, &y)) in run.iter().zip(&row).enumerate() {
            assert!(
                values_agree(x, y, None),
                "{name}: the run's {} {x} does not reproduce the row's {y}",
                NAMES[i]
            );
        }
        let geometry: Vec<f64> = out.values.get("result_geometry").unwrap().clone();
        let known = enclosed
            .iter()
            .find(|(r, g, _)| *r == row && *g == geometry)
            .map(|(_, _, far)| far.clone());
        let far = known.unwrap_or_else(|| {
            let mut bodies = captured.into_iter().filter(|c| c.label == "mass");
            let body = bodies.next().unwrap().brep.unwrap();
            assert!(bodies.next().is_none(), "{name}: more than one mass body");
            let k = Kernel::default();
            let mut b = body.clone_standing_for_self().unwrap();
            let r = 0.5
                * calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default())
                    .unwrap()
                    .size()
                    .length();
            let e = enclose_mass_properties(&k, &mut b, ENCLOSURE_WIDTH).unwrap();
            if let Some((q, stop)) = e.short {
                eprintln!("{name}: the {q:?} enclosure stopped at {stop:?}, its band wider");
            }
            let v = e.volume.hi;
            let vr2 = v * r * r;
            let mut bands = vec![e.area, e.volume, e.volume];
            let mut scales = vec![e.area.hi, v, v];
            for (group, scale) in [e.centroid, e.inertia, e.products]
                .into_iter()
                .zip([r, vr2, vr2])
            {
                bands.extend(group);
                scales.extend([scale; 3]);
            }
            let far: Vec<f64> = (0..12)
                .map(|i| {
                    let (n, band) = (row[i], bands[i]);
                    // (every comparison written so that a NaN or an empty band fails)
                    if n.is_finite() && band.lo <= band.hi && scales[i].is_finite() {
                        (n - band.lo).max(band.hi - n) / scales[i]
                    } else {
                        f64::INFINITY
                    }
                })
                .collect();
            enclosed.push((row, geometry, far.clone()));
            far
        });
        for (i, &f) in far.iter().enumerate() {
            entries += 1;
            eprintln!("WR493 {name}: {} farthest/scale {f:e}", NAMES[i]);
            if f.is_nan() || f > ACCURACY {
                eprintln!("WR493 WIDE {name}: {}", NAMES[i]);
                wide.push((name, i));
            }
        }
    }
    eprintln!(
        "WR493 SUMMARY part {part}: {} bodies, {entries} entries, {} wide",
        enclosed.len(),
        wide.len()
    );
    let pinned: Vec<(&str, usize)> = WIDE
        .iter()
        .copied()
        .filter(|e| part_of(ROWS.iter().position(|r| r.0 == e.0).unwrap()) == part)
        .collect();
    assert_eq!(wide, pinned, "{wide:#?}");
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_0() {
    check(0);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_1() {
    check(1);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn wr493_moved_entries_are_classified_part_2() {
    check(2);
}

/// Every [`WIDE`] entry is an entry of a row of [`ROWS`], and every row is in a part.
#[test]
fn wide_entries_are_rows_entries() {
    for &(case, i) in &WIDE {
        assert!(ROWS.iter().any(|r| r.0 == case) && i < 12, "{case} {i}");
    }
    assert!((0..ROWS.len()).all(|i| part_of(i) < PARTS));
}
