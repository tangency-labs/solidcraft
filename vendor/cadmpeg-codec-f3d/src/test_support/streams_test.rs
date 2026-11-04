// SPDX-License-Identifier: Apache-2.0
//! Synthetic Design and ACT MetaStream/BulkStream payloads.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::u64_from_index;

use crate::test_support::component_body_test::{append_body_owners, body_type, container_type};
use crate::test_support::{
    indexed_header, lp_ascii, lp_utf16, push_marked_reference, push_reference_u64,
    write_indexed_header, write_marked_reference,
};

/// Build one Design `MetaStream` segment with an empty primary record index.
pub(crate) fn design_metastream(types: &[(&str, &str, u32, &str, &[u64])]) -> Vec<u8> {
    design_metastream_with_records(types, &[])
}

/// Build one Design `MetaStream` segment holding `types`, each entry a
/// `(type GUID, base type GUID, version, module, entity ids)` tuple, and the
/// ordered primary `(entity id, BulkStream offset)` record index. An empty base
/// GUID marks a root type. The segment carries the modern header shape and
/// closes on its own end.
pub(crate) fn design_metastream_with_records(
    types: &[(&str, &str, u32, &str, &[u64])],
    records: &[(u64, u64)],
) -> Vec<u8> {
    segment_metastream(
        "Design",
        "FusionDesignSegmentType",
        "Fusion",
        types,
        records,
    )
}

fn segment_metastream(
    short_name: &str,
    full_name: &str,
    add_in: &str,
    types: &[(&str, &str, u32, &str, &[u64])],
    records: &[(u64, u64)],
) -> Vec<u8> {
    let mut out = Vec::new();
    lp_ascii(&mut out, short_name);
    out.extend_from_slice(&0u32.to_le_bytes());
    let asset_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    out.extend_from_slice(
        &(u32::try_from(asset_guid.encode_utf16().count()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    for unit in asset_guid.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&1234u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    lp_ascii(&mut out, full_name);
    lp_ascii(&mut out, add_in);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(
        &(u32::try_from(types.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (type_guid, base_type_guid, version, module, entity_ids) in types {
        lp_ascii(&mut out, type_guid);
        lp_ascii(&mut out, base_type_guid);
        out.extend_from_slice(&version.to_le_bytes());
        lp_ascii(&mut out, module);
        out.extend_from_slice(
            &(u32::try_from(entity_ids.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for entity_id in *entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
        }
    }
    // Empty named-entity list, the primary record index, and an empty secondary
    // index, then the next-entity counter, the flag, and an empty property
    // block.
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(
        &(u32::try_from(records.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (entity_id, bulk_offset) in records {
        out.extend_from_slice(&entity_id.to_le_bytes());
        out.extend_from_slice(&bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    out
}

pub(super) fn generated_design_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(records, GeneratedDesignMetastreamVariant::Base)
}

pub(super) fn generated_design_sketch_dimension_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::SketchDimension,
    )
}

pub(super) fn generated_design_base_feature_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::BaseFeature,
    )
}

pub(super) fn generated_design_base_flange_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::BaseFlange,
    )
}

pub(super) fn generated_design_remove_body_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::RemoveBody,
    )
}

pub(super) fn generated_design_surface_stitch_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::SurfaceStitch,
    )
}

pub(super) fn generated_design_copy_paste_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::CopyPaste,
    )
}

pub(super) fn generated_design_copy_paste_bodies_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(
        records,
        GeneratedDesignMetastreamVariant::CopyPasteBodies,
    )
}

pub(super) fn generated_design_form_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    generated_design_metastream_with_sketch_types(records, GeneratedDesignMetastreamVariant::Form)
}

#[derive(Clone, Copy)]
pub(super) enum GeneratedDesignMetastreamVariant {
    Base,
    SketchDimension,
    BaseFeature,
    BaseFlange,
    RemoveBody,
    SurfaceStitch,
    CopyPaste,
    CopyPasteBodies,
    Form,
}

fn generated_design_metastream_with_sketch_types(
    records: &[(u64, u64)],
    variant: GeneratedDesignMetastreamVariant,
) -> Vec<u8> {
    design_metastream_with_records(&generated_design_types(variant), records)
}

type Text = &'static str;

pub(super) fn generated_design_types(
    variant: GeneratedDesignMetastreamVariant,
) -> Vec<(Text, Text, u32, Text, &'static [u64])> {
    let include_sketch_types = matches!(variant, GeneratedDesignMetastreamVariant::SketchDimension);
    let include_dimension_types =
        matches!(variant, GeneratedDesignMetastreamVariant::SketchDimension);
    let include_feature_types = matches!(
        variant,
        GeneratedDesignMetastreamVariant::BaseFeature
            | GeneratedDesignMetastreamVariant::BaseFlange
            | GeneratedDesignMetastreamVariant::RemoveBody
            | GeneratedDesignMetastreamVariant::SurfaceStitch
            | GeneratedDesignMetastreamVariant::CopyPaste
            | GeneratedDesignMetastreamVariant::CopyPasteBodies
            | GeneratedDesignMetastreamVariant::Form
    );
    let include_construction_types = matches!(
        variant,
        GeneratedDesignMetastreamVariant::RemoveBody
            | GeneratedDesignMetastreamVariant::SurfaceStitch
    );
    let base = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let point_entity_ids: &[u64] = if include_dimension_types {
        &[100, 200, 300, 400, 700, 1300, 1302]
    } else {
        &[100, 200, 300, 400, 700]
    };
    let point_companion_entity_ids: &[u64] = if include_dimension_types {
        &[101, 201, 301, 401, 701, 1301, 1303]
    } else {
        &[101, 201, 301, 401, 701]
    };
    let owner_entity_ids: &[u64] = if include_dimension_types {
        &[1200, 1203]
    } else {
        &[]
    };
    let parameter_entity_ids: &[u64] = if include_dimension_types {
        &[1201, 1204]
    } else {
        &[]
    };
    let companion_entity_ids: &[u64] = if include_dimension_types {
        &[1202, 1205]
    } else {
        &[]
    };
    let feature_entity_ids: &[u64] = if matches!(
        variant,
        GeneratedDesignMetastreamVariant::RemoveBody
            | GeneratedDesignMetastreamVariant::CopyPasteBodies
    ) {
        &[1400, 1600]
    } else if include_feature_types {
        &[1400]
    } else {
        &[]
    };
    let geometry_entity_ids: &[u64] = if include_construction_types
        || matches!(
            variant,
            GeneratedDesignMetastreamVariant::CopyPaste
                | GeneratedDesignMetastreamVariant::CopyPasteBodies
                | GeneratedDesignMetastreamVariant::Form
        ) {
        if matches!(variant, GeneratedDesignMetastreamVariant::CopyPasteBodies) {
            &[600, 1500, 1700]
        } else {
            &[600, 1500]
        }
    } else {
        &[600]
    };
    let mut types: Vec<(&str, &str, u32, &str, &[u64])> = vec![
        (
            crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
            crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
            crate::design::presentation::BODY_PRESENTATION_TYPE_VERSION,
            "Body",
            &[985],
        ),
        (
            crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
            base,
            4,
            "MSketch",
            &[277],
        ),
        (
            "33333333-4444-5555-6666-777777777777",
            "",
            5,
            "Dimension",
            &[270, 271],
        ),
        (
            "60403D47-0C49-49B0-BDE8-1679608164A2",
            base,
            1,
            "MSketch",
            &[33, 44],
        ),
        (
            crate::design::presentation::BROWSER_NODE_TYPE_GUID,
            crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID,
            crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[900],
        ),
        (
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            base,
            crate::design::presentation::BREP_CONTAINER_TYPE_VERSION,
            crate::records::entity_header::DESIGN_MODULE_BODY,
            &[7],
        ),
        (
            crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID,
            base,
            crate::design::presentation::BODY_SCENE_NODE_TYPE_VERSION,
            "Scene",
            &[986],
        ),
        (
            crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
            crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
            crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
            crate::records::entity_header::DESIGN_MODULE_BODY,
            &[899],
        ),
        (
            "F0130424-8B7E-4092-93C9-1CA807482534",
            base,
            0,
            "Geometry",
            geometry_entity_ids,
        ),
        (
            "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B",
            base,
            3,
            crate::records::entity_header::DESIGN_MODULE_SKETCH,
            &[800],
        ),
        (
            "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
            base,
            11,
            "Geometry",
            point_entity_ids,
        ),
        (
            crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.0,
            base,
            crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
            crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.2,
            point_companion_entity_ids,
        ),
    ];
    if include_sketch_types {
        types.push((
            "00000000-0000-0000-0000-000000001100",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_SKETCH,
            &[1100],
        ));
        types.push((
            "00000000-0000-0000-0000-000000000584",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_SKETCH,
            &[584],
        ));
    }
    if include_dimension_types {
        types.push((
            "00000000-0000-0000-0000-000000001200",
            base,
            1,
            "Dimension",
            owner_entity_ids,
        ));
        types.push((
            "00000000-0000-0000-0000-000000001201",
            base,
            1,
            "Dimension",
            parameter_entity_ids,
        ));
        types.push((
            "00000000-0000-0000-0000-000000001202",
            base,
            1,
            "Dimension",
            companion_entity_ids,
        ));
    }
    if include_feature_types {
        types.push((
            "00000000-0000-0000-0000-000000001400",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            feature_entity_ids,
        ));
    }
    if matches!(variant, GeneratedDesignMetastreamVariant::RemoveBody) {
        types.push((
            "00000000-0000-0000-0000-000000001401",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001402",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1601],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001403",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1602],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001404",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1603],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001405",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1604],
        ));
    } else if matches!(variant, GeneratedDesignMetastreamVariant::SurfaceStitch) {
        types.push((
            "00000000-0000-0000-0000-000000001401",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1600],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001402",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1700],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001403",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1701],
        ));
    } else if matches!(variant, GeneratedDesignMetastreamVariant::CopyPaste) {
        types.push((
            "00000000-0000-0000-0000-000000001401",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1600],
        ));
        types.push((
            "00000000-0000-0000-0000-000000001402",
            base,
            1,
            crate::records::entity_header::DESIGN_MODULE_FUSION,
            &[1601],
        ));
    }
    types
}

pub(super) fn generated_act_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    let entity_1 = [1];
    let entity_2 = [2];
    let entity_7 = [7];
    let entity_9 = records
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| ![1, 2, 7].contains(id))
        .collect::<Vec<_>>();
    let types = [
        (
            "00000000-0000-0000-0000-000000000100",
            "",
            1,
            "ACT",
            &entity_2[..],
        ),
        (
            "00000000-0000-0000-0000-000000000101",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000102",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000103",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000104",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000105",
            "",
            1,
            "ACT",
            &entity_7[..],
        ),
        (
            "00000000-0000-0000-0000-000000000106",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000107",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000108",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000109",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010a",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010b",
            "",
            1,
            "ACT",
            &entity_9[..],
        ),
        (
            "00000000-0000-0000-0000-00000000010c",
            "",
            1,
            "ACT",
            &entity_1[..],
        ),
    ];
    segment_metastream(
        "FusionACTSegmentType",
        "FusionACTSegmentType",
        "Fusion",
        &types,
        records,
    )
}

pub(super) fn generated_act_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let mut out = Vec::new();
    let mut records = vec![(2, u64_from_index(out.len()))];
    lp_ascii(&mut out, "256");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(b"decoy:ACTTable");
    out.extend_from_slice(&[0; 6]);
    records.push((1, u64_from_index(out.len())));
    lp_ascii(&mut out, "268");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut out, "ACTTable");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, "eeeeeeee-1111-2222-3333-ffffffffffff");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    records.push((9, u64_from_index(out.len())));
    lp_ascii(&mut out, "267");
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.push(1);
    out.extend_from_slice(&12u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_3");
    out.push(1);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut out, "(Unsaved)");
    out.push(0);
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    records.push((7, u64_from_index(out.len())));
    lp_ascii(&mut out, "261");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    lp_utf16(&mut out, "0_985");
    (out, records)
}

pub(super) fn generated_design_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn reference(relation: &mut [u8], at: usize, target: u32) {
        relation[at] = 1;
        relation[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
    }

    fn close_current_point(out: &mut Vec<u8>, paired_reference: u32, owner_reference: u32) {
        out.extend_from_slice(&[0; 16]);
        out.push(1);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0]);
        push_reference_u64(out, u64::from(paired_reference));
        push_reference_u64(out, u64::from(owner_reference));
    }

    fn push_point_companion(
        out: &mut Vec<u8>,
        class_tag: &str,
        record_index: u32,
        point_record_index: u32,
    ) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; 15]);
        push_reference_u64(out, u64::from(point_record_index));
    }

    let mut out = Vec::new();
    let mut records = vec![(899, 0)];
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"263");
    out.extend_from_slice(&899u32.to_le_bytes());
    out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&42u64.to_le_bytes());
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&1793u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut out, "BREP.synthetic.smbh");
    records.push((
        985,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let node_guid = "ABCD0000-1111-8222-A333-444444444444";
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"256");
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    lp_utf16(&mut out, "99999999-8888-8777-A666-555555555555");
    lp_utf16(
        &mut out,
        crate::design::presentation::PHYSICAL_MATERIAL_LIBRARY_ID,
    );
    lp_utf16(&mut out, "PrismMaterial-018");
    push_reference_u64(&mut out, 7);
    out.push(0);
    push_reference_u64(&mut out, 986);
    lp_utf16(&mut out, "Body");
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&[1, 1]);
    lp_utf16(&mut out, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut out, crate::design::presentation::APPEARANCE_LIBRARY_ID);
    lp_utf16(&mut out, "Prism-001");
    records.push((
        900,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"260");
    out.extend_from_slice(&900u32.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[0, 1, 1]);
    out.extend_from_slice(&985u64.to_le_bytes());
    records.push((
        277,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"257");
    out.extend_from_slice(&277u64.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&[0u8; 4]);
    lp_utf16(&mut out, "0_277");
    out.extend_from_slice(&584u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&2u32.to_le_bytes());
    for reference in [33u32, 44] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }
    for (class_tag, record_index, members) in
        [("259", 33u32, [100u32, 200u32]), ("259", 44, [300, 400])]
    {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        // A relation is `u8 1`, the counted `(reference, relation ordinal)`
        // pairs, the property-block presence byte, `ParentNode`, the u64
        // mask, the counted return run, and one zero byte.
        let mut relation = vec![0u8; 101];
        relation[0..4].copy_from_slice(&3u32.to_le_bytes());
        relation[4..7].copy_from_slice(class_tag.as_bytes());
        relation[7..11].copy_from_slice(&record_index.to_le_bytes());
        relation[19] = 1;
        relation[20..24].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 24, members[0]);
        reference(&mut relation, 39, members[1]);
        reference(&mut relation, 55, 277);
        let state = if record_index == 33 { 0x10u64 } else { 0x04 };
        relation[66..74].copy_from_slice(&state.to_le_bytes());
        relation[74..78].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 78, members[1]);
        reference(&mut relation, 89, members[0]);
        out.extend_from_slice(&relation);
    }
    for (record_index, persistent_id, coordinates) in [
        (100u32, 500u64, [1.25f64, -2.5f64]),
        (200, 501, [3.0, 4.0]),
        (300, 502, [-1.0, 0.5]),
        (400, 503, [2.0, 1.0]),
    ] {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        let paired_reference = record_index + 1;
        let mut point = vec![0u8; 105];
        point[0..4].copy_from_slice(&3u32.to_le_bytes());
        point[4..7].copy_from_slice(b"266");
        point[7..11].copy_from_slice(&record_index.to_le_bytes());
        point[20] = 1;
        point[21..25].copy_from_slice(&1u32.to_le_bytes());
        point[25..29].copy_from_slice(&6u32.to_le_bytes());
        point[29..35].copy_from_slice(b"pt_tag");
        point[35..39].copy_from_slice(&23u32.to_le_bytes());
        point[39..62].copy_from_slice(b"IntrinsicMetaTypeuint64");
        point[62..70].copy_from_slice(&persistent_id.to_le_bytes());
        point[70] = 1;
        point[71..75].copy_from_slice(&paired_reference.to_le_bytes());
        point[89..97].copy_from_slice(&coordinates[0].to_le_bytes());
        point[97..105].copy_from_slice(&coordinates[1].to_le_bytes());
        close_current_point(&mut point, paired_reference, 277);
        out.extend_from_slice(&point);
        records.push((
            u64::from(paired_reference),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        push_point_companion(&mut out, "267", paired_reference, record_index);
    }
    records.push((
        600,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut curve = vec![0u8; 229];
    curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    curve[4..7].copy_from_slice(b"264");
    curve[7..11].copy_from_slice(&600u32.to_le_bytes());
    curve[20] = 1;
    curve[21..25].copy_from_slice(&2u32.to_le_bytes());
    curve[25..29].copy_from_slice(&14u32.to_le_bytes());
    curve[29..43].copy_from_slice(b"crv_primary_id");
    curve[43..47].copy_from_slice(&23u32.to_le_bytes());
    curve[47..70].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[70..78].copy_from_slice(&440u64.to_le_bytes());
    curve[78..82].copy_from_slice(&16u32.to_le_bytes());
    curve[82..98].copy_from_slice(b"crv_secondary_id");
    curve[98..102].copy_from_slice(&23u32.to_le_bytes());
    curve[102..125].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[125..133].copy_from_slice(&0u64.to_le_bytes());
    for (ordinal, value) in [
        1.0f64,
        2.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        3.0,
        0.0,
        std::f64::consts::PI,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 133 + ordinal * 8;
        curve[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    push_reference_u64(&mut curve, 277);
    out.extend_from_slice(&curve);
    records.push((
        700,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_point = vec![0u8; 157];
    alternate_point[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_point[4..7].copy_from_slice(b"266");
    alternate_point[7..11].copy_from_slice(&700u32.to_le_bytes());
    alternate_point[20] = 1;
    alternate_point[21..25].copy_from_slice(&2u32.to_le_bytes());
    alternate_point[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_point[29..42].copy_from_slice(b"EntityGenesis");
    alternate_point[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[69..77].copy_from_slice(&9u64.to_le_bytes());
    alternate_point[77..81].copy_from_slice(&6u32.to_le_bytes());
    alternate_point[81..87].copy_from_slice(b"pt_tag");
    alternate_point[87..91].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[91..114].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[114..122].copy_from_slice(&600u64.to_le_bytes());
    alternate_point[122] = 1;
    alternate_point[123..127].copy_from_slice(&701u32.to_le_bytes());
    alternate_point[141..149].copy_from_slice(&(-4.0f64).to_le_bytes());
    alternate_point[149..157].copy_from_slice(&5.0f64.to_le_bytes());
    close_current_point(&mut alternate_point, 701, 277);
    out.extend_from_slice(&alternate_point);
    records.push((
        701,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    push_point_companion(&mut out, "267", 701, 700);

    records.push((
        800,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_curve = vec![0u8; 443];
    alternate_curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[4..7].copy_from_slice(b"265");
    alternate_curve[7..11].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[20] = 1;
    alternate_curve[21..25].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_curve[29..42].copy_from_slice(b"EntityGenesis");
    alternate_curve[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[69..77].copy_from_slice(&10u64.to_le_bytes());
    alternate_curve[77..81].copy_from_slice(&14u32.to_le_bytes());
    alternate_curve[81..95].copy_from_slice(b"crv_primary_id");
    alternate_curve[95..99].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[99..122].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[122..130].copy_from_slice(&700u64.to_le_bytes());
    alternate_curve[130..134].copy_from_slice(&16u32.to_le_bytes());
    alternate_curve[134..150].copy_from_slice(b"crv_secondary_id");
    alternate_curve[150..154].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[154..177].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[177..185].copy_from_slice(&0u64.to_le_bytes());
    alternate_curve[185..193].copy_from_slice(&42u64.to_le_bytes());
    alternate_curve[193..197].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[197..200].copy_from_slice(b"365");
    alternate_curve[200..204].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[273] = 1;
    alternate_curve[275..279].copy_from_slice(&2u32.to_le_bytes());
    alternate_curve[279..287].copy_from_slice(&1.0e-9f64.to_le_bytes());
    alternate_curve[287..291].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[291..295].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[295..299].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, knot) in [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
        let offset = 299 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&knot.to_le_bytes());
    }
    alternate_curve[347..351].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[351..355].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[355..359].copy_from_slice(&8u32.to_le_bytes());
    alternate_curve[359..363].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[363..367].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[367..371].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, coordinate) in [0.0f64, 0.0, 0.0, 1.0, 2.0, 0.0, 3.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 371 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    push_reference_u64(&mut alternate_curve, 277);
    out.extend_from_slice(&alternate_curve);
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&2u32.to_le_bytes());
    for entity_suffix in [985u64, 8422] {
        out.push(1);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.push(0);
    let mut recipe_prefix = vec![0u8; 27];
    recipe_prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    recipe_prefix[4..7].copy_from_slice(b"322");
    recipe_prefix[11..15].copy_from_slice(&123i32.to_le_bytes());
    recipe_prefix[23..27].copy_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&recipe_prefix);
    out.extend_from_slice(b"body_recipe_data");
    out.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [2i32, 0, -1, 1, -1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(b"pt_tag");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&439u64.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4645u32.to_le_bytes());
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(&19u32.to_le_bytes());
    out.extend_from_slice(b"EDGE_REFERENCE_LOST");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4646u32.to_le_bytes());
    out.extend_from_slice(b"body_recipe_data");
    (out, records)
}

/// Add a localized Sketch scope and its identity placement carrier to the
/// generated Design stream. The existing stream already carries the sketch
/// entity header, points, curves, and relations; this variant closes the
/// scope-to-placement join so the normal projection pipeline can materialize
/// that graph as a neutral Sketch.
fn generated_design_sketch_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();

    let scope_record = 1_100_u32;
    let placement_record = 584_u32;
    let scope_class_tag = 268_u32;
    let placement_class_tag = 269_u32;

    let scope_offset = u64::try_from(out.len()).expect("synthetic Sketch scope offset");
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(scope_class_tag.to_string().as_bytes());
    out.extend_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&2_u32.to_le_bytes());
    for reference in [277_u32, placement_record] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
    }
    out.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut out, "Sketch");
    let mut tail = [0_u8; 78];
    tail[..4].copy_from_slice(&1_u32.to_le_bytes());
    tail[30..34].copy_from_slice(&u32::MAX.to_le_bytes());
    out.extend_from_slice(&tail);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(scope_class_tag.to_string().as_bytes());
    out.extend_from_slice(&scope_record.to_le_bytes());
    records.push((u64::from(scope_record), scope_offset));

    let placement_offset = u64::try_from(out.len()).expect("synthetic Sketch placement offset");
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(placement_class_tag.to_string().as_bytes());
    out.extend_from_slice(&placement_record.to_le_bytes());
    out.extend_from_slice(&[0; 190]);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"261");
    out.extend_from_slice(&placement_record.to_le_bytes());
    records.push((u64::from(placement_record), placement_offset));

    (out, records)
}

/// Add a typed `BaseFlange` scope, its profile group, and the sketch-profile
/// placement join to the generated Design stream.
pub(crate) fn generated_design_base_flange_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn lp_utf16(bytes: &mut [u8], at: usize, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        bytes[at..at + 4].copy_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
        for (ordinal, unit) in units.into_iter().enumerate() {
            let offset = at + 4 + ordinal * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let profile_group_record = 2_000_u32;
    let profile_record = 1_501_u32;
    let thickness_record = 1_502_u32;
    let settings_record = 1_503_u32;

    let placement_head_record = 2_100_u32;
    indexed_header(&mut out, *b"261", 800);
    out.extend_from_slice(&[0; 8]);
    out.push(1);
    out.extend_from_slice(&placement_head_record.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    let mut placement_head = Vec::new();
    indexed_header(&mut placement_head, *b"264", placement_head_record);
    placement_head.extend_from_slice(&[0; 10]);
    placement_head.extend_from_slice(&[1, 0, 1]);
    placement_head.extend_from_slice(&[0; 4]);
    placement_head.extend_from_slice(&[0; 6]);
    assert_eq!(placement_head.len(), 34);
    out.extend_from_slice(&placement_head);

    let scope_offset = u64::try_from(out.len()).expect("synthetic BaseFlange scope offset");
    let mut scope = vec![0_u8; 416];
    write_indexed_header(&mut scope, *b"268", scope_record);
    scope[73..77].copy_from_slice(&1_u32.to_le_bytes());
    scope[81] = 1;
    scope[82..86].copy_from_slice(&settings_record.to_le_bytes());
    scope[92..96].copy_from_slice(&1_u32.to_le_bytes());
    scope[112] = 1;
    scope[113..117].copy_from_slice(&thickness_record.to_le_bytes());
    scope[123..131].copy_from_slice(&0.2_f64.to_le_bytes());
    scope[141..145].copy_from_slice(&1_u32.to_le_bytes());
    scope[145] = 1;
    scope[146..150].copy_from_slice(&profile_group_record.to_le_bytes());
    scope[258..262].copy_from_slice(&4_u32.to_le_bytes());
    for (ordinal, record_index) in [
        profile_group_record,
        profile_record,
        thickness_record,
        settings_record,
    ]
    .into_iter()
    .enumerate()
    {
        write_marked_reference(&mut scope, 262 + ordinal * 11, record_index);
    }
    scope[306..310].copy_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut scope, 310, "BaseFlange");
    scope[334..338].copy_from_slice(&1_u32.to_le_bytes());
    indexed_header(&mut scope, *b"261", scope_record);
    assert_eq!(scope.len(), 427);
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let mut group = Vec::new();
    indexed_header(&mut group, *b"264", profile_group_record);
    group.extend_from_slice(&[0; 8]);
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&u64::from(profile_record).to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&0_u32.to_le_bytes());
    group.extend_from_slice(&0x0000_0041_0000_0000_u64.to_le_bytes());
    group.extend_from_slice(&[0; 10]);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.extend_from_slice(&1.0_f64.to_le_bytes());
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&u64::from(profile_group_record + 2).to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut group, u64::from(profile_group_record + 1));
    group.push(0);
    push_reference_u64(&mut group, u64::from(scope_record));
    indexed_header(&mut group, *b"259", profile_group_record);
    out.extend_from_slice(&group);

    let mut profile = Vec::new();
    indexed_header(&mut profile, *b"377", profile_record);
    profile.extend_from_slice(&[0; 10]);
    profile.push(1);
    profile.extend_from_slice(&(profile_record + 3).to_le_bytes());
    profile.extend_from_slice(&[0; 6]);
    profile.extend_from_slice(&1_u32.to_le_bytes());
    let asset = "11111111-2222-3333-4444-555555555555";
    let asset_units: Vec<u16> = asset.encode_utf16().collect();
    profile.extend_from_slice(&u32::try_from(asset_units.len()).unwrap().to_le_bytes());
    for unit in asset_units {
        profile.extend_from_slice(&unit.to_le_bytes());
    }
    let suffix_units: Vec<u16> = "800".encode_utf16().collect();
    profile.extend_from_slice(&u32::try_from(suffix_units.len()).unwrap().to_le_bytes());
    for unit in suffix_units {
        profile.extend_from_slice(&unit.to_le_bytes());
    }
    let tail_offset = profile.len();
    let tail = vec![0_u8; 94];
    profile.extend_from_slice(&tail);
    indexed_header(&mut profile, *b"264", profile_record);
    assert_eq!(tail_offset + 94, profile.len() - 11);
    out.extend_from_slice(&profile);

    let mut thickness = vec![0_u8; 100];
    write_indexed_header(&mut thickness, *b"270", thickness_record);
    thickness[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    thickness[24] = 1;
    thickness[25..29].copy_from_slice(&scope_record.to_le_bytes());
    thickness[40..48].copy_from_slice(&0.2_f64.to_le_bytes());
    indexed_header(&mut thickness, *b"270", thickness_record);
    out.extend_from_slice(&thickness);

    let mut settings = Vec::new();
    indexed_header(&mut settings, *b"271", settings_record);
    settings.extend_from_slice(&[0; 20]);
    out.extend_from_slice(&settings);

    (out, records)
}

/// Add the self-contained 267-byte result-body form of a `Base Feature`
/// parameter scope to the generated Design stream.
pub(super) fn generated_design_base_feature_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let scope_class_tag = 268_u32;
    let scope_offset = u64::try_from(out.len()).expect("synthetic Base Feature scope offset");

    let mut scope = Vec::new();
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(scope_class_tag.to_string().as_bytes());
    scope.extend_from_slice(&scope_record.to_le_bytes());
    scope.extend_from_slice(&[0; 10]);
    scope.resize(138, 0);
    scope.extend_from_slice(&1_u32.to_le_bytes());
    scope.push(1);
    scope.extend_from_slice(&985_u32.to_le_bytes());
    scope.extend_from_slice(&[0; 6]);
    scope.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut scope, "Base Feature");
    scope.extend_from_slice(&1_u32.to_le_bytes());
    scope.extend_from_slice(&[0; 78]);
    assert_eq!(scope.len(), 267);
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(b"261");
    scope.extend_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    (out, records)
}

/// Add a `RemoveBody` scope and its single whole-body construction group to
/// the generated Design stream.
pub(super) fn generated_design_remove_body_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let group_record = 1_500_u32;
    let scope_offset = u64::try_from(out.len()).expect("synthetic RemoveBody scope offset");

    let mut scope = Vec::new();
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(b"268");
    scope.extend_from_slice(&scope_record.to_le_bytes());
    scope.extend_from_slice(&[0; 10]);
    scope.extend_from_slice(&2_u32.to_le_bytes());
    for reference in [u64::from(group_record), 1600] {
        scope.push(1);
        scope.extend_from_slice(&reference.to_le_bytes());
        scope.extend_from_slice(&[0, 0]);
    }
    scope.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut scope, "RemoveBody");
    scope.extend_from_slice(&1_u32.to_le_bytes());
    scope.extend_from_slice(&[0; 78]);
    assert_eq!(scope.len(), 157);
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(b"261");
    scope.extend_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let group_offset = u64::try_from(out.len()).expect("synthetic RemoveBody group offset");
    let mut group = Vec::new();
    group.extend_from_slice(&3_u32.to_le_bytes());
    group.extend_from_slice(b"264");
    group.extend_from_slice(&group_record.to_le_bytes());
    group.extend_from_slice(&[0; 8]);
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&1600_u64.to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&0_u32.to_le_bytes());
    group.extend_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    group.extend_from_slice(&[0; 10]);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.extend_from_slice(&1.0_f64.to_le_bytes());
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&u64::from(group_record + 2).to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut group, u64::from(group_record + 1));
    group.push(0);
    push_reference_u64(&mut group, u64::from(scope_record));
    group.extend_from_slice(&3_u32.to_le_bytes());
    group.extend_from_slice(b"259");
    group.extend_from_slice(&group_record.to_le_bytes());
    out.extend_from_slice(&group);
    records.push((u64::from(group_record), group_offset));

    let operand_offset = u64::try_from(out.len()).expect("synthetic body operand offset");
    let mut operand = Vec::new();
    operand.extend_from_slice(&3_u32.to_le_bytes());
    operand.extend_from_slice(b"268");
    operand.extend_from_slice(&1600_u32.to_le_bytes());
    operand.extend_from_slice(&[0; 10]);
    operand.extend_from_slice(&1_u32.to_le_bytes());
    operand.extend_from_slice(&985_u64.to_le_bytes());
    operand.extend_from_slice(&3_u32.to_le_bytes());
    operand.push(1);
    operand.extend_from_slice(&1603_u64.to_le_bytes());
    operand.extend_from_slice(&[0, 0]);
    operand.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut operand, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut operand, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    operand.extend_from_slice(&2_u32.to_le_bytes());
    operand.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&operand);
    records.push((1600, operand_offset));

    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"269");
    out.extend_from_slice(&1600_u32.to_le_bytes());

    for (record_index, class_tag) in [
        (1601_u32, "270"),
        (1602_u32, "271"),
        (1603_u32, "272"),
        (1604_u32, "273"),
    ] {
        let record_offset = u64::try_from(out.len()).expect("synthetic body operand header");
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&record_index.to_le_bytes());
        records.push((u64::from(record_index), record_offset));
        if record_index == 1603 {
            out.extend_from_slice(&4_u32.to_le_bytes());
            out.extend_from_slice(b"1603");
            out.extend_from_slice(&1604_u32.to_le_bytes());
            out.extend_from_slice(&[0; 12]);
            out.extend_from_slice(&16_u32.to_le_bytes());
            out.extend_from_slice(b"body_recipe_data");
        }
    }

    (out, records)
}

/// Add a `SurfaceStitch` scope, one face-selection group, and its tolerance
/// and settings records to the generated Design stream.
pub(super) fn generated_design_surface_stitch_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let group_record = 1_500_u32;
    let selection_record = 1_600_u32;
    let tolerance_record = 1_700_u32;
    let settings_record = 1_701_u32;

    let scope_offset = u64::try_from(out.len()).expect("synthetic SurfaceStitch scope offset");
    let mut scope = Vec::new();
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(b"268");
    scope.extend_from_slice(&scope_record.to_le_bytes());
    scope.extend_from_slice(&[0; 10]);
    scope.extend_from_slice(&4_u32.to_le_bytes());
    for reference in [
        u64::from(group_record),
        u64::from(selection_record),
        u64::from(tolerance_record),
        u64::from(settings_record),
    ] {
        push_reference_u64(&mut scope, reference);
    }
    scope.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut scope, "SurfaceStitch");
    scope.extend_from_slice(&1_u32.to_le_bytes());
    scope.extend_from_slice(&[0; 78]);
    assert_eq!(scope.len(), 185);
    scope.extend_from_slice(&3_u32.to_le_bytes());
    scope.extend_from_slice(b"261");
    scope.extend_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let group_offset = u64::try_from(out.len()).expect("synthetic SurfaceStitch group offset");
    let mut group = Vec::new();
    group.extend_from_slice(&3_u32.to_le_bytes());
    group.extend_from_slice(b"264");
    group.extend_from_slice(&group_record.to_le_bytes());
    group.extend_from_slice(&[0; 8]);
    group.push(0);
    group.push(1);
    group.extend_from_slice(&1_u32.to_le_bytes());
    let property_name = b"SurfaceStitchGapSettingX";
    assert_eq!(property_name.len(), 24);
    group.extend_from_slice(
        &(u32::try_from(property_name.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    group.extend_from_slice(property_name);
    let property_type = b"IntrinsicMetaTypeuint64";
    assert_eq!(property_type.len(), 23);
    group.extend_from_slice(
        &(u32::try_from(property_type.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    group.extend_from_slice(property_type);
    group.extend_from_slice(&[0; 8]);
    assert_eq!(group.len(), 88);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&u64::from(selection_record).to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&0_u32.to_le_bytes());
    group.extend_from_slice(&0x0000_0005_0000_0000_u64.to_le_bytes());
    group.extend_from_slice(&[0; 10]);
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.extend_from_slice(&1.0_f64.to_le_bytes());
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.push(1);
    group.extend_from_slice(&u64::from(group_record + 2).to_le_bytes());
    group.extend_from_slice(&[0, 0]);
    group.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut group, u64::from(group_record + 1));
    group.push(0);
    push_reference_u64(&mut group, u64::from(scope_record));
    group.extend_from_slice(&3_u32.to_le_bytes());
    group.extend_from_slice(b"259");
    group.extend_from_slice(&group_record.to_le_bytes());
    out.extend_from_slice(&group);
    records.push((u64::from(group_record), group_offset));

    let selection_offset = u64::try_from(out.len()).expect("synthetic selection operand offset");
    let mut selection = Vec::new();
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"269");
    selection.extend_from_slice(&selection_record.to_le_bytes());
    selection.extend_from_slice(&[0; 10]);
    selection.push(1);
    selection.extend_from_slice(&(selection_record + 3).to_le_bytes());
    selection.extend_from_slice(&[0; 6]);
    selection.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut selection, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut selection, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    selection.extend_from_slice(&2_u32.to_le_bytes());
    selection.extend_from_slice(&[0; 4]);
    selection.extend_from_slice(&[1, 0, 0]);
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"269");
    selection.extend_from_slice(&selection_record.to_le_bytes());
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"270");
    selection.extend_from_slice(&(selection_record + 1).to_le_bytes());
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"271");
    selection.extend_from_slice(&(selection_record + 2).to_le_bytes());
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"272");
    selection.extend_from_slice(&(selection_record + 3).to_le_bytes());
    selection.extend_from_slice(&[0; 10]);
    selection.extend_from_slice(&42_u64.to_le_bytes());
    selection.extend_from_slice(&3_u32.to_le_bytes());
    selection.extend_from_slice(b"273");
    selection.extend_from_slice(&(selection_record + 4).to_le_bytes());
    out.extend_from_slice(&selection);
    records.push((u64::from(selection_record), selection_offset));

    let tolerance_offset = u64::try_from(out.len()).expect("synthetic tolerance offset");
    let mut tolerance = vec![0_u8; 100];
    tolerance[0..4].copy_from_slice(&3_u32.to_le_bytes());
    tolerance[4..7].copy_from_slice(b"270");
    tolerance[7..11].copy_from_slice(&tolerance_record.to_le_bytes());
    tolerance[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    tolerance[24] = 1;
    tolerance[25..29].copy_from_slice(&scope_record.to_le_bytes());
    tolerance[40..48].copy_from_slice(&0.01_f64.to_le_bytes());
    out.extend_from_slice(&tolerance);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"270");
    out.extend_from_slice(&tolerance_record.to_le_bytes());
    records.push((u64::from(tolerance_record), tolerance_offset));

    let settings_offset = u64::try_from(out.len()).expect("synthetic settings offset");
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"271");
    out.extend_from_slice(&settings_record.to_le_bytes());
    out.extend_from_slice(&[0; 20]);
    records.push((u64::from(settings_record), settings_offset));

    (out, records)
}

/// Add two local component occurrences, their copy relation, and a `CopyPaste`
/// scope with the long transform-bearing frame.
pub(super) fn generated_design_copy_paste_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    const COMPONENT_GUID: &str = "11111111-2222-3333-4444-555555555555";
    const SOURCE_OCCURRENCE_GUID: &str = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
    const COPIED_OCCURRENCE_GUID: &str = "cccccccc-1111-2222-3333-dddddddddddd";

    fn guid(bytes: &mut [u8], at: usize, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        assert_eq!(units.len(), 36);
        bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
        for (ordinal, unit) in units.into_iter().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn transform(bytes: &mut [u8], at: usize, translation: [f64; 3]) {
        let values = [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        for (ordinal, value) in values.into_iter().enumerate() {
            bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }

    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let relation_record = 1_500_u32;
    let source_record = 1_600_u32;
    let copied_record = 1_601_u32;
    let component_record = 1_800_u64;

    let source_offset = u64::try_from(out.len()).expect("synthetic source occurrence offset");
    let mut source = vec![0_u8; 229];
    write_indexed_header(&mut source, *b"269", source_record);
    source[19] = 1;
    source[20..24].copy_from_slice(&1_u32.to_le_bytes());
    source[24] = 1;
    source[25..33].copy_from_slice(&component_record.to_le_bytes());
    source[40..44].copy_from_slice(&1_u32.to_le_bytes());
    guid(&mut source, 44, COMPONENT_GUID);
    guid(&mut source, 120, SOURCE_OCCURRENCE_GUID);
    source[197] = 1;
    source[198..206].copy_from_slice(&component_record.to_le_bytes());
    source[208] = 1;
    source[218] = 1;
    out.extend_from_slice(&source);
    records.push((u64::from(source_record), source_offset));

    let copied_offset = u64::try_from(out.len()).expect("synthetic copied occurrence offset");
    let mut copied = vec![0_u8; 357];
    write_indexed_header(&mut copied, *b"270", copied_record);
    copied[19] = 1;
    copied[20..24].copy_from_slice(&1_u32.to_le_bytes());
    copied[24] = 1;
    copied[25..33].copy_from_slice(&component_record.to_le_bytes());
    copied[40..44].copy_from_slice(&2_u32.to_le_bytes());
    guid(&mut copied, 44, COMPONENT_GUID);
    guid(&mut copied, 120, COPIED_OCCURRENCE_GUID);
    copied[197] = 1;
    copied[198..206].copy_from_slice(&component_record.to_le_bytes());
    transform(&mut copied, 209, [2.0, 3.0, 4.0]);
    copied[346] = 1;
    out.extend_from_slice(&copied);
    records.push((u64::from(copied_record), copied_offset));

    let relation_offset = u64::try_from(out.len()).expect("synthetic CopyPaste relation offset");
    let mut relation = vec![0_u8; 57];
    write_indexed_header(&mut relation, *b"264", relation_record);
    relation[21] = 1;
    relation[22..26].copy_from_slice(&copied_record.to_le_bytes());
    relation[34] = 1;
    relation[35..39].copy_from_slice(&source_record.to_le_bytes());
    relation[46] = 1;
    relation[47..51].copy_from_slice(&scope_record.to_le_bytes());
    out.extend_from_slice(&relation);
    records.push((u64::from(relation_record), relation_offset));

    let mut relation_pair = [0_u8; 11];
    write_indexed_header(&mut relation_pair, *b"259", relation_record);
    out.extend_from_slice(&relation_pair);

    let scope_offset = u64::try_from(out.len()).expect("synthetic CopyPaste scope offset");
    let mut scope = vec![0_u8; 529];
    write_indexed_header(&mut scope, *b"268", scope_record);
    transform(&mut scope, 38, [0.0, 0.0, 0.0]);
    scope[378..382].copy_from_slice(&1_u32.to_le_bytes());
    scope[382] = 1;
    scope[383..391].copy_from_slice(&u64::from(relation_record).to_le_bytes());
    scope[393..397].copy_from_slice(&0_u32.to_le_bytes());
    scope[397..401].copy_from_slice(&9_u32.to_le_bytes());
    for (ordinal, unit) in "CopyPaste".encode_utf16().enumerate() {
        scope[401 + ordinal * 2..403 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
    }
    scope[419..423].copy_from_slice(&1_u32.to_le_bytes());
    transform(&mut scope, 194, [2.0, 3.0, 4.0]);
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let mut scope_pair = [0_u8; 11];
    write_indexed_header(&mut scope_pair, *b"261", scope_record);
    out.extend_from_slice(&scope_pair);

    (out, records)
}

/// Add one source-to-copy body relation and a `CopyPasteBodies` scope whose
/// fixed relation lane is separate from the ordered body-operand table.
pub(crate) fn generated_design_copy_paste_bodies_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let body_map_count_at = 11 + crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN;
    let second_body_pair_at = body_map_count_at + 4 + 16;
    out[body_map_count_at..body_map_count_at + 4].copy_from_slice(&2_u32.to_le_bytes());
    let second_body_pair = [42_u64.to_le_bytes(), 8_422_u64.to_le_bytes()].concat();
    out.splice(
        second_body_pair_at..second_body_pair_at,
        second_body_pair.iter().copied(),
    );
    for (_, offset) in &mut records {
        if *offset >= u64::try_from(second_body_pair_at).expect("body pair insertion offset") {
            *offset += u64::try_from(second_body_pair.len()).expect("body pair length");
        }
    }
    let scope_record = 1_400_u32;
    let body_group_record = 1_500_u32;
    let body_operand_record = 1_600_u32;
    let relation_record = 1_700_u32;

    let scope_offset = u64::try_from(out.len()).expect("synthetic CopyPasteBodies scope offset");
    let mut scope = Vec::new();
    indexed_header(&mut scope, *b"268", scope_record);
    scope.extend_from_slice(&[0; 18]);
    push_marked_reference(&mut scope, body_group_record);
    push_marked_reference(&mut scope, relation_record);
    scope.extend_from_slice(&2_u32.to_le_bytes());
    push_marked_reference(&mut scope, body_group_record);
    push_marked_reference(&mut scope, body_operand_record);
    scope.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut scope, "CopyPasteBodies");
    let mut tail = vec![0_u8; 110];
    tail[0..4].copy_from_slice(&1_u32.to_le_bytes());
    tail[53..57].copy_from_slice(&u32::MAX.to_le_bytes());
    scope.extend_from_slice(&tail);
    indexed_header(&mut scope, *b"261", scope_record);
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let body_group_offset = u64::try_from(out.len()).expect("synthetic body group offset");
    let mut body_group = Vec::new();
    indexed_header(&mut body_group, *b"264", body_group_record);
    body_group.extend_from_slice(&[0; 10]);
    body_group.extend_from_slice(&1_u32.to_le_bytes());
    push_marked_reference(&mut body_group, body_operand_record);
    out.extend_from_slice(&body_group);
    records.push((u64::from(body_group_record), body_group_offset));

    let body_operand_offset = u64::try_from(out.len()).expect("synthetic body operand offset");
    let mut body_operand = Vec::new();
    indexed_header(&mut body_operand, *b"268", body_operand_record);
    body_operand.extend_from_slice(&[0; 10]);
    body_operand.extend_from_slice(&1_u32.to_le_bytes());
    body_operand.extend_from_slice(&985_u64.to_le_bytes());
    out.extend_from_slice(&body_operand);
    records.push((u64::from(body_operand_record), body_operand_offset));

    let relation_offset = u64::try_from(out.len()).expect("synthetic body relation offset");
    let mut relation = Vec::new();
    indexed_header(&mut relation, *b"264", relation_record);
    relation.extend_from_slice(&[0; 8]);
    relation.push(1);
    relation.extend_from_slice(&2_u32.to_le_bytes());
    relation.push(1);
    relation.extend_from_slice(&985_u32.to_le_bytes());
    relation.extend_from_slice(&[0; 10]);
    relation.push(1);
    relation.extend_from_slice(&8_422_u32.to_le_bytes());
    relation.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&relation);
    records.push((u64::from(relation_record), relation_offset));

    (out, records)
}

/// Add a `Form` scope and its retained compact selection-group carrier.
pub(super) fn generated_design_form_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let scope_record = 1_400_u32;
    let cage_record = 1_500_u32;
    let cage_object_record = 1_600_u32;

    let scope_offset = u64::try_from(out.len()).expect("synthetic Form scope offset");
    let mut scope = Vec::new();
    indexed_header(&mut scope, *b"268", scope_record);
    scope.extend_from_slice(&[0; 10]);
    scope.extend_from_slice(&1_u32.to_le_bytes());
    push_marked_reference(&mut scope, cage_record);
    scope.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut scope, "Form");
    scope.extend_from_slice(&1_u32.to_le_bytes());
    scope.extend_from_slice(&[0; 78]);
    indexed_header(&mut scope, *b"261", scope_record);
    out.extend_from_slice(&scope);
    records.push((u64::from(scope_record), scope_offset));

    let cage_offset = u64::try_from(out.len()).expect("synthetic Form selection-group offset");
    let mut cage = vec![0_u8; 100];
    cage[0..4].copy_from_slice(&3_u32.to_le_bytes());
    cage[4..7].copy_from_slice(b"264");
    cage[7..11].copy_from_slice(&cage_record.to_le_bytes());
    cage[21] = 1;
    cage[22..30].copy_from_slice(&u64::from(scope_record).to_le_bytes());
    cage[32..36].copy_from_slice(&1_u32.to_le_bytes());
    cage[36] = 1;
    cage[37..45].copy_from_slice(&u64::from(cage_object_record).to_le_bytes());
    cage[47..51].copy_from_slice(&252_u32.to_le_bytes());
    cage[51..59].copy_from_slice(&0.25_f64.to_le_bytes());
    cage[59..63].copy_from_slice(&252_u32.to_le_bytes());
    cage[63] = 1;
    cage[64..72].copy_from_slice(&u64::from(cage_record + 2).to_le_bytes());
    cage[74] = 1;
    cage[77] = 1;
    cage[78..86].copy_from_slice(&u64::from(cage_record + 1).to_le_bytes());
    cage[89] = 1;
    cage[90..98].copy_from_slice(&u64::from(scope_record).to_le_bytes());
    out.extend_from_slice(&cage);
    indexed_header(&mut out, *b"261", cage_record);
    records.push((u64::from(cage_record), cage_offset));

    indexed_header(&mut out, *b"301", cage_object_record);

    (out, records)
}

/// Add paired dimensional parameters and their companion payloads to the
/// generated Sketch stream. The payload contains a paired two-locus frame over
/// two additional point records and one construction recipe record.
pub(super) fn generated_design_sketch_dimension_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn append_point(
        out: &mut Vec<u8>,
        records: &mut Vec<(u64, u64)>,
        record_index: u32,
        paired_record_index: u32,
        persistent_id: u64,
        coordinates: [f64; 2],
    ) {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic dimension point offset"),
        ));
        let mut point = vec![0_u8; 105];
        point[0..4].copy_from_slice(&3_u32.to_le_bytes());
        point[4..7].copy_from_slice(b"266");
        point[7..11].copy_from_slice(&record_index.to_le_bytes());
        point[20] = 1;
        point[21..25].copy_from_slice(&1_u32.to_le_bytes());
        point[25..29].copy_from_slice(&6_u32.to_le_bytes());
        point[29..35].copy_from_slice(b"pt_tag");
        point[35..39].copy_from_slice(&23_u32.to_le_bytes());
        point[39..62].copy_from_slice(b"IntrinsicMetaTypeuint64");
        point[62..70].copy_from_slice(&persistent_id.to_le_bytes());
        point[70] = 1;
        point[71..75].copy_from_slice(&paired_record_index.to_le_bytes());
        point[89..97].copy_from_slice(&coordinates[0].to_le_bytes());
        point[97..105].copy_from_slice(&coordinates[1].to_le_bytes());
        point.extend_from_slice(&[0; 16]);
        point.push(1);
        point.extend_from_slice(&[0; 12]);
        point.extend_from_slice(&1.0_f32.to_le_bytes());
        point.extend_from_slice(&1.0_f32.to_le_bytes());
        point.extend_from_slice(&[0, 1, 0, 0, 0]);
        push_reference_u64(&mut point, u64::from(paired_record_index));
        push_reference_u64(&mut point, 277);
        out.extend_from_slice(&point);
        records.push((
            u64::from(paired_record_index),
            u64::try_from(out.len()).expect("synthetic dimension point pair offset"),
        ));
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(b"267");
        out.extend_from_slice(&paired_record_index.to_le_bytes());
        out.extend_from_slice(&[0; 15]);
        push_reference_u64(out, u64::from(record_index));
    }

    fn append_owner(
        out: &mut Vec<u8>,
        records: &mut Vec<(u64, u64)>,
        owner_record_index: u32,
        parameter_record_index: u32,
        companion_record_index: u32,
        scope_record_index: u32,
        local_ordinal: u32,
    ) {
        records.push((
            u64::from(owner_record_index),
            u64::try_from(out.len()).expect("synthetic dimension owner offset"),
        ));
        let mut owner = crate::design::test_support::parameter_owner_frame();
        owner[4..7].copy_from_slice(b"270");
        owner[7..11].copy_from_slice(&owner_record_index.to_le_bytes());
        owner[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
        owner[40..48].copy_from_slice(&1.0_f64.to_le_bytes());
        owner[49..53].copy_from_slice(&parameter_record_index.to_le_bytes());
        owner[35..39].copy_from_slice(&local_ordinal.to_le_bytes());
        owner[68..72].copy_from_slice(&scope_record_index.to_le_bytes());
        owner[82..86].copy_from_slice(&companion_record_index.to_le_bytes());
        owner[94..98].copy_from_slice(&scope_record_index.to_le_bytes());
        out.extend_from_slice(&owner);
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(b"270");
        out.extend_from_slice(&owner_record_index.to_le_bytes());
    }

    fn append_parameter(
        out: &mut Vec<u8>,
        records: &mut Vec<(u64, u64)>,
        owner_record_index: u32,
        parameter_record_index: u32,
        name: &str,
        source_kind: &str,
    ) {
        records.push((
            u64::from(parameter_record_index),
            u64::try_from(out.len()).expect("synthetic dimension parameter offset"),
        ));
        let mut parameter = crate::design::test_support::parameter_record(
            Some(owner_record_index),
            "10 mm",
            source_kind,
            Some("mm"),
            name,
            1.0,
        );
        parameter[4..7].copy_from_slice(b"271");
        parameter[7..11].copy_from_slice(&parameter_record_index.to_le_bytes());
        out.extend_from_slice(&parameter);
    }

    fn append_companion(
        out: &mut Vec<u8>,
        records: &mut Vec<(u64, u64)>,
        owner_record_index: u32,
        companion_record_index: u32,
    ) {
        records.push((
            u64::from(companion_record_index),
            u64::try_from(out.len()).expect("synthetic dimension companion offset"),
        ));
        let mut companion = vec![0_u8; 58];
        companion[0..4].copy_from_slice(&3_u32.to_le_bytes());
        companion[4..7].copy_from_slice(b"272");
        companion[7..11].copy_from_slice(&companion_record_index.to_le_bytes());
        companion[31] = 1;
        companion[32..36].copy_from_slice(&owner_record_index.to_le_bytes());
        companion[42..50].copy_from_slice(&1_700_000_000_000_000_u64.to_le_bytes());
        out.extend_from_slice(&companion);
    }

    let (mut out, mut records) = generated_design_sketch_bulkstream();
    append_point(&mut out, &mut records, 1300, 1301, 800, [0.0, 0.0]);
    append_point(&mut out, &mut records, 1302, 1303, 801, [1.0, 0.0]);

    let scope_record_index = 1100_u32;

    append_owner(
        &mut out,
        &mut records,
        1200,
        1201,
        1202,
        scope_record_index,
        0,
    );
    append_parameter(
        &mut out,
        &mut records,
        1200,
        1201,
        "d1",
        "Linear Dimension-1",
    );
    append_companion(&mut out, &mut records, 1200, 1202);

    let mut locus_pair = vec![0_u8; 80];
    locus_pair[0..4].copy_from_slice(&3_u32.to_le_bytes());
    locus_pair[4..7].copy_from_slice(b"274");
    locus_pair[7..11].copy_from_slice(&1304_u32.to_le_bytes());
    locus_pair[19] = 1;
    locus_pair[20..24].copy_from_slice(&3_u32.to_le_bytes());
    locus_pair[24] = 1;
    locus_pair[35..39].copy_from_slice(&4_u32.to_le_bytes());
    locus_pair[39] = 1;
    locus_pair[40..44].copy_from_slice(&1300_u32.to_le_bytes());
    locus_pair[50..54].copy_from_slice(&0_u32.to_le_bytes());
    locus_pair[54] = 1;
    locus_pair[55..59].copy_from_slice(&1302_u32.to_le_bytes());
    locus_pair[65..69].copy_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&locus_pair);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"273");
    out.extend_from_slice(&1304_u32.to_le_bytes());

    let recipe_prefix = [0_u8; 9];
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"275");
    out.extend_from_slice(&1305_u32.to_le_bytes());
    out.extend_from_slice(&recipe_prefix);
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(b"body_recipe_data");
    for value in [0_i32, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }

    append_owner(
        &mut out,
        &mut records,
        1203,
        1204,
        1205,
        scope_record_index,
        1,
    );
    append_parameter(
        &mut out,
        &mut records,
        1203,
        1204,
        "d2",
        "Linear Dimension-2",
    );
    append_companion(&mut out, &mut records, 1203, 1205);
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(b"272");
    out.extend_from_slice(&1205_u32.to_le_bytes());

    (out, records)
}

/// A repeated local module with two children and one existing body-map binding.
pub(crate) fn nested_local_component_design(body_owners: &[u64]) -> (Vec<u8>, Vec<u8>) {
    let mut bulk = Vec::new();
    let mut records = Vec::new();
    for (record, root, context) in [
        (100, 101_u64, "00000000-0000-4000-8000-000000000100"),
        (200, 201, "00000000-0000-4000-8000-000000000200"),
        (300, 301, "00000000-0000-4000-8000-000000000300"),
    ] {
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"256", record);
        bulk.push(1);
        bulk.extend_from_slice(&root.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
        lp_utf16(&mut bulk, context);
    }
    records.push((400, u64_from_index(bulk.len())));
    indexed_header(&mut bulk, *b"257", 400);
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    lp_ascii(&mut bulk, "rootInstance");
    bulk.push(1);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.push(1);
    bulk.extend_from_slice(&100_u64.to_le_bytes());
    bulk.extend_from_slice(&[0; 7]);
    let parent = [
        [0.0, -1.0, 0.0, 3.0],
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let translated = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (record, definition, owner, ordinal, transform) in [
        (401, 200_u64, 101_u64, 1_u32, Some(parent)),
        (402, 300, 201, 1, None),
        (403, 300, 201, 2, Some(translated)),
        (404, 200, 101, 2, Some(parent)),
    ] {
        records.push((u64::from(record), u64_from_index(bulk.len())));
        indexed_header(&mut bulk, *b"257", record);
        bulk.extend_from_slice(&[0; 8]);
        bulk.push(1);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&definition.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
        bulk.extend_from_slice(&0_u32.to_le_bytes());
        bulk.push(1);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        lp_ascii(&mut bulk, "SelfGated");
        lp_ascii(&mut bulk, "IntrinsicMetaTypebool");
        bulk.push(0);
        bulk.extend_from_slice(&[0; 2]);
        bulk.extend_from_slice(&ordinal.to_le_bytes());
        lp_utf16(&mut bulk, "00000000-0000-4000-8000-000000000999");
        lp_utf16(&mut bulk, &format!("00000000-0000-4000-8001-{record:012}"));
        bulk.push(1);
        bulk.extend_from_slice(&500_u64.to_le_bytes());
        bulk.extend_from_slice(&[0, 1, 1, 0, 0, 0, 0]);
        bulk.push(1);
        bulk.extend_from_slice(&definition.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
        bulk.push(u8::from(transform.is_none()));
        if let Some(transform) = transform {
            for value in transform.into_iter().flatten() {
                bulk.extend_from_slice(&f64::to_le_bytes(value));
            }
        }
        bulk.extend_from_slice(&[0; 9]);
        bulk.push(1);
        bulk.extend_from_slice(&owner.to_le_bytes());
        bulk.extend_from_slice(&[0; 2]);
    }
    records.push((500, u64_from_index(bulk.len())));
    indexed_header(&mut bulk, *b"258", 500);
    bulk.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    bulk.extend_from_slice(&1_u32.to_le_bytes());
    bulk.extend_from_slice(&42_u64.to_le_bytes());
    bulk.extend_from_slice(&350_u64.to_le_bytes());
    bulk.extend_from_slice(&1793_u64.to_le_bytes());
    bulk.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bulk, "BREP.module.smbh");
    let containers = append_body_owners(&mut bulk, &mut records, body_owners);
    let meta = design_metastream_with_records(
        &[
            (
                crate::records::body_ownership::COMPONENT_GUID,
                "",
                2,
                "Component",
                &[100, 200, 300],
            ),
            (
                crate::records::feature::assembly_features::OCCURRENCE_PLACEMENT_TYPE_GUID,
                "",
                2,
                "Component",
                &[400, 401, 402, 403, 404],
            ),
            (
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                "Body",
                &[500],
            ),
            (
                "00000000-0000-4000-8000-000000000011",
                "21F379C8-CAFD-4985-B461-767673A4C502",
                2,
                "Component",
                &[101, 201, 301],
            ),
            container_type(&containers),
            body_type(),
        ],
        &records,
    );
    (bulk, meta)
}
