//! Analytic regressions for lofted passages (#2435).

use solidcraft_geom::prelude::*;
use std::f64::consts::PI;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::{Body, loft_regions, measure, step_export, step_import};

fn square(half: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(-half, -half), Vec2::new(half, -half), Vec2::new(half, half), Vec2::new(-half, half)])
}

fn check(body: &Body, volume: f64, tol: f64) {
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    assert_eq!(body.shell_count(), 1);
    let got = measure(body).unwrap().volume;
    assert!((got - volume).abs() < tol * volume, "{got} vs {volume}");
}

#[test]
fn loft_regions_preserve_a_tapered_round_passage_and_step() {
    let ring = |r, h| Region2 { outer: Loop2::circle(Vec2::ZERO, r), holes: vec![Loop2::circle(Vec2::ZERO, h)] };
    let secs = [(Plane::XY, ring(20.0, 12.0)), (Plane::XY.offset(30.0), ring(14.0, 8.0))];
    let want = PI * 30.0 / 3.0 * ((400.0 + 280.0 + 196.0) - (144.0 + 96.0 + 64.0));
    for secs in [secs.to_vec(), secs.into_iter().rev().collect()] {
        let body = loft_regions(&secs).unwrap();
        check(&body, want, 1e-3);
        assert!(!body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap().contains(Vec3::new(0.0, 0.0, 10.0)), "passage was filled");
        let imported = step_import(&step_export(&[&body], "SolidCraft").unwrap()).unwrap();
        assert_eq!(imported.bodies.len(), 1);
        check(&imported.bodies[0].body, want, 1e-3);
    }
}

#[test]
fn loft_regions_smooth_inner_and_outer_walls_together() {
    // Half-width 10 + 10 (2t-1)²; the hole is half as wide. Integral of the
    // material cross-section over 40 mm is 40 * 3 * (100 + 200/3 + 100/5).
    let ring = |r| Region2 { outer: square(r), holes: vec![square(r * 0.5)] };
    let secs = [(Plane::XY, ring(20.0)), (Plane::XY.offset(20.0), ring(10.0)), (Plane::XY.offset(40.0), ring(20.0))];
    for secs in [secs.to_vec(), secs.into_iter().rev().collect()] {
        let b = loft_regions(&secs).unwrap();
        check(&b, 22400.0, 1e-4);
        assert!(!b.mesh(&crate::MeshTolerance::chord(0.001)).unwrap().contains(Vec3::new(0.0, 0.0, 10.0)), "passage was filled");
        assert_eq!(measure(&b).unwrap().merged.faces, 10, "four outer walls, four inner walls, two caps");
    }
}

#[test]
fn loft_regions_match_holes_in_opposite_curve_orders_and_facing_planes() {
    let holes = vec![Loop2::circle(Vec2::new(-8.0, 0.0), 3.0), Loop2::circle(Vec2::new(8.0, 0.0), 5.0)];
    let a = Region2 { outer: square(20.0), holes: holes.clone() };
    // Reversing the plane's x direction mirrors local x. Opposite normals must
    // leave the same physical holes and inward-facing walls.
    let down = Plane::new(Vec3::new(0.0, 0.0, 20.0), -Vec3::X, Vec3::Y).unwrap();
    let b = Region2 { outer: square(20.0), holes: vec![Loop2::circle(Vec2::new(-8.0, 0.0), 5.0), Loop2::circle(Vec2::new(8.0, 0.0), 3.0)] };
    let secs = [(Plane::XY, a), (down, b)];
    for secs in [secs.to_vec(), secs.into_iter().rev().collect()] {
        let body = loft_regions(&secs).unwrap();
        check(&body, (1600.0 - PI * 34.0) * 20.0, 1e-3);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
        assert!(!mesh.contains(Vec3::new(-8.0, 0.0, 10.0)));
        assert!(!mesh.contains(Vec3::new(8.0, 0.0, 10.0)));
        assert!(mesh.contains(Vec3::new(0.0, 12.0, 10.0)));
        let history = body.history().unwrap();
        // Both holes keep the original section/segment numbering, even after pairing.
        for segment in 0..(8 + holes.iter().map(|h| h.segs.len()).sum::<usize>() * 2) {
            assert!(history.faces.iter().any(|f| f.origins.contains(&crate::history::FaceOrigin::Profile { segment })), "{segment}");
        }
    }
}

#[test]
fn loft_regions_refuse_missing_holes_and_hostile_sections() {
    let a = Region2 { outer: square(20.0), holes: vec![square(10.0)] };
    let b = Region2 { outer: square(20.0), holes: vec![] };
    for secs in [vec![], vec![(Plane::XY, a.clone())], vec![(Plane::XY, a.clone()); 51]] {
        assert!(loft_regions(&secs).is_err());
    }
    assert!(loft_regions(&[(Plane::XY, a.clone()), (Plane::XY.offset(20.0), b)]).unwrap_err().to_string().contains("same number of holes"));
    let many = Region2 { outer: square(20.0), holes: vec![square(1.0); 129] };
    assert!(loft_regions(&[(Plane::XY, many.clone()), (Plane::XY.offset(20.0), many)]).is_err());
    let bad = Region2 { outer: square(20.0), holes: vec![Loop2::circle(Vec2::ZERO, f64::NAN)] };
    assert!(loft_regions(&[(Plane::XY, a), (Plane::XY.offset(20.0), bad)]).is_err());
}

#[test]
fn loft_regions_bound_segments_after_matching() {
    // Each sketch fits the input budget, but independently resampling both holes
    // would exceed it in every section. Refuse before allocating the topology.
    let circle = |x, n| Loop2 {
        segs: (0..n)
            .map(|i| solidcore_record::Seg2::Arc {
                center: Vec2::new(x, 0.0),
                radius: 1.0,
                start: i as f64 * std::f64::consts::TAU / n as f64,
                sweep: std::f64::consts::TAU / n as f64,
            })
            .collect(),
    };
    let a = Region2 { outer: square(20.0), holes: vec![circle(-8.0, 10_001), circle(8.0, 2)] };
    let b = Region2 { outer: square(20.0), holes: vec![circle(-8.0, 2), circle(8.0, 10_001)] };
    let err = loft_regions(&[(Plane::XY, a), (Plane::XY.offset(20.0), b)]).unwrap_err().to_string();
    assert!(err.contains("matching loft sections would create too many segments"), "{err}");
}

#[test]
fn loft_regions_refuse_ambiguous_hole_pairings_in_any_draw_order() {
    let ring = |centres: [Vec2; 2]| Region2 { outer: square(20.0), holes: centres.into_iter().map(|c| Loop2::circle(c, 1.0)).collect() };
    let a = ring([Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0)]);
    // Ties, and two source holes whose unique nearest candidate is the same hole.
    for centres in [[Vec2::new(0.0, -5.0), Vec2::new(0.0, 5.0)], [Vec2::new(4.0, 0.0), Vec2::new(10.0, 0.0)]] {
        for centres in [centres, [centres[1], centres[0]]] {
            let secs = [(Plane::XY, a.clone()), (Plane::XY.offset(20.0), ring(centres))];
            for secs in [secs.to_vec(), secs.into_iter().rev().collect()] {
                let err = loft_regions(&secs).unwrap_err().to_string();
                assert!(err.contains("hole correspondence is ambiguous"), "{err}");
            }
        }
    }
}

/// #13331: a loft between two parallel squares has planar sides, each joining two parallel
/// lines: a tapered square duct (the reporter's hopper) and a square prism alike. The skin
/// gave B-spline walls, which count as "other" and which Convert to Sheet Metal refuses.
#[test]
fn a_loft_between_parallel_squares_has_planar_sides() {
    for top in [100.0, 55.0] {
        let secs =
            [(Plane::XY, Region2 { outer: square(55.0), holes: vec![] }), (Plane::XY.offset(120.0), Region2 { outer: square(top), holes: vec![] })];
        let body = loft_regions(&secs).unwrap();
        let (a, b) = (110.0f64 * 110.0, (2.0 * top) * (2.0 * top));
        check(&body, 120.0 / 3.0 * (a + b + (a * b).sqrt()), 1e-6);
        let types = measure(&body).unwrap().merged.face_types;
        assert_eq!(types.get("plane"), Some(&6), "top {top}: {types:?}");
    }
}

/// The faces of `body` whose surface `measure` names `kind`.
fn faces_of(body: &Body, kind: &str) -> usize {
    measure(body).unwrap().merged.face_types.get(kind).copied().unwrap_or(0)
}

/// #13331: a loft through three parallel squares whose corners line up (half sides 55, 77.5 and
/// 100 at z 0, 60 and 120) is one tapered duct with four flat walls. The skin through three
/// sections made each wall a B-spline surface ("other"), which Convert to Sheet Metal refuses.
#[test]
fn a_loft_through_three_squares_in_line_has_planar_walls() {
    let secs: Vec<(Plane, Region2)> = [(0.0, 55.0), (60.0, 77.5), (120.0, 100.0)]
        .iter()
        .map(|&(z, h)| (Plane::XY.offset(z), Region2 { outer: square(h), holes: vec![] }))
        .collect();
    let body = loft_regions(&secs).unwrap();
    let (a, b) = (110.0f64 * 110.0, 200.0f64 * 200.0);
    check(&body, 120.0 / 3.0 * (a + b + (a * b).sqrt()), 1e-6);
    assert_eq!(faces_of(&body, "plane"), 6, "{:?}", measure(&body).unwrap().merged.face_types);
}

/// Three squares whose corners do not line up (half sides 55, 80, 100): the walls bend between
/// the sections, and stay B-spline surfaces.
#[test]
fn a_loft_through_three_squares_out_of_line_keeps_curved_walls() {
    let secs: Vec<(Plane, Region2)> = [(0.0, 55.0), (60.0, 80.0), (120.0, 100.0)]
        .iter()
        .map(|&(z, h)| (Plane::XY.offset(z), Region2 { outer: square(h), holes: vec![] }))
        .collect();
    let body = loft_regions(&secs).unwrap();
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    assert_eq!((faces_of(&body, "plane"), faces_of(&body, "other")), (2, 4));
}

/// A loft between two D shapes (three lines and a half circle), the top one 0.6 times the
/// bottom: the three walls on the lines are flat, the one on the arcs is not.
#[test]
fn a_loft_of_lines_and_an_arc_has_planar_walls_on_the_lines() {
    let d = |k: f64| {
        let (w, h) = (30.0 * k, 20.0 * k);
        let line = |a: Vec2, b: Vec2| solidcore_record::Seg2::Line { a, b };
        Loop2 {
            segs: vec![
                line(Vec2::new(-w, -h), Vec2::new(w, -h)),
                solidcore_record::Seg2::Arc { center: Vec2::new(w, 0.0), radius: h, start: -PI / 2.0, sweep: PI },
                line(Vec2::new(w, h), Vec2::new(-w, h)),
                line(Vec2::new(-w, h), Vec2::new(-w, -h)),
            ],
        }
    };
    let secs = [(Plane::XY, Region2 { outer: d(1.0), holes: vec![] }), (Plane::XY.offset(50.0), Region2 { outer: d(0.6), holes: vec![] })];
    let body = loft_regions(&secs).unwrap();
    let a = 60.0 * 40.0 + PI * 400.0 / 2.0;
    let b = a * 0.36;
    check(&body, 50.0 / 3.0 * (a + b + (a * b).sqrt()), 1e-6);
    assert_eq!(faces_of(&body, "plane"), 5, "{:?}", measure(&body).unwrap().merged.face_types);
    // Every wall, flat or not, still comes from its segments: the names a saved design picks by.
    let history = body.history().unwrap();
    for segment in 0..8 {
        assert!(history.faces.iter().any(|f| f.origins.contains(&crate::history::FaceOrigin::Profile { segment })), "{segment}");
    }
}
