// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_binding_projects_a_datum_or_retains_native_with_loss() {
    use crate::design::decode::placement_construction::work_planes::bind;
    use crate::design::feature_project::{
        project_parameter_design_with_edge_identities, ProjectInputs,
    };
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let placement = crate::test_support::work_plane::placements().remove(0);
    for admitted in [true, false] {
        let mut scopes = vec![crate::test_support::work_plane::scope(&placement)];
        let placements = if admitted {
            vec![placement.clone()]
        } else {
            vec![]
        };
        let losses = bind(&mut scopes, &placements);
        assert_eq!(losses.len(), usize::from(!admitted));
        if !admitted {
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::WorkPlaneFrameUnbound
                    .note("")
                    .code
            );
        }
        crate::test_support::with_decode_context(|ctx| {
            let timelines = crate::design::test_support::synthetic_feature_timelines(&scopes);
            let projected = project_parameter_design_with_edge_identities(
                ctx,
                &ProjectInputs {
                    scopes: &scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(projected.features.len(), 1);
            let definition = projected.features[0].evaluation.definition();
            if admitted {
                assert!(matches!(
                    definition,
                    FeatureDefinition::Operation(FeatureOperation::DatumPlane { .. })
                ));
            } else {
                assert!(matches!(
                    definition,
                    FeatureDefinition::Operation(FeatureOperation::Native { .. })
                ));
            }
        });
    }
}
