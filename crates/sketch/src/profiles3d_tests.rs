//! Tests for planar 3D loop detection (#741): the pyramid of the issue report, skew loops,
//! nesting, tolerance, and the no-duplication rule against the 2D profiles.

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::model::{CurveKind, Sketch};
use crate::profiles3d::find_planar_3d_profiles;
use crate::wire::WireSpace;

fn wire(id: &str, pts: &[Vec3]) -> crate::wire::Wire {
    crate::wire::Wire { id: id.into(), pts: pts.to_vec(), space: WireSpace::Component, link: None, fit: Vec::new(), arc: None, exact: Vec::new() }
}

/// The issue's pyramid: a 20×20 square on XY, 3D lines from its corners to an apex at (0,0,15).
fn pyramid() -> Sketch {
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let corners = [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
    let mut pts = Vec::new();
    for c in corners {
        pts.push(sk.add_point(Vec2 { x: c[0], y: c[1] }, None).unwrap());
    }
    for k in 0..4 {
        sk.add_curve(CurveKind::Line { a: pts[k], b: pts[(k + 1) % 4] }, None).unwrap();
    }
    let apex = Vec3 { x: 0.0, y: 0.0, z: 15.0 };
    for (k, c) in corners.iter().enumerate() {
        let c = Vec3 { x: c[0], y: c[1], z: 0.0 };
        sk.wires.push(wire(&format!("w{k}"), &[c, apex]));
    }
    sk
}

/// Triangle area: base 20, the apex 15 above the centre, so the slant height is √(15²+10²) = √325.
const TRI_AREA: f64 = 180.277_563_773_199_46;

fn dist_to_plane(p: &Plane, w: Vec3) -> f64 {
    (p.normal().dot(w - p.origin)).abs()
}

#[test]
fn pyramid_sides_are_four_planar_profiles_in_their_own_planes() {
    let sk = pyramid();
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 4, "one triangle per side: {:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
    let apex = Vec3 { x: 0.0, y: 0.0, z: 15.0 };
    for p in &ps {
        assert!((p.area - TRI_AREA).abs() < 1e-6, "area {}", p.area);
        assert_eq!(p.outer_curves.len(), 3, "two 3D lines and one outline edge: {:?}", p.outer_curves);
        assert!(p.hole_curves.is_empty());
        // The plane holds the apex and the mapped centroid floats above the sketch plane.
        assert!(dist_to_plane(&p.plane, apex) < 1e-6);
        assert!(p.centroid.z > 0.0 && p.centroid.z < 15.0);
    }
    // The square itself is not listed again: it is bounded only by in-plane curves.
    assert!(ps.iter().all(|p| p.area < TRI_AREA + 1.0), "the square must not appear: {:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
}

#[test]
fn the_issue_repro_lists_the_triangle() {
    // Exactly the repro: rectangle, two 3D lines to the apex, one triangle expected.
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let mut pts = Vec::new();
    for c in [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]] {
        pts.push(sk.add_point(Vec2 { x: c[0], y: c[1] }, None).unwrap());
    }
    for k in 0..4 {
        sk.add_curve(CurveKind::Line { a: pts[k], b: pts[(k + 1) % 4] }, None).unwrap();
    }
    let apex = Vec3 { x: 0.0, y: 0.0, z: 15.0 };
    sk.wires.push(wire("w0", &[Vec3 { x: -10.0, y: -10.0, z: 0.0 }, apex]));
    sk.wires.push(wire("w1", &[Vec3 { x: 10.0, y: -10.0, z: 0.0 }, apex]));
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 1);
    assert!((ps[0].area - TRI_AREA).abs() < 1e-6);
}

#[test]
fn a_skew_quadrilateral_is_not_a_profile_but_a_planar_one_is() {
    // Four 3D lines closing a quadrilateral: skew → nothing; flat → one profile.
    for (lift, want) in [(1.0, 0), (0.0, 1)] {
        let mut sk = Sketch::new();
        sk.view.three_d = true;
        let corners =
            [Vec3 { x: 0.0, y: 0.0, z: 0.0 }, Vec3 { x: 10.0, y: 0.0, z: 0.0 }, Vec3 { x: 10.0, y: 10.0, z: 0.0 }, Vec3 { x: 0.0, y: 10.0, z: lift }];
        for k in 0..4 {
            sk.wires.push(wire(&format!("w{k}"), &[corners[k], corners[(k + 1) % 4]]));
        }
        let ps = find_planar_3d_profiles(&sk, Plane::XY);
        assert_eq!(ps.len(), want, "lift {lift}: {:?}", ps.iter().map(|p| p.area).collect::<Vec<_>>());
        if want == 1 {
            assert!((ps[0].area - 100.0).abs() < 1e-6);
        }
    }
}

#[test]
fn a_bent_leg_breaks_the_two_sides_that_use_it() {
    // The pyramid with one apex leg bent through a point off its planes: each triangle uses the
    // legs at both its corners, so the bent leg's two neighbours lose their loop (the bent path
    // contributes no edge lying in either plane); the two far sides stay.
    let mut sk = pyramid();
    sk.wires.retain(|w| w.id != "w2");
    let corner = Vec3 { x: 10.0, y: 10.0, z: 0.0 };
    let apex = Vec3 { x: 0.0, y: 0.0, z: 15.0 };
    sk.wires.push(wire("w2b", &[corner, Vec3 { x: 10.0, y: 8.0, z: 7.0 }, apex]));
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 2, "{:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
}

#[test]
fn a_tilted_loop_with_a_hole_nests() {
    // Two nested rectangles on the tilted plane z = x: one profile with one hole.
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let ring = |sk: &mut Sketch, id: &str, half: f64| {
        let corners = [
            Vec3 { x: -half, y: -half, z: -half },
            Vec3 { x: half, y: -half, z: half },
            Vec3 { x: half, y: half, z: half },
            Vec3 { x: -half, y: half, z: -half },
        ];
        for k in 0..4 {
            sk.wires.push(wire(&format!("{id}{k}"), &[corners[k], corners[(k + 1) % 4]]));
        }
    };
    ring(&mut sk, "o", 10.0);
    ring(&mut sk, "i", 4.0);
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    // As in the sketch plane: the outer with its hole, and the inner loop on its own.
    assert_eq!(ps.len(), 2, "{:?}", ps.iter().map(|p| (p.area, p.hole_curves.len())).collect::<Vec<_>>());
    // The rectangles' sides are 2·half·√2 and 2·half, so the areas are 4√2·half²:
    // outer 400√2 − inner 64√2 = 336√2, and the inner 64√2.
    assert!((ps[0].area - 336.0 * 2.0f64.sqrt()).abs() < 1e-6, "area {}", ps[0].area);
    assert_eq!(ps[0].hole_curves.len(), 1);
    assert!((ps[1].area - 64.0 * 2.0f64.sqrt()).abs() < 1e-6, "area {}", ps[1].area);
    assert!(ps[1].hole_curves.is_empty());
}

#[test]
fn flat_wire_loops_on_the_sketch_plane_are_listed_once() {
    // A square drawn entirely with 3D lines flat on the sketch plane: it is not a 2D profile
    // (2D detection ignores wires), so it must appear here exactly once.
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let corners = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
    for k in 0..4 {
        let a = Vec3 { x: corners[k][0], y: corners[k][1], z: 0.0 };
        let b = Vec3 { x: corners[(k + 1) % 4][0], y: corners[(k + 1) % 4][1], z: 0.0 };
        sk.wires.push(wire(&format!("w{k}"), &[a, b]));
    }
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 1);
    assert!((ps[0].area - 100.0).abs() < 1e-6);
}

#[test]
fn a_loop_closed_by_a_drawn_spline_is_a_profile() {
    // #17296: a 3D spline through 5 points in the plane y = 0, closed by a 3-segment 3D
    // polyline in the same plane. The spline is sampled into ~100 points; the loop must still be
    // found (it was refused as soon as one curve was a spline).
    let v = |x: f64, z: f64| Vec3 { x, y: 0.0, z };
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    // As `sketch.spline3d` and `sketch.line3d` draw them: one wire for the spline, one per segment.
    let draw = |sk: &mut Sketch, fit: &[Vec3]| sk.add_drawn_wire_in(fit, WireSpace::Component).unwrap();
    draw(&mut sk, &[v(-40.0, -4.0), v(-20.0, -1.0), v(0.0, 0.0), v(20.0, -1.0), v(40.0, -4.0)]);
    draw(&mut sk, &[v(40.0, -4.0), v(40.0, -30.0)]);
    draw(&mut sk, &[v(40.0, -30.0), v(-40.0, -30.0)]);
    draw(&mut sk, &[v(-40.0, -30.0), v(-40.0, -4.0)]);
    assert!(sk.wires[0].pts.len() > 64, "the spline is sampled finely: {}", sk.wires[0].pts.len());
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 1, "{:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
    // The issue's 2D twin of this loop patches to 2291.5 mm²; the 80 × 26 rectangle below the
    // spline's ends is 2080 mm², and the spline bulges up to 4 mm above them.
    assert!(ps[0].area > 2080.0 + 150.0 && ps[0].area < 2080.0 + 320.0, "area {}", ps[0].area);
    assert_eq!(ps[0].outer_curves.len(), 4, "{:?}", ps[0].outer_curves);
    assert!(dist_to_plane(&ps[0].plane, v(0.0, 0.0)) < 1e-6);
}

#[test]
fn a_half_disc_closed_by_a_drawn_arc_is_a_profile() {
    // A 3D arc of radius 10 in the plane y = 5 (start, through, end), closed by a 3D line.
    let v = |x: f64, z: f64| Vec3 { x, y: 5.0, z };
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    sk.add_drawn_arc_in(&[v(-10.0, 0.0), v(0.0, 10.0), v(10.0, 0.0)], WireSpace::Component).unwrap();
    sk.add_drawn_wire_in(&[v(10.0, 0.0), v(-10.0, 0.0)], WireSpace::Component).unwrap();
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 1, "{:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
    let half_disc = std::f64::consts::PI * 100.0 / 2.0;
    assert!((ps[0].area - half_disc).abs() < 0.01 * half_disc, "area {} vs {half_disc}", ps[0].area);
    assert!(dist_to_plane(&ps[0].plane, v(0.0, 10.0)) < 1e-6);
}

#[test]
fn a_closed_wire_whose_quarter_points_line_up_is_a_profile() {
    // One closed wire in the plane y = 0: a triangle with a zigzag bottom. Its start, quarter,
    // half and three-quarter points all lie on z = 0, and its ends meet, so the line through its
    // ends has no direction. A point off that line must still span the wire's plane.
    let v = |x: f64, z: f64| Vec3 { x, y: 0.0, z };
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let pts = [v(0.0, 0.0), v(5.0, -3.0), v(10.0, 0.0), v(15.0, -3.0), v(20.0, 0.0), v(25.0, -3.0), v(30.0, 0.0), v(15.0, 20.0), v(0.0, 0.0)];
    sk.wires.push(wire("w", &pts));
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 1, "{:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
    // The 30 × 20 triangle (300 mm²) and three notches of 10 × 3 / 2 below it.
    assert!((ps[0].area - 345.0).abs() < 1e-6, "area {}", ps[0].area);
    assert!(dist_to_plane(&ps[0].plane, v(15.0, 20.0)) < 1e-6);
}

#[test]
fn every_tooth_of_a_zigzag_wire_is_a_profile() {
    // One 3D polyline zigzags from base points on the x axis to apexes leaning out to both sides,
    // and in-plane lines join the base points. Each tooth is a triangle in a plane of its own,
    // which holds an apex that is none of the wire's key points (ends, quarters, farthest), so a
    // sketch this small spans planes by all its points.
    let mut sk = Sketch::new();
    sk.view.three_d = true;
    let lean = [5.0, -5.0, 8.0, -8.0];
    let mut pts = Vec::new();
    let mut base = Vec::new();
    for k in 0..=4 {
        let x = 10.0 * k as f64;
        base.push(sk.add_point(Vec2 { x, y: 0.0 }, None).unwrap());
        pts.push(Vec3 { x, y: 0.0, z: 0.0 });
        if let Some(&y) = lean.get(k) {
            pts.push(Vec3 { x: x + 5.0, y, z: 10.0 });
        }
    }
    for k in 0..4 {
        sk.add_curve(CurveKind::Line { a: base[k], b: base[k + 1] }, None).unwrap();
    }
    sk.wires.push(wire("w", &pts));
    let ps = find_planar_3d_profiles(&sk, Plane::XY);
    assert_eq!(ps.len(), 4, "one triangle per tooth: {:?}", ps.iter().map(|p| (&p.outer_curves, p.area)).collect::<Vec<_>>());
    // Base 10; the apex is sqrt(lean² + 10²) from the base line.
    let mut want: Vec<f64> = lean.iter().map(|y: &f64| 5.0 * (y * y + 100.0).sqrt()).collect();
    let mut got: Vec<f64> = ps.iter().map(|p| p.area).collect();
    want.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-6, "areas {got:?} vs {want:?}");
    }
}

#[test]
fn nothing_without_3d_wires() {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2 { x: 0.0, y: 0.0 }, None).unwrap();
    let b = sk.add_point(Vec2 { x: 10.0, y: 0.0 }, None).unwrap();
    sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    assert!(find_planar_3d_profiles(&sk, Plane::XY).is_empty());
    sk.view.three_d = true;
    assert!(find_planar_3d_profiles(&sk, Plane::XY).is_empty());
}
