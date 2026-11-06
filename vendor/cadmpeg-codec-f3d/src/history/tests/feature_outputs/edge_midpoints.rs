// SPDX-License-Identifier: Apache-2.0
use super::{base_feature, scope_between, state};
use crate::history::bind_feature_outputs;

fn midpoint_fillet(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
    use cadmpeg_ir::features::{
        EdgeSelection, FeatureDefinition, FeatureOperation, NonEmptyMembers,
    };
    let mut feature = base_feature();
    feature.id = crate::ids::neutral_feature_id(scope);
    feature.native_ref = Some(scope.id.clone());
    feature.evaluation.set_outputs(Default::default());
    let previous = scope.previous_history_state_id().unwrap();
    let state = crate::ids::feature_input_topology_id(&feature.id, previous);
    let edge = crate::ids::history_input_edge_id(
        &crate::ids::feature_input_prefix(&feature.id, previous),
        10,
    );
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::historical(state, vec![edge], "native:group".into()).unwrap(),
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }),
        }));
    feature
}

fn midpoint_history(id: &str, x: f64) -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmHistoricalEdgeSample, AsmHistoricalTopology};
    let input = AsmHistoricalTopology {
        edges: vec![10],
        edge_samples: vec![AsmHistoricalEdgeSample {
            edge: 10,
            interior: cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
            tangent: None,
        }],
        ..Default::default()
    };
    let mut states = vec![
        state(3, 3, Some(2), AsmHistoricalTopology::default()),
        state(2, 2, Some(0), AsmHistoricalTopology::default()),
        state(0, 0, None, input),
    ];
    for (state, _, _) in &mut states {
        state.parent = id.into();
        state.id = format!("{id}:state#{}", state.state_id);
    }
    crate::test_support::history::history(id.into(), 0, None, false, states)
}

#[test]
fn edge_midpoints_use_named_input_state_and_bound_history() {
    for current in [3, 0] {
        let scope = scope_between(current, 0);
        let mut feature = midpoint_fillet(&scope);
        let mut histories = [
            midpoint_history("history:first", 1.0),
            midpoint_history("history:second", 2.0),
        ];
        let owners = std::collections::HashMap::from([(scope.id.clone(), histories[0].id.clone())]);
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut histories,
            &[],
            &owners,
        )
        .unwrap();
        assert_eq!(
            feature
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("edge_midpoints"))
                .map(String::as_str),
            Some("1,0,0")
        );
        assert!(!losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind()));
    }
}

#[test]
fn edge_midpoints_refuse_missing_or_ambiguous_input_history() {
    let scope = scope_between(3, 0);
    let histories = [
        midpoint_history("history:first", 1.0),
        midpoint_history("history:second", 2.0),
    ];
    for (mut histories, owners) in [
        (Vec::new(), std::collections::HashMap::new()),
        (histories.to_vec(), std::collections::HashMap::new()),
        (
            histories.to_vec(),
            std::collections::HashMap::from([(scope.id.clone(), "history:missing".into())]),
        ),
    ] {
        let mut feature = midpoint_fillet(&scope);
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut histories,
            &[],
            &owners,
        )
        .unwrap();
        assert!(!feature
            .source_properties
            .contains_key(&cadmpeg_core::nonblank_literal!("edge_midpoints")));
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.code
                    == crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind())
                .count(),
            1
        );
    }
}

#[test]
fn edge_midpoints_refuse_a_truncated_state_chain() {
    let scope = scope_between(3, 0);
    let mut history = midpoint_history("history:first", 1.0);
    history.states_mut()[1].next_ref = Some(99);
    let mut feature = midpoint_fillet(&scope);
    let owners = std::collections::HashMap::from([(scope.id.clone(), history.id.clone())]);
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        &[],
        &owners,
    )
    .unwrap();
    assert!(!feature
        .source_properties
        .contains_key(&cadmpeg_core::nonblank_literal!("edge_midpoints")));
    assert!(losses.iter().any(
        |loss| loss.code == crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind()
    ));
}

#[test]
fn edge_midpoints_keep_features_with_equal_state_numbers_separate() {
    let first_scope = scope_between(3, 0);
    let mut second_scope = first_scope.clone();
    second_scope.id = "f3d:Other/BulkStream.dat:design-parameter-scope#3".into();
    let first = midpoint_fillet(&first_scope);
    let second = midpoint_fillet(&second_scope);
    let mut features = [first, second];
    let mut histories = [
        midpoint_history("history:first", 1.0),
        midpoint_history("history:second", 2.0),
    ];
    let owners = std::collections::HashMap::from([
        (first_scope.id.clone(), histories[0].id.clone()),
        (second_scope.id.clone(), histories[1].id.clone()),
    ]);
    let losses = bind_feature_outputs(
        &mut features,
        &[first_scope, second_scope],
        &mut histories,
        &[],
        &owners,
    )
    .unwrap();
    for (feature, expected) in features.iter().zip(["1,0,0", "2,0,0"]) {
        assert_eq!(
            feature
                .source_properties
                .get(&cadmpeg_core::nonblank_literal!("edge_midpoints"))
                .map(String::as_str),
            Some(expected)
        );
    }
    assert!(!losses.iter().any(
        |loss| loss.code == crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind()
    ));
}

#[test]
fn edge_midpoints_report_a_missing_input_carrier() {
    let scope = scope_between(3, 0);
    let mut history = midpoint_history("history:first", 1.0);
    crate::test_support::history::edit(&mut history, 2, |topology, _| {
        topology.edge_samples.clear();
    });
    let mut feature = midpoint_fillet(&scope);
    let owners = std::collections::HashMap::from([(scope.id.clone(), history.id.clone())]);
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        &[],
        &owners,
    )
    .unwrap();
    assert!(!feature
        .source_properties
        .contains_key(&cadmpeg_core::nonblank_literal!("edge_midpoints")));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code
                == crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind())
            .count(),
        1
    );
}
