// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::test_support::indexed_header;

fn parse_edge_operand(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    ordinal: u32,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
    terminal_group_limit: Option<u64>,
) -> Option<crate::records::topology::edge_identity::DesignEdgeOperand> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    crate::design::decode::operands::parse_edge_operand(
        &ctx,
        bytes,
        records,
        scope,
        (ordinal, header),
        recipes,
        terminal_group_limit,
    )
    .map(|result| result.expect("recipe allocation admitted"))
}

fn parse_face_operand(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    (ordinal, group_ownership): (u32, Option<(u32, u32)>),
    next_byte_offset: Option<u64>,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<crate::records::topology::face::DesignFaceOperand> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test context");
    crate::design::decode::operands::parse_face_operand(
        &ctx,
        bytes,
        records,
        crate::design::decode::operands::FaceOperandFrame {
            scope,
            scope_reference_ordinal: ordinal,
            group_ownership,
            next_byte_offset,
            header,
        },
        recipes,
    )
    .map(|result| result.expect("recipe allocation admitted"))
}

fn parse_vertex_recipe(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    stream: &str,
    header: &DesignRecordHeader,
    recipes: &[ConstructionRecipe],
) -> Option<crate::records::feature::work_geometry::DesignVertexRecipe> {
    crate::design::decode::operands::parse_vertex_recipe(bytes, records, stream, header, recipes)
}

#[test]
fn surface_patch_long_field_rejected_before_copy() {
    let mut program = vec![0; 7];
    program.extend_from_slice(&[2, 1, 2, 3, -1]);
    assert!(crate::design::decode::operands::surface_patch_recipe_structure(&program, 4).is_none());
}

#[test]
fn face_recipe_boundary_accepts_omitted_n_plus_four() {
    let mut ordinary = Vec::new();
    for record_index in 100..=104 {
        indexed_header(&mut ordinary, *b"306", record_index);
    }
    let ordinary_position = ordinary.len() - 11;
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &ordinary,
            ordinary_position,
            100,
            None,
        ),
        Some((ordinary_position, 104))
    );

    let mut omitted = Vec::new();
    for record_index in 100..=103 {
        indexed_header(&mut omitted, *b"306", record_index);
    }
    let position = omitted.len();
    indexed_header(&mut omitted, *b"124", 0);
    let next = omitted.len();
    indexed_header(&mut omitted, *b"317", 105);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(&omitted, position, 100, None),
        Some((next, 105))
    );

    let mut arbitrary = Vec::new();
    for record_index in 100..=103 {
        indexed_header(&mut arbitrary, *b"306", record_index);
    }
    let arbitrary_position = arbitrary.len();
    indexed_header(&mut arbitrary, *b"124", 205);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &arbitrary,
            arbitrary_position,
            100,
            None,
        ),
        Some((arbitrary_position, 205))
    );
}

mod group_records;
mod nested_records;

mod face_prefix;
