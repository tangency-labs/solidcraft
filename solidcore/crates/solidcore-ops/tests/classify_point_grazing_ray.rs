//! A point just inside a solid cylinder, near the top of its circle, classifies as inside.
//! The classification fires a ray along +x first; from such a point the ray leaves through the
//! side face at a shallow angle (a few degrees), and that crossing must be found.
//! SolidCraft #12539: an R1 fillet beside a 1 mm chamfer round an R30.475 outline was refused
//! as cutting the part, because the middle of the round's section, 0.08 mm inside the wall,
//! classified as outside.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test

mod common;

use common::built;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore_ops::{Body, PointClass};
use solidcore_record::brep::Brep as Record;

const R: f64 = 30.475;
const AXIS: [f64; 2] = [-8.0, 0.0];

/// The point at `angle` degrees round the axis, `radial` from it, at height `z`.
fn at(angle: f64, radial: f64, z: f64) -> [f64; 3] {
    let a = angle.to_radians();
    [AXIS[0] + radial * a.cos(), AXIS[1] + radial * a.sin(), z]
}

/// The points either side of the wall round the top of the circle that classify wrongly.
fn misclassified(cyl: &Body) -> Vec<String> {
    let mut wrong = Vec::new();
    // Round the top of the circle, where a +x ray meets the wall at a shallow angle, and below
    // it, where it meets the wall steeply. 0.075 mm either side of the wall: inside exactly when
    // nearer the axis than the radius.
    for step in 0..=60 {
        let angle = 60.0 + 0.5 * f64::from(step);
        for (radial, want) in [
            (R - 0.075, PointClass::Inside),
            (R + 0.075, PointClass::Outside),
        ] {
            for z in [1.03, 10.0, 44.0] {
                let p = at(angle, radial, z);
                let got = cyl.classify_point(p).unwrap();
                if got != want {
                    wrong.push(format!(
                        "{angle} deg, {radial} from the axis, z {z}: {got:?}, want {want:?}"
                    ));
                }
            }
        }
    }
    wrong
}

/// SolidCraft's cylinder: the extruded circle, its seam at 0.6131 rad from x, its rational
/// spline in four quarter-circle spans.
#[test]
fn points_beside_an_extruded_circle_wall_classify_by_their_distance_to_its_axis() {
    let record: Record =
        serde_json::from_str(include_str!("data/extruded_circle_r30_475.json")).unwrap();
    let cyl = Body::from_record(&record, true).unwrap().0;
    let wrong = misclassified(&cyl);
    assert!(
        wrong.is_empty(),
        "{} of 366 points misclassified:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn points_beside_a_cylinder_wall_classify_by_their_distance_to_its_axis() {
    let cyl = built(|k| create_cylinder(k, Point3::new(AXIS[0], AXIS[1], 0.0), R, 45.0));
    let wrong = misclassified(&cyl);
    assert!(
        wrong.is_empty(),
        "{} of 366 points misclassified:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
