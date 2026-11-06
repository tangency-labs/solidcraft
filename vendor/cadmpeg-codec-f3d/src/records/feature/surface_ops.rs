// SPDX-License-Identifier: Apache-2.0
//! Surface operations: stitch, extend, offset, trim, rule and patch.

use crate::records::{
    identity::NonEmptyVec, mesh::DesignRelaxedGuidText, references::DesignClassTag,
};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use serde::{Deserialize, Serialize};
/// Fixed operation records named by a `SurfaceStitch` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSurfaceStitchOperation {
    /// Positive maximum stitched-boundary gap in centimetres.
    pub(crate) gap_tolerance: PositiveReal,
    /// Byte offset of `gap_tolerance`.
    pub(crate) gap_tolerance_offset: u64,
    /// Indexed tolerance-record identity.
    pub(crate) tolerance_record_index: u32,
    /// Indexed operation-settings record identity.
    pub(crate) settings_record_index: u32,
}

/// Geometric continuation law encoded by a `SurfaceExtend` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignSurfaceExtendMethod {
    /// Continue the source surface parameterization.
    Natural,
    /// Create faces tangent to the source faces.
    Tangent,
    /// Create faces perpendicular to the source faces.
    Perpendicular,
}

/// Fixed construction records named by a `SurfaceExtend` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSurfaceExtendOperation {
    /// Extension distance in source centimetres.
    pub(crate) distance: FiniteReal,
    /// Byte offset of `distance`.
    pub(crate) distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub(crate) distance_record_index: u32,
    /// Geometric continuation law.
    pub(crate) method: DesignSurfaceExtendMethod,
    /// Byte offset of the method enum.
    pub(crate) method_offset: u64,
    /// Indexed boundary-carrier record.
    pub(crate) boundary_record_index: u32,
    /// Additional indexed reference carried by the boundary tail.
    pub(crate) boundary_reference_record_index: u32,
    /// Byte offset of `boundary_reference_record_index`'s marked reference.
    pub(crate) boundary_reference_offset: u64,
    /// Ordered edge-recipe records contained by the boundary carrier.
    pub(crate) edge_record_indices: Vec<u32>,
    /// Modelling tolerance in source centimetres.
    pub(crate) tolerance: cadmpeg_ir::scalar::PositiveReal,
    /// Byte offset of `tolerance`.
    pub(crate) tolerance_offset: u64,
}

/// Source selection form named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum DesignSurfaceOffsetSupport {
    /// A boundary carrier followed by edge recipes.
    BoundaryCarrier {
        /// Indexed boundary-carrier record.
        boundary_record_index: u32,
        /// Additional indexed reference carried by the boundary tail.
        boundary_reference_record_index: u32,
        /// Byte offset of `boundary_reference_record_index`'s marked reference.
        boundary_reference_offset: u64,
        /// Ordered edge-recipe records contained by the boundary carrier.
        edge_record_indices: Vec<u32>,
        /// Modelling tolerance in source centimetres.
        tolerance: cadmpeg_ir::scalar::PositiveReal,
        /// Byte offset of `tolerance`.
        tolerance_offset: u64,
    },
    /// Counted role-0x41 groups containing bounded-face recipes.
    FaceGroups {
        /// Ordered construction-group records named by the scope.
        group_record_indices: Vec<u32>,
    },
}

/// Fixed construction records named by a `SurfaceOffset` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSurfaceOffsetOperation {
    /// Signed offset distance in source centimetres.
    pub(crate) distance: FiniteReal,
    /// Byte offset of `distance`.
    pub(crate) distance_offset: u64,
    /// Indexed scalar record carrying `distance`.
    pub(crate) distance_record_index: u32,
    /// Exact source selection form.
    pub(crate) support: DesignSurfaceOffsetSupport,
}

/// One indexed record in the auxiliary chain preceding a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignSurfaceTrimChainRecord {
    /// Indexed record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Source per-file dynamic class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Bytes from the primary header to the following indexed header.
    pub(crate) frame_length: u64,
}

/// One source `BRep` cell entry in a `SurfaceTrim` cell table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignSurfaceTrimCellEntry {
    /// Indexed cell-record identity.
    pub(crate) record_index: u32,
    /// Byte offset of the marked cell-record reference.
    pub(crate) record_reference_offset: u64,
    /// One-based partition ordinal of a cell selected for removal.
    pub(crate) ordinal: u64,
    /// Byte offset of the serialized entry ordinal.
    pub(crate) ordinal_offset: u64,
}

/// Exact auxiliary carrier of a `SurfaceTrim` operation.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignSurfaceTrimOperationWire")]
pub(crate) struct DesignSurfaceTrimOperation {
    /// Globally unique deterministic identifier for this native carrier.
    pub(crate) id: String,
    /// Owning `SurfaceTrim` parameter-scope record index.
    pub(crate) scope_record_index: u32,
    /// Indexed entity-selection record that starts the trimming tool chain.
    pub(crate) selection_record_index: u32,
    /// Byte offset of the entity-selection record.
    selection_byte_offset: u64,
    /// Indexed record immediately following the entity-selection frame.
    pub(crate) selection_next_record_index: u32,
    /// Byte offset of the record immediately following the entity-selection frame.
    selection_next_byte_offset: u64,
    /// Two indexed records between the entity selection and the cell table.
    pub(crate) chain_records: [DesignSurfaceTrimChainRecord; 2],
    /// Indexed record carrying the counted BRep-cell table.
    pub(crate) cell_table_record_index: u32,
    /// Byte offset of the cell-table primary header.
    cell_table_byte_offset: u64,
    /// Dynamic class tag of the cell-table primary frame.
    pub(crate) cell_table_class_tag: DesignClassTag,
    /// Bytes from the cell-table primary header to its paired header.
    cell_table_frame_length: u64,
    /// Dynamic class tag of the cell-table paired frame.
    pub(crate) cell_table_paired_class_tag: DesignClassTag,
    /// Byte offset of the cell-table paired header.
    cell_table_paired_byte_offset: u64,
    /// Byte offset of the cell-table count.
    cell_count_offset: u64,
    /// Ordered cell-table entries.
    cell_entries: NonEmptyVec<DesignSurfaceTrimCellEntry>,
    /// Total number of cells in the operation's partition.
    pub(crate) trailing_value: u32,
    /// Byte offset of `trailing_value`.
    pub(crate) trailing_value_offset: u64,
    /// Byte offset of the zero value after `trailing_value`.
    pub(crate) trailing_zero_offset: u64,
}

#[cfg(test)]
thread_local! {
    static SURFACE_TRIM_OPERATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignSurfaceTrimOperation {
    fn clone(&self) -> Self {
        #[cfg(test)]
        SURFACE_TRIM_OPERATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            selection_record_index: self.selection_record_index,
            selection_byte_offset: self.selection_byte_offset,
            selection_next_record_index: self.selection_next_record_index,
            selection_next_byte_offset: self.selection_next_byte_offset,
            chain_records: self.chain_records.clone(),
            cell_table_record_index: self.cell_table_record_index,
            cell_table_byte_offset: self.cell_table_byte_offset,
            cell_table_class_tag: self.cell_table_class_tag.clone(),
            cell_table_frame_length: self.cell_table_frame_length,
            cell_table_paired_class_tag: self.cell_table_paired_class_tag.clone(),
            cell_table_paired_byte_offset: self.cell_table_paired_byte_offset,
            cell_count_offset: self.cell_count_offset,
            cell_entries: self.cell_entries.clone(),
            trailing_value: self.trailing_value,
            trailing_value_offset: self.trailing_value_offset,
            trailing_zero_offset: self.trailing_zero_offset,
        }
    }
}

#[derive(Serialize)]
struct DesignSurfaceTrimOperationRef<'a> {
    id: &'a str,
    scope_record_index: u32,
    selection_record_index: u32,
    selection_byte_offset: u64,
    selection_next_record_index: u32,
    selection_next_byte_offset: u64,
    chain_records: &'a [DesignSurfaceTrimChainRecord; 2],
    cell_table_record_index: u32,
    cell_table_byte_offset: u64,
    cell_table_class_tag: &'a str,
    cell_table_frame_length: u64,
    cell_table_paired_class_tag: &'a str,
    cell_table_paired_byte_offset: u64,
    cell_count_offset: u64,
    cell_entries: &'a [DesignSurfaceTrimCellEntry],
    trailing_value: u32,
    trailing_value_offset: u64,
    trailing_zero_offset: u64,
}

impl Serialize for DesignSurfaceTrimOperation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignSurfaceTrimOperationRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            selection_record_index: self.selection_record_index,
            selection_byte_offset: self.selection_byte_offset,
            selection_next_record_index: self.selection_next_record_index,
            selection_next_byte_offset: self.selection_next_byte_offset,
            chain_records: &self.chain_records,
            cell_table_record_index: self.cell_table_record_index,
            cell_table_byte_offset: self.cell_table_byte_offset,
            cell_table_class_tag: self.cell_table_class_tag.as_str(),
            cell_table_frame_length: self.cell_table_frame_length,
            cell_table_paired_class_tag: self.cell_table_paired_class_tag.as_str(),
            cell_table_paired_byte_offset: self.cell_table_paired_byte_offset,
            cell_count_offset: self.cell_count_offset,
            cell_entries: self.cell_entries.as_slice(),
            trailing_value: self.trailing_value,
            trailing_value_offset: self.trailing_value_offset,
            trailing_zero_offset: self.trailing_zero_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct DesignSurfaceTrimOperationWire {
    /// Globally unique deterministic identifier for this native carrier.
    pub(crate) id: String,
    /// Owning `SurfaceTrim` parameter-scope record index.
    pub(crate) scope_record_index: u32,
    /// Indexed entity-selection record that starts the trimming tool chain.
    pub(crate) selection_record_index: u32,
    /// Byte offset of the entity-selection record.
    pub(crate) selection_byte_offset: u64,
    /// Indexed record immediately following the entity-selection frame.
    pub(crate) selection_next_record_index: u32,
    /// Byte offset of the record immediately following the entity-selection frame.
    pub(crate) selection_next_byte_offset: u64,
    /// Two indexed records between the entity selection and the cell table.
    pub(crate) chain_records: [DesignSurfaceTrimChainRecord; 2],
    /// Indexed record carrying the counted BRep-cell table.
    pub(crate) cell_table_record_index: u32,
    /// Byte offset of the cell-table primary header.
    pub(crate) cell_table_byte_offset: u64,
    /// Dynamic class tag of the cell-table primary frame.
    pub(crate) cell_table_class_tag: DesignClassTag,
    /// Bytes from the cell-table primary header to its paired header.
    pub(crate) cell_table_frame_length: u64,
    /// Dynamic class tag of the cell-table paired frame.
    pub(crate) cell_table_paired_class_tag: DesignClassTag,
    /// Byte offset of the cell-table paired header.
    pub(crate) cell_table_paired_byte_offset: u64,
    /// Byte offset of the cell-table count.
    pub(crate) cell_count_offset: u64,
    /// Ordered cell-table entries.
    pub(crate) cell_entries: Vec<DesignSurfaceTrimCellEntry>,
    /// Total number of cells in the operation's partition.
    pub(crate) trailing_value: u32,
    /// Byte offset of `trailing_value`.
    pub(crate) trailing_value_offset: u64,
    /// Byte offset of the zero value after `trailing_value`.
    pub(crate) trailing_zero_offset: u64,
}

impl TryFrom<DesignSurfaceTrimOperationWire> for DesignSurfaceTrimOperation {
    type Error = &'static str;
    fn try_from(wire: DesignSurfaceTrimOperationWire) -> Result<Self, Self::Error> {
        let cell_entries =
            NonEmptyVec::new(wire.cell_entries).ok_or("cell_entries must not be empty")?;
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            selection_record_index: wire.selection_record_index,
            selection_byte_offset: wire.selection_byte_offset,
            selection_next_record_index: wire.selection_next_record_index,
            selection_next_byte_offset: wire.selection_next_byte_offset,
            chain_records: wire.chain_records,
            cell_table_record_index: wire.cell_table_record_index,
            cell_table_byte_offset: wire.cell_table_byte_offset,
            cell_table_class_tag: wire.cell_table_class_tag,
            cell_table_frame_length: wire.cell_table_frame_length,
            cell_table_paired_class_tag: wire.cell_table_paired_class_tag,
            cell_table_paired_byte_offset: wire.cell_table_paired_byte_offset,
            cell_count_offset: wire.cell_count_offset,
            cell_entries,
            trailing_value: wire.trailing_value,
            trailing_value_offset: wire.trailing_value_offset,
            trailing_zero_offset: wire.trailing_zero_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignSurfaceTrimOperation> for DesignSurfaceTrimOperationWire {
    fn from(value: DesignSurfaceTrimOperation) -> Self {
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            selection_record_index: value.selection_record_index,
            selection_byte_offset: value.selection_byte_offset,
            selection_next_record_index: value.selection_next_record_index,
            selection_next_byte_offset: value.selection_next_byte_offset,
            chain_records: value.chain_records,
            cell_table_record_index: value.cell_table_record_index,
            cell_table_byte_offset: value.cell_table_byte_offset,
            cell_table_class_tag: value.cell_table_class_tag,
            cell_table_frame_length: value.cell_table_frame_length,
            cell_table_paired_class_tag: value.cell_table_paired_class_tag,
            cell_table_paired_byte_offset: value.cell_table_paired_byte_offset,
            cell_count_offset: value.cell_count_offset,
            cell_entries: value.cell_entries.into_vec(),
            trailing_value: value.trailing_value,
            trailing_value_offset: value.trailing_value_offset,
            trailing_zero_offset: value.trailing_zero_offset,
        }
    }
}

impl DesignSurfaceTrimOperation {
    pub(crate) fn cell_entries(&self) -> &[DesignSurfaceTrimCellEntry] {
        self.cell_entries.as_slice()
    }
}

/// Direction law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignRuledSurfaceMethod {
    /// Generate ruled strips tangent to the support faces.
    Tangent,
    /// Generate ruled strips normal to the support faces.
    Normal,
    /// Generate ruled strips along an explicitly selected direction.
    Direction,
}

/// Corner law encoded by a `SurfaceRuled` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignRuledSurfaceCorner {
    /// Round adjacent ruled strips through a common corner.
    Rounded,
    /// Intersect adjacent ruled strips at a miter.
    Mitered,
}

/// Fixed construction carried by a `SurfaceRuled` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignRuledSurfaceOperation {
    /// Direction law.
    pub(crate) method: DesignRuledSurfaceMethod,
    /// Byte offset of the direction-law enum.
    pub(crate) method_offset: u64,
    /// Corner construction law.
    pub(crate) corner: DesignRuledSurfaceCorner,
    /// Byte offset of the corner-law enum.
    pub(crate) corner_offset: u64,
    /// Whether the opposite incident face supplies the angle reference.
    pub(crate) alternate_face: bool,
    /// Byte offset of the alternate-face Boolean.
    pub(crate) alternate_face_offset: u64,
    /// Referenced ruled-angle parameter owner.
    pub(crate) angle_owner_record_index: u32,
    /// Referenced ruled-distance parameter owner.
    pub(crate) distance_owner_record_index: u32,
    /// Ordered role-`0x08` edge-group records.
    pub(crate) edge_group_record_indices: Vec<u32>,
    /// Ordered auxiliary selection records between the edge-group runs.
    pub(crate) auxiliary_record_indices: Vec<u32>,
    /// Serialized direction entity identity; the all-zero UUID means absent.
    pub(crate) direction_entity_id: Option<DesignRelaxedGuidText>,
}

/// Boundary condition a `SurfacePatch` component imposes against its adjacent
/// face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignPatchContinuity {
    /// Positional continuity only.
    Connected,
    /// First-derivative continuity.
    Tangent,
    /// Second-derivative continuity.
    Curvature,
    /// A serialized value whose continuity meaning is not settled.
    Unknown(u32),
}

impl DesignPatchContinuity {
    /// Decode the serialized continuity ordinal without discarding unknown values.
    #[must_use]
    pub(crate) fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Connected,
            1 => Self::Tangent,
            2 => Self::Curvature,
            code => Self::Unknown(code),
        }
    }
}

/// Versioned whole-surface normal state, independent of boundary-side ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DesignSurfacePatchNormalState {
    Explicit {
        reversed: crate::records::identity::Located<bool>,
    },
    Unsupported,
}

/// Boundary settings and whole-surface normal state of one `SurfacePatch`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DesignSurfacePatchScope {
    pub(crate) boundaries: Vec<DesignSurfacePatchBoundary>,
    pub(crate) normal: Option<DesignSurfacePatchNormalState>,
}

/// Settings a `SurfacePatch` scope carries for one boundary component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSurfacePatchBoundary {
    /// Position of the settings record in the scope's ordered reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Indexed settings-record identity.
    pub(crate) record_index: u32,
    /// Source `IsSeedSel` flag.
    pub(crate) is_seed_selection: bool,
    /// Boundary condition this component imposes against its adjacent face.
    pub(crate) continuity: DesignPatchContinuity,
    /// Source `PatchFlip` ordinal. Retained without a neutral meaning.
    pub(crate) flip: u32,
    /// Raw continuity weight; `-1.0` selects effective `0.5` for G1/G2 and G0 ignores it.
    pub(crate) scale: FiniteReal,
    /// Indexed record the `rPatchModelRef` reference names: this boundary
    /// component's model reference.
    pub(crate) model_reference: u32,
}

#[cfg(test)]
mod tests {
    use super::{
        DesignSurfaceTrimCellEntry, DesignSurfaceTrimChainRecord, DesignSurfaceTrimOperation,
        DesignSurfaceTrimOperationWire, SURFACE_TRIM_OPERATION_CLONE_COUNT,
    };

    fn record(count: u32) -> DesignSurfaceTrimOperation {
        let cell_entries = (0..count)
            .map(|ordinal| DesignSurfaceTrimCellEntry {
                record_index: 10 + ordinal,
                record_reference_offset: 100 + u64::from(ordinal) * 8,
                ordinal: u64::from(ordinal) + 1,
                ordinal_offset: 104 + u64::from(ordinal) * 8,
            })
            .collect();
        DesignSurfaceTrimOperation::try_from(DesignSurfaceTrimOperationWire {
            id: "f3d:native:surface-trim-operation#0".into(),
            scope_record_index: 1,
            selection_record_index: 2,
            selection_byte_offset: 20,
            selection_next_record_index: 3,
            selection_next_byte_offset: 30,
            chain_records: std::array::from_fn(|index| DesignSurfaceTrimChainRecord {
                record_index: 4 + u32::try_from(index).unwrap(),
                byte_offset: 40 + u64::try_from(index).unwrap() * 10,
                class_tag: "300".to_owned().try_into().unwrap(),
                frame_length: 10,
            }),
            cell_table_record_index: 6,
            cell_table_byte_offset: 60,
            cell_table_class_tag: "301".to_owned().try_into().unwrap(),
            cell_table_frame_length: 20,
            cell_table_paired_class_tag: "302".to_owned().try_into().unwrap(),
            cell_table_paired_byte_offset: 80,
            cell_count_offset: 90,
            cell_entries,
            trailing_value: 10,
            trailing_value_offset: 110,
            trailing_zero_offset: 114,
        })
        .unwrap()
    }

    #[test]
    fn surface_trim_operation_borrowed_wire_matches_owned_wire_bytes() {
        for operation in [record(1), record(2)] {
            let owned = DesignSurfaceTrimOperationWire::from(operation.clone());
            assert_eq!(
                serde_json::to_vec(&operation).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }

    #[test]
    fn surface_trim_operation_native_retained_limit_refuses_before_clone() {
        let operation = record(2);
        crate::test_support::native_test::assert_borrowed_native_retained_limit(
            &operation,
            "design_surface_trim_operations",
            || SURFACE_TRIM_OPERATION_CLONE_COUNT.with(|count| count.set(0)),
            || SURFACE_TRIM_OPERATION_CLONE_COUNT.with(std::cell::Cell::get),
        );
    }
}

mod identity_rewrite;
