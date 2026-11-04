// SPDX-License-Identifier: Apache-2.0
use crate::design::edge_resolve::resolved_edge_candidate_intersection;

#[test]
fn edge_recipe_candidate_intersection_must_be_uniquely_corroborated() {
    use crate::records::topology::{
        edge_recipe::DesignEdgeRecipeSelectorContext, edge_recipe::DesignTopologyIncidentSide,
        edge_recipe::DesignTopologyRecipeEntry, edge_recipe::DesignTopologyRecipeTriplet,
    };

    let selector = |selector, edges: &[i64]| DesignEdgeRecipeSelectorContext {
        selector,
        clauses: vec![None, None],
        incidence_matching_edge_slots: edges.to_vec(),

        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selector_with_counts = |ordinal: i32, incidence: &[i64], counts: &[i64]| {
        let mut context = selector(ordinal, incidence);
        context.boundary_count_matching_edge_slots = counts.to_vec();
        context
    };
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 19])],
            [&[17, 20][..], &[15, 17][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17]), selector(1, &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17]), selector(1, &[])], [&[17][..]],),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17])], [&[][..]]),
        None
    );
    assert_eq!(resolved_edge_candidate_intersection(&[], [&[17][..]]), None);
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 19]),
                selector_with_counts(1, &[17, 20], &[17, 21]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 18]),
                selector_with_counts(1, &[17, 18], &[17, 18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17], &[18]),
                selector_with_counts(1, &[17], &[18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 19][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[][..], &[17, 18][..], &[][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 18][..]]),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[18])], [&[17, 18][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[], &[17, 18]),
                selector_with_counts(1, &[], &[17, 19]),
            ],
            [&[17, 20][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector_with_counts(0, &[17], &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17][..]]
        ),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]]
        ),
        Some(vec![18])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17][..]]
        ),
        None
    );
    let assignment_candidates = [
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::unique_bipartite_assignment(
                &assignment_candidates,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![17, 18])
    );
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(3).unwrap(),
        middle: 2,
        incident: Some(
            crate::records::topology::edge_recipe::DesignTopologyIncident {
                ordinal: 1,
                side: DesignTopologyIncidentSide::Preceding,
            },
        ),
    };
    let mut common = selector(0, &[]);
    common.clauses[0] = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
            entry: DesignTopologyRecipeEntry {
                selector: 0,
                boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
                topology_triplets: [triplet, triplet],
            },
            triplet_edge_slots: [vec![17, 18], vec![17]],
        },
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17, 18][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[][..]]),
        Some(17)
    );
    let mut common = selector(0, &[]);
    common.clauses[0] = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
            entry: DesignTopologyRecipeEntry {
                selector: 0,
                boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
                topology_triplets: [triplet, triplet],
            },
            triplet_edge_slots: [vec![17, 18, 19], vec![17, 18]],
        },
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[19][..]]),
        None
    );
    let clause = |triplet_edge_slots| {
        Some(
            crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
                entry: DesignTopologyRecipeEntry {
                    selector: 0,
                    boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
                    topology_triplets: [
                        triplet,
                        DesignTopologyRecipeTriplet {
                            outer: std::num::NonZeroU32::new(4).unwrap(),
                            incident: Some(
                                crate::records::topology::edge_recipe::DesignTopologyIncident {
                                    ordinal: 2,
                                    side: DesignTopologyIncidentSide::Preceding,
                                },
                            ),
                            ..triplet
                        },
                    ],
                },
                triplet_edge_slots,
            },
        )
    };
    let mut cross_clause = selector(0, &[]);
    cross_clause.clauses = vec![
        clause([vec![18], vec![17, 19]]),
        clause([vec![20], vec![17]]),
    ];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], std::iter::empty::<&[i64]>(),),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[17, 21][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[18][..]]),
        None
    );
    cross_clause.clauses = vec![clause([vec![18], vec![17]]), clause([vec![18], vec![17]])];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause], std::iter::empty::<&[i64]>(),),
        None
    );
}
