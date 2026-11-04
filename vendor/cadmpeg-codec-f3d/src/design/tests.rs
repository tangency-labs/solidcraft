// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
//! Relation, decode, and projection unit tests for the design modules.

use crate::design::constraints::project_sketch_constraints;
use crate::design::decode::operands::has_typed_edge_treatment_group;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::sketch::bind_sketch_graph;
use crate::design::sketch_project::{project_sketch_design, SketchProjectInputs};
use crate::design::test_support::parameter_record;
use crate::design::{design_feature_family, is_localized_edge_treatment_kind, DesignFeatureFamily};
use crate::ids::feature_input_topology_id;
use crate::ids::{
    neutral_dimension_constraint_id, neutral_feature_id_parts, neutral_parameter_id_parts,
    neutral_sketch_curve_id, neutral_sketch_point_id,
};
use crate::records::{
    entity_header::{DesignEntityHeader, DESIGN_MODULE_SKETCH},
    sketch_geometry::SketchPoint,
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchRelation, SketchRelationMember, SketchRelationReturnMember},
};
use cadmpeg_ir::math::Point2;
use std::collections::HashSet;

#[test]
fn feature_family_tokens_are_localized() {
    use crate::records::feature::scope::DesignFeatureKind;
    let family = |token: &str| {
        design_feature_family(
            &DesignFeatureKind::try_from(token.to_owned()).expect("nonempty family name"),
        )
    };
    let treatment = |token: &str| {
        has_typed_edge_treatment_group(
            &DesignFeatureKind::try_from(token.to_owned()).expect("nonempty family name"),
        )
    };
    let localized = |token: &str| {
        is_localized_edge_treatment_kind(
            &DesignFeatureKind::try_from(token.to_owned()).expect("nonempty family name"),
        )
    };
    assert_eq!(family("As-built"), Some(DesignFeatureFamily::Assemble));
    assert_eq!(family("Esquisse"), Some(DesignFeatureFamily::Sketch));
    assert_eq!(family("Extrusion"), Some(DesignFeatureFamily::Extrude));
    assert_eq!(family("Extrusão"), Some(DesignFeatureFamily::Extrude));
    for token in ["Skizze", "Esboço", "Boceto", "スケッチ"] {
        assert_eq!(family(token), Some(DesignFeatureFamily::Sketch));
    }
    assert_eq!(family("Congé"), Some(DesignFeatureFamily::Fillet));
    for token in ["Abrundung", "Arredondamento", "Empalme", "フィレット"] {
        assert_eq!(family(token), Some(DesignFeatureFamily::Fillet));
        assert!(treatment(token));
    }
    assert_eq!(family("Chanfrein"), Some(DesignFeatureFamily::Chamfer));
    for token in ["Chaflán", "面取り"] {
        assert_eq!(family(token), Some(DesignFeatureFamily::Chamfer));
        assert!(treatment(token));
    }
    for token in ["Extruir", "押し出し"] {
        assert_eq!(family(token), Some(DesignFeatureFamily::Extrude));
    }
    assert_eq!(family("Escala"), Some(DesignFeatureFamily::Scale));
    assert_eq!(family("Solevación"), Some(DesignFeatureFamily::Loft));
    assert_eq!(family("Vaciado"), Some(DesignFeatureFamily::Shell));
    assert_eq!(family("PlanoDeObra"), None);
    assert_eq!(family("Lienzo"), None);
    assert_eq!(family("EsferaPrimitiva"), None);
    for token in [
        "Congé",
        "Abrundung",
        "Arredondamento",
        "Chanfrein",
        "Empalme",
        "フィレット",
        "Chaflán",
        "面取り",
    ] {
        assert!(localized(token));
    }
    for token in ["Fillet", "Chamfer", "Extrusion", "unknown"] {
        assert!(!localized(token));
    }
    for token in ["C-Pattern", "Réseau C"] {
        assert_eq!(family(token), Some(DesignFeatureFamily::CircularPattern));
    }
    assert_eq!(family("Symétrie miroir"), Some(DesignFeatureFamily::Mirror));
    assert_eq!(
        family("DécalerLesFaces"),
        Some(DesignFeatureFamily::OffsetFaces)
    );
    assert_eq!(
        family("ReplaceFace"),
        Some(DesignFeatureFamily::ReplaceFace)
    );
    assert_eq!(family("Schale"), Some(DesignFeatureFamily::Shell));
    assert_eq!(family("SpirePrimitive"), Some(DesignFeatureFamily::Coil));
    assert_eq!(family("Hem"), Some(DesignFeatureFamily::SheetMetalHem));
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        &DesignFeatureKind::Hem
    ));
    assert_eq!(
        family("SurfacePatch"),
        Some(DesignFeatureFamily::SurfacePatch)
    );
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        &DesignFeatureKind::SurfacePatch
    ));
    assert!(crate::design::decode::operands::has_edge_recipe_operands(
        &DesignFeatureKind::WorkPoint
    ));
    assert_eq!(
        family("SurfaceRuled"),
        Some(DesignFeatureFamily::SurfaceRuled)
    );
    assert_eq!(
        family("BoundaryFill"),
        Some(DesignFeatureFamily::BoundaryFill)
    );
    assert_eq!(
        family("SurfaceTrim"),
        Some(DesignFeatureFamily::SurfaceTrim)
    );
    assert_eq!(family("Hole"), Some(DesignFeatureFamily::Hole));
    assert_eq!(family("Split"), Some(DesignFeatureFamily::Split));
    assert_eq!(family("Loft"), Some(DesignFeatureFamily::Loft));
    assert_eq!(family("Sweep"), Some(DesignFeatureFamily::Sweep));
    assert_eq!(family("Pipe"), Some(DesignFeatureFamily::Pipe));
}

#[test]
fn feature_identity_uses_stream_family_ordinal_and_scope_record() {
    let first = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let same = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 41);
    let different_stream = neutral_feature_id_parts("Design/A", "B:Kind:12", 3, 41);
    let different_family = neutral_feature_id_parts("Design/A:B", "Kind", 123, 41);
    let different_scope = neutral_feature_id_parts("Design/A:B", "Kind:12", 3, 42);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_family);
    assert_ne!(first, different_scope);

    let localized = neutral_feature_id_parts("Design Name", "Symétrie miroir", 1, 41);
    let literal_escape = neutral_feature_id_parts("Design%20Name", "Symétrie%20miroir", 1, 41);
    assert!(!localized.as_str().chars().any(char::is_whitespace));
    assert!(localized.as_str().contains("Design%20Name"));
    assert!(localized.as_str().contains("Symétrie%20miroir"));
    assert_ne!(localized, literal_escape);
    assert!(!feature_input_topology_id(&localized, 2)
        .as_str()
        .chars()
        .any(char::is_whitespace));
}

#[test]
fn parameter_identity_uses_stream_and_native_record_index() {
    let first = neutral_parameter_id_parts("Design/A:12", 3);
    let same = neutral_parameter_id_parts("Design/A:12", 3);
    let different_stream = neutral_parameter_id_parts("Design/A", 123);
    let different_record = neutral_parameter_id_parts("Design/A:12", 4);

    assert_eq!(first, same);
    assert_ne!(first, different_stream);
    assert_ne!(first, different_record);
}

#[test]
fn parameter_identity_distinguishes_repeated_source_ordinals() {
    let mut first = parse_design_parameter_record(&parameter_record(
        Some(40),
        "1 cm",
        "AlongDistance",
        Some("cm"),
        "d9",
        1.0,
    ))
    .expect("first parameter");
    first.id = "f3d:Design/A:design-parameter#100".into();

    let mut second = first.clone();
    second.id = "f3d:Design/A:design-parameter#200".into();
    second.record_index = first.record_index + 1;

    assert_eq!(first.source_ordinal, second.source_ordinal);
    assert_ne!(
        crate::ids::neutral_parameter_id(&first),
        crate::ids::neutral_parameter_id(&second)
    );
}

#[test]
fn sketch_geometry_identity_uses_owner_and_native_persistent_ids() {
    use cadmpeg_ir::sketches::{SketchId, SpatialSketchId};

    let sketch = SketchId::mint("f3d:model:sketch#Design/A@10").unwrap();
    let other_sketch = SketchId::mint("f3d:model:sketch#Design/A@11").unwrap();
    let point = neutral_sketch_point_id(&sketch, 42);
    let same_point = neutral_sketch_point_id(&sketch, 42);
    let curve = neutral_sketch_curve_id(&sketch, 42, 0);
    let same_curve = neutral_sketch_curve_id(&sketch, 42, 0);

    assert_eq!(point, same_point);
    assert_eq!(curve, same_curve);
    assert_ne!(point, curve);
    assert_ne!(curve, neutral_sketch_curve_id(&sketch, 42, 1));
    assert_ne!(point, neutral_sketch_point_id(&other_sketch, 42));
    assert_ne!(curve, neutral_sketch_curve_id(&other_sketch, 42, 0));

    let spatial = SpatialSketchId::mint("f3d:model:spatial-sketch#Design/A@10").unwrap();
    let other_spatial = SpatialSketchId::mint("f3d:model:spatial-sketch#Design/A@11").unwrap();
    assert_ne!(
        crate::ids::neutral_spatial_sketch_point_id(&spatial, 42),
        crate::ids::neutral_spatial_sketch_point_id(&other_spatial, 42)
    );
    assert_ne!(
        crate::ids::neutral_spatial_sketch_curve_id(&spatial, 42, 0),
        crate::ids::neutral_spatial_sketch_curve_id(&other_spatial, 42, 0)
    );
}

#[test]
fn governing_dimension_identity_uses_parameter_identity() {
    let parameter = cadmpeg_ir::features::ParameterId::mint("f3d:model:parameter#Design/A:12")
        .expect("identity grammar");
    let relocated = neutral_dimension_constraint_id(&parameter, "pair");
    let same = neutral_dimension_constraint_id(&parameter, "pair");
    let other_form = neutral_dimension_constraint_id(&parameter, "null-pair");
    let other_parameter = neutral_dimension_constraint_id(
        &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:Design/A")
            .expect("identity grammar"),
        "12:pair",
    );

    assert_eq!(relocated, same);
    assert_ne!(relocated, other_form);
    assert_ne!(relocated, other_parameter);
    assert_eq!(relocated.as_str().matches('#').count(), 1);
}

#[test]
fn design_streams_scope_sketch_graphs_identities_and_parameter_names() {
    let placement = |stream: &str| DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from(format!("{stream}_100"))
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let header = |stream: &str| DesignEntityHeader {
        id: format!("f3d:{stream}:design-entity-header#0"),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from(format!("{stream}_100"))
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        optional_slot_present: true,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 0,
                references: vec![30]
                    .into_iter()
                    .zip(vec![0])
                    .map(|(value, offset)| crate::records::identity::Located { value, offset })
                    .collect(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let point = |stream: &str| {
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("f3d:{stream}:sketch-point#0"),
            record_index: 20,
            owner_reference: None,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 89,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                20,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(1.0, 2.0),
        })
        .unwrap()
    };
    let relation = |stream: &str| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: format!("f3d:{stream}:sketch-relation#30"),
            record_index: 30,
            class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned())
                .unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 100,
            owner_entity_id: None,
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: (vec![SketchRelationMember::from_index(20)])
                .try_into()
                .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![SketchRelationReturnMember::from_index(20)])
                .try_into()
                .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap()
    };

    let placements = [placement("A"), placement("B")];
    let mut points = [point("A"), point("B")];
    let mut relations = [relation("A"), relation("B")];
    bind_sketch_graph(
        &[header("A"), header("B")],
        &mut points,
        &mut [],
        &mut [],
        &mut relations,
    )
    .expect("stream-local sketch graphs bind independently");
    assert_eq!(
        relations[0]
            .owner_entity_id
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("A_100")
    );
    assert_eq!(
        relations[1]
            .owner_entity_id
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("B_100")
    );

    let mut overflowing_header = header("A");
    overflowing_header.entity_id =
        crate::records::identity::DesignEntityId::from_parts("A", u64::from(u32::MAX) + 101);
    assert!(bind_sketch_graph(
        &[overflowing_header],
        &mut [point("A")],
        &mut [],
        &mut [],
        &mut [relation("A")]
    )
    .is_err());

    let (mut sketches, mut entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &SketchProjectInputs::new(&placements, &points, &[], &[], &[]),
            1.0e-6,
        )
    })
    .expect("sketch lanes pair")
    .into_tuple();
    let mut constraints = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_constraints(
            decode_ctx,
            &placements,
            &[],
            (&points, &[], &[]),
            &relations,
            &entities,
        )
    })
    .unwrap();
    assert_eq!(sketches.len(), 2);
    assert_eq!(entities.len(), 2);
    assert_eq!(constraints.len(), 2);
    assert_eq!(
        sketches
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        entities
            .iter()
            .map(cadmpeg_ir::SketchEntity::id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        constraints
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );

    let parameter = |stream: &str, record_index, name: &str, expression: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated user parameter is canonical");
        parameter.id = format!("f3d:{stream}:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let crate::design::feature_project::ProjectedParameterDesign { parameters, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter("A", 40, "Width", "1 mm"),
                        parameter("A", 41, "Half", "Width / 2"),
                        parameter("B", 40, "Width", "2 mm"),
                    ],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let half = parameters
        .iter()
        .find(|parameter| parameter.name == "Half")
        .expect("projected Half parameter");
    let a_width = parameters
        .iter()
        .find(|parameter| {
            parameter.name == "Width"
                && parameter.native_ref.as_deref() == Some("f3d:A:parameter#40")
        })
        .expect("projected stream A Width parameter");
    assert_eq!(
        half.dependencies.as_slice(),
        std::slice::from_ref(&a_width.id)
    );
    assert_eq!(
        parameters
            .iter()
            .map(|item| &item.id)
            .collect::<HashSet<_>>()
            .len(),
        3
    );

    for sketch in &mut sketches {
        sketch.native_ref = None;
    }
    for entity in &mut entities {
        entity.native_ref = None;
    }
    for constraint in &mut constraints {
        constraint.native_ref = None;
    }
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.sketches = sketches;
    ir.model.sketch_entities = entities;
    ir.model.sketch_constraints = constraints;
    ir.finalize();
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}
