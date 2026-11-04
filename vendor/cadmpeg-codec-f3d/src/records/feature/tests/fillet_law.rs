use crate::records::feature::fixed_parameters::{
    DesignFixedFilletGroup, DesignFixedFilletIntermediate, DesignFixedFilletLaw,
    DesignFixedFilletScalar,
};

fn scalar(value: f64) -> DesignFixedFilletScalar {
    DesignFixedFilletScalar {
        value: crate::test_support::real(value),
        record_index: 1,
        value_offset: 40,
    }
}

fn variable(start: f64, end: f64, rows: &[(f64, f64)]) -> DesignFixedFilletLaw {
    DesignFixedFilletLaw::Variable {
        start: scalar(start),
        end: scalar(end),
        intermediate: rows
            .iter()
            .map(|&(radius, parameter)| DesignFixedFilletIntermediate {
                radius: scalar(radius),
                parameter: scalar(parameter),
            })
            .collect(),
    }
}

#[test]
fn fillet_law_admission_rejects_invalid_numeric_values() {
    let value = -1.0;
    assert!(
        DesignFixedFilletGroup::try_new(None, DesignFixedFilletLaw::Constant(scalar(value)))
            .unwrap_err()
            .contains("radii")
    );
    assert!(
        DesignFixedFilletGroup::try_new(None, variable(1.0, value, &[]))
            .unwrap_err()
            .contains("radii")
    );
    assert!(
        DesignFixedFilletGroup::try_new(None, variable(1.0, 2.0, &[(value, 0.5)]))
            .unwrap_err()
            .contains("radii")
    );
    for value in [-1.0, 0.0] {
        assert!(DesignFixedFilletGroup::try_new(
            Some(scalar(value)),
            DesignFixedFilletLaw::Constant(scalar(1.0))
        )
        .unwrap_err()
        .contains("tangency_weight"));
    }
    assert!(
        DesignFixedFilletGroup::try_new(None, DesignFixedFilletLaw::Constant(scalar(0.0))).is_err()
    );
    assert!(DesignFixedFilletGroup::try_new(None, variable(0.0, 0.0, &[(0.0, 0.5)])).is_err());
    for parameter in [-1.0, 1.0] {
        assert!(
            DesignFixedFilletGroup::try_new(None, variable(1.0, 2.0, &[(1.0, parameter)]))
                .unwrap_err()
                .contains("intermediate_parameters")
        );
    }
    for parameters in [[0.5, 0.5], [0.75, 0.25]] {
        assert!(DesignFixedFilletGroup::try_new(
            None,
            variable(1.0, 2.0, &[(1.0, parameters[0]), (1.0, parameters[1])])
        )
        .unwrap_err()
        .contains("intermediate_parameters"));
    }
}

#[test]
fn fillet_law_serde_rejects_negative_zero_only_and_unordered_laws() {
    let input = serde_json::json!({
        "radii": [0.0, 0.0, 1.0, 0.0],
        "radius_record_indexes": [1, 2, 3, 4], "radius_offsets": [40, 48, 56, 64],
        "intermediate_parameters": [0.0, 0.5],
        "intermediate_parameter_record_indexes": [5, 6], "intermediate_parameter_offsets": [72, 80],
    });
    let group: DesignFixedFilletGroup = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(group).unwrap(), input);
    for (field, value) in [
        ("radii", serde_json::json!([-1.0, 0.0, 1.0, 0.0])),
        ("radii", serde_json::json!([0.0, 0.0, 0.0, 0.0])),
        ("intermediate_parameters", serde_json::json!([0.5, 0.5])),
        ("intermediate_parameters", serde_json::json!([0.75, 0.25])),
        ("intermediate_parameters", serde_json::json!([0.0, 1.0])),
        (
            "tangency_weight",
            serde_json::json!({"value": 0.0, "record_index": 7, "value_offset": 88}),
        ),
    ] {
        let mut invalid = input.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<DesignFixedFilletGroup>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}
