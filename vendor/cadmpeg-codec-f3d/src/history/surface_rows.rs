// SPDX-License-Identifier: Apache-2.0
//! Plane and cylinder rows of the faces of one historical state.

use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalPlane, AsmHistoricalTopology,
};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) enum Carrier<'a> {
    Plane(&'a AsmHistoricalPlane),
    Cylinder(&'a AsmHistoricalCylinder),
}

/// Rows of one state, keyed by slot. A slot with two rows maps to `None`.
fn unique_rows<'a, R>(
    rows: impl IntoIterator<Item = &'a R>,
    slot: impl Fn(&R) -> i64,
) -> HashMap<i64, Option<&'a R>> {
    let mut unique = HashMap::new();
    for row in rows {
        unique
            .entry(slot(row))
            .and_modify(|found| *found = None)
            .or_insert(Some(row));
    }
    unique
}

/// The face bindings and the plane and cylinder rows of one state.
pub(super) struct StateRows<'a> {
    bindings: HashMap<i64, Option<&'a AsmHistoricalCarrierBinding>>,
    planes: HashMap<i64, Option<&'a AsmHistoricalPlane>>,
    cylinders: HashMap<i64, Option<&'a AsmHistoricalCylinder>>,
}

impl<'a> StateRows<'a> {
    pub(super) fn new(topology: &'a AsmHistoricalTopology) -> Self {
        Self {
            bindings: unique_rows(&topology.face_surfaces, |row| row.entity),
            planes: unique_rows(&topology.surface_planes, |row| row.surface),
            cylinders: unique_rows(&topology.surface_cylinders, |row| row.surface),
        }
    }

    /// The one plane or cylinder row of a face's surface.
    pub(super) fn carrier(&self, face: i64) -> Option<Carrier<'a>> {
        let surface = (*self.bindings.get(&face)?)?.carrier;
        match (self.planes.get(&surface), self.cylinders.get(&surface)) {
            (Some(plane), None) => plane.map(Carrier::Plane),
            (None, Some(cylinder)) => cylinder.map(Carrier::Cylinder),
            _ => None,
        }
    }
}
