//! Ellipses and circles (`SmEllipse`): the analytic curve `O + rx cos(s) X + ry sin(s)
//! Y` over an angular (STEP) domain in degrees, kept together with its rational
//! quadratic B-spline and the polar conversion table between the two
//! parameterisations (`SmPolarConversion`).
//!
//! Upstream: `source/SMLib/src/SmEllipse.cpp`, `source/SMLib/inc/SmEllipse.h`.
//! The file is split over several plan units (`curve.ellipse_p1`, `_p2`); each lives
//! in its own submodule (`p1` also ports `SmCircle`: `source/SMLib/src/SmCircle.cpp`,
//! `source/SMLib/inc/SmCircle.h`) and adds methods to [`Ellipse`] and [`PolarConversion`].

pub mod p1;
pub mod p2;

pub use p1::CIRCLE_CLASS_TYPE;
pub use p2::{
    Ellipse, EllipseCanonical, EllipseIntersection, EllipseOptions, EllipseStartPlanePoint,
    PolarConversion,
};

use crate::curve::curve::{Curve, CurveKind};
use crate::geom::tol::coincidence_reach_3d;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::math::gamma;

/// The radial bounds about a point or an analytic ellipse's center.
#[derive(Clone, Copy)]
pub(crate) struct RadialSupport {
    center: Point3,
    lower: f64,
    upper: f64,
}

impl RadialSupport {
    /// A point has zero radial extent.
    pub(crate) fn point(center: Point3) -> Self {
        Self {
            center,
            lower: 0.0,
            upper: 0.0,
        }
    }

    /// A genuine 3D ellipse stays between its two semi-axis radii at every angle.
    pub(crate) fn curve(curve: &Curve) -> Option<Self> {
        let CurveKind::Ellipse(ellipse) = curve.kind() else {
            return None;
        };
        if curve.dim() != 3 {
            return None;
        }
        let (x, y) = (ellipse.position().x_axis(), ellipse.position().y_axis());
        // Three-product dot products read orthonormality to rounding. The shared
        // coincidence reach covers the resulting radial uncertainty at this scale.
        if ![x.dot(x) - 1.0, y.dot(y) - 1.0, x.dot(y)]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= gamma(4))
        {
            return None;
        }
        let (rx, ry) = (ellipse.x_radius().abs(), ellipse.y_radius().abs());
        if !rx.is_finite() || !ry.is_finite() {
            return None;
        }
        Some(Self {
            center: ellipse.position().origin(),
            lower: rx.min(ry),
            upper: rx.max(ry),
        })
    }

    /// Proves separation past coincidence. Triangle inequalities hold for every arc trim
    /// and orientation. Nonfinite inputs or intermediate values give no separation proof.
    pub(crate) fn disjoint(self, k: &Kernel, zone: f64, other: Self, other_zone: f64) -> bool {
        self.gap(other).is_some_and(|(gap, size)| {
            [zone, other_zone]
                .iter()
                .all(|z| z.is_finite() && *z >= 0.0)
                && gap > coincidence_reach_3d(k, zone, other_zone, size)
        })
    }

    fn gap(self, other: Self) -> Option<(f64, f64)> {
        let (a, b) = (self.center, other.center);
        if ![a.x, a.y, a.z, b.x, b.y, b.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        let distance = a.distance(b);
        let gaps = [
            distance - self.upper - other.upper,
            self.lower - other.upper - distance,
            other.lower - self.upper - distance,
        ];
        let size = a.max_dimension().max(b.max_dimension()) + self.upper.max(other.upper);
        if !size.is_finite() || !gaps.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some((gaps.into_iter().fold(f64::NEG_INFINITY, f64::max), size))
    }
}
