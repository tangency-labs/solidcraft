// SPDX-License-Identifier: Apache-2.0

use super::combine::DesignExternalVersion;
use super::hole::DesignHoleConstruction;
use super::work_geometry::DesignWorkPointConstruction;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::identity::Located;
use crate::records::mesh::{DesignOccurrenceKey, DesignRelaxedGuidText};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::FiniteReal;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Deserialize, Serialize};

/// Domain of the two scalar limits carried by a legacy As-built scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignAssemblyLimitKind {
    /// Limits on the joint's angular degree of freedom.
    #[default]
    Angular,
    /// Limits on the joint's linear degree of freedom.
    Linear,
}

/// Ordered lower and upper limits carried by a legacy As-built assembly scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignAssemblyLimitsWire")]
pub(crate) struct DesignAssemblyLimits {
    /// Degree-of-freedom domain of the limits.
    #[serde(default)]
    pub(crate) kind: DesignAssemblyLimitKind,
    /// Lower bound in the domain's native units.
    minimum: f64,
    /// Upper bound in the domain's native units.
    maximum: f64,
    /// Parameter-owner records for the lower and upper bounds.
    pub(crate) owner_record_indices: [u32; 2],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 2],
}

/// Wire fields for finite ordered assembly limits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblyLimitsWire {
    /// Degree-of-freedom domain of the limits.
    #[serde(default)]
    pub(crate) kind: DesignAssemblyLimitKind,
    /// Lower bound in the domain's native units.
    pub(crate) minimum: f64,
    /// Upper bound in the domain's native units.
    pub(crate) maximum: f64,
    /// Parameter-owner records for the lower and upper bounds.
    pub(crate) owner_record_indices: [u32; 2],
    /// Evaluated-value offsets parallel to `owner_record_indices`.
    pub(crate) value_offsets: [u64; 2],
}

impl DesignAssemblyLimits {
    pub(crate) fn minimum(&self) -> f64 {
        self.minimum
    }
    pub(crate) fn maximum(&self) -> f64 {
        self.maximum
    }
}

impl TryFrom<DesignAssemblyLimitsWire> for DesignAssemblyLimits {
    type Error = String;
    fn try_from(wire: DesignAssemblyLimitsWire) -> Result<Self, Self::Error> {
        if !wire.minimum.is_finite() || !wire.maximum.is_finite() || wire.minimum > wire.maximum {
            return Err("assembly limits minimum and maximum must be finite and ordered".into());
        }
        Ok(Self {
            kind: wire.kind,
            minimum: wire.minimum,
            maximum: wire.maximum,
            owner_record_indices: wire.owner_record_indices,
            value_offsets: wire.value_offsets,
        })
    }
}
/// Exact solved frame carried by a legacy 421-byte `As-built` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblySolvedFrame {
    /// Frame-carrier record named by reference-table entry eight.
    pub(crate) reference_record_index: u32,
    /// Byte offset of the frame-carrier reference in the scope.
    pub(crate) reference_offset: u64,
    /// Byte offset of the frame-carrier indexed header.
    pub(crate) record_byte_offset: u64,
    /// Dynamic class of the frame-carrier indexed record.
    pub(crate) class_tag: DesignClassTag,
    /// Row-major solved connector frame.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
}

/// A construction and its face selection in a legacy assembly operand.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignAssemblyLegacyOperand<C> {
    pub(crate) construction_class_tag: DesignClassTag,
    pub(crate) construction: C,
    pub(crate) selection: DesignAssemblyLegacySelection,
    pub(crate) reference_offset: u64,
}

/// The ordered point and hole constructions of a legacy 421-byte assembly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignAssemblyLegacyOperands {
    point: DesignAssemblyLegacyOperand<Box<DesignWorkPointConstruction>>,
    hole: DesignAssemblyLegacyOperand<Box<DesignHoleConstruction>>,
}

impl DesignAssemblyLegacyOperands {
    /// Pair the point and hole operand carriers. Their solved positions are
    /// finite reals.
    pub(crate) fn new(
        point: DesignAssemblyLegacyOperand<Box<DesignWorkPointConstruction>>,
        hole: DesignAssemblyLegacyOperand<Box<DesignHoleConstruction>>,
    ) -> Self {
        Self { point, hole }
    }

    pub(crate) fn references(&self) -> [Located<u32>; 2] {
        [
            Located {
                value: self.point.construction.point_record_index,
                offset: self.point.reference_offset,
            },
            Located {
                value: self.hole.construction.point_record_index,
                offset: self.hole.reference_offset,
            },
        ]
    }

    pub(crate) fn selections(&self) -> [&DesignAssemblyLegacySelection; 2] {
        [&self.point.selection, &self.hole.selection]
    }

    pub(crate) fn frames(
        &self,
        solved: &DesignAssemblySolvedFrame,
    ) -> Result<[DesignAssemblyOperandFrame; 2], String> {
        let references = self.references();
        let positions = [
            self.point.construction.position,
            self.hole.construction.position,
        ];
        let frames: [Result<DesignAssemblyOperandFrame, String>; 2] = [0, 1].map(|index| {
            let mut transform = solved.transform.rows();
            for (row, value) in positions[index].into_iter().enumerate() {
                transform[row][3] = value.get();
            }
            Ok(DesignAssemblyOperandFrame {
                reference_record_index: references[index].value,
                reference_offset: references[index].offset,
                transform: transform.try_into()?,
                transform_offset: solved.transform_offset,
            })
        });
        let [first, second] = frames;
        Ok([first?, second?])
    }

    fn from_wire(
        wire: [DesignAssemblyLegacyOperandWire; 2],
        solved: &DesignAssemblySolvedFrame,
    ) -> Result<Self, String> {
        let [point, hole] = wire;
        let DesignAssemblyLegacyConstruction::Point(point_construction) = point.construction else {
            return Err("legacy_operand_carriers[0].construction requires a point".into());
        };
        let DesignAssemblyLegacyConstruction::Hole(hole_construction) = hole.construction else {
            return Err("legacy_operand_carriers[1].construction requires a hole".into());
        };
        if point.construction_record_index != point_construction.point_record_index
            || hole.construction_record_index != hole_construction.point_record_index
        {
            return Err(
                "legacy_operand_carriers construction_record_index disagrees with construction"
                    .into(),
            );
        }
        if point.construction_byte_offset != point_construction.point_record_byte_offset
            || hole.construction_byte_offset != hole_construction.point_record_byte_offset
        {
            return Err(
                "legacy_operand_carriers construction_byte_offset disagrees with construction"
                    .into(),
            );
        }
        let carriers = Self::new(
            DesignAssemblyLegacyOperand {
                construction_class_tag: point
                    .construction_class_tag
                    .try_into()
                    .map_err(|error| format!("construction_class_tag: {error}"))?,
                construction: point_construction,
                selection: point.selection,
                reference_offset: point.frame.reference_offset,
            },
            DesignAssemblyLegacyOperand {
                construction_class_tag: hole
                    .construction_class_tag
                    .try_into()
                    .map_err(|error| format!("construction_class_tag: {error}"))?,
                construction: hole_construction,
                selection: hole.selection,
                reference_offset: hole.frame.reference_offset,
            },
        );
        if [point.frame, hole.frame] != carriers.frames(solved)? {
            return Err(
                "legacy_operand_carriers frame disagrees with construction and solved_frame".into(),
            );
        }
        Ok(carriers)
    }
}

/// Exact construction and face-selection pair carried by a legacy 421-byte
/// `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyLegacyOperandWire {
    /// Primary construction record named by the scope reference table.
    construction_record_index: u32,
    /// Byte offset of the primary construction record header.
    construction_byte_offset: u64,
    /// Dynamic class of the primary construction record.
    construction_class_tag: String,
    /// Exact point or point-and-direction construction.
    construction: DesignAssemblyLegacyConstruction,
    /// Face-selection record paired with the construction record.
    selection: DesignAssemblyLegacySelection,
    /// Connector-local frame derived from the stored solved-frame orientation
    /// and this operand's construction position.
    frame: DesignAssemblyOperandFrame,
}

/// Construction carrier family used by a legacy 421-byte `As-built` operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum DesignAssemblyLegacyConstruction {
    /// Point-only connector construction.
    Point(Box<DesignWorkPointConstruction>),
    /// Point-and-direction connector construction.
    Hole(Box<DesignHoleConstruction>),
}

/// Exact face-recipe selection paired with a legacy 421-byte construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblyLegacySelection {
    /// Indexed selection record.
    pub(crate) record_index: u32,
    /// Byte offset of the selection record header.
    pub(crate) byte_offset: u64,
    /// Dynamic class of the selection record.
    pub(crate) class_tag: DesignClassTag,
    /// Asset UUID qualifying the selection namespace.
    pub(crate) asset_id: DesignRelaxedGuidText,
    /// Byte offset of the asset UUID's UTF-16LE payload.
    pub(crate) asset_id_offset: u64,
    /// Context UUID qualifying the selection.
    pub(crate) context_id: DesignRelaxedGuidText,
    /// Byte offset of the context UUID's UTF-16LE payload.
    pub(crate) context_id_offset: u64,
    /// Indexed record containing the face recipe.
    pub(crate) recipe_record_index: u32,
    /// Byte offset of the recipe record's indexed header.
    pub(crate) recipe_record_byte_offset: u64,
    /// Construction-recipe arena id.
    pub(crate) recipe_id: String,
    /// Exact face-recipe family.
    pub(crate) recipe_kind: ConstructionRecipeKind,
    /// Persistent selector/reference tails carried by the recipe prefix.
    pub(crate) recipe_references: Vec<DesignRecipeReference>,
    /// Byte offset of the indexed record immediately after the recipe.
    pub(crate) next_byte_offset: u64,
}

/// Alignment scalars carried by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DesignAssemblyAlignmentSerde")]
pub(crate) struct DesignAssemblyAlignment {
    /// Signed alignment rotation in radians.
    angle: FiniteReal,
    /// Signed local-frame translation in source centimetres.
    offset: [FiniteReal; 3],
    /// Parameter-owner records and their evaluated-value locations.
    pub(crate) owners: Vec<Located<u32>>,
    /// Datum, legacy solved-carrier, or qualified-operand form.
    pub(crate) form: Option<DesignAssemblyAlignmentForm>,
    /// How the two operand frames mate, when the scope stores it.
    pub(crate) frame_sense: Option<DesignJointFrameSense>,
    pub(crate) additional_owner_run: Option<DesignAssemblyOwnerRun>,
    /// Members of a rigid group after the two in `form`, in scope order.
    pub(crate) additional_members: Vec<DesignQualifiedAssemblyOperand>,
}

/// Counted owner references before a placement-ordered scope's locator run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblyOwnerRun {
    pub(crate) count: Located<u32>,
    pub(crate) references: Vec<Located<u32>>,
}

/// How a joint's two operand frames mate: with their z axes opposed or
/// aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignJointFrameSense {
    /// `z₁ = −z₀`.
    Opposed,
    /// `z₁ = z₀`.
    Aligned,
}

/// One assembly operand frame and its source qualifier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignQualifiedAssemblyOperand {
    pub(crate) frame: DesignAssemblyOperandFrame,
    pub(crate) qualifier: DesignAssemblyOperandQualifier,
}

/// Native evidence retained by an assembly-alignment form.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignAssemblyAlignmentForm {
    DatumEnvelope {
        joint_origin_scope_record_index: u32,
    },
    LimitsOnly {
        limits: DesignAssemblyLimits,
    },
    SolvedOnly {
        solved_frame: DesignAssemblySolvedFrame,
        limits: Option<DesignAssemblyLimits>,
    },
    LegacyAsBuilt421 {
        carriers: DesignAssemblyLegacyOperands,
        solved_frame: DesignAssemblySolvedFrame,
        limits: Option<DesignAssemblyLimits>,
        /// Preserve omission of the redundant frame field in older native records.
        frames_field_present: bool,
    },
    Frames {
        frames: [DesignAssemblyOperandFrame; 2],
    },
    Qualified([DesignQualifiedAssemblyOperand; 2]),
}

impl DesignAssemblyAlignmentForm {
    pub(crate) fn qualified(
        frames: [DesignAssemblyOperandFrame; 2],
        qualifiers: [DesignAssemblyOperandQualifier; 2],
    ) -> Self {
        let [first_frame, second_frame] = frames;
        let [first_qualifier, second_qualifier] = qualifiers;
        Self::Qualified([
            super::assembly::DesignQualifiedAssemblyOperand {
                frame: first_frame,
                qualifier: first_qualifier,
            },
            super::assembly::DesignQualifiedAssemblyOperand {
                frame: second_frame,
                qualifier: second_qualifier,
            },
        ])
    }
}

impl DesignAssemblyAlignment {
    pub(crate) fn try_new(
        angle: f64,
        offset: [f64; 3],
        owners: Vec<Located<u32>>,
        form: Option<DesignAssemblyAlignmentForm>,
    ) -> Result<Self, String> {
        let angle = FiniteReal::new(angle)
            .ok_or_else(|| "assembly alignment angle must be finite".to_owned())?;
        let [Some(first), Some(second), Some(third)] = offset.map(FiniteReal::new) else {
            return Err("assembly alignment offset must be finite".into());
        };
        let offset = [first, second, third];
        Ok(Self {
            angle,
            offset,
            owners,
            form,
            frame_sense: None,
            additional_owner_run: None,
            additional_members: Vec::new(),
        })
    }
    /// This alignment with the rigid-group members after its first two.
    pub(crate) fn with_additional_members(
        mut self,
        members: Vec<DesignQualifiedAssemblyOperand>,
    ) -> Result<Self, String> {
        if !members.is_empty()
            && !matches!(self.form, Some(DesignAssemblyAlignmentForm::Qualified(_)))
        {
            return Err("additional rigid-group members require qualified operands".into());
        }
        self.additional_members = members;
        Ok(self)
    }
    /// This alignment with the mate sense its scope stores.
    pub(crate) fn with_frame_sense(mut self, sense: Option<DesignJointFrameSense>) -> Self {
        self.frame_sense = sense;
        self
    }
    pub(crate) fn angle(&self) -> f64 {
        self.angle.get()
    }
    pub(crate) fn offset(&self) -> [f64; 3] {
        self.offset.map(FiniteReal::get)
    }

    pub(crate) fn operand_frames(&self) -> Option<[DesignAssemblyOperandFrame; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Frames { frames } => Some(frames.clone()),
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                Some(operands.each_ref().map(|operand| operand.frame.clone()))
            }
            DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                ..
            } => carriers.frames(solved_frame).ok(),
            DesignAssemblyAlignmentForm::DatumEnvelope { .. }
            | DesignAssemblyAlignmentForm::LimitsOnly { .. }
            | DesignAssemblyAlignmentForm::SolvedOnly { .. } => None,
        }
    }

    pub(crate) fn solved_frame(&self) -> Option<&DesignAssemblySolvedFrame> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, .. }
            | DesignAssemblyAlignmentForm::LegacyAsBuilt421 { solved_frame, .. } => {
                Some(solved_frame)
            }
            _ => None,
        }
    }

    pub(crate) fn operand_qualifiers(&self) -> Option<[DesignAssemblyOperandQualifier; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                Some(operands.each_ref().map(|operand| operand.qualifier.clone()))
            }
            _ => None,
        }
    }

    /// Return both occurrence paths when every operand uses that qualifier form.
    #[cfg(test)]
    pub(crate) fn operand_paths(&self) -> Option<[DesignAssemblyOperandPath; 2]> {
        self.operand_path_refs()
            .map(|paths| paths.map(Clone::clone))
    }

    /// Borrow both occurrence paths when every operand uses that qualifier form.
    pub(crate) fn operand_path_refs(&self) -> Option<[&DesignAssemblyOperandPath; 2]> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::Qualified(operands) => {
                let [Some(first), Some(second)] = operands
                    .each_ref()
                    .map(|operand| operand.qualifier.occurrence_path())
                else {
                    return None;
                };
                Some([first, second])
            }
            _ => None,
        }
    }

    pub(crate) fn limits(&self) -> Option<&DesignAssemblyLimits> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::LimitsOnly { limits } => Some(limits),
            DesignAssemblyAlignmentForm::SolvedOnly { limits, .. }
            | DesignAssemblyAlignmentForm::LegacyAsBuilt421 { limits, .. } => limits.as_ref(),
            _ => None,
        }
    }

    pub(crate) fn joint_origin_scope_record_index(&self) -> Option<u32> {
        match self.form.as_ref()? {
            DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index,
            } => Some(*joint_origin_scope_record_index),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyAlignmentSerde {
    angle: f64,
    offset: [f64; 3],
    owner_record_indices: Vec<u32>,
    value_offsets: Vec<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operand_frames"
    )]
    operand_frames: Option<[DesignAssemblyOperandFrame; 2]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_legacy_operand_carriers"
    )]
    legacy_operand_carriers: Option<[DesignAssemblyLegacyOperandWire; 2]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solved_frame"
    )]
    solved_frame: Option<DesignAssemblySolvedFrame>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_operand_qualifiers"
    )]
    operand_qualifiers: Option<[DesignAssemblyOperandQualifier; 2]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_limits"
    )]
    #[serde(alias = "angular_limits")]
    limits: Option<DesignAssemblyLimits>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_joint_origin_scope_record_index"
    )]
    joint_origin_scope_record_index: Option<u32>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_frame_sense"
    )]
    frame_sense: Option<DesignJointFrameSense>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_additional_owner_run"
    )]
    additional_owner_run: Option<DesignAssemblyOwnerRun>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    additional_operand_frames: Vec<DesignAssemblyOperandFrame>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    additional_operand_qualifiers: Vec<DesignAssemblyOperandQualifier>,
}

cadmpeg_core::named_optional_field!(
    deserialize_additional_owner_run,
    DesignAssemblyOwnerRun,
    "additional_owner_run"
);

cadmpeg_core::named_optional_field!(
    deserialize_frame_sense,
    DesignJointFrameSense,
    "frame_sense"
);

impl TryFrom<DesignAssemblyAlignmentSerde> for DesignAssemblyAlignment {
    type Error = String;

    fn try_from(wire: DesignAssemblyAlignmentSerde) -> Result<Self, Self::Error> {
        if wire.owner_record_indices.len() != wire.value_offsets.len() {
            return Err("owner_record_indices and value_offsets must have equal lengths".into());
        }
        let owners = wire
            .owner_record_indices
            .into_iter()
            .zip(wire.value_offsets)
            .map(|(value, offset)| Located { value, offset })
            .collect();
        let form = match (
            wire.legacy_operand_carriers,
            wire.solved_frame,
            wire.operand_frames,
            wire.operand_qualifiers,
            wire.limits,
            wire.joint_origin_scope_record_index,
        ) {
            (None, None, None, None, None, Some(joint_origin_scope_record_index)) => Some(
                DesignAssemblyAlignmentForm::DatumEnvelope { joint_origin_scope_record_index }
            ),
            (Some(carriers), Some(solved_frame), frames, None, limits, None) => {
                let carriers = DesignAssemblyLegacyOperands::from_wire(carriers, &solved_frame)?;
                let derived_frames = carriers.frames(&solved_frame)?;
                if frames.as_ref().is_some_and(|frames| frames != &derived_frames) {
                    return Err("operand_frames must match legacy_operand_carriers frames".into());
                }
                Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 { carriers, solved_frame, limits, frames_field_present: frames.is_some() })
            }
            (None, Some(solved_frame), None, None, limits, None) => Some(
                DesignAssemblyAlignmentForm::SolvedOnly { solved_frame, limits }
            ),
            (None, None, None, None, Some(limits), None) => Some(
                DesignAssemblyAlignmentForm::LimitsOnly { limits }
            ),
            (None, None, Some(frames), None, None, None) => Some(
                DesignAssemblyAlignmentForm::Frames { frames }
            ),
            (None, None, Some(frames), Some(qualifiers), None, None) => Some(
                DesignAssemblyAlignmentForm::qualified(frames, qualifiers)
            ),
            (None, None, None, None, None, None) => None,
            _ => return Err("assembly alignment operand_frames, legacy_operand_carriers, solved_frame, operand_qualifiers, limits, and joint_origin_scope_record_index disagree with one form".into()),
        };
        if wire.additional_operand_frames.len() != wire.additional_operand_qualifiers.len() {
            return Err(
                "additional_operand_frames and additional_operand_qualifiers must have equal lengths"
                    .into(),
            );
        }
        let additional_members = wire
            .additional_operand_frames
            .into_iter()
            .zip(wire.additional_operand_qualifiers)
            .map(|(frame, qualifier)| DesignQualifiedAssemblyOperand { frame, qualifier })
            .collect();
        let mut alignment = Self::try_new(wire.angle, wire.offset, owners, form)?
            .with_frame_sense(wire.frame_sense)
            .with_additional_members(additional_members)?;
        alignment.additional_owner_run = wire.additional_owner_run;
        Ok(alignment)
    }
}

impl Serialize for DesignAssemblyAlignment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("DesignAssemblyAlignmentSerde", 14)?;
        record.serialize_field("angle", &self.angle.get())?;
        record.serialize_field("offset", &self.offset.map(FiniteReal::get))?;
        record.serialize_field("owner_record_indices", &OwnerRecordIndices(&self.owners))?;
        record.serialize_field("value_offsets", &OwnerValueOffsets(&self.owners))?;
        match self.form.as_ref() {
            None => {}
            Some(DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index,
            }) => {
                record.serialize_field(
                    "joint_origin_scope_record_index",
                    joint_origin_scope_record_index,
                )?;
            }
            Some(DesignAssemblyAlignmentForm::LimitsOnly { limits }) => {
                record.serialize_field("limits", limits)?;
            }
            Some(DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame,
                limits,
            }) => {
                record.serialize_field("solved_frame", solved_frame)?;
                if let Some(limits) = limits {
                    record.serialize_field("limits", limits)?;
                }
            }
            Some(DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                carriers,
                solved_frame,
                limits,
                frames_field_present,
            }) => {
                let frames = carriers
                    .frames(solved_frame)
                    .map_err(serde::ser::Error::custom)?;
                if *frames_field_present {
                    record.serialize_field("operand_frames", &frames)?;
                }
                let [point_frame, hole_frame] = frames;
                let legacy = [
                    LegacyOperandOut {
                        construction_record_index: carriers.point.construction.point_record_index,
                        construction_byte_offset: carriers
                            .point
                            .construction
                            .point_record_byte_offset,
                        construction_class_tag: carriers.point.construction_class_tag.as_str(),
                        construction: LegacyConstructionOut::Point(&carriers.point.construction),
                        selection: &carriers.point.selection,
                        frame: &point_frame,
                    },
                    LegacyOperandOut {
                        construction_record_index: carriers.hole.construction.point_record_index,
                        construction_byte_offset: carriers
                            .hole
                            .construction
                            .point_record_byte_offset,
                        construction_class_tag: carriers.hole.construction_class_tag.as_str(),
                        construction: LegacyConstructionOut::Hole(&carriers.hole.construction),
                        selection: &carriers.hole.selection,
                        frame: &hole_frame,
                    },
                ];
                record.serialize_field("legacy_operand_carriers", &legacy)?;
                record.serialize_field("solved_frame", solved_frame)?;
                if let Some(limits) = limits {
                    record.serialize_field("limits", limits)?;
                }
            }
            Some(DesignAssemblyAlignmentForm::Frames { frames }) => {
                record.serialize_field("operand_frames", frames)?;
            }
            Some(DesignAssemblyAlignmentForm::Qualified(operands)) => {
                let frames = operands.each_ref().map(|operand| &operand.frame);
                let qualifiers = operands.each_ref().map(|operand| &operand.qualifier);
                record.serialize_field("operand_frames", &frames)?;
                record.serialize_field("operand_qualifiers", &qualifiers)?;
            }
        }
        if let Some(run) = &self.additional_owner_run {
            record.serialize_field("additional_owner_run", run)?;
        }
        if let Some(sense) = &self.frame_sense {
            record.serialize_field("frame_sense", sense)?;
        }
        if !self.additional_members.is_empty() {
            let frames = self
                .additional_members
                .iter()
                .map(|member| &member.frame)
                .collect::<Vec<_>>();
            let qualifiers = self
                .additional_members
                .iter()
                .map(|member| &member.qualifier)
                .collect::<Vec<_>>();
            record.serialize_field("additional_operand_frames", &frames)?;
            record.serialize_field("additional_operand_qualifiers", &qualifiers)?;
        }
        record.end()
    }
}

struct OwnerRecordIndices<'a>(&'a [Located<u32>]);

impl Serialize for OwnerRecordIndices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for owner in self.0 {
            seq.serialize_element(&owner.value)?;
        }
        seq.end()
    }
}

struct OwnerValueOffsets<'a>(&'a [Located<u32>]);

impl Serialize for OwnerValueOffsets<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for owner in self.0 {
            seq.serialize_element(&owner.offset)?;
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct LegacyOperandOut<'a> {
    construction_record_index: u32,
    construction_byte_offset: u64,
    construction_class_tag: &'a str,
    construction: LegacyConstructionOut<'a>,
    selection: &'a DesignAssemblyLegacySelection,
    frame: &'a DesignAssemblyOperandFrame,
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum LegacyConstructionOut<'a> {
    Point(&'a DesignWorkPointConstruction),
    Hole(&'a DesignHoleConstruction),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignAssemblyAxialOperandTarget {
    /// Connector object selected inside a placed `Component Insert` occurrence.
    ComponentInsertOccurrence {
        /// `Component Insert` scope whose placement has the selected occurrence role.
        component_insert_scope_record_index: u32,
        /// Construction carrier referenced by the operand frame.
        construction_record_index: u32,
        /// Dynamic class of the construction carrier's primary record.
        construction_class_tag: DesignClassTag,
        /// Byte offset of the construction carrier's primary indexed header.
        construction_byte_offset: u64,
        /// Byte offset of the construction carrier's transform.
        construction_transform_offset: u64,
        /// Byte offsets of the two axis-record indices in the construction carrier.
        axis_record_index_offsets: [u64; 2],
        /// Dynamic class of the construction carrier's paired record.
        construction_paired_class_tag: DesignClassTag,
        /// Byte offset of the construction carrier's paired indexed header.
        construction_paired_byte_offset: u64,
        /// Two axis selectors that identify the same connector object.
        selectors: Box<[DesignAssemblyAxialSelectorIdentity; 2]>,
    },
    /// Datum connector owned directly by the current document root.
    DocumentRootJointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
    },
}

/// Persistent connector identity carried by one axial assembly selector.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DesignAssemblyAxialSelectorIdentityWire")]
pub(crate) struct DesignAssemblyAxialSelectorIdentity {
    /// Axis record named by the operand construction carrier.
    pub(crate) axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    pub(crate) axis_class_tag: DesignClassTag,
    /// Byte offset of the axis record's primary indexed header.
    pub(crate) axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    pub(crate) axis_paired_class_tag: DesignClassTag,
    /// Byte offset of the axis record's paired indexed header.
    pub(crate) axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    pub(crate) selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    pub(crate) selector_class_tag: DesignClassTag,
    /// Byte offset of the selector record's primary indexed header.
    pub(crate) selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    pub(crate) selector_paired_class_tag: DesignClassTag,
    /// Byte offset of the selector record's paired indexed header.
    pub(crate) selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    pub(crate) nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    pub(crate) nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    pub(crate) selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    pub(crate) selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    pub(crate) selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    pub(crate) selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    pub(crate) occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    pub(crate) occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    pub(crate) external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    pub(crate) external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    pub(crate) external_segment: u32,
    /// Byte offset of `external_segment`.
    pub(crate) external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    pub(crate) external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    pub(crate) external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    pub(crate) external_link_name: String,
    /// Byte offset of `external_link_name`.
    pub(crate) external_link_name_offset: u64,
    /// Located property key and referenced-document version identity.
    pub(crate) external_version: Option<DesignExternalVersion>,
    /// Embedded record that carries the selected occurrence role.
    pub(crate) role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    pub(crate) role_class_tag: DesignClassTag,
    /// Byte offset of the occurrence-role record's indexed header.
    pub(crate) role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    pub(crate) occurrence_role: DesignRelaxedGuidText,
    /// Byte offset of `occurrence_role`.
    pub(crate) occurrence_role_offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesignAssemblyAxialSelectorIdentityWire {
    /// Axis record named by the operand construction carrier.
    axis_record_index: u32,
    /// Dynamic class of the axis record's primary indexed header.
    axis_class_tag: String,
    /// Byte offset of the axis record's primary indexed header.
    axis_byte_offset: u64,
    /// Dynamic class of the axis record's paired indexed header.
    axis_paired_class_tag: String,
    /// Byte offset of the axis record's paired indexed header.
    axis_paired_byte_offset: u64,
    /// Selector record three indices after the axis record.
    selector_record_index: u32,
    /// Dynamic class of the selector record's primary indexed header.
    selector_class_tag: String,
    /// Byte offset of the selector record's primary indexed header.
    selector_byte_offset: u64,
    /// Dynamic class of the selector record's paired indexed header.
    selector_paired_class_tag: String,
    /// Byte offset of the selector record's paired indexed header.
    selector_paired_byte_offset: u64,
    /// Nested record named by the selector prefix.
    nested_record_index: u32,
    /// Byte offset of `nested_record_index`.
    nested_record_index_offset: u64,
    /// Asset GUID of the enclosing selector.
    selector_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_asset_id`.
    selector_asset_id_offset: u64,
    /// Context GUID of the enclosing selector.
    selector_context_id: DesignRelaxedGuidText,
    /// Byte offset of `selector_context_id`.
    selector_context_id_offset: u64,
    /// Axis-specific same-segment occurrence reference.
    occurrence_reference: u64,
    /// Byte offset of `occurrence_reference`.
    occurrence_reference_offset: u64,
    /// Entity reference of the selected object in the referenced document.
    external_object_reference: u64,
    /// Byte offset of `external_object_reference`.
    external_object_reference_offset: u64,
    /// Segment carried by the cross-document object reference.
    external_segment: u32,
    /// Byte offset of `external_segment`.
    external_segment_offset: u64,
    /// Asset GUID carried by the cross-document object reference.
    external_asset_id: DesignRelaxedGuidText,
    /// Byte offset of `external_asset_id`.
    external_asset_id_offset: u64,
    /// Link name carried by the cross-document object reference.
    external_link_name: String,
    /// Byte offset of `external_link_name`.
    external_link_name_offset: u64,
    /// Optional property key preceding the version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key"
    )]
    external_property_key: Option<DesignRelaxedGuidText>,
    /// Byte offset of `external_property_key` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_property_key_offset"
    )]
    external_property_key_offset: Option<u64>,
    /// Optional referenced-document version identity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn"
    )]
    external_version_urn: Option<String>,
    /// Byte offset of `external_version_urn` when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_external_version_urn_offset"
    )]
    external_version_urn_offset: Option<u64>,
    /// Embedded record that carries the selected occurrence role.
    role_record_index: u32,
    /// Dynamic class of the occurrence-role record.
    role_class_tag: String,
    /// Byte offset of the occurrence-role record's indexed header.
    role_byte_offset: u64,
    /// Occurrence-role GUID joining this selector to a component insertion.
    occurrence_role: DesignRelaxedGuidText,
    /// Byte offset of `occurrence_role`.
    occurrence_role_offset: u64,
}

#[cfg(test)]
thread_local! {
    static ASSEMBLY_AXIAL_SELECTOR_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignAssemblyAxialSelectorIdentity {
    fn clone(&self) -> Self {
        #[cfg(test)]
        ASSEMBLY_AXIAL_SELECTOR_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            axis_record_index: self.axis_record_index,
            axis_class_tag: self.axis_class_tag.clone(),
            axis_byte_offset: self.axis_byte_offset,
            axis_paired_class_tag: self.axis_paired_class_tag.clone(),
            axis_paired_byte_offset: self.axis_paired_byte_offset,
            selector_record_index: self.selector_record_index,
            selector_class_tag: self.selector_class_tag.clone(),
            selector_byte_offset: self.selector_byte_offset,
            selector_paired_class_tag: self.selector_paired_class_tag.clone(),
            selector_paired_byte_offset: self.selector_paired_byte_offset,
            nested_record_index: self.nested_record_index,
            nested_record_index_offset: self.nested_record_index_offset,
            selector_asset_id: self.selector_asset_id.clone(),
            selector_asset_id_offset: self.selector_asset_id_offset,
            selector_context_id: self.selector_context_id.clone(),
            selector_context_id_offset: self.selector_context_id_offset,
            occurrence_reference: self.occurrence_reference,
            occurrence_reference_offset: self.occurrence_reference_offset,
            external_object_reference: self.external_object_reference,
            external_object_reference_offset: self.external_object_reference_offset,
            external_segment: self.external_segment,
            external_segment_offset: self.external_segment_offset,
            external_asset_id: self.external_asset_id.clone(),
            external_asset_id_offset: self.external_asset_id_offset,
            external_link_name: self.external_link_name.clone(),
            external_link_name_offset: self.external_link_name_offset,
            external_version: self.external_version.clone(),
            role_record_index: self.role_record_index,
            role_class_tag: self.role_class_tag.clone(),
            role_byte_offset: self.role_byte_offset,
            occurrence_role: self.occurrence_role.clone(),
            occurrence_role_offset: self.occurrence_role_offset,
        }
    }
}

#[derive(Serialize)]
struct DesignAssemblyAxialSelectorIdentityWireRef<'a> {
    axis_record_index: u32,
    axis_class_tag: &'a str,
    axis_byte_offset: u64,
    axis_paired_class_tag: &'a str,
    axis_paired_byte_offset: u64,
    selector_record_index: u32,
    selector_class_tag: &'a str,
    selector_byte_offset: u64,
    selector_paired_class_tag: &'a str,
    selector_paired_byte_offset: u64,
    nested_record_index: u32,
    nested_record_index_offset: u64,
    selector_asset_id: &'a DesignRelaxedGuidText,
    selector_asset_id_offset: u64,
    selector_context_id: &'a DesignRelaxedGuidText,
    selector_context_id_offset: u64,
    occurrence_reference: u64,
    occurrence_reference_offset: u64,
    external_object_reference: u64,
    external_object_reference_offset: u64,
    external_segment: u32,
    external_segment_offset: u64,
    external_asset_id: &'a DesignRelaxedGuidText,
    external_asset_id_offset: u64,
    external_link_name: &'a str,
    external_link_name_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_property_key: Option<&'a DesignRelaxedGuidText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_property_key_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_version_urn: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_version_urn_offset: Option<u64>,
    role_record_index: u32,
    role_class_tag: &'a str,
    role_byte_offset: u64,
    occurrence_role: &'a DesignRelaxedGuidText,
    occurrence_role_offset: u64,
}

impl<'a> From<&'a DesignAssemblyAxialSelectorIdentity>
    for DesignAssemblyAxialSelectorIdentityWireRef<'a>
{
    fn from(record: &'a DesignAssemblyAxialSelectorIdentity) -> Self {
        Self {
            axis_record_index: record.axis_record_index,
            axis_class_tag: record.axis_class_tag.as_str(),
            axis_byte_offset: record.axis_byte_offset,
            axis_paired_class_tag: record.axis_paired_class_tag.as_str(),
            axis_paired_byte_offset: record.axis_paired_byte_offset,
            selector_record_index: record.selector_record_index,
            selector_class_tag: record.selector_class_tag.as_str(),
            selector_byte_offset: record.selector_byte_offset,
            selector_paired_class_tag: record.selector_paired_class_tag.as_str(),
            selector_paired_byte_offset: record.selector_paired_byte_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            selector_asset_id: &record.selector_asset_id,
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: &record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_object_reference: record.external_object_reference,
            external_object_reference_offset: record.external_object_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: &record.external_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name.as_str(),
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|v| &v.property_key.value),
            external_property_key_offset: record
                .external_version
                .as_ref()
                .map(|v| v.property_key.offset),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|v| v.version_urn.value.as_str()),
            external_version_urn_offset: record
                .external_version
                .as_ref()
                .map(|v| v.version_urn.offset),
            role_record_index: record.role_record_index,
            role_class_tag: record.role_class_tag.as_str(),
            role_byte_offset: record.role_byte_offset,
            occurrence_role: &record.occurrence_role,
            occurrence_role_offset: record.occurrence_role_offset,
        }
    }
}

impl Serialize for DesignAssemblyAxialSelectorIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignAssemblyAxialSelectorIdentityWireRef::from(self).serialize(serializer)
    }
}

impl TryFrom<DesignAssemblyAxialSelectorIdentityWire> for DesignAssemblyAxialSelectorIdentity {
    type Error = String;
    fn try_from(wire: DesignAssemblyAxialSelectorIdentityWire) -> Result<Self, Self::Error> {
        Ok(Self {
            axis_record_index: wire.axis_record_index,
            axis_class_tag: wire.axis_class_tag.try_into().map_err(|error| format!("axis_class_tag: {error}"))?,
            axis_byte_offset: wire.axis_byte_offset,
            axis_paired_class_tag: wire.axis_paired_class_tag.try_into().map_err(|error| format!("axis_paired_class_tag: {error}"))?,
            axis_paired_byte_offset: wire.axis_paired_byte_offset,
            selector_record_index: wire.selector_record_index,
            selector_class_tag: wire.selector_class_tag.try_into().map_err(|error| format!("selector_class_tag: {error}"))?,
            selector_byte_offset: wire.selector_byte_offset,
            selector_paired_class_tag: wire.selector_paired_class_tag.try_into().map_err(|error| format!("selector_paired_class_tag: {error}"))?,
            selector_paired_byte_offset: wire.selector_paired_byte_offset,
            nested_record_index: wire.nested_record_index,
            nested_record_index_offset: wire.nested_record_index_offset,
            selector_asset_id: wire.selector_asset_id,
            selector_asset_id_offset: wire.selector_asset_id_offset,
            selector_context_id: wire.selector_context_id,
            selector_context_id_offset: wire.selector_context_id_offset,
            occurrence_reference: wire.occurrence_reference,
            occurrence_reference_offset: wire.occurrence_reference_offset,
            external_object_reference: wire.external_object_reference,
            external_object_reference_offset: wire.external_object_reference_offset,
            external_segment: wire.external_segment,
            external_segment_offset: wire.external_segment_offset,
            external_asset_id: wire.external_asset_id,
            external_asset_id_offset: wire.external_asset_id_offset,
            external_link_name: wire.external_link_name,
            external_link_name_offset: wire.external_link_name_offset,
            external_version: match (wire.external_property_key, wire.external_property_key_offset, wire.external_version_urn, wire.external_version_urn_offset) {
                (None, None, None, None) => None,
                (Some(key), Some(key_offset), Some(urn), Some(urn_offset)) => Some(DesignExternalVersion { property_key: Located { value: key, offset: key_offset }, version_urn: Located { value: urn, offset: urn_offset } }),
                _ => return Err("external_property_key, external_property_key_offset, external_version_urn and external_version_urn_offset must occur together".into()),
            },
            role_record_index: wire.role_record_index,
            role_class_tag: wire.role_class_tag.try_into().map_err(|error| format!("role_class_tag: {error}"))?,
            role_byte_offset: wire.role_byte_offset,
            occurrence_role: wire.occurrence_role,
            occurrence_role_offset: wire.occurrence_role_offset,
        })
    }
}

#[cfg(test)]
impl From<DesignAssemblyAxialSelectorIdentity> for DesignAssemblyAxialSelectorIdentityWire {
    fn from(record: DesignAssemblyAxialSelectorIdentity) -> Self {
        Self {
            axis_record_index: record.axis_record_index,
            axis_class_tag: record.axis_class_tag.into(),
            axis_byte_offset: record.axis_byte_offset,
            axis_paired_class_tag: record.axis_paired_class_tag.into(),
            axis_paired_byte_offset: record.axis_paired_byte_offset,
            selector_record_index: record.selector_record_index,
            selector_class_tag: record.selector_class_tag.into(),
            selector_byte_offset: record.selector_byte_offset,
            selector_paired_class_tag: record.selector_paired_class_tag.into(),
            selector_paired_byte_offset: record.selector_paired_byte_offset,
            nested_record_index: record.nested_record_index,
            nested_record_index_offset: record.nested_record_index_offset,
            selector_asset_id: record.selector_asset_id,
            selector_asset_id_offset: record.selector_asset_id_offset,
            selector_context_id: record.selector_context_id,
            selector_context_id_offset: record.selector_context_id_offset,
            occurrence_reference: record.occurrence_reference,
            occurrence_reference_offset: record.occurrence_reference_offset,
            external_object_reference: record.external_object_reference,
            external_object_reference_offset: record.external_object_reference_offset,
            external_segment: record.external_segment,
            external_segment_offset: record.external_segment_offset,
            external_asset_id: record.external_asset_id,
            external_asset_id_offset: record.external_asset_id_offset,
            external_link_name: record.external_link_name,
            external_link_name_offset: record.external_link_name_offset,
            external_property_key: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.value.clone()),
            external_property_key_offset: record
                .external_version
                .as_ref()
                .map(|version| version.property_key.offset),
            external_version_urn: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.value.clone()),
            external_version_urn_offset: record
                .external_version
                .as_ref()
                .map(|version| version.version_urn.offset),
            role_record_index: record.role_record_index,
            role_class_tag: record.role_class_tag.into(),
            role_byte_offset: record.role_byte_offset,
            occurrence_role: record.occurrence_role,
            occurrence_role_offset: record.occurrence_role_offset,
        }
    }
}

impl DesignAssemblyAxialSelectorIdentity {
    /// Report whether two axis selectors carry the same persistent connector identity.
    pub(crate) fn selects_same_object(&self, other: &Self) -> bool {
        self.selector_asset_id
            .as_str()
            .eq_ignore_ascii_case(other.selector_asset_id.as_str())
            && self
                .selector_context_id
                .as_str()
                .eq_ignore_ascii_case(other.selector_context_id.as_str())
            && self.external_object_reference == other.external_object_reference
            && self.external_segment == other.external_segment
            && self
                .external_asset_id
                .as_str()
                .eq_ignore_ascii_case(other.external_asset_id.as_str())
            && self.external_link_name == other.external_link_name
            && match (&self.external_version, &other.external_version) {
                (None, None) => true,
                (Some(first), Some(second)) => {
                    first
                        .property_key
                        .value
                        .as_str()
                        .eq_ignore_ascii_case(second.property_key.value.as_str())
                        && first.version_urn.value == second.version_urn.value
                }
                _ => false,
            }
    }
}

/// Exact reference chain from an assembly scope to one occurrence-path record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblyOperandPathLink {
    /// Byte offset of the locator-record index in the assembly scope.
    pub(crate) locator_reference_offset: u64,
    /// Locator-record index named by the assembly scope.
    pub(crate) locator_record_index: u32,
    /// Dynamic indexed-record class carrying the locator.
    pub(crate) locator_class_tag: DesignClassTag,
    /// Byte offset of the locator's indexed header.
    pub(crate) locator_byte_offset: u64,
    /// Byte offset of the assembly-scope backlink in the locator.
    pub(crate) locator_scope_reference_offset: u64,
    /// Wrapper-record index named by the locator.
    pub(crate) wrapper_record_index: u32,
    /// Byte offset of the wrapper-record index in the locator.
    pub(crate) wrapper_reference_offset: u64,
    /// Dynamic indexed-record class carrying the wrapper.
    pub(crate) wrapper_class_tag: DesignClassTag,
    /// Byte offset of the wrapper's indexed header.
    pub(crate) wrapper_byte_offset: u64,
    /// Byte offset of the path-record index in the wrapper.
    pub(crate) path_reference_offset: u64,
}

/// Type GUID of the identity-qualified counted occurrence-path record. A path
/// record of this type reads in that form whatever its dynamic class tag.
pub(crate) const TYPED_OCCURRENCE_PATH_TYPE_GUID: &str = "96CBEA21-17E0-4BCB-BA77-04EA87B47934";

/// Counted occurrence path qualifying one assembly operand construction.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignAssemblyOperandPathWire")]
pub(crate) struct DesignAssemblyOperandPath {
    link: DesignAssemblyOperandPathLink,
    pub(crate) record_index: u32,
    class_tag: DesignClassTag,
    /// Type GUID that admitted the path record, when its type admitted it.
    type_guid: Option<DesignRelaxedGuidText>,
    byte_offset: u64,
    /// Ordered occurrence GUIDs and their UTF-16 code-unit locations.
    occurrence_guids: Vec<Located<DesignOccurrenceKey>>,
    /// Ordered identity GUIDs and their UTF-16 code-unit locations.
    identity_guids: Vec<Located<DesignRelaxedGuidText>>,
    /// The occurrence identity pairs that follow the identity GUIDs, two
    /// entries per pair, and their UTF-16 code-unit locations.
    paired_occurrence_keys: Vec<Located<DesignOccurrenceKey>>,
}

#[cfg(test)]
thread_local! {
    pub(super) static ASSEMBLY_OPERAND_PATH_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignAssemblyOperandPath {
    fn clone(&self) -> Self {
        #[cfg(test)]
        ASSEMBLY_OPERAND_PATH_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            link: self.link.clone(),
            record_index: self.record_index,
            class_tag: self.class_tag.clone(),
            type_guid: self.type_guid.clone(),
            byte_offset: self.byte_offset,
            occurrence_guids: self.occurrence_guids.clone(),
            identity_guids: self.identity_guids.clone(),
            paired_occurrence_keys: self.paired_occurrence_keys.clone(),
        }
    }
}

struct PathGuidValues<'a, T>(&'a [Located<T>]);
struct PathGuidOffsets<'a, T>(&'a [Located<T>]);

impl<T> PathGuidValues<'_, T> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T> PathGuidOffsets<'_, T> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: Serialize> Serialize for PathGuidValues<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|guid| &guid.value))
    }
}

impl<T> Serialize for PathGuidOffsets<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|guid| guid.offset))
    }
}

#[derive(Serialize)]
struct DesignAssemblyOperandPathRef<'a> {
    link: &'a DesignAssemblyOperandPathLink,
    record_index: u32,
    class_tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_guid: Option<&'a DesignRelaxedGuidText>,
    byte_offset: u64,
    occurrence_guids: PathGuidValues<'a, DesignOccurrenceKey>,
    occurrence_guid_offsets: PathGuidOffsets<'a, DesignOccurrenceKey>,
    #[serde(skip_serializing_if = "PathGuidValues::is_empty")]
    identity_guids: PathGuidValues<'a, DesignRelaxedGuidText>,
    #[serde(skip_serializing_if = "PathGuidOffsets::is_empty")]
    identity_guid_offsets: PathGuidOffsets<'a, DesignRelaxedGuidText>,
    #[serde(skip_serializing_if = "PathGuidValues::is_empty")]
    paired_occurrence_keys: PathGuidValues<'a, DesignOccurrenceKey>,
    #[serde(skip_serializing_if = "PathGuidOffsets::is_empty")]
    paired_occurrence_key_offsets: PathGuidOffsets<'a, DesignOccurrenceKey>,
}

impl Serialize for DesignAssemblyOperandPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignAssemblyOperandPathRef {
            link: &self.link,
            record_index: self.record_index,
            class_tag: self.class_tag.as_str(),
            type_guid: self.type_guid.as_ref(),
            byte_offset: self.byte_offset,
            occurrence_guids: PathGuidValues(&self.occurrence_guids),
            occurrence_guid_offsets: PathGuidOffsets(&self.occurrence_guids),
            identity_guids: PathGuidValues(&self.identity_guids),
            identity_guid_offsets: PathGuidOffsets(&self.identity_guids),
            paired_occurrence_keys: PathGuidValues(&self.paired_occurrence_keys),
            paired_occurrence_key_offsets: PathGuidOffsets(&self.paired_occurrence_keys),
        }
        .serialize(serializer)
    }
}

/// Counted occurrence path qualifying one assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignAssemblyOperandPathWire {
    /// Exact ordered scope-to-locator-to-wrapper reference chain.
    link: DesignAssemblyOperandPathLink,
    /// Path-record index.
    record_index: u32,
    /// Indexed-record class carrying this path.
    class_tag: String,
    /// Type GUID that admitted the path record, when its type admitted it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_path_type_guid"
    )]
    type_guid: Option<DesignRelaxedGuidText>,
    /// Byte offset of the indexed header.
    byte_offset: u64,
    /// Ordered occurrence keys, or the zero key of a document-root path.
    occurrence_guids: Vec<DesignOccurrenceKey>,
    /// Byte offsets of the UTF-16 GUID code units parallel to `occurrence_guids`.
    occurrence_guid_offsets: Vec<u64>,
    /// Ordered identity GUIDs of each path record; a root path has two.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity_guids: Vec<DesignRelaxedGuidText>,
    /// Byte offsets parallel to `identity_guids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity_guid_offsets: Vec<u64>,
    /// Occurrence identity pairs after the identity GUIDs, two entries per pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paired_occurrence_keys: Vec<DesignOccurrenceKey>,
    /// Byte offsets parallel to `paired_occurrence_keys`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paired_occurrence_key_offsets: Vec<u64>,
}

impl DesignAssemblyOperandPath {
    pub(crate) fn try_new(
        link: DesignAssemblyOperandPathLink,
        record_index: u32,
        class_tag: DesignClassTag,
        byte_offset: u64,
        occurrence_guids: Vec<Located<DesignOccurrenceKey>>,
        identity_guids: Vec<Located<DesignRelaxedGuidText>>,
    ) -> Result<Self, String> {
        Self::try_new_typed(
            link,
            record_index,
            class_tag,
            None,
            byte_offset,
            occurrence_guids,
            identity_guids,
        )
    }
    /// A path whose record `type_guid` admitted it, or `None` for a path
    /// admitted by its class tag.
    pub(crate) fn try_new_typed(
        link: DesignAssemblyOperandPathLink,
        record_index: u32,
        class_tag: DesignClassTag,
        type_guid: Option<DesignRelaxedGuidText>,
        byte_offset: u64,
        occurrence_guids: Vec<Located<DesignOccurrenceKey>>,
        identity_guids: Vec<Located<DesignRelaxedGuidText>>,
    ) -> Result<Self, String> {
        let typed = match &type_guid {
            Some(guid)
                if guid
                    .as_str()
                    .eq_ignore_ascii_case(TYPED_OCCURRENCE_PATH_TYPE_GUID) =>
            {
                true
            }
            Some(_) => return Err("type_guid names no occurrence-path record type".into()),
            None => false,
        };
        let legacy_pair =
            class_tag.as_str() == "386" && matches!(link.locator_class_tag.as_str(), "363" | "378");
        if occurrence_guids.is_empty() || (legacy_pair && occurrence_guids.len() != 1) {
            return Err("occurrence_guids has invalid assembly path arity".into());
        }
        let root_path = occurrence_guids.len() == 1
            && occurrence_guids[0].value.as_str() == "00000000-0000-0000-0000-000000000000"
            && identity_guids.len() == 2;
        let identity_arity = if typed {
            root_path || !identity_guids.is_empty() && identity_guids.len().is_multiple_of(4)
        } else if legacy_pair {
            identity_guids.len() == 1
        } else {
            match class_tag.as_str() {
                "294" | "299" | "307" | "386" | "390" | "412" => identity_guids.len() == 4,
                "329" => matches!(identity_guids.len(), 0 | 4),
                "330" => !identity_guids.is_empty() && identity_guids.len().is_multiple_of(4),
                _ => false,
            }
        };
        if !identity_arity {
            return Err("identity_guids has invalid assembly path class arity".into());
        }
        let follows_byte_offset = !legacy_pair && class_tag.as_str() != "412";
        for (field, offsets) in [
            (
                "occurrence_guids",
                occurrence_guids
                    .iter()
                    .map(|guid| guid.offset)
                    .collect::<Vec<_>>(),
            ),
            (
                "identity_guids",
                identity_guids
                    .iter()
                    .map(|guid| guid.offset)
                    .collect::<Vec<_>>(),
            ),
        ] {
            if !offsets.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err(format!("{field} offsets must strictly increase"));
            }
            if follows_byte_offset && offsets.iter().any(|offset| *offset <= byte_offset) {
                return Err(format!("{field} offsets must follow byte_offset"));
            }
        }
        Ok(Self {
            link,
            record_index,
            class_tag,
            type_guid,
            byte_offset,
            occurrence_guids,
            identity_guids,
            paired_occurrence_keys: Vec::new(),
        })
    }
    pub(crate) fn link(&self) -> &DesignAssemblyOperandPathLink {
        &self.link
    }
    pub(crate) fn class_tag(&self) -> &DesignClassTag {
        &self.class_tag
    }
    /// Whether the path record's type, not its class tag, admitted it.
    pub(crate) fn admitted_by_type(&self) -> bool {
        self.type_guid.is_some()
    }
    pub(crate) fn is_root(&self) -> bool {
        self.admitted_by_type()
            && self.occurrence_guids.len() == 1
            && self.occurrence_guids[0].value.as_str() == "00000000-0000-0000-0000-000000000000"
            && self.identity_guids.len() == 2
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.byte_offset
    }
    pub(crate) fn occurrence_guids(&self) -> &[Located<DesignOccurrenceKey>] {
        &self.occurrence_guids
    }
    pub(crate) fn identity_guids(&self) -> &[Located<DesignRelaxedGuidText>] {
        &self.identity_guids
    }
    pub(crate) fn paired_occurrence_keys(&self) -> &[Located<DesignOccurrenceKey>] {
        &self.paired_occurrence_keys
    }
    /// This path with the occurrence identity pairs that follow its identity
    /// GUIDs, two entries per pair, in source order after its first
    /// occurrence. A continued path keeps each record's pairs, so a pair can
    /// precede a continuation's identities.
    pub(crate) fn with_paired_occurrence_keys(
        mut self,
        keys: Vec<Located<DesignOccurrenceKey>>,
    ) -> Result<Self, String> {
        if !keys.len().is_multiple_of(2) {
            return Err("paired_occurrence_keys must hold whole pairs".into());
        }
        let mut previous = self
            .occurrence_guids
            .first()
            .map_or(self.byte_offset, |guid| guid.offset);
        for key in &keys {
            if key.offset <= previous {
                return Err(
                    "paired_occurrence_keys must follow the path's first occurrence in order"
                        .into(),
                );
            }
            previous = key.offset;
        }
        self.paired_occurrence_keys = keys;
        Ok(self)
    }
    /// This path continued by a further path record of its own form: two
    /// class-330 records, or two records admitted by the typed path type.
    pub(crate) fn try_append(self, continuation: Self) -> Option<Self> {
        if self.is_root() || continuation.is_root() {
            return None;
        }
        let typed = self.admitted_by_type() && continuation.admitted_by_type();
        if !typed && (self.class_tag.as_str() != "330" || continuation.class_tag.as_str() != "330")
        {
            return None;
        }
        let mut occurrences = self.occurrence_guids;
        occurrences.extend(continuation.occurrence_guids);
        let mut identities = self.identity_guids;
        identities.extend(continuation.identity_guids);
        let mut pairs = self.paired_occurrence_keys;
        pairs.extend(continuation.paired_occurrence_keys);
        Self::try_new_typed(
            self.link,
            self.record_index,
            self.class_tag,
            self.type_guid,
            self.byte_offset,
            occurrences,
            identities,
        )
        .ok()?
        .with_paired_occurrence_keys(pairs)
        .ok()
    }
}

impl TryFrom<DesignAssemblyOperandPathWire> for DesignAssemblyOperandPath {
    type Error = String;

    fn try_from(wire: DesignAssemblyOperandPathWire) -> Result<Self, Self::Error> {
        if wire.occurrence_guids.len() != wire.occurrence_guid_offsets.len() {
            return Err(
                "occurrence_guids and occurrence_guid_offsets must have equal lengths".into(),
            );
        }
        if wire.identity_guids.len() != wire.identity_guid_offsets.len() {
            return Err("identity_guids and identity_guid_offsets must have equal lengths".into());
        }
        if wire.paired_occurrence_keys.len() != wire.paired_occurrence_key_offsets.len() {
            return Err(
                "paired_occurrence_keys and paired_occurrence_key_offsets must have equal lengths"
                    .into(),
            );
        }
        let pairs = wire
            .paired_occurrence_keys
            .into_iter()
            .zip(wire.paired_occurrence_key_offsets)
            .map(|(value, offset)| Located { value, offset })
            .collect();
        Self::try_new_typed(
            wire.link,
            wire.record_index,
            wire.class_tag.try_into()?,
            wire.type_guid,
            wire.byte_offset,
            wire.occurrence_guids
                .into_iter()
                .zip(wire.occurrence_guid_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
            wire.identity_guids
                .into_iter()
                .zip(wire.identity_guid_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        )?
        .with_paired_occurrence_keys(pairs)
    }
}

#[cfg(test)]
impl From<DesignAssemblyOperandPath> for DesignAssemblyOperandPathWire {
    fn from(path: DesignAssemblyOperandPath) -> Self {
        let (occurrence_guids, occurrence_guid_offsets) = path
            .occurrence_guids
            .into_iter()
            .map(|guid| (guid.value, guid.offset))
            .unzip();
        let (identity_guids, identity_guid_offsets) = path
            .identity_guids
            .into_iter()
            .map(|guid| (guid.value, guid.offset))
            .unzip();
        let (paired_occurrence_keys, paired_occurrence_key_offsets) = path
            .paired_occurrence_keys
            .into_iter()
            .map(|key| (key.value, key.offset))
            .unzip();
        Self {
            link: path.link,
            record_index: path.record_index,
            class_tag: path.class_tag.into(),
            type_guid: path.type_guid,
            byte_offset: path.byte_offset,
            occurrence_guids,
            occurrence_guid_offsets,
            identity_guids,
            identity_guid_offsets,
            paired_occurrence_keys,
            paired_occurrence_key_offsets,
        }
    }
}

/// One exact native qualifier for an assembly operand construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignAssemblyOperandQualifier {
    /// Ordered occurrence path carried by a locator graph.
    OccurrencePath {
        /// Exact path and its native reference chain.
        path: DesignAssemblyOperandPath,
    },
    /// Pathless target carried by an axial selector graph.
    AxialTarget {
        /// Exact pathless target.
        target: DesignAssemblyAxialOperandTarget,
    },
    /// Datum connector owned directly by the current document root.
    JointOrigin {
        /// Referenced `JointOrigin` feature scope.
        scope_record_index: u32,
        /// Dynamic class of the scope's primary indexed header.
        class_tag: DesignClassTag,
        /// Byte offset of the primary indexed header.
        byte_offset: u64,
        /// Dynamic class of the paired indexed header.
        paired_class_tag: DesignClassTag,
        /// Byte offset of the paired indexed header.
        paired_byte_offset: u64,
    },
}

impl DesignAssemblyOperandQualifier {
    /// Return the occurrence path when this qualifier carries one.
    pub(crate) fn occurrence_path(&self) -> Option<&DesignAssemblyOperandPath> {
        match self {
            Self::OccurrencePath { path } => Some(path),
            Self::AxialTarget { .. } | Self::JointOrigin { .. } => None,
        }
    }
}

/// One operand frame embedded by an assembly-operation scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignAssemblyOperandFrame {
    /// Record that supplies or carries the operand frame.
    pub(crate) reference_record_index: u32,
    /// Byte offset of `reference_record_index`.
    pub(crate) reference_offset: u64,
    /// Row-major operand-local-to-model transform.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first transform scalar.
    pub(crate) transform_offset: u64,
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(
    deserialize_path_type_guid,
    DesignRelaxedGuidText,
    "type_guid"
);
cadmpeg_core::named_optional_field!(
    deserialize_operand_frames,
    [DesignAssemblyOperandFrame; 2],
    "operand_frames"
);
cadmpeg_core::named_optional_field!(
    deserialize_legacy_operand_carriers,
    [DesignAssemblyLegacyOperandWire; 2],
    "legacy_operand_carriers"
);
cadmpeg_core::named_optional_field!(
    deserialize_solved_frame,
    DesignAssemblySolvedFrame,
    "solved_frame"
);
cadmpeg_core::named_optional_field!(
    deserialize_operand_qualifiers,
    [DesignAssemblyOperandQualifier; 2],
    "operand_qualifiers"
);
cadmpeg_core::named_optional_field!(deserialize_limits, DesignAssemblyLimits, "limits");
cadmpeg_core::named_optional_field!(
    deserialize_joint_origin_scope_record_index,
    u32,
    "joint_origin_scope_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key,
    DesignRelaxedGuidText,
    "external_property_key"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_property_key_offset,
    u64,
    "external_property_key_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn,
    String,
    "external_version_urn"
);
cadmpeg_core::named_optional_field!(
    deserialize_external_version_urn_offset,
    u64,
    "external_version_urn_offset"
);

mod identity_rewrite;
