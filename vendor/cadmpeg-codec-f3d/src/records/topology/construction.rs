// SPDX-License-Identifier: Apache-2.0
//! Construction operand groups, frames, paths, identities and tracking paths.

use super::extrude_selection::DesignExtrudeFaceEncoding;
use super::extrude_selection::DesignExtrudeFaceRole;
use super::extrude_selection::DesignExtrudeOperandRole;
use super::extrude_selection::DesignExtrudeOperandRoleTag;
use super::extrude_selection::DesignOperandRole;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::references::DesignClassTag;
use crate::records::serde_column::SliceColumn;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::Deserialize;
use serde::Serialize;
use std::num::NonZeroU32;

/// A construction role classified in its owning scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesignConstructionOperandRole {
    /// A source role without an Extrude classification.
    Other(DesignOperandRole),
    /// Extrude body operand run A.
    ExtrudeBodiesA,
    /// Extrude body operand run B.
    ExtrudeBodiesB,
    /// Extrude profile operand run.
    ExtrudeProfile,
    /// An ordered Extrude face operand run.
    ExtrudeFaces {
        /// Source encoding admitted by the Extrude scope.
        encoding: DesignExtrudeFaceEncoding,
        /// Use assigned by the face run's position in the scope.
        usage: DesignExtrudeFaceRole,
    },
}

impl DesignConstructionOperandRole {
    /// The source role code.
    fn source(self) -> DesignOperandRole {
        match self {
            Self::Other(role) => role,
            Self::ExtrudeBodiesA => DesignOperandRole::BODIES_A,
            Self::ExtrudeBodiesB => DesignOperandRole::BODIES_B,
            Self::ExtrudeProfile => DesignOperandRole::PROFILE,
            Self::ExtrudeFaces { encoding, .. } => match encoding {
                DesignExtrudeFaceEncoding::Faces => DesignOperandRole::FACES,
                DesignExtrudeFaceEncoding::SelectedStart => DesignOperandRole::ROLE_0X5,
                DesignExtrudeFaceEncoding::LegacyTermination => DesignOperandRole::ROLE_0X12,
            },
        }
    }

    /// The Extrude role of a scope-classified source encoding.
    fn extrude(self) -> Option<DesignExtrudeOperandRole> {
        match self {
            Self::Other(_) => None,
            Self::ExtrudeBodiesA | Self::ExtrudeBodiesB => Some(DesignExtrudeOperandRole::Bodies),
            Self::ExtrudeProfile => Some(DesignExtrudeOperandRole::Profile),
            Self::ExtrudeFaces { usage, .. } => Some(DesignExtrudeOperandRole::Faces(usage)),
        }
    }
}

/// Construction-operand group owned by a feature scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandGroupSerde")]
pub(crate) struct DesignConstructionOperandGroup {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning feature scope record.
    pub(crate) scope_record_index: u32,
    /// Position in the scope reference table.
    pub(crate) scope_reference_ordinal: u32,
    /// Primary indexed-record identity.
    pub(crate) record_index: u32,
    /// Primary indexed-header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Ordered operand-record references.
    members: Vec<Located<u32>>,
    /// Ordered unresolved-edge records whose run terminates at this group's identity.
    pub(crate) lost_edge_references: Vec<String>,
    /// Exact framing of the operand-member run and its auxiliary fields.
    pub(crate) frame: DesignConstructionOperandGroupFrame,
    /// Source role classified in its owning scope.
    pub(crate) operand_role: DesignConstructionOperandRole,
    /// Per-file dynamic paired class tag.
    paired_class_tag: DesignClassTag,
    /// Same-index paired-header byte offset.
    pub(crate) paired_byte_offset: u64,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_GROUP_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandGroup {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_GROUP_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.clone(),
            members: self.members.clone(),
            lost_edge_references: self.lost_edge_references.clone(),
            frame: self.frame.clone(),
            operand_role: self.operand_role,
            paired_class_tag: self.paired_class_tag.clone(),
            paired_byte_offset: self.paired_byte_offset,
        }
    }
}

impl Serialize for DesignConstructionOperandGroup {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            scope_record_index: u32,
            scope_reference_ordinal: u32,
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            members: SliceColumn<'a, Located<u32>, u32>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            lost_edge_references: &'a Vec<String>,
            member_offsets: SliceColumn<'a, Located<u32>, u64>,
            frame: &'a DesignConstructionOperandGroupFrame,
            role: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            extrude_role: Option<DesignExtrudeOperandRoleTag>,
            #[serde(skip_serializing_if = "Option::is_none")]
            extrude_face_role: Option<DesignExtrudeFaceRole>,
            role_offset: u64,
            paired_class_tag: &'a str,
            paired_byte_offset: u64,
        }
        let (extrude_role, extrude_face_role) = match self.operand_role.extrude() {
            Some(DesignExtrudeOperandRole::Bodies) => {
                (Some(DesignExtrudeOperandRoleTag::Bodies), None)
            }
            Some(DesignExtrudeOperandRole::Profile) => {
                (Some(DesignExtrudeOperandRoleTag::Profile), None)
            }
            Some(DesignExtrudeOperandRole::Faces(face_role)) => {
                (Some(DesignExtrudeOperandRoleTag::Faces), Some(face_role))
            }
            None => (None, None),
        };
        BorrowedWire {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            scope_reference_ordinal: self.scope_reference_ordinal,
            record_index: self.record_index,
            byte_offset: self.byte_offset,
            class_tag: self.class_tag.as_str(),
            members: SliceColumn::new(&self.members, |member| member.value),
            lost_edge_references: &self.lost_edge_references,
            member_offsets: SliceColumn::new(&self.members, |member| member.offset),
            frame: &self.frame,
            role: self.role().raw(),
            extrude_role,
            extrude_face_role,
            role_offset: self.role_offset(),
            paired_class_tag: self.paired_class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset,
        }
        .serialize(serializer)
    }
}

/// Unchecked construction-group input.
pub(crate) struct DesignConstructionOperandGroupDraft {
    pub(crate) id: String,
    pub(crate) scope_record_index: u32,
    pub(crate) scope_reference_ordinal: u32,
    pub(crate) record_index: u32,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
    pub(crate) members: Vec<Located<u32>>,
    pub(crate) lost_edge_references: Vec<String>,
    pub(crate) frame: DesignConstructionOperandGroupFrame,
    pub(crate) operand_role: DesignConstructionOperandRole,
    pub(crate) role_offset: u64,
    pub(crate) paired_class_tag: DesignClassTag,
    pub(crate) paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupDraft> for DesignConstructionOperandGroup {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupDraft) -> Result<Self, Self::Error> {
        Self::check_members(&draft.members)?;
        if draft.role_offset != draft.frame.role_offset() {
            return Err("role_offset must precede opaque_index_offset by 18 bytes".into());
        }
        Ok(Self {
            id: draft.id,
            scope_record_index: draft.scope_record_index,
            scope_reference_ordinal: draft.scope_reference_ordinal,
            record_index: draft.record_index,
            byte_offset: draft.byte_offset,
            class_tag: draft.class_tag,
            members: draft.members,
            lost_edge_references: draft.lost_edge_references,
            frame: draft.frame,
            operand_role: draft.operand_role,
            paired_class_tag: draft.paired_class_tag,
            paired_byte_offset: draft.paired_byte_offset,
        })
    }
}

impl DesignConstructionOperandGroup {
    fn check_members(members: &[Located<u32>]) -> Result<(), String> {
        if members.windows(2).any(|pair| {
            pair[0]
                .offset
                .checked_add(11)
                .is_none_or(|next| pair[1].offset < next)
        }) {
            return Err("construction member offsets must have a stride of at least 11".into());
        }
        Ok(())
    }

    /// Ordered operand references with checked strides.
    pub(crate) fn members(&self) -> &[Located<u32>] {
        &self.members
    }

    /// Checked replacement of the operand-reference run.
    #[cfg(test)]
    pub(crate) fn try_set_members(&mut self, members: Vec<Located<u32>>) -> Result<(), String> {
        Self::check_members(&members)?;
        self.members = members;
        Ok(())
    }

    /// Role offset derived from the opaque-index location.
    pub(crate) fn role_offset(&self) -> u64 {
        self.frame.role_offset()
    }

    /// The source role code.
    pub(crate) fn role(&self) -> DesignOperandRole {
        self.operand_role.source()
    }

    /// The Extrude role derived from the scope-classified source encoding.
    pub(crate) fn extrude_role(&self) -> Option<DesignExtrudeOperandRole> {
        self.operand_role.extrude()
    }

    pub(crate) fn extrude_face_role(&self) -> Option<DesignExtrudeFaceRole> {
        match self.extrude_role() {
            Some(DesignExtrudeOperandRole::Faces(role)) => Some(role),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandGroupSerde {
    id: String,
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    byte_offset: u64,
    class_tag: String,
    members: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lost_edge_references: Vec<String>,
    member_offsets: Vec<u64>,
    frame: DesignConstructionOperandGroupFrame,
    role: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_role"
    )]
    extrude_role: Option<DesignExtrudeOperandRoleTag>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_extrude_face_role"
    )]
    extrude_face_role: Option<DesignExtrudeFaceRole>,
    role_offset: u64,
    paired_class_tag: String,
    paired_byte_offset: u64,
}

impl TryFrom<DesignConstructionOperandGroupSerde> for DesignConstructionOperandGroup {
    type Error = String;

    fn try_from(wire: DesignConstructionOperandGroupSerde) -> Result<Self, Self::Error> {
        if wire.members.len() != wire.member_offsets.len() {
            return Err("members and member_offsets must have equal lengths".into());
        }
        let role = DesignOperandRole::from_raw(wire.role);
        let operand_role = match (role, wire.extrude_role, wire.extrude_face_role) {
            (DesignOperandRole::BODIES_A, Some(DesignExtrudeOperandRoleTag::Bodies), None) => {
                DesignConstructionOperandRole::ExtrudeBodiesA
            }
            (DesignOperandRole::BODIES_B, Some(DesignExtrudeOperandRoleTag::Bodies), None) => {
                DesignConstructionOperandRole::ExtrudeBodiesB
            }
            (DesignOperandRole::PROFILE, Some(DesignExtrudeOperandRoleTag::Profile), None) => {
                DesignConstructionOperandRole::ExtrudeProfile
            }
            (role, Some(DesignExtrudeOperandRoleTag::Faces), Some(usage)) => {
                let encoding = match role {
                    DesignOperandRole::FACES => DesignExtrudeFaceEncoding::Faces,
                    DesignOperandRole::ROLE_0X5 => DesignExtrudeFaceEncoding::SelectedStart,
                    DesignOperandRole::ROLE_0X12 => DesignExtrudeFaceEncoding::LegacyTermination,
                    _ => return Err("role does not encode a faces extrude_role".into()),
                };
                DesignConstructionOperandRole::ExtrudeFaces { encoding, usage }
            }
            (role, None, None) => DesignConstructionOperandRole::Other(role),
            _ => return Err("role, extrude_role, and extrude_face_role disagree".into()),
        };
        Self::try_from(DesignConstructionOperandGroupDraft {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            scope_reference_ordinal: wire.scope_reference_ordinal,
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            members: wire
                .members
                .into_iter()
                .zip(wire.member_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            lost_edge_references: wire.lost_edge_references,
            frame: wire.frame,
            operand_role,
            role_offset: wire.role_offset,
            paired_class_tag: wire.paired_class_tag.try_into()?,
            paired_byte_offset: wire.paired_byte_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandGroup> for DesignConstructionOperandGroupSerde {
    fn from(group: DesignConstructionOperandGroup) -> Self {
        let role_offset = group.role_offset();
        let (members, member_offsets) = group
            .members
            .into_iter()
            .map(|member| (member.value, member.offset))
            .unzip();
        let (extrude_role, extrude_face_role) = match group.operand_role.extrude() {
            Some(DesignExtrudeOperandRole::Bodies) => {
                (Some(DesignExtrudeOperandRoleTag::Bodies), None)
            }
            Some(DesignExtrudeOperandRole::Profile) => {
                (Some(DesignExtrudeOperandRoleTag::Profile), None)
            }
            Some(DesignExtrudeOperandRole::Faces(face_role)) => {
                (Some(DesignExtrudeOperandRoleTag::Faces), Some(face_role))
            }
            None => (None, None),
        };
        Self {
            id: group.id,
            scope_record_index: group.scope_record_index,
            scope_reference_ordinal: group.scope_reference_ordinal,
            record_index: group.record_index,
            byte_offset: group.byte_offset,
            class_tag: group.class_tag.into(),
            members,
            lost_edge_references: group.lost_edge_references,
            member_offsets,
            frame: group.frame,
            role: group.operand_role.source().raw(),
            extrude_role,
            extrude_face_role,
            role_offset,
            paired_class_tag: group.paired_class_tag.into(),
            paired_byte_offset: group.paired_byte_offset,
        }
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandGroupFrameWire")]
pub(crate) struct DesignConstructionOperandGroupFrame {
    /// Byte offset of the member count.
    pub(crate) member_count_offset: u64,
    /// Auxiliary records named by the two optional references that follow the
    /// member run; an absent reference contributes no entry.
    pub(crate) auxiliary_records: Vec<Located<u32>>,
    /// Exact selection-path records selected by the optional references.
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    /// Indexed records named by the counted trailing-reference run. The target
    /// grammar is selected by the owning operand family: persistent-selection
    /// groups name identity wrappers and placed-selection groups name affine
    /// transforms.
    trailing_records: Option<Located<u32>>,
    /// Exact affine-transform records selected from the trailing-reference
    /// run. Other trailing records remain represented by their indices and
    /// offsets and can select another typed grammar.
    trailing_transforms: Vec<DesignConstructionOperandTransform>,
    /// Exact dual-transform records selected from the trailing-reference run.
    trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    /// Exact compact flag records selected from the trailing-reference run.
    trailing_flags: Vec<DesignConstructionOperandFlag>,
    /// Opaque ordinal: nonzero and below 256, repeated after `opaque_scalar` in
    /// every container generation but one.
    pub(crate) opaque_index: NonZeroU32,
    /// Byte offset of the first `opaque_index` copy.
    opaque_index_offset: u64,
    /// Opaque scalar.
    opaque_scalar: cadmpeg_ir::scalar::NonNegativeReal,
    /// Boolean tail variant.
    pub(crate) variant: bool,
    /// Last byte of the tail flag block.
    pub(crate) closing_flag: bool,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_FRAME_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandGroupFrame {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_FRAME_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            member_count_offset: self.member_count_offset,
            auxiliary_records: self.auxiliary_records.clone(),
            auxiliary_paths: self.auxiliary_paths.clone(),
            trailing_records: self.trailing_records,
            trailing_transforms: self.trailing_transforms.clone(),
            trailing_dual_transforms: self.trailing_dual_transforms.clone(),
            trailing_flags: self.trailing_flags.clone(),
            opaque_index: self.opaque_index,
            opaque_index_offset: self.opaque_index_offset,
            opaque_scalar: self.opaque_scalar,
            variant: self.variant,
            closing_flag: self.closing_flag,
        }
    }
}

impl Serialize for DesignConstructionOperandGroupFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            member_count_offset: u64,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            auxiliary_record_indices: SliceColumn<'a, Located<u32>, u32>,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            auxiliary_record_offsets: SliceColumn<'a, Located<u32>, u64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            auxiliary_paths: &'a Vec<DesignConstructionOperandPath>,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            trailing_record_indices: SliceColumn<'a, Located<u32>, u32>,
            #[serde(skip_serializing_if = "SliceColumn::is_empty")]
            trailing_record_offsets: SliceColumn<'a, Located<u32>, u64>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_transforms: &'a Vec<DesignConstructionOperandTransform>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_dual_transforms: &'a Vec<DesignConstructionOperandDualTransform>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            trailing_flags: &'a Vec<DesignConstructionOperandFlag>,
            opaque_index: u32,
            opaque_index_offset: u64,
            opaque_scalar: f64,
            opaque_scalar_offset: u64,
            variant: bool,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            closing_flag: bool,
        }
        let trailing = self.trailing_records.as_slice();
        BorrowedWire {
            member_count_offset: self.member_count_offset,
            auxiliary_record_indices: SliceColumn::new(&self.auxiliary_records, |row| row.value),
            auxiliary_record_offsets: SliceColumn::new(&self.auxiliary_records, |row| row.offset),
            auxiliary_paths: &self.auxiliary_paths,
            trailing_record_indices: SliceColumn::new(trailing, |row| row.value),
            trailing_record_offsets: SliceColumn::new(trailing, |row| row.offset),
            trailing_transforms: &self.trailing_transforms,
            trailing_dual_transforms: &self.trailing_dual_transforms,
            trailing_flags: &self.trailing_flags,
            opaque_index: self.opaque_index.get(),
            opaque_index_offset: self.opaque_index_offset(),
            opaque_scalar: self.opaque_scalar().get(),
            opaque_scalar_offset: self.opaque_scalar_offset(),
            variant: self.variant,
            closing_flag: self.closing_flag,
        }
        .serialize(serializer)
    }
}

/// Construction-frame fields before aggregate validation.
pub(crate) struct DesignConstructionOperandGroupFrameDraft<T = f64, I = u32> {
    pub(crate) member_count_offset: u64,
    pub(crate) auxiliary_records: Vec<Located<u32>>,
    pub(crate) auxiliary_paths: Vec<DesignConstructionOperandPath>,
    pub(crate) trailing_records: Vec<Located<u32>>,
    pub(crate) trailing_transforms: Vec<DesignConstructionOperandTransform>,
    pub(crate) trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    pub(crate) trailing_flags: Vec<DesignConstructionOperandFlag>,
    pub(crate) opaque_index: I,
    pub(crate) opaque_index_offset: u64,
    pub(crate) opaque_scalar: T,
    pub(crate) opaque_scalar_offset: u64,
    pub(crate) variant: bool,
    pub(crate) closing_flag: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameDraft> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandGroupFrameDraft) -> Result<Self, Self::Error> {
        if draft.trailing_records.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        let opaque_index =
            NonZeroU32::new(draft.opaque_index).ok_or("opaque_index must be nonzero")?;
        let opaque_scalar = cadmpeg_ir::scalar::NonNegativeReal::new(draft.opaque_scalar)
            .ok_or("opaque_scalar must be finite and nonnegative")?;
        Self::from_parts(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: draft.member_count_offset,
            auxiliary_records: draft.auxiliary_records,
            auxiliary_paths: draft.auxiliary_paths,
            trailing_records: draft.trailing_records,
            trailing_transforms: draft.trailing_transforms,
            trailing_dual_transforms: draft.trailing_dual_transforms,
            trailing_flags: draft.trailing_flags,
            opaque_index,
            opaque_index_offset: draft.opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset: draft.opaque_scalar_offset,
            variant: draft.variant,
            closing_flag: draft.closing_flag,
        })
    }
}

impl DesignConstructionOperandGroupFrame {
    pub(crate) fn from_parts(
        draft: DesignConstructionOperandGroupFrameDraft<
            cadmpeg_ir::scalar::NonNegativeReal,
            NonZeroU32,
        >,
    ) -> Result<Self, String> {
        if draft.trailing_records.len() > 1 {
            return Err("construction frame permits at most one trailing record".into());
        }
        if draft.opaque_index_offset < 18
            || draft.opaque_index_offset.checked_add(4) != Some(draft.opaque_scalar_offset)
        {
            return Err("opaque_index_offset and opaque_scalar_offset must follow the role by 18 and 22 bytes".into());
        }
        distinct_construction_records(
            "auxiliary_paths",
            draft
                .auxiliary_paths
                .iter()
                .map(DesignConstructionOperandPath::record_index),
        )?;
        distinct_construction_records(
            "trailing_transforms",
            draft
                .trailing_transforms
                .iter()
                .map(DesignConstructionOperandTransform::record_index),
        )?;
        distinct_construction_records(
            "trailing_dual_transforms",
            draft
                .trailing_dual_transforms
                .iter()
                .map(|record| record.record_index),
        )?;
        distinct_construction_records(
            "trailing_flags",
            draft
                .trailing_flags
                .iter()
                .map(|record| record.record_index),
        )?;
        Ok(Self {
            member_count_offset: draft.member_count_offset,
            auxiliary_records: draft.auxiliary_records,
            auxiliary_paths: draft.auxiliary_paths,
            trailing_records: draft.trailing_records.into_iter().next(),
            trailing_transforms: draft.trailing_transforms,
            trailing_dual_transforms: draft.trailing_dual_transforms,
            trailing_flags: draft.trailing_flags,
            opaque_index: draft.opaque_index,
            opaque_index_offset: draft.opaque_index_offset,
            opaque_scalar: draft.opaque_scalar,
            variant: draft.variant,
            closing_flag: draft.closing_flag,
        })
    }
}

fn distinct_construction_records(
    field: &str,
    records: impl Iterator<Item = u32>,
) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for record in records {
        if !seen.insert(record) {
            return Err(format!("{field} record_index values must be distinct"));
        }
    }
    Ok(())
}

impl DesignConstructionOperandGroupFrame {
    /// Role offset derived from the opaque-index location.
    fn role_offset(&self) -> u64 {
        self.opaque_index_offset - 18
    }
    /// First opaque-index location.
    pub(crate) fn opaque_index_offset(&self) -> u64 {
        self.opaque_index_offset
    }
    /// Scalar location following the opaque index.
    pub(crate) fn opaque_scalar_offset(&self) -> u64 {
        self.opaque_index_offset + 4
    }
    /// Opaque scalar.
    pub(crate) fn opaque_scalar(&self) -> cadmpeg_ir::scalar::NonNegativeReal {
        self.opaque_scalar
    }
    /// Zero or one trailing reference.
    pub(crate) fn trailing_records(&self) -> &[Located<u32>] {
        self.trailing_records.as_slice()
    }
    /// Distinct auxiliary path records.
    pub(crate) fn auxiliary_paths(&self) -> &[DesignConstructionOperandPath] {
        &self.auxiliary_paths
    }
    /// Checked replacement of auxiliary path records.
    pub(crate) fn try_set_auxiliary_paths(
        &mut self,
        records: Vec<DesignConstructionOperandPath>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "auxiliary_paths",
            records
                .iter()
                .map(DesignConstructionOperandPath::record_index),
        )?;
        self.auxiliary_paths = records;
        Ok(())
    }
    /// Distinct trailing affine transforms.
    pub(crate) fn trailing_transforms(&self) -> &[DesignConstructionOperandTransform] {
        &self.trailing_transforms
    }
    /// Checked replacement of trailing affine transforms.
    pub(crate) fn try_set_trailing_transforms(
        &mut self,
        records: Vec<DesignConstructionOperandTransform>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_transforms",
            records
                .iter()
                .map(DesignConstructionOperandTransform::record_index),
        )?;
        self.trailing_transforms = records;
        Ok(())
    }
    /// Distinct trailing dual transforms.
    pub(crate) fn trailing_dual_transforms(&self) -> &[DesignConstructionOperandDualTransform] {
        &self.trailing_dual_transforms
    }
    /// Checked replacement of trailing dual transforms.
    pub(crate) fn try_set_trailing_dual_transforms(
        &mut self,
        records: Vec<DesignConstructionOperandDualTransform>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_dual_transforms",
            records.iter().map(|record| record.record_index),
        )?;
        self.trailing_dual_transforms = records;
        Ok(())
    }
    /// Distinct trailing flag records.
    pub(crate) fn trailing_flags(&self) -> &[DesignConstructionOperandFlag] {
        &self.trailing_flags
    }
    /// Checked replacement of trailing flag records.
    pub(crate) fn try_set_trailing_flags(
        &mut self,
        records: Vec<DesignConstructionOperandFlag>,
    ) -> Result<(), String> {
        distinct_construction_records(
            "trailing_flags",
            records.iter().map(|record| record.record_index),
        )?;
        self.trailing_flags = records;
        Ok(())
    }
}

/// Serialized framing of a construction-operand group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandGroupFrameWire {
    member_count_offset: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    auxiliary_paths: Vec<DesignConstructionOperandPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_indices: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_record_offsets: Vec<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_transforms: Vec<DesignConstructionOperandTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_dual_transforms: Vec<DesignConstructionOperandDualTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    trailing_flags: Vec<DesignConstructionOperandFlag>,
    opaque_index: u32,
    opaque_index_offset: u64,
    opaque_scalar: f64,
    opaque_scalar_offset: u64,
    variant: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    closing_flag: bool,
}

impl TryFrom<DesignConstructionOperandGroupFrameWire> for DesignConstructionOperandGroupFrame {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandGroupFrameWire) -> Result<Self, Self::Error> {
        if wire.auxiliary_record_indices.len() != wire.auxiliary_record_offsets.len() {
            return Err("auxiliary_record_offsets must match auxiliary_record_indices".into());
        }
        if wire.trailing_record_indices.len() != wire.trailing_record_offsets.len() {
            return Err("trailing_record_offsets must match trailing_record_indices".into());
        }
        Self::try_from(DesignConstructionOperandGroupFrameDraft {
            member_count_offset: wire.member_count_offset,
            auxiliary_records: wire
                .auxiliary_record_indices
                .into_iter()
                .zip(wire.auxiliary_record_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            auxiliary_paths: wire.auxiliary_paths,
            trailing_records: wire
                .trailing_record_indices
                .into_iter()
                .zip(wire.trailing_record_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            trailing_transforms: wire.trailing_transforms,
            trailing_dual_transforms: wire.trailing_dual_transforms,
            trailing_flags: wire.trailing_flags,
            opaque_index: wire.opaque_index,
            opaque_index_offset: wire.opaque_index_offset,
            opaque_scalar: wire.opaque_scalar,
            opaque_scalar_offset: wire.opaque_scalar_offset,
            variant: wire.variant,
            closing_flag: wire.closing_flag,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandGroupFrame> for DesignConstructionOperandGroupFrameWire {
    fn from(frame: DesignConstructionOperandGroupFrame) -> Self {
        let opaque_scalar_offset = frame.opaque_scalar_offset();
        let opaque_scalar = frame.opaque_scalar().get();
        let opaque_index_offset = frame.opaque_index_offset();
        Self {
            member_count_offset: frame.member_count_offset,
            auxiliary_record_indices: frame
                .auxiliary_records
                .iter()
                .map(|record| record.value)
                .collect(),
            auxiliary_record_offsets: frame
                .auxiliary_records
                .iter()
                .map(|record| record.offset)
                .collect(),
            auxiliary_paths: frame.auxiliary_paths,
            trailing_record_indices: frame
                .trailing_records
                .iter()
                .map(|record| record.value)
                .collect(),
            trailing_record_offsets: frame
                .trailing_records
                .iter()
                .map(|record| record.offset)
                .collect(),
            trailing_transforms: frame.trailing_transforms,
            trailing_dual_transforms: frame.trailing_dual_transforms,
            trailing_flags: frame.trailing_flags,
            opaque_index: frame.opaque_index.get(),
            opaque_index_offset,
            opaque_scalar,
            opaque_scalar_offset,
            variant: frame.variant,
            closing_flag: frame.closing_flag,
        }
    }
}

/// Compact boolean record named by a construction-operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionOperandFlag {
    /// Indexed flag-record identity.
    pub(crate) record_index: u32,
    /// Flag-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic flag-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Stored boolean value.
    pub(crate) value: bool,
    /// Byte offset of the stored boolean.
    pub(crate) value_offset: u64,
}

/// Affine placement named by a construction-operand group's trailing run.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandTransformDraft")]
pub(crate) struct DesignConstructionOperandTransform {
    frame: crate::records::frame_chain::RecordFrameChain,
    following_record_index: u32,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Row-major local-to-model affine transform.
    pub(crate) transform: SketchPlacementMatrix,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_TRANSFORM_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandTransform {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            following_record_index: self.following_record_index,
            class_tag: self.class_tag.clone(),
            transform: self.transform,
            following_class_tag: self.following_class_tag.clone(),
        }
    }
}

impl Serialize for DesignConstructionOperandTransform {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a DesignClassTag,
            transform: SketchPlacementMatrix,
            transform_offset: u64,
            following_record_index: u32,
            following_byte_offset: u64,
            following_class_tag: &'a DesignClassTag,
        }
        WireRef {
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: &self.class_tag,
            transform: self.transform,
            transform_offset: self.transform_offset(),
            following_record_index: self.following_record_index(),
            following_byte_offset: self.following_byte_offset(),
            following_class_tag: &self.following_class_tag,
        }
        .serialize(serializer)
    }
}

impl DesignConstructionOperandTransform {
    pub(crate) fn try_new(draft: DesignConstructionOperandTransformDraft) -> Result<Self, String> {
        if draft.following_record_index <= draft.record_index {
            return Err("following_record_index must exceed record_index".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            0,
            152,
        )?;
        let value = Self {
            frame,
            following_record_index: draft.following_record_index,
            class_tag: draft.class_tag,
            transform: draft.transform,
            following_class_tag: draft.following_class_tag,
        };
        if value.transform_offset() != draft.transform_offset {
            return Err("transform_offset disagrees with frame layout".into());
        }
        if value.following_byte_offset() != draft.following_byte_offset {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    fn into_draft(self) -> DesignConstructionOperandTransformDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let following_record_index = self.following_record_index();
        let transform_offset = self.transform_offset();
        let following_byte_offset = self.following_byte_offset();
        DesignConstructionOperandTransformDraft {
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            transform: self.transform,
            transform_offset,
            following_record_index,
            following_byte_offset,
            following_class_tag: self.following_class_tag,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn transform_offset(&self) -> u64 {
        self.frame.offset(22)
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.following_record_index
    }
    pub(crate) fn following_byte_offset(&self) -> u64 {
        self.frame.offset(152)
    }
}

/// Unadmitted `DesignConstructionOperandTransform` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionOperandTransformDraft {
    /// Indexed transform-record identity.
    pub(crate) record_index: u32,
    /// Transform-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Row-major local-to-model affine transform.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
    /// Indexed record immediately following the transform.
    pub(crate) following_record_index: u32,
    /// Following-record header byte offset.
    pub(crate) following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

impl TryFrom<DesignConstructionOperandTransformDraft> for DesignConstructionOperandTransform {
    type Error = String;
    fn try_from(draft: DesignConstructionOperandTransformDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandTransform> for DesignConstructionOperandTransformDraft {
    fn from(value: DesignConstructionOperandTransform) -> Self {
        let value = value.into_draft();
        Self {
            record_index: value.record_index,
            byte_offset: value.byte_offset,
            class_tag: value.class_tag,
            transform: value.transform,
            transform_offset: value.transform_offset,
            following_record_index: value.following_record_index,
            following_byte_offset: value.following_byte_offset,
            following_class_tag: value.following_class_tag,
        }
    }
}

/// Two ordered affine placements named by an operand group's trailing run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionOperandDualTransform {
    /// Indexed transform-record identity.
    pub(crate) record_index: u32,
    /// Transform-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic transform-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// First row-major affine transform.
    pub(crate) first_transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) first_transform_offset: u64,
    /// Second row-major affine transform.
    pub(crate) second_transform: SketchPlacementMatrix,
    /// Byte offset of the second matrix scalar.
    pub(crate) second_transform_offset: u64,
}

/// One persistent-entity step in a construction operand's selection path.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandPathWire")]
pub(crate) struct DesignConstructionOperandPath {
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Per-file dynamic path-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Persistent entity identity carried by this path step.
    pub(crate) entity_ref: u64,
    /// Transform or compact selection-path layout.
    placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub(crate) scope_record_index: u32,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
    /// Whether the entity reference follows the extended path prefix.
    extended_prefix: bool,
}

/// Entity-reference offset in the short path prefix.
const SHORT_PATH_ENTITY_REFERENCE: u64 = 22;

/// Entity-reference offset in the extended path prefix.
const EXTENDED_PATH_ENTITY_REFERENCE: u64 = cadmpeg_core::decode::u64_from_index(
    crate::layout::construction_operand_extended_path_prefix::ENTITY_REFERENCE,
);

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_PATH_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandPath {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_PATH_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            class_tag: self.class_tag.clone(),
            entity_ref: self.entity_ref,
            placement: self.placement,
            scope_record_index: self.scope_record_index,
            following_class_tag: self.following_class_tag.clone(),
            extended_prefix: self.extended_prefix,
        }
    }
}

impl Serialize for DesignConstructionOperandPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            record_index: u32,
            byte_offset: u64,
            class_tag: &'a str,
            entity_ref: u64,
            entity_ref_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform: Option<SketchPlacementMatrix>,
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            compact_variant: Option<bool>,
            scope_record_index: u32,
            scope_record_index_offset: u64,
            nested_record_index: u32,
            nested_record_index_offset: u64,
            following_record_index: u32,
            following_byte_offset: u64,
            following_class_tag: &'a str,
        }
        let (transform, transform_offset, compact_variant) = match self.placement {
            DesignConstructionPathPlacement::Transform(transform) => {
                (Some(transform), Some(self.transform_offset()), None)
            }
            DesignConstructionPathPlacement::Compact(variant) => (None, None, Some(variant)),
        };
        BorrowedWire {
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            entity_ref: self.entity_ref,
            entity_ref_offset: self.entity_ref_offset(),
            transform,
            transform_offset,
            compact_variant,
            scope_record_index: self.scope_record_index,
            scope_record_index_offset: self.scope_record_index_offset(),
            nested_record_index: self.nested_record_index(),
            nested_record_index_offset: self.nested_record_index_offset(),
            following_record_index: self.following_record_index(),
            following_byte_offset: self.following_byte_offset(),
            following_class_tag: self.following_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}

impl DesignConstructionOperandPath {
    pub(crate) fn try_new(draft: DesignConstructionOperandPathDraft) -> Result<Self, String> {
        let extended_prefix = draft
            .byte_offset
            .checked_add(EXTENDED_PATH_ENTITY_REFERENCE)
            == Some(draft.entity_ref_offset);
        let prefix_shift = if extended_prefix {
            EXTENDED_PATH_ENTITY_REFERENCE - SHORT_PATH_ENTITY_REFERENCE
        } else {
            0
        };
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.record_index,
            draft.byte_offset,
            2,
            prefix_shift
                + if matches!(
                    draft.placement,
                    DesignConstructionPathPlacement::Transform(_)
                ) {
                    190
                } else {
                    62
                },
        )?;
        let value = Self {
            frame,
            class_tag: draft.class_tag,
            entity_ref: draft.entity_ref,
            placement: draft.placement,
            scope_record_index: draft.scope_record_index,
            following_class_tag: draft.following_class_tag,
            extended_prefix,
        };
        if value.entity_ref_offset() != draft.entity_ref_offset {
            return Err("entity_ref_offset disagrees with frame layout".into());
        }
        if value.nested_record_index() != draft.nested_record_index {
            return Err("nested_record_index disagrees with frame layout".into());
        }
        if value.following_record_index() != draft.following_record_index {
            return Err("following_record_index disagrees with frame layout".into());
        }
        if value.scope_record_index_offset() != draft.scope_record_index_offset {
            return Err("scope_record_index_offset disagrees with frame layout".into());
        }
        if value.nested_record_index_offset() != draft.nested_record_index_offset {
            return Err("nested_record_index_offset disagrees with frame layout".into());
        }
        if value.following_byte_offset() != draft.following_byte_offset {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignConstructionOperandPathDraft {
        let record_index = self.record_index();
        let byte_offset = self.byte_offset();
        let entity_ref_offset = self.entity_ref_offset();
        let nested_record_index = self.nested_record_index();
        let following_record_index = self.following_record_index();
        let scope_record_index_offset = self.scope_record_index_offset();
        let nested_record_index_offset = self.nested_record_index_offset();
        let following_byte_offset = self.following_byte_offset();
        DesignConstructionOperandPathDraft {
            record_index,
            byte_offset,
            class_tag: self.class_tag,
            entity_ref: self.entity_ref,
            entity_ref_offset,
            placement: self.placement,
            scope_record_index: self.scope_record_index,
            scope_record_index_offset,
            nested_record_index,
            nested_record_index_offset,
            following_record_index,
            following_byte_offset,
            following_class_tag: self.following_class_tag,
        }
    }
    pub(crate) fn record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    fn prefix_shift(&self) -> u64 {
        if self.extended_prefix {
            EXTENDED_PATH_ENTITY_REFERENCE - SHORT_PATH_ENTITY_REFERENCE
        } else {
            0
        }
    }
    fn entity_ref_offset(&self) -> u64 {
        self.frame
            .offset(SHORT_PATH_ENTITY_REFERENCE + self.prefix_shift())
    }
    fn transform_offset(&self) -> u64 {
        self.byte_offset() + 33 + self.prefix_shift()
    }
    pub(crate) fn scope_record_index_offset(&self) -> u64 {
        self.frame.offset(
            self.prefix_shift()
                + if matches!(
                    self.placement,
                    DesignConstructionPathPlacement::Transform(_)
                ) {
                    163
                } else {
                    35
                },
        )
    }
    pub(crate) fn nested_record_index(&self) -> u32 {
        self.frame.index(2)
    }
    pub(crate) fn nested_record_index_offset(&self) -> u64 {
        self.scope_record_index_offset() + 11
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.frame.index(1)
    }
    pub(crate) fn following_byte_offset(&self) -> u64 {
        self.scope_record_index_offset() + 27
    }
}

/// Unadmitted `DesignConstructionOperandPath` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionOperandPathDraft {
    /// Indexed path-record identity.
    pub(crate) record_index: u32,
    /// Path-record header byte offset.
    pub(crate) byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Persistent entity identity carried by this path step.
    pub(crate) entity_ref: u64,
    /// Byte offset of `entity_ref`.
    pub(crate) entity_ref_offset: u64,
    /// Transform or compact selection-path layout.
    pub(crate) placement: DesignConstructionPathPlacement,
    /// Owning feature-scope record.
    pub(crate) scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    pub(crate) scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    pub(crate) nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    pub(crate) nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    pub(crate) following_record_index: u32,
    /// Following-record header byte offset.
    pub(crate) following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandPathWire {
    /// Indexed path-record identity.
    record_index: u32,
    /// Path-record header byte offset.
    byte_offset: u64,
    /// Per-file dynamic path-record class tag.
    class_tag: String,
    /// Persistent entity identity carried by this path step.
    entity_ref: u64,
    /// Byte offset of `entity_ref`.
    entity_ref_offset: u64,
    /// Optional row-major selection-path placement.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
    /// Byte offset of the first transform scalar.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Compact-frame boolean; absent from the transform frame.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_compact_variant"
    )]
    compact_variant: Option<bool>,
    /// Owning feature-scope record.
    scope_record_index: u32,
    /// Byte offset of the owning-scope reference.
    scope_record_index_offset: u64,
    /// Nested record selected after the owning scope.
    nested_record_index: u32,
    /// Byte offset of the nested-record reference.
    nested_record_index_offset: u64,
    /// Indexed record immediately following this path frame.
    following_record_index: u32,
    /// Following-record header byte offset.
    following_byte_offset: u64,
    /// Per-file dynamic following-record class tag.
    following_class_tag: String,
}

impl TryFrom<DesignConstructionOperandPathWire> for DesignConstructionOperandPath {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandPathWire) -> Result<Self, Self::Error> {
        Self::try_new(DesignConstructionOperandPathDraft {
            record_index: wire.record_index,
            byte_offset: wire.byte_offset,
            class_tag: wire.class_tag.try_into()?,
            entity_ref: wire.entity_ref,
            entity_ref_offset: wire.entity_ref_offset,
            placement: match (wire.transform, wire.transform_offset, wire.compact_variant) {
                (Some(value), Some(offset), None)
                    if offset.checked_sub(wire.entity_ref_offset) == Some(33 - SHORT_PATH_ENTITY_REFERENCE) =>
                {
                    DesignConstructionPathPlacement::Transform(value)
                }
                (None, None, Some(variant)) => DesignConstructionPathPlacement::Compact(variant),
                _ => return Err("transform and transform_offset must occur together and exclude compact_variant; compact_variant is required without transform".into()),
            },
            scope_record_index: wire.scope_record_index,
            scope_record_index_offset: wire.scope_record_index_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandPath> for DesignConstructionOperandPathWire {
    fn from(record: DesignConstructionOperandPath) -> Self {
        let record = record.into_draft();
        let (transform, transform_offset, compact_variant) = match record.placement {
            DesignConstructionPathPlacement::Transform(transform) => (
                Some(transform),
                Some(record.entity_ref_offset + 33 - SHORT_PATH_ENTITY_REFERENCE),
                None,
            ),
            DesignConstructionPathPlacement::Compact(variant) => (None, None, Some(variant)),
        };
        Self {
            record_index: record.record_index,
            byte_offset: record.byte_offset,
            class_tag: record.class_tag.into(),
            entity_ref: record.entity_ref,
            entity_ref_offset: record.entity_ref_offset,
            transform,
            transform_offset,
            compact_variant,
            scope_record_index: record.scope_record_index,
            scope_record_index_offset: record.scope_record_index_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            following_record_index: record.following_record_index,
            following_byte_offset: record.following_byte_offset,
            following_class_tag: record.following_class_tag.into(),
        }
    }
}

/// Placement layout carried by a persistent-entity selection path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) enum DesignConstructionPathPlacement {
    Transform(SketchPlacementMatrix),
    Compact(bool),
}

/// Nested identity chain named by a construction-operand group.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionOperandIdentityWire")]
pub(crate) struct DesignConstructionOperandIdentity {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning operand-group record.
    pub(crate) group_record_index: u32,
    /// Ordered identity-wrapper indexed records.
    wrappers: Vec<DesignIdentityWrapper>,
    /// Indexed identity of the record physically following the wrappers.
    following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    following_class_tag: DesignClassTag,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_IDENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionOperandIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            group_record_index: self.group_record_index,
            wrappers: self.wrappers.clone(),
            following_record_index: self.following_record_index,
            following_byte_offset: self.following_byte_offset,
            following_class_tag: self.following_class_tag.clone(),
            tracking_path: self.tracking_path.clone(),
            persistent_identity: self.persistent_identity.clone(),
        }
    }
}

impl Serialize for DesignConstructionOperandIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BorrowedWire<'a> {
            id: &'a str,
            group_record_index: u32,
            wrapper_record_indices: SliceColumn<'a, DesignIdentityWrapper, u32>,
            wrapper_byte_offsets: SliceColumn<'a, DesignIdentityWrapper, u64>,
            wrapper_class_tags: SliceColumn<'a, DesignIdentityWrapper, &'a str>,
            following_record_index: u32,
            following_byte_offset: u64,
            following_class_tag: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            tracking_path: Option<&'a DesignConstructionTrackingPath>,
            #[serde(skip_serializing_if = "Option::is_none")]
            persistent_identity: Option<&'a DesignConstructionPersistentIdentity>,
        }
        BorrowedWire {
            id: &self.id,
            group_record_index: self.group_record_index,
            wrapper_record_indices: SliceColumn::new(&self.wrappers, |row| row.record_index),
            wrapper_byte_offsets: SliceColumn::new(&self.wrappers, |row| row.byte_offset),
            wrapper_class_tags: SliceColumn::new(&self.wrappers, |row| row.class_tag.as_str()),
            following_record_index: self.following_record_index,
            following_byte_offset: self.following_byte_offset,
            following_class_tag: self.following_class_tag.as_str(),
            tracking_path: self.tracking_path.as_ref(),
            persistent_identity: self.persistent_identity.as_ref(),
        }
        .serialize(serializer)
    }
}

impl DesignConstructionOperandIdentity {
    pub(crate) fn try_new(draft: DesignConstructionOperandIdentityDraft) -> Result<Self, String> {
        let mut indices = std::collections::HashSet::new();
        if !draft
            .wrappers
            .iter()
            .all(|wrapper| indices.insert(wrapper.record_index))
            || !draft
                .wrappers
                .windows(2)
                .all(|pair| pair[0].byte_offset.checked_add(24) == Some(pair[1].byte_offset))
        {
            return Err("wrappers must have unique indices and stride-24 offsets".into());
        }
        if let Some(path) = &draft.tracking_path {
            if draft.wrappers.last().is_some_and(|wrapper| {
                wrapper.byte_offset.checked_add(24) != Some(path.wrapper_byte_offset())
            }) || draft.following_record_index != path.following_record_index()
                || draft.following_byte_offset != path.following_byte_offset()
                || draft.following_class_tag != path.following_class_tag
            {
                return Err("tracking_path disagrees with following record or wrappers".into());
            }
        } else if draft.wrappers.last().is_some_and(|wrapper| {
            wrapper.byte_offset.checked_add(24) != Some(draft.following_byte_offset)
        }) {
            return Err("following_byte_offset must follow wrappers by 24 bytes".into());
        }
        if draft
            .persistent_identity
            .as_ref()
            .is_some_and(|persistent| {
                draft.following_byte_offset.checked_add(21) != Some(persistent.local_id_offset())
            })
        {
            return Err(
                "persistent_identity.local_id_offset disagrees with following_byte_offset".into(),
            );
        }
        let value = Self {
            id: draft.id,
            group_record_index: draft.group_record_index,
            wrappers: draft.wrappers,
            following_record_index: draft.following_record_index,
            following_byte_offset: draft.following_byte_offset,
            following_class_tag: draft.following_class_tag,
            tracking_path: draft.tracking_path,
            persistent_identity: draft.persistent_identity,
        };
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignConstructionOperandIdentityDraft {
        DesignConstructionOperandIdentityDraft {
            id: self.id,
            group_record_index: self.group_record_index,
            wrappers: self.wrappers,
            following_record_index: self.following_record_index,
            following_byte_offset: self.following_byte_offset,
            following_class_tag: self.following_class_tag,
            tracking_path: self.tracking_path,
            persistent_identity: self.persistent_identity,
        }
    }
    pub(crate) fn wrappers(&self) -> &Vec<DesignIdentityWrapper> {
        &self.wrappers
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.following_record_index
    }
    pub(crate) fn following_byte_offset(&self) -> u64 {
        self.following_byte_offset
    }
    pub(crate) fn following_class_tag(&self) -> &DesignClassTag {
        &self.following_class_tag
    }
    pub(crate) fn tracking_path(&self) -> Option<&DesignConstructionTrackingPath> {
        self.tracking_path.as_ref()
    }
    pub(crate) fn persistent_identity(&self) -> Option<&DesignConstructionPersistentIdentity> {
        self.persistent_identity.as_ref()
    }
}

/// Unadmitted `DesignConstructionOperandIdentity` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionOperandIdentityDraft {
    /// Globally unique deterministic identifier.
    pub(crate) id: String,
    /// Owning operand-group record.
    pub(crate) group_record_index: u32,
    /// Ordered identity-wrapper indexed records.
    pub(crate) wrappers: Vec<DesignIdentityWrapper>,
    /// Indexed identity of the record physically following the wrappers.
    pub(crate) following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    pub(crate) following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    pub(crate) following_class_tag: DesignClassTag,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    pub(crate) tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    pub(crate) persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

/// Identity and location of one indexed construction wrapper.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignIdentityWrapper {
    pub(crate) record_index: u32,
    pub(crate) byte_offset: u64,
    pub(crate) class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionOperandIdentityWire {
    /// Globally unique deterministic identifier.
    id: String,
    /// Owning operand-group record.
    group_record_index: u32,
    /// Ordered identity-wrapper indexed-record identities.
    wrapper_record_indices: Vec<u32>,
    /// Indexed-header byte offsets parallel to `wrapper_record_indices`.
    wrapper_byte_offsets: Vec<u64>,
    /// Per-file dynamic class tags parallel to `wrapper_record_indices`.
    wrapper_class_tags: Vec<String>,
    /// Indexed identity of the record physically following the wrappers.
    following_record_index: u32,
    /// Indexed-header byte offset of the record following the wrappers.
    following_byte_offset: u64,
    /// Per-file dynamic class tag of the record following the wrappers.
    following_class_tag: String,
    /// Entity-tracking path between the outer wrappers and persistent identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tracking_path"
    )]
    tracking_path: Option<DesignConstructionTrackingPath>,
    /// Fixed-width persistent identity, when the following record has that grammar.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_persistent_identity"
    )]
    persistent_identity: Option<DesignConstructionPersistentIdentity>,
}

impl TryFrom<DesignConstructionOperandIdentityWire> for DesignConstructionOperandIdentity {
    type Error = String;
    fn try_from(wire: DesignConstructionOperandIdentityWire) -> Result<Self, Self::Error> {
        if wire.wrapper_record_indices.len() != wire.wrapper_byte_offsets.len()
            || wire.wrapper_record_indices.len() != wire.wrapper_class_tags.len()
        {
            return Err("wrapper_record_indices, wrapper_byte_offsets, and wrapper_class_tags must have equal lengths".into());
        }
        Self::try_new(DesignConstructionOperandIdentityDraft {
            id: wire.id,
            group_record_index: wire.group_record_index,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
            tracking_path: wire.tracking_path,
            persistent_identity: wire.persistent_identity,
            wrappers: wire
                .wrapper_record_indices
                .into_iter()
                .zip(wire.wrapper_byte_offsets)
                .zip(wire.wrapper_class_tags)
                .map(|((record_index, byte_offset), class_tag)| {
                    Ok(DesignIdentityWrapper {
                        record_index,
                        byte_offset,
                        class_tag: class_tag
                            .try_into()
                            .map_err(|error| format!("wrapper_class_tags: {error}"))?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionOperandIdentity> for DesignConstructionOperandIdentityWire {
    // Output cardinalities are bounded by already-materialized input vectors.
    #[allow(clippy::disallowed_methods)]
    fn from(identity: DesignConstructionOperandIdentity) -> Self {
        let identity = identity.into_draft();
        let mut wrapper_record_indices = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_byte_offsets = Vec::with_capacity(identity.wrappers.len());
        let mut wrapper_class_tags = Vec::with_capacity(identity.wrappers.len());
        for wrapper in identity.wrappers {
            wrapper_record_indices.push(wrapper.record_index);
            wrapper_byte_offsets.push(wrapper.byte_offset);
            wrapper_class_tags.push(wrapper.class_tag.into());
        }
        Self {
            id: identity.id,
            group_record_index: identity.group_record_index,
            following_record_index: identity.following_record_index,
            following_byte_offset: identity.following_byte_offset,
            following_class_tag: identity.following_class_tag.into(),
            tracking_path: identity.tracking_path,
            persistent_identity: identity.persistent_identity,
            wrapper_record_indices,
            wrapper_byte_offsets,
            wrapper_class_tags,
        }
    }
}

/// Entity-tracking path embedded in a construction-operand identity chain.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionTrackingPathWire")]
pub(crate) struct DesignConstructionTrackingPath {
    frame: crate::records::frame_chain::RecordFrameChain,
    /// Outer tracking-wrapper dynamic class tag.
    pub(crate) wrapper_class_tag: DesignClassTag,
    /// Nested tracking-carrier dynamic class tag.
    pub(crate) carrier_class_tag: DesignClassTag,
    /// Primary persistent identity stored by the carrier.
    pub(crate) primary_identity: u64,
    /// One-based fragment ordinal in the serialized primary curve direction.
    pub(crate) selector: i32,
    /// Positive fragment count defined by the serialized tracking graph.
    pub(crate) kind: u32,
    /// First optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_related_identity: Option<u64>,
    /// Second optional related persistent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_related_identity: Option<u64>,
    /// Following-record dynamic class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[cfg(test)]
thread_local! {
    static CONSTRUCTION_TRACKING_PATH_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionTrackingPath {
    fn clone(&self) -> Self {
        #[cfg(test)]
        CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            frame: self.frame,
            wrapper_class_tag: self.wrapper_class_tag.clone(),
            carrier_class_tag: self.carrier_class_tag.clone(),
            primary_identity: self.primary_identity,
            selector: self.selector,
            kind: self.kind,
            first_related_identity: self.first_related_identity,
            second_related_identity: self.second_related_identity,
            following_class_tag: self.following_class_tag.clone(),
        }
    }
}

impl Serialize for DesignConstructionTrackingPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            wrapper_record_index: u32,
            wrapper_byte_offset: u64,
            wrapper_class_tag: &'a str,
            carrier_record_index: u32,
            carrier_byte_offset: u64,
            carrier_class_tag: &'a str,
            primary_identity: u64,
            primary_identity_offset: u64,
            selector: i32,
            selector_offset: u64,
            kind: u32,
            kind_offset: u64,
            #[serde(skip_serializing_if = "Option::is_none")]
            first_related_identity: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            first_related_identity_offset: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            second_related_identity: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            second_related_identity_offset: Option<u64>,
            following_record_index: u32,
            following_byte_offset: u64,
            following_class_tag: &'a str,
        }
        WireRef {
            wrapper_record_index: self.wrapper_record_index(),
            wrapper_byte_offset: self.wrapper_byte_offset(),
            wrapper_class_tag: self.wrapper_class_tag.as_str(),
            carrier_record_index: self.carrier_record_index(),
            carrier_byte_offset: self.carrier_byte_offset(),
            carrier_class_tag: self.carrier_class_tag.as_str(),
            primary_identity: self.primary_identity,
            primary_identity_offset: self.primary_identity_offset(),
            selector: self.selector,
            selector_offset: self.selector_offset(),
            kind: self.kind,
            kind_offset: self.kind_offset(),
            first_related_identity: self.first_related_identity().map(|value| value.value),
            first_related_identity_offset: self.first_related_identity().map(|value| value.offset),
            second_related_identity: self.second_related_identity().map(|value| value.value),
            second_related_identity_offset: self
                .second_related_identity()
                .map(|value| value.offset),
            following_record_index: self.following_record_index(),
            following_byte_offset: self.following_byte_offset(),
            following_class_tag: self.following_class_tag.as_str(),
        }
        .serialize(serializer)
    }
}

impl DesignConstructionTrackingPath {
    pub(crate) fn try_new(draft: DesignConstructionTrackingPathDraft) -> Result<Self, String> {
        if draft.kind == 0 {
            return Err("tracking kind must be a positive fragment count".into());
        }
        if !u32::try_from(draft.selector).is_ok_and(|ordinal| (1..=draft.kind).contains(&ordinal)) {
            return Err(
                "tracking selector must be a fragment ordinal from one through kind".into(),
            );
        }
        if draft.first_related_identity.is_some_and(|identity| {
            draft.wrapper_byte_offset.checked_add(110) != Some(identity.offset)
        }) || draft.second_related_identity.is_some_and(|identity| {
            draft
                .wrapper_byte_offset
                .checked_add(114 + u64::from(draft.first_related_identity.is_some()) * 8)
                != Some(identity.offset)
        }) {
            return Err("related_identity_offset disagrees with tracking frame".into());
        }
        let frame = crate::records::frame_chain::RecordFrameChain::try_new(
            draft.wrapper_record_index,
            draft.wrapper_byte_offset,
            2,
            114 + u64::from(draft.first_related_identity.is_some()) * 8
                + u64::from(draft.second_related_identity.is_some()) * 8,
        )?;
        let value = Self {
            frame,
            wrapper_class_tag: draft.wrapper_class_tag,
            carrier_class_tag: draft.carrier_class_tag,
            primary_identity: draft.primary_identity,
            selector: draft.selector,
            kind: draft.kind,
            first_related_identity: draft.first_related_identity.map(|identity| identity.value),
            second_related_identity: draft.second_related_identity.map(|identity| identity.value),
            following_class_tag: draft.following_class_tag,
        };
        if value.carrier_record_index() != draft.carrier_record_index {
            return Err("carrier_record_index disagrees with frame layout".into());
        }
        if value.carrier_byte_offset() != draft.carrier_byte_offset {
            return Err("carrier_byte_offset disagrees with frame layout".into());
        }
        if value.primary_identity_offset() != draft.primary_identity_offset {
            return Err("primary_identity_offset disagrees with frame layout".into());
        }
        if value.selector_offset() != draft.selector_offset {
            return Err("selector_offset disagrees with frame layout".into());
        }
        if value.kind_offset() != draft.kind_offset {
            return Err("kind_offset disagrees with frame layout".into());
        }
        if value.following_record_index() != draft.following_record_index {
            return Err("following_record_index disagrees with frame layout".into());
        }
        if value.following_byte_offset() != draft.following_byte_offset {
            return Err("following_byte_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    fn into_draft(self) -> DesignConstructionTrackingPathDraft {
        let first_related_identity = self.first_related_identity();
        let second_related_identity = self.second_related_identity();
        let wrapper_record_index = self.wrapper_record_index();
        let wrapper_byte_offset = self.wrapper_byte_offset();
        let carrier_record_index = self.carrier_record_index();
        let carrier_byte_offset = self.carrier_byte_offset();
        let primary_identity_offset = self.primary_identity_offset();
        let selector_offset = self.selector_offset();
        let kind_offset = self.kind_offset();
        let following_record_index = self.following_record_index();
        let following_byte_offset = self.following_byte_offset();
        DesignConstructionTrackingPathDraft {
            wrapper_record_index,
            wrapper_byte_offset,
            wrapper_class_tag: self.wrapper_class_tag,
            carrier_record_index,
            carrier_byte_offset,
            carrier_class_tag: self.carrier_class_tag,
            primary_identity: self.primary_identity,
            primary_identity_offset,
            selector: self.selector,
            selector_offset,
            kind: self.kind,
            kind_offset,
            first_related_identity,
            second_related_identity,
            following_record_index,
            following_byte_offset,
            following_class_tag: self.following_class_tag,
        }
    }
    pub(crate) fn wrapper_record_index(&self) -> u32 {
        self.frame.index(0)
    }
    pub(crate) fn wrapper_byte_offset(&self) -> u64 {
        self.frame.offset(0)
    }
    pub(crate) fn carrier_record_index(&self) -> u32 {
        self.frame.index(1)
    }
    pub(crate) fn carrier_byte_offset(&self) -> u64 {
        self.frame.offset(33)
    }
    pub(crate) fn primary_identity_offset(&self) -> u64 {
        self.frame.offset(70)
    }
    fn selector_offset(&self) -> u64 {
        self.frame.offset(90)
    }
    fn kind_offset(&self) -> u64 {
        self.frame.offset(94)
    }
    pub(crate) fn first_related_identity(&self) -> Option<Located<u64>> {
        self.first_related_identity.map(|value| Located {
            value,
            offset: self.frame.offset(110),
        })
    }
    pub(crate) fn second_related_identity(&self) -> Option<Located<u64>> {
        self.second_related_identity.map(|value| Located {
            value,
            offset: self
                .frame
                .offset(114 + u64::from(self.first_related_identity.is_some()) * 8),
        })
    }
    pub(crate) fn following_record_index(&self) -> u32 {
        self.frame.index(2)
    }
    pub(crate) fn following_byte_offset(&self) -> u64 {
        self.frame.offset(
            114 + u64::from(self.first_related_identity.is_some()) * 8
                + u64::from(self.second_related_identity.is_some()) * 8,
        )
    }
}

/// Unadmitted `DesignConstructionTrackingPath` fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignConstructionTrackingPathDraft {
    /// Outer tracking-wrapper record identity.
    pub(crate) wrapper_record_index: u32,
    /// Outer tracking-wrapper header byte offset.
    pub(crate) wrapper_byte_offset: u64,
    /// Outer tracking-wrapper dynamic class tag.
    pub(crate) wrapper_class_tag: DesignClassTag,
    /// Nested tracking-carrier record identity.
    pub(crate) carrier_record_index: u32,
    /// Nested tracking-carrier header byte offset.
    pub(crate) carrier_byte_offset: u64,
    /// Nested tracking-carrier dynamic class tag.
    pub(crate) carrier_class_tag: DesignClassTag,
    /// Primary persistent identity stored by the carrier.
    pub(crate) primary_identity: u64,
    /// Byte offset of `primary_identity`.
    pub(crate) primary_identity_offset: u64,
    /// One-based fragment ordinal in the serialized primary curve direction.
    pub(crate) selector: i32,
    /// Byte offset of `selector`.
    pub(crate) selector_offset: u64,
    /// Positive fragment count defined by the serialized tracking graph.
    pub(crate) kind: u32,
    /// Byte offset of `kind`.
    pub(crate) kind_offset: u64,
    /// First optional related persistent identity.
    pub(crate) first_related_identity: Option<Located<u64>>,
    /// Second optional related persistent identity.
    pub(crate) second_related_identity: Option<Located<u64>>,
    /// Indexed record immediately following the carrier.
    pub(crate) following_record_index: u32,
    /// Following-record header byte offset.
    pub(crate) following_byte_offset: u64,
    /// Following-record dynamic class tag.
    pub(crate) following_class_tag: DesignClassTag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignConstructionTrackingPathWire {
    wrapper_record_index: u32,
    wrapper_byte_offset: u64,
    wrapper_class_tag: String,
    carrier_record_index: u32,
    carrier_byte_offset: u64,
    carrier_class_tag: String,
    primary_identity: u64,
    primary_identity_offset: u64,
    selector: i32,
    selector_offset: u64,
    kind: u32,
    kind_offset: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_related_identity"
    )]
    first_related_identity: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_first_related_identity_offset"
    )]
    first_related_identity_offset: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_related_identity"
    )]
    second_related_identity: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_related_identity_offset"
    )]
    second_related_identity_offset: Option<u64>,
    following_record_index: u32,
    following_byte_offset: u64,
    following_class_tag: String,
}

impl TryFrom<DesignConstructionTrackingPathWire> for DesignConstructionTrackingPath {
    type Error = String;
    fn try_from(wire: DesignConstructionTrackingPathWire) -> Result<Self, Self::Error> {
        Self::try_new(DesignConstructionTrackingPathDraft {
            wrapper_record_index: wire.wrapper_record_index,
            wrapper_byte_offset: wire.wrapper_byte_offset,
            wrapper_class_tag: wire.wrapper_class_tag.try_into()?,
            carrier_record_index: wire.carrier_record_index,
            carrier_byte_offset: wire.carrier_byte_offset,
            carrier_class_tag: wire.carrier_class_tag.try_into()?,
            primary_identity: wire.primary_identity,
            primary_identity_offset: wire.primary_identity_offset,
            selector: wire.selector,
            selector_offset: wire.selector_offset,
            kind: wire.kind,
            kind_offset: wire.kind_offset,
            first_related_identity: Located::from_wire(
                wire.first_related_identity,
                wire.first_related_identity_offset,
                "first_related_identity",
            )?,
            second_related_identity: Located::from_wire(
                wire.second_related_identity,
                wire.second_related_identity_offset,
                "second_related_identity",
            )?,
            following_record_index: wire.following_record_index,
            following_byte_offset: wire.following_byte_offset,
            following_class_tag: wire.following_class_tag.try_into()?,
        })
    }
}

#[cfg(test)]
impl From<DesignConstructionTrackingPath> for DesignConstructionTrackingPathWire {
    fn from(value: DesignConstructionTrackingPath) -> Self {
        let value = value.into_draft();
        Self {
            wrapper_record_index: value.wrapper_record_index,
            wrapper_byte_offset: value.wrapper_byte_offset,
            wrapper_class_tag: value.wrapper_class_tag.into(),
            carrier_record_index: value.carrier_record_index,
            carrier_byte_offset: value.carrier_byte_offset,
            carrier_class_tag: value.carrier_class_tag.into(),
            primary_identity: value.primary_identity,
            primary_identity_offset: value.primary_identity_offset,
            selector: value.selector,
            selector_offset: value.selector_offset,
            kind: value.kind,
            kind_offset: value.kind_offset,
            first_related_identity: value.first_related_identity.map(|located| located.value),
            first_related_identity_offset: value
                .first_related_identity
                .map(|located| located.offset),
            second_related_identity: value.second_related_identity.map(|located| located.value),
            second_related_identity_offset: value
                .second_related_identity
                .map(|located| located.offset),
            following_record_index: value.following_record_index,
            following_byte_offset: value.following_byte_offset,
            following_class_tag: value.following_class_tag.into(),
        }
    }
}

/// Fixed-width persistent identity following a construction-operand identity chain.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignConstructionPersistentIdentityDraft")]
pub(crate) struct DesignConstructionPersistentIdentity {
    tail: PersistentIdentityTail,
    /// Local persistent identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Byte offset of `local_id`.
    local_id_offset: u64,
    /// Asset UUID qualifying the local identity.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// UUID of the local identity context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    tail_slot_present: bool,
    /// Identity of the indexed record immediately following this identity.
    pub(crate) next_record_index: u32,
}

#[cfg(test)]
thread_local! {
    pub(super) static PERSISTENT_IDENTITY_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignConstructionPersistentIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        PERSISTENT_IDENTITY_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            tail: self.tail,
            local_id: self.local_id,
            local_id_offset: self.local_id_offset,
            asset_id: self.asset_id.clone(),
            context_id: self.context_id.clone(),
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            next_record_index: self.next_record_index,
        }
    }
}

impl Serialize for DesignConstructionPersistentIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            local_id: u64,
            local_id_offset: u64,
            asset_id: &'a str,
            asset_id_offset: u64,
            context_id: &'a str,
            context_id_offset: u64,
            tail_slot_present: bool,
            tail_slot_offset: u64,
            next_record_index: u32,
            next_byte_offset: u64,
        }
        WireRef {
            local_id: self.local_id,
            local_id_offset: self.local_id_offset,
            asset_id: self.asset_id.as_str(),
            asset_id_offset: self.asset_id_offset(),
            context_id: self.context_id.as_str(),
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            tail_slot_offset: self.tail_slot_offset(),
            next_record_index: self.next_record_index,
            next_byte_offset: self.next_byte_offset(),
        }
        .serialize(serializer)
    }
}

impl DesignConstructionPersistentIdentity {
    pub(crate) fn try_new(
        draft: DesignConstructionPersistentIdentityDraft,
    ) -> Result<Self, String> {
        let base = draft
            .local_id_offset
            .checked_sub(21)
            .ok_or("local_id_offset precedes identity header")?;
        if draft.local_id_offset.checked_add(12) != Some(draft.asset_id_offset)
            || draft.context_id_offset <= draft.asset_id_offset
            || !(base.checked_add(190) == Some(draft.next_byte_offset)
                || (base.checked_add(185) == Some(draft.tail_slot_offset)
                    && draft.tail_slot_offset.checked_add(15) == Some(draft.next_byte_offset)))
        {
            return Err("persistent identity offsets disagree with frame layout".into());
        }
        let tail = if base.checked_add(190) == Some(draft.next_byte_offset) {
            PersistentIdentityTail::Fixed {
                tail_slot_offset: draft.tail_slot_offset,
            }
        } else {
            PersistentIdentityTail::Extended
        };
        let value = Self {
            tail,
            local_id: draft.local_id,
            local_id_offset: draft.local_id_offset,
            asset_id: draft.asset_id,
            context_id: draft.context_id,
            context_id_offset: draft.context_id_offset,
            tail_slot_present: draft.tail_slot_present,
            next_record_index: draft.next_record_index,
        };
        if value.asset_id_offset() != draft.asset_id_offset {
            return Err("asset_id_offset disagrees with frame layout".into());
        }
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn into_draft(self) -> DesignConstructionPersistentIdentityDraft {
        let tail_slot_offset = self.tail_slot_offset();
        let next_byte_offset = self.next_byte_offset();
        let asset_id_offset = self.asset_id_offset();
        DesignConstructionPersistentIdentityDraft {
            local_id: self.local_id,
            local_id_offset: self.local_id_offset,
            asset_id: self.asset_id,
            asset_id_offset,
            context_id: self.context_id,
            context_id_offset: self.context_id_offset,
            tail_slot_present: self.tail_slot_present,
            tail_slot_offset,
            next_record_index: self.next_record_index,
            next_byte_offset,
        }
    }
    pub(crate) fn local_id_offset(&self) -> u64 {
        self.local_id_offset
    }
    pub(crate) fn asset_id_offset(&self) -> u64 {
        self.local_id_offset + 12
    }
    pub(crate) fn context_id_offset(&self) -> u64 {
        self.context_id_offset
    }
    pub(crate) fn tail_slot_offset(&self) -> u64 {
        match self.tail {
            PersistentIdentityTail::Fixed { tail_slot_offset } => tail_slot_offset,
            PersistentIdentityTail::Extended => self.local_id_offset + 164,
        }
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.local_id_offset
            + match self.tail {
                PersistentIdentityTail::Fixed { .. } => 169,
                PersistentIdentityTail::Extended => 179,
            }
    }
}

/// Unadmitted `DesignConstructionPersistentIdentity` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignConstructionPersistentIdentityDraft {
    /// Local persistent identity preceding the two UUID fields.
    pub(crate) local_id: u64,
    /// Byte offset of `local_id`.
    pub(crate) local_id_offset: u64,
    /// Asset UUID qualifying the local identity.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE code units.
    pub(crate) asset_id_offset: u64,
    /// UUID of the local identity context.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE code units.
    pub(crate) context_id_offset: u64,
    /// Whether the fixed tail's optional slot is present.
    #[serde(default)]
    pub(crate) tail_slot_present: bool,
    /// Byte offset of the optional-slot marker.
    #[serde(default)]
    pub(crate) tail_slot_offset: u64,
    /// Identity of the indexed record immediately following this identity.
    pub(crate) next_record_index: u32,
    /// Byte offset of the indexed record immediately following this identity.
    pub(crate) next_byte_offset: u64,
}

impl TryFrom<DesignConstructionPersistentIdentityDraft> for DesignConstructionPersistentIdentity {
    type Error = String;
    fn try_from(draft: DesignConstructionPersistentIdentityDraft) -> Result<Self, String> {
        Self::try_new(draft)
    }
}

#[cfg(test)]
impl From<DesignConstructionPersistentIdentity> for DesignConstructionPersistentIdentityDraft {
    fn from(value: DesignConstructionPersistentIdentity) -> Self {
        let value = value.into_draft();
        Self {
            local_id: value.local_id,
            local_id_offset: value.local_id_offset,
            asset_id: value.asset_id,
            asset_id_offset: value.asset_id_offset,
            context_id: value.context_id,
            context_id_offset: value.context_id_offset,
            tail_slot_present: value.tail_slot_present,
            tail_slot_offset: value.tail_slot_offset,
            next_record_index: value.next_record_index,
            next_byte_offset: value.next_byte_offset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistentIdentityTail {
    Fixed { tail_slot_offset: u64 },
    Extended,
}

cadmpeg_core::named_optional_field!(
    deserialize_extrude_role,
    DesignExtrudeOperandRoleTag,
    "extrude_role"
);

cadmpeg_core::named_optional_field!(
    deserialize_extrude_face_role,
    DesignExtrudeFaceRole,
    "extrude_face_role"
);

cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");

cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");

cadmpeg_core::named_optional_field!(deserialize_compact_variant, bool, "compact_variant");

cadmpeg_core::named_optional_field!(
    deserialize_tracking_path,
    DesignConstructionTrackingPath,
    "tracking_path"
);

cadmpeg_core::named_optional_field!(
    deserialize_persistent_identity,
    DesignConstructionPersistentIdentity,
    "persistent_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_first_related_identity,
    u64,
    "first_related_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_first_related_identity_offset,
    u64,
    "first_related_identity_offset"
);

cadmpeg_core::named_optional_field!(
    deserialize_second_related_identity,
    u64,
    "second_related_identity"
);

cadmpeg_core::named_optional_field!(
    deserialize_second_related_identity_offset,
    u64,
    "second_related_identity_offset"
);

#[cfg(test)]
mod tests;
