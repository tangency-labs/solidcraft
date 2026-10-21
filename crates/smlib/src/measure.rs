//! Exact mass properties from SMLib: area, volume, centroid and inertia, integrated on the exact
//! surfaces rather than a tessellation.

use crate::{Brep, Result, error, ffi};

/// The accuracy [`Brep::mass_properties`] and [`Brep::area`] use unless told otherwise: the
/// finest SMLib allows. Analytic primitives then come out within 2e-7 of their closed forms.
pub const MEASURE_ACCURACY: f64 = 1e-8;

/// Mass properties of a closed solid at unit density (mm, mm², mm³, mm⁵). For a density ρ the
/// mass is ρ·`volume` and the inertia scales by ρ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    pub area: f64,
    pub volume: f64,
    pub centroid: [f64; 3],
    /// Ixx, Iyy, Izz about axes through the centroid parallel to the world axes:
    /// ∫(y²+z²), ∫(z²+x²), ∫(x²+y²) dV.
    pub moments: [f64; 3],
    /// Iyz, Izx, Ixy about the centroid: ∫yz, ∫zx, ∫xy dV relative to it.
    pub products: [f64; 3],
}

impl MassProperties {
    /// The inertia tensor about the centroid (row-major), whose off-diagonal terms are the
    /// negated products.
    pub fn inertia_tensor(&self) -> [[f64; 3]; 3] {
        let [ixx, iyy, izz] = self.moments;
        let [iyz, izx, ixy] = self.products;
        [[ixx, -ixy, -izx], [-ixy, iyy, -iyz], [-izx, -iyz, izz]]
    }
}

impl Brep {
    /// Mass properties at `relative_accuracy` (clamped to [1e-8, 1e-1]; [`MEASURE_ACCURACY`] is
    /// the finest). A body that is not a closed manifold solid is an error; [`Brep::area`]
    /// measures those.
    pub fn mass_properties(&self, relative_accuracy: f64) -> Result<MassProperties> {
        let m = ffi::measure::mass_properties(&self.0, relative_accuracy).map_err(error)?;
        let [area, volume, cx, cy, cz, ixx, iyy, izz, iyz, izx, ixy] = m;
        Ok(MassProperties { area, volume, centroid: [cx, cy, cz], moments: [ixx, iyy, izz], products: [iyz, izx, ixy] })
    }

    /// Total area of every face, each counted once, so sheets and open shells too.
    pub fn area(&self, relative_accuracy: f64) -> Result<f64> {
        ffi::measure::area(&self.0, relative_accuracy).map_err(error)
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use crate::Error;

    fn rel(a: f64, b: f64) -> f64 {
        if b == 0.0 { a.abs() } else { (a / b - 1.0).abs() }
    }

    /// tests/oracle/01-box: the analytic values its expected.json gives, plus the box's inertia.
    #[test]
    fn the_oracle_box_measures_exactly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/oracle/01-box/expected.json");
        let expected: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir).unwrap()).unwrap();
        assert_eq!(expected["source"], "analytic");
        let max = &expected["bbox"]["max"];
        let size = [0, 1, 2].map(|i| max[i].as_f64().unwrap());
        let b = Brep::make_box([0.0; 3], size).unwrap();
        let m = b.mass_properties(MEASURE_ACCURACY).unwrap();
        assert!(rel(m.volume, expected["volume_mm3"].as_f64().unwrap()) < 1e-12, "{m:?}");
        assert!(rel(m.area, expected["area_mm2"].as_f64().unwrap()) < 1e-12, "{m:?}");
        assert!(rel(b.area(MEASURE_ACCURACY).unwrap(), expected["area_mm2"].as_f64().unwrap()) < 1e-12);
        for i in 0..3 {
            assert!((m.centroid[i] - expected["center_of_mass"][i].as_f64().unwrap()).abs() < 1e-9, "{m:?}");
        }
        let [a, bb, c] = size;
        let v = a * bb * c;
        let want = [v * (bb * bb + c * c) / 12.0, v * (a * a + c * c) / 12.0, v * (a * a + bb * bb) / 12.0];
        for i in 0..3 {
            assert!(rel(m.moments[i], want[i]) < 1e-12, "{m:?}");
            assert!(m.products[i].abs() < 1e-9 * want[2], "{m:?}");
        }
    }

    /// Far from the origin the inertia is still taken about the centroid, without cancellation.
    #[test]
    fn a_box_far_from_the_origin_keeps_its_inertia() {
        let b = Brep::make_box([1.0e5, -2.0e5, 3.0e4], [3.0, 50.0, 0.5]).unwrap();
        let m = b.mass_properties(MEASURE_ACCURACY).unwrap();
        let v = 75.0;
        assert!(rel(m.volume, v) < 1e-12);
        assert!(rel(m.centroid[0], 1.0e5 + 1.5) < 1e-14 && rel(m.centroid[1], -2.0e5 + 25.0) < 1e-14);
        assert!(rel(m.moments[0], v * (2500.0 + 0.25) / 12.0) < 1e-9, "{m:?}");
        assert!(rel(m.moments[1], v * (9.0 + 0.25) / 12.0) < 1e-9, "{m:?}");
        assert!(rel(m.moments[2], v * (9.0 + 2500.0) / 12.0) < 1e-9, "{m:?}");
        let t = m.inertia_tensor();
        assert_eq!(t[0][1], t[1][0]);
        assert!(t[0][1].abs() < 1e-9 * m.moments[2]);
    }

    #[test]
    fn bad_accuracies_are_errors() {
        let b = Brep::make_box([0.0; 3], [1.0; 3]).unwrap();
        assert_eq!(b.mass_properties(f64::NAN).unwrap_err(), Error::Argument);
        assert_eq!(b.area(f64::INFINITY).unwrap_err(), Error::Argument);
        // Out of range is clamped, not refused.
        assert!(rel(b.mass_properties(1e-30).unwrap().volume, 1.0) < 1e-12);
        assert!(rel(b.mass_properties(10.0).unwrap().volume, 1.0) < 1e-12);
    }
}
