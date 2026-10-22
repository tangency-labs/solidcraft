//! A round or a chamfer on the end circle of a bent lofted tube.
//!
//! The wall of such a tube is two faces of the loft that meet in a seam, a curve (the loft bends),
//! and the seam ends on the circle at each of the circle's two vertices. The blend's section at such
//! a vertex has its contact on the seam. It was refused for a seam that is not straight ("not
//! supported yet: a curved seam where the edge chain turns"); now the contact is held on the curved
//! seam itself.

use std::f64::consts::PI;

use solidcraft_geom::Vec3;

use super::*;

/// Radius of the tube, radius of the bend of its axis, and the turn (radians).
const TUBE: f64 = 20.0;
const BEND: f64 = 100.0;
const TURN: f64 = PI / 3.0;

/// Five circles of the tube's radius, square to an arc of radius `BEND` that turns `TURN` from the
/// origin (the axis leaves along +x, the arc's centre is (0, BEND)), lofted into one body. The
/// start circle lies in the plane x = 0.
fn bent_tube(s: &mut Session) {
    let mut sections = Vec::new();
    for i in 0..5 {
        let t = TURN * i as f64 / 4.0;
        let name = format!("S{i}");
        let plane = json!({"origin": [BEND * t.sin(), BEND * (1.0 - t.cos()), 0], "x_dir": [0, 0, 1], "y_dir": [t.sin(), -t.cos(), 0]});
        run(s, "sketch.create", json!({"plane": plane, "name": name}));
        run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": TUBE}));
        run(s, "sketch.finish", json!({}));
        sections.push(json!({"sketch": name}));
    }
    run(s, "solid.loft", json!({"sections": sections, "operation": "new", "name": "Bent", "body_name": "Tube"}));
}

/// A point of the start circle, 45 degrees round from the arc's plane (nowhere near its vertices,
/// which are in that plane's perpendicular, the seams' ends).
fn start_circle() -> Value {
    let a = PI / 4.0;
    json!([[0.0, -TUBE * a.cos(), TUBE * a.sin()]])
}

/// The tube's distance from the axis to a point: how far it is from the core circle of the bend.
fn core(p: Vec3) -> f64 {
    ((p.x.hypot(p.y - BEND) - BEND).hypot(p.z)).abs()
}

/// What a ball of radius `round` rolling along the start circle makes: it touches the end plane
/// (x = 0) and the wall, so its centre is on x = `round` and `TUBE - round` from the core circle.
/// The mesh vertices of the round are all `round` from the curve of those centres. Returns how many
/// vertices were looked at and the worst error of that distance.
fn off_the_ball(s: &Session, round: f64) -> (usize, f64) {
    let body = &s.model.state().bodies[0].body;
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let centres: Vec<Vec3> = (0..720)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / 720.0;
            let rho = BEND + (TUBE - round) * a.cos();
            Vec3::new(round, BEND - (rho * rho - round * round).sqrt(), (TUBE - round) * a.sin())
        })
        .collect();
    let (mut count, mut worst) = (0, 0.0_f64);
    for p in &mesh.positions {
        // On the round: off the end plane, inside the wall, and within reach of the ball.
        if p.x > 1e-3 && p.x < round + 0.5 && core(*p) < TUBE - 0.02 && core(*p) > TUBE - round - 0.5 {
            let d = centres.iter().map(|c| c.dist(*p)).fold(f64::INFINITY, f64::min);
            count += 1;
            worst = worst.max((d - round).abs());
        }
    }
    (count, worst)
}

#[test]
fn a_round_on_the_end_circle_of_a_bent_lofted_tube_is_a_rolling_ball_blend() {
    let mut s = Session::default();
    bent_tube(&mut s);
    let before = volume(&mut s);
    // The loft is the bent tube itself: pi r^2 times the length of the axis.
    assert!(rel(before, PI * TUBE * TUBE * BEND * TURN) < 1e-3, "{before}");
    run(&mut s, "solid.fillet", json!({"edges": start_circle(), "radius": 3.0}));
    assert_eq!(s.model.state().bodies.len(), 1);
    // About what the same round takes off a straight tube (a body of revolution, Pappus: the corner
    // between the ball and the wall, 9 (1 - pi / 4) mm2 at 0.2234 * 3 mm inside the wall).
    let straight = 2.0 * PI * 9.0 * (1.0 - PI / 4.0) * (TUBE - 0.2234 * 3.0);
    let removed = before - volume(&mut s);
    assert!((removed - straight).abs() < 0.08 * straight, "removed {removed}, a straight tube loses {straight}");
    let (count, worst) = off_the_ball(&s, 3.0);
    assert!(count > 200, "only {count} vertices on the round");
    assert!(worst < 0.02, "the round's surface is {worst} mm off the ball's");
}

#[test]
fn a_chamfer_on_the_end_circle_of_a_bent_lofted_tube_builds() {
    let mut s = Session::default();
    bent_tube(&mut s);
    let before = volume(&mut s);
    run(&mut s, "solid.chamfer", json!({"edges": start_circle(), "distance": 3.0}));
    assert_eq!(s.model.state().bodies.len(), 1);
    // A straight tube loses a triangle of 3 x 3 / 2 mm2 with its middle a third of 3 mm inside the wall.
    let straight = 2.0 * PI * 4.5 * (TUBE - 1.0);
    let removed = before - volume(&mut s);
    assert!((removed - straight).abs() < 0.08 * straight, "removed {removed}, a straight tube loses {straight}");
}
