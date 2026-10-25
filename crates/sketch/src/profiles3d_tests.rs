//! Tests for planar 3D loop detection (#741): the pyramid of the issue report, skew loops,
//! nesting, tolerance, and the no-duplication rule against the 2D profiles.

use solidcraft_geom::{Plane, Vec2, Vec3};

use crate::model::{CurveKind, Sketch};
use crate::profiles3d::find_planar_3d_profiles;
use crate::wire::WireSpace;

fn wire(id: &str, pts: &[Vec3]) -> crate::wire::Wire {
    crate::wire::Wire { id: id.into(), pts: pts.to_vec(), space: WireSpace::Component, link: None, fit: Vec::new(), arc: None }
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
fn nothing_without_3d_wires() {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2 { x: 0.0, y: 0.0 }, None).unwrap();
    let b = sk.add_point(Vec2 { x: 10.0, y: 0.0 }, None).unwrap();
    sk.add_curve(CurveKind::Line { a, b }, None).unwrap();
    assert!(find_planar_3d_profiles(&sk, Plane::XY).is_empty());
    sk.view.three_d = true;
    assert!(find_planar_3d_profiles(&sk, Plane::XY).is_empty());
}
