//! #2408, reconstructed from the public screenshot (the original design is unavailable).
//! A cylindrical side cut through a rounded bar leaves a very short seam where the top lip
//! meets the tangent junction of the bar's straight and round walls. The chamfer must trim
//! against the round wall instead of extending that short seam outside the solid.

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel::{Body, BoolOp, boolean, chamfer, cylinder, extrude};
use std::f64::consts::PI;

fn holder() -> Body {
    let segs = vec![
        Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, 70.0) },
        Seg2::Arc { center: Vec2::new(10.0, 70.0), radius: 10.0, start: PI, sweep: -PI },
        Seg2::Line { a: Vec2::new(20.0, 70.0), b: Vec2::new(20.0, 10.0) },
        Seg2::Arc { center: Vec2::new(10.0, 10.0), radius: 10.0, start: 0.0, sweep: -PI },
    ];
    let bar = extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 60.0).unwrap().pop().unwrap();
    let tool = cylinder(Vec3::new(-1.0, 70.0, 60.0), Vec3::X, 60.0, 22.0).unwrap();
    boolean(&bar, &tool, BoolOp::Cut).unwrap().unwrap()
}

/// Integrate the removed horizontal strips analytically across the capsule's width, then
/// use Simpson integration in height. The rolling section's centre is 2 below the top and
/// 62 from the cutter's axis; its wall contact is 60/62 of that vector from the axis.
fn expected_removed(distance: f64) -> f64 {
    let center_y = 70.0 - ((60.0 + distance).powi(2) - distance.powi(2)).sqrt();
    let contact_y = 70.0 + (center_y - 70.0) * 60.0 / (60.0 + distance);
    let depth = distance * 60.0 / (60.0 + distance);
    let area = |y: f64| {
        let u = (y - 10.0).clamp(-10.0, 0.0);
        u * (100.0 - u * u).sqrt() + 100.0 * (u / 10.0).asin() + 20.0 * (y - 10.0).max(0.0)
    };
    let n = 4000;
    let h = depth / n as f64;
    (0..=n)
        .map(|i| {
            let z = i as f64 * h;
            let cut = 70.0 - (3600.0 - z * z).sqrt();
            let bevel = center_y + (contact_y - center_y) * z / depth;
            let weight = if i == 0 || i == n {
                1.0
            } else if i % 2 == 0 {
                2.0
            } else {
                4.0
            };
            weight * (area(cut) - area(bevel))
        })
        .sum::<f64>()
        * h
        / 3.0
}

#[test]
fn chamfer_the_top_lip_at_a_tangent_wall_junction() {
    let body = holder();
    let made = chamfer(&body, &[Vec3::new(10.0, 10.0, 60.0)], 2.0).unwrap();
    assert!(made.validity().is_empty(), "{:?}", made.validity());
    assert_eq!(made.crossed_face_loops().unwrap(), 0);
    let history = made.history().expect("the chamfer retains exact face provenance");
    assert!(history.faces.iter().any(|f| f.origins.iter().any(|o| matches!(o, solidcraft_kernel::history::FaceOrigin::Edge { operand: 0, .. }))));
    let mesh = |body: &Body| body.mesh(&crate::MeshTolerance::chord(0.0005)).unwrap();
    let before = mesh(&body).measure().volume;
    let after = mesh(&made);
    let removed = before - after.measure().volume;
    let expected = expected_removed(2.0);
    eprintln!("removed {removed:.8} mm³, expected {expected:.8} mm³");
    assert!((removed - expected).abs() < 0.03, "removed {removed}, expected {expected}");
    assert!(!made.contains(Vec3::new(10.0, 9.5, 59.5)), "the old corner was removed");
    assert!(made.contains(Vec3::new(10.0, 7.0, 59.5)), "material behind the bevel remains");
    assert!(made.contains(Vec3::new(10.0, 10.0, 55.0)), "material below the bevel remains");
    // A closed mesh's signed volume is independent of the origin.
    let volume_about = |origin: Vec3| {
        after
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| after.positions[i as usize] - origin);
                a.dot(b.cross(c)) / 6.0
            })
            .sum::<f64>()
    };
    assert!((volume_about(Vec3::ZERO) - volume_about(Vec3::new(-100.0, 50.0, 30.0))).abs() < 0.01);
}

#[test]
fn a_chamfer_that_uses_up_the_holder_top_is_refused() {
    let body = holder();
    assert!(chamfer(&body, &[Vec3::new(10.0, 10.0, 60.0)], 30.0).is_err());
}

/// A plate of side `plate` with a 50 x 10 mm stadium boss, 10 mm tall, joined on top.
fn plate_with_stadium(plate: f64) -> Body {
    let base = solidcraft_kernel::box_solid(Vec3::new(-plate / 2.0, -plate / 2.0, 0.0), Vec3::new(plate / 2.0, plate / 2.0, 10.0)).unwrap();
    let segs = vec![
        Seg2::Line { a: Vec2::new(-20.0, -5.0), b: Vec2::new(20.0, -5.0) },
        Seg2::Arc { center: Vec2::new(20.0, 0.0), radius: 5.0, start: -PI / 2.0, sweep: PI },
        Seg2::Line { a: Vec2::new(20.0, 5.0), b: Vec2::new(-20.0, 5.0) },
        Seg2::Arc { center: Vec2::new(-20.0, 0.0), radius: 5.0, start: PI / 2.0, sweep: PI },
    ];
    let plane = Plane::XY.offset(10.0);
    let boss = extrude(&plane, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 10.0).unwrap().pop().unwrap();
    boolean(&base, &boss, BoolOp::Union).unwrap().unwrap()
}

/// #2672: one pick on a straight edge that runs on into an arc chamfers the whole tangent
/// chain, whatever the size of the part (it used to stop at the straight edge once the
/// bounding-box diagonal passed about 200 mm).
#[test]
fn one_chamfer_pick_follows_the_tangent_chain_at_any_part_size() {
    for plate in [140.0, 145.0, 250.0] {
        let body = plate_with_stadium(plate);
        let made = chamfer(&body, &[Vec3::new(0.0, 5.0, 20.0)], 1.0).unwrap();
        assert!(made.validity().is_empty(), "{:?}", made.validity());
        // Two straight edges (c^2/2 * 40 each) and both end arcs (Pappus, c^2/2 * 2 pi (R - c/3)).
        let expected = 40.0 + 0.5 * 2.0 * PI * (5.0 - 1.0 / 3.0);
        let volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
        let removed = volume(&body) - volume(&made);
        assert!((removed - expected).abs() < 0.3, "plate {plate}: removed {removed}, expected {expected}");
        assert_eq!(made.face_count(), body.face_count() + 4, "plate {plate}: one chamfer face per rim edge");
    }
}
