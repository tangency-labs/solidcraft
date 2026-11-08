//! #12760: a round tube measures the same after a STEP export and import.
//!
//! The tube's cylinders have their seams on their frames' x axes; read back, each seam's angle
//! came a rounding short of a whole turn, the cylinder's NURBS form started a quarter early and
//! its last quarter was off the form: the tube measured 14 % over.

use solidcore_record::Vec3;
use solidcraft_kernel::{BoolOp, boolean, cylinder, measure_mass, step_export, step_import};

#[test]
fn a_tube_measures_the_same_after_a_step_round_trip() {
    // The report's handrail: a round tube 42.4 x 2.6 along (4274.44, 0, 2730.16).
    let d: [f64; 3] = [9079.496433 - 4805.058271, 0.0, 6471.008415 - 3740.849768];
    let len = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let axis = Vec3::new(d[0] / len, 0.0, d[2] / len);
    for base in [Vec3::new(0.0, 447.0, 0.0), Vec3::new(4805.058271, 447.0, 3740.849768)] {
        let outer = cylinder(base, axis, 21.2, len).unwrap();
        let inner = cylinder(Vec3::new(base.x - axis.x, base.y, base.z - axis.z), axis, 18.6, len + 2.0).unwrap();
        let tube = boolean(&outer, &inner, BoolOp::Cut).unwrap().unwrap();
        let built = measure_mass(&tube).unwrap();
        let read = step_import(&step_export(&[&tube], "test").unwrap()).unwrap();
        let back = measure_mass(&read.bodies[0].body).unwrap();
        let exact = std::f64::consts::PI * (21.2_f64.powi(2) - 18.6_f64.powi(2)) * len;
        assert!((built.volume - exact).abs() < 1e-6 * exact, "built at {base:?}: {} against {exact}", built.volume);
        assert!((back.volume - exact).abs() < 1e-6 * exact, "read back at {base:?}: {} against {exact}", back.volume);
        assert!((back.area - built.area).abs() < 1e-6 * built.area, "read back at {base:?}: area {} against {}", back.area, built.area);
        let (c, b) = (back.centroid, built.centroid);
        let off = ((c.x - b.x).powi(2) + (c.y - b.y).powi(2) + (c.z - b.z).powi(2)).sqrt();
        assert!(off < 1e-6, "read back at {base:?}: centre {:?} against {:?}", back.centroid, built.centroid);
    }
}
