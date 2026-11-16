//! The measure family on closed forms: mass properties, area, box, point class, gap samples, and
//! the picks' distances and normals.
#![allow(clippy::unwrap_used)]

use std::f64::consts::PI;

use solidcore_ops::session;
use solidcore_ops::{make_box, make_sphere, make_torus, PointClass};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn a_box_mass_properties_are_its_closed_form() {
    all_fixes();
    let b = make_box([1.0, -2.0, 3.0], [1.0, 2.0, 3.0]).unwrap();
    let m = b.mass_properties(1e-8).unwrap();
    assert_eq!(m.requested_accuracy, 1e-8);
    assert!(
        near(m.volume, 6.0, 1e-9) && near(m.area, 22.0, 1e-9),
        "{m:?}"
    );
    for (c, e) in m.centroid.iter().zip([1.5, -1.0, 4.5]) {
        assert!(near(*c, e, 1e-9), "{m:?}");
    }
    for (i, e) in m.moments.iter().zip([6.5, 5.0, 2.5]) {
        assert!(near(*i, e, 1e-8), "{m:?}");
    }
    assert!(m.products.iter().all(|p| p.abs() <= 1e-8), "{m:?}");
    assert_eq!(m.inertia_tensor()[0][0], m.moments[0]);
}

#[test]
fn a_sphere_mass_properties_and_area() {
    all_fixes();
    let s = make_sphere([1.0, 2.0, 3.0], 2.0).unwrap();
    let m = s.mass_properties(1e-8).unwrap();
    let v = 4.0 / 3.0 * PI * 8.0;
    assert!(
        near(m.volume, v, 1e-6) && near(m.area, 16.0 * PI, 1e-6),
        "{m:?}"
    );
    for i in m.moments {
        assert!(near(i, 0.4 * v * 4.0, 1e-6), "{m:?}");
    }
    let a = s.area(1e-8).unwrap();
    assert!(near(a, 16.0 * PI, 1e-6), "{a}");
    assert!(s.area(f64::NAN).is_err() && s.area(0.0).is_err() && s.mass_properties(-1.0).is_err());
}

#[test]
fn a_box_bounds_class_and_counts() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!(b.bounds().unwrap(), ([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]));
    assert_eq!(
        b.classify_point([0.5, 1.0, 1.5]).unwrap(),
        PointClass::Inside
    );
    assert_eq!(
        b.classify_point([5.0, 1.0, 1.5]).unwrap(),
        PointClass::Outside
    );
    assert_eq!(
        b.classify_point([1.0, 1.0, 1.5]).unwrap(),
        PointClass::OnBoundary
    );
    assert!(b.classify_point([f64::INFINITY, 0.0, 0.0]).is_err());
    assert!(b.is_manifold_solid().unwrap());
    assert_eq!(b.solid_count().unwrap(), 1);
}

/// Many points at once classify as each does alone, in order, a point that is not finite
/// refused in its place; a batch of only such points is refused point by point.
#[test]
fn a_box_classifies_many_points_as_each_alone() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let points = [
        [0.5, 1.0, 1.5],
        [f64::INFINITY, 0.0, 0.0],
        [5.0, 1.0, 1.5],
        [1.0, 1.0, 1.5],
        [0.0, 0.0, 0.0],
        [0.25, 0.5, 2.5],
    ];
    let batch = b.classify_points(&points).unwrap();
    let alone: Vec<_> = points.iter().map(|&p| b.classify_point(p)).collect();
    assert_eq!(batch, alone);
    assert_eq!(
        batch
            .iter()
            .map(|c| c.as_ref().ok().copied())
            .collect::<Vec<_>>(),
        [
            Some(PointClass::Inside),
            None,
            Some(PointClass::Outside),
            Some(PointClass::OnBoundary),
            Some(PointClass::OnBoundary),
            Some(PointClass::Inside)
        ]
    );
    assert!(b.classify_points(&[]).unwrap().is_empty());
    let refused = b
        .classify_points(&[[f64::NAN, 0.0, 0.0], [0.0, f64::INFINITY, 0.0]])
        .unwrap();
    assert!(refused.len() == 2 && refused.iter().all(Result::is_err));
}

#[test]
fn a_torus_area_is_its_closed_form() {
    all_fixes();
    let t = make_torus([0.0, 0.0, 0.0], 4.0, 1.0).unwrap();
    let a = t.area(1e-8).unwrap();
    assert!(near(a, 16.0 * PI * PI, 1e-6), "{a}");
}

/// `face_distances_within` answers as `face_distances` does per face, for every point the
/// caller's `reach` keeps: on a 30 x 20 x 10 box, the distance from each of several points to
/// each of the six faces. A face the reach cannot cover answers `None` instead of its distance,
/// and the nearest face within the reach is always answered, so a caller that keeps distances
/// within the reach reads the same faces.
#[test]
fn a_batch_of_face_distances_matches_one_face_at_a_time_within_its_reach() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [30.0, 20.0, 10.0]).unwrap();
    let (record, order) = b.to_record().unwrap();
    let points = [
        [15.0, 10.0, 20.0],
        [15.0, 10.0, 5.0],
        [-4.0, 10.0, 5.0],
        [30.5, 10.0, 5.0],
    ];
    let one: Vec<Vec<Option<f64>>> = (0..record.faces.len())
        .map(|f| b.face_distances(&order, f, &points).unwrap())
        .collect();
    // A reach past the box's own size keeps every pair, so the batch must match throughout,
    // entry for entry. This compares the `None`s too: a pair whose shape or solve answers with
    // a status is `None` in both, and neither fails the call.
    let all = b.face_distances_within(&order, &points, 1e3).unwrap();
    assert_eq!(all, one, "every pair kept");
    // A small reach drops the far pairs and keeps the near ones, and each kept distance is the
    // one the per-face call gives.
    let reach = 1.0;
    let near = b.face_distances_within(&order, &points, reach).unwrap();
    assert_eq!(near.len(), one.len());
    let mut kept = 0;
    for (f, (row, want)) in near.iter().zip(&one).enumerate() {
        for (j, (got, want)) in row.iter().zip(want).enumerate() {
            match got {
                Some(d) => {
                    assert_eq!(Some(*d), *want, "face {f} point {j}");
                    kept += 1;
                }
                // Dropped only where the per-face answer is further than the reach: the
                // caller's own test would have discarded it too.
                None => assert!(
                    want.is_none_or(|w| w > reach),
                    "face {f} point {j} dropped at {want:?}"
                ),
            }
        }
    }
    assert!(kept > 0, "the small reach kept nothing");
    // For each point, the nearest face within the reach survives the batch.
    for (j, _) in points.iter().enumerate() {
        let best = one
            .iter()
            .filter_map(|r| r.get(j).copied().flatten())
            .fold(f64::INFINITY, f64::min);
        if best <= reach {
            let got = near
                .iter()
                .filter_map(|r| r.get(j).copied().flatten())
                .fold(f64::INFINITY, f64::min);
            assert_eq!(got, best, "point {j}'s nearest face");
        }
    }
    assert!(b.face_distances_within(&order, &points, -1.0).is_err());
    assert!(b.face_distances_within(&order, &points, f64::NAN).is_err());
    assert!(b
        .face_distances_within(&order, &[[f64::INFINITY, 0.0, 0.0]], 1.0)
        .is_err());
    assert_eq!(
        b.face_distances_within(&order, &[], 1.0).unwrap().len(),
        record.faces.len()
    );
}
