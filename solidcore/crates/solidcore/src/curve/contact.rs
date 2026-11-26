//! Certified separation of native curves and fixed points at their local tolerances.
//!
//! Upstream: none.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`NativeSupport`] | none |
//!
//! Positive-weight B-splines lie in their Euclidean control hulls. A direction only
//! chooses a witness: outward-rounded projections must separate the entire hulls.
//! Deviations: no upstream counterpart. No recursion.

use crate::blend::fillet_section::interval::Ivl;
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::{Ellipse, RadialSupport};
use crate::geom::axis2_placement::axes_are_orthonormal;
use crate::geom::extent1d::Extent1d;
use crate::geom::interval_vector::{cross, dot, norm, unit};
use crate::geom::tol::coincidence_reach_3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::surface::implicit::AnalyticSupport;

/// Native geometry whose separation can be certified without a sampled approximation.
pub(crate) struct NativeSupport<'a> {
    geometry: Geometry<'a>,
    owner_zone: Option<f64>,
}

enum Geometry<'a> {
    Point(Point3),
    Ellipse(&'a Ellipse, RadialSupport),
    Hull(Vec<(Point3, f64)>),
}

impl<'a> NativeSupport<'a> {
    /// The fixed point of a native vertex.
    pub(crate) fn point(point: Point3) -> Self {
        Self {
            geometry: Geometry::Point(point),
            owner_zone: None,
        }
    }

    /// An analytic ellipse or a native positive-weight B-spline over its actual trim.
    pub(crate) fn curve(curve: &'a Curve, interval: &Extent1d) -> Option<Self> {
        if curve.dim() != 3 {
            return None;
        }
        let geometry = match curve.kind() {
            CurveKind::Ellipse(ellipse) => Geometry::Ellipse(ellipse, RadialSupport::curve(curve)?),
            CurveKind::BSpline(_) => {
                let hull = curve.hull_points_over(interval)?;
                if hull.is_empty()
                    || !hull.iter().all(|(p, e)| {
                        [p.x, p.y, p.z, *e].iter().all(|v| v.is_finite()) && *e >= 0.0
                    })
                {
                    return None;
                }
                Geometry::Hull(hull)
            }
            _ => return None,
        };
        Some(Self {
            geometry,
            owner_zone: curve.owner_zone_tol_3d(),
        })
    }

    /// A radial or projection lower bound proves the native supports farther apart
    /// than their coincidence reach. Unknown data and unsupported pairs give no proof.
    pub(crate) fn disjoint(&self, k: &Kernel, zone: f64, other: &Self, other_zone: f64) -> bool {
        let (Some(zone), Some(other_zone)) = (self.zone(zone), other.zone(other_zone)) else {
            return false;
        };
        if let (Some(a), Some(b)) = (self.radial(), other.radial()) {
            if a.disjoint(k, zone, b, other_zone) {
                return true;
            }
        }
        let normal = match (&self.geometry, &other.geometry) {
            (Geometry::Ellipse(ellipse, _), _) | (_, Geometry::Ellipse(ellipse, _)) => {
                Some(ellipse.position().z_axis())
            }
            (Geometry::Hull(hull), Geometry::Point(_))
            | (Geometry::Point(_), Geometry::Hull(hull)) => {
                let mut best = Vec3::ZERO;
                let mut largest = 0.0;
                for triple in hull.windows(3) {
                    let [(a, _), (b, _), (c, _)] = triple else {
                        continue;
                    };
                    let candidate = (*b - *a).cross(*c - *a);
                    let length = candidate.length();
                    if length.is_finite() && length > largest {
                        largest = length;
                        best = candidate;
                    }
                }
                best.unitize().ok()
            }
            _ => None,
        };
        let Some(normal) = normal else { return false };
        let Some(axis) = unit(xyz(normal)) else {
            return false;
        };
        let Some(origin) = self.origin() else {
            return false;
        };
        let (Some(a), Some(b)) = (self.project(origin, axis), other.project(origin, axis)) else {
            return false;
        };
        let gap = a - b;
        let size = self.size().max(other.size());
        gap.is_finite()
            && size.is_finite()
            && gap.mig() > coincidence_reach_3d(k, zone, other_zone, size)
    }

    /// The entire native boundary clears a foreign analytic surface's local band.
    /// Ellipse radial distances use bounded first and second harmonics of its angle.
    /// Other geometry gives no proof unless a plane separates its positive control hull.
    pub(crate) fn outside_surface(
        &self,
        k: &Kernel,
        zone: f64,
        support: &AnalyticSupport,
        surface_zone: f64,
    ) -> bool {
        let Some(zone) = self.zone(zone) else {
            return false;
        };
        if !surface_zone.is_finite() || surface_zone < 0.0 {
            return false;
        }
        let (placement, radius) = match support {
            AnalyticSupport::Plane { placement, .. } => (placement, 0.0),
            AnalyticSupport::Sphere { placement, radius }
            | AnalyticSupport::Cylinder { placement, radius } => (placement, *radius),
            _ => return false,
        };
        let origin = placement.origin();
        let (x_axis, y_axis) = (placement.x_axis(), placement.y_axis());
        if !axes_are_orthonormal(x_axis, y_axis)
            || !radius.is_finite()
            || radius < 0.0
            || ![origin.x, origin.y, origin.z].iter().all(|v| v.is_finite())
        {
            return false;
        }
        let Some(axis) = unit(cross(xyz(x_axis), xyz(y_axis))) else {
            return false;
        };
        let distance = if matches!(support, AnalyticSupport::Plane { .. }) {
            let Some(projected) = self.project(origin, axis) else {
                return false;
            };
            projected
        } else {
            let radial = |vector: [Ivl; 3]| {
                if matches!(support, AnalyticSupport::Cylinder { .. }) {
                    let along = dot(vector, axis);
                    let [x, y, z] = vector;
                    let [nx, ny, nz] = axis;
                    [x - nx * along, y - ny * along, z - nz * along]
                } else {
                    vector
                }
            };
            let delta = |point: Point3| {
                let [x, y, z] = xyz(point);
                let [ox, oy, oz] = xyz(origin);
                radial([x - ox, y - oy, z - oz])
            };
            let radial_distance = match &self.geometry {
                Geometry::Point(point) => norm(delta(*point)),
                Geometry::Ellipse(ellipse, _) => {
                    let position = ellipse.position();
                    let center = delta(position.origin());
                    let a =
                        radial(xyz(position.x_axis()).map(|v| v * Ivl::from(ellipse.x_radius())));
                    let b =
                        radial(xyz(position.y_axis()).map(|v| v * Ivl::from(ellipse.y_radius())));
                    let aa = dot(a, a);
                    let bb = dot(b, b);
                    let half = Ivl::from(0.5);
                    let mean = dot(center, center) + (aa + bb) * half;
                    let first = norm([
                        dot(center, a) * Ivl::from(2.0),
                        dot(center, b) * Ivl::from(2.0),
                        Ivl::from(0.0),
                    ]);
                    let second = norm([(aa - bb) * half, dot(a, b), Ivl::from(0.0)]);
                    let variation = first + second;
                    (mean + Ivl::new(-variation.hi, variation.hi)).sqrt()
                }
                Geometry::Hull(_) => return false,
            };
            radial_distance - Ivl::from(radius)
        };
        let size = self.size().max(origin.max_dimension()) + radius;
        distance.is_finite()
            && size.is_finite()
            && distance.mig() > coincidence_reach_3d(k, zone, surface_zone, size)
    }

    /// Includes the actual owning curve's zone, abstaining on unknown numeric bounds.
    pub(crate) fn zone(&self, zone: f64) -> Option<f64> {
        if !zone.is_finite()
            || zone < 0.0
            || self.owner_zone.is_some_and(|z| !z.is_finite() || z < 0.0)
        {
            return None;
        }
        Some(zone.max(self.owner_zone.unwrap_or(zone)))
    }

    fn radial(&self) -> Option<RadialSupport> {
        match &self.geometry {
            Geometry::Point(point) => Some(RadialSupport::point(*point)),
            Geometry::Ellipse(_, radial) => Some(*radial),
            Geometry::Hull(_) => None,
        }
    }

    fn origin(&self) -> Option<Point3> {
        match &self.geometry {
            Geometry::Point(point) => Some(*point),
            Geometry::Ellipse(ellipse, _) => Some(ellipse.position().origin()),
            Geometry::Hull(hull) => hull.first().map(|(point, _)| *point),
        }
    }

    fn size(&self) -> f64 {
        match &self.geometry {
            Geometry::Point(point) => point.max_dimension(),
            Geometry::Ellipse(ellipse, _) => {
                ellipse.position().origin().max_dimension()
                    + ellipse.x_radius().abs().max(ellipse.y_radius().abs())
            }
            Geometry::Hull(hull) => hull
                .iter()
                .fold(0.0_f64, |m, (p, e)| m.max(p.max_dimension() + e)),
        }
    }

    /// Encloses `(point - origin) · axis` over the entire native support.
    pub(crate) fn project(&self, origin: Point3, axis: [Ivl; 3]) -> Option<Ivl> {
        let projection = |point: Point3, error: f64| {
            let [x, y, z] = xyz(point);
            let [ox, oy, oz] = xyz(origin);
            let e = Ivl::new(-error, error);
            dot([x + e - ox, y + e - oy, z + e - oz], axis)
        };
        let projected = match &self.geometry {
            Geometry::Point(point) => projection(*point, 0.0),
            Geometry::Ellipse(ellipse, _) => {
                let position = ellipse.position();
                let width = norm([
                    dot(xyz(position.x_axis()), axis) * Ivl::from(ellipse.x_radius()),
                    dot(xyz(position.y_axis()), axis) * Ivl::from(ellipse.y_radius()),
                    Ivl::from(0.0),
                ]);
                projection(position.origin(), 0.0) + Ivl::new(-width.hi, width.hi)
            }
            Geometry::Hull(hull) => {
                let mut points = hull.iter();
                let &(point, error) = points.next()?;
                points.fold(projection(point, error), |p, &(point, error)| {
                    p.hull(projection(point, error))
                })
            }
        };
        projected.is_finite().then_some(projected)
    }
}

fn xyz(point: Point3) -> [Ivl; 3] {
    [Ivl::from(point.x), Ivl::from(point.y), Ivl::from(point.z)]
}
