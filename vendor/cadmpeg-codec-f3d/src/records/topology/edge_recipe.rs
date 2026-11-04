// SPDX-License-Identifier: Apache-2.0
//! Edge-recipe selectors and the topology recipe structures they name.

use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// Edge-recipe topology entries sharing one selector value.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignEdgeRecipeSelectorContextWire")]
pub(crate) struct DesignEdgeRecipeSelectorContext {
    pub(crate) selector: i32,
    pub(crate) clauses: Vec<Option<DesignEdgeRecipeSelectorClause>>,
    pub(crate) incidence_matching_edge_slots: Vec<i64>,
    pub(crate) boundary_count_matching_edge_slots: Vec<i64>,
}

#[cfg(test)]
thread_local! {
    pub(super) static EDGE_RECIPE_SELECTOR_CONTEXT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignEdgeRecipeSelectorContext {
    fn clone(&self) -> Self {
        #[cfg(test)]
        EDGE_RECIPE_SELECTOR_CONTEXT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selector: self.selector,
            clauses: self.clauses.clone(),
            incidence_matching_edge_slots: self.incidence_matching_edge_slots.clone(),
            boundary_count_matching_edge_slots: self.boundary_count_matching_edge_slots.clone(),
        }
    }
}

struct EdgeRecipeClauseEntries<'a>(&'a [Option<DesignEdgeRecipeSelectorClause>]);
struct EdgeRecipeClauseSlots<'a>(&'a [Option<DesignEdgeRecipeSelectorClause>]);

impl Serialize for EdgeRecipeClauseEntries<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .map(|clause| clause.as_ref().map(|clause| &clause.entry)),
        )
    }
}

impl Serialize for EdgeRecipeClauseSlots<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .map(|clause| clause.as_ref().map(|clause| &clause.triplet_edge_slots)),
        )
    }
}

#[derive(Serialize)]
struct DesignEdgeRecipeSelectorContextRef<'a> {
    selector: i32,
    clause_entries: EdgeRecipeClauseEntries<'a>,
    clause_triplet_edge_slots: EdgeRecipeClauseSlots<'a>,
    incidence_matching_edge_slots: &'a [i64],
    #[serde(skip_serializing_if = "Option::is_none")]
    unique_incidence_edge_slot: Option<i64>,
    boundary_count_matching_edge_slots: &'a [i64],
}

impl Serialize for DesignEdgeRecipeSelectorContext {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignEdgeRecipeSelectorContextRef {
            selector: self.selector,
            clause_entries: EdgeRecipeClauseEntries(&self.clauses),
            clause_triplet_edge_slots: EdgeRecipeClauseSlots(&self.clauses),
            incidence_matching_edge_slots: &self.incidence_matching_edge_slots,
            unique_incidence_edge_slot: self.unique_incidence_edge_slot(),
            boundary_count_matching_edge_slots: &self.boundary_count_matching_edge_slots,
        }
        .serialize(serializer)
    }
}

/// One selector entry and the historical edge slots selected by its triplets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignEdgeRecipeSelectorClause {
    pub(crate) entry: DesignTopologyRecipeEntry,
    pub(crate) triplet_edge_slots: [Vec<i64>; 2],
}

impl DesignEdgeRecipeSelectorContext {
    pub(crate) fn unique_incidence_edge_slot(&self) -> Option<i64> {
        match self.incidence_matching_edge_slots.as_slice() {
            [edge] => Some(*edge),
            _ => None,
        }
    }
}

/// Serialized selector context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignEdgeRecipeSelectorContextWire {
    /// Selector value stored in each grouped entry.
    selector: i32,
    /// Entry from each ordered recipe clause; a selector occurs at most once in
    /// one clause.
    clause_entries: Vec<Option<DesignTopologyRecipeEntry>>,
    /// Changed historical edge slots at the loop position named by each of the
    /// two triplets in each present clause entry.
    clause_triplet_edge_slots: Vec<Option<[Vec<i64>; 2]>>,
    /// Changed historical edges satisfying both triplets of every present
    /// clause entry.
    incidence_matching_edge_slots: Vec<i64>,
    /// The sole incidence-compatible historical edge when the matching set is
    /// a singleton.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unique_incidence_edge_slot"
    )]
    unique_incidence_edge_slot: Option<i64>,
    /// Changed historical edges whose incident loop counts satisfy every
    /// present clause entry.
    boundary_count_matching_edge_slots: Vec<i64>,
}

impl TryFrom<DesignEdgeRecipeSelectorContextWire> for DesignEdgeRecipeSelectorContext {
    type Error = String;

    fn try_from(wire: DesignEdgeRecipeSelectorContextWire) -> Result<Self, Self::Error> {
        if wire.clause_entries.len() != wire.clause_triplet_edge_slots.len() {
            return Err("clause_triplet_edge_slots must match clause_entries length".into());
        }
        let clauses = wire
            .clause_entries
            .into_iter()
            .zip(wire.clause_triplet_edge_slots)
            .map(|(entry, slots)| match (entry, slots) {
                (None, None) => Ok(None),
                (Some(entry), Some(triplet_edge_slots)) => {
                    Ok(Some(DesignEdgeRecipeSelectorClause {
                        entry,
                        triplet_edge_slots,
                    }))
                }
                _ => Err(
                    "clause_entries and clause_triplet_edge_slots must be present together"
                        .to_owned(),
                ),
            })
            .collect::<Result<_, _>>()?;
        let context = Self {
            selector: wire.selector,
            clauses,
            incidence_matching_edge_slots: wire.incidence_matching_edge_slots,
            boundary_count_matching_edge_slots: wire.boundary_count_matching_edge_slots,
        };
        if context.unique_incidence_edge_slot() != wire.unique_incidence_edge_slot {
            return Err(
                "unique_incidence_edge_slot must name the singleton incidence_matching_edge_slots"
                    .into(),
            );
        }
        Ok(context)
    }
}

#[cfg(test)]
impl From<DesignEdgeRecipeSelectorContext> for DesignEdgeRecipeSelectorContextWire {
    fn from(context: DesignEdgeRecipeSelectorContext) -> Self {
        let unique_incidence_edge_slot = context.unique_incidence_edge_slot();
        let (clause_entries, clause_triplet_edge_slots) = context
            .clauses
            .into_iter()
            .map(|clause| match clause {
                Some(clause) => (Some(clause.entry), Some(clause.triplet_edge_slots)),
                None => (None, None),
            })
            .unzip();
        Self {
            selector: context.selector,
            clause_entries,
            clause_triplet_edge_slots,
            incidence_matching_edge_slots: context.incidence_matching_edge_slots,
            unique_incidence_edge_slot,
            boundary_count_matching_edge_slots: context.boundary_count_matching_edge_slots,
        }
    }
}

/// Standard delimiter structure following an edge recipe's common prologue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignEdgeRecipeStructure {
    /// Number of ordered side clauses.
    pub(crate) root: i32,
    /// Ordered side clauses.
    pub(crate) sides: Vec<DesignTopologyRecipeSide>,
}

/// The alternate two-clause structure used by a fixed-path `SurfacePatch`
/// edge recipe.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignSurfacePatchRecipeStructureWire")]
pub(crate) struct DesignSurfacePatchRecipeStructure {
    /// Ordered clauses in the recipe program.
    pub(crate) clauses: [DesignSurfacePatchRecipeClause; 2],
}

#[cfg(test)]
thread_local! {
    pub(super) static SURFACE_PATCH_STRUCTURE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignSurfacePatchRecipeStructure {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SURFACE_PATCH_STRUCTURE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            clauses: self.clauses.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignSurfacePatchRecipeStructureRef<'a> {
    root: i32,
    clauses: &'a [DesignSurfacePatchRecipeClause; 2],
}

impl Serialize for DesignSurfacePatchRecipeStructure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignSurfacePatchRecipeStructureRef {
            root: 2,
            clauses: &self.clauses,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DesignSurfacePatchRecipeStructureWire {
    root: i32,
    clauses: Vec<DesignSurfacePatchRecipeClause>,
}

impl TryFrom<DesignSurfacePatchRecipeStructureWire> for DesignSurfacePatchRecipeStructure {
    type Error = String;

    fn try_from(wire: DesignSurfacePatchRecipeStructureWire) -> Result<Self, Self::Error> {
        if wire.root != 2 {
            return Err("surface patch recipe root must be 2".into());
        }
        let clauses = wire.clauses.try_into().map_err(|_| {
            "surface patch recipe clauses must contain exactly two clauses".to_owned()
        })?;
        Ok(Self { clauses })
    }
}

#[cfg(test)]
impl From<DesignSurfacePatchRecipeStructure> for DesignSurfacePatchRecipeStructureWire {
    fn from(value: DesignSurfacePatchRecipeStructure) -> Self {
        Self {
            root: 2,
            clauses: value.clauses.into(),
        }
    }
}

/// One clause in a `SurfacePatch` edge recipe.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignSurfacePatchRecipeClauseWire")]
pub(crate) struct DesignSurfacePatchRecipeClause {
    /// Six delimiter-bounded fields before the counted topology payload.
    pub(crate) fields: Vec<Vec<i32>>,
    /// Zero-based face-reference ordinals named by the first two fields.
    pub(crate) face_reference_ordinals: [u32; 2],
    /// Zero-based edge-reference ordinals named by the third and fifth fields.
    pub(crate) edge_reference_ordinals: [u32; 2],
    /// Ordered topology entries in the payload.
    pub(crate) entries: Vec<DesignTopologyRecipeEntry>,
}

#[cfg(test)]
thread_local! {
    pub(super) static SURFACE_PATCH_CLAUSE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignSurfacePatchRecipeClause {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SURFACE_PATCH_CLAUSE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            fields: self.fields.clone(),
            face_reference_ordinals: self.face_reference_ordinals,
            edge_reference_ordinals: self.edge_reference_ordinals,
            entries: self.entries.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignSurfacePatchRecipeClauseRef<'a> {
    fields: &'a [Vec<i32>],
    face_reference_ordinals: [u32; 2],
    edge_reference_ordinals: [u32; 2],
    payload_entry_count: usize,
    entries: &'a [DesignTopologyRecipeEntry],
}

impl Serialize for DesignSurfacePatchRecipeClause {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignSurfacePatchRecipeClauseRef {
            fields: &self.fields,
            face_reference_ordinals: self.face_reference_ordinals,
            edge_reference_ordinals: self.edge_reference_ordinals,
            payload_entry_count: self.entries.len(),
            entries: &self.entries,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DesignSurfacePatchRecipeClauseWire {
    /// Six delimiter-bounded fields before the counted topology payload.
    fields: Vec<Vec<i32>>,
    /// Zero-based face-reference ordinals named by the first two fields.
    face_reference_ordinals: [u32; 2],
    /// Zero-based edge-reference ordinals named by the third and fifth fields.
    edge_reference_ordinals: [u32; 2],
    /// Number of eight-word topology entries in the payload.
    payload_entry_count: usize,
    /// Ordered topology entries in the payload.
    entries: Vec<DesignTopologyRecipeEntry>,
}

impl TryFrom<DesignSurfacePatchRecipeClauseWire> for DesignSurfacePatchRecipeClause {
    type Error = &'static str;
    fn try_from(wire: DesignSurfacePatchRecipeClauseWire) -> Result<Self, Self::Error> {
        if wire.payload_entry_count != wire.entries.len() {
            return Err("payload_entry_count disagrees with entries");
        }
        Ok(Self {
            fields: wire.fields,
            face_reference_ordinals: wire.face_reference_ordinals,
            edge_reference_ordinals: wire.edge_reference_ordinals,
            entries: wire.entries,
        })
    }
}

#[cfg(test)]
impl From<DesignSurfacePatchRecipeClause> for DesignSurfacePatchRecipeClauseWire {
    fn from(value: DesignSurfacePatchRecipeClause) -> Self {
        Self {
            payload_entry_count: value.entries.len(),
            fields: value.fields,
            face_reference_ordinals: value.face_reference_ordinals,
            edge_reference_ordinals: value.edge_reference_ordinals,
            entries: value.entries,
        }
    }
}

/// One delimiter-bounded side clause in a standard edge recipe.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignTopologyRecipeSideWire")]
pub(crate) struct DesignTopologyRecipeSide {
    /// Second word of the side header.
    pub(crate) header_value: i32,
    /// Ordered scalar fields following the side header.
    pub(crate) scalars: Vec<i32>,
    /// Exact field program preceding the topology-entry count.
    pub(crate) payload_prefix: Vec<i32>,
    /// Ordered eight-word payload entries.
    pub(crate) entries: Vec<DesignTopologyRecipeEntry>,
}

#[cfg(test)]
thread_local! {
    pub(super) static TOPOLOGY_RECIPE_SIDE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignTopologyRecipeSide {
    fn clone(&self) -> Self {
        #[cfg(test)]
        TOPOLOGY_RECIPE_SIDE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            header_value: self.header_value,
            scalars: self.scalars.clone(),
            payload_prefix: self.payload_prefix.clone(),
            entries: self.entries.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignTopologyRecipeSideRef<'a> {
    field_count: usize,
    header_value: i32,
    scalars: &'a [i32],
    payload_prefix: &'a [i32],
    payload_entry_count: usize,
    entries: &'a [DesignTopologyRecipeEntry],
}

impl Serialize for DesignTopologyRecipeSide {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignTopologyRecipeSideRef {
            field_count: self.field_count(),
            header_value: self.header_value,
            scalars: &self.scalars,
            payload_prefix: &self.payload_prefix,
            payload_entry_count: self.entries.len(),
            entries: &self.entries,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeSideWire {
    /// Encoded number of fields after the header count: scalar fields plus the payload.
    field_count: usize,
    /// Second word of the side header.
    header_value: i32,
    /// Ordered scalar fields following the side header.
    scalars: Vec<i32>,
    /// Exact field program preceding the topology-entry count.
    payload_prefix: Vec<i32>,
    /// Encoded number of eight-word topology entries following the field program.
    payload_entry_count: usize,
    /// Ordered eight-word payload entries.
    entries: Vec<DesignTopologyRecipeEntry>,
}

impl TryFrom<DesignTopologyRecipeSideWire> for DesignTopologyRecipeSide {
    type Error = &'static str;
    fn try_from(wire: DesignTopologyRecipeSideWire) -> Result<Self, Self::Error> {
        if wire.field_count != wire.scalars.len() + 1 {
            return Err("field_count disagrees with scalars");
        }
        if wire.payload_entry_count != wire.entries.len() {
            return Err("payload_entry_count disagrees with entries");
        }
        Ok(Self {
            header_value: wire.header_value,
            scalars: wire.scalars,
            payload_prefix: wire.payload_prefix,
            entries: wire.entries,
        })
    }
}

#[cfg(test)]
impl From<DesignTopologyRecipeSide> for DesignTopologyRecipeSideWire {
    fn from(value: DesignTopologyRecipeSide) -> Self {
        Self {
            field_count: value.field_count(),
            payload_entry_count: value.entries.len(),
            header_value: value.header_value,
            scalars: value.scalars,
            payload_prefix: value.payload_prefix,
            entries: value.entries,
        }
    }
}

impl DesignTopologyRecipeSide {
    pub(crate) fn field_count(&self) -> usize {
        self.scalars.len() + 1
    }
}

/// One eight-word topology entry in an edge-recipe side clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignTopologyRecipeEntryWire",
    into = "DesignTopologyRecipeEntryWire"
)]
pub(crate) struct DesignTopologyRecipeEntry {
    /// Nonnegative clause-local selector, strictly increasing within one clause.
    pub(crate) selector: i32,
    /// Number of boundary edges on the referenced face loop.
    pub(crate) boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    pub(crate) topology_triplets: [DesignTopologyRecipeTriplet; 2],
}

#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeEntryWire {
    /// Nonnegative clause-local selector, strictly increasing within one clause.
    selector: i32,
    /// Number of boundary edges on the referenced face loop.
    boundary_edge_count: NonZeroU32,
    /// Two ordered topology triplets.
    topology_triplets: [DesignTopologyRecipeTriplet; 2],
    /// Zero-based boundary-edge ordinal named by both triplets when equal.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_common_incident_edge_ordinal"
    )]
    common_incident_edge_ordinal: Option<u32>,
}

impl DesignTopologyRecipeEntry {
    /// Return the incident edge ordinal shared by both triplets.
    pub(crate) fn common_incident_edge_ordinal(&self) -> Option<u32> {
        self.topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal)
            .filter(|ordinal| {
                self.topology_triplets[1]
                    .incident
                    .map(|incident| incident.ordinal)
                    == Some(*ordinal)
            })
    }
}

impl TryFrom<DesignTopologyRecipeEntryWire> for DesignTopologyRecipeEntry {
    type Error = String;
    fn try_from(wire: DesignTopologyRecipeEntryWire) -> Result<Self, Self::Error> {
        let value = Self {
            selector: wire.selector,
            boundary_edge_count: wire.boundary_edge_count,
            topology_triplets: wire.topology_triplets,
        };
        if value.common_incident_edge_ordinal() != wire.common_incident_edge_ordinal {
            return Err("common_incident_edge_ordinal disagrees with its source fields".into());
        }
        Ok(value)
    }
}

impl From<DesignTopologyRecipeEntry> for DesignTopologyRecipeEntryWire {
    fn from(value: DesignTopologyRecipeEntry) -> Self {
        let common_incident_edge_ordinal = value.common_incident_edge_ordinal();
        Self {
            selector: value.selector,
            boundary_edge_count: value.boundary_edge_count,
            topology_triplets: value.topology_triplets,
            common_incident_edge_ordinal,
        }
    }
}

/// One three-word invariant in an edge-recipe entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignTopologyRecipeTripletWire",
    into = "DesignTopologyRecipeTripletWire"
)]
pub(crate) struct DesignTopologyRecipeTriplet {
    /// Equal positive first and third words, not exceeding the containing
    /// entry's boundary-edge count.
    pub(crate) outer: NonZeroU32,
    /// Signed middle word retained from the source triplet.
    pub(crate) middle: i32,
    /// Incident edge and its side at the encoded vertex, when derived.
    pub(crate) incident: Option<DesignTopologyIncident>,
}

#[derive(Serialize, Deserialize)]
struct DesignTopologyRecipeTripletWire {
    /// Equal positive first and third words, not exceeding the containing
    /// entry's boundary-edge count.
    outer: NonZeroU32,
    /// Signed middle word retained from the source triplet.
    middle: i32,
    /// Zero-based loop vertex ordinal encoded by `outer`.
    vertex_ordinal: u32,
    /// Zero-based boundary-edge ordinal incident to `vertex_ordinal`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_incident_edge_ordinal"
    )]
    incident_edge_ordinal: Option<u32>,
    /// Whether the incident edge precedes or follows the vertex in loop order.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_incident_side"
    )]
    incident_side: Option<DesignTopologyIncidentSide>,
}

impl DesignTopologyRecipeTriplet {
    /// Return the zero-based vertex ordinal encoded by the outer word.
    pub(crate) fn vertex_ordinal(&self) -> u32 {
        self.outer.get() - 1
    }
}

impl TryFrom<DesignTopologyRecipeTripletWire> for DesignTopologyRecipeTriplet {
    type Error = String;
    fn try_from(wire: DesignTopologyRecipeTripletWire) -> Result<Self, Self::Error> {
        let incident = match (wire.incident_edge_ordinal, wire.incident_side) {
            (None, None) => None,
            (Some(ordinal), Some(side)) => Some(DesignTopologyIncident { ordinal, side }),
            _ => return Err("incident_edge_ordinal and incident_side must occur together".into()),
        };
        let value = Self {
            outer: wire.outer,
            middle: wire.middle,
            incident,
        };
        if value.vertex_ordinal() != wire.vertex_ordinal {
            return Err("vertex_ordinal disagrees with its source fields".into());
        }
        Ok(value)
    }
}

impl From<DesignTopologyRecipeTriplet> for DesignTopologyRecipeTripletWire {
    fn from(value: DesignTopologyRecipeTriplet) -> Self {
        let vertex_ordinal = value.vertex_ordinal();
        Self {
            outer: value.outer,
            middle: value.middle,
            incident_edge_ordinal: value.incident.map(|incident| incident.ordinal),
            incident_side: value.incident.map(|incident| incident.side),
            vertex_ordinal,
        }
    }
}

/// One incident boundary edge and its side at the selected vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignTopologyIncident {
    pub(crate) ordinal: u32,
    pub(crate) side: DesignTopologyIncidentSide,
}

/// Which loop edge incident to a recipe vertex is named by a topology triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignTopologyIncidentSide {
    /// Edge immediately preceding the vertex in cyclic loop order.
    Preceding,
    /// Edge immediately following the vertex in cyclic loop order.
    Following,
}

cadmpeg_core::named_optional_field!(
    deserialize_unique_incidence_edge_slot,
    i64,
    "unique_incidence_edge_slot"
);

cadmpeg_core::named_optional_field!(
    deserialize_common_incident_edge_ordinal,
    u32,
    "common_incident_edge_ordinal"
);

cadmpeg_core::named_optional_field!(
    deserialize_incident_edge_ordinal,
    u32,
    "incident_edge_ordinal"
);

cadmpeg_core::named_optional_field!(
    deserialize_incident_side,
    DesignTopologyIncidentSide,
    "incident_side"
);

#[cfg(test)]
mod tests;

mod identity_rewrite;
