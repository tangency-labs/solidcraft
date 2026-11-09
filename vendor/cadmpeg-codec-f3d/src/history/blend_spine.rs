// SPDX-License-Identifier: Apache-2.0
//! The edge that a constant-radius blend face replaces.

use crate::history_records::AsmHistoricalTopology;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{HashMap, HashSet};

/// Millimetres between a circle centre and the torus axis line that holds it.
const BLEND_SPINE_POSITION_TOLERANCE: f64 = 1.0e-6;
/// Cross or dot product of two unit directions that are parallel or
/// perpendicular.
const BLEND_SPINE_DIRECTION_TOLERANCE: f64 = 1.0e-9;
/// Millimetres between a distance from a cylinder axis line and the cylinder
/// radius that equals it.
const BLEND_RADIUS_DISTANCE_TOLERANCE: f64 = 1.0e-6;
/// Millimetres between the radius of a cylinder row and the radius row of
/// its carrier.
const CYLINDER_ROW_RADIUS_TOLERANCE: f64 = 1.0e-6;

/// The edge geometry that a blend carrier admits as its replaced edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BlendSpine {
    /// A cylinder blend replaces a straight edge parallel to the cylinder axis.
    Line { direction: Vector3 },
    /// A torus blend replaces a circle around the torus axis.
    Circle { origin: Point3, direction: Vector3 },
}

/// The axis row and midpoint tangent of one edge.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EdgeGeometry {
    axis: Option<(Point3, Vector3)>,
    tangent: Option<Vector3>,
}

impl EdgeGeometry {
    /// Whether the edge has the finite axis row and the finite nonzero
    /// directions that `BlendSpine::admits` compares.
    pub(super) fn is_classified(self) -> bool {
        self.axis
            .is_some_and(|(origin, direction)| finite(origin) && direction.unit().is_some())
            && self.tangent.is_some_and(|tangent| tangent.unit().is_some())
    }
}

/// The axis line and row radius of a cylinder blend carrier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CylinderAxis {
    origin: Point3,
    direction: Vector3,
    radius: f64,
}

impl CylinderAxis {
    /// The distance of `point` from the axis line.
    pub(super) fn distance(self, point: Point3) -> f64 {
        point.vector_from(self.origin).cross(self.direction).norm()
    }

    /// Whether `point` lies farther than `radius` from the axis line.
    pub(super) fn clears(self, radius: f64, point: Point3) -> bool {
        self.distance(point) > radius + BLEND_RADIUS_DISTANCE_TOLERANCE
    }

    /// Whether `point` lies on the cylinder of `radius` about the axis.
    pub(super) fn meets(self, radius: f64, point: Point3) -> bool {
        (self.distance(point) - radius).abs() <= BLEND_RADIUS_DISTANCE_TOLERANCE
    }

    /// The position of `point` along the axis.
    pub(super) fn position(self, point: Point3) -> f64 {
        point.vector_from(self.origin).dot(self.direction)
    }

    /// Whether the cylinder row's radius is within
    /// `CYLINDER_ROW_RADIUS_TOLERANCE` of `radius`.
    pub(super) fn has_radius(self, radius: f64) -> bool {
        (self.radius - radius).abs() <= CYLINDER_ROW_RADIUS_TOLERANCE
    }

    /// Whether a plane touches the cylinder of `radius` about the axis: its
    /// normal is perpendicular to the axis and its distance from the axis
    /// line equals the radius. `None` when the plane has a non-finite origin
    /// or a zero or non-finite normal.
    pub(super) fn touches(self, radius: f64, (origin, normal): (Point3, Vector3)) -> Option<bool> {
        let normal = normal.unit().filter(|_| finite(origin))?;
        Some(
            perpendicular(normal, self.direction)
                && (self.origin.vector_from(origin).dot(normal).abs() - radius).abs()
                    <= BLEND_RADIUS_DISTANCE_TOLERANCE,
        )
    }
}

/// The axis line of each surface with one cylinder row whose origin is
/// finite and whose direction is finite and nonzero.
pub(super) fn cylinder_axes(result: &AsmHistoricalTopology) -> HashMap<i64, CylinderAxis> {
    unique_rows(&result.surface_cylinders, |row| {
        (row.surface, (row.origin, row.axis, row.radius))
    })
    .into_iter()
    .filter_map(|(surface, row)| {
        let (origin, axis, radius) = row?;
        let direction = axis.unit().filter(|_| finite(origin))?;
        Some((
            surface,
            CylinderAxis {
                origin,
                direction,
                radius,
            },
        ))
    })
    .collect()
}

/// The interior point of each sampled edge, or `None` for an edge with two
/// samples or a non-finite point.
pub(super) fn edge_points(topology: &AsmHistoricalTopology) -> HashMap<i64, Option<Point3>> {
    unique_rows(&topology.edge_samples, |row| (row.edge, row.interior))
        .into_iter()
        .map(|(edge, point)| (edge, point.filter(|point| finite(*point))))
        .collect()
}

/// The origin and normal of each plane carrier, or `None` for a carrier with
/// two plane rows or with a cylinder, axis, or radius row.
pub(super) fn plane_rows(
    topology: &AsmHistoricalTopology,
) -> HashMap<i64, Option<(Point3, Vector3)>> {
    let curved = topology
        .surface_cylinders
        .iter()
        .map(|row| row.surface)
        .chain(topology.surface_axes.iter().map(|row| row.surface))
        .chain(topology.surface_radii.iter().map(|row| row.surface))
        .collect::<HashSet<_>>();
    let mut planes = unique_rows(&topology.surface_planes, |row| {
        (row.surface, (row.origin, row.normal))
    });
    for (surface, plane) in &mut planes {
        if curved.contains(surface) {
            *plane = None;
        }
    }
    planes
}

fn finite(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

/// Rows of one state, keyed by slot. A slot with two rows maps to `None`.
fn unique_rows<'a, R, V>(
    rows: impl IntoIterator<Item = &'a R>,
    row: impl Fn(&'a R) -> (i64, V),
) -> HashMap<i64, Option<V>>
where
    R: 'a,
{
    let mut unique = HashMap::new();
    for entry in rows {
        let (slot, value) = row(entry);
        unique
            .entry(slot)
            .and_modify(|found| *found = None)
            .or_insert(Some(value));
    }
    unique
}

fn unique<V: Copy>(rows: &HashMap<i64, Option<V>>, slot: i64) -> Option<V> {
    rows.get(&slot).copied().flatten()
}

/// The spine of each new carrier in a result state. A carrier with a cylinder
/// row is a cylinder. A carrier with an axis and a radius and without a
/// cylinder row is a torus. Other carriers have no spine.
pub(super) fn blend_spines(result: &AsmHistoricalTopology) -> HashMap<i64, BlendSpine> {
    let cylinders = unique_rows(&result.surface_cylinders, |row| (row.surface, row.axis));
    let axes = unique_rows(&result.surface_axes, |row| {
        (row.surface, (row.origin, row.direction))
    });
    let radii = unique_rows(&result.surface_radii, |row| (row.surface, row.radius));
    let mut spines = HashMap::new();
    for &surface in &result.surfaces {
        let spine = match (cylinders.get(&surface), unique(&axes, surface)) {
            (Some(Some(axis)), _) => axis.unit().map(|direction| BlendSpine::Line { direction }),
            (None, Some((origin, axis))) if unique(&radii, surface).is_some() && finite(origin) => {
                axis.unit()
                    .map(|direction| BlendSpine::Circle { origin, direction })
            }
            _ => None,
        };
        if let Some(spine) = spine {
            spines.insert(surface, spine);
        }
    }
    spines
}

/// The axis row and midpoint tangent of each edge in a preceding state.
pub(super) fn edge_geometry(preceding: &AsmHistoricalTopology) -> HashMap<i64, EdgeGeometry> {
    let curves = unique_rows(&preceding.edge_curves, |row| (row.entity, row.carrier));
    let axes = unique_rows(&preceding.curve_axes, |row| {
        (row.curve, (row.origin, row.direction))
    });
    let tangents = unique_rows(&preceding.edge_samples, |row| (row.edge, row.tangent));
    let mut geometry = HashMap::new();
    for &edge in &preceding.edges {
        let axis = unique(&curves, edge)
            .flatten()
            .and_then(|curve| unique(&axes, curve));
        let tangent = unique(&tangents, edge).flatten();
        geometry.insert(edge, EdgeGeometry { axis, tangent });
    }
    geometry
}

fn parallel(left: Vector3, right: Vector3) -> bool {
    left.unit()
        .is_some_and(|left| left.cross(right).norm() <= BLEND_SPINE_DIRECTION_TOLERANCE)
}

fn perpendicular(left: Vector3, right: Vector3) -> bool {
    left.unit()
        .is_some_and(|left| left.dot(right).abs() <= BLEND_SPINE_DIRECTION_TOLERANCE)
}

impl BlendSpine {
    /// Whether the edge's midpoint tangent shows that the blend cannot
    /// replace it: a line spine needs a tangent along its axis, and a circle
    /// spine a tangent perpendicular to its axis.
    pub(super) fn rules_out(self, edge: EdgeGeometry) -> bool {
        let Some(tangent) = edge.tangent.filter(|tangent| tangent.unit().is_some()) else {
            return false;
        };
        match self {
            Self::Line { direction } => !parallel(tangent, direction),
            Self::Circle { direction, .. } => !perpendicular(tangent, direction),
        }
    }

    /// Whether the blend can replace an edge with this geometry. A line edge
    /// runs along its axis row; a circle edge has its tangent perpendicular
    /// to its axis row.
    pub(super) fn admits(self, edge: EdgeGeometry) -> bool {
        let (Some((origin, axis)), Some(tangent)) = (edge.axis, edge.tangent) else {
            return false;
        };
        match self {
            Self::Line { direction } => parallel(axis, direction) && parallel(tangent, direction),
            Self::Circle {
                origin: centre,
                direction,
            } => {
                parallel(axis, direction)
                    && perpendicular(tangent, direction)
                    && origin.vector_from(centre).cross(direction).norm()
                        <= BLEND_SPINE_POSITION_TOLERANCE
            }
        }
    }
}

#[cfg(test)]
mod tests;
