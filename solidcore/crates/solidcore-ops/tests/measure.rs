//! The measure family on closed forms: mass properties, area, box, point class, gap samples, and
//! the picks' distances and normals.
#![allow(clippy::unwrap_used)]

use std::f64::consts::PI;

use solidcore_ops::face_surface::FaceSurface;
use solidcore_ops::session;
use solidcore_ops::surface::ruled;
use solidcore_ops::sweep::{planar_faces, Curve as SweepCurve};
use solidcore_ops::{
    make_box, make_cylinder, make_sphere, make_torus, Body, Error, Order, PointClass,
    EDGE_POINTS_MAX,
};
use solidcore_record::brep::{Brep, Curve};

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

/// Located points are the classified points, each a location where the class names one; an
/// unsure region never arises on a box, whose regions the kernel knows.
#[test]
fn a_box_locates_points_as_it_classifies_them() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let points = [
        [0.5, 1.0, 1.5],
        [f64::INFINITY, 0.0, 0.0],
        [5.0, 1.0, 1.5],
        [1.0, 1.0, 1.5],
    ];
    let located = b.locate_points(&points).unwrap();
    let classified = b.classify_points(&points).unwrap();
    assert_eq!(located.len(), classified.len());
    for (l, c) in located.iter().zip(&classified) {
        let as_class = l.as_ref().ok().map(|l| match l {
            solidcore_ops::PointLocation::OnBoundary => PointClass::OnBoundary,
            solidcore_ops::PointLocation::Inside => PointClass::Inside,
            solidcore_ops::PointLocation::Outside => PointClass::Outside,
            solidcore_ops::PointLocation::Unsure { .. } => panic!("a box has no unsure region"),
        });
        assert_eq!(as_class, c.as_ref().ok().copied());
        assert_eq!(l.is_err(), c.is_err());
    }
    assert_eq!(located[1], Err(Error::Argument));
}

/// A body's own order is its lists as the kernel holds them: a box's 6 faces, 12 edges and 8
/// vertices, and an edge picked by a point is the one at that index of the own order (the
/// nearest edge to a point on it is at distance 0).
#[test]
fn a_box_own_order_names_its_picked_edges() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let order = b.own_order();
    assert_eq!(
        (order.faces.len(), order.edges.len(), order.vertices.len()),
        (6, 12, 8)
    );
    assert_eq!(order.split_edges, 0);
    let at = [0.5, 0.0, 0.0];
    let Ok(picked) = solidcore_ops::pick::edges(&b, &[at], 1e-6).unwrap() else {
        panic!("a point on an edge picks it")
    };
    let p = b
        .edge_closest_point(&order, picked[0], at)
        .unwrap()
        .unwrap();
    assert!(p.distance <= 1e-12, "{p:?}");
}

/// The largest gap sample of an edge against each face of a box is the largest of the gap
/// samples, and an edge lies on the faces it bounds: its largest gap to them is nothing.
#[test]
fn a_box_edge_largest_gap_is_its_largest_sample() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let order = b.own_order();
    let mut bounded = 0;
    for face in 0..order.faces.len() {
        let largest = b.edge_face_largest_gap(&order, 0, face).unwrap();
        let samples = b.edge_face_gap_samples(&order, 0, face).unwrap();
        let most = samples
            .iter()
            .map(|s| s.distance.value)
            .fold(0.0_f64, f64::max);
        assert!(
            (largest - most).abs() <= 1e-12,
            "face {face}: {largest} against {most}"
        );
        if largest <= 1e-12 {
            bounded += 1;
        }
    }
    assert!(bounded >= 2, "an edge lies on the two faces it bounds");
    assert!(b.edge_face_largest_gap(&order, 99, 0).is_err());
}

/// A check scoped to the whole body is the plain check; a valid box is valid in either scope.
#[test]
fn a_box_checks_the_same_in_either_scope() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let plain = b.check(solidcore_ops::check::CheckLevel::All).unwrap();
    assert_eq!(
        b.check_scoped(
            solidcore_ops::check::CheckLevel::All,
            solidcore_ops::check::ValidityScope::Body
        )
        .unwrap(),
        plain
    );
    assert!(plain.is_valid(), "{plain}");
    assert!(b
        .check_scoped(
            solidcore_ops::check::CheckLevel::All,
            solidcore_ops::check::ValidityScope::Stitched
        )
        .unwrap()
        .is_valid());
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

/// A ray down the line x = 5, y = -3 from z = 4, beside a 10 mm box at the origin: of the box's
/// edges, the one along x at y = 0, z = 0 is nearest, at (5, 0, 0), 3 from the ray's (5, -3, 0).
#[test]
fn a_ray_beside_a_box_is_nearest_the_edge_it_passes() {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = b.to_record().unwrap();
    let nearest = (0..order.edges.len())
        .filter_map(|i| {
            b.edge_closest_to_ray(&order, i, [5.0, -3.0, 4.0], [0.0, 0.0, -1.0])
                .unwrap()
        })
        .min_by(|a, c| a.distance.total_cmp(&c.distance))
        .unwrap();
    let close = |p: [f64; 3], q: [f64; 3]| p.iter().zip(q).all(|(a, b)| (a - b).abs() <= 1e-6);
    assert!(near(nearest.distance, 3.0, 1e-6), "{nearest:?}");
    assert!(close(nearest.point, [5.0, 0.0, 0.0]), "{nearest:?}");
    assert!(close(nearest.on_ray, [5.0, -3.0, 0.0]), "{nearest:?}");
    assert_eq!(
        b.edge_closest_to_ray(&order, order.edges.len(), [0.0; 3], [0.0, 0.0, 1.0]),
        Err(Error::Argument)
    );
}

/// A tube: the ruled sheet between the radius-5 circles about z at z = 0 and z = 10, one face.
/// Each circle is the exact rational quadratic on the square around it (Piegl and Tiller, The
/// NURBS Book, 7.5): corners weighted sqrt(2) / 2, the circle's seam at (5, 0).
fn tube() -> (Body, Brep, Order) {
    let r = 5.0;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = |z: f64| Curve::RationalSpline {
        knots: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        homogeneous: [
            (r, 0.0, 1.0),
            (r, r, w),
            (0.0, r, 1.0),
            (-r, r, w),
            (-r, 0.0, 1.0),
            (-r, -r, w),
            (0.0, -r, 1.0),
            (r, -r, w),
            (r, 0.0, 1.0),
        ]
        .map(|(x, y, w)| [x * w, y * w, z * w, w])
        .to_vec(),
    };
    let sheet = ruled(&ring(0.0), &ring(10.0), false).unwrap();
    let (body, _) = Body::from_record(&sheet, false).unwrap();
    let (record, order) = body.to_record().unwrap();
    (body, record, order)
}

/// Whether two points are within 1e-6 of each other in every coordinate.
fn close(p: [f64; 3], q: [f64; 3]) -> bool {
    p.iter().zip(q).all(|(a, b)| (a - b).abs() <= 1e-6)
}

/// A ray along +x at y = 3, z = 4 crosses the radius-5 tube twice, at x = -4 and x = 4 (a 3-4-5
/// triangle in the plane z = 4). From x = -20 the first hit is (-4, 3, 4), 16 along the ray,
/// whatever the direction's length; its (u, v) is on the face it names, the face's own surface
/// evaluating back to the point. From inside the tube only the far side is ahead. The same ray
/// from x = 20 points away from the tube and meets nothing, as does one at y = 6, past the
/// radius; a direction of no length and a coordinate that is not finite are refused.
#[test]
fn a_ray_through_a_tube_hits_its_nearer_side_with_the_face_and_uv() {
    all_fixes();
    let (b, record, order) = tube();
    for direction in [[1.0, 0.0, 0.0], [2.5, 0.0, 0.0], [1e200, 0.0, 0.0]] {
        let hit = b
            .face_hit_by_ray(&order, [-20.0, 3.0, 4.0], direction)
            .unwrap()
            .unwrap();
        assert!(close(hit.point, [-4.0, 3.0, 4.0]), "{hit:?}");
        assert!(near(hit.distance, 16.0, 1e-6), "{hit:?}");
        let on = FaceSurface::of(&record, hit.face)
            .unwrap()
            .point(hit.uv)
            .unwrap();
        assert!(close(on, hit.point), "{hit:?} evaluates to {on:?}");
    }
    let inside = b
        .face_hit_by_ray(&order, [0.0, 3.0, 4.0], [1.0, 0.0, 0.0])
        .unwrap()
        .unwrap();
    assert!(close(inside.point, [4.0, 3.0, 4.0]), "{inside:?}");
    assert!(near(inside.distance, 4.0, 1e-6), "{inside:?}");
    assert_eq!(
        b.face_hit_by_ray(&order, [20.0, 3.0, 4.0], [1.0, 0.0, 0.0]),
        Ok(None)
    );
    assert_eq!(
        b.face_hit_by_ray(&order, [-20.0, 6.0, 4.0], [1.0, 0.0, 0.0]),
        Ok(None)
    );
    for (origin, direction) in [
        ([-20.0, 3.0, 4.0], [0.0, 0.0, 0.0]),
        ([f64::NAN, 3.0, 4.0], [1.0, 0.0, 0.0]),
        ([-20.0, 3.0, 4.0], [f64::INFINITY, 0.0, 0.0]),
    ] {
        assert_eq!(
            b.face_hit_by_ray(&order, origin, direction),
            Err(Error::Argument),
            "{origin:?} {direction:?}"
        );
    }
}

/// Every point where the ray meets a face, nearest first: along +x at y = 3, z = 4 the tube's
/// one face gives (-4, 3, 4) at 16 and (4, 3, 4) at 24, the first where `face_hit_by_ray` lands;
/// at y = 0 the far crossing, (5, 0, 4), is on the seam and still one hit; from inside, only the
/// far side; facing away, none.
/// What is no ray is refused as `face_hit_by_ray` refuses it.
#[test]
fn a_ray_through_a_tube_hits_both_sides_nearest_first() {
    all_fixes();
    let (b, _, order) = tube();
    let hits = b
        .face_hits_by_ray(&order, [-20.0, 3.0, 4.0], [1.0, 0.0, 0.0])
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for (hit, (at, distance)) in hits
        .iter()
        .zip([([-4.0, 3.0, 4.0], 16.0), ([4.0, 3.0, 4.0], 24.0)])
    {
        assert!(close(hit.point, at), "{hits:?}");
        assert!(near(hit.distance, distance, 1e-6), "{hits:?}");
    }
    let first = b
        .face_hit_by_ray(&order, [-20.0, 3.0, 4.0], [1.0, 0.0, 0.0])
        .unwrap()
        .unwrap();
    assert_eq!(first.face, hits[0].face);
    assert!(close(first.point, hits[0].point), "{first:?} {hits:?}");
    // through the seam: the tube's rings start at (5, 0), so x = 5 is one hit, not two
    let seam = b
        .face_hits_by_ray(&order, [-20.0, 0.0, 4.0], [1.0, 0.0, 0.0])
        .unwrap();
    assert_eq!(seam.len(), 2, "{seam:?}");
    assert!(close(seam[0].point, [-5.0, 0.0, 4.0]), "{seam:?}");
    assert!(close(seam[1].point, [5.0, 0.0, 4.0]), "{seam:?}");
    let inside = b
        .face_hits_by_ray(&order, [0.0, 3.0, 4.0], [1.0, 0.0, 0.0])
        .unwrap();
    assert_eq!(inside.len(), 1, "{inside:?}");
    assert!(close(inside[0].point, [4.0, 3.0, 4.0]), "{inside:?}");
    assert_eq!(
        b.face_hits_by_ray(&order, [20.0, 3.0, 4.0], [1.0, 0.0, 0.0]),
        Ok(vec![])
    );
    assert_eq!(
        b.face_hits_by_ray(&order, [-20.0, 3.0, 4.0], [0.0, 0.0, 0.0]),
        Err(Error::Argument)
    );
}

/// What, the body, the line's point and direction, and the parameters it meets faces at.
type LineCase = (&'static str, Body, [f64; 3], [f64; 3], Vec<f64>);

/// Where a whole line meets a body's faces, both ways from its point, on each face's exact
/// surface, in closed form:
/// - the 10 mm cube at the origin, slanted along (0.6, 0, 0.8) through its centre (5, 5, 5): its
///   top z = 10 at x = 8.75 and its bottom z = 0 at x = 1.25, parameters -6.25 and 6.25; the
///   planes x = 0 and x = 10 it meets at z = -1.67 and 11.67, outside their trims;
/// - the radius-5 cylinder about +Z (z 0..20), along x through (0, 3, 10): its side at x = -4 and
///   4 (a 3-4-5 triangle), parameters -4 and 4, and neither end;
/// - the cone of base radius 6 and top radius 2 over height 8, radius 6 - z/2, along x through
///   (0, 0, 4): its side where the radius is 4, parameters -4 and 4;
/// - the radius-2 sphere about (1, 2, 3), along z through (1.72, 2.96, 3): 1.2 off its axis
///   (0.24 times a 3-4-5 triangle), at z = 3 -+ 1.6, parameters -1.6 and 1.6;
/// - the torus of radii 10 and 2 about +Z, along x through (0, 0, 0): x = -12, -8, 8 and 12;
/// - the rational B-spline tube of radius 5 (z 0..10), along x through (-20, 3, 4): x = -4 and 4,
///   parameters 16 and 24.
///
/// The cone meets the line on its seam (+x) once. Each point is the line's at its parameter. A
/// face index past the faces and a direction of no length are refused.
#[test]
fn a_line_meets_each_kind_of_face_on_its_exact_surface_both_ways() {
    all_fixes();
    let (tube_body, _, _) = tube();
    let cases: Vec<LineCase> = vec![
        (
            "cube",
            make_box([0.0; 3], [10.0; 3]).unwrap(),
            [5.0; 3],
            [0.6, 0.0, 0.8],
            vec![-6.25, 6.25],
        ),
        (
            "cylinder",
            make_cylinder([0.0; 3], 5.0, 20.0).unwrap(),
            [0.0, 3.0, 10.0],
            [1.0, 0.0, 0.0],
            vec![-4.0, 4.0],
        ),
        (
            "cone",
            solidcore_ops::make_cone([0.0; 3], 6.0, 2.0, 8.0).unwrap(),
            [0.0, 0.0, 4.0],
            [1.0, 0.0, 0.0],
            vec![-4.0, 4.0],
        ),
        (
            "sphere",
            make_sphere([1.0, 2.0, 3.0], 2.0).unwrap(),
            [1.72, 2.96, 3.0],
            [0.0, 0.0, 1.0],
            vec![-1.6, 1.6],
        ),
        (
            "torus",
            make_torus([0.0; 3], 10.0, 2.0).unwrap(),
            [0.0; 3],
            [1.0, 0.0, 0.0],
            vec![-12.0, -8.0, 8.0, 12.0],
        ),
        (
            "B-spline tube",
            tube_body,
            [-20.0, 3.0, 4.0],
            [1.0, 0.0, 0.0],
            vec![16.0, 24.0],
        ),
    ];
    for (what, b, through, direction, expected) in cases {
        let (record, order) = b.to_record().unwrap();
        let mut hits = Vec::new();
        for f in 0..record.faces.len() {
            hits.extend(b.face_line_hits(&order, f, through, direction).unwrap());
        }
        hits.sort_by(|x, y| x.parameter.total_cmp(&y.parameter));
        let got: Vec<f64> = hits.iter().map(|h| h.parameter).collect();
        assert_eq!(got.len(), expected.len(), "{what}: {got:?}");
        for (h, t) in hits.iter().zip(&expected) {
            assert!(near(h.parameter, *t, 1e-9), "{what}: {got:?}");
            let on_line = [0, 1, 2].map(|i| through[i] + direction[i] * t);
            assert!(close(h.point, on_line), "{what}: {h:?}");
        }
        assert_eq!(
            b.face_line_hits(&order, record.faces.len(), through, direction),
            Err(Error::Argument),
            "{what}"
        );
        assert_eq!(
            b.face_line_hits(&order, 0, through, [0.0; 3]),
            Err(Error::Argument),
            "{what}"
        );
    }
}

/// A line lying on a face is no single place on it: the face leaves it out and answers, it does
/// not fail (a hole on a cylinder's side drilled along its axis). The radius-5 cylinder about +Z
/// (z 0..20), along z through its side at (3, 4) and on its seam (+x), at z = 10: its side
/// holds the whole line, which meets the body only where it leaves through its ends, z = 0 and
/// z = 20, parameters -10 and 10 (each end face once, on its rim). The 10 mm cube, along x through
/// (5, 5, 10) on its top: the top holds the line, which meets the body where it crosses x = 0 and
/// x = 10, parameters -5 and 5. Every face answers; no face fails.
#[test]
fn a_line_lying_on_a_face_is_left_out_of_its_hits() {
    all_fixes();
    let cases: Vec<LineCase> = vec![
        (
            "cylinder side at (3, 4)",
            make_cylinder([0.0; 3], 5.0, 20.0).unwrap(),
            [3.0, 4.0, 10.0],
            [0.0, 0.0, -1.0],
            vec![-10.0, 10.0],
        ),
        (
            "cylinder seam",
            make_cylinder([0.0; 3], 5.0, 20.0).unwrap(),
            [5.0, 0.0, 10.0],
            [0.0, 0.0, 1.0],
            vec![-10.0, 10.0],
        ),
        (
            "cube top",
            make_box([0.0; 3], [10.0; 3]).unwrap(),
            [5.0, 5.0, 10.0],
            [1.0, 0.0, 0.0],
            vec![-5.0, 5.0],
        ),
    ];
    for (what, b, through, direction, expected) in cases {
        let (record, order) = b.to_record().unwrap();
        let mut hits = Vec::new();
        for f in 0..record.faces.len() {
            let found = b.face_line_hits(&order, f, through, direction);
            let Ok(found) = found else {
                panic!("{what}: face {f}: {found:?}")
            };
            hits.extend(found);
        }
        hits.sort_by(|x, y| x.parameter.total_cmp(&y.parameter));
        let got: Vec<f64> = hits.iter().map(|h| h.parameter).collect();
        assert_eq!(got.len(), expected.len(), "{what}: {got:?}");
        for (h, t) in hits.iter().zip(&expected) {
            assert!(near(h.parameter, *t, 1e-9), "{what}: {got:?}");
            let on_line = [0, 1, 2].map(|i| through[i] + direction[i] * t);
            assert!(close(h.point, on_line), "{what}: {h:?}");
        }
    }
}

/// A doubly curved face: a ray along +z through (1.72, 2.96) runs (0.72, 0.96) off the axis of
/// the radius-2 sphere about (1, 2, 3), 1.2 from it (a 3-4-5 triangle scaled by 0.24), which
/// leaves sqrt(4 - 1.44) = 1.6 in z: the ray meets the sphere first at z = 3 - 1.6 = 1.4, 11.4
/// along it from z = -10, the face's (u, v) evaluating back to the point.
#[test]
fn a_ray_through_a_sphere_hits_its_near_side() {
    all_fixes();
    let s = make_sphere([1.0, 2.0, 3.0], 2.0).unwrap();
    let (record, order) = s.to_record().unwrap();
    let hit = s
        .face_hit_by_ray(&order, [1.72, 2.96, -10.0], [0.0, 0.0, 1.0])
        .unwrap()
        .unwrap();
    assert!(close(hit.point, [1.72, 2.96, 1.4]), "{hit:?}");
    assert!(near(hit.distance, 11.4, 1e-6), "{hit:?}");
    let on = FaceSurface::of(&record, hit.face)
        .unwrap()
        .point(hit.uv)
        .unwrap();
    assert!(close(on, hit.point), "{hit:?} evaluates to {on:?}");
}

/// The unit vectors along x, y and z.
const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

fn dot(p: [f64; 3], q: [f64; 3]) -> f64 {
    p.iter().zip(q).map(|(a, b)| a * b).sum()
}

/// The record face of a box that lies in the plane `dot(axis, p) = at`, found from the faces' own
/// surfaces (the point and normal in the middle of each face's domain), apart from any ray.
fn box_face(record: &Brep, order: &Order, axis: [f64; 3], at: f64) -> usize {
    let in_plane: Vec<usize> = (0..order.faces.len())
        .filter(|&f| {
            let surface = FaceSurface::of(record, f).unwrap();
            let [[u0, u1], [v0, v1]] = surface.domain();
            let middle = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
            let p = surface.point(middle).unwrap();
            let n = surface.normal(middle).unwrap();
            near(dot(axis, p), at, 1e-9) && near(dot(axis, n).abs(), 1.0, 1e-9)
        })
        .collect();
    match in_plane.as_slice() {
        &[face] => face,
        other => unreachable!("faces in {axis:?} . p = {at}: {other:?}"),
    }
}

/// A box [0, 30] x [0, 20] x [0, 10] has six faces, and a ray meets the one whose plane it
/// crosses first: from x = -7 along +x the face x = 0 at 7; from y = 26 along -y the face y = 20
/// at 6; from z = 16 along -z the face z = 10 at 6. From inside, at x = 12, a ray along +x meets
/// the face ahead, x = 30 at 18, not x = 0 behind it; along -y from y = 5 it meets y = 0 at 5.
/// Each face is named by its plane, and its (u, v) evaluates back to the point.
#[test]
fn a_ray_at_a_box_hits_the_face_whose_plane_it_crosses_first() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 20.0, 10.0]).unwrap();
    let (record, order) = b.to_record().unwrap();
    assert_eq!(order.faces.len(), 6);
    for (origin, direction, (axis, at), point, distance) in [
        (
            [-7.0, 5.0, 4.0],
            [1.0, 0.0, 0.0],
            (X, 0.0),
            [0.0, 5.0, 4.0],
            7.0,
        ),
        (
            [10.0, 26.0, 4.0],
            [0.0, -1.0, 0.0],
            (Y, 20.0),
            [10.0, 20.0, 4.0],
            6.0,
        ),
        (
            [12.0, 5.0, 16.0],
            [0.0, 0.0, -1.0],
            (Z, 10.0),
            [12.0, 5.0, 10.0],
            6.0,
        ),
        (
            [12.0, 5.0, 4.0],
            [1.0, 0.0, 0.0],
            (X, 30.0),
            [30.0, 5.0, 4.0],
            18.0,
        ),
        (
            [12.0, 5.0, 4.0],
            [0.0, -1.0, 0.0],
            (Y, 0.0),
            [12.0, 0.0, 4.0],
            5.0,
        ),
    ] {
        let hit = b
            .face_hit_by_ray(&order, origin, direction)
            .unwrap()
            .unwrap();
        assert_eq!(
            hit.face,
            box_face(&record, &order, axis, at),
            "{origin:?} {direction:?}: {hit:?}"
        );
        assert!(close(hit.point, point), "{hit:?}");
        assert!(near(hit.distance, distance, 1e-6), "{hit:?}");
        let on = FaceSurface::of(&record, hit.face)
            .unwrap()
            .point(hit.uv)
            .unwrap();
        assert!(close(on, point), "{hit:?} evaluates to {on:?}");
    }
}

/// A ray crossing a face's plane outside its trim does not hit it. On the box [0, 2]^3 the ray
/// from (-1.5, 6, 1) along (3, -4, 0) (unit (0.6, -0.8, 0)) crosses the plane x = 0 at (0, 4, 1),
/// above the face there, and then meets the face y = 2 at (1.5, 2, 1), 5 along it (a 3-4-5
/// triangle). A disk of radius 5 in z = 0 is hit at (2, 1, 0) from either side, 10 from z = 10
/// or z = -10; a ray down through (4, 4), sqrt(32) from the centre, crosses the disk's plane
/// outside the disk and meets nothing.
#[test]
fn a_ray_through_a_faces_plane_outside_its_trim_misses_that_face() {
    all_fixes();
    let b = make_box([0.0; 3], [2.0; 3]).unwrap();
    let (record, order) = b.to_record().unwrap();
    let hit = b
        .face_hit_by_ray(&order, [-1.5, 6.0, 1.0], [3.0, -4.0, 0.0])
        .unwrap()
        .unwrap();
    assert_eq!(hit.face, box_face(&record, &order, Y, 2.0), "{hit:?}");
    assert!(close(hit.point, [1.5, 2.0, 1.0]), "{hit:?}");
    assert!(near(hit.distance, 5.0, 1e-6), "{hit:?}");

    let disk = planar_faces(&[SweepCurve::Arc {
        center: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 5.0,
        start: 0.0,
        end: 2.0 * PI,
    }])
    .unwrap();
    let (_, order) = disk.to_record().unwrap();
    assert_eq!(order.faces.len(), 1);
    for (z, direction) in [(10.0, [0.0, 0.0, -1.0]), (-10.0, [0.0, 0.0, 1.0])] {
        let hit = disk
            .face_hit_by_ray(&order, [2.0, 1.0, z], direction)
            .unwrap()
            .unwrap();
        assert_eq!(hit.face, 0, "{hit:?}");
        assert!(close(hit.point, [2.0, 1.0, 0.0]), "{hit:?}");
        assert!(near(hit.distance, 10.0, 1e-6), "{hit:?}");
    }
    assert_eq!(
        disk.face_hit_by_ray(&order, [4.0, 4.0, 10.0], [0.0, 0.0, -1.0]),
        Ok(None)
    );
}

/// A ray through an edge meets both of its faces at the same distance and returns one of them:
/// on the box [0, 2]^3 the ray from (-3, -4, 1) along (3, 4, 0) passes the edge x = 0, y = 0 at
/// (0, 0, 1), 5 along it; the face is x = 0 or y = 0, whichever, and its (u, v) evaluates to the
/// point on the edge.
#[test]
fn a_ray_through_a_box_edge_hits_one_of_its_two_faces_on_the_edge() {
    all_fixes();
    let b = make_box([0.0; 3], [2.0; 3]).unwrap();
    let (record, order) = b.to_record().unwrap();
    let hit = b
        .face_hit_by_ray(&order, [-3.0, -4.0, 1.0], [3.0, 4.0, 0.0])
        .unwrap()
        .unwrap();
    let faces = [
        box_face(&record, &order, X, 0.0),
        box_face(&record, &order, Y, 0.0),
    ];
    assert!(
        faces.contains(&hit.face),
        "{hit:?} is on neither of {faces:?}"
    );
    assert!(close(hit.point, [0.0, 0.0, 1.0]), "{hit:?}");
    assert!(near(hit.distance, 5.0, 1e-6), "{hit:?}");
    let on = FaceSurface::of(&record, hit.face)
        .unwrap()
        .point(hit.uv)
        .unwrap();
    assert!(close(on, [0.0, 0.0, 1.0]), "{hit:?} evaluates to {on:?}");
}

/// Every face at an edge or a corner, once, at one point and distance, in the record's face order:
/// on the box [0, 2]^3 the ray from (-3, -4, 1) along (3, 4, 0) meets the faces x = 0 and y = 0
/// on their edge at (0, 0, 1), 5 along it, then leaves through y = 2 at 7.5; the ray from
/// (-1, -1, -1) along (1, 1, 1) meets the three faces x = 0, y = 0, z = 0 at the corner (0, 0, 0),
/// sqrt 3 along it, and x = 2, y = 2, z = 2 at (2, 2, 2), 3 sqrt 3 along it (compared squared:
/// ops takes no square roots). `face_hit_by_ray` names one of the first.
#[test]
fn a_ray_through_a_box_edge_or_corner_hits_every_face_there_once() {
    all_fixes();
    let b = make_box([0.0; 3], [2.0; 3]).unwrap();
    let (record, order) = b.to_record().unwrap();
    // each group: its distance squared and its faces
    let cases = [
        (
            [-3.0, -4.0, 1.0],
            [3.0, 4.0, 0.0],
            vec![(25.0, vec![(X, 0.0), (Y, 0.0)]), (56.25, vec![(Y, 2.0)])],
        ),
        (
            [-1.0; 3],
            [1.0; 3],
            vec![
                (3.0, vec![(X, 0.0), (Y, 0.0), (Z, 0.0)]),
                (27.0, vec![(X, 2.0), (Y, 2.0), (Z, 2.0)]),
            ],
        ),
    ];
    for (origin, direction, groups) in cases {
        let hits = b.face_hits_by_ray(&order, origin, direction).unwrap();
        let mut rest = hits.as_slice();
        for (squared, planes) in &groups {
            let mut faces: Vec<usize> = planes
                .iter()
                .map(|&(axis, at)| box_face(&record, &order, axis, at))
                .collect();
            faces.sort_unstable();
            assert!(rest.len() >= faces.len(), "{hits:?}");
            let (here, after) = rest.split_at(faces.len());
            assert_eq!(
                here.iter().map(|h| h.face).collect::<Vec<_>>(),
                faces,
                "{hits:?}"
            );
            for hit in here {
                assert_eq!((hit.point, hit.distance), (here[0].point, here[0].distance));
                assert!(
                    near(hit.distance * hit.distance, *squared, 1e-9),
                    "{hits:?}"
                );
            }
            rest = after;
        }
        assert!(rest.is_empty(), "{hits:?}");
        let first = b
            .face_hit_by_ray(&order, origin, direction)
            .unwrap()
            .unwrap();
        assert!(
            hits.iter()
                .take_while(|h| h.distance == hits[0].distance)
                .any(|h| h.face == first.face),
            "{first:?} {hits:?}"
        );
    }
}

#[test]
fn a_box_face_holds_the_points_on_it_and_no_others() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [30.0, 20.0, 10.0]).unwrap();
    let (record, order) = b.to_record().unwrap();
    let top = (0..record.faces.len())
        .find(|&f| {
            b.point_on_face(&order, f, [15.0, 10.0, 10.0], 1e-6)
                .unwrap()
                .is_some()
        })
        .unwrap();
    let on = b
        .point_on_face(&order, top, [15.0, 10.0, 10.0], 1e-6)
        .unwrap()
        .unwrap();
    assert!(
        on.iter()
            .zip([15.0, 10.0, 10.0])
            .all(|(a, e)| (a - e).abs() < 1e-6),
        "{on:?}"
    );
    // in the face's plane but off its trim, and off its plane
    assert_eq!(
        b.point_on_face(&order, top, [45.0, 10.0, 10.0], 1e-6)
            .unwrap(),
        None
    );
    assert_eq!(
        b.point_on_face(&order, top, [15.0, 10.0, 12.0], 1e-6)
            .unwrap(),
        None
    );
    assert!(b
        .point_on_face(&order, record.faces.len(), [15.0, 10.0, 10.0], 1e-6)
        .is_err());
}

/// The record face of `b` that holds `p`.
fn face_holding(b: &Body, order: &Order, p: [f64; 3]) -> usize {
    (0..order.faces.len())
        .find(|&f| b.point_on_face(order, f, p, 1e-6).unwrap().is_some())
        .unwrap()
}

/// A cut across a sheet's edge runs past it: points 3 mm above the square sheet [0, 20]^2 at
/// z = 0, from over its middle to 200 mm past its edge at x = 20, are 3 mm off the sheet's
/// plane all along, where the trimmed face reads the overshoot past the edge, hypot(d, 3) at
/// d past it (compared squared, d^2 + 9).
#[test]
fn points_above_a_sheet_and_past_its_edge_read_their_height_off_its_surface() {
    all_fixes();
    let corners = [
        [0.0, 0.0, 0.0],
        [20.0, 0.0, 0.0],
        [20.0, 20.0, 0.0],
        [0.0, 20.0, 0.0],
    ];
    let sides: Vec<SweepCurve> = (0..4)
        .map(|i| SweepCurve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let b = planar_faces(&sides).unwrap();
    let (_, order) = b.to_record().unwrap();
    let points: Vec<[f64; 3]> = (0..=21)
        .map(|i| [10.0 + 10.0 * f64::from(i), 10.0, 3.0])
        .collect();
    let gaps = b.face_surface_gaps(&order, 0, &points).unwrap();
    let trimmed = b.face_distances(&order, 0, &points).unwrap();
    for ((p, gap), trimmed) in points.iter().zip(&gaps).zip(&trimmed) {
        assert!(gap.is_some_and(|g| near(g, 3.0, 1e-9)), "{p:?}: {gap:?}");
        let past = (p[0] - 20.0).max(0.0);
        assert!(
            trimmed.is_some_and(|t| near(t * t, past * past + 9.0, 1e-6)),
            "{p:?}: {trimmed:?}"
        );
    }
}

/// The unit directions in the plane whose coordinates are rational, from the Pythagorean
/// triples 3-4-5, 5-12-13, 8-15-17 and 20-99-101, both ways round and in all four quadrants,
/// with the axes: 36 directions round the turn, two of them (99/101, ±20/101) about 0.2 rad
/// either side of +x.
fn rational_directions() -> Vec<[f64; 2]> {
    let mut d = vec![[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]];
    for (a, b, c) in [
        (3.0, 4.0, 5.0),
        (5.0, 12.0, 13.0),
        (8.0, 15.0, 17.0),
        (20.0, 99.0, 101.0),
    ] {
        for (x, y) in [(a / c, b / c), (b / c, a / c)] {
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                d.push([sx * x, sy * y]);
            }
        }
    }
    d
}

/// Points round a closed surface, across its seam wherever it lies, read their true distance
/// off it: on the side of a cylinder of radius 5 and height 10 about +z, points at radius 8 and
/// at radius 2 in 36 directions round the axis ([`rational_directions`]) read 3, and a point of
/// its axis reads 5 (a whole circle of the side nearest it); on a sphere of radius 2, points at
/// radius 7 read 5: its poles and the axes' points, and the 48 points (±2, ±3, ±6) with their
/// coordinates in every order (2^2 + 3^2 + 6^2 = 7^2).
#[test]
fn points_round_a_closed_surface_read_their_distance_across_its_seam() {
    all_fixes();
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let (_, order) = c.to_record().unwrap();
    let side = face_holding(&c, &order, [5.0, 0.0, 5.0]);
    let ring = |r: f64, z: f64| -> Vec<[f64; 3]> {
        rational_directions()
            .iter()
            .map(|[x, y]| [r * x, r * y, z])
            .collect()
    };
    for (r, z) in [(8.0, 5.0), (2.0, 5.0), (8.0, 0.5), (8.0, 9.5)] {
        let gaps = c.face_surface_gaps(&order, side, &ring(r, z)).unwrap();
        for (i, gap) in gaps.iter().enumerate() {
            assert!(
                gap.is_some_and(|g| near(g, 3.0, 1e-9)),
                "radius {r} height {z} step {i}: {gap:?}"
            );
        }
    }
    let axis = c
        .face_surface_gaps(&order, side, &[[0.0, 0.0, 5.0]])
        .unwrap();
    assert!(axis[0].is_some_and(|g| near(g, 5.0, 1e-9)), "{axis:?}");

    let s = make_sphere([0.0; 3], 2.0).unwrap();
    let (_, order) = s.to_record().unwrap();
    let mut points = vec![
        [0.0, 0.0, 7.0],
        [0.0, 0.0, -7.0],
        [7.0, 0.0, 0.0],
        [-7.0, 0.0, 0.0],
        [0.0, 7.0, 0.0],
        [0.0, -7.0, 0.0],
    ];
    for [a, b, c] in [
        [2.0, 3.0, 6.0],
        [2.0, 6.0, 3.0],
        [3.0, 2.0, 6.0],
        [3.0, 6.0, 2.0],
        [6.0, 2.0, 3.0],
        [6.0, 3.0, 2.0],
    ] {
        for [sx, sy, sz] in [
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
            [1.0, -1.0, 1.0],
            [-1.0, -1.0, 1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [1.0, -1.0, -1.0],
            [-1.0, -1.0, -1.0],
        ] {
            points.push([sx * a, sy * b, sz * c]);
        }
    }
    let gaps = s.face_surface_gaps(&order, 0, &points).unwrap();
    for (p, gap) in points.iter().zip(&gaps) {
        assert!(gap.is_some_and(|g| near(g, 5.0, 1e-9)), "{p:?}: {gap:?}");
    }
}

/// Points beside a surface read `None`. The ruled sheet between the lines (0, 0, 0)-(10, 0, 0)
/// and (0, 10, 0)-(10, 10, 5) is the bilinear patch S(s, t) = (10 s, 10 t, 5 s t), no analytic
/// surface, bounded by its domain [0, 1]^2. Off its sides the nearest point of it is on a side,
/// held there and not square to the patch: (20, 5, 0) past s = 1, (5, -8, 3) past t = 0 (its
/// nearest point (5, 0, 0), where `(0, -8, 3) . S_t = -72.5`), (-6, 5, -2) past s = 0. Over its
/// middle (5, 5, 1.25), 3 along the unit normal `(-1, -1, 4) / sqrt(18)`, that is
/// `(-1, -1, 4) / sqrt(2)`, the point reads 3.
///
/// The tube of radius 5 from z = 0 to z = 10 ([`tube`]) reads 3 at points 3 off it across its
/// seam at (5, 0) (1e-9 either side, and at (99/101, ±20/101) times 8) and square over its end
/// circle; the analytic cylinder of the same size is its definition, unbounded along its axis,
/// so the points off its ends, 8 or 2 from its axis, read 3.
#[test]
fn points_beside_a_surface_read_none() {
    all_fixes();
    let rule = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let sheet = ruled(
        &rule([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
        &rule([0.0, 10.0, 0.0], [10.0, 10.0, 5.0]),
        false,
    )
    .unwrap();
    let (patch, _) = Body::from_record(&sheet, false).unwrap();
    let (_, order) = patch.to_record().unwrap();
    let beside = [[20.0, 5.0, 0.0], [5.0, -8.0, 3.0], [-6.0, 5.0, -2.0]];
    let gaps = patch.face_surface_gaps(&order, 0, &beside).unwrap();
    assert_eq!(gaps, vec![None; beside.len()]);
    let unit = std::f64::consts::FRAC_1_SQRT_2;
    let over = [5.0 - unit, 5.0 - unit, 1.25 + 4.0 * unit];
    let gap = patch.face_surface_gaps(&order, 0, &[over]).unwrap();
    assert!(gap[0].is_some_and(|g| near(g, 3.0, 1e-9)), "{gap:?}");

    let (t, _, order) = tube();
    let seam = [
        [8.0, 0.0, 10.0],
        [8.0, 1e-9, 5.0],
        [8.0, -1e-9, 5.0],
        [8.0 * 99.0 / 101.0, 8.0 * 20.0 / 101.0, 2.0],
        [8.0 * 99.0 / 101.0, -8.0 * 20.0 / 101.0, 2.0],
    ];
    let over = t.face_surface_gaps(&order, 0, &seam).unwrap();
    for (p, gap) in seam.iter().zip(&over) {
        assert!(gap.is_some_and(|g| near(g, 3.0, 1e-9)), "{p:?}: {gap:?}");
    }

    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let (_, order) = c.to_record().unwrap();
    let side = face_holding(&c, &order, [5.0, 0.0, 5.0]);
    let off_the_ends = [
        [8.0, 0.0, 13.0],
        [0.0, 8.0, 10.5],
        [-2.0, 0.0, 12.0],
        [0.0, -8.0, -4.0],
    ];
    let gaps = c.face_surface_gaps(&order, side, &off_the_ends).unwrap();
    for (p, gap) in off_the_ends.iter().zip(&gaps) {
        assert!(gap.is_some_and(|g| near(g, 3.0, 1e-9)), "{p:?}: {gap:?}");
    }
}

/// A point that is not finite, or a face past the faces, is refused.
#[test]
fn face_surface_gaps_refuse_hostile_input() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = b.to_record().unwrap();
    for p in [
        [f64::NAN, 0.0, 0.0],
        [0.0, f64::INFINITY, 0.0],
        [0.0, 0.0, f64::NEG_INFINITY],
    ] {
        assert_eq!(
            b.face_surface_gaps(&order, 0, &[[1.0; 3], p]),
            Err(Error::Argument)
        );
    }
    assert_eq!(
        b.face_surface_gaps(&order, order.faces.len(), &[[1.0; 3]]),
        Err(Error::Argument)
    );
    assert_eq!(
        b.face_surface_gaps(&order, 0, &[]).unwrap(),
        Vec::<Option<f64>>::new()
    );
}

/// A ray fired at a body meets its exact faces: straight down onto a 10 mm box, the top face at
/// distance 10; down beside a sphere of radius 2 about (1, 2, 3) at 1 from its axis, the sphere
/// at z = 3 + √3, a point of the surface, not of a facet; through an edge of the box, that edge;
/// past the box, nothing; a coordinate that is not finite or a direction of no length is refused.
#[test]
fn a_fired_ray_meets_the_exact_face_edge_or_nothing() {
    use solidcore_ops::RayEntity;
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, order) = b.to_record().unwrap();
    let top = b
        .fire_ray(&order, [3.0, 4.0, 20.0], [0.0, 0.0, -2.0])
        .unwrap()
        .unwrap();
    assert!(near(top.distance, 10.0, 1e-12), "{top:?}");
    let RayEntity::Face(f) = top.entity else {
        panic!("{top:?}")
    };
    let n = b.face_plane_normal(&order, f).unwrap().unwrap().value;
    assert!(near(n[2], 1.0, 1e-12), "the top face's normal {n:?}");
    let edge = b
        .fire_ray(&order, [10.0, 5.0, 20.0], [0.0, 0.0, -1.0])
        .unwrap()
        .unwrap();
    assert!(
        matches!(edge.entity, RayEntity::Edge(_)) && near(edge.distance, 10.0, 1e-9),
        "{edge:?}"
    );
    assert_eq!(
        b.fire_ray(&order, [30.0, 5.0, 20.0], [0.0, 0.0, -1.0])
            .unwrap(),
        None
    );
    assert_eq!(
        b.fire_ray(&order, [3.0, 4.0, 20.0], [0.0, 0.0, 1.0])
            .unwrap(),
        None
    );
    assert_eq!(
        b.fire_ray(&order, [f64::NAN, 4.0, 20.0], [0.0, 0.0, -1.0]),
        Err(Error::Argument)
    );
    assert_eq!(
        b.fire_ray(&order, [3.0, 4.0, 20.0], [0.0; 3]),
        Err(Error::Argument)
    );

    let s = make_sphere([1.0, 2.0, 3.0], 2.0).unwrap();
    let (_, order) = s.to_record().unwrap();
    let hit = s
        .fire_ray(&order, [1.0, 3.0, 20.0], [0.0, 0.0, -1.0])
        .unwrap()
        .unwrap();
    assert!(matches!(hit.entity, RayEntity::Face(_)), "{hit:?}");
    assert!(
        near(20.0 - hit.distance, 3.0 + 1.732_050_807_568_877_2, 1e-9),
        "{hit:?}"
    );
}

/// Along each straight edge of a 1 x 2 x 3 box, `edge_points` gives `n` points evenly inside
/// the edge: one point is `edge_middle`'s, and of three points the middle one is at the edge's
/// middle and each is a quarter of the edge's length from its neighbours, so none is at an end.
/// `EDGE_POINTS_MAX` points all lie inside the edge. A count of 0 or past `EDGE_POINTS_MAX`,
/// and an edge the body does not have, are refused.
#[test]
fn edge_points_spread_evenly_inside_an_edge_and_refuse_a_bad_count() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let (_, order) = b.to_record().unwrap();
    // Squared distances: each check below compares them with a squared length.
    let dist2 =
        |p: [f64; 3], q: [f64; 3]| p.iter().zip(q).map(|(a, b)| (a - b) * (a - b)).sum::<f64>();
    let most = EDGE_POINTS_MAX as usize;
    for edge in 0..12 {
        let middle = b.edge_middle(&order, edge).unwrap();
        let length = b.edge_length(&order, edge, 1e-9).unwrap();
        let one = b.edge_points(&order, edge, 1).unwrap();
        assert_eq!(one.len(), 1);
        assert!(close(one[0], middle), "{one:?} vs {middle:?}");
        let three = b.edge_points(&order, edge, 3).unwrap();
        assert_eq!(three.len(), 3);
        assert!(close(three[1], middle), "{three:?} vs {middle:?}");
        for w in three.windows(2) {
            assert!(
                near(dist2(w[0], w[1]), length * length / 16.0, 1e-9),
                "{three:?}"
            );
        }
        let many = b.edge_points(&order, edge, most).unwrap();
        assert_eq!(many.len(), most);
        for p in &many {
            // Inside the edge: nearer the middle than half its length, never at an end.
            let reach = length / 2.0 - length / 200.0;
            assert!(
                dist2(*p, middle) < reach * reach,
                "{p:?} is at or past an end"
            );
        }
        for bad in [0, most + 1, usize::MAX] {
            assert!(
                matches!(b.edge_points(&order, edge, bad), Err(Error::Argument)),
                "a count of {bad}"
            );
        }
    }
    assert!(matches!(b.edge_points(&order, 12, 3), Err(Error::Argument)));
}

/// On a cylinder of radius 5 and height 10 about the z axis, every edge lies on its side: each
/// point `edge_points` gives is 5 from the axis and between z 0 and 10. On each rim circle (an
/// edge whose points share one z) the points are all different, none is at the rim's ends, and
/// the one point of a count of 1, and the middle one of 3, are `edge_middle`'s.
#[test]
fn edge_points_lie_on_the_circles_of_a_cylinder() {
    all_fixes();
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let (record, order) = c.to_record().unwrap();
    let mut rims = 0;
    for edge in 0..record.edges.len() {
        let points = c.edge_points(&order, edge, 9).unwrap();
        assert_eq!(points.len(), 9);
        for p in &points {
            assert!(near(p[0] * p[0] + p[1] * p[1], 25.0, 1e-9), "{p:?}");
            assert!(p[2] > -1e-9 && p[2] < 10.0 + 1e-9, "{p:?}");
        }
        if !points.iter().all(|p| (p[2] - points[0][2]).abs() <= 1e-9) {
            continue;
        }
        rims += 1;
        for (i, p) in points.iter().enumerate() {
            for q in points.iter().skip(i + 1) {
                assert!(!close(*p, *q), "{points:?}");
            }
        }
        let middle = c.edge_middle(&order, edge).unwrap();
        assert!(close(c.edge_points(&order, edge, 1).unwrap()[0], middle));
        assert!(close(c.edge_points(&order, edge, 3).unwrap()[1], middle));
    }
    assert_eq!(rims, 2, "the cylinder's two rims");
}

/// The line helpers the app's hole start uses: a direction made unit (and refused when it has no
/// length or a bad coordinate), the nearer of two meetings with ties going ahead, a point along a
/// line, and a distance measured across a surface.
#[test]
fn line_helpers_make_unit_choose_the_nearer_meeting_and_measure_across() {
    assert_eq!(
        solidcore_ops::unit_direction([0.0, 0.0, 5.0]),
        Some([0.0, 0.0, 1.0])
    );
    assert_eq!(solidcore_ops::unit_direction([0.0; 3]), None);
    assert_eq!(solidcore_ops::unit_direction([f64::NAN, 0.0, 1.0]), None);
    assert_eq!(
        solidcore_ops::nearer_line_meeting(Some(3.0), Some(2.0)),
        Some(-2.0)
    );
    assert_eq!(
        solidcore_ops::nearer_line_meeting(Some(2.0), Some(2.0)),
        Some(2.0)
    );
    assert_eq!(
        solidcore_ops::nearer_line_meeting(None, Some(4.0)),
        Some(-4.0)
    );
    assert_eq!(solidcore_ops::nearer_line_meeting(None, None), None);
    assert_eq!(
        solidcore_ops::point_along_line([1.0, 2.0, 3.0], [0.0, 0.0, 1.0], -2.0),
        Some([1.0, 2.0, 1.0])
    );
    // Off a surface along its normal (z): only the z gap counts across it; on it, plain distance.
    let across =
        solidcore_ops::distance_across_surface([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [3.0, 0.0, 3.0])
            .unwrap();
    assert!((across - 2.0).abs() < 1e-12, "{across}");
    let plain =
        solidcore_ops::distance_across_surface([0.0; 3], [0.0; 3], [3.0, 4.0, 0.0]).unwrap();
    assert!((plain - 5.0).abs() < 1e-12, "{plain}");
}

/// A body that names its geometry owners by keys the check cannot know (its naming scheme
/// `Unknown`) is refused by the check, scoped or not: the check runs on a copy, named afresh,
/// and the refusal reads the body's own scheme. A body named by its maker is checked.
#[test]
fn a_body_of_unknown_naming_is_refused_by_the_check() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    b.kernel()
        .set_naming(solidcore::topology::NamingScheme::Unknown);
    assert!(matches!(
        b.check(solidcore_ops::check::CheckLevel::All),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        b.check_scoped(
            solidcore_ops::check::CheckLevel::All,
            solidcore_ops::check::ValidityScope::Stitched
        ),
        Err(Error::Unsupported(_))
    ));
    b.kernel()
        .set_naming(solidcore::topology::NamingScheme::Caller);
    assert!(b.check(solidcore_ops::check::CheckLevel::All).is_ok());
}

/// `Body::curve_face_gap_bound` bounds a spline and a conic on a face near 0, bounds a lifted
/// spline at its shift, rejects out-of-range face indices, returns `None` for invalid interval
/// or floor, and honours cancellation.
#[test]
fn curve_face_gap_bound_bounds_splines_and_conics_and_honours_cancel() {
    use solidcore_ops::cancel::{self, CancelToken};
    use solidcore_ops::curve_gap::GapCurve;
    use solidcore_record::brep::Frame;

    all_fixes();
    let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let order = b.own_order();
    assert_eq!(order.faces.len(), 6);

    let on_top = GapCurve::new(&Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[2.0, 0.0, 33.0], [10.0, 16.0, 33.0]],
    })
    .unwrap();
    let bounds_all: Vec<f64> = (0..6)
        .map(|f| {
            b.curve_face_gap_bound(&order, f, &on_top, [0.0, 1.0], 1e-9)
                .unwrap()
                .unwrap()
        })
        .collect();
    let zero_faces: Vec<usize> = bounds_all
        .iter()
        .enumerate()
        .filter_map(|(f, &d)| (d < 1e-9).then_some(f))
        .collect();
    assert_eq!(zero_faces.len(), 1, "exactly the top face: {bounds_all:?}");
    let top = zero_faces[0];

    let lifted = GapCurve::new(&Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[2.0, 0.0, 35.5], [10.0, 16.0, 35.5]],
    })
    .unwrap();
    let gap = b
        .curve_face_gap_bound(&order, top, &lifted, [0.0, 1.0], 1e-9)
        .unwrap()
        .unwrap();
    assert!((2.5..=2.5 + 1e-6).contains(&gap), "lifted gap {gap}");

    // Bisection stops once a piece's remainder is at most `floor`, so `floor = 1e-10` proves a
    // zero-gap quarter circle within 1e-9 (remainder <= 1e-10 plus evaluation rounding).
    let circle = Curve::Circle {
        frame: Frame {
            origin: [6.0, 8.0, 33.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        radius: 2.0,
    };
    let (arc, arc_ivl) = GapCurve::of_conic(&circle, [0.0, std::f64::consts::FRAC_PI_2])
        .unwrap()
        .unwrap();
    let arc_gap = b
        .curve_face_gap_bound(&order, top, &arc, arc_ivl, 1e-10)
        .unwrap()
        .unwrap();
    assert!(arc_gap < 1e-9, "quarter circle on top face: {arc_gap}");

    // A quarter circle rising off the top face to z = 35 at theta = pi/2 proves the full
    // converted spline interval is measured (not truncated to `[0, pi/2]` in degree domain,
    // which would give ~0.055 mm). Pieces close at `CLOSE_RELATIVE = 1e-3` of end distance,
    // so the proved upper bound lies in `[2.0, 2.0 * (1 + 2e-3)]`.
    let vertical_circle = Curve::Circle {
        frame: Frame {
            origin: [6.0, 8.0, 33.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 0.0, 1.0],
        },
        radius: 2.0,
    };
    let (v_arc, v_arc_ivl) =
        GapCurve::of_conic(&vertical_circle, [0.0, std::f64::consts::FRAC_PI_2])
            .unwrap()
            .unwrap();
    let v_gap = b
        .curve_face_gap_bound(&order, top, &v_arc, v_arc_ivl, 1e-9)
        .unwrap()
        .unwrap();
    assert!(
        (2.0..=2.0 * (1.0 + 2.0e-3)).contains(&v_gap),
        "vertical arc {v_gap}"
    );

    assert_eq!(
        b.curve_face_gap_bound(&order, 6, &on_top, [0.0, 1.0], 1e-9),
        Err(Error::Argument)
    );
    assert_eq!(
        b.curve_face_gap_bound(&order, usize::MAX, &on_top, [0.0, 1.0], 1e-9),
        Err(Error::Argument)
    );
    for bad_ivl in [
        [1.0, 0.0],
        [1.0, 1.0],
        [f64::NAN, 1.0],
        [f64::NEG_INFINITY, 1.0],
    ] {
        assert_eq!(
            b.curve_face_gap_bound(&order, top, &on_top, bad_ivl, 1e-9),
            Err(Error::Argument),
            "interval {bad_ivl:?}"
        );
    }
    for bad_floor in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            b.curve_face_gap_bound(&order, top, &on_top, [0.0, 1.0], bad_floor),
            Err(Error::Argument),
            "floor {bad_floor}"
        );
    }

    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        cancel::within(&token, || {
            b.curve_face_gap_bound(&order, top, &on_top, [0.0, 1.0], 1e-9)
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
}

/// `Body::face_internal_point` and `Body::face_internal_points` return a point strictly inside
/// each face of a box and of a holed plate, match per-face calls bit for bit, reject bad indices,
/// handle empty orders, and stop mid-loop when cancelled.
#[test]
fn face_internal_points_lie_inside_each_face_and_honour_cancel() {
    use solidcore_ops::cancel::{self, CancelToken};

    all_fixes();
    let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let order = b.own_order();
    let pts = b.face_internal_points(&order).unwrap();
    assert_eq!(pts.len(), 6);

    let lo = [1.0, -2.0, 3.0];
    let hi = [11.0, 18.0, 33.0];
    let mut sides = std::collections::BTreeSet::new();
    for (f, &p) in pts.iter().enumerate() {
        assert_eq!(b.face_internal_point(&order, f).unwrap(), p, "face {f}");
        assert!(p.iter().all(|x| x.is_finite()), "{p:?}");
        assert_eq!(
            b.classify_point(p).unwrap(),
            PointClass::OnBoundary,
            "{p:?}"
        );
        assert!(
            b.point_on_face(&order, f, p, 1e-9).unwrap().is_some(),
            "face {f} point {p:?}"
        );
        let mut on = None;
        for axis in 0..3 {
            assert!(
                (lo[axis] - 1e-9..=hi[axis] + 1e-9).contains(&p[axis]),
                "{p:?}"
            );
            if (p[axis] - lo[axis]).abs() < 1e-9 {
                assert!(on.replace((axis, false)).is_none(), "edge/corner {p:?}");
            } else if (p[axis] - hi[axis]).abs() < 1e-9 {
                assert!(on.replace((axis, true)).is_none(), "edge/corner {p:?}");
            } else {
                assert!(
                    p[axis] > lo[axis] + 1e-6 && p[axis] < hi[axis] - 1e-6,
                    "too close to edge: {p:?}"
                );
            }
        }
        assert!(
            sides.insert(on.expect("point on a box face")),
            "duplicate face"
        );
    }
    assert_eq!(sides.len(), 6);

    assert_eq!(b.face_internal_point(&order, 6), Err(Error::Argument));
    assert_eq!(
        b.face_internal_point(&order, usize::MAX),
        Err(Error::Argument)
    );
    let mut empty = order.clone();
    empty.faces.clear();
    assert_eq!(
        b.face_internal_points(&empty).unwrap(),
        Vec::<[f64; 3]>::new()
    );

    // Pre-cancelled token refuses both single and batch queries.
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        cancel::within(&token, || b.face_internal_point(&order, 0)),
        Err(Error::Cancelled)
    );
    assert_eq!(
        cancel::within(&token, || b.face_internal_points(&order)),
        Err(Error::Cancelled)
    );
    // Mid-loop cancellation: count exact polls on the warmed box to prove `face_internal_points`
    // polls once per face inside its loop (beyond `run_with` entry and `kept` exit) and stops
    // before face 1 when cancelled at that poll.
    let mut single_kernel_polls = 0u64;
    let mut first_face_polls = 0u64;
    for f in 0..6 {
        let p = CancelToken::after_polls(u64::MAX);
        cancel::within(&p, || b.face_internal_point(&order, f)).unwrap();
        let k_polls = p.polls().unwrap().saturating_sub(2);
        if f == 0 {
            first_face_polls = k_polls;
        }
        single_kernel_polls += k_polls;
    }
    let probe = CancelToken::after_polls(u64::MAX);
    cancel::within(&probe, || b.face_internal_points(&order)).unwrap();
    let whole = probe.polls().unwrap();
    assert_eq!(whole, 8 + single_kernel_polls, "batch polls {whole}");
    let cut = 3 + first_face_polls;
    assert!(cut + 1 < whole, "cut {cut} vs whole {whole}");
    let limited = CancelToken::after_polls(cut);
    assert_eq!(
        cancel::within(&limited, || b.face_internal_points(&order)),
        Err(Error::Cancelled)
    );
    assert_eq!(limited.polls(), Some(cut + 1));
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);

    // Holed plate (20 x 20 x 5 box minus radius-3 cylinder): 7 faces, 15 edges, 10 vertices.
    let block = make_box([0.0; 3], [20.0, 20.0, 5.0]).unwrap();
    let pin = make_cylinder([10.0, 10.0, -1.0], 3.0, 7.0).unwrap();
    let plate = solidcore_ops::boolean::boolean(block, pin, solidcore_ops::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    assert_eq!(plate.counts().unwrap(), [7, 15, 10]);
    let plate_order = plate.own_order();
    let plate_pts = plate.face_internal_points(&plate_order).unwrap();
    assert_eq!(plate_pts.len(), 7);
    let mut plate_faces = std::collections::BTreeSet::new();
    for (f, &p) in plate_pts.iter().enumerate() {
        assert_eq!(plate.face_internal_point(&plate_order, f).unwrap(), p);
        assert!(p.iter().all(|x| x.is_finite()), "{p:?}");
        assert_eq!(
            plate.classify_point(p).unwrap(),
            PointClass::OnBoundary,
            "{p:?}"
        );
        // Strictly inside face `f` alone: not on any edge or vertex shared with another face.
        let matching: Vec<usize> = (0..7)
            .filter(|&other| {
                plate
                    .point_on_face(&plate_order, other, p, 1e-6)
                    .unwrap()
                    .is_some()
            })
            .collect();
        assert_eq!(matching, vec![f], "face {f} point {p:?}");
        let [x, y, z] = p;
        let dx = x - 10.0;
        let dy = y - 10.0;
        let r2 = dx * dx + dy * dy;
        let hole_lo = (3.0 - 1e-6) * (3.0 - 1e-6);
        let hole_hi = (3.0 + 1e-6) * (3.0 + 1e-6);
        let in_x = x > 1e-6 && x < 20.0 - 1e-6;
        let in_y = y > 1e-6 && y < 20.0 - 1e-6;
        let in_z = z > 1e-6 && z < 5.0 - 1e-6;
        let tag = if x.abs() < 1e-6 && in_y && in_z {
            0
        } else if (x - 20.0).abs() < 1e-6 && in_y && in_z {
            1
        } else if y.abs() < 1e-6 && in_x && in_z {
            2
        } else if (y - 20.0).abs() < 1e-6 && in_x && in_z {
            3
        } else if z.abs() < 1e-6 && in_x && in_y && r2 > hole_hi {
            4
        } else if (z - 5.0).abs() < 1e-6 && in_x && in_y && r2 > hole_hi {
            5
        } else if (hole_lo..=hole_hi).contains(&r2) && in_z {
            6
        } else {
            panic!("point {p:?} not strictly inside any face of the holed plate");
        };
        assert!(plate_faces.insert(tag), "duplicate plate face {tag}: {p:?}");
    }
    assert_eq!(plate_faces.len(), 7);
}
