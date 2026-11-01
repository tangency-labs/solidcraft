//! Exact mass properties from SMLib: area, volume, centroid and inertia, integrated on the exact
//! surfaces rather than a tessellation; the exact box; how far an edge lies from a face's surface;
//! where a point lies against a body.

use crate::{Brep, Result, core, error, ffi, route};

/// Whether the Measure family runs on solidcore ([`route`]).
fn on_core() -> bool {
    route::backend() == route::Backend::Solidcore
}

/// The accuracy [`Brep::mass_properties`] and [`Brep::area`] use unless told otherwise: the
/// finest SMLib allows. Analytic primitives then come out within 2e-7 of their closed forms.
pub const MEASURE_ACCURACY: f64 = 1e-8;

/// Mass properties of a body at unit density (mm, mm², mm³, mm⁵). For a density ρ the
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

/// Where a point lies against a body ([`Brep::classify_point`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointClass {
    /// In a void region: outside the material, or in a cavity.
    Outside,
    /// In a material region.
    Inside,
    /// Within the body's tolerance of a face, an edge or a vertex.
    OnBoundary,
}

impl Brep {
    /// Where `p` (mm) lies against this body, decided exactly on its curves and surfaces (no
    /// tessellation) by SMLib's `SmBrep::Point3DClassify` at the body's own zone tolerance: on
    /// the boundary when within that tolerance of a face, an edge or a vertex (sheet faces too),
    /// else inside a material region or outside. A body with no material (a sheet, an open shell)
    /// has no inside. A non-finite coordinate is [`Error::Argument`](crate::Error::Argument).
    pub fn classify_point(&self, p: [f64; 3]) -> Result<PointClass> {
        if on_core() {
            return core::classify_point(self.core()?, p);
        }
        match ffi::measure::classify_point(self.cpp()?, p).map_err(error)? {
            ffi::measure::POINT_OUTSIDE => Ok(PointClass::Outside),
            ffi::measure::POINT_INSIDE => Ok(PointClass::Inside),
            ffi::measure::POINT_ON_BOUNDARY => Ok(PointClass::OnBoundary),
            // The C API writes only the three above; any other value is passed on as it came.
            other => Err(crate::Error::Status(other)),
        }
    }

    /// Mass properties at `relative_accuracy` (clamped to [1e-8, 1e-1]; [`MEASURE_ACCURACY`] is
    /// the finest): the sum over the body's material regions, so several lumps, or lumps
    /// touching along edges or at points, measure too. A body with no material (a sheet, an open
    /// shell) has volume 0 and no inertia, its faces' area and the centroid of that area.
    pub fn mass_properties(&self, relative_accuracy: f64) -> Result<MassProperties> {
        if on_core() {
            return core::mass_properties(self.core()?, relative_accuracy);
        }
        let m = ffi::measure::mass_properties(self.cpp()?, relative_accuracy).map_err(error)?;
        let [area, volume, cx, cy, cz, ixx, iyy, izz, iyz, izx, ixy] = m;
        Ok(MassProperties { area, volume, centroid: [cx, cy, cz], moments: [ixx, iyy, izz], products: [iyz, izx, ixy] })
    }

    /// The exact axis-aligned box, least corner then greatest: the extrema of every vertex, edge
    /// and face on its exact curve or surface, not grown by tolerances or control hulls.
    pub fn bounds(&self) -> Result<([f64; 3], [f64; 3])> {
        if on_core() {
            return core::bounds(self.core()?);
        }
        ffi::measure::bounds(self.cpp()?).map_err(error)
    }

    /// How far edge `edge` (its curve over its interval) lies from the surface of face `face`,
    /// over that surface's own domain and not beyond it, in mm: SMLib's own edge/face gap, the
    /// one it sizes an edge's tolerance by. Indices are in the body's edge and face order (as
    /// it was imported and is exported). An edge run past its face's patch measures how far it
    /// goes; SMLib keeps such an edge whole when making its UV trim only when its zone tolerance
    /// (half the flat arrays' `EDGE_TOLERANCE`) covers this.
    pub fn edge_face_distance(&self, edge: usize, face: usize) -> Result<f64> {
        if on_core() {
            return core::edge_face_distance(self.core()?, edge, face);
        }
        let (Ok(e), Ok(f)) = (i64::try_from(edge), i64::try_from(face)) else { return Err(error(ffi::ERR_ARG)) };
        ffi::measure::edge_face_distance(self.cpp()?, e, f).map_err(error)
    }

    /// Total area of every face, each counted once, so sheets and open shells too.
    pub fn area(&self, relative_accuracy: f64) -> Result<f64> {
        if on_core() {
            return core::area(self.core()?, relative_accuracy);
        }
        ffi::measure::area(self.cpp()?, relative_accuracy).map_err(error)
    }
}

/// The closed forms of the primitives' measures, for the tests of both kernels.
#[cfg(test)]
pub(crate) mod closed {
    use std::f64::consts::PI;

    /// Area, volume, centroid, Ixx, Iyy, Izz about it (products are 0 for all), and the exact box.
    pub(crate) struct Closed {
        pub area: f64,
        pub volume: f64,
        pub centroid: [f64; 3],
        pub moments: [f64; 3],
        pub bounds: ([f64; 3], [f64; 3]),
    }

    /// An axis-aligned box from its lower corner.
    pub(crate) fn cuboid(o: [f64; 3], size: [f64; 3]) -> Closed {
        let [a, b, c] = size;
        let v = a * b * c;
        Closed {
            area: 2.0 * (a * b + b * c + c * a),
            volume: v,
            centroid: [o[0] + a / 2.0, o[1] + b / 2.0, o[2] + c / 2.0],
            moments: [v * (b * b + c * c) / 12.0, v * (a * a + c * c) / 12.0, v * (a * a + b * b) / 12.0],
            bounds: (o, [o[0] + a, o[1] + b, o[2] + c]),
        }
    }

    /// A cylinder about +Z on its base centre.
    pub(crate) fn cylinder(o: [f64; 3], r: f64, h: f64) -> Closed {
        let v = PI * r * r * h;
        let ixx = v * (3.0 * r * r + h * h) / 12.0;
        Closed {
            area: 2.0 * PI * r * r + 2.0 * PI * r * h,
            volume: v,
            centroid: [o[0], o[1], o[2] + h / 2.0],
            moments: [ixx, ixx, v * r * r / 2.0],
            bounds: ([o[0] - r, o[1] - r, o[2]], [o[0] + r, o[1] + r, o[2] + h]),
        }
    }

    pub(crate) fn sphere(o: [f64; 3], r: f64) -> Closed {
        let v = 4.0 / 3.0 * PI * r * r * r;
        let i = 0.4 * v * r * r;
        Closed {
            area: 4.0 * PI * r * r,
            volume: v,
            centroid: o,
            moments: [i; 3],
            bounds: ([o[0] - r, o[1] - r, o[2] - r], [o[0] + r, o[1] + r, o[2] + r]),
        }
    }

    /// A frustum (a cone when `rt` is 0) about +Z on its base centre. Its volume integrals are
    /// polynomials in z of degree at most 4, so three-point Gauss-Legendre gives them exactly.
    pub(crate) fn cone(o: [f64; 3], rb: f64, rt: f64, h: f64) -> Closed {
        let (x, w) = ([-(0.6f64).sqrt(), 0.0, (0.6f64).sqrt()], [5.0 / 9.0, 8.0 / 9.0, 5.0 / 9.0]);
        let (mut v, mut mz, mut zz, mut izz) = (0.0, 0.0, 0.0, 0.0);
        for (xi, wi) in x.iter().zip(w) {
            let (z, wi) = (h / 2.0 * (1.0 + xi), wi * h / 2.0);
            let rho = rb + (rt - rb) * z / h;
            let disc = PI * rho * rho;
            v += wi * disc;
            mz += wi * disc * z;
            zz += wi * disc * z * z;
            izz += wi * disc * rho * rho / 2.0;
        }
        let zc = mz / v;
        let ixx = izz / 2.0 + zz - v * zc * zc;
        let s = ((rb - rt).powi(2) + h * h).sqrt();
        let m = rb.max(rt);
        Closed {
            area: PI * (rb * rb + rt * rt + (rb + rt) * s),
            volume: v,
            centroid: [o[0], o[1], o[2] + zc],
            moments: [ixx, ixx, izz],
            bounds: ([o[0] - m, o[1] - m, o[2]], [o[0] + m, o[1] + m, o[2] + h]),
        }
    }

    /// A ring torus about +Z (Pappus for area and volume).
    pub(crate) fn torus(o: [f64; 3], big: f64, r: f64) -> Closed {
        let v = 2.0 * PI * PI * big * r * r;
        let ixx = v * (big * big / 2.0 + 5.0 * r * r / 8.0);
        let m = big + r;
        Closed {
            area: 4.0 * PI * PI * big * r,
            volume: v,
            centroid: o,
            moments: [ixx, ixx, v * (big * big + 0.75 * r * r)],
            bounds: ([o[0] - m, o[1] - m, o[2] - r], [o[0] + m, o[1] + m, o[2] + r]),
        }
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

    /// Reads `tests/oracle/<case>/expected.json`.
    #[allow(clippy::unwrap_used)] // test fixture
    fn oracle(case: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/oracle").join(case).join("expected.json");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(v["source"], "analytic", "{case}");
        v
    }

    use closed::Closed;

    /// A primitive the C++ kernel made, with its closed forms.
    struct Expected {
        name: &'static str,
        body: Brep,
        area: f64,
        volume: f64,
        centroid: [f64; 3],
        moments: [f64; 3],
    }

    #[allow(clippy::unwrap_used)] // test fixture
    fn expected(name: &'static str, body: Result<Brep>, c: Closed) -> Expected {
        Expected { name, body: body.unwrap(), area: c.area, volume: c.volume, centroid: c.centroid, moments: c.moments }
    }

    fn cylinder(name: &'static str, o: [f64; 3], r: f64, h: f64) -> Expected {
        expected(name, Brep::make_cylinder(o, r, h), closed::cylinder(o, r, h))
    }

    fn sphere(name: &'static str, o: [f64; 3], r: f64) -> Expected {
        expected(name, Brep::make_sphere(o, r), closed::sphere(o, r))
    }

    fn cone(name: &'static str, o: [f64; 3], rb: f64, rt: f64, h: f64) -> Expected {
        expected(name, Brep::make_cone(o, rb, rt, h), closed::cone(o, rb, rt, h))
    }

    fn torus(name: &'static str, o: [f64; 3], big: f64, r: f64) -> Expected {
        expected(name, Brep::make_torus(o, big, r), closed::torus(o, big, r))
    }

    /// Every primitive against its closed form, at the origin and 2e5 mm out, to 1e-6 (tests/oracle
    /// accepts 1e-3). Cylinders as oracle 02 and 38 give them, against their expected.json too.
    #[test]
    fn primitives_match_their_closed_forms() {
        for (case, r) in [("02-cylinder", 15.0), ("38-coincident-cylinders", 10.0)] {
            let e = oracle(case);
            let m = Brep::make_cylinder([0.0; 3], r, 50.0).unwrap().mass_properties(MEASURE_ACCURACY).unwrap();
            assert!(rel(m.volume, e["volume_mm3"].as_f64().unwrap()) < 1e-6, "{case}: {m:?}");
            assert!(rel(m.area, e["area_mm2"].as_f64().unwrap()) < 1e-6, "{case}: {m:?}");
            for i in 0..3 {
                assert!((m.centroid[i] - e["center_of_mass"][i].as_f64().unwrap()).abs() < 1e-6, "{case}: {m:?}");
            }
        }
        let far = [1.0e5, -2.0e5, 3.0e4];
        let cases = [
            cylinder("cylinder r15 h50", [0.0; 3], 15.0, 50.0),
            cylinder("cylinder r0.25 h80, far", far, 0.25, 80.0),
            // The primitives of oracle 40-box-sphere-torus, whole.
            sphere("sphere r12", [0.0; 3], 12.0),
            sphere("sphere r7, far", far, 7.0),
            torus("torus R22 r4", [0.0; 3], 22.0, 4.0),
            torus("torus R30 r0.5, far", far, 30.0, 0.5),
            cone("frustum r10..4 h20", [0.0; 3], 10.0, 4.0, 20.0),
            cone("cone r10 h20", [0.0; 3], 10.0, 0.0, 20.0),
            cone("inverted frustum r3..9 h7", [0.0; 3], 3.0, 9.0, 7.0),
            cone("frustum r6..2 h9, far", far, 6.0, 2.0, 9.0),
        ];
        for e in &cases {
            let m = e.body.mass_properties(MEASURE_ACCURACY).unwrap();
            let name = e.name;
            assert!(rel(m.area, e.area) < 1e-6, "{name}: {m:?}");
            assert!(rel(e.body.area(MEASURE_ACCURACY).unwrap(), e.area) < 1e-6, "{name}");
            assert!(rel(m.volume, e.volume) < 1e-6, "{name}: {m:?}");
            // The centroid against the body's size: twice its largest radius of gyration.
            let size = 2.0 * e.moments.iter().fold(0.0f64, |a, &b| a.max(b)).sqrt() / e.volume.sqrt();
            let imax = e.moments.iter().fold(0.0f64, |a, &b| a.max(b));
            for i in 0..3 {
                assert!((m.centroid[i] - e.centroid[i]).abs() < 1e-6 * size, "{name}: {m:?}");
                assert!(rel(m.moments[i], e.moments[i]) < 1e-6, "{name}: {m:?}");
                assert!(m.products[i].abs() < 1e-6 * imax, "{name}: {m:?}");
            }
        }
    }

    /// A 10 × 20 × 30 box: inside, outside, on a face, an edge and a vertex; a cylinder of radius
    /// 5 just off its curved wall, within the tolerance (on it) and beyond (inside or outside).
    #[test]
    fn points_classify_exactly() {
        let b = Brep::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        assert_eq!(b.classify_point([5.0, 10.0, 15.0]).unwrap(), PointClass::Inside);
        assert_eq!(b.classify_point([5.0, 10.0, 30.0 + 1e-4]).unwrap(), PointClass::Outside);
        assert_eq!(b.classify_point([5.0, 10.0, 30.0 - 1e-4]).unwrap(), PointClass::Inside);
        assert_eq!(b.classify_point([5.0, 10.0, 30.0]).unwrap(), PointClass::OnBoundary);
        assert_eq!(b.classify_point([10.0, 10.0, 30.0]).unwrap(), PointClass::OnBoundary);
        assert_eq!(b.classify_point([10.0, 20.0, 30.0]).unwrap(), PointClass::OnBoundary);
        assert_eq!(b.classify_point([5.0, 10.0, f64::NAN]).unwrap_err(), Error::Argument);
        let c = Brep::make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
        let at = |r: f64| [r * std::f64::consts::FRAC_1_SQRT_2, r * std::f64::consts::FRAC_1_SQRT_2, 5.0];
        assert_eq!(c.classify_point(at(5.0 + 1e-8)).unwrap(), PointClass::OnBoundary);
        assert_eq!(c.classify_point(at(5.0 - 1e-8)).unwrap(), PointClass::OnBoundary);
        assert_eq!(c.classify_point(at(5.0 + 1e-4)).unwrap(), PointClass::Outside);
        assert_eq!(c.classify_point(at(5.0 - 1e-4)).unwrap(), PointClass::Inside);
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
