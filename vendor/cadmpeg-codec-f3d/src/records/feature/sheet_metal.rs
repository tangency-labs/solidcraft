// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal features: base flange, edge flange and hem, with the bend and width forms they state.

use crate::records::serde_column::SliceColumn;
use cadmpeg_ir::scalar::PositiveReal;
use serde::{Deserialize, Serialize};
/// Fixed construction carried by a planar sheet-metal `BaseFlange` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignBaseFlangeOperation {
    /// Positive sheet thickness in centimetres.
    pub(crate) thickness: PositiveReal,
    /// Byte offset of `thickness`.
    pub(crate) thickness_offset: u64,
    /// Counted sketch-profile operand group.
    pub(crate) profile_group_record_index: u32,
    /// Sketch-profile record contained by the profile group.
    pub(crate) profile_record_index: u32,
    /// Indexed thickness-construction record.
    pub(crate) thickness_record_index: u32,
    /// Indexed operation-settings record.
    pub(crate) settings_record_index: u32,
}

/// Bend position used by sheet-metal edge operations.
///
/// The position places the bend region against the selected edge: `Outside` and
/// `Inside` put the bend beyond and within the source face boundary, `Adjacent`
/// starts it at the boundary, and `TangentToSide` makes it tangent to the side
/// reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignBendPosition {
    /// The bend lies outside the selected edge.
    Outside,
    /// The bend lies inside the selected edge.
    Inside,
    /// The bend starts at the selected edge.
    Adjacent,
    /// The bend is tangent to the side reference plane.
    TangentToSide,
    /// A serialized value whose bend-position meaning is not settled.
    Unknown(u32),
}

impl DesignBendPosition {
    /// Decode the serialized bend-position discriminator without discarding unknown values.
    #[must_use]
    pub(crate) fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Outside,
            2 => Self::Inside,
            3 => Self::Adjacent,
            4 => Self::TangentToSide,
            code => Self::Unknown(code),
        }
    }
}

/// Face pair an `EdgeFlange` height is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignSheetMetalHeightDatum {
    /// The height is measured from the inner faces of the sheet.
    InnerFaces,
    /// The height is measured from the outer faces of the sheet.
    OuterFaces,
    /// A serialized value whose height-datum meaning is not settled.
    Unknown(u32),
}

impl DesignSheetMetalHeightDatum {
    /// Decode the serialized height-datum discriminator without discarding unknown values.
    #[must_use]
    pub(crate) fn from_code(code: u32) -> Self {
        match code {
            1 => Self::InnerFaces,
            2 => Self::OuterFaces,
            code => Self::Unknown(code),
        }
    }
}

/// Extent of an `EdgeFlange` along its selected edge.
///
/// Ordinary forms derive the mode from the count of width-distance parameter
/// owners in the ordered reference table. Classed forms can carry a distinct
/// explicit mode when that count has per-edge meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignEdgeWidthMode {
    /// The flange spans the complete selected edge and adds no width owner.
    FullEdge,
    /// The flange is centred on the edge and adds one width owner.
    Symmetric,
    /// The flange is measured from each end and adds two width owners.
    TwoSides,
    /// The fixed section carries one symmetric-width owner per selected edge.
    ///
    /// Neutral projection can collapse these owners to one symmetric width only
    /// when their stored values agree. Distinct values remain source-native.
    SymmetricPerEdge,
    /// The fixed section carries one `EdgeWidth_1`/`EdgeWidth_2` pair per
    /// selected edge. The edge-local orientation is not part of the neutral law.
    TwoSidesPerEdge,
}

/// A selected flange edge and its width parameter owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignFlangeEdgeWidth<T> {
    pub(crate) edge: DesignEdgeFlangeEdge,
    pub(crate) owners: T,
}

/// Flange extent, with width owners attached to the edges they describe.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignEdgeFlangeShape {
    FullEdge {
        edges: Vec<DesignEdgeFlangeEdge>,
        height: DesignEdgeFlangeHeightExtent,
    },
    Symmetric {
        edges: Vec<DesignEdgeFlangeEdge>,
        owner: u32,
    },
    TwoSides {
        edges: Vec<DesignEdgeFlangeEdge>,
        owners: [u32; 2],
    },
    SymmetricPerEdge(Vec<DesignFlangeEdgeWidth<u32>>),
    TwoSidesPerEdge {
        edges: Vec<DesignFlangeEdgeWidth<[u32; 2]>>,
        source: DesignEdgeFlangeWidthParameterSource,
    },
}

struct FlangeEdgeColumns<'a> {
    shared: &'a [DesignEdgeFlangeEdge],
    symmetric: &'a [DesignFlangeEdgeWidth<u32>],
    two_sided: &'a [DesignFlangeEdgeWidth<[u32; 2]>],
}

struct FlangeOwnerColumns<'a> {
    shared: &'a [u32],
    symmetric: &'a [DesignFlangeEdgeWidth<u32>],
    two_sided: &'a [DesignFlangeEdgeWidth<[u32; 2]>],
}

impl DesignEdgeFlangeShape {
    pub(crate) fn edges(&self) -> impl Iterator<Item = &DesignEdgeFlangeEdge> {
        let FlangeEdgeColumns {
            shared,
            symmetric,
            two_sided,
        } = match self {
            Self::FullEdge { edges, .. }
            | Self::Symmetric { edges, .. }
            | Self::TwoSides { edges, .. } => FlangeEdgeColumns {
                shared: edges,
                symmetric: &[],
                two_sided: &[],
            },
            Self::SymmetricPerEdge(edges) => FlangeEdgeColumns {
                shared: &[],
                symmetric: edges,
                two_sided: &[],
            },
            Self::TwoSidesPerEdge { edges, .. } => FlangeEdgeColumns {
                shared: &[],
                symmetric: &[],
                two_sided: edges,
            },
        };
        shared
            .iter()
            .chain(symmetric.iter().map(|row| &row.edge))
            .chain(two_sided.iter().map(|row| &row.edge))
    }

    pub(crate) fn mode(&self) -> DesignEdgeWidthMode {
        match self {
            Self::FullEdge { .. } => DesignEdgeWidthMode::FullEdge,
            Self::Symmetric { .. } => DesignEdgeWidthMode::Symmetric,
            Self::TwoSides { .. } => DesignEdgeWidthMode::TwoSides,
            Self::SymmetricPerEdge(_) => DesignEdgeWidthMode::SymmetricPerEdge,
            Self::TwoSidesPerEdge { .. } => DesignEdgeWidthMode::TwoSidesPerEdge,
        }
    }

    pub(crate) fn height(&self) -> DesignEdgeFlangeHeightExtent {
        match self {
            Self::FullEdge { height, .. } => *height,
            _ => DesignEdgeFlangeHeightExtent::Distance,
        }
    }

    pub(crate) fn source(&self) -> DesignEdgeFlangeWidthParameterSource {
        match self {
            Self::TwoSidesPerEdge { source, .. } => *source,
            _ => DesignEdgeFlangeWidthParameterSource::EdgeWidth,
        }
    }

    pub(crate) fn owner_indices(&self) -> impl Iterator<Item = &u32> {
        let FlangeOwnerColumns {
            shared,
            symmetric,
            two_sided,
        } = match self {
            Self::FullEdge { .. } => FlangeOwnerColumns {
                shared: &[],
                symmetric: &[],
                two_sided: &[],
            },
            Self::Symmetric { owner, .. } => FlangeOwnerColumns {
                shared: std::slice::from_ref(owner),
                symmetric: &[],
                two_sided: &[],
            },
            Self::TwoSides { owners, .. } => FlangeOwnerColumns {
                shared: owners,
                symmetric: &[],
                two_sided: &[],
            },
            Self::SymmetricPerEdge(edges) => FlangeOwnerColumns {
                shared: &[],
                symmetric: edges,
                two_sided: &[],
            },
            Self::TwoSidesPerEdge { edges, .. } => FlangeOwnerColumns {
                shared: &[],
                symmetric: &[],
                two_sided: edges,
            },
        };
        shared
            .iter()
            .chain(symmetric.iter().map(|row| &row.owners))
            .chain(two_sided.iter().flat_map(|row| &row.owners))
    }

    pub(crate) fn from_wire(
        edges: Vec<DesignEdgeFlangeEdge>,
        width_mode: Option<DesignEdgeWidthMode>,
        owners: Vec<u32>,
        owners_by_edge: Vec<[u32; 2]>,
        source: DesignEdgeFlangeWidthParameterSource,
        height: DesignEdgeFlangeHeightExtent,
    ) -> Result<Self, String> {
        let mode = width_mode.unwrap_or(match owners.len() {
            0 => DesignEdgeWidthMode::FullEdge,
            1 => DesignEdgeWidthMode::Symmetric,
            _ => DesignEdgeWidthMode::TwoSides,
        });
        if source == DesignEdgeFlangeWidthParameterSource::EdgeOffset
            && mode != DesignEdgeWidthMode::TwoSidesPerEdge
        {
            return Err(
                "width_parameter_source edge_offset requires width_mode two_sides_per_edge".into(),
            );
        }
        if !matches!(height, DesignEdgeFlangeHeightExtent::Distance)
            && mode != DesignEdgeWidthMode::FullEdge
        {
            return Err("height_extent to_object requires width_mode full_edge".into());
        }
        match mode {
            DesignEdgeWidthMode::FullEdge if owners.is_empty() && owners_by_edge.is_empty() => Ok(Self::FullEdge { edges, height }),
            DesignEdgeWidthMode::Symmetric if owners.len() == 1 && owners_by_edge.is_empty() => Ok(Self::Symmetric { edges, owner: owners[0] }),
            DesignEdgeWidthMode::TwoSides if owners.len() == 2 && owners_by_edge.is_empty() => Ok(Self::TwoSides { edges, owners: [owners[0], owners[1]] }),
            DesignEdgeWidthMode::SymmetricPerEdge if !owners.is_empty() && owners.len() == edges.len() && owners_by_edge.is_empty() => {
                Ok(Self::SymmetricPerEdge(edges.into_iter().zip(owners).map(|(edge, owners)| DesignFlangeEdgeWidth { edge, owners }).collect()))
            }
            DesignEdgeWidthMode::TwoSidesPerEdge if !owners_by_edge.is_empty() && owners_by_edge.len() == edges.len()
                && owners.iter().eq(owners_by_edge.iter().flatten()) => {
                Ok(Self::TwoSidesPerEdge { edges: edges.into_iter().zip(owners_by_edge).map(|(edge, owners)| DesignFlangeEdgeWidth { edge, owners }).collect(), source })
            }
            _ => Err("width_mode, width_distance_owner_record_indices, and width_distance_owner_record_indices_by_edge must match the selected edges".into()),
        }
    }
}

/// Parameter source used by a typed `EdgeFlange` width law.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignEdgeFlangeWidthParameterSource {
    /// Width parameters use the ordinary positive `EdgeWidth` source kinds.
    #[default]
    EdgeWidth,
    /// Legacy edge-end parameters use signed `EdgeOffset` source kinds.
    EdgeOffset,
}

/// Height extent law carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum DesignEdgeFlangeHeightExtent {
    /// The flange height is a direct distance from the selected sheet datum.
    #[default]
    Distance,
    /// The flange height is measured from a selected construction entity.
    ToObject {
        /// Role-`0x21` construction-operand group containing the target.
        target_group_record_index: u32,
        /// Entity-selection operand carried by the target group.
        target_operand_record_index: u32,
        /// Parameter owner carrying the signed target offset.
        offset_owner_record_index: u32,
        /// Two marked references inserted in the fixed operation section.
        reference_record_indices: [u32; 2],
    },
}

/// A group record index with a representable recipe index three records later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct DesignRecipeGroupIndex(u32);

impl DesignRecipeGroupIndex {
    pub(crate) fn get(self) -> u32 {
        self.0
    }
    fn operand(self) -> u32 {
        self.0 + 3
    }
}

impl TryFrom<u32> for DesignRecipeGroupIndex {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        value
            .checked_add(3)
            .ok_or("group_record_index + 3 overflows")?;
        Ok(Self(value))
    }
}

impl From<DesignRecipeGroupIndex> for u32 {
    fn from(value: DesignRecipeGroupIndex) -> Self {
        value.get()
    }
}

/// One selected flange edge and its aggregate operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Field names are the native record serialized keys.
pub(crate) struct DesignEdgeFlangeEdge {
    pub(crate) wrapper: u32,
    pub(crate) group_record_index: DesignRecipeGroupIndex,
    pub(crate) aggregate_operand_record_index: u32,
}

impl DesignEdgeFlangeEdge {
    pub(crate) fn operand_record_index(&self) -> u32 {
        self.group_record_index.operand()
    }

    pub(crate) fn from_columns(
        wrappers: Vec<u32>,
        groups: Vec<u32>,
        operands: &[u32],
        aggregate_operands: Vec<u32>,
    ) -> Result<Vec<Self>, String> {
        if groups.len() != wrappers.len()
            || operands.len() != wrappers.len()
            || aggregate_operands.len() != wrappers.len()
        {
            return Err("edge_wrapper_record_indices, edge_group_record_indices, edge_operand_record_indices, and aggregate_operand_record_indices must have equal lengths".into());
        }
        if groups
            .iter()
            .zip(operands)
            .any(|(group, operand)| Some(*operand) != group.checked_add(3))
        {
            return Err(
                "edge_operand_record_indices must equal edge_group_record_indices + 3".into(),
            );
        }
        wrappers
            .into_iter()
            .zip(groups)
            .zip(aggregate_operands)
            .map(
                |((wrapper_record_index, group_record_index), aggregate_operand_record_index)| {
                    Ok(Self {
                        wrapper: wrapper_record_index,
                        group_record_index: group_record_index.try_into()?,
                        aggregate_operand_record_index,
                    })
                },
            )
            .collect()
    }
}

/// Fixed construction carried by a sheet-metal `EdgeFlange` scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignEdgeFlangeOperationSerde")]
pub(crate) struct DesignEdgeFlangeOperation {
    /// Selected flange edges and their aggregate operand group.
    pub(crate) selection: DesignEdgeFlangeSelection,
    /// Height parameter-owner record.
    pub(crate) height_owner_record_index: u32,
    /// Angle parameter-owner record.
    pub(crate) angle_owner_record_index: u32,
    /// Scope references retained by a classed layout after typed roles and
    /// width owners have been claimed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) auxiliary_reference_record_indices: Vec<u32>,
    /// Indexed operation-settings record.
    pub(crate) settings_record_index: u32,
    /// Retain the scope control for native replay.
    pub(crate) bend_radius: PositiveReal,
    /// Byte offset of `bend_radius`.
    pub(crate) bend_radius_offset: u64,
    /// Face pair the flange height is measured from.
    pub(crate) height_datum: DesignSheetMetalHeightDatum,
    /// Bend position relative to the selected edge.
    pub(crate) bend_position: DesignBendPosition,
}

#[cfg(test)]
thread_local! {
    static EDGE_FLANGE_OPERATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignEdgeFlangeOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        EDGE_FLANGE_OPERATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selection: self.selection.clone(),
            height_owner_record_index: self.height_owner_record_index,
            angle_owner_record_index: self.angle_owner_record_index,
            auxiliary_reference_record_indices: self.auxiliary_reference_record_indices.clone(),
            settings_record_index: self.settings_record_index,
            bend_radius: self.bend_radius,
            bend_radius_offset: self.bend_radius_offset,
            height_datum: self.height_datum,
            bend_position: self.bend_position,
        }
    }
}

struct FlangeEdgeColumn<'a, T> {
    shape: &'a DesignEdgeFlangeShape,
    value: fn(&DesignEdgeFlangeEdge) -> T,
}

impl<T: Serialize> Serialize for FlangeEdgeColumn<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.shape.edges().map(self.value))
    }
}

struct FlangeOwnerColumn<'a>(&'a DesignEdgeFlangeShape);

impl Serialize for FlangeOwnerColumn<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.owner_indices())
    }
}

impl Serialize for DesignEdgeFlangeOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            edge_wrapper_record_indices: FlangeEdgeColumn<'a, u32>,
            edge_group_record_indices: FlangeEdgeColumn<'a, u32>,
            edge_operand_record_indices: FlangeEdgeColumn<'a, u32>,
            aggregate_group_record_index: u32,
            aggregate_operand_record_indices: FlangeEdgeColumn<'a, u32>,
            height_owner_record_index: u32,
            height_extent: DesignEdgeFlangeHeightExtent,
            angle_owner_record_index: u32,
            width_mode: Option<DesignEdgeWidthMode>,
            width_distance_owner_record_indices: FlangeOwnerColumn<'a>,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            width_distance_owner_record_indices_by_edge:
                SliceColumn<'a, DesignFlangeEdgeWidth<[u32; 2]>, [u32; 2]>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            auxiliary_reference_record_indices: &'a Vec<u32>,
            width_parameter_source: DesignEdgeFlangeWidthParameterSource,
            settings_record_index: u32,
            bend_radius: f64,
            bend_radius_offset: u64,
            reference_side_code: u32,
            height_datum: DesignSheetMetalHeightDatum,
            bend_position: DesignBendPosition,
        }
        let shape = self.selection.shape();
        let per_edge = match shape {
            DesignEdgeFlangeShape::TwoSidesPerEdge { edges, .. } => edges.as_slice(),
            _ => &[],
        };
        BorrowedWire {
            edge_wrapper_record_indices: FlangeEdgeColumn {
                shape,
                value: |edge| edge.wrapper,
            },
            edge_group_record_indices: FlangeEdgeColumn {
                shape,
                value: |edge| edge.group_record_index.get(),
            },
            edge_operand_record_indices: FlangeEdgeColumn {
                shape,
                value: DesignEdgeFlangeEdge::operand_record_index,
            },
            aggregate_group_record_index: self.selection.aggregate_group_record_index(),
            aggregate_operand_record_indices: FlangeEdgeColumn {
                shape,
                value: |edge| edge.aggregate_operand_record_index,
            },
            height_owner_record_index: self.height_owner_record_index,
            height_extent: shape.height(),
            angle_owner_record_index: self.angle_owner_record_index,
            width_mode: Some(shape.mode()),
            width_distance_owner_record_indices: FlangeOwnerColumn(shape),
            width_distance_owner_record_indices_by_edge: SliceColumn::new(per_edge, |row| {
                row.owners
            }),
            auxiliary_reference_record_indices: &self.auxiliary_reference_record_indices,
            width_parameter_source: shape.source(),
            settings_record_index: self.settings_record_index,
            bend_radius: self.bend_radius.get(),
            bend_radius_offset: self.bend_radius_offset,
            reference_side_code: 4,
            height_datum: self.height_datum,
            bend_position: self.bend_position,
        }
        .serialize(serializer)
    }
}

/// Flange edge shape paired with its aggregate operand group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignEdgeFlangeSelection {
    shape: DesignEdgeFlangeShape,
    aggregate_group_record_index: u32,
}

impl DesignEdgeFlangeSelection {
    pub(crate) fn try_new(
        shape: DesignEdgeFlangeShape,
        aggregate_group_record_index: u32,
    ) -> Result<Self, String> {
        let mismatched_aggregate = {
            let mut edges = shape.edges();
            let first = edges.next();
            edges.next().is_none()
                && first.is_some_and(|edge| {
                    Some(edge.aggregate_operand_record_index)
                        != aggregate_group_record_index.checked_add(3)
                })
        };
        if mismatched_aggregate {
            return Err("single-edge aggregate_operand_record_indices must equal aggregate_group_record_index + 3".into());
        }
        Ok(Self {
            shape,
            aggregate_group_record_index,
        })
    }
    pub(crate) fn shape(&self) -> &DesignEdgeFlangeShape {
        &self.shape
    }
    pub(crate) fn aggregate_group_record_index(&self) -> u32 {
        self.aggregate_group_record_index
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DesignEdgeFlangeOperationSerde {
    pub(super) edge_wrapper_record_indices: Vec<u32>,
    pub(super) edge_group_record_indices: Vec<u32>,
    pub(super) edge_operand_record_indices: Vec<u32>,
    pub(super) aggregate_group_record_index: u32,
    pub(super) aggregate_operand_record_indices: Vec<u32>,
    pub(super) height_owner_record_index: u32,
    #[serde(default)]
    pub(super) height_extent: DesignEdgeFlangeHeightExtent,
    pub(super) angle_owner_record_index: u32,
    #[serde(deserialize_with = "cadmpeg_core::absent_key::nullable")]
    pub(super) width_mode: Option<DesignEdgeWidthMode>,
    pub(super) width_distance_owner_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) width_distance_owner_record_indices_by_edge: Vec<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) auxiliary_reference_record_indices: Vec<u32>,
    #[serde(default)]
    pub(super) width_parameter_source: DesignEdgeFlangeWidthParameterSource,
    pub(super) settings_record_index: u32,
    pub(super) bend_radius: f64,
    pub(super) bend_radius_offset: u64,
    pub(super) reference_side_code: u32,
    pub(super) height_datum: DesignSheetMetalHeightDatum,
    pub(super) bend_position: DesignBendPosition,
}

impl TryFrom<DesignEdgeFlangeOperationSerde> for DesignEdgeFlangeOperation {
    type Error = String;

    fn try_from(wire: DesignEdgeFlangeOperationSerde) -> Result<Self, Self::Error> {
        if wire.reference_side_code != 4 {
            return Err("reference_side_code must be 4".into());
        }
        let edges = DesignEdgeFlangeEdge::from_columns(
            wire.edge_wrapper_record_indices,
            wire.edge_group_record_indices,
            &wire.edge_operand_record_indices,
            wire.aggregate_operand_record_indices,
        )?;
        Ok(Self {
            selection: DesignEdgeFlangeSelection::try_new(
                DesignEdgeFlangeShape::from_wire(
                    edges,
                    wire.width_mode,
                    wire.width_distance_owner_record_indices,
                    wire.width_distance_owner_record_indices_by_edge,
                    wire.width_parameter_source,
                    wire.height_extent,
                )?,
                wire.aggregate_group_record_index,
            )?,
            height_owner_record_index: wire.height_owner_record_index,
            angle_owner_record_index: wire.angle_owner_record_index,
            auxiliary_reference_record_indices: wire.auxiliary_reference_record_indices,
            settings_record_index: wire.settings_record_index,
            bend_radius: PositiveReal::new(wire.bend_radius)
                .ok_or("bend_radius must be positive and finite")?,
            bend_radius_offset: wire.bend_radius_offset,
            height_datum: wire.height_datum,
            bend_position: wire.bend_position,
        })
    }
}

#[cfg(test)]
impl From<DesignEdgeFlangeOperation> for DesignEdgeFlangeOperationSerde {
    fn from(operation: DesignEdgeFlangeOperation) -> Self {
        let width_mode = Some(operation.selection.shape().mode());
        let width_distance_owner_record_indices = operation
            .selection
            .shape()
            .owner_indices()
            .copied()
            .collect();
        let width_distance_owner_record_indices_by_edge = match operation.selection.shape() {
            DesignEdgeFlangeShape::TwoSidesPerEdge { edges, .. } => {
                edges.iter().map(|row| row.owners).collect()
            }
            _ => Vec::new(),
        };
        Self {
            edge_wrapper_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.wrapper)
                .collect(),
            edge_group_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.group_record_index.get())
                .collect(),
            edge_operand_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(DesignEdgeFlangeEdge::operand_record_index)
                .collect(),
            aggregate_group_record_index: operation.selection.aggregate_group_record_index(),
            aggregate_operand_record_indices: operation
                .selection
                .shape()
                .edges()
                .map(|edge| edge.aggregate_operand_record_index)
                .collect(),
            height_owner_record_index: operation.height_owner_record_index,
            height_extent: operation.selection.shape().height(),
            angle_owner_record_index: operation.angle_owner_record_index,
            width_mode,
            width_distance_owner_record_indices,
            width_distance_owner_record_indices_by_edge,
            auxiliary_reference_record_indices: operation.auxiliary_reference_record_indices,
            width_parameter_source: operation.selection.shape().source(),
            settings_record_index: operation.settings_record_index,
            bend_radius: operation.bend_radius.get(),
            bend_radius_offset: operation.bend_radius_offset,
            reference_side_code: 4,
            height_datum: operation.height_datum,
            bend_position: operation.bend_position,
        }
    }
}

/// Parameter-owner layout carried by a sheet-metal `Hem` scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignHemParameterOwners {
    /// Flat and open forms own a gap and a length.
    GapLength {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
    },
    /// Rolled form owns a radius and an angle.
    RadiusAngle {
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
        /// Angle parameter-owner record.
        angle_owner_record_index: u32,
    },
    /// Teardrop form owns a gap, a length, and a radius.
    GapLengthRadius {
        /// Gap parameter-owner record.
        gap_owner_record_index: u32,
        /// Length parameter-owner record.
        length_owner_record_index: u32,
        /// Radius parameter-owner record.
        radius_owner_record_index: u32,
    },
}

/// Fixed operation section and parameter-owner layout carried by a sheet-metal
/// `Hem` scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignHemOperationWire", into = "DesignHemOperationWire")]
pub(crate) struct DesignHemOperation {
    /// Placement of the bend against the selected edge.
    pub(crate) bend_position: cadmpeg_ir::features::SheetMetalHemBendPosition,
    /// Native Flip flag; false is forward, true is reverse.
    pub(crate) flipped: bool,
    /// Selection-wrapper record for the hem edge.
    pub(crate) edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    pub(crate) edge_group_record_index: DesignRecipeGroupIndex,
    /// Role-`0x43` aggregate operand-group record.
    pub(crate) aggregate_group_record_index: DesignRecipeGroupIndex,
    /// Parameter-owner layout selected by the owned source kinds.
    pub(crate) parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    pub(crate) settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    pub(crate) bend_radius: PositiveReal,
    /// Byte offset of `bend_radius`.
    pub(crate) bend_radius_offset: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignHemOperationWire {
    bend_position: cadmpeg_ir::features::SheetMetalHemBendPosition,
    flipped: bool,
    /// Selection-wrapper record for the hem edge.
    edge_wrapper_record_index: u32,
    /// Role-`0x08` operand-group record.
    edge_group_record_index: u32,
    /// Recipe-backed role-`0x08` operand record.
    edge_operand_record_index: u32,
    /// Role-`0x43` aggregate operand-group record.
    aggregate_group_record_index: u32,
    /// Recipe-backed role-`0x43` operand record.
    aggregate_operand_record_index: u32,
    /// Parameter-owner layout selected by the owned source kinds.
    parameter_owners: DesignHemParameterOwners,
    /// Indexed operation-settings record.
    settings_record_index: u32,
    /// Positive rule-derived inside bend radius in centimetres.
    bend_radius: f64,
    /// Byte offset of `bend_radius`.
    bend_radius_offset: u64,
}

impl DesignHemOperation {
    pub(crate) fn edge_operand_record_index(&self) -> u32 {
        self.edge_group_record_index.operand()
    }
    pub(crate) fn aggregate_operand_record_index(&self) -> u32 {
        self.aggregate_group_record_index.operand()
    }
}

impl TryFrom<DesignHemOperationWire> for DesignHemOperation {
    type Error = String;

    fn try_from(wire: DesignHemOperationWire) -> Result<Self, Self::Error> {
        if wire.bend_position != cadmpeg_ir::features::SheetMetalHemBendPosition::Adjacent {
            return Err("unsupported native Hem bend position".into());
        }
        if Some(wire.edge_operand_record_index) != wire.edge_group_record_index.checked_add(3) {
            return Err("edge_operand_record_index must equal edge_group_record_index + 3".into());
        }
        if Some(wire.aggregate_operand_record_index)
            != wire.aggregate_group_record_index.checked_add(3)
        {
            return Err(
                "aggregate_operand_record_index must equal aggregate_group_record_index + 3".into(),
            );
        }
        Ok(Self {
            bend_position: wire.bend_position,
            flipped: wire.flipped,
            edge_wrapper_record_index: wire.edge_wrapper_record_index,
            edge_group_record_index: wire.edge_group_record_index.try_into()?,
            aggregate_group_record_index: wire.aggregate_group_record_index.try_into()?,
            parameter_owners: wire.parameter_owners,
            settings_record_index: wire.settings_record_index,
            bend_radius: PositiveReal::new(wire.bend_radius)
                .ok_or("bend_radius must be positive and finite")?,
            bend_radius_offset: wire.bend_radius_offset,
        })
    }
}

impl From<DesignHemOperation> for DesignHemOperationWire {
    fn from(record: DesignHemOperation) -> Self {
        Self {
            bend_position: record.bend_position,
            flipped: record.flipped,
            edge_wrapper_record_index: record.edge_wrapper_record_index,
            edge_group_record_index: record.edge_group_record_index.get(),
            edge_operand_record_index: record.edge_operand_record_index(),
            aggregate_group_record_index: record.aggregate_group_record_index.get(),
            aggregate_operand_record_index: record.aggregate_operand_record_index(),
            parameter_owners: record.parameter_owners,
            settings_record_index: record.settings_record_index,
            bend_radius: record.bend_radius.get(),
            bend_radius_offset: record.bend_radius_offset,
        }
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
