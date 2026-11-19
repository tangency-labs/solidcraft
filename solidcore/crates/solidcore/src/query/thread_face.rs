//! Exact axial measurements of round faces and of a cone continuing a cylindrical shank.

use crate::geom::geom_utility::p1::line_point_distance;
use crate::Vec3;

/// The radial profile of a cylinder or of a cone, measured from its apex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Profile {
    /// A constant cylinder radius.
    Cylinder(f64),
    /// The cone's half-angle in radians, with its input axis pointing away from the apex.
    Cone(f64),
}

/// A round face's axis, axial trim and linear radial profile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxialFace {
    /// A point on the axis. For cones it is the apex.
    pub origin: [f64; 3],
    /// The unit axis, oriented consistently across faces.
    pub axis: [f64; 3],
    /// The lowest and highest distances along the axis from `origin`.
    pub extent: [f64; 2],
    /// Radius at the wide end, or the constant cylinder radius.
    pub radius: f64,
    /// Signed change in radius per unit along the axis.
    pub slope: f64,
    /// Whether material is on the outside of the face.
    pub internal: bool,
}

/// Why a neighbouring conical face cannot continue the shank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointGeometryError {
    /// The two axes do not coincide.
    NonCoaxial,
    /// The cone is internal or widens away from the selected end.
    Widens,
    /// The two surfaces do not meet at the shank's radius.
    RadiusMismatch,
}

/// Whether a direction must reverse to match the canonical orientation of a round face.
#[must_use]
pub fn reverses_axis([x, y, z]: [f64; 3]) -> bool {
    x + y * 1e-3 + z * 1e-6 < 0.0
}

impl AxialFace {
    /// Measure a round face bounded by rims normal to its axis and straight generators.
    /// Axial extremes occur at the vertices, including an apex if present.
    #[must_use]
    pub fn measure(
        origin: [f64; 3],
        axis: [f64; 3],
        profile: Profile,
        internal: bool,
        vertices: impl IntoIterator<Item = [f64; 3]>,
    ) -> Option<Self> {
        let raw_axis = Vec3::from_array(axis);
        let length = raw_axis.length();
        if !length.is_finite() || length == 0.0 {
            return None;
        }
        let original = raw_axis / length;
        let direction = if reverses_axis(original.to_array()) {
            -original
        } else {
            original
        };
        let origin_point = Vec3::from_array(origin);
        let [start, end] = vertices
            .into_iter()
            .map(|p| Vec3::from_array(p).distance_along(origin_point, direction))
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], z| {
                [lo.min(z), hi.max(z)]
            });
        let (radius, slope) = match profile {
            Profile::Cylinder(radius) => (radius, 0.0),
            Profile::Cone(half_angle) => {
                let slope = original.dot(direction) * half_angle.tan();
                (wide_end([start, end], slope) * slope, slope)
            }
        };
        (start.is_finite() && end > start && radius.is_finite() && radius > 0.0).then_some(Self {
            origin,
            axis: direction.to_array(),
            extent: [start, end],
            radius,
            slope,
            internal,
        })
    }

    /// Project a point into the face's axial coordinates.
    #[must_use]
    pub fn project(self, point: [f64; 3]) -> f64 {
        self.project_vec(Vec3::from_array(point))
    }

    fn project_vec(self, point: Vec3) -> f64 {
        point.distance_along(Vec3::from_array(self.origin), Vec3::from_array(self.axis))
    }

    /// Index of the end of the face that is closest in axial coordinates to a point.
    #[must_use]
    pub fn nearest_end(self, point: [f64; 3]) -> usize {
        let z = self.project(point);
        let [start, end] = self.extent;
        usize::from((z - start).abs() > (z - end).abs())
    }

    /// Evaluate the face's radial profile at an axial coordinate.
    fn radius_at(self, z: f64) -> f64 {
        self.radius + self.slope * (z - wide_end(self.extent, self.slope))
    }

    /// Measure a cone continuing this shank at the given rim in shank coordinates.
    ///
    /// Returns `(far_end, slope)` in shank coordinates. `None` means this cone ends at that
    /// rim and lies entirely towards the other end. The caller establishes edge adjacency.
    ///
    /// # Errors
    ///
    /// Returns the geometric reason the cone cannot continue the shank.
    pub fn point_at_end(
        self,
        cone: Self,
        low_end: bool,
        tol: f64,
    ) -> Result<Option<(f64, f64)>, PointGeometryError> {
        let axis = Vec3::from_array(self.axis);
        let cone_axis = Vec3::from_array(cone.axis);
        let cone_origin = Vec3::from_array(cone.origin);
        let offset = cone_origin - Vec3::from_array(self.origin);
        let (axis_gap, origin_along) = line_point_distance(Vec3::ZERO, axis, offset);
        let direction = cone_axis.dot(axis);
        if direction.abs() < 1.0 - 1e-9 || axis_gap > tol {
            return Err(PointGeometryError::NonCoaxial);
        }
        let [lo, hi] = cone
            .extent
            .map(|z| self.project_vec(cone_origin + cone_axis * z));
        let [start, end] = self.extent;
        let (end, far) = if low_end {
            (start, lo.min(hi))
        } else {
            (end, lo.max(hi))
        };
        if (far - end).abs() <= tol {
            return Ok(None);
        }
        let slope = direction * cone.slope;
        if (far - end) * slope >= 0.0 || cone.internal {
            return Err(PointGeometryError::Widens);
        }
        if (cone.radius_at((end - origin_along) / direction) - self.radius).abs() > tol {
            return Err(PointGeometryError::RadiusMismatch);
        }
        Ok(Some((far, slope)))
    }
}

fn wide_end([start, end]: [f64; 2], slope: f64) -> f64 {
    if slope < 0.0 {
        start
    } else {
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_point_at_either_end_of_the_shank() {
        let origin = [0.0, 0.0, 0.0];
        let shank = AxialFace::measure(
            origin,
            [0.0, 0.0, -1.0],
            Profile::Cylinder(4.0),
            false,
            [[4.0, 0.0, 0.0], [4.0, 0.0, 3.0]],
        )
        .unwrap();

        for (end, tip, direction, low_end) in [(0.0, -8.0, 1.0, true), (3.0, 11.0, -1.0, false)] {
            let apex = [0.0, 0.0, tip];
            let cone = AxialFace::measure(
                apex,
                [0.0, 0.0, direction],
                Profile::Cone(0.5f64.atan()),
                false,
                [apex, [4.0, 0.0, end]],
            )
            .unwrap();
            assert_eq!(shank.nearest_end(apex), usize::from(!low_end));
            let (far, slope) = shank.point_at_end(cone, low_end, 1e-7).unwrap().unwrap();
            assert_eq!(far, tip);
            assert!((slope - direction * 0.5).abs() < 1e-12);
            let offset = AxialFace {
                origin: [1.0, 0.0, tip],
                ..cone
            };
            assert_eq!(
                shank.point_at_end(offset, low_end, 1e-7),
                Err(PointGeometryError::NonCoaxial)
            );
            let narrow = AxialFace {
                radius: 3.0,
                ..cone
            };
            assert_eq!(
                shank.point_at_end(narrow, low_end, 1e-7),
                Err(PointGeometryError::RadiusMismatch)
            );
        }
    }
}
