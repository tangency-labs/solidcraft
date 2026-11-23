//! A dropped UV trim does not fold back over a sliver of its edge that lies just outside
//! the face's domain.
//!
//! On `Fillet/sphere_diff_3cylinders.smb`, edge 30 starts about `2.1e-8` below `v = 0`
//! of face 7 (domain `[0, 1]²`) and reaches the boundary at `t = 0.5`, a 3D piece about
//! `2.6e-6` long. The drop's first span covers that sliver. Its polygon had both ends
//! clamped onto `v = 0` while its interior points kept the unclamped tangent
//! (`v = 0, +7e-9, -7e-9, 5.6e-15`): an S that the B-spline check reports. The span is
//! now left out as a clamped sliver and the next span stretched over its range
//! (`intersect::surface_drop_lib::clamped_sliver`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::attribute::AttributeStore;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::face;
use solidcore::topology::edgeuse::p3::create_uv_trim_curve;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EdgeId, Kernel};

/// Reads an upstream `.smb` test file (healing on, as `SmBrep::ReadFromFile`).
fn read(k: &mut Kernel, name: &str) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b
}

/// The control points of a B-spline curve in Euclidean form, with its knots.
fn polygon(c: &Curve) -> (Vec<(f64, f64)>, Vec<f64>) {
    let mut c = c.clone();
    let CurveKind::BSpline(s) = c.kind_mut() else {
        panic!("the dropped trim is a B-spline");
    };
    let n = s.get_or_create_gw_nurb().unwrap();
    let points = n
        .cpts()
        .iter()
        .map(|q| {
            let e = q.to_euclid();
            (e.x, e.y)
        })
        .collect();
    (points, n.knots().to_vec())
}

#[test]
fn the_trim_over_the_sliver_is_monotone_and_passes_the_curve_checks() {
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_diff_3cylinders.smb");
    let eu = EdgeuseId::from_index(142);
    let edge = b.edgeuse(eu).unwrap().edge.unwrap();
    assert_eq!(edge, EdgeId::from_index(30));
    let f = face(&b, eu).unwrap().unwrap();
    assert_eq!(f.index(), 7);
    let interval = b.edge(edge).unwrap().interval;

    let made = create_uv_trim_curve(&k, &mut b, eu, false, None).unwrap();

    // the trim still covers the whole edge
    let ivl = made.curve.natural_interval();
    assert_eq!(ivl.min(), interval.min());
    assert_eq!(ivl.max(), interval.max());

    // The first span's polygon is monotone in v and inside v >= 0, so the span is
    // (variation diminishing): no S at the start.
    let (points, knots) = polygon(&made.curve);
    assert_eq!(knots[0], interval.min());
    let first: Vec<f64> = points[..4].iter().map(|p| p.1).collect();
    assert!(first.iter().all(|&v| v >= 0.0), "{first:?}");
    assert!(
        first.windows(2).all(|w| w[0] <= w[1]) || first.windows(2).all(|w| w[0] >= w[1]),
        "{first:?}"
    );

    // The B-spline check on the face raises none of the vanishing first derivative (11),
    // radius of curvature (13) or coincident control point (15) tests the S raised.
    let surface = b.face(f).unwrap().surface.as_ref();
    let CurveKind::BSpline(s) = made.curve.kind() else {
        panic!("the dropped trim is a B-spline");
    };
    let mut list = AssertArray::new();
    s.assert_valid_on(&k, Some(&mut list), true, ObjectKey(5), surface)
        .unwrap();
    let failed: Vec<_> = (0..list.len())
        .filter_map(|i| list.get(i))
        .filter(|r| !r.ok)
        .map(|r| r.test_index)
        .collect();
    for test in [11, 13, 15] {
        assert!(!failed.contains(&test), "test {test} fails: {failed:?}");
    }
}
