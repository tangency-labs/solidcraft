// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

#[test]
fn generated_source_less_writes_design_type_metastream() {
    use crate::records::entity_header::SegmentType;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#0".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 0,
                module: "Fusion".to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![1, 2]),
                type_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 7,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#1".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 1,
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
                type_guid: "22222222-3333-4444-5555-666666666666"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .expect("base GUID"),
                    offset: 0,
                },
                version: 9,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#2".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 2,
                module: "FutureFeature".to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![999]),
                type_guid: "33333333-4444-5555-6666-777777777777"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .expect("base GUID"),
                    offset: 0,
                },
                version: 11,
                version_offset: 0,
            },
        )
        .unwrap(),
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design MetaStream encode");
    let mut guid_module = source_less.clone();
    f3d_native_mut(&mut guid_module).design_types[2]
        .set_module("11111111-2222-3333-4444-555555555555".into());
    let error = F3dCodec
        .plan(EncodeInput::new(&guid_module, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a GUID-shaped Design module name must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type module name is GUID-shaped"));
    f3d_native_mut(&mut source_less).design_types[0].set_base_type_guid(
        crate::records::entity_header::BaseTypeGuid::Guid {
            value: "22222222-3333-4444-5555-666666666666"
                .to_owned()
                .try_into()
                .expect("base GUID"),
            offset: 0,
        },
    );
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a cyclic Design type hierarchy must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type hierarchy contains a cycle"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design MetaStream round trip");
    let types = &f3d_native(round_trip.ir()).design_types;
    assert_eq!(types.len(), 3);
    let fusion = types
        .iter()
        .find(|design_type| {
            design_type.type_guid.as_str() == "11111111-2222-3333-4444-555555555555"
        })
        .expect("Fusion type");
    assert_eq!(fusion.module, "Fusion");
    assert_eq!(
        fusion.entities.values().copied().collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(fusion.version, 7);
    assert_eq!(
        fusion.base_type_guid,
        crate::records::entity_header::BaseTypeGuid::Absent
    );
    let sketch = types
        .iter()
        .find(|design_type| {
            design_type.module == crate::records::entity_header::DESIGN_MODULE_SKETCH
        })
        .expect("sketch-module type");
    assert_eq!(sketch.entities.values().copied().collect::<Vec<_>>(), [277]);
    assert_eq!(
        sketch
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(sketch.version, 9);
    let future = types
        .iter()
        .find(|design_type| design_type.module == "FutureFeature")
        .expect("forward-compatible module");
    assert_eq!(future.entities.values().copied().collect::<Vec<_>>(), [999]);
    assert_eq!(future.version, 11);
}

#[test]
fn generated_source_less_writes_design_recipes_and_persistent_references() {
    use crate::records::{
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
        references::{LostEdgeReference, PersistentReference, PersistentReferenceKind},
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.construction_recipes = [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Face,
        ConstructionRecipeKind::BoundedFace,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, kind)| ConstructionRecipe {
        id: format!("f3d:generated:recipe#{ordinal}"),
        byte_offset: 0,
        kind,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: format!("{}", 320 + ordinal),
                offset: 0,
            },
            selector: None,
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 100 + i32::try_from(ordinal).unwrap(),
            offset: 0,
        }),
    })
    .collect();
    native.persistent_references = vec![
        PersistentReference {
            id: "f3d:generated:persistent-reference#0".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::Point,
            value: 900,
        },
        PersistentReference {
            id: "f3d:generated:persistent-reference#1".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurvePrimary,
            value: 100,
        },
        PersistentReference {
            id: "f3d:generated:persistent-reference#2".into(),
            byte_offset: 0,
            value_offset: 0,
            kind: PersistentReferenceKind::CurveSecondary,
            value: 500,
        },
    ];
    native.lost_edge_references = vec![LostEdgeReference::new(
        "f3d:generated:lost-edge-reference#0".into(),
        0,
        "419".into(),
        4645,
        "419".into(),
        4646,
    )
    .expect("valid lost-edge record layout")];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design BulkStream encode");
    f3d_native_mut(&mut source_less).construction_recipes[0].recipe_index = 1;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("recipe group indices must not be renumbered");
    assert!(error
        .to_string()
        .contains("has noncontiguous group index 1"));
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut bulkstream = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
        .expect("generated Design BulkStream")
        .read_to_end(&mut bulkstream)
        .expect("read generated Design BulkStream");
    for name in [
        b"body_recipe_data".as_slice(),
        b"face_recipe_data".as_slice(),
        b"bounded_face_recipe_data".as_slice(),
        b"edge_recipe_data".as_slice(),
        b"vertex_recipe_data".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated recipe name");
        assert_eq!(
            u32::from_le_bytes(bulkstream[offset - 4..offset].try_into().unwrap()),
            u32::try_from(name.len()).unwrap()
        );
        let payload = offset + name.len();
        assert_eq!(
            i64::from_le_bytes(bulkstream[payload..payload + 8].try_into().unwrap()),
            -1
        );
        assert_eq!(
            (0..5)
                .map(|ordinal| {
                    let at = payload + 8 + ordinal * 4;
                    i32::from_le_bytes(bulkstream[at..at + 4].try_into().unwrap())
                })
                .collect::<Vec<_>>(),
            [2, 0, -1, 1, -1]
        );
    }
    for name in [
        b"pt_tag".as_slice(),
        b"crv_primary_id".as_slice(),
        b"crv_secondary_id".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated persistent-reference name");
        let payload = offset + name.len();
        assert_eq!(
            &bulkstream[payload..payload + 8],
            &[2, 0, 0, 0, 14, 0, 0, 0]
        );
        assert_eq!(&bulkstream[payload + 8..payload + 22], &[0; 14]);
        assert_eq!(
            u32::from_le_bytes(bulkstream[payload + 22..payload + 26].try_into().unwrap()),
            23
        );
        assert_eq!(
            &bulkstream[payload + 26..payload + 49],
            b"IntrinsicMetaTypeuint64"
        );
    }
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.construction_recipes.len(), 5);
    let body_recipe = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::Body)
        .expect("body recipe");
    assert_eq!(body_recipe.record_index.map(|index| index.value), Some(100));
    assert_eq!(
        body_recipe
            .design
            .as_ref()
            .map(|design| design.id.value.as_str()),
        Some("320")
    );
    assert!(native
        .construction_recipes
        .iter()
        .any(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace));
    let bounded = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace)
        .expect("bounded-face recipe");
    assert_eq!(
        bounded
            .design
            .as_ref()
            .map(|design| design.id.value.as_str()),
        Some("322")
    );
    assert_eq!(bounded.record_index.map(|index| index.value), Some(102));
    assert_eq!(native.persistent_references.len(), 3);
    assert_eq!(
        native
            .persistent_references
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [900, 100, 500]
    );
    assert_eq!(
        native.persistent_references[1].kind,
        PersistentReferenceKind::CurvePrimary
    );
    assert_eq!(native.lost_edge_references.len(), 1);
    assert_eq!(native.lost_edge_references[0].class_tag.as_str(), "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4645);
    assert_eq!(
        native.lost_edge_references[0].next_class_tag.as_str(),
        "419"
    );
    assert_eq!(native.lost_edge_references[0].next_record_index, 4646);
}

#[test]
fn generated_source_less_writes_design_ownership_and_record_headers() {
    use crate::records::{
        bodies::DesignBodyMember,
        decal::DesignRecordHeader,
        entity_header::{DesignEntityHeader, SegmentType},
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![SegmentType::try_new(
        "f3d:generated/MetaStream.dat:design-type#0".into(),
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 0,
            module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
            type_guid: "22222222-3333-4444-5555-666666666666"
                .to_owned()
                .try_into()
                .expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 4,
            version_offset: 0,
        },
    )
    .unwrap()];
    native.design_body_members = vec![
        DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberWire {
            id: "f3d:generated:design-body-member#0".into(),
            byte_offset: 0,
            entity_suffix: 985,
            flags: 0,
        })
        .unwrap(),
        DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberWire {
            id: "f3d:generated:design-body-member#1".into(),
            byte_offset: 1,
            entity_suffix: 8422,
            flags: 3,
        })
        .unwrap(),
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated:entity-header#0".into(),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from("0_277".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: true,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(584),
                record_reference_offset: 0,
                references: vec![33, 44]
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    }];
    native.design_record_headers = vec![
        DesignRecordHeader {
            id: "f3d:generated:record-header#0".into(),
            record_index: 33,
            class_tag: crate::records::references::DesignClassTag::try_from("350".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
        DesignRecordHeader {
            id: "f3d:generated:record-header#1".into(),
            record_index: 44,
            class_tag: crate::records::references::DesignClassTag::try_from("351".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design ownership encode");
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.design_entity_headers[0].registration =
            crate::records::entity_header::DesignEntityRegistration::new(
                Some("Body".to_owned()),
                None,
                crate::records::identity::ReferenceRun::unlocated(Vec::new()),
            )
            .expect("body registration");
    }
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("cross-stream modules must not diverge");
    assert!(error
        .to_string()
        .contains("module conflicts with MetaStream ownership"));
    crate::test_support::zip_test::with_scan(&encoded, |scan| {
        let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let bulk = scan.entry_bytes(bulk_name).unwrap();
        let meta = scan
            .parsed_metastream(&bulk_name.replace("BulkStream.dat", "MetaStream.dat"))
            .unwrap();
        assert!(scan.unindexed_record_runs.is_empty());
        assert!(meta.secondary_records.is_empty());
        assert_eq!(meta.records.len(), 1);
        assert_eq!(meta.records[0].entity_id, 277);
        assert_eq!(meta.records[0].bulk_offset, 79);
        assert_eq!(&bulk[57..68], &[3, 0, 0, 0, b'3', b'5', b'0', 33, 0, 0, 0]);
        assert_eq!(&bulk[68..79], &[3, 0, 0, 0, b'3', b'5', b'1', 44, 0, 0, 0]);
        assert_eq!(&bulk[79..90], &[3, 0, 0, 0, b'2', b'5', b'6', 21, 1, 0, 0]);
        assert_eq!(bulk.len(), 153);
    });
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design ownership round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.design_body_members.len(), 2);
    assert_eq!(native.design_body_members[0].entity_suffix, 985);
    assert_eq!(native.design_body_members[1].flags, 3);
    assert_eq!(native.design_entity_headers.len(), 1);
    assert_eq!(native.design_entity_headers[0].entity_id.as_str(), "0_277");
    assert_eq!(
        native.design_entity_headers[0].declared_reference_count(),
        Some(2)
    );
    assert_eq!(
        native.design_entity_headers[0]
            .sketch_references()
            .and_then(|list| list.record_reference),
        Some(584)
    );
    assert_eq!(
        native.design_entity_headers[0]
            .reference_values()
            .copied()
            .collect::<Vec<_>>(),
        [33, 44]
    );
    assert_eq!(native.design_record_headers.len(), 2);
    assert_eq!(native.design_record_headers[0].record_index, 33);
    assert_eq!(native.design_record_headers[1].class_tag.as_str(), "351");
}

#[test]
fn generated_source_less_writes_sketch_points_curves_and_constraints() {
    use crate::records::{
        entity_header::{DesignEntityHeader, SegmentType},
        sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity, SketchPoint},
        sketch_relations::{SketchRelation, SketchRelationMember, SketchRelationReturnMember},
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#0".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 0,
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
                type_guid: crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 1,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#1".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 1,
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![33]),
                type_guid: "60403D47-0C49-49B0-BDE8-1679608164A2"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 1,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#2".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 2,
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![100]),
                type_guid: "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 11,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#3".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 3,
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![600]),
                type_guid: "DCA267ED-D615-4934-B64F-AD805E8003E2"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 2,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#4".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 4,
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![601]),
                type_guid: "F0130424-8B7E-4092-93C9-1CA807482534"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 0,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#5".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 5,
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![602]),
                type_guid: "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 3,
                version_offset: 0,
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#6".into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: 6,
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![101]),
                type_guid: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE
                    .0
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
                version_offset: 0,
            },
        )
        .unwrap(),
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated:sketch-header#0".into(),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from("0_277".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: true,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(584),
                record_reference_offset: 0,
                references: vec![33]
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    }];
    native.sketch_points =
        vec![
            SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
                id: "f3d:generated:sketch-point#0".into(),
                record_index: 100,
                owner_reference: Some(277),
                class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
                    .unwrap(),
                byte_offset: 0,
                coordinate_offset: 89,
                companion: crate::records::sketch_geometry::SketchPointCompanion {
                    incident_curves: Vec::new(),
                },
                record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                    500,
                    crate::records::sketch_geometry::SketchPointClosure::Selector0State1,
                    Some(900),
                    0.0,
                ),
                paired_reference: 101,
                coordinates: Point2::new(12.5, -25.0),
            })
            .unwrap(),
        ];
    native.sketch_curve_identities = vec![
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#0".into(),
            record_index: 600,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
                .unwrap(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: Some(901),
            primary_id: std::num::NonZeroU64::new(700).unwrap(),
            secondary_id: 701,
            geometry: Some(
                SketchCurveGeometry::line(
                    Point3::new(10.0, 20.0, 0.0),
                    Point3::new(40.0, 20.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            planar_z_noise: None,
        },
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#1".into(),
            record_index: 601,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
                .unwrap(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(702).unwrap(),
            secondary_id: 703,
            geometry: Some(
                SketchCurveGeometry::arc(
                    Point3::new(5.0, 6.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    30.0,
                    0.25,
                    2.5,
                )
                .unwrap(),
            ),
            planar_z_noise: None,
        },
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#2".into(),
            record_index: 602,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("261".to_owned())
                .unwrap(),
            byte_offset: 0,
            geometry_offset: 133,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(704).unwrap(),
            secondary_id: 705,
            geometry: Some(SketchCurveGeometry::nurbs_from_parts(
                None,
                crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
                602,
                crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
                    2,
                    1.0e-8,
                    8,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
                        vec![
                            Point3::new(0.0, 0.0, 0.0),
                            Point3::new(10.0, 20.0, 0.0),
                            Point3::new(30.0, 10.0, 0.0),
                        ],
                        vec![1.0, 0.8, 1.0],
                    )
                    .unwrap(),
                )
                .unwrap(),
            )),
            planar_z_noise: None,
        },
    ];
    native.sketch_relations =
        vec![
            SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
                id: "f3d:generated:sketch-relation#0".into(),
                record_index: 33,
                class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
                    .unwrap(),
                byte_offset: 0,
                state_offset: 0,
                owner_reference: 277,
                owner_entity_id: None,
                owner_reference_offset: 0,
                auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
                rectangular_counted_reference_count: None,
                members: (vec![
                    SketchRelationMember {
                        relation_ordinal: Some(0),
                        ..SketchRelationMember::from_index(100)
                    },
                    SketchRelationMember {
                        relation_ordinal: Some(0),
                        ..SketchRelationMember::from_index(600)
                    },
                ])
                .try_into()
                .expect("uniform member resolution"),
                definition: crate::records::sketch_relations::SketchRelationDefinition::new(
                    0x11, None,
                )
                .expect("valid relation definition"),
                entity_genesis: None,
                return_members: (vec![
                    SketchRelationReturnMember::from_index(600),
                    SketchRelationReturnMember::from_index(100),
                ])
                .try_into()
                .expect("uniform member resolution"),
                raw_bytes: vec![0; 160],
            })
            .unwrap(),
        ];

    let expected_geometries = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.geometry.clone().unwrap())
        .collect::<Vec<_>>();
    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less sketch BulkStream encode");
    let mut extended_source_less = source_less.clone();
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut bulkstream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
            .expect("generated Design BulkStream")
            .read_to_end(&mut bulkstream)
            .expect("read generated Design BulkStream");
        let mut companion = Vec::new();
        companion.extend_from_slice(&3u32.to_le_bytes());
        companion.extend_from_slice(b"262");
        companion.extend_from_slice(&101u32.to_le_bytes());
        companion.extend_from_slice(&[0; 15]);
        companion.push(1);
        companion.extend_from_slice(&100u64.to_le_bytes());
        companion.extend_from_slice(&[0; 2]);
        assert_eq!(companion.len(), 37);
        assert!(bulkstream
            .windows(companion.len())
            .any(|window| window == companion));
    }
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = None;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require their direct owner backlink");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = Some(277);
    f3d_native_mut(&mut source_less).design_types[6]
        .set_entities(crate::records::identity::ReferenceRun::unlocated(Vec::new()));
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require a registered inverse companion");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
    f3d_native_mut(&mut source_less).design_types[6]
        .set_entities(crate::records::identity::ReferenceRun::unlocated(vec![101]));
    f3d_native_mut(&mut source_less).design_types[2].set_version(10);
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("source-less points require the current writable class version");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    f3d_native_mut(&mut source_less).design_types[2].set_version(11);
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation
            .try_edit(|draft| {
                draft.members = ([100, 600, 100, 600, 100, 600, 100, 600]
                    .into_iter()
                    .map(|record_index| SketchRelationMember {
                        relation_ordinal: Some(0),
                        ..SketchRelationMember::from_index(record_index)
                    })
                    .collect::<Vec<_>>())
                .try_into()
                .expect("uniform member resolution")
            })
            .unwrap();
        relation
            .try_edit(|draft| {
                draft.return_members = (draft
                    .members
                    .iter()
                    .rev()
                    .map(|member| {
                        SketchRelationReturnMember::from_index(member.reference.record_index())
                    })
                    .collect::<Vec<_>>())
                .try_into()
                .expect("uniform member resolution")
            })
            .unwrap();
    }
    let mut variable_relation = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut variable_relation))
        .expect("source-less variable-width sketch relation encode");
    let variable_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(variable_relation),
            &DecodeOptions::default(),
        )
        .expect("source-less variable-width sketch relation round trip");
    assert_eq!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0].member_indices(),
        vec![100, 600, 100, 600, 100, 600, 100, 600]
    );
    assert!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0]
            .raw_bytes()
            .len()
            > 101
    );
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation
            .try_edit(|draft| {
                draft.members = (vec![
                    SketchRelationMember::from_index(100),
                    SketchRelationMember::from_index(600),
                ])
                .try_into()
                .expect("uniform member resolution")
            })
            .unwrap();
        relation
            .try_edit(|draft| {
                draft.return_members = (vec![
                    SketchRelationReturnMember::from_index(600),
                    SketchRelationReturnMember::from_index(100),
                ])
                .try_into()
                .expect("uniform member resolution")
            })
            .unwrap();
    }
    f3d_native_mut(&mut source_less).sketch_relations[0].owner_reference = 999;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("relations with missing sketch owners must not disappear");
    assert!(error
        .to_string()
        .contains("references missing sketch owner"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.sketch_relations[0].owner_reference = 277;
        native.sketch_points[0].record_index = 600;
    }
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate typed sketch indices must not be deduplicated");
    assert!(error.to_string().contains("share record index 600"));
    f3d_native_mut(&mut source_less).sketch_points[0].record_index = 100;
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id(), Some(500));
    assert_eq!(native.sketch_points[0].entity_genesis(), Some(900));
    assert_eq!(native.sketch_points[0].coordinate_offset, 141);
    assert_eq!(native.sketch_points[0].owner_reference, Some(277));
    assert_eq!(native.sketch_points[0].depth(), 0.0);
    assert_eq!(
        native.sketch_points[0].closure(),
        Some(crate::records::sketch_geometry::SketchPointClosure::Selector0State1)
    );
    assert_eq!(
        native.sketch_points[0].companion(),
        crate::records::sketch_geometry::SketchPointCompanionRef {
            prefix_present_zero: false,
            incident_curves: &[],
        }
    );
    assert_eq!(
        native.sketch_points[0].coordinates(),
        Point2::new(12.5, -25.0)
    );
    assert_eq!(native.sketch_curve_identities.len(), 3);
    let genesis_curve = native
        .sketch_curve_identities
        .iter()
        .find(|curve| curve.primary_id.get() == 700)
        .expect("genesis curve");
    assert_eq!(genesis_curve.entity_genesis, Some(901));
    assert_eq!(genesis_curve.geometry_offset, 185);
    assert_eq!(genesis_curve.owner_reference, Some(277));
    assert!(native
        .sketch_curve_identities
        .iter()
        .all(|curve| curve.owner_reference == Some(277)));
    for expected in expected_geometries {
        assert!(native
            .sketch_curve_identities
            .iter()
            .any(|curve| curve.geometry.as_ref() == Some(&expected)));
    }
    assert_eq!(native.sketch_relations.len(), 1);
    assert_eq!(native.sketch_relations[0].member_indices(), vec![100, 600]);
    assert!(native.sketch_relations[0].auxiliary_references().is_empty());
    assert_eq!(native.sketch_relations[0].owner_reference, 277);
    assert_eq!(
        native.sketch_relations[0]
            .owner_entity_id
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("0_277")
    );
    assert_eq!(native.sketch_relations[0].definition.state(), 0x11);
    assert_eq!(
        native.sketch_relations[0].return_member_indices(),
        vec![600, 100]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_members(),
        [
            crate::records::sketch_relations::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
            crate::records::sketch_relations::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
        ]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_return_members(),
        [
            crate::records::sketch_relations::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
            crate::records::sketch_relations::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
        ]
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());

    {
        let point = &mut f3d_native_mut(&mut extended_source_less).sketch_points[0];
        let persistent_id = std::num::NonZeroU64::new(point.persistent_id().unwrap()).unwrap();
        point
            .try_set_record_form(
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    depth: 7.5,
                    entity_genesis: point.entity_genesis(),
                    padded_paired_reference: true,
                    derived_type: false,
                    companion_prefix_present_zero: true,
                    persistent_id,
                    flags: [true, false, false, true, true, false, true, false],
                    closure: crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
                },
            )
            .unwrap();
        point
            .try_set_companion(crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: vec![600],
            })
            .unwrap();
    }
    let mut extended_encoded = Vec::new();
    F3dCodec
        .plan(
            EncodeInput::new(&extended_source_less, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut extended_encoded))
        .expect("source-less extended sketch point encode");
    let extended_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(extended_encoded),
            &DecodeOptions::default(),
        )
        .expect("source-less extended sketch point round trip");
    let extended_native = f3d_native(extended_round_trip.ir());
    let extended_point = &extended_native.sketch_points[0];
    assert_eq!(extended_point.depth(), 7.5);
    assert_eq!(extended_point.flags(), [1, 0, 0, 1, 1, 0, 1, 0]);
    assert_eq!(
        extended_point.closure(),
        Some(crate::records::sketch_geometry::SketchPointClosure::Selector4State0)
    );
    assert_eq!(
        extended_point.companion(),
        crate::records::sketch_geometry::SketchPointCompanionRef {
            prefix_present_zero: true,
            incident_curves: &[600],
        }
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, extended_round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());

    let mut invalid_zero_marker = extended_source_less.clone();
    {
        let mut zero_native = f3d_native_mut(&mut invalid_zero_marker);
        let point = &mut zero_native.sketch_points[0];
        point
            .try_set_record_form(
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    depth: 7.5,
                    entity_genesis: point.entity_genesis(),
                    padded_paired_reference: true,
                    derived_type: false,
                    companion_prefix_present_zero: true,
                    persistent_id: std::num::NonZeroU64::new(point.persistent_id().unwrap())
                        .unwrap(),
                    flags: [false, false, false, true, true, true, false, false],
                    closure: crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
                },
            )
            .unwrap();
    }
    let mut refused_output = Vec::new();
    let error = F3dCodec
        .plan(
            EncodeInput::new(&invalid_zero_marker, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut refused_output))
        .expect_err("a nonzero point cannot carry the zero-coordinate marker");
    assert!(error.to_string().contains("flag lane 5"));

    let mut inconsistent = round_trip.ir().clone();
    f3d_native_mut(&mut inconsistent).sketch_relations[0]
        .try_edit(|draft| {
            draft.members.resolve(|record_index| {
                crate::records::sketch_relations::SketchRelationOperand::Point {
                    record_index,
                    persistent_id: Some(u64::MAX),
                }
            });
        })
        .unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &inconsistent).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.message.contains("typed operands disagree")
    }));

    let mut points = native.sketch_points.clone();
    let mut curves = native.sketch_curve_identities.clone();
    let mut relations = native.sketch_relations.clone();
    let mut conflicting_relation = relations[0].clone();
    let relation_scope = relations[0]
        .id
        .rsplit_once(':')
        .expect("generated relation identity has a stream")
        .0;
    conflicting_relation.id = format!("{relation_scope}:sketch-relation-conflict#1");
    conflicting_relation.owner_reference = 278;
    relations.push(conflicting_relation);
    let mut entities = native.design_entity_headers.clone();
    let mut second_owner = entities[0].clone();
    let entity_scope = entities[0]
        .id
        .rsplit_once(':')
        .expect("generated entity identity has a stream")
        .0;
    second_owner.id = format!("{entity_scope}:sketch-header-conflict#1");
    second_owner.entity_id = crate::records::identity::DesignEntityId::from_parts("0", 278);
    entities.push(second_owner);
    let error = crate::design::decode::sketch::bind_sketch_graph(
        &entities,
        &mut points,
        &mut curves,
        &mut [],
        &mut relations,
    )
    .expect_err("typed sketch geometry cannot belong to two sketches");
    assert!(error.to_string().contains("belongs to multiple sketches"));
}
