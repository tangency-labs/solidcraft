// SPDX-License-Identifier: Apache-2.0

use crate::native::F3dNative;
use crate::records::feature::scope::DesignScopePayload;
use crate::test_support::pipe::{Config, Fixture};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

fn project(native: &F3dNative) -> Vec<cadmpeg_ir::features::Feature> {
    let timelines =
        crate::design::test_support::synthetic_feature_timelines(&native.design_parameter_scopes);
    crate::test_support::with_decode_context(|ctx| {
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                owners: &native.design_parameter_owners,
                scopes: &native.design_parameter_scopes,
                timelines: &timelines,
                construction_groups: &native.design_construction_operand_groups,
                ..Default::default()
            },
        )
        .unwrap()
        .features
    })
}

#[test]
fn pipe_dispatch_preserves_native_facts_without_inventing_a_section() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for raw in [[1, 1], [0, 1], [127, 254]] {
        for hollow in [false, true] {
            for operation in [1, 2, 3, 4] {
                for fractions in [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.25, 0.75]] {
                    let native = Fixture::new(Config {
                        raw,
                        hollow,
                        operation,
                        values: [fractions[0], fractions[1], 0.6, 0.35],
                        ..Default::default()
                    })
                    .decode(&ctx)
                    .unwrap();
                    let DesignScopePayload::Pipe(Some(pipe)) =
                        native.design_parameter_scopes[0].payload()
                    else {
                        panic!("complete source fixture")
                    };
                    assert_eq!(pipe.is_hollow, hollow);
                    assert_eq!(pipe.opaque_post_operation, raw);
                    let owner = |lane: usize| {
                        native
                            .design_parameter_owners
                            .iter()
                            .find(|owner| owner.record_index() == pipe.record_indexes[lane])
                            .unwrap()
                            .evaluated_value()
                            .get()
                    };
                    assert_eq!(owner(2), 0.6);
                    assert_eq!(owner(3), 0.35);
                    let features = project(&native);
                    assert_eq!(features.len(), 1);
                    assert!(matches!(features[0].evaluation.definition(),
                        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                        if kind.as_str() == "Pipe"));
                    assert!(!serde_json::to_string(&features)
                        .unwrap()
                        .contains("circular_region"));
                }
            }
        }
    }
}

#[test]
fn pipe_native_dimensions_have_no_shape_specific_wall_predicate() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for thickness in [-1.0, 0.0, 0.35, 2.0] {
        for hollow in [false, true] {
            let native = Fixture::new(Config {
                hollow,
                values: [1.0, 0.0, 0.6, thickness],
                ..Default::default()
            })
            .decode(&ctx)
            .unwrap();
            assert!(matches!(
                native.design_parameter_scopes[0].payload(),
                DesignScopePayload::Pipe(Some(_))
            ));
            crate::records::feature::path_features::pipe::admit_native(&native).unwrap();
            let features = project(&native);
            assert!(matches!(
                features[0].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
            ));
        }
    }
}
