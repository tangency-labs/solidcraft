//! GitHub tangency-labs/solidcraft#23288: a 100 mm cube with a 50 mm through hole
//! refuses a 26 mm round on a top edge. 25 mm stops on the hole's rim and builds.
//! 26 mm crosses that rim, so the round is kept and trimmed where it meets the hole
//! wall. The result is a closed solid.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// Cube from (-50, -50, 0) to (50, 50, 100), hole radius 25 through it, and the top
/// edge at y = 50.
fn holed_cube(k: &mut Kernel) -> (Brep, EdgeId) {
    let block = create_box(k, Point3::new(-50.0, -50.0, 0.0), 100.0, 100.0, 100.0).unwrap();
    let drill = create_cylinder(k, Point3::new(0.0, 0.0, -1.0), 25.0, 102.0).unwrap();
    let b = boolean_difference(k, block, drill).unwrap();
    let top: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (Some(s), Some(t)) = (s, t) else {
                return false;
            };
            let (p, q) = (vertex::point(&b, s).unwrap(), vertex::point(&b, t).unwrap());
            [p, q]
                .iter()
                .all(|v| (v.y - 50.0).abs() < 1e-6 && (v.z - 100.0).abs() < 1e-6)
                && (p.x - q.x).abs() > 99.0
        })
        .collect();
    assert_eq!(top.len(), 1, "the top edge at y = 50");
    let top = *top.first().expect("the top edge at y = 50");
    (b, top)
}

fn round_top(radius: f64) -> Result<f64, String> {
    let mut k = Kernel::default();
    let (mut b, top) = holed_cube(&mut k);
    fillet_edges(&mut k, &mut b, &[top], radius, 1, 1, 1.0).map_err(|e| e.to_string())?;
    if !is_manifold_solid(&b).unwrap() {
        return Err("not a closed solid".to_string());
    }
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    if !lines.is_empty() {
        return Err(format!("{lines:?}"));
    }
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(&k, &mut b, 1e-6, &attrs).map_err(|e| e.to_string())
}

/// Cube minus the hole, minus one straight round of radius `r` along an edge of length
/// 100, plus the part of that round's corner that lies inside the hole (already gone).
/// At 25 the rail is tangent to the hole, so that part is 0. At 26 it is the integral
/// `∫_24^25 2 sqrt(625 - y^2) (26 - sqrt(26^2 - (y - 24)^2)) dy`.
fn expected_volume(radius: f64, overlap: f64) -> f64 {
    let hole = std::f64::consts::PI * 625.0 * 100.0;
    let removed = 100.0 * radius * radius * (1.0 - std::f64::consts::PI / 4.0);
    1_000_000.0 - hole - removed + overlap
}

/// 25 mm meets the hole at one point and builds. 26 mm crosses the hole and builds
/// the same way: a closed solid of the round's volume.
#[test]
fn a_round_that_crosses_the_center_hole_builds() {
    let clear = round_top(25.0).expect("25 mm");
    let clear_expected = expected_volume(25.0, 0.0);
    assert!(
        (clear - clear_expected).abs() <= 1e-6 * clear_expected,
        "25 mm volume {clear} expected {clear_expected}"
    );
    let barely = round_top(25.1).expect("25.1 mm");
    let barely_expected = expected_volume(25.1, 0.0000135706);
    assert!(
        (barely - barely_expected).abs() <= 1e-6 * barely_expected,
        "25.1 mm volume {barely} expected {barely_expected}"
    );
    let crossed = round_top(26.0).expect("26 mm");
    // overlap by Simpson's rule on 400_000 steps
    let crossed_expected = expected_volume(26.0, 0.0413111165);
    assert!(
        (crossed - crossed_expected).abs() <= 1e-6 * crossed_expected,
        "26 mm volume {crossed} expected {crossed_expected}"
    );
}
