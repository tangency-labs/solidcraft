// SPDX-License-Identifier: Apache-2.0
//! Borrowed native wire fields of a parameter scope.

use super::{DesignParameterScope, DesignScopePayload};
use crate::records::identity::{MaybeRecordedValue, ReferenceRun};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

struct ReferenceValues<'a>(&'a ReferenceRun<u32>);

impl Serialize for ReferenceValues<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.values() {
            sequence.serialize_element(value)?;
        }
        sequence.end()
    }
}

struct ReferenceOffsets<'a>(&'a ReferenceRun<u32>);

impl Serialize for ReferenceOffsets<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.offsets().len()))?;
        for offset in self.0.offsets() {
            sequence.serialize_element(offset)?;
        }
        sequence.end()
    }
}

fn maybe_value<T>(field: &MaybeRecordedValue<T>) -> &T {
    match field {
        MaybeRecordedValue::Located(located) => &located.value,
        MaybeRecordedValue::Unlocated(value) => value,
    }
}

fn maybe_offset<T>(field: &MaybeRecordedValue<T>) -> Option<u64> {
    match field {
        MaybeRecordedValue::Located(located) => Some(located.offset),
        MaybeRecordedValue::Unlocated(_) => None,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "primitive")]
enum SolidPrimitiveRef<'a> {
    Box(&'a super::DesignBoxPrimitive),
    Cylinder(&'a super::DesignCylinderPrimitive),
    Sphere(&'a super::DesignSpherePrimitive),
    Torus(&'a super::DesignTorusPrimitive),
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
enum DirectFaceRef<'a> {
    OffsetFaces(&'a super::DesignOffsetFacesOperation),
    Shell(&'a super::DesignShellOperation),
    Thicken(&'a super::DesignThickenOperation),
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PathConstructionRef<'a> {
    Revolve(&'a super::DesignRevolveConstruction),
    Loft(&'a super::DesignLoftConstruction),
    Sweep(&'a super::DesignSweepConstruction),
    Pipe(&'a super::DesignPipeConstruction),
}

impl Serialize for DesignParameterScope {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        macro_rules! emit {
            ($key:literal, $value:expr) => {
                map.serialize_entry($key, &$value)?
            };
        }
        macro_rules! emit_opt {
            ($key:literal, $value:expr) => {
                if let Some(value) = $value {
                    map.serialize_entry($key, &value)?;
                }
            };
        }

        emit_opt!("occurrence_transfer", self.occurrence_transfer.as_ref());
        emit!("id", &self.id);
        emit!("byte_offset", self.byte_offset);
        emit!("class_tag", self.class_tag.as_str());
        emit!("record_index", self.record_index);
        emit!("frame_length", self.frame_length);
        emit!("kind", self.kind_name());
        emit_opt!("source_kind", self.source_kind.as_ref());
        emit!("kind_offset", self.kind_offset);

        if let Some(extrude) = self.extrude() {
            emit_opt!("extrude_prologue", extrude.extrude_prologue.as_ref());
            emit_opt!(
                "fixed_extrude_parameters",
                extrude.fixed_extrude_parameters.as_ref()
            );
            emit_opt!("extrude_profile", extrude.extrude_profile.as_ref());
            emit_opt!(
                "extrude_native_profiles",
                extrude.extrude_native_profiles.as_ref()
            );
        }
        if let Some(coil) = self.coil() {
            if let Some(value) = coil.operation.as_ref() {
                emit!("coil_operation", &value.value);
                emit!("coil_operation_offset", value.offset);
            }
            if let Some(value) = coil.extent.as_ref() {
                emit!("coil_extent", maybe_value(value));
                emit_opt!("coil_extent_offset", maybe_offset(value));
            }
            if let Some(value) = coil.section.as_ref() {
                emit!("coil_section", maybe_value(value));
                emit_opt!("coil_section_offset", maybe_offset(value));
            }
            if let Some(value) = coil.section_placement.as_ref() {
                emit!("coil_section_placement", maybe_value(value));
                emit_opt!("coil_section_placement_offset", maybe_offset(value));
            }
            if let Some(value) = coil.clockwise.as_ref() {
                emit!("coil_clockwise", maybe_value(value));
                emit_opt!("coil_clockwise_offset", maybe_offset(value));
            }
            emit_opt!("coil_placement", coil.placement.as_ref());
            emit_opt!("coil_transform", coil.transform.as_ref());
        }

        emit!("feature_ordinal", self.feature_ordinal.get());
        emit!("feature_ordinal_offset", self.feature_ordinal_offset);
        emit_opt!("history_state_id", self.history_state_id);
        emit!("history_state_id_offset", self.history_state_id_offset());
        emit_opt!("previous_history_state_id", self.previous_history_state_id);
        emit!(
            "previous_history_state_id_offset",
            self.previous_history_state_id_offset.unwrap_or(0)
        );
        emit!("reference_count_offset", self.reference_count_offset);
        emit!(
            "reference_members",
            ReferenceValues(&self.reference_members)
        );
        emit!(
            "reference_member_offsets",
            ReferenceOffsets(&self.reference_members)
        );

        match &self.payload {
            DesignScopePayload::SpherePrimitive(Some(value)) => {
                emit!("solid_primitive", SolidPrimitiveRef::Sphere(value));
            }
            DesignScopePayload::TorusPrimitive(Some(value)) => {
                emit!("solid_primitive", SolidPrimitiveRef::Torus(value));
            }
            DesignScopePayload::BoxPrimitive(Some(value)) => {
                emit!("solid_primitive", SolidPrimitiveRef::Box(value));
            }
            DesignScopePayload::CylinderPrimitive(Some(value)) => {
                emit!("solid_primitive", SolidPrimitiveRef::Cylinder(value));
            }
            DesignScopePayload::OffsetFaces(Some(value))
            | DesignScopePayload::DecalerLesFaces(Some(value)) => {
                emit!("direct_face_operation", DirectFaceRef::OffsetFaces(value));
            }
            DesignScopePayload::Shell(Some(value)) | DesignScopePayload::Schale(Some(value)) => {
                emit!("direct_face_operation", DirectFaceRef::Shell(value));
            }
            DesignScopePayload::Thicken(Some(value)) => {
                emit!("direct_face_operation", DirectFaceRef::Thicken(value));
            }
            _ => {}
        }

        emit_opt!("align_envelope", self.align_envelope());
        emit_opt!("move_operation", self.move_operation());
        emit_opt!("scale_operation", self.scale_operation());
        emit_opt!("surface_stitch_operation", self.surface_stitch_operation());
        emit_opt!("surface_extend_operation", self.surface_extend_operation());
        emit_opt!("surface_offset_operation", self.surface_offset_operation());
        emit_opt!("ruled_surface_operation", self.ruled_surface_operation());
        if let Some(base_flange) = self.base_flange() {
            emit_opt!(
                "base_flange_operation",
                base_flange.base_flange_operation.as_ref()
            );
            emit_opt!(
                "base_flange_profile",
                base_flange.base_flange_profile.as_ref()
            );
        }
        if let DesignScopePayload::SurfacePatch(patch) = &self.payload {
            if !patch.boundaries.is_empty() {
                emit!("surface_patch_boundaries", &patch.boundaries);
            }
            emit_opt!("surface_patch_normal", patch.normal.as_ref());
        }
        emit_opt!("edge_flange_operation", self.edge_flange_operation());
        emit_opt!("hem_operation", self.hem_operation());
        emit_opt!("fixed_fillet_parameters", self.fixed_fillet_parameters());
        emit_opt!("fixed_chamfer_parameters", self.fixed_chamfer_parameters());

        match &self.payload {
            DesignScopePayload::Revolve(Some(value)) => {
                emit!(
                    "path_feature_construction",
                    PathConstructionRef::Revolve(value)
                );
            }
            DesignScopePayload::Loft(Some(value)) => {
                emit!(
                    "path_feature_construction",
                    PathConstructionRef::Loft(value)
                );
            }
            DesignScopePayload::Pipe(Some(value)) => {
                emit!(
                    "path_feature_construction",
                    PathConstructionRef::Pipe(value)
                );
            }
            DesignScopePayload::Sweep(Some(value)) => {
                emit_opt!(
                    "path_feature_construction",
                    value.construction.as_ref().map(PathConstructionRef::Sweep)
                );
                emit_opt!("sweep_profile", value.sweep_profile.as_ref());
            }
            _ => {}
        }

        emit_opt!("combine_operation", self.combine_operation());
        emit_opt!("thread_construction", self.thread_construction());
        emit_opt!("emboss_construction", self.emboss_construction());
        emit_opt!("draft_operation", self.draft_operation());
        match &self.payload {
            DesignScopePayload::CPattern(value)
            | DesignScopePayload::CircularPattern(value)
            | DesignScopePayload::ReseauC(value) => {
                emit_opt!("circular_pattern_construction", value.as_ref());
            }
            DesignScopePayload::RPattern(value) | DesignScopePayload::RectangularPattern(value) => {
                emit_opt!("rectangular_pattern_construction", value.as_ref());
            }
            _ => {}
        }
        emit_opt!("assembly_alignment", self.assembly_alignment());
        match &self.payload {
            DesignScopePayload::ComponentInsert(value) => {
                emit_opt!("component_insert_construction", value.as_ref());
            }
            DesignScopePayload::DerivedInstance(value) => {
                emit_opt!("derived_instance_construction", value.as_ref());
            }
            DesignScopePayload::CopyPaste(value) => {
                emit_opt!("copy_paste_component_operation", value.as_ref());
            }
            _ => {}
        }
        emit_opt!("mirror_construction", self.mirror_construction());
        emit_opt!(
            "copy_paste_bodies_operation",
            self.copy_paste_bodies_operation()
        );
        emit_opt!(
            "base_feature_construction",
            self.base_feature_construction()
        );
        if let Some(frame) = self.work_plane_frame() {
            emit!("work_plane_transform", &frame.work_plane_transform);
            emit!(
                "work_plane_transform_offset",
                frame.work_plane_transform_offset
            );
            if let Some(reference) = frame.reference.as_ref() {
                emit!("work_plane_reference", reference.work_plane_reference);
                emit!(
                    "work_plane_reference_offset",
                    reference.work_plane_reference_offset
                );
            }
            emit_opt!(
                "work_plane_construction",
                frame.work_plane_construction.as_ref()
            );
        }
        emit_opt!("work_axis_construction", self.work_axis_construction());
        if let Some(frame) = self.joint_origin_frame() {
            emit!("joint_origin_transform", &frame.joint_origin_transform);
            emit!(
                "joint_origin_transform_offset",
                frame.joint_origin_transform_offset
            );
            if let Some(reference) = frame.reference.as_ref() {
                emit!("joint_origin_reference", reference.joint_origin_reference);
                emit!(
                    "joint_origin_reference_offset",
                    reference.joint_origin_reference_offset
                );
            }
        }
        emit_opt!("work_point_construction", self.work_point_construction());
        if !self.unclosed_construction_operand_groups.is_empty() {
            emit!(
                "unclosed_construction_operand_groups",
                &self.unclosed_construction_operand_groups
            );
        }
        emit_opt!("hole_construction", self.hole_construction());
        emit_opt!("hole_extent", self.hole_extent());
        if let Some(sketch) = self.sketch_entity() {
            emit!("entity_id", sketch.entity_id.as_str());
            emit!("entity_suffix", sketch.entity_id.suffix());
            emit!("entity_reference_offset", sketch.entity_reference_offset);
        }
        emit!("paired_class_tag", self.paired_class_tag.as_str());
        emit!("paired_byte_offset", self.paired_byte_offset);
        map.end()
    }
}
