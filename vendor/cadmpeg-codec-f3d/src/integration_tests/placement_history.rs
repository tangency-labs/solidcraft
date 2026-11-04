// SPDX-License-Identifier: Apache-2.0
//! Occurrence placements of a Design whose duplicate `Component Insert`
//! envelopes resolve through ASM history header links, decoded under
//! retained-byte allowances with and without room for history binding.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeResult};
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;
use crate::test_support::body_map_test::f3d_with_breps_and_design;
use crate::test_support::component_insert_test::{
    component_insert_design, ComponentInsertEnvelope,
};
use crate::test_support::history_stream::DesignScale;
use crate::F3dCodec;

const SCALE: DesignScale = DesignScale {
    states: 20,
    bodies: 20,
    faces_per_body: 1,
    updates_per_state: 2,
};

/// State ids count down from the head, so state 3 links to state 2.
const HISTORY_BOUND: ComponentInsertEnvelope = ComponentInsertEnvelope {
    translation_cm: [1.0, 2.0, 3.0],
    history_state_id: 3,
    previous_history_state_id: 2,
};

/// A later envelope whose state pair is absent from the history.
const UNBOUND: ComponentInsertEnvelope = ComponentInsertEnvelope {
    translation_cm: [4.0, 5.0, 6.0],
    history_state_id: 1_000,
    previous_history_state_id: 999,
};

struct Decoded {
    result: DecodeResult,
    retained_bytes: u64,
}

fn design() -> Vec<u8> {
    let (bulk, meta) = component_insert_design(&[HISTORY_BOUND, UNBOUND]);
    f3d_with_breps_and_design(
        &[("BREP.synthetic.smbh", &SCALE.history_smbh())],
        &bulk,
        &meta,
    )
}

fn decode(bytes: &[u8], max_retained_bytes: u64) -> Result<Decoded, String> {
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .map_err(|error| error.to_string())?;
    let available = ctx.retained_bytes_available();
    let result = F3dCodec.decode_with_context(&ctx, root);
    let retained_bytes = available - ctx.retained_bytes_available();
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(format!("{refusal:?}"));
    }
    ctx.finish_session().map_err(|error| error.to_string())?;
    Ok(Decoded {
        result: result.map_err(|error| error.to_string())?,
        retained_bytes,
    })
}

/// Decode under the desktop allowance, where history binding fits.
fn decode_bound(bytes: &[u8]) -> Decoded {
    let bound = decode(bytes, DecodePolicy::desktop().limits.max_retained_bytes)
        .unwrap_or_else(|error| panic!("the design decodes under the desktop allowance: {error}"));
    assert!(!binding_skipped(&bound.result));
    bound
}

/// Decode under each `step / steps` fraction of `full_charge` below it.
fn below_full_charge(
    bytes: &[u8],
    full_charge: u64,
    steps: u64,
) -> Vec<(u64, Result<Decoded, String>)> {
    (1..steps)
        .map(|step| {
            let allowance = full_charge * step / steps;
            (allowance, decode(bytes, allowance))
        })
        .collect()
}

fn binding_skipped(result: &DecodeResult) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoryBindingBudgetExceeded.kind())
}

#[test]
fn duplicate_component_insert_placements_do_not_depend_on_the_retained_allowance() {
    let bytes = design();
    let bound = decode_bound(&bytes);
    let occurrences = &bound.result.ir().model.occurrences;
    assert_eq!(occurrences.len(), 1, "{occurrences:#?}");
    let expected = Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 20.0],
        [0.0, 0.0, 1.0, 30.0],
    ])
    .unwrap();
    assert_eq!(occurrences[0].transform, expected);

    for (allowance, outcome) in below_full_charge(&bytes, bound.retained_bytes, 16) {
        if let Ok(decoded) = outcome {
            assert_eq!(
                &decoded.result.ir().model.occurrences,
                occurrences,
                "allowance {allowance}"
            );
        }
    }
}

#[test]
fn an_allowance_that_skips_history_binding_keeps_the_placements() {
    let bytes = design();
    let bound = decode_bound(&bytes);
    let mut outcomes = Vec::new();
    let mut skipped = None;
    for (allowance, outcome) in below_full_charge(&bytes, bound.retained_bytes, 64) {
        match outcome {
            Ok(decoded) if binding_skipped(&decoded.result) => skipped = Some(decoded),
            Ok(decoded) => outcomes.push(format!("{allowance}: bound, {}", decoded.retained_bytes)),
            Err(error) => outcomes.push(format!("{allowance}: {error}")),
        }
    }
    let skipped = skipped.unwrap_or_else(|| {
        panic!(
            "no allowance skips history binding and still decodes; full charge {}: {outcomes:#?}",
            bound.retained_bytes
        )
    });
    assert_eq!(
        skipped.result.ir().model.occurrences,
        bound.result.ir().model.occurrences
    );
}

#[test]
fn attributed_local_children_repeat_under_each_containing_instance() {
    const TOLERANCE: f64 = 1e-12;
    let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(&[300]);
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    let model = &result.ir().model;
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.product_definitions.len(), 3);
    assert_eq!(model.occurrences.len(), 7);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::LocalOccurrenceUnplaced.kind()));
    let graph = cadmpeg_ir::products::AssemblyGraph::new(&model.occurrences).unwrap();
    let leaves = model
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.id.as_str().contains(':')
                && occurrence
                    .id
                    .as_str()
                    .split('#')
                    .next_back()
                    .unwrap()
                    .contains(':')
        })
        .collect::<Vec<_>>();
    assert_eq!(leaves.len(), 4);
    assert!(leaves
        .iter()
        .all(|leaf| leaf.prototype == leaves[0].prototype));
    for leaf in leaves {
        let pose = cadmpeg_test_support::assembly::resolved_transform(&graph, &leaf.id).unwrap();
        let child_b = leaf.id.as_str().ends_with("000000000403");
        let face = pose
            .apply_point(cadmpeg_ir::math::Point3::new(
                if child_b { 0.0 } else { 10.0 },
                0.0,
                0.0,
            ))
            .unwrap()
            .get();
        assert!((face.y - 50.0).abs() < TOLERANCE);
        assert!((face.x - 30.0).abs() < TOLERANCE);
    }
}

#[test]
fn only_absent_membership_uses_the_component_cluster() {
    for owners in [&[][..], &[200, 300][..], &[999][..]] {
        let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(owners);
        let bytes = f3d_with_breps_and_design(
            &[(
                "BREP.module.smbh",
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(
            result
                .ir()
                .model
                .product_definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            usize::from(owners.is_empty())
        );
        assert!(result.report().losses.iter().any(|loss| loss.code
            == if owners.is_empty() {
                F3dLossCode::ComponentBodyOwnerFallback.kind()
            } else {
                F3dLossCode::ComponentBodyOwnerUnresolved.kind()
            }));
    }
}

#[test]
fn history_only_body_map_pair_leaves_the_current_model_and_reports_its_loss() {
    use crate::test_support::component_body_test::{
        history_only_design, HistoryOnlyContainer as Container,
    };
    for (hidden, container, bodies) in [
        (true, Container::Lists(&[]), 0),
        (false, Container::Lists(&[]), 1),
        (true, Container::Lists(&[350]), 1),
        (true, Container::Malformed, 1),
    ] {
        let (bulk, meta) = history_only_design("BREP.module.smbh", Some(hidden), container);
        let bytes = f3d_with_breps_and_design(
            &[(
                "BREP.module.smbh",
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(result.ir().model.bodies.len(), bodies, "{hidden}");
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == F3dLossCode::HistoryOnlyBodyOmitted.kind()),
            bodies == 0,
            "{hidden}"
        );
        assert!(!result.report().losses.iter().any(
            |loss| bodies == 0 && loss.code == F3dLossCode::DesignBodyBindingUnresolved.kind()
        ));
    }
}

#[test]
fn consumed_pair_beside_a_listed_survivor_keeps_exactly_the_survivor() {
    use crate::test_support::component_body_test::{
        history_only_pairs_design, HistoryOnlyContainer as Container,
    };
    const BLOB: &str = "BREP.module.smbh";
    for (keys, survivor_key, consumed_key) in [([10, 20], 10, 20), ([10, 20], 20, 10)] {
        let (bulk, meta) = history_only_pairs_design(
            BLOB,
            &[
                (survivor_key, 350, Some(false)),
                (consumed_key, 351, Some(true)),
            ],
            Container::Lists(&[350]),
        );
        let bytes = f3d_with_breps_and_design(
            &[(
                BLOB,
                &crate::test_support::smbh_geometry_test::synthetic_two_body_smbh(keys),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(result.ir().model.bodies.len(), 1, "{survivor_key}");
        assert_eq!(result.ir().model.faces.len(), 1, "{survivor_key}");
        let native = crate::test_support::native_test::f3d_native(result.ir());
        let native_keys = native
            .body_native_keys
            .iter()
            .map(|key| key.asm_body_key)
            .collect::<Vec<_>>();
        assert_eq!(native_keys, [Some(survivor_key)]);
        let mut pairs = native
            .design_body_bindings
            .iter()
            .map(|binding| {
                (
                    binding.entity_suffix,
                    binding.history_only,
                    binding.body.is_some(),
                    binding.component_owner.is_some(),
                )
            })
            .collect::<Vec<_>>();
        pairs.sort_unstable();
        assert_eq!(pairs, [(350, false, true, true), (351, true, false, false)]);
        let codes = result
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>();
        assert!(codes.contains(&F3dLossCode::HistoryOnlyBodyOmitted.kind()));
        assert!(!codes.contains(&F3dLossCode::DesignBodyBindingUnresolved.kind()));
    }
}

#[test]
fn no_physical_model_requires_every_body_map_pair_to_be_history_only() {
    use crate::test_support::component_body_test::{
        history_only_pairs_design, HistoryOnlyContainer as Container,
    };
    const BLOB: &str = "BREP.module.smbh";
    const NO_PHYSICAL_MODEL: &str = "No physical model BREP was selected";
    const EMPTY_SELECTION: &str = "the selected model has no surfaces";
    // A live selector that names no body leaves the blob empty, so the reported
    // outcome depends on the pair predicate alone.
    for (pairs, listed, expected, absent) in [
        (
            &[(99, 350, Some(false)), (42, 351, Some(true))][..],
            &[350][..],
            EMPTY_SELECTION,
            NO_PHYSICAL_MODEL,
        ),
        (
            &[(42, 350, Some(true)), (43, 351, Some(true))][..],
            &[][..],
            NO_PHYSICAL_MODEL,
            EMPTY_SELECTION,
        ),
    ] {
        let (bulk, meta) = history_only_pairs_design(BLOB, pairs, Container::Lists(listed));
        let bytes = f3d_with_breps_and_design(
            &[(
                BLOB,
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert!(result.ir().model.bodies.is_empty(), "{expected}");
        let has = |text: &str| {
            result
                .report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(text))
        };
        assert!(has(expected), "{expected}");
        assert!(!has(absent), "{expected}");
    }
}

#[test]
fn admitted_component_membership_wins_over_an_unregistered_owner() {
    for owners in [&[999, 300][..], &[300, 999][..]] {
        let (bulk, meta) = crate::test_support::streams_test::nested_local_component_design(owners);
        let bytes = f3d_with_breps_and_design(
            &[(
                "BREP.module.smbh",
                &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
            )],
            &bulk,
            &meta,
        );
        let result = super::decode(bytes);
        assert_eq!(
            result
                .ir()
                .model
                .product_definitions
                .iter()
                .flat_map(|definition| &definition.bodies)
                .count(),
            1
        );
        assert!(!result.report().losses.iter().any(|loss| loss.code
            == F3dLossCode::ComponentBodyOwnerFallback.kind()
            || loss.code == F3dLossCode::ComponentBodyOwnerUnresolved.kind()));
    }
}

#[test]
fn empty_attribute_local_children_place_identity_matrix_and_nested_occurrences() {
    use cadmpeg_ir::products::OccurrenceParent;
    let (bulk, meta) =
        crate::test_support::empty_attribute_occurrences::empty_attribute_local_component_design(
            &[300],
        );
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    super::assert_valid(&result);
    let model = &result.ir().model;
    assert_eq!(model.product_definitions.len(), 3);
    assert_eq!(model.occurrences.len(), 7);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::LocalOccurrenceUnplaced.kind()));
    let root = model
        .occurrences
        .iter()
        .find(|o| o.parent == OccurrenceParent::Root {})
        .unwrap();
    let children = model
        .occurrences
        .iter()
        .filter(|o| {
            o.parent
                == OccurrenceParent::Occurrence {
                    occurrence: root.id.clone(),
                }
        })
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2);
    assert_eq!(
        children
            .iter()
            .filter(|o| o.transform == Transform::identity())
            .count(),
        1
    );
    for child in children {
        assert_eq!(
            model
                .occurrences
                .iter()
                .filter(|o| o.parent
                    == OccurrenceParent::Occurrence {
                        occurrence: child.id.clone()
                    })
                .count(),
            2
        );
    }
}

#[test]
fn later_fixed_identity_occurrence_reports_loss_and_stays_out_of_neutral_structure() {
    let (bulk, meta) =
        crate::test_support::empty_attribute_occurrences::later_identity_local_component_design();
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    super::assert_valid(&result);
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::LocalOccurrenceIdentityOrdinalUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("ordinal 2"));
    assert_eq!(result.ir().model.occurrences.len(), 4);
    assert!(result
        .ir()
        .model
        .occurrences
        .iter()
        .all(|o| !o.id.as_str().ends_with("000000000404")));
}

#[test]
fn history_only_hidden_uncontained_body_with_a_bound_cluster_stays_in_model() {
    use crate::test_support::component_body_test::{
        history_only_pairs_with_cluster, HistoryOnlyContainer as Container,
    };
    let (bulk, meta) = history_only_pairs_with_cluster(
        "BREP.module.smbh",
        &[(0, 350, Some(true))],
        Container::Lists(&[]),
        true,
    );
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    assert_eq!(result.ir().model.bodies.len(), 1);
    let native = crate::test_support::native_test::f3d_native(result.ir());
    assert!(!native.design_body_bindings[0].history_only);
    assert!(native.design_body_bindings[0].body.is_some());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ComponentBodyOwnerFallback.kind()));
    super::assert_valid(&result);
}

#[test]
fn a_body_a_consuming_scope_names_leaves_the_model_with_a_bound_cluster() {
    use crate::design::decode::body_consumers::REMOVE_BODY_TYPE_GUID;
    use crate::test_support::component_body_test::{
        history_only_pairs_with_consumer, BodyConsumer, HistoryOnlyContainer as Container,
        BODY_CONSUMER_SCOPE,
    };
    let (bulk, meta) = history_only_pairs_with_consumer(
        "BREP.module.smbh",
        &[(0, 350, None)],
        Container::Lists(&[]),
        true,
        Some(BodyConsumer {
            scope_type_guid: REMOVE_BODY_TYPE_GUID,
            entity: 350,
            hidden: 1,
            flag: 1,
            selection_owner: Some(BODY_CONSUMER_SCOPE),
            registered_node: true,
        }),
    );
    let bytes = f3d_with_breps_and_design(
        &[(
            "BREP.module.smbh",
            &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
        )],
        &bulk,
        &meta,
    );
    let result = super::decode(bytes);
    assert!(result.ir().model.bodies.is_empty());
    let native = crate::test_support::native_test::f3d_native(result.ir());
    assert!(native.design_body_bindings[0].history_only);
    assert!(native.design_body_bindings[0].body.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoryOnlyBodyOmitted.kind()));
    super::assert_valid(&result);
}

#[test]
fn history_only_consumed_member_keeps_the_inserted_design_body() {
    use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
    use crate::test_support::component_body_test::{
        history_only_pairs_with_cluster, HistoryOnlyContainer as Container,
    };
    let geometry = crate::test_support::smbh_geometry_test::synthetic_geometry_smbh();
    let document = |bound_cluster| {
        let (bulk, meta) = history_only_pairs_with_cluster(
            "BREP.module.smbh",
            &[(0, 350, Some(true))],
            Container::Lists(&[]),
            bound_cluster,
        );
        f3d_with_breps_and_design(&[("BREP.module.smbh", &geometry)], &bulk, &meta)
    };
    let consumed = document(false);
    let inserted = document(true);
    let root = f3d_without_brep(
        "assembly",
        "root.f3d",
        &[("consumed.f3d", XREF_ROLE), ("inserted.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("consumed.f3d", &consumed),
            ("inserted.f3d", &inserted),
        ],
    );
    let result = super::decode(archive);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::HistoryOnlyBodyOmitted.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::ComponentBodyOwnerFallback.kind()));
    super::assert_valid(&result);
}
