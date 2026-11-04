// SPDX-License-Identifier: Apache-2.0

#[test]
fn face_operand_rejects_overflowed_node_offset() {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        references::DesignClassTag,
        topology::face::{DesignFaceOperand, DesignFaceOperandDraft},
    };

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let stream = "f3d:Design/BulkStream.dat";
    let recipe_id = format!("{stream}:construction-recipe#0");
    let program = vec![0, -1];
    let next_byte_offset = 1_063 + u64::try_from(program.len()).unwrap() * 4;
    let mut operand = DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("{stream}:design-face-operand#100"),
        scope_record_index: 10,
        scope_reference_ordinal: 2,
        group: None,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
        paired_byte_offset: 1_016,
        paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
        recipe_record_index: 103,
        recipe_record_byte_offset: 1_032,
        recipe_id,
        recipe_prefix_offset: 1_043,
        recipe_references: Vec::new(),
        recipe_prefix_bytes: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1_063,
        recipe_program: program,
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 105,
        next_byte_offset,
    })
    .unwrap();
    operand.recipe_program_offset = u64::MAX;
    operand.recipe_program = vec![0, -1, -1, 2];
    let native = crate::native::F3dNative {
        design_face_operands: vec![operand],
        ..Default::default()
    };
    let error = crate::test_support::with_decode_context(|service_ctx| {
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_face_operands(&ctx, &mut Vec::new(), &[]).unwrap_err()
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::Malformed(ref message)
        if message == "F3D face recipe node offset overflows")
    );
}

#[test]
fn full_round_face_selection_needs_a_resolved_or_candidate_face() {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        references::DesignClassTag,
        topology::face::{DesignFaceOperand, DesignFaceOperandDraft},
    };
    use cadmpeg_ir::ids::FaceId;

    let stream = "f3d:Design/BulkStream.dat";
    let operand = || {
        DesignFaceOperand::try_new(DesignFaceOperandDraft {
            id: format!("{stream}:design-face-operand#100"),
            scope_record_index: 10,
            scope_reference_ordinal: 2,
            group: None,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("365".to_owned()).unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: DesignClassTag::try_from("366".to_owned()).unwrap(),
            recipe_record_index: 103,
            recipe_record_byte_offset: 1_032,
            recipe_id: format!("{stream}:construction-recipe#0"),
            recipe_prefix_offset: 1_043,
            recipe_references: Vec::new(),
            recipe_prefix_bytes: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 1_063,
            recipe_program: vec![0, -1],
            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 105,
            next_byte_offset: 1_071,
        })
        .unwrap()
    };
    let face = FaceId::mint("f3d:brep:entity#12").expect("identity grammar");
    assert!(!super::super::full_round_face_is_selected(&operand()));
    let mut resolved = operand();
    resolved.resolved_face_slots = vec![12];
    assert!(super::super::full_round_face_is_selected(&resolved));
    let mut preceding = operand();
    preceding.preceding_candidate_faces = vec![face.clone()];
    assert!(super::super::full_round_face_is_selected(&preceding));
    let mut changed = operand();
    changed.changed_candidate_faces = vec![face];
    assert!(super::super::full_round_face_is_selected(&changed));
}

#[test]
fn revolve_axis_admits_face_and_bounded_face_recipes() {
    use crate::records::recipes::ConstructionRecipeKind as Kind;
    assert!(super::super::revolve_axis_recipe_kind_admitted(Kind::Face));
    assert!(super::super::revolve_axis_recipe_kind_admitted(
        Kind::BoundedFace
    ));
    assert!(!super::super::revolve_axis_recipe_kind_admitted(Kind::Body));
}

#[test]
fn mirror_admits_its_selection_runs_and_bounded_plane_operand() {
    use crate::records::recipes::ConstructionRecipeKind as Kind;
    use crate::records::topology::extrude_selection::DesignOperandRole as Role;
    for role in [
        Role::BODIES_A,
        Role::BODIES_B,
        Role::ROLE_0X5,
        Role::ROLE_0X10,
    ] {
        assert!(super::super::mirror_group_role_admitted(role));
    }
    for role in [Role::PROFILE, Role::FACES, Role::ROLE_0X21, Role::ROLE_0X7] {
        assert!(!super::super::mirror_group_role_admitted(role));
    }
    assert!(super::super::mirror_operand_recipe_admitted(
        Role::BODIES_B,
        Kind::Face
    ));
    assert!(super::super::mirror_operand_recipe_admitted(
        Role::ROLE_0X5,
        Kind::BoundedFace
    ));
    assert!(!super::super::mirror_operand_recipe_admitted(
        Role::BODIES_B,
        Kind::BoundedFace
    ));
    assert!(super::super::mirror_operand_recipe_admitted(
        Role::ROLE_0X5,
        Kind::Face
    ));
    assert!(!super::super::mirror_operand_recipe_admitted(
        Role::ROLE_0X5,
        Kind::Edge
    ));
    assert!(!super::super::mirror_operand_recipe_admitted(
        Role::ROLE_0X10,
        Kind::Face
    ));
}
