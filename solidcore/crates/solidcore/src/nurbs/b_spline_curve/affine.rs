//! The image of a B-spline curve under an affine map: the same degree, knots and
//! weights, each control point carried by the map ([`crv_nurb::Curve::transform`], the
//! kernel's own carrying of a net by a 4x4 matrix). An affine map carries a NURBS curve
//! exactly by its control points (it commutes with the curve's convex combinations,
//! and leaves the weights alone), so the image is exact up to the rounding of each
//! carried point. The weights must be positive: only then is every point of the curve
//! a convex combination of its control points, so within their bound.
//!
//! Not upstream: SolidCraft's station sweep (`placed`). Deviations: none (no upstream
//! counterpart). No recursion.
//!
//! [`crv_nurb::Curve::transform`]: crate::nurbs::crv_nurb::Curve::transform

use crate::error::{Result, SolidError};
use crate::geom::certified::gamma;
use crate::math::Transform;
use crate::nurbs::b_spline_curve::p1::matrix_of;
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

const OP: &str = "nurbs::b_spline_curve::affine::BSplineCurve::affine_image";

impl BSplineCurve {
    /// The image of this curve under the affine map `t`: a new plain B-spline of the
    /// same degree, knots and weights, its control points carried by `t`; with the
    /// bound on how far any of its control points (so any of its points: with positive
    /// weights the curve is their convex combination) lies from the exact image. Each
    /// coordinate of a carried point is its Euclidean point (one division by the
    /// weight) times a column of `t` summed (three products, three sums), weighted again
    /// and divided once more where the curve is evaluated: `gamma_9` of the sizes of its
    /// terms, doubled from a coordinate to the vector (`sqrt 3 < 2`).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a map not proved regular ([`Transform::is_regular`] at ratio
    /// 0: a singular map flattens the curve), a curve not of dimension 3, or a weight
    /// not finite and positive; those of [`BSplineCurve::canonical`],
    /// [`BSplineCurve::create_canonical`] and the carrying.
    pub fn affine_image(&self, t: &Transform) -> Result<(Self, f64)> {
        if !t.is_regular(0.0).is_yes() {
            return Err(SolidError::invalid_input(
                OP,
                "the map is not proved regular",
            ));
        }
        let can = self.canonical()?;
        if can.dim != 3 {
            return Err(SolidError::invalid_input(OP, "the curve is not 3D"));
        }
        if !can.weights.iter().all(|w| w.is_finite() && *w > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a weight not finite and positive",
            ));
        }
        let [x, y, z, w] = t.m;
        let mut bound = 0.0_f64;
        for &p in &can.control_points {
            // Each coordinate's terms: the rows' entries in its column.
            for ((mx, my), (mz, mw)) in x.iter().zip(&y).zip(z.iter().zip(&w)).take(3) {
                let size = (mx * p.x).abs() + (my * p.y).abs() + (mz * p.z).abs() + mw.abs();
                bound = bound.max(gamma(9) * size);
            }
        }
        // A plain copy of the curve's numbers, then carried by the kernel's matrix
        // (column `j` of `t`'s linear part and translation as row `j`).
        let data = CanonicalData {
            dim: 3,
            degree: can.degree,
            control_points: &can.control_points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &can.knot_multiplicities,
            knots: &can.knots,
            knot_type: KnotType::Unspecified,
            weights: (!can.weights.is_empty()).then_some(can.weights.as_slice()),
            trim: None,
        };
        let mut image = Self::create_canonical(&data)?;
        let matrix = matrix_of(&[
            [x[0], y[0], z[0], w[0]],
            [x[1], y[1], z[1], w[1]],
            [x[2], y[2], z[2], w[2]],
            [x[3], y[3], z[3], w[3]],
        ])?;
        image.get_or_create_gw_nurb()?.transform(&matrix)?;
        Ok((image, 2.0 * bound))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::arc::ArcDefinition;
    use crate::curve::ellipse::Ellipse;
    use crate::geom::vector3d::Point3;
    use crate::geom::vector3d::Vec3;
    use crate::kernel::Kernel;

    #[test]
    fn a_stretched_circle_is_the_ellipse_of_the_stretch() {
        let k = Kernel::default();
        let arc = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::Z,
            reference: Vec3::X,
            radius: 1.0,
            start: 0.0,
            end: std::f64::consts::TAU,
        };
        let circle = Ellipse::arc(&k, &arc).unwrap();
        // x * 2, y * 3, moved by (1, 1, 1): the ellipse ((x - 1)/2)^2 + ((y - 1)/3)^2 = 1.
        // TODO(d0b44726f): takes the arc's net as on its circle (bound 0 by its label).
        let t = Transform::from_axes(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::Z,
            Vec3::new(1.0, 1.0, 1.0),
        );
        let (image, bound) = circle.base().affine_image(&t).unwrap();
        assert!(bound > 0.0 && bound < 1e-14);
        assert_eq!(
            image.canonical().unwrap().weights,
            circle.base().canonical().unwrap().weights
        );
        let iv = image.natural_interval();
        for i in 0..=16 {
            let s = iv.min() + (iv.max() - iv.min()) * f64::from(i) / 16.0;
            let p = image.evaluate_point(s).unwrap();
            let q = circle.base().evaluate_point(s).unwrap();
            // The image of each point is the point of the image.
            assert!((p - t.transform_point(q)).length() < 1e-13);
            let (x, y) = ((p.x - 1.0) / 2.0, (p.y - 1.0) / 3.0);
            assert!((x * x + y * y - 1.0).abs() < 1e-13);
            assert!((p.z - 1.0).abs() < 1e-15);
        }
    }

    #[test]
    fn a_weight_not_positive_is_refused() {
        // A quadratic whose middle weight is made negative in its net (the canonical
        // constructor refuses one, but a net edited in place can carry it): its points are
        // no convex combination of its control points, so no bound holds.
        let points = [
            Point3::ZERO,
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ];
        let data = CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[3, 3],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&[1.0, 0.5, 1.0]),
            trim: None,
        };
        let mut c = BSplineCurve::create_canonical(&data).unwrap();
        assert!(c.affine_image(&Transform::IDENTITY).is_ok());
        for w in [-0.5, 0.0] {
            if let Some(p) = c.get_or_create_gw_nurb().unwrap().cpts_mut().get_mut(1) {
                p.w = w;
            }
            assert_eq!(c.canonical().unwrap().weights.get(1), Some(&w));
            assert!(matches!(
                c.affine_image(&Transform::IDENTITY),
                Err(SolidError::InvalidInput { .. })
            ));
        }
    }

    #[test]
    fn a_singular_map_is_refused() {
        let line = crate::curve::line::Line::create_line_segment(3, Point3::ZERO, Point3::X, None)
            .unwrap();
        let b = line.base_with_nurb().unwrap().into_owned();
        let flat = Transform::from_axes(Vec3::X, Vec3::Y, Vec3::ZERO, Point3::ZERO);
        assert!(matches!(
            b.affine_image(&flat),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
