//! A chamfer of a hole's rim larger than the plate round it (a user's report): a 50 mm square
//! plate, 3/4 in thick, with a 12.7 mm hole, its bottom rim chamfered by a distance up the hole's
//! wall and an angle. Past the plate's sides (or its top) the chamfer used to build a cone sticking
//! out of the part, or was refused; it is now cut through the faces it reaches, and the part stays
//! within its bounds.

use std::f64::consts::PI;

use serde_json::json;

use super::{run, volume};
use crate::Session;

const HALF: f64 = 25.0;
const THICK: f64 = 19.05;
const HOLE: f64 = 6.35;

/// The area of the square of half-width `HALF` within `rho` of its centre.
fn square_disc(rho: f64) -> f64 {
    if rho <= HALF {
        PI * rho * rho
    } else if rho < HALF * 2f64.sqrt() {
        PI * rho * rho - 4.0 * (rho * rho * (HALF / rho).acos() - HALF * (rho * rho - HALF * HALF).sqrt())
    } else {
        4.0 * HALF * HALF
    }
}

/// The plate less the chamfer: `sw` up the wall, `sf` out across the bottom, the cut at height z
/// the square within R + sf (1 − z/sw) of the axis (Simpson's rule over the height).
fn expected(sf: f64, sw: f64) -> f64 {
    let n = 100_000;
    let top = THICK.min(sw);
    let h = top / n as f64;
    let f = |z: f64| (square_disc(HOLE + sf * (1.0 - z / sw)) - PI * HOLE * HOLE).max(0.0);
    let mut sum = f(0.0) + f(top);
    for i in 1..n {
        sum += f(h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    (4.0 * HALF * HALF - PI * HOLE * HOLE) * THICK - sum * h / 3.0
}

fn chamfer_error(s: &Session) -> Option<String> {
    s.model.results.iter().find(|r| r.name == "Chamfer1").and_then(|r| r.error.clone())
}

#[test]
fn hole_rim_chamfer_past_the_plate_is_cut_through_it() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [25, 25]}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 12.7}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "3/4in", "profiles": [{"point": [20, 20]}]}));
    let rim = json!([[0, 6.35, 0], [0, -6.35, 0]]);
    run(&mut s, "solid.chamfer", json!({"edges": rim, "distance": "10mm", "angle": "45deg"}));
    // (distance up the wall, angle, setback across the bottom)
    let ratio = 30f64.to_radians().sin() / 120f64.to_radians().sin();
    for (d, angle, sf) in [(10.0, 45.0, 10.0), (19.0, 45.0, 19.0), (THICK, 45.0, THICK), (25.0, 45.0, 25.0), (25.0, 30.0, 25.0 * ratio)] {
        run(&mut s, "timeline.edit", json!({"feature": "Chamfer1", "set": {"distance": format!("{d} mm"), "angle": format!("{angle} deg")}}));
        assert_eq!(chamfer_error(&s), None, "{d} mm at {angle}°");
        let want = expected(sf, d);
        let got = volume(&mut s);
        // `inspect.measure` reads cones a few parts in 1e5 heavy.
        assert!((got - want).abs() < want * 5e-5, "{d} mm at {angle}°: {got} vs {want}");
        let world = s.world_state();
        let body = world.bodies.first().expect("the plate");
        let bb = body.body.tessellate(1e-3).expect("mesh").bounds();
        let slack = 1e-3;
        assert!(bb.min.x >= -HALF - slack && bb.min.y >= -HALF - slack && bb.min.z >= -slack, "{d} mm at {angle}°: {bb:?}");
        assert!(bb.max.x <= HALF + slack && bb.max.y <= HALF + slack && bb.max.z <= THICK + slack, "{d} mm at {angle}°: {bb:?}");
    }
}
