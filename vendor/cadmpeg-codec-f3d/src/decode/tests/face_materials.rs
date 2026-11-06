// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::topology::Color;

use crate::materials::FaceAppearanceAssignment;

const FACE_GUID: &str = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
const OTHER_GUID: &str = "cccccccc-1111-2222-3333-dddddddddddd";
const VISUAL: &str = "11111111-2222-3333-4444-555555555555_Post2015";
const REVISION: &str = "11111111-2222-3333-4444-555555555555_Post2015_Post2015";

fn assignment(guid: &str, token: &str) -> FaceAppearanceAssignment {
    FaceAppearanceAssignment {
        face_guid: guid.into(),
        visual_guid: token.to_owned().try_into().expect("visual token"),
        color: Some(Color::new(0.25, 0.5, 0.75, 1.0).expect("normalized color")),
    }
}

fn model() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("synthetic cube");
    for (index, guid) in [FACE_GUID, OTHER_GUID].into_iter().enumerate() {
        ir.model.attributes.push(SourceAttribute {
            id: format!("test:model:attribute#{index}")
                .try_into()
                .expect("identity"),
            target: AttributeTarget::Face(ir.model.faces[index].id.clone()),
            name: "ATTRIB_CUSTOM-attrib".into(),
            values: vec![
                AttributeValue::String("NEUTRON_Material_attrib_def".into()),
                AttributeValue::String(guid.into()),
            ],
        });
    }
    for (index, token) in [VISUAL, REVISION].into_iter().enumerate() {
        ir.model.appearances.push(Appearance {
            id: format!("test:model:appearance#{index}")
                .try_into()
                .expect("identity"),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: Some(token.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: Default::default(),
            textures: Vec::new(),
        });
    }
    ir
}

#[test]
fn distinct_face_material_revisions_report_loss_without_selecting_by_order() {
    for tokens in [[VISUAL, REVISION, VISUAL], [REVISION, VISUAL, REVISION]] {
        let mut ir = model();
        let explicit = Color::new(0.75, 0.5, 0.25, 1.0).expect("normalized color");
        if tokens[0] == REVISION {
            ir.model.faces[0].color = Some(explicit);
        }
        let expected_color = ir.model.faces[0]
            .color
            .or(assignment(FACE_GUID, VISUAL).color);
        let mut assignments = tokens.map(|token| assignment(FACE_GUID, token)).to_vec();
        assignments.push(assignment(OTHER_GUID, VISUAL));
        let losses = super::super::resolve_face_appearance_bindings(&mut ir, &assignments)
            .expect("distinct visual tokens are admitted");
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "material.face-appearance-ambiguous"
        );
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::FaceAppearanceAmbiguous.kind()
        );
        assert_eq!(ir.model.appearances.len(), 2);
        assert_eq!(ir.model.appearance_bindings.len(), 1);
        assert_eq!(
            ir.model.appearance_bindings[0].target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(ir.model.faces[1].id.clone())
        );
        assert_eq!(ir.model.faces[0].color, expected_color);
    }
}

#[test]
fn repeated_complete_face_material_token_binds_once_without_loss() {
    let mut ir = model();
    let assignment = assignment(FACE_GUID, VISUAL);
    let losses =
        super::super::resolve_face_appearance_bindings(&mut ir, &[assignment.clone(), assignment])
            .expect("equal complete tokens identify one candidate");
    assert!(losses.is_empty());
    assert_eq!(ir.model.appearance_bindings.len(), 1);
    assert_eq!(
        ir.model.appearance_bindings[0].appearance,
        ir.model.appearances[0].id
    );
}

fn synthetic_body_presentation_payloads(entity: u64, body_name: &str) -> (Vec<u8>, Vec<u8>) {
    use crate::design::presentation::{
        BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, MODERN_APPEARANCE_LIBRARY_IDS,
        PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::records::entity_header::DESIGN_MODULE_BODY;
    use crate::test_support::streams_test::design_metastream_with_records;
    use crate::test_support::{lp_ascii, lp_utf16, push_reference_u64};

    let body_tag = 256u32;
    let node_guid = "11111111-2222-8333-A444-555555555555";
    let visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE_Post2015";
    let mut bulk = Vec::new();
    lp_ascii(&mut bulk, &body_tag.to_string());
    bulk.extend_from_slice(&entity.to_le_bytes());
    bulk.extend_from_slice(&[0; 6]);
    lp_utf16(&mut bulk, &format!("0_{entity}"));
    lp_utf16(&mut bulk, node_guid);
    bulk.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    lp_utf16(&mut bulk, "99999999-8888-8777-A666-555555555555");
    lp_utf16(&mut bulk, PHYSICAL_MATERIAL_LIBRARY_ID);
    lp_utf16(&mut bulk, "PrismMaterial-001");
    push_reference_u64(&mut bulk, 7);
    bulk.push(0);
    push_reference_u64(&mut bulk, entity + 1);
    lp_utf16(&mut bulk, body_name);
    bulk.extend_from_slice(&1.0f32.to_le_bytes());
    bulk.extend_from_slice(&[1, 1]);
    lp_utf16(&mut bulk, visual_guid);
    for marker in MODERN_APPEARANCE_LIBRARY_IDS {
        lp_utf16(&mut bulk, marker);
    }

    let types = [
        (
            BODY_PRESENTATION_TYPE_GUID,
            BODY_PRESENTATION_BASE_TYPE_GUID,
            BODY_PRESENTATION_TYPE_VERSION,
            DESIGN_MODULE_BODY,
            &[entity][..],
        ),
        (
            BREP_CONTAINER_TYPE_GUID,
            "",
            BREP_CONTAINER_TYPE_VERSION,
            "",
            &[7],
        ),
        (
            BODY_SCENE_NODE_TYPE_GUID,
            "",
            BODY_SCENE_NODE_TYPE_VERSION,
            "",
            &[entity + 1],
        ),
    ];
    let records = [(entity, 0)];
    let meta = design_metastream_with_records(&types, &records);
    (bulk, meta)
}

fn synthetic_body_presentation_archive(stream_name: &str, entity: u64, body_name: &str) -> Vec<u8> {
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let (bulk, meta) = synthetic_body_presentation_payloads(entity, body_name);
    let meta_entry = format!(
        "{}/MetaStream.dat",
        stream_name
            .strip_suffix("/BulkStream.dat")
            .unwrap_or(stream_name)
    );

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream_name, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file(&meta_entry, stored).unwrap();
    zip.write_all(&meta).unwrap();
    zip.finish().unwrap().into_inner()
}

fn synthetic_two_stream_presentation_archive(
    stream1: &str,
    entity1: u64,
    name1: &str,
    stream2: &str,
    entity2: u64,
    name2: &str,
) -> Vec<u8> {
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);

    for (stream, entity, name) in [(stream1, entity1, name1), (stream2, entity2, name2)] {
        let (bulk, meta) = synthetic_body_presentation_payloads(entity, name);
        let meta_entry = format!(
            "{}/MetaStream.dat",
            stream.strip_suffix("/BulkStream.dat").unwrap_or(stream)
        );
        zip.start_file(stream, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file(&meta_entry, stored).unwrap();
        zip.write_all(&meta).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn make_body_binding(
    stream: &str,
    asm_key_offset: u64,
    entity_suffix: u64,
    body: &str,
) -> crate::records::bodies::DesignBodyBinding {
    crate::records::bodies::DesignBodyBinding::try_from(
        crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: crate::ids::native_design_body_binding_id(stream, asm_key_offset),
            stream: stream.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 7,
            asm_body_key_offset: asm_key_offset,
            entity_suffix,
            entity_suffix_offset: asm_key_offset + 8,
            blob_name: "BREP.synthetic.smbh".into(),
            blob_name_offset: asm_key_offset + 32,
            body: Some(cadmpeg_ir::ids::BodyId::mint(body).expect("identity grammar")),
        },
    )
    .unwrap()
}

#[test]
fn default_body_names_are_identified() {
    use super::super::is_default_body_name;
    assert!(is_default_body_name("Body"));
    assert!(is_default_body_name("Body1"));
    assert!(is_default_body_name("Body2"));
    assert!(is_default_body_name("Body42"));
    assert!(is_default_body_name("Body999"));
    assert!(!is_default_body_name("Body_1"));
    assert!(!is_default_body_name("BodyPart"));
    assert!(!is_default_body_name("Bottom"));
    assert!(!is_default_body_name("Top"));
    assert!(!is_default_body_name(""));
}

#[test]
fn apply_body_presentation_names_joins_custom_name_stream_qualified() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_body_presentation_archive(stream, entity, "Bottom");

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = None;
        ir.model.bodies[0].visible = Some(true);

        let binding = make_body_binding(stream, 100, entity, body_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding]).unwrap();
        assert_eq!(ir.model.bodies[0].name.as_deref(), Some("Bottom"));
    });
}

#[test]
fn apply_body_presentation_names_skips_when_stream_mismatches() {
    let presentation_stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let binding_stream = "FusionAssetName[Active]/Design2/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_body_presentation_archive(presentation_stream, entity, "Bottom");

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = None;
        ir.model.bodies[0].visible = Some(true);

        let binding = make_body_binding(binding_stream, 100, entity, body_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding]).unwrap();
        assert_eq!(ir.model.bodies[0].name, None);
    });
}

#[test]
fn apply_body_presentation_names_skips_ambiguous_bindings_in_same_stream() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_body_presentation_archive(stream, entity, "Bottom");

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = None;
        ir.model.bodies[0].visible = Some(true);

        let binding1 = make_body_binding(stream, 100, entity, body_id.as_str());
        let binding2 = make_body_binding(stream, 200, entity, body_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding1, binding2]).unwrap();
        assert_eq!(
            ir.model.bodies[0].name, None,
            "sole-pair check must skip ambiguous bindings"
        );
    });
}

#[test]
fn apply_body_presentation_names_ignores_default_body_names() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let entity = 42u64;
    for default_name in ["Body", "Body1", "Body42"] {
        let archive = synthetic_body_presentation_archive(stream, entity, default_name);

        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut ir = model();
            let body_id = ir.model.bodies[0].id.clone();
            ir.model.bodies[0].name = None;
            ir.model.bodies[0].visible = Some(true);

            let binding = make_body_binding(stream, 100, entity, body_id.as_str());
            super::super::apply_body_presentation_names(&mut ir, scan, &[binding]).unwrap();
            assert_eq!(ir.model.bodies[0].name, None);
        });
    }
}

#[test]
fn apply_body_presentation_names_renames_body_with_none_visibility() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_body_presentation_archive(stream, entity, "Bottom");

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = None;
        ir.model.bodies[0].visible = None;

        let binding = make_body_binding(stream, 100, entity, body_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding]).unwrap();
        assert_eq!(ir.model.bodies[0].name.as_deref(), Some("Bottom"));
    });
}

#[test]
fn apply_body_presentation_names_preserves_existing_body_name() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_body_presentation_archive(stream, entity, "Bottom");

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = Some("CustomExisting".to_string());
        ir.model.bodies[0].visible = Some(true);

        let binding = make_body_binding(stream, 100, entity, body_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding]).unwrap();
        assert_eq!(ir.model.bodies[0].name.as_deref(), Some("CustomExisting"));
    });
}

#[test]
fn apply_body_presentation_names_does_not_alias_across_streams() {
    let stream1 = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let stream2 = "FusionAssetName[Active]/FusionDesignSegmentType2/BulkStream.dat";
    let entity = 42u64;
    let archive = synthetic_two_stream_presentation_archive(
        stream1, entity, "PlateOne", stream2, entity, "PlateTwo",
    );

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut ir = model();
        let body1_id = ir.model.bodies[0].id.clone();
        ir.model.bodies[0].name = None;
        ir.model.bodies[0].visible = Some(true);

        let mut body2 = ir.model.bodies[0].clone();
        let body2_id = cadmpeg_ir::ids::BodyId::mint("test:model:body#2").unwrap();
        body2.id = body2_id.clone();
        body2.name = None;
        body2.visible = Some(true);
        ir.model.bodies.push(body2);

        let binding1 = make_body_binding(stream1, 100, entity, body1_id.as_str());
        let binding2 = make_body_binding(stream2, 200, entity, body2_id.as_str());
        super::super::apply_body_presentation_names(&mut ir, scan, &[binding1, binding2]).unwrap();
        assert_eq!(ir.model.bodies[0].name.as_deref(), Some("PlateOne"));
        assert_eq!(ir.model.bodies[1].name.as_deref(), Some("PlateTwo"));
    });
}
