// SPDX-License-Identifier: Apache-2.0
//! Support faces of a Hole, from the transition that drills it.

use super::same_axis_line;
use crate::history_records::{AsmHistoricalPlane, AsmHistoricalTopology, AsmHistoricalTransition};
use crate::records::feature::hole::DesignHoleConstruction;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashSet;

/// Relative tolerance for a support plane normal lying along the Hole axis.
const HOLE_SUPPORT_NORMAL_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for a Hole entry point lying on its support plane.
const HOLE_SUPPORT_POINT_TOLERANCE: f64 = 1.0e-8;
/// Model millimetres per source centimetre.
const MILLIMETRES_PER_SOURCE_UNIT: f64 = 10.0;

/// Faces of the state before a Hole that carry its entry points, sorted and
/// distinct.
///
/// Every cylinder that the transition inserts lies on the axis of an entry
/// point, and every entry point has such a cylinder. Each entry point lies on
/// exactly one plane of a face that the transition updates: that plane is
/// normal to the axis, and the transition gives the face a circular edge
/// centred on the entry point. Historical topology stores no face sense, so a
/// plane normal of either sign along the axis qualifies.
pub(super) fn hole_support_faces(
    construction: &DesignHoleConstruction,
    transition: &AsmHistoricalTransition,
    preceding: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let axis = Vector3::new(
        construction.direction[0].get(),
        construction.direction[1].get(),
        construction.direction[2].get(),
    )
    .unit()?;
    let entries: Vec<Point3> = construction
        .entry_points()?
        .into_iter()
        .map(|point| {
            Point3::new(
                point[0].get() * MILLIMETRES_PER_SOURCE_UNIT,
                point[1].get() * MILLIMETRES_PER_SOURCE_UNIT,
                point[2].get() * MILLIMETRES_PER_SOURCE_UNIT,
            )
        })
        .collect();
    let cylinders = inserted_cylinder_axes(transition)?;
    let coaxial =
        |entry: &Point3, cylinder: &(Point3, Vector3)| same_axis_line(*cylinder, (*entry, axis));
    if !entries
        .iter()
        .all(|entry| cylinders.iter().any(|cylinder| coaxial(entry, cylinder)))
        || !cylinders
            .iter()
            .all(|cylinder| entries.iter().any(|entry| coaxial(entry, cylinder)))
    {
        return None;
    }
    let planes = updated_planes(transition, preceding);
    let mut faces = Vec::with_capacity(entries.len());
    for entry in &entries {
        let mut carriers = planes.iter().filter(|(face, plane)| {
            plane_carries_entry(plane, *entry, axis)
                && face_has_rim(transition, *face, *entry, axis)
        });
        let (face, _) = carriers.next()?;
        if carriers.next().is_some() {
            return None;
        }
        faces.push(*face);
    }
    faces.sort_unstable();
    faces.dedup();
    Some(faces)
}

/// Unit axis lines of the cylinders on faces that the transition inserts.
fn inserted_cylinder_axes(transition: &AsmHistoricalTransition) -> Option<Vec<(Point3, Vector3)>> {
    let inserted: HashSet<i64> = transition.topology.faces.inserted.iter().copied().collect();
    let carriers: HashSet<i64> = transition
        .rows
        .face_surfaces
        .set
        .iter()
        .filter(|binding| inserted.contains(&binding.entity))
        .map(|binding| binding.carrier)
        .collect();
    transition
        .rows
        .surface_cylinders
        .set
        .iter()
        .filter(|cylinder| carriers.contains(&cylinder.surface))
        .map(|cylinder| Some((cylinder.origin, cylinder.axis.unit()?)))
        .collect()
}

/// Planes of the preceding state's faces that the transition updates. A face
/// with more than one carrier or plane row has no plane here.
fn updated_planes<'a>(
    transition: &AsmHistoricalTransition,
    preceding: &'a AsmHistoricalTopology,
) -> Vec<(i64, &'a AsmHistoricalPlane)> {
    let faces: HashSet<i64> = preceding.faces.iter().copied().collect();
    transition
        .topology
        .faces
        .updated
        .iter()
        .copied()
        .filter(|face| faces.contains(face))
        .filter_map(|face| {
            let mut bindings = preceding
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == face);
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut planes = preceding
                .surface_planes
                .iter()
                .filter(|plane| plane.surface == binding.carrier);
            let plane = planes.next()?;
            planes.next().is_none().then_some((face, plane))
        })
        .collect()
}

fn plane_carries_entry(plane: &AsmHistoricalPlane, entry: Point3, axis: Vector3) -> bool {
    let Some(normal) = plane.normal.unit() else {
        return false;
    };
    (normal.dot(axis).abs() - 1.0).abs() <= HOLE_SUPPORT_NORMAL_TOLERANCE
        && entry.vector_from(plane.origin).dot(normal).abs()
            <= HOLE_SUPPORT_POINT_TOLERANCE * coordinate_scale(entry, plane.origin)
}

/// Whether a loop of `face` in the transition rows has an edge on a circle
/// centred on `entry` along `axis`.
fn face_has_rim(
    transition: &AsmHistoricalTransition,
    face: i64,
    entry: Point3,
    axis: Vector3,
) -> bool {
    let rows = &transition.rows;
    rows.face_loops
        .set
        .iter()
        .filter(|relation| relation.owner_ref == face)
        .flat_map(|relation| &relation.member_refs)
        .flat_map(|face_loop| {
            rows.loop_coedges
                .set
                .iter()
                .filter(move |relation| relation.owner_ref == *face_loop)
        })
        .flat_map(|relation| &relation.member_refs)
        .flat_map(|coedge| {
            rows.coedge_topology
                .set
                .iter()
                .filter(move |row| row.coedge == *coedge)
        })
        .flat_map(|coedge| {
            rows.edge_curves
                .set
                .iter()
                .filter(move |row| row.entity == coedge.edge)
        })
        .filter_map(|edge| edge.carrier)
        .flat_map(|curve| {
            rows.curve_axes
                .set
                .iter()
                .filter(move |row| row.curve == curve)
        })
        .any(|circle| {
            circle.direction.unit().is_some_and(|direction| {
                (direction.dot(axis).abs() - 1.0).abs() <= HOLE_SUPPORT_NORMAL_TOLERANCE
            }) && entry.vector_from(circle.origin).norm()
                <= HOLE_SUPPORT_POINT_TOLERANCE * coordinate_scale(entry, circle.origin)
        })
}

fn coordinate_scale(left: Point3, right: Point3) -> f64 {
    [left.x, left.y, left.z, right.x, right.y, right.z]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max)
}

#[cfg(test)]
mod tests;
