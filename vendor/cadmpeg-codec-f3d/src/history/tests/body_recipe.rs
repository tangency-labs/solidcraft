// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::history::{bind_direct_body_recipe_body_selection, FeatureBodySelectionInputs};

#[test]
fn form33_without_unique_body_proof_remains_unresolved() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let body = |slot| Body {
        id: BodyId::mint(format!("f3d:brep:body#{slot}")).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![
            RegionId::mint(format!("test:model:region#{slot}")).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let bodies = [body(1), body(2)];
    let regions = [
        Region {
            id: RegionId::mint("test:model:region#1").expect("identity grammar"),
            body: bodies[0].id.clone(),
            shells: vec![ShellId::mint("test:model:shell#1").expect("identity grammar")],
        },
        Region {
            id: RegionId::mint("test:model:region#2").expect("identity grammar"),
            body: bodies[1].id.clone(),
            shells: vec![ShellId::mint("test:model:shell#2").expect("identity grammar")],
        },
    ];
    let shells = [
        Shell::with_face(
            ShellId::mint("test:model:shell#1").expect("identity grammar"),
            RegionId::mint("test:model:region#1").expect("identity grammar"),
            FaceId::mint("test:model:face#1").expect("identity grammar"),
        ),
        Shell::with_face(
            ShellId::mint("test:model:shell#2").expect("identity grammar"),
            RegionId::mint("test:model:region#2").expect("identity grammar"),
            FaceId::mint("test:model:face#2").expect("identity grammar"),
        ),
    ];
    let operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:body-recipe#1".into(),
            scope_record_index: 10,
            owner: crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            },
            record_index: 1,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![
                crate::records::topology::body_recipe::DesignBodyRecipeReference {
                    design_reference: 301,
                    design_reference_offset: 25,
                    form: 33,
                    form_offset: 33,
                    candidate_faces: vec![
                        FaceId::mint("test:model:face#1").expect("identity grammar"),
                        FaceId::mint("test:model:face#2").expect("identity grammar"),
                    ],
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: Vec::new(),
                },
            ],
            nested_record_index: 4,
            nested_record_index_offset: 38,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#3".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 5,
            next_byte_offset: 256,
        },
    )
    .unwrap();

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut candidates =
        crate::history::body_candidates::BodyCandidates::new(&bodies, &regions, &shells, &[], &[]);
    for _ in 0..1_000 {
        assert_eq!(candidates.external(&operand, None), None);
    }
    ctx.finish_session().unwrap();
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(&bodies, &regions, &shells, &[], &[])
            .external(&operand, None),
        None
    );

    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        10,
    );
    let native = "f3d:Design/BulkStream.dat:design-record#1".to_owned();
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: &[],
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &bodies,
        regions: &regions,
        shells: &shells,
    };
    let mut selection = BodySelection::NativeSet(vec![native.clone()].try_into().unwrap());
    bind_direct_body_recipe_body_selection(
        &mut selection,
        &scope,
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    assert_eq!(
        selection,
        BodySelection::NativeSet(vec![native].try_into().unwrap())
    );
}
