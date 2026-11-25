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
//! EQUAL ranks nothing, but bounds the new value's error by `|n - o| / 2 + w`. Each test
//! also checks that bound against the accuracy the value claims (`within_accuracy`), and
//! lists the entries whose enclosure is too wide for it ([`WIDE`], [`CORPUS_WIDE`]).
//!
//! Result: 617 moved fillet entries (72 rows) are all EQUAL. Their moves are at most 1.5e-9
//! of their scale, and each enclosure is 8.8e-6 to 1.6e-4 of its scale wide; some bodies'
//! enclosures spend their cell budget and stay valid, wider (41 entries past the rows'
//! accuracy). Of the 19 moved corpus entries (14 files), `test1.smb`'s volume is CLOSER: its
//! enclosure is 6.7e-11 of the volume wide, against a move of 7.0e-9. The other areas,
//! enclosed face by face at 1e-10, are EQUAL within the accuracy; `vase1.smb`'s area is
//! classified by its moved B-spline faces alone ([`CORPUS_MOVED_FACES`]). The other volumes
//! are EQUAL, those whose enclosure spends its budget too wide for the accuracy. The volumes
//! of `vase1.smb` and `vase3.smb` ([`CORPUS_UNENCLOSED`], moves of 1.4e-13 and 3.7e-14) have
//! no certified enclosure: those bodies have offset faces the enclosure has no NURBS form
//! of, and the volume does not split by face. No entry is FARTHER.
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
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::enclosure::{enclose_face_area_relative, enclose_volume, Bounds};
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::PPF_AREA;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, FaceId, Kernel, Result, SolidError};

const UNIT: &str = "blend.fillet_executive_p3";
/// The width asked of each enclosure, relative to its value's scale.
const ENCLOSURE_WIDTH: f64 = 1e-5;
/// The fillet rows' integration accuracy (`brep_mass_properties` at `1e-4`), relative to
/// each value's scale.
const FILLET_ACCURACY: f64 = 1e-4;
/// The tests the fillet rows are split into, by body.
const PARTS: usize = 4;

#[path = "moved_mass_rows/harness.rs"]
mod harness;
use harness::{side, within_accuracy, Row, Side, Table};

include!("mass_knot_line_moved_rows/rows.rs");
include!("mass_knot_line_moved_rows/expected.rs");

/// The fillet rows and how they are classified.
const TABLE: Table = Table {
    unit: UNIT,
    tag: "KNOTLINE",
    rows: &ROWS,
    parts: PARTS,
    width: ENCLOSURE_WIDTH,
    accuracy: FILLET_ACCURACY,
};

/// Classifies `part` and checks it against [`EXPECTED`] and [`WIDE`]: no entry is FARTHER.
fn check(part: usize) {
    assert!(EXPECTED.iter().all(|e| e.2 != Side::Farther));
    TABLE.check(part, &EXPECTED, &WIDE);
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
    let pinned: Vec<(&str, usize)> = EXPECTED.iter().map(|e| (e.0, e.1)).collect();
    assert_eq!(TABLE.moved(), pinned);
    assert!((0..ROWS.len()).all(|i| TABLE.part_of(i) < PARTS));
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

/// Main's area share (`corpus.parity`'s per-face magnitude) of each face whose share the
/// change moved, by index in `bl::faces` order, for a file whose area is classified by its
/// moved faces alone. `vase1.smb`'s offset faces (7 to 10) have no NURBS form for the
/// enclosure, but no knot lines either, so the change leaves their shares as they were; its
/// moved faces are B-spline faces. Main's total is these shares put back into this branch's
/// per-face sum, in order, bit for bit (asserted).
const CORPUS_MOVED_FACES: [(&str, &[(usize, f64)]); 1] = [(
    "Fillet/vase1.smb",
    &[
        (11, 1625.6165255730168),
        (12, 1625.4290821567192),
        (13, 1625.4190215795325),
        (14, 1625.4276823035011),
        (15, 1387.8705788203556),
        (16, 1387.8724374646633),
        (17, 1387.8724608831935),
        (18, 1387.8705996055176),
    ],
)];

/// The moved `corpus.parity` volumes with no certified enclosure (ARGUED, not PROVEN):
/// their bodies have offset faces, which the enclosure has no NURBS form of, and
/// `brep_volume` does not split into per-face shares (the gap-closure strips and corners
/// are shared between faces), so the moved part cannot be enclosed alone as an area can.
/// Each moved by less than `values_agree` (1.4e-13 and 3.7e-14 of the volume).
const CORPUS_UNENCLOSED: [(&str, &str); 2] = [
    ("Fillet/vase1.smb", "volume"),
    ("Fillet/vase3.smb", "volume"),
];

/// The classified corpus entries whose enclosure, wider than asked (the volume
/// enclosures' refinement stopped at its cell budget), leaves `|n - o| / 2 + w` above
/// [`CORPUS_ACCURACY`] of the value: EQUAL there bounds the new value's error only by that
/// width.
const CORPUS_WIDE: [(&str, &str); 9] = [
    ("Fillet/knife.smb", "volume"),
    ("Fillet/blade_hub_1.smb", "volume"),
    ("Fillet/blade_hub_2.smb", "volume"),
    ("Fillet/blade_hub_3.smb", "volume"),
    ("Fillet/bottle.smb", "volume"),
    ("Fillet/box_extrusion2.smb", "volume"),
    ("Fillet/cliff_1.smb", "volume"),
    ("Fillet/cliff_2.smb", "volume"),
    ("Solids/revolution_mold.smb", "volume"),
];

/// `corpus.parity`'s integration accuracy.
const CORPUS_ACCURACY: f64 = 1e-6;
/// The volume enclosure's width, relative to the volume: narrow enough to rank the moves
/// (8.0e-9 and 7.0e-9 of the volume) where the enclosure reaches it.
const CORPUS_VOLUME_WIDTH: f64 = 1e-10;
/// Each face's area enclosure's width, relative to its area.
const CORPUS_AREA_WIDTH: f64 = 1e-10;

/// An enclosure of the total area of `faces` of `b`, face by face (each positive), the sum
/// rounded outward.
fn enclose_faces_area(k: &Kernel, b: &mut Brep, faces: &[FaceId]) -> Result<Bounds> {
    let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
    for &f in faces {
        let e = enclose_face_area_relative(k, b, &[f], CORPUS_AREA_WIDTH)?;
        assert!(e.lo > 0.0, "{f:?}: [{}, {}]", e.lo, e.hi);
        lo = (lo + e.lo).next_down();
        hi = (hi + e.hi).next_up();
    }
    Ok(Bounds { lo, hi })
}

/// Each moved `corpus.parity` entry, read and measured as that unit measures it. Each run
/// reproduces this branch's value, and each entry is classified against a certified
/// enclosure. The area is `corpus.parity`'s per-face sum. The volume is `brep_volume`, whose
/// gap-closure strips the change splits at the knot-line crossings too.
#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_corpus_entries_are_classified() {
    let mut classes = Vec::new();
    let mut unenclosed = Vec::new();
    let mut wide = Vec::new();
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
        let mut shares = Vec::new();
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
            shares.push(p.area.abs());
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
            // The area is `corpus.parity`'s sum of each face's magnitude (a sheet's face, such
            // as `shell_fillet2.smb`'s, counts from its upward use), so it is enclosed face by
            // face. Where only some faces' shares moved ([`CORPUS_MOVED_FACES`]), the entry
            // is classified by those faces' shares alone: main's and this branch's sums of
            // them, in face order, against an enclosure of those faces' area.
            let moved = CORPUS_MOVED_FACES
                .iter()
                .find(|m| m.0 == path && what == "area");
            let (oc, nc, band) = if let Some((_, moved)) = moved {
                let mut main_shares = shares.clone();
                let (mut o_m, mut n_m) = (0.0, 0.0);
                for &(i, share) in *moved {
                    assert_ne!(shares[i], share, "{path}: face {i}'s share did not move");
                    main_shares[i] = share;
                    o_m += share;
                    n_m += shares[i];
                }
                let main_sum = main_shares.iter().fold(0.0, |s, a| s + a);
                assert_eq!(
                    main_sum.to_bits(),
                    o.to_bits(),
                    "{path}: main's shares do not give main's area"
                );
                let ids: Vec<FaceId> = moved.iter().map(|&(i, _)| faces[i]).collect();
                (o_m, n_m, enclose_faces_area(&k, &mut b, &ids))
            } else if what == "area" {
                (o, n, enclose_faces_area(&k, &mut b, &faces))
            } else {
                let band = enclose_volume(&k, &mut b, CORPUS_VOLUME_WIDTH * n.abs()).map(|e| {
                    eprintln!("{path}: the volume enclosure stopped at {:?}", e.stop);
                    Bounds { lo: e.lo, hi: e.hi }
                });
                (o, n, band)
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
            let class = side(oc, nc, band.lo, band.hi).unwrap();
            eprintln!(
                "KNOTLINE {class:?} {path}: {what} main {oc:e}, now {nc:e}, move/value {:e}, \
                 enclosure [{:e}, {:e}], w/value {:e}",
                (nc - oc).abs() / n.abs(),
                band.lo,
                band.hi,
                band.width() / n.abs()
            );
            // the accuracy the value claims, relative to the whole value
            if !within_accuracy(oc, nc, band, CORPUS_ACCURACY, n.abs()) {
                eprintln!("KNOTLINE WIDE {path}: {what}");
                wide.push((path, what));
            }
            classes.push((path, what, class));
        }
    }
    assert_eq!(unenclosed, CORPUS_UNENCLOSED.to_vec(), "{unenclosed:#?}");
    assert!(classes.iter().all(|c| c.2 != Side::Farther), "{classes:#?}");
    assert_eq!(classes, CORPUS_EXPECTED.to_vec(), "{classes:#?}");
    assert_eq!(wide, CORPUS_WIDE.to_vec(), "{wide:#?}");
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
