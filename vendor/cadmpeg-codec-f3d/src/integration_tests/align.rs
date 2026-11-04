// SPDX-License-Identifier: Apache-2.0

#[test]
fn align_scopes_retain_typed_prefix_or_emit_exact_refusal_loss() {
    use crate::design::decode::scopes::parameter_scope::{
        decode_parameter_scopes, ParameterScopeSources,
    };
    use crate::records::feature::align::DesignAlignForm;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        for refusal in 0..3 {
            let refused = refusal == 1;
            let constructor_refused = refusal == 2;
            let archive =
                crate::test_support::align::align_archive(form, refused, constructor_refused);
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let result = decode_parameter_scopes(
                    &cadmpeg_test_support::service_decode_context(),
                    scan,
                    &ParameterScopeSources::default(),
                )
                .unwrap();
                assert_eq!(result.scopes.len(), 1);
                assert_eq!(result.scopes[0].kind(), DesignFeatureKind::Align);
                assert_eq!(result.scopes[0].align_envelope().is_none(), refused);
                if let Some(envelope) = result.scopes[0].align_envelope() {
                    assert_eq!(envelope.construction.is_none(), constructor_refused);
                }
                let wire = serde_json::to_value(&result.scopes[0]).unwrap();
                let back: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
                assert_eq!(back, result.scopes[0]);
                if !refused {
                    let mut wrong_kind = wire.clone();
                    wrong_kind["kind"] = "Move".into();
                    assert!(serde_json::from_value::<DesignParameterScope>(wrong_kind).is_err());
                    let mut null_construction = wire;
                    null_construction["align_envelope"]["construction"] = serde_json::Value::Null;
                    assert!(
                        serde_json::from_value::<DesignParameterScope>(null_construction).is_err()
                    );
                }
                assert_eq!(result.losses.len(), 1);
                assert_eq!(
                    result.losses[0].code.to_string(),
                    if refused {
                        "f3d/feature.align-envelope-unparsed"
                    } else if constructor_refused {
                        "f3d/feature.align-construction-unparsed"
                    } else {
                        "f3d/feature.align-operands-unresolved"
                    }
                );
            });
        }
    }
}
