// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::native_scope_definition;
use crate::design::test_support::parameter_record;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

#[test]
fn native_scope_kind_keeps_source_name() {
    let kind = "source雪%";
    let scope = DesignParameterScope::empty(
        "f3d:test:design-parameter-scope#10",
        DesignFeatureKind::try_from(kind.to_owned()).unwrap(),
        10,
    );
    let definition = native_scope_definition(&scope, &[]).unwrap();
    assert!(
        matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref name), .. }) if name == kind)
    );
}

#[test]
fn native_parameter_restated_key_keeps_refusal() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine,
        10,
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        None,
        "2 mm",
        "User Parameter",
        Some("mm"),
        "Length",
        2.0,
    ))
    .unwrap();
    let error = native_scope_definition(&scope, &[(0, &parameter), (1, &parameter)]).unwrap_err();
    assert!(error
        .to_string()
        .contains("states the property Length a second time"));
}
