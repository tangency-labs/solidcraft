//! The golden rows that the mass integrator's knot-line change moves, each moved entry
//! classified against a certified enclosure of the body's true value. Under the change a
//! face's trim pieces, and the closure strips along its trims, end where a trim crosses one
//! of its surface's knot lines.
//!
//! The records are estimates:
//! * `blend.fillet_executive_p3`'s `mass_properties`: `brep_mass_properties(k, b, 1e-4,
//!   density 1, origin 0)`;
//! * `corpus.parity`'s `area` and `volume` at `1e-6`, with their round-trip copies
//!   `rt_area` and `rt_volume`.
//!
//! For a moved entry, main's value `o` and this branch's `n` are exact floats, so which is
//! nearer the true value `I` depends only on which side of their midpoint `m` the true value
//! lies: `n` is nearer exactly when `I` is on `n`'s side. Let `[lo, hi]` be a certified
//! enclosure of `I` (`enclose_mass_properties` / `enclose_volume` /
//! `enclose_face_area_relative`: Taylor models in
//! outward-rounded interval arithmetic, their own rounding counted). Bracket `m` by the
//! floats either side of its rounded value ([`side`]). Then:
//! * CLOSER: the whole enclosure is strictly on `n`'s side of `m`;
//! * FARTHER: the whole enclosure is strictly on `o`'s side (a defect, never relabelled);
//! * EQUAL within `w`: the enclosure holds `m`, so neither can be ranked; `w = hi - lo` is
//!   printed next to the move.
//!
//! For each fillet row the test runs the case's Rust side with `goldens::dump::capture` on.
//! It checks that the run reproduces this branch's row (to `values_agree`'s `1e-9`, as a
//! bless keeps a line that agrees), so the captured body is the one the row measures. It then
//! encloses that body, asking each quantity for [`ENCLOSURE_WIDTH`] of its scale; one that
//! spends its cell budget stays a valid, wider enclosure. Finally it classifies every moved
//! entry. Rows with the same rows and the same result geometry are enclosed once. The rows
//! are split into [`PARTS`] tests by body, so they run in parallel. The corpus files are read
//! as `corpus.parity` reads them, and remeasured to reproduce this branch's values. The
//! classes are pinned in [`EXPECTED`] and [`CORPUS_EXPECTED`].
//!
//! Result: 616 moved fillet entries (72 rows) are all EQUAL. Their moves are at most 1.5e-9
//! of their scale, and each enclosure is 8.8e-6 to 1.6e-4 of its scale wide; some bodies'
//! enclosures spend their cell budget and stay valid, wider. Of the 19 moved corpus entries
//! (14 files), `test1.smb`'s volume is CLOSER: its enclosure is 6.7e-11 of the volume wide,
//! against a move of 7.0e-9. 15 are EQUAL; the volume enclosures that spend their budget are
//! up to 0.13 of the volume wide. `vase1.smb`'s area and volume and `vase3.smb`'s volume
//! ([`CORPUS_UNENCLOSED`], moves of 1.1e-16, 1.4e-13 and 3.7e-14) have no certified
//! enclosure, as those bodies have a surface the enclosure has no NURBS form of. No entry is
//! FARTHER.
//!
//! Run (each body's enclosure is a full sweep, so `#[ignore]`d; the goldens job checks the
//! rows themselves):
//! `cargo test --profile goldens -p goldens --test mass_knot_line_moved_rows_are_classified -- --include-ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::float_cmp,
    clippy::too_many_lines
)] // test

use goldens::case::repo_root;
use goldens::compare::values_agree;
use goldens::{registry, CaseContext};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::enclosure::{
    enclose_face_area_relative, enclose_mass_properties, enclose_volume, Bounds,
};
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::PPF_AREA;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel, SolidError};

const UNIT: &str = "blend.fillet_executive_p3";
/// The goldens' default `--seeds`.
const SEEDS: usize = 16;
/// The width asked of each enclosure, relative to its value's scale.
const ENCLOSURE_WIDTH: f64 = 1e-5;
/// The tests the fillet rows are split into, by body.
const PARTS: usize = 4;

/// Where a certified enclosure of the true value lies against two estimates of it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    /// Strictly on the new value's side of their midpoint: the new value is nearer.
    Closer,
    /// Strictly on the old value's side: the new value is farther.
    Farther,
    /// Holding (or touching) the midpoint: neither can be ranked.
    Equal,
}

/// The side of `[lo, hi]` against the midpoint of `o` (old) and `n` (new). `None` when they
/// did not move (`o == n`), for an empty enclosure, or for a value that is not finite (every
/// comparison is written so that NaN fails). The midpoint `(o + n) / 2` is bracketed by the
/// floats either side of its rounded value: the sum rounds by at most half a unit in its last
/// place, and the halving is exact.
fn side(o: f64, n: f64, lo: f64, hi: f64) -> Option<Side> {
    if !(o.is_finite() && n.is_finite() && lo <= hi) || o == n {
        return None;
    }
    let m = (o + n) / 2.0;
    if !m.is_finite() {
        return None;
    }
    let (m_lo, m_hi) = (m.next_down(), m.next_up());
    let (above, below) = (lo > m_hi, hi < m_lo);
    Some(if (n > o && above) || (n < o && below) {
        Side::Closer
    } else if (n > o && below) || (n < o && above) {
        Side::Farther
    } else {
        Side::Equal
    })
}

/// A moved row: its case, main's `mass_properties` and this branch's.
type Row = (&'static str, [f64; 12], [f64; 12]);

include!("mass_knot_line_moved_rows/rows.rs");
include!("mass_knot_line_moved_rows/expected.rs");

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

/// A body enclosed already: its rows (main's, this branch's), its result geometry, and
/// each entry's enclosure and scale.
type Enclosed = ([f64; 12], [f64; 12], Vec<f64>, Vec<Bounds>, Vec<f64>);

/// The part of [`ROWS`] row `index` is classified in: rows with the same old and new rows
/// share one, so a body met twice is enclosed once.
fn part_of(index: usize) -> usize {
    let (_, old, new) = ROWS[index];
    let first = ROWS.iter().position(|r| r.1 == old && r.2 == new).unwrap();
    first % PARTS
}

/// Classifies every moved entry of the rows of [`ROWS`] in `part`: `(case, entry, side)` in
/// order.
fn classify(part: usize) -> Vec<(&'static str, usize, Side)> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let reg = registry();
    let unit = reg.iter().find(|u| u.id() == UNIT).unwrap();
    let cases = unit.cases(&ctx).unwrap();
    let mut classes = Vec::new();
    let mut enclosed: Vec<Enclosed> = Vec::new();
    let (mut rows, mut entries, mut equal) = (0_usize, 0_usize, 0_usize);
    for (index, &(name, old, new)) in ROWS.iter().enumerate() {
        if part_of(index) != part {
            continue;
        }
        rows += 1;
        let case = cases.iter().find(|c| c.name == name).unwrap();
        goldens::dump::capture();
        let out = unit.run_rust(case);
        let captured = goldens::dump::take();
        let out = out.unwrap();
        // A bless keeps a committed line that agrees with the run under `values_agree`
        // (`1e-9` relative), so the run is held to that, and the classes to the committed row.
        let run: Vec<f64> = out.values.get("mass_properties").unwrap().clone();
        assert_eq!(run.len(), 12, "{name}");
        for (i, (&x, &y)) in run.iter().zip(&new).enumerate() {
            assert!(
                values_agree(x, y, None),
                "{name}: the run's {} {x} does not reproduce this branch's row {y}",
                NAMES[i]
            );
        }
        let geometry: Vec<f64> = out.values.get("result_geometry").unwrap().clone();
        let known = enclosed
            .iter()
            .find(|(o, n, g, _, _)| *o == old && *n == new && *g == geometry);
        let (bands, scales) = if let Some((_, _, _, bands, scales)) = known {
            eprintln!("{name}: the body of an earlier row (same rows, same result geometry)");
            (bands.clone(), scales.clone())
        } else {
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
            enclosed.push((old, new, geometry, bands.clone(), scales.clone()));
            (bands, scales)
        };
        for i in 0..12 {
            let (o, n, band) = (old[i], new[i], bands[i]);
            let Some(class) = side(o, n, band.lo, band.hi) else {
                assert_eq!(
                    o.to_bits(),
                    n.to_bits(),
                    "{name}: {} not classified",
                    NAMES[i]
                );
                continue;
            };
            entries += 1;
            if class == Side::Equal {
                equal += 1;
            }
            eprintln!(
                "KNOTLINE {class:?} {name}: {} main {o:e}, now {n:e}, move/scale {:e}, \
                 enclosure [{:e}, {:e}], w/scale {:e}",
                NAMES[i],
                (n - o).abs() / scales[i],
                band.lo,
                band.hi,
                band.width() / scales[i]
            );
            classes.push((name, i, class));
        }
    }
    eprintln!(
        "KNOTLINE SUMMARY part {part}: {rows} rows, {} bodies, {entries} moved entries: \
         {equal} EQUAL",
        enclosed.len()
    );
    classes
}

/// Classifies `part` and checks it against [`EXPECTED`].
fn check(part: usize) {
    let classes = classify(part);
    let expected: Vec<(&str, usize, Side)> = EXPECTED
        .iter()
        .copied()
        .filter(|e| {
            let index = ROWS.iter().position(|r| r.0 == e.0).unwrap();
            part_of(index) == part
        })
        .collect();
    assert!(classes.iter().all(|c| c.2 != Side::Farther), "{classes:#?}");
    assert_eq!(classes, expected, "{classes:#?}");
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_fillet_entries_are_classified_part_0() {
    check(0);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_fillet_entries_are_classified_part_1() {
    check(1);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_fillet_entries_are_classified_part_2() {
    check(2);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_fillet_entries_are_classified_part_3() {
    check(3);
}

/// The pinned table is the moved entries of [`ROWS`], in order, and every row is in a part.
#[test]
fn expected_lists_every_moved_entry() {
    let moved: Vec<(&str, usize)> = ROWS
        .iter()
        .flat_map(|(name, old, new)| {
            (0..12)
                .filter(|&i| old[i].to_bits() != new[i].to_bits())
                .map(move |i| (*name, i))
        })
        .collect();
    let pinned: Vec<(&str, usize)> = EXPECTED.iter().map(|e| (e.0, e.1)).collect();
    assert_eq!(moved, pinned);
    assert!((0..ROWS.len()).all(|i| part_of(i) < PARTS));
}

/// A `corpus.parity` file whose record moved: its path under `pt_TestFiles`, and its area
/// and volume, main's and this branch's (`rt_area` and `rt_volume` moved with them, equal).
type CorpusRow = (&'static str, [f64; 2], [f64; 2]);

/// The moved `corpus.parity` rows; an entry that did not move is NaN. The first two moved
/// past `values_agree`. The rest moved by less, but the tolerance-substitution replay
/// (`goldens::tolsub::replay`) holds their lines exactly, so their lines are rewritten;
/// except `sphere_plane.smb` and `vase3.smb`, whose lines stay main's, which
/// `solidcore`'s `heal_edge_tolerance_covers_edge_face_gap` pins bit for bit.
const CORPUS_ROWS: [CorpusRow; 15] = [
    (
        "Fillet/knife.smb",
        [1892.4391961736515, 2917.0678021580534],
        [1892.439196148405, 2917.0678254893255],
    ),
    (
        "Fillet/test1.smb",
        [f64::NAN, 811.4005999267134],
        [f64::NAN, 811.4006055800013],
    ),
    (
        "Fillet/blade_hub_1.smb",
        [f64::NAN, 13864.897548041092],
        [f64::NAN, 13864.897546682343],
    ),
    (
        "Fillet/blade_hub_2.smb",
        [f64::NAN, 12399.55532408717],
        [f64::NAN, 12399.555323606635],
    ),
    (
        "Fillet/blade_hub_3.smb",
        [f64::NAN, 13013.306377648714],
        [f64::NAN, 13013.30637439093],
    ),
    (
        "Fillet/bottle.smb",
        [f64::NAN, 11.06680127978012],
        [f64::NAN, 11.066801272818358],
    ),
    (
        "Fillet/box_extrusion2.smb",
        [1996.6198693367699, 3800.1138979993243],
        [1996.6198693359593, 3800.1138979990997],
    ),
    (
        "Fillet/cliff_1.smb",
        [f64::NAN, 13864.897548041092],
        [f64::NAN, 13864.897546682343],
    ),
    (
        "Fillet/cliff_2.smb",
        [f64::NAN, 12399.55532408717],
        [f64::NAN, 12399.555323606635],
    ),
    (
        "Fillet/shell_fillet2.smb",
        [89.56453637164611, f64::NAN],
        [89.56453637236255, f64::NAN],
    ),
    (
        "Fillet/sphere_dif_box.smb",
        [700.7752626561011, f64::NAN],
        [700.7752626563098, f64::NAN],
    ),
    (
        "Fillet/sphere_plane.smb",
        [309.82221560471106, f64::NAN],
        [309.8222156047131, f64::NAN],
    ),
    (
        "Fillet/vase1.smb",
        [129231.74333708207, 1103968.529547489],
        [129231.74333708205, 1103968.5295473293],
    ),
    (
        "Fillet/vase3.smb",
        [f64::NAN, 1103584.0777680236],
        [f64::NAN, 1103584.0777679826],
    ),
    (
        "Solids/revolution_mold.smb",
        [19098.629204575405, 100913.14334351463],
        [19098.62920457912, 100913.14334368486],
    ),
];

/// The moved `corpus.parity` entries with no certified enclosure: their bodies have a
/// surface the enclosure has no NURBS form of. Each moved by less than `values_agree`.
const CORPUS_UNENCLOSED: [(&str, &str); 3] = [
    ("Fillet/vase1.smb", "area"),
    ("Fillet/vase1.smb", "volume"),
    ("Fillet/vase3.smb", "volume"),
];

/// `corpus.parity`'s integration accuracy.
const CORPUS_ACCURACY: f64 = 1e-6;
/// The volume enclosure's width, relative to the volume: narrow enough to rank the moves
/// (8.0e-9 and 7.0e-9 of the volume) where the enclosure reaches it.
const CORPUS_VOLUME_WIDTH: f64 = 1e-10;

/// Each moved `corpus.parity` entry, read and measured as that unit measures it. Each run
/// reproduces this branch's value, and each entry is classified against a certified
/// enclosure. The area is `corpus.parity`'s per-face sum. The volume is `brep_volume`, whose
/// gap-closure strips the change splits at the knot-line crossings too.
#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_corpus_entries_are_classified() {
    let mut classes = Vec::new();
    let mut unenclosed = Vec::new();
    for (path, old, new) in CORPUS_ROWS {
        let mut k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        let mut records = NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let notify = solidcore::topology::loopuse::notify_ref;
        let key = |_| ObjectKey(0);
        let mut env = TopologyEnv {
            all_attributes: &[],
            attributes: &mut attributes,
            key_of: &notify,
            records: &mut records,
            next_key: 1 << 50,
        };
        let data = std::fs::read(
            repo_root()
                .join("upstream/TestFiles/pt_TestFiles")
                .join(path),
        )
        .unwrap();
        read_from_file(&mut k, &mut b, data, FileType::Ascii, false, &mut env).unwrap();
        // corpus.parity's measures, in its order: the area, then the volume
        let mut area = 0.0;
        let faces = bl::faces(&b, None).unwrap();
        for &f in &faces {
            let bbox =
                solidcore::query::face::p1::calculate_bounding_box(&k, &b, f, false).unwrap();
            let s = bbox.size();
            let d = s.x.max(s.y).max(s.z);
            let p = face_properties(
                &k,
                &mut b,
                f,
                OrientType::Same,
                CORPUS_ACCURACY,
                bbox.mid(),
                d * d / 100.0,
                0.0,
                PPF_AREA,
            )
            .unwrap();
            area += p.area.abs();
        }
        let store = AttributeStore::new();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &key,
        };
        let volume = brep_volume(&k, &mut b, CORPUS_ACCURACY, &attrs).unwrap();
        for (what, run, o, n) in [
            ("area", area, old[0], new[0]),
            ("volume", volume, old[1], new[1]),
        ] {
            if o.is_nan() {
                continue;
            }
            assert!(
                values_agree(run, n, None),
                "{path}: the run's {what} {run} does not reproduce this branch's {n}"
            );
            // the area face by face, as `corpus.parity` sums each face's magnitude (a sheet's
            // face, such as `shell_fillet2.smb`'s, counts from its upward use, so its
            // enclosure may be its area negated), the sum rounded outward
            let band = if what == "area" {
                let mut sum = Ok((0.0_f64, 0.0_f64));
                for &f in &faces {
                    let Ok((lo, hi)) = sum else { break };
                    sum = enclose_face_area_relative(&k, &mut b, &[f], ENCLOSURE_WIDTH).map(|e| {
                        let (flo, fhi) = if e.hi <= 0.0 {
                            (-e.hi, -e.lo)
                        } else {
                            assert!(e.lo >= 0.0, "{path} {f:?}: [{}, {}] holds 0", e.lo, e.hi);
                            (e.lo, e.hi)
                        };
                        ((lo + flo).next_down(), (hi + fhi).next_up())
                    });
                }
                sum.map(|(lo, hi)| Bounds { lo, hi })
            } else {
                enclose_volume(&k, &mut b, CORPUS_VOLUME_WIDTH * n.abs()).map(|e| {
                    eprintln!("{path}: the volume enclosure stopped at {:?}", e.stop);
                    Bounds { lo: e.lo, hi: e.hi }
                })
            };
            // a surface the enclosure has no NURBS form of (an offset surface, say) has no
            // certified enclosure: the entry is listed as unenclosed, with its move
            let band = match band {
                Err(SolidError::Unsupported { reason, .. }) => {
                    eprintln!(
                        "KNOTLINE UNENCLOSED {path}: {what} main {o:e}, now {n:e}, move/value \
                         {:e}: {reason}",
                        (n - o).abs() / n.abs()
                    );
                    unenclosed.push((path, what));
                    continue;
                }
                band => band.unwrap(),
            };
            let class = side(o, n, band.lo, band.hi).unwrap();
            eprintln!(
                "KNOTLINE {class:?} {path}: {what} main {o:e}, now {n:e}, move/value {:e}, \
                 enclosure [{:e}, {:e}], w/value {:e}",
                (n - o).abs() / n.abs(),
                band.lo,
                band.hi,
                band.width() / n.abs()
            );
            classes.push((path, what, class));
        }
    }
    assert_eq!(unenclosed, CORPUS_UNENCLOSED.to_vec(), "{unenclosed:#?}");
    assert!(classes.iter().all(|c| c.2 != Side::Farther), "{classes:#?}");
    assert_eq!(classes, CORPUS_EXPECTED.to_vec(), "{classes:#?}");
}

#[test]
fn side_ranks_only_an_enclosure_past_the_midpoint() {
    // n above o: the enclosure above the midpoint 1.5 is CLOSER, below it FARTHER
    assert_eq!(side(1.0, 2.0, 1.6, 1.7), Some(Side::Closer));
    assert_eq!(side(1.0, 2.0, 1.3, 1.4), Some(Side::Farther));
    // n below o: mirrored
    assert_eq!(side(2.0, 1.0, 1.3, 1.4), Some(Side::Closer));
    assert_eq!(side(2.0, 1.0, 1.6, 1.7), Some(Side::Farther));
    // holding or touching the midpoint, or within a float of it: EQUAL
    assert_eq!(side(1.0, 2.0, 1.4, 1.6), Some(Side::Equal));
    assert_eq!(side(1.0, 2.0, 1.5, 1.7), Some(Side::Equal));
    assert_eq!(side(1.0, 2.0, 1.5_f64.next_up(), 1.7), Some(Side::Equal));
    assert_eq!(
        side(1.0, 2.0, 1.5_f64.next_up().next_up(), 1.7),
        Some(Side::Closer)
    );
    assert_eq!(side(1.0, 2.0, 1.3, 1.5_f64.next_down()), Some(Side::Equal));
    // a point enclosure exactly at the midpoint
    assert_eq!(side(1.0, 2.0, 1.5, 1.5), Some(Side::Equal));
    // not moved, an empty enclosure, NaN or infinity: no class
    assert_eq!(side(1.0, 1.0, 0.0, 2.0), None);
    assert_eq!(side(1.0, 2.0, 1.7, 1.6), None);
    assert_eq!(side(1.0, 2.0, f64::NAN, 1.6), None);
    assert_eq!(side(1.0, 2.0, 1.6, f64::NAN), None);
    assert_eq!(side(f64::NAN, 2.0, 1.6, 1.7), None);
    assert_eq!(side(1.0, f64::INFINITY, 1.6, 1.7), None);
    // huge and tiny magnitudes, one float apart
    let big = 1e300_f64;
    assert_eq!(
        side(big, big.next_up(), big.next_up(), big.next_up()),
        Some(Side::Equal)
    );
    let (o, n) = (1e300_f64, 1.5e300);
    assert_eq!(side(o, n, 1.4e300, 1.45e300), Some(Side::Closer));
    let (o, n) = (-3e-300_f64, -2e-300);
    assert_eq!(side(o, n, -2.1e-300, -2.05e-300), Some(Side::Closer));
    assert_eq!(side(o, n, -2.9e-300, -2.8e-300), Some(Side::Farther));
}

/// A CLOSER entry turns FARTHER when its new value is moved past the midpoint, to
/// `o + 2 (o - n)` on the old side. An EQUAL enclosure shrunk to a point off the midpoint is
/// ranked. So the classes come from the values, not from a catch-all.
#[test]
fn side_flips_when_the_values_or_the_enclosure_move() {
    let (o, n, lo, hi) = (1.0, 2.0, 1.6, 1.7);
    assert_eq!(side(o, n, lo, hi), Some(Side::Closer));
    let flipped = o + 2.0 * (o - n);
    assert_eq!(side(o, flipped, lo, hi), Some(Side::Farther));
    let (lo, hi) = (1.2, 1.9);
    assert_eq!(side(o, n, lo, hi), Some(Side::Equal));
    let point = (lo + hi) / 2.0;
    assert_eq!(side(o, n, point, point), Some(Side::Closer));
}
