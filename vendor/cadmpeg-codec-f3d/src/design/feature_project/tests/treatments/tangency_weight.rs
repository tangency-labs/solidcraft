// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::{project_fillet_arm, ProjectInputs};
use crate::design::test_support::parameter_record;
use crate::records::topology::fillet::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};

#[test]
fn indexed_tangency_weight_with_a_unit_retains_the_native_feature(
) -> Result<(), Box<dyn std::error::Error>> {
    let scope = super::localized_fillet_scope();
    let operand_groups = super::localized_fillet_operand_groups();
    let assignment = DesignFilletRadiusGroup {
        id: "f3d:native/BulkStream.dat:design-fillet-radius-group#100".into(),
        scope_record_index: 12,
        group_ordinal: 0,
        group_record_index: 100,
        edge_operand_record_indices: vec![200],
        law: DesignFilletRadiusLaw::Constant {
            radius_parameter_record_index: 11,
        },
        tangency_weight_parameter_record_index: Some(21),
    };
    for unit in ["mm", "rad"] {
        let mut weight = parse_design_parameter_record(&parameter_record(
            Some(20),
            "value",
            "TangencyWeight",
            Some(unit),
            "w1",
            1.0,
        ))
        .ok_or("expected a parsed tangency weight")?;
        weight.id = "f3d:native/BulkStream.dat:parameter#21".into();
        weight.record_index = 21;
        let parameters = [
            super::localized_fillet_parameter(10, 11, "Radius", Some("mm"), 0.2),
            weight,
        ];
        let inputs = ProjectInputs {
            construction_groups: &operand_groups[..1],
            fillet_radius_groups: std::slice::from_ref(&assignment),
            ..Default::default()
        };
        let definition = crate::test_support::with_decode_context(|ctx| {
            project_fillet_arm(
                ctx,
                &inputs,
                &scope,
                &[(0, &parameters[0]), (1, &parameters[1])],
                "f3d:native/BulkStream.dat",
            )
        })?;
        let FeatureDefinition::Operation(FeatureOperation::Native {
            parameters: expressions,
            ..
        }) = definition
        else {
            return Err("expected a native Fillet for a unit-bearing tangency weight".into());
        };
        assert_eq!(
            expressions
                .iter()
                .map(|(name, expression)| (name.as_str(), expression.as_str()))
                .collect::<Vec<_>>(),
            [("d1", "value"), ("w1", "value")]
        );
        assert_eq!(parameters[1].evaluated_value().get(), 1.0);
        assert_eq!(
            parameters[1].unit().map(|field| field.value.as_str()),
            Some(unit)
        );
    }
    Ok(())
}
