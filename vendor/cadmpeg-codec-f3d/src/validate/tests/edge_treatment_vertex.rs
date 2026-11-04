// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand {
    use crate::records::{
        feature::work_geometry::{
            DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignVertexRecipeDraft,
        },
        references::DesignClassTag,
    };
    DesignEdgeTreatmentVertexOperand {
        id: "f3d:Design/BulkStream.dat:edge-treatment-vertex-operand#100".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        group_record_index: 20,
        group_member_ordinal: 0,
        recipe: DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
            record_index: 100,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("306".to_owned()).unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
            recipe_record_index: 103,
            recipe_record_byte_offset: 1_032,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            recipe_prefix_offset: 1_043,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 1_050,
            recipe_program: vec![0],
            resolution: None,
            next_record_index: 105,
            next_byte_offset: 1_054,
        })
        .unwrap(),
    }
}

fn vertex_recipe_valid_at(recipe_offset: u64) -> bool {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            scope::{DesignFeatureKind, DesignParameterScope},
            work_geometry::DesignVertexRecipe,
        },
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
    };
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut draft = operand().recipe.into_draft();
        draft.byte_offset = recipe_offset - 47;
        draft.paired_byte_offset = recipe_offset - 31;
        draft.recipe_record_byte_offset = recipe_offset - 15;
        draft.recipe_prefix_offset = recipe_offset - 4;
        draft.recipe_program_offset = recipe_offset.saturating_add(18);
        draft.next_byte_offset = draft.recipe_program_offset.saturating_add(4);
        let vertex = DesignVertexRecipe::try_new(draft).unwrap();
        let stream = super::super::design_stream(&vertex.recipe_id);
        let scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#10",
            DesignFeatureKind::WorkPoint,
            10,
        );
        let native = crate::native::F3dNative {
            design_record_headers: vec![DesignRecordHeader {
                id: "f3d:Design/BulkStream.dat:header#100".into(),
                record_index: 100,
                class_tag: vertex.class_tag.clone(),
                byte_offset: vertex.byte_offset(),
            }],
            construction_recipes: vec![ConstructionRecipe {
                id: vertex.recipe_id.clone(),
                byte_offset: recipe_offset,
                kind: ConstructionRecipeKind::Vertex,
                design: None,
                recipe_index: 0,
                record_index: None,
            }],
            ..Default::default()
        };
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        super::super::valid_vertex_recipe(&ctx, &scope, stream, 100, &vertex)
    })
}

#[test]
fn vertex_recipe_preserves_representable_program_layout() {
    assert!(vertex_recipe_valid_at(1_047));
}

#[test]
fn vertex_recipe_rejects_overflowed_program_origin() {
    assert!(!vertex_recipe_valid_at(u64::MAX - 17));
}

#[test]
fn vertex_recipe_rejects_overflowed_program_extent() {
    assert!(!vertex_recipe_valid_at(u64::MAX - 20));
}
