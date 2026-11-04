// SPDX-License-Identifier: Apache-2.0
//! The edge that a constant-radius blend face replaces.

use crate::history_records::AsmHistoricalTopology;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashMap;

/// Millimetres between a circle centre and the torus axis line that holds it.
const BLEND_SPINE_POSITION_TOLERANCE: f64 = 1.0e-6;
/// Cross or dot product of two unit directions that are parallel or
/// perpendicular.
const BLEND_SPINE_DIRECTION_TOLERANCE: f64 = 1.0e-9;

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
            (None, Some((origin, axis))) if unique(&radii, surface).is_some() => axis
                .unit()
                .map(|direction| BlendSpine::Circle { origin, direction }),
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
