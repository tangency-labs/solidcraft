// SPDX-License-Identifier: Apache-2.0
use super::super::{
    edge_recipe_structure, edge_recipe_structure_within, face_recipe_nodes, FaceRecipeProgramKind,
    RECIPE_SIDE_SEARCH_WORK,
};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
};
use cadmpeg_core::CodecError;

const EDGE_RECIPE: [i32; 25] = [
    -1, -1, 2, 0, -1, 1, -1, 1, -1, 2, 0, -1, 1, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1,
];

#[test]
fn edge_recipe_structure_refuses_recursion_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        edge_recipe_structure(&ctx, &EDGE_RECIPE),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RecursionDepth
                && failure.operation == "f3d recipe side recursion"
    ));
}

#[test]
fn face_recipe_nodes_split_at_each_node_marker() {
    let program = [0, -1, 4, -1, -1, 2, 7];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let nodes = face_recipe_nodes(
        &ctx,
        &program,
        0,
        FaceRecipeProgramKind::Counted { header_value: 4 },
    )
    .unwrap()
    .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(nodes[0].byte_offset, 12);
}

/// An edge-recipe program whose one counted side has 3,000 payload-prefix
/// splits, so the side search needs more work than its cap allows.
fn exhausting_edge_recipe_program() -> Vec<i32> {
    let mut program = vec![-1, -1, 2, 0, -1, 1, -1];
    program.extend_from_slice(&[1, -1, 2, 5, -1, 7, -1]);
    program.extend_from_slice(&[0; 3000]);
    program
}

#[test]
fn edge_recipe_side_search_skips_the_structure_past_its_work_cap() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let program = exhausting_edge_recipe_program();
    assert_eq!(edge_recipe_structure(&ctx, &program).unwrap(), None);
    let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
    assert_eq!(
        edge_recipe_structure_within(&ctx, &budget, &program).unwrap(),
        None
    );
    assert!(budget.exhausted());
    assert_eq!(ctx.resource_refusal(), None);

    // The same search on a resolvable program completes within the cap and
    // keeps its structure; a cap too small for it skips the structure.
    let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
    assert!(edge_recipe_structure_within(&ctx, &budget, &EDGE_RECIPE)
        .unwrap()
        .is_some());
    assert!(!budget.exhausted());
    let budget = WorkBudget::new(1);
    assert_eq!(
        edge_recipe_structure_within(&ctx, &budget, &EDGE_RECIPE).unwrap(),
        None
    );
    assert!(budget.exhausted());
}

#[test]
fn edge_recipe_admits_variable_preludes_single_headers_and_empty_extended_payloads() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for prelude in [
        vec![-1, -1, 2, 0, 1, 1, 0],
        vec![0, 1, 1, 1, -1],
        vec![0, 0, 2, 1, 1, 2, 0],
    ] {
        for field in [
            vec![0],
            vec![0, -1, 0, 0, -1],
            vec![0, 0, 0, 0, -1],
            vec![2, 0, 0, 0, -1, 0, -1, 0, 0, -1],
        ] {
            for tag in [-1, 0, 1] {
                let mut program = prelude.clone();
                program.extend_from_slice(&[1, 1, 1, 0, 1]);
                program.extend_from_slice(&field);
                program.extend_from_slice(&[0, tag]);
                let parsed = edge_recipe_structure(&ctx, &program)
                    .unwrap()
                    .expect("one complete side");
                assert_eq!(parsed.root, 1);
                assert_eq!(parsed.sides.len(), 1);
                assert_eq!(parsed.sides[0].header_value, 0);
                assert!(parsed.sides[0].scalars.is_empty());
                assert_eq!(parsed.sides[0].payload_prefix, field);
                assert!(parsed.sides[0].entries.is_empty());
                program.pop();
                assert!(edge_recipe_structure(&ctx, &program).unwrap().is_none());
                program.push(2);
                assert!(edge_recipe_structure(&ctx, &program).unwrap().is_none());
            }
        }
    }
}

#[test]
fn edge_recipe_requires_valid_prelude_ordinals_tags_and_complete_terminal() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let program = [-1, -1, 2, 0, -1, 1, -1, 1, -1, 1, 0, -1, 0, 0, -1];
    assert!(edge_recipe_structure(&ctx, &program).unwrap().is_some());
    for offset in [0, 1, 2, 3, 4, 5, 6, 8, 11, 14] {
        let mut invalid = program;
        invalid[offset] = 9;
        assert!(
            edge_recipe_structure(&ctx, &invalid).unwrap().is_none(),
            "offset {offset}"
        );
    }
    for invalid in [
        vec![0, -1, 0],
        vec![0, -1, 1, 2, -1, 1, -1, 1, 0, -1, 0, 0, -1],
        vec![0, 2, 1, 1, -1, 1, -1, 1, 0, -1, 0, 0, -1],
    ] {
        assert!(edge_recipe_structure(&ctx, &invalid).unwrap().is_none());
    }
}

#[test]
fn face_recipe_retains_its_delimiter_and_header_count_domain() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let program = [
        0, -1, 2, -1, 3, -1, 2, 0, -1, 1, -1, 0, 0, -1, 2, 0, -1, 1, -1, 0, 0, -1,
    ];
    assert!(super::super::face_recipe_structure(&ctx, &program)
        .unwrap()
        .is_some());
    for offset in [8, 10, 13, 16, 18] {
        let mut invalid = program;
        invalid[offset] = 1;
        assert!(super::super::face_recipe_structure(&ctx, &invalid)
            .unwrap()
            .is_none());
    }
    let mut single_header = program.to_vec();
    single_header[6] = 1;
    single_header.drain(9..11);
    assert!(super::super::face_recipe_structure(&ctx, &single_header)
        .unwrap()
        .is_none());
}
