// SPDX-License-Identifier: Apache-2.0
//! Which ACT component link names the document root.

use super::super::act_link_is_document_root;
use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
use crate::records::identity::ReferenceRun;

const ACT_STREAM: &str = "f3d:Asset[Active]/FusionACTSegmentType1/BulkStream.dat";

fn component_type(folder: &str, entities: Vec<u64>) -> SegmentType {
    SegmentType::try_new(
        format!("f3d:{folder}/Design1/MetaStream.dat:design-type#0"),
        SegmentTypeData {
            byte_offset: 0,
            type_guid: "E03784ED-5E19-4E14-B9F2-3B07017018CD"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: 4,
            base_type_guid: BaseTypeGuid::Absent,
            version: 1,
            version_offset: 44,
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(entities),
        },
    )
    .unwrap()
}

fn native(entities: Vec<u64>) -> crate::native::F3dNative {
    crate::native::F3dNative {
        design_types: vec![component_type("Asset[Active]", entities)],
        ..Default::default()
    }
}

#[test]
fn entity_three_is_the_document_root() {
    assert!(act_link_is_document_root(
        &native(vec![3, 308]),
        ACT_STREAM,
        3
    ));
    assert!(act_link_is_document_root(
        &crate::native::F3dNative::default(),
        ACT_STREAM,
        3
    ));
}

#[test]
fn another_component_is_the_root_only_when_entity_three_is_not_a_component() {
    let without_three = native(vec![203, 308]);
    assert!(act_link_is_document_root(&without_three, ACT_STREAM, 308));
    assert!(act_link_is_document_root(&without_three, ACT_STREAM, 203));
    assert!(!act_link_is_document_root(&without_three, ACT_STREAM, 4));
    assert!(!act_link_is_document_root(
        &native(vec![3, 308]),
        ACT_STREAM,
        308
    ));
}

#[test]
fn a_link_without_a_component_type_is_not_a_root() {
    assert!(!act_link_is_document_root(
        &crate::native::F3dNative::default(),
        ACT_STREAM,
        308
    ));
}

#[test]
fn component_types_of_another_asset_folder_do_not_count() {
    assert!(!act_link_is_document_root(
        &native(vec![308]),
        "f3d:Other[Active]/FusionACTSegmentType1/BulkStream.dat",
        308
    ));
}

#[test]
fn inserted_document_roots_use_their_complete_asset_scope() {
    let folders = [
        "xref/role-a/reference-1/occurrence-0/Asset[Active]",
        "xref/role-b/reference-2/occurrence-0/instance-parent/Asset[Active]",
        "xref/role-b/reference-2/occurrence-0/instance-sibling/Asset[Active]",
    ];
    for folder in folders {
        let stream = format!("f3d:{folder}/FusionACTSegmentType1/BulkStream.dat");
        for other in folders.into_iter().filter(|other| *other != folder) {
            let native = crate::native::F3dNative {
                design_types: vec![
                    component_type(folder, vec![203]),
                    component_type(other, vec![3, 308]),
                ],
                ..Default::default()
            };
            assert!(act_link_is_document_root(&native, &stream, 203));
            assert!(!act_link_is_document_root(&native, &stream, 308));
        }
    }
}
