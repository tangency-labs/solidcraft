//! The `corpus.parity` row the retirement of `uv_trim_corner_end_points_only` moves:
//! `Fillet/sphere_diff_3cylinders.smb`, whose read closes its faces' UV trim corners
//! (`check_and_fix_uv_trim_curves`) by moving only the trims' end control points instead
//! of trimming each trim at the vertex's drop point and stretching it back.
//!
//! * `area`, `volume` and their round-trip copies `rt_area`, `rt_volume` move. Each is
//!   read and measured as `corpus.parity` measures it, the run reproduces this branch's
//!   value, and the move is classified against a certified enclosure of the true value
//!   (`enclose_face_area_relative` face by face, `enclose_volume`), as
//!   `mass_knot_line_moved_rows_are_classified` classifies its corpus entries: all EQUAL,
//!   none FARTHER ([`moved_corpus_entries_are_classified`]).
//! * `check` and `rt_check` move from 3 to 6: the three new lines (tests 11, 13 and 15:
//!   a slow first derivative, a cusp, internal coincident control points) are on edge 24's
//!   trim, whose first knot span `[0.49999995785315, 0.5]` maps to one point. Every run of
//!   `degree + 1` control points within `3e-8` of each other in UV on every trim the read
//!   leaves with a pole is, bit for bit, a run of one of the file's own UV curves
//!   ([`the_new_check_lines_are_the_files_pole_span`]): the read keeps the file's pole span,
//!   it does not make one. Upstream's trim at the far end's drop point cut it off as a side
//!   effect (a trim interval within tolerance of a knot snaps to it), while its stretch back
//!   over the interval moved every interior parameter off the edge's, the defect the
//!   retirement removes.
//!
//! Run: `cargo test --profile goldens -p goldens --test uvtc_corpus_row -- --include-ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::float_cmp
)] // test

#[path = "moved_mass_rows/harness.rs"]
#[allow(dead_code)] // this test uses only `side`, `within_accuracy` and `Side`
mod harness;
use harness::{side, within_accuracy, Side};

use goldens::case::repo_root;
use goldens::compare::values_agree;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::tol::ZONE_TOL_3D;
use solidcore::mass::enclosure::{enclose_face_area_relative, enclose_volume, Bounds};
use solidcore::mass::integrator::properties::{brep_volume, face_properties, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::PPF_AREA;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EdgeId, FaceId, Kernel};

/// The moved file, under `pt_TestFiles`.
const PATH: &str = "Fillet/sphere_diff_3cylinders.smb";
/// `corpus.parity`'s integration accuracy.
const ACCURACY: f64 = 1e-6;
/// Each enclosure's width, relative to its value (as `mass_knot_line_moved_rows_are_classified`).
const WIDTH: f64 = 1e-10;

/// The moved entries: (name, main's value, this branch's value), from the two trees'
/// `corpus.parity.jsonl`.
const MOVED: [(&str, f64, f64); 4] = [
    ("area", 38_235.992_697_050_83, 38_235.992_695_772_5),
    ("volume", 169_499.150_354_326_24, 169_499.150_354_326_27),
    ("rt_area", 38_235.996_596_079_174, 38_235.996_594_800_28),
    ("rt_volume", 169_499.150_354_326_27, 169_499.150_354_326_3),
];
/// Each moved entry's class.
const EXPECTED: [(&str, Side); 4] = [
    ("area", Side::Equal),
    ("volume", Side::Equal),
    ("rt_area", Side::Equal),
    ("rt_volume", Side::Equal),
];
/// The entries whose enclosure, refined to its cell budget, is wider than
/// [`ACCURACY`]: EQUAL there says only that both values lie in it. The volumes move by
/// one unit in their last place.
const WIDE: [&str; 2] = ["volume", "rt_volume"];

fn file_bytes() -> Vec<u8> {
    std::fs::read(
        repo_root()
            .join("upstream/TestFiles/pt_TestFiles")
            .join(PATH),
    )
    .unwrap()
}

/// `p7::read_from_file` of `bytes` (as `corpus.parity` reads a B-rep file).
fn read(k: &mut Kernel, bytes: Vec<u8>) -> Brep {
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let notify = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &notify,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b
}

/// `corpus.parity`'s area: the sum of each face's magnitude.
fn area(k: &Kernel, b: &mut Brep) -> f64 {
    let mut area = 0.0;
    for f in bl::faces(b, None).unwrap() {
        let bbox = solidcore::query::face::p1::calculate_bounding_box(k, &*b, f, false).unwrap();
        let s = bbox.size();
        let d = s.x.max(s.y).max(s.z);
        let p = face_properties(
            k,
            b,
            f,
            OrientType::Same,
            ACCURACY,
            bbox.mid(),
            d * d / 100.0,
            0.0,
            PPF_AREA,
        )
        .unwrap();
        area += p.area.abs();
    }
    area
}

/// `corpus.parity`'s volume.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::new();
    let key = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key,
    };
    brep_volume(k, b, ACCURACY, &attrs).unwrap()
}

/// `corpus.parity`'s round trip: `b` flattened, written as ASCII and read back.
fn round_trip(k: &mut Kernel, b: &Brep) -> Brep {
    let attributes = AttributeStore::default();
    let records = NotifyRecords::new();
    let key = solidcore::topology::loopuse::notify_ref;
    let env = FromBrepEnv {
        attributes: &attributes,
        key_of: &key,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let mut all = Vec::new();
    let bytes = BrepData::from_brep(b, &env, &mut all)
        .and_then(|mut data| data.write_to_file_with_nurbs(k, FileType::Ascii))
        .unwrap();
    read(k, bytes)
}

/// An enclosure of the total area of `b`'s faces, face by face, the sum rounded outward.
fn enclose_area(k: &Kernel, b: &mut Brep) -> Bounds {
    let (mut lo, mut hi) = (0.0_f64, 0.0_f64);
    for f in bl::faces(b, None).unwrap() {
        let e = enclose_face_area_relative(k, b, &[f], WIDTH).unwrap();
        assert!(e.lo > 0.0, "{f:?}: [{}, {}]", e.lo, e.hi);
        lo = (lo + e.lo).next_down();
        hi = (hi + e.hi).next_up();
    }
    Bounds { lo, hi }
}

/// Each moved entry reproduced by the run and classified against a certified enclosure.
#[test]
#[ignore = "encloses the body and its round trip (minutes); see the module docs"]
fn moved_corpus_entries_are_classified() {
    let mut k = Kernel::default();
    let mut b = read(&mut k, file_bytes());
    let mut rt = round_trip(&mut k, &b);
    let mut classes = Vec::new();
    let mut wide = Vec::new();
    for (name, o, n) in MOVED {
        let body = if name.starts_with("rt_") {
            &mut rt
        } else {
            &mut b
        };
        let (run, band) = if name.ends_with("area") {
            (area(&k, body), enclose_area(&k, body))
        } else {
            let run = volume(&k, body);
            let e = enclose_volume(&k, body, WIDTH * n.abs()).unwrap();
            eprintln!("UVTC {name}: the volume enclosure stopped at {:?}", e.stop);
            (run, Bounds { lo: e.lo, hi: e.hi })
        };
        assert!(
            values_agree(run, n, None),
            "{name}: the run's {run} does not reproduce this branch's {n}"
        );
        let class = side(o, n, band.lo, band.hi).unwrap();
        eprintln!(
            "UVTC {class:?} {name}: main {o:e}, now {n:e}, move/value {:e}, enclosure [{:e}, \
             {:e}], w/value {:e}",
            (n - o).abs() / n.abs(),
            band.lo,
            band.hi,
            band.width() / n.abs()
        );
        if !within_accuracy(o, n, band, ACCURACY, n.abs()) {
            eprintln!("UVTC WIDE {name}");
            wide.push(name);
        }
        classes.push((name, class));
    }
    assert!(classes.iter().all(|c| c.1 != Side::Farther), "{classes:#?}");
    assert_eq!(classes, EXPECTED.to_vec(), "{classes:#?}");
    assert_eq!(wide, WIDE.to_vec(), "{wide:#?}");
}

/// A B-spline's NURB (a line or other curve: `None`).
fn nurb(c: &Curve) -> Option<solidcore::nurbs::crv_basic::Curve> {
    let mut c = c.clone();
    match c.kind_mut() {
        CurveKind::BSpline(s) => Some(s.get_or_create_gw_nurb().unwrap().clone()),
        _ => None,
    }
}

/// Whether `c` has a knot span all of whose `degree + 1` control points lie within
/// `ZONE_TOL_3D / 10` (test 15's tolerance) of each other in UV.
fn has_pole(k: &Kernel, c: &Curve) -> bool {
    let mut c = c.clone();
    match c.kind_mut() {
        CurveKind::BSpline(s) => s.has_internal_pole(k, ZONE_TOL_3D / 10.0).unwrap(),
        _ => false,
    }
}

/// The control points, as bits, of each run of `degree + 1` consecutive control points of
/// `c` that lie within `3e-8` of the run's first in UV.
fn pole_spans(c: &solidcore::nurbs::crv_basic::Curve) -> Vec<Vec<[u64; 3]>> {
    let p = c.cpts();
    let n = c.degree() + 1;
    let uv = |q: &solidcore::nurbs::math::CPoint| (q.x / q.w, q.y / q.w);
    (0..p.len().saturating_sub(n - 1))
        .filter(|&i| {
            let (u0, v0) = uv(&p[i]);
            p[i + 1..i + n].iter().all(|q| {
                let (u, v) = uv(q);
                ((u - u0).powi(2) + (v - v0).powi(2)).sqrt() < 3e-8
            })
        })
        .map(|i| {
            p[i..i + n]
                .iter()
                .map(|q| [q.x.to_bits(), q.y.to_bits(), q.w.to_bits()])
                .collect()
        })
        .collect()
}

/// The three check lines the retirement adds are on a pole span the file itself holds:
/// every UV trim of the read with a pole (a knot span whose control points all lie within
/// test 15's tolerance) has a run of `degree + 1` control points within `3e-8` of each
/// other in UV that is, bit for bit, a run of one of the file's own UV curves.
#[test]
fn the_new_check_lines_are_the_files_pole_span() {
    let mut k = Kernel::default();
    let data = BrepData::read_from_file(&k, file_bytes(), FileType::Ascii).unwrap();
    let file_spans: Vec<Vec<[u64; 3]>> = data
        .uv_curves
        .iter()
        .filter_map(|c| nurb(&c.object))
        .flat_map(|c| pole_spans(&c))
        .collect();
    let file_poles = data
        .uv_curves
        .iter()
        .filter(|c| has_pole(&k, &c.object))
        .count();
    drop(data);

    let b = read(&mut k, file_bytes());
    let mut poles = Vec::new();
    for (eu, r) in b.edgeuses().iter() {
        let Some(c) = r.uv_trim_curve.as_ref() else {
            continue;
        };
        if !has_pole(&k, c) {
            continue;
        }
        let n = nurb(c).unwrap();
        let spans = pole_spans(&n);
        let edge = r.edge.map(EdgeId::index);
        eprintln!(
            "UVTC pole trim {eu:?} edge {edge:?}: degree {}, knots {:?}, {} spans, from the file: {}",
            n.degree(),
            &n.knots()[..n.knots().len().min(8)],
            spans.len(),
            spans.iter().filter(|s| file_spans.contains(s)).count()
        );
        poles.push((eu, edge, spans));
    }
    eprintln!(
        "UVTC {} pole trims read, {file_poles} pole curves and {} pole spans in the file",
        poles.len(),
        file_spans.len()
    );
    assert!(
        poles.iter().any(|p| p.1 == Some(24)),
        "edge 24's trim has no pole"
    );
    for (eu, edge, spans) in &poles {
        assert!(
            !spans.is_empty() && spans.iter().all(|s| file_spans.contains(s)),
            "{eu:?} (edge {edge:?}): a pole span that is not the file's"
        );
    }
}
