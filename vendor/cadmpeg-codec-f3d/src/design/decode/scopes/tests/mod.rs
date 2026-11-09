// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod assembly;
mod assembly_variable_reference;
mod combine;
mod component_pattern;
mod copy_paste_bodies;
mod derived_instance;
mod draft_flags;
mod extended_thread;
mod fixed_extrude_lanes;
mod fixed_kind_operations;
mod fixed_kind_tail;
mod fixed_scalar_frames;
mod flange;
mod history_admission;
mod legacy_class_397;
mod legacy_frames;
mod legacy_work_planes;
mod move_transform;
mod named_empty_label;
mod named_variable_tail;
mod offset_faces;
mod referenced_revolve;
mod remove_body_named_tail;
mod scale;
mod shell_wall_slots;
mod surface_offset_groups;
mod surfaces;
mod thicken;
mod thread;
mod to_object_revolve;
mod typed_references;

mod fixed_kind_path_operations;
mod fixed_kind_tail_operations;

fn move_transform_type(guid: &str, version: u32) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType::try_new(
        "f3d:Design/MetaStream.dat:design-type#0".into(),
        crate::design::test_support::design_type(guid, None, version, "Fusion", vec![]),
    )
    .unwrap()
}

fn move_transform_types<'a>(
    row: &'a crate::records::entity_header::SegmentType,
    tags: &[u32],
) -> std::collections::HashMap<u32, &'a crate::records::entity_header::SegmentType> {
    tags.iter().map(|&tag| (tag, row)).collect()
}
