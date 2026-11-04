// SPDX-License-Identifier: Apache-2.0

use super::{
    attach_roots, project_configurations, root_document, unresolved_configuration_member_count,
    unresolved_configuration_parameter_override_count, unresolved_configuration_rule_count,
    unresolved_configuration_suppressed_feature_count, ConfigurationTargets,
};
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, object_target, property_target, string_cell, table_document, Row,
};
use cadmpeg_ir::features::{
    ConfigurationEvaluation, DesignParameter as NeutralParameter, Feature, FeatureDefinition,
    FeatureId, FeatureOperation,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const ENTRY: &str =
    "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.t.dsgcfg";
const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn table(document: &Value) -> DesignConfiguration {
    DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        document.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn rule(name: &str, payload: &Value) -> DesignConfiguration {
    DesignConfiguration::try_new(
        name.into(),
        DesignConfigurationKind::Rule,
        payload.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn active(table_id: &str, row_id: &str) -> DesignConfigurationActiveRow {
    DesignConfigurationActiveRow {
        record_index: 349,
        table_id: table_id.into(),
        row_id: row_id.into(),
    }
}

/// Document parameter record `index` in the test parameter stream.
fn parameter(index: u32, name: &str) -> DesignParameter {
    let mut frame = crate::design::test_support::parameter_record(
        None,
        "10 mm",
        "User Parameter",
        Some("mm"),
        name,
        1.0,
    );
    frame[7..11].copy_from_slice(&index.to_le_bytes());
    crate::design::decode::parameters::parse_design_parameter_record(&frame).unwrap()
}

fn owner(stream: &str, index: u32, parameter: u32) -> DesignParameterOwner {
    serde_json::from_value(json!({
        "id": format!("{stream}:design-parameter-owner#{index}"),
        "byte_offset": 2000,
        "frame_length": 104,
        "class_tag": "351",
        "record_index": index,
        "scope_record_index": 314,
        "local_ordinal": 0,
        "evaluated_value": 1.0,
        "evaluated_value_offset": 2040,
        "parameter_record_index": parameter,
        "owned_ordinal": 1,
        "variant": 0,
        "companion_record_index": index + 1,
    }))
    .unwrap()
}

fn scope(index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{index}"),
        DesignFeatureKind::Extrude,
        index,
    )
}

fn neutral_parameter(native: &DesignParameter) -> NeutralParameter {
    NeutralParameter {
        product_definition: None,
        id: crate::ids::neutral_parameter_id(native),
        owner: None,
        ordinal: 0,
        name: native.name().to_owned(),
        expression: "10 mm".into(),
        display: None,
        value: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn neutral_feature(scope: &DesignParameterScope, dependency: &str) -> Feature {
    Feature {
        product_definition: None,
        id: crate::ids::neutral_feature_id(scope),
        ordinal: 1,
        name: Some("Extrusion 2".into()),
        suppressed: Some(false),
        dependencies: std::iter::once(FeatureId::mint(dependency).unwrap()).collect(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Extrude".into(),
                parameters: BTreeMap::new(),
            }),
        ),
        native_ref: None,
    }
}

fn columns() -> Vec<(&'static str, Value)> {
    vec![
        ("P", property_target("Part Number")),
        ("H", object_target("202", "UserParameterValue")),
        ("D", object_target("330", "ParameterDoubleValue")),
        ("F", object_target("314", "DcFeature")),
    ]
}

fn row<'a>(id: &'a str, name: &'a str, height: &'a str, computed: bool) -> Row<'a> {
    (
        id,
        name,
        vec![
            ("P", string_cell(name)),
            ("H", expression_cell(height, 1.0)),
            ("D", expression_cell("10.00 mm", 1.0)),
            ("F", bool_cell(computed)),
        ],
    )
}

fn document() -> Value {
    table_document(
        "1",
        &columns(),
        &[
            row("tall", "Tall", "35 mm", false),
            row("c1", "Configuration 1", "25 mm", true),
        ],
        &["c1", "tall"],
    )
}

struct Natives {
    parameters: Vec<DesignParameter>,
    owners: Vec<DesignParameterOwner>,
    scopes: Vec<DesignParameterScope>,
    neutral_parameters: Vec<NeutralParameter>,
    neutral_features: Vec<Feature>,
}

impl Natives {
    fn complete() -> Self {
        let parameters = vec![parameter(202, "hh"), parameter(329, "d4")];
        let scopes = vec![scope(314)];
        Self {
            neutral_parameters: parameters.iter().map(neutral_parameter).collect(),
            neutral_features: vec![neutral_feature(&scopes[0], "f3d:model:feature#sketch")],
            owners: vec![owner(STREAM, 330, 329)],
            parameters,
            scopes,
        }
    }

    fn targets(&self) -> ConfigurationTargets<'_> {
        ConfigurationTargets {
            parameters: &self.parameters,
            owners: &self.owners,
            scopes: &self.scopes,
            neutral_parameters: &self.neutral_parameters,
            neutral_features: &self.neutral_features,
        }
    }
}

#[test]
fn configuration_rows_project_in_row_order_with_the_active_row() {
    let mut native = table(&document());
    native.set_active_row(active("1", "tall")).unwrap();
    let natives = Natives::complete();
    let projected = project_configurations(&[native], &natives.targets()).unwrap();
    let mut rows = projected
        .iter()
        .map(|configuration| {
            (
                configuration.ordinal,
                configuration.name.as_deref().unwrap(),
                configuration.active,
                configuration.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(ordinal, ..)| *ordinal);
    assert_eq!(
        rows,
        [
            (
                0,
                "Configuration 1",
                false,
                crate::ids::neutral_configuration_id(ENTRY, "c1")
            ),
            (
                1,
                "Tall",
                true,
                crate::ids::neutral_configuration_id(ENTRY, "tall")
            ),
        ]
    );

    for table_id in ["2", ""] {
        let mut native = table(&document());
        native.set_active_row(active(table_id, "tall")).unwrap();
        let projected = project_configurations(&[native], &natives.targets()).unwrap();
        assert!(projected.iter().all(|configuration| !configuration.active));
    }
}

#[test]
fn configuration_cells_bind_targets_by_record_index() {
    let natives = Natives::complete();
    let projected = project_configurations(&[table(&document())], &natives.targets()).unwrap();
    let tall = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Tall"))
        .unwrap();
    let height = &natives.neutral_parameters[0].id;
    let depth = &natives.neutral_parameters[1].id;
    assert_eq!(tall.parameter_overrides[height], "35 mm");
    assert_eq!(tall.parameter_overrides[depth], "10.00 mm");
    assert_eq!(tall.parameter_overrides.len(), 2);
    let feature = &natives.neutral_features[0];
    let state = &tall.feature_states[&feature.id];
    assert!(state.evaluation.is_suppressed());
    assert_eq!(state.dependencies, feature.dependencies);
    assert_eq!(&state.definition, feature.evaluation.definition());
    assert_eq!(
        tall.properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [("property:Part Number", "Tall")]
    );
    let first = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
        .unwrap();
    assert!(matches!(
        first.feature_states[&feature.id].evaluation,
        ConfigurationEvaluation::Active { ref outputs } if outputs.is_empty()
    ));
    assert_eq!(
        unresolved_configuration_parameter_override_count(&projected),
        0
    );
    assert_eq!(
        unresolved_configuration_suppressed_feature_count(&projected),
        0
    );
}

#[test]
fn configuration_cells_without_a_neutral_target_stay_properties() {
    let mut cases: Vec<(&str, Natives)> = Vec::new();
    let mut natives = Natives::complete();
    natives.neutral_parameters = vec![neutral_parameter(&parameter(500, "other"))];
    natives.neutral_features = vec![neutral_feature(&scope(600), "f3d:model:feature#sketch")];
    cases.push(("other neutral entities", natives));
    let mut natives = Natives::complete();
    natives.parameters.clear();
    natives.scopes.clear();
    natives.owners.clear();
    cases.push(("no native records", natives));
    let mut natives = Natives::complete();
    natives.owners = vec![owner("f3d:Other/BulkStream.dat", 330, 329)];
    natives.scopes.push(DesignParameterScope::empty(
        "f3d:Other/BulkStream.dat:design-parameter-scope#314",
        DesignFeatureKind::Extrude,
        314,
    ));
    cases.push(("other streams", natives));
    for (name, natives) in cases {
        let projected = project_configurations(&[table(&document())], &natives.targets()).unwrap();
        let tall = projected
            .iter()
            .find(|configuration| configuration.name.as_deref() == Some("Tall"))
            .unwrap();
        assert!(tall.feature_states.is_empty(), "{name}");
        assert_eq!(tall.properties["suppressed:F"], "true", "{name}");
        assert_eq!(tall.properties["parameter:D"], "10.00 mm", "{name}");
        let unresolved_height = name != "other streams";
        assert_eq!(
            tall.properties.get("parameter:H").map(String::as_str),
            unresolved_height.then_some("35 mm"),
            "{name}"
        );
        let first = projected
            .iter()
            .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
            .unwrap();
        assert_eq!(first.properties["suppressed:F"], "false", "{name}");
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&projected),
            2,
            "{name}"
        );
        assert_eq!(
            unresolved_configuration_parameter_override_count(&projected),
            if unresolved_height { 4 } else { 2 },
            "{name}"
        );
    }
}

#[test]
fn configuration_owner_records_resolve_only_one_parameter() {
    let mut natives = Natives::complete();
    natives
        .owners
        .push(owner("f3d:Other/BulkStream.dat", 330, 329));
    let projected = project_configurations(&[table(&document())], &natives.targets()).unwrap();
    assert!(projected
        .iter()
        .all(|configuration| configuration.properties.contains_key("parameter:D")));

    let mut natives = Natives::complete();
    natives.owners.push(owner(STREAM, 330, 329));
    let projected = project_configurations(&[table(&document())], &natives.targets()).unwrap();
    assert!(projected
        .iter()
        .all(|configuration| !configuration.properties.contains_key("parameter:D")));
}

#[test]
fn configuration_tables_with_rows_supply_one_list() {
    let natives = Natives::complete();
    let empty = table_document("2", &columns(), &[], &[]);
    let mut second = table(&empty);
    second.set_active_row(active("2", "tall")).unwrap();
    let projected =
        project_configurations(&[table(&document()), second], &natives.targets()).unwrap();
    assert_eq!(projected.len(), 2);

    let other = DesignConfiguration::try_new(
        "Other.dsgcfg".into(),
        DesignConfigurationKind::Table,
        document().as_object().unwrap().clone(),
    )
    .unwrap();
    let error = project_configurations(&[table(&document()), other], &natives.targets())
        .expect_err("two populated tables");
    assert!(error
        .to_string()
        .contains("configuration tables have no shared authored order"));
}

#[test]
fn configuration_rules_bind_only_one_named_row() {
    let natives = Natives::complete();
    let bound = rule(
        "rule.dsgcfgrule",
        &json!({"when": "width > 20 mm", "activate": "Tall"}),
    );
    let native = [table(&document()), bound];
    let projected = project_configurations(&native, &natives.targets()).unwrap();
    let tall = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Tall"))
        .unwrap();
    assert_eq!(
        tall.properties["activation_rule:rule.dsgcfgrule"],
        "width > 20 mm"
    );
    assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

    let duplicate = table_document(
        "1",
        &columns(),
        &[
            row("a", "Tall", "35 mm", false),
            row("b", "Tall", "25 mm", true),
        ],
        &["a", "b"],
    );
    let native = [
        table(&duplicate),
        rule(
            "rule.dsgcfgrule",
            &json!({"when": "width > 20 mm", "activate": "Tall"}),
        ),
        rule("partial.dsgcfgrule", &json!({"when": "w > 1", "x": 7})),
    ];
    let projected = project_configurations(&native, &natives.targets()).unwrap();
    assert_eq!(unresolved_configuration_rule_count(&native, &projected), 2);
    assert_eq!(unresolved_configuration_member_count(&native), 1);
}

#[test]
fn configuration_root_attaches_to_the_table_document_that_holds_its_table() {
    let empty = |name: &str| {
        DesignConfiguration::try_new(
            name.into(),
            DesignConfigurationKind::Table,
            serde_json::Map::new(),
        )
        .unwrap()
    };
    let holding = table(&document());
    let empty_rule = rule("rule.dsgcfgrule", &json!({}));
    assert_eq!(
        root_document(&[empty_rule.clone(), empty("a.dsgcfg")], "1"),
        Some(1)
    );
    assert_eq!(
        root_document(
            &[empty("a.dsgcfg"), holding.clone(), empty_rule.clone()],
            "1"
        ),
        Some(1)
    );
    assert_eq!(
        root_document(&[empty("a.dsgcfg"), empty("b.dsgcfg")], "1"),
        None
    );
    assert_eq!(root_document(&[holding.clone(), holding], "1"), None);
    assert_eq!(root_document(&[empty_rule], "1"), None);
}

#[test]
fn configuration_roots_attach_only_when_one_root_names_a_table_document() {
    let mut documents = [table(&document())];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall")]).unwrap(),
        0
    );
    assert_eq!(documents[0].active_row(), Some(&active("1", "tall")));

    let mut documents = [table(&document())];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall"), active("1", "c1")]).unwrap(),
        2
    );
    assert_eq!(documents[0].active_row(), None);

    let mut documents = [rule("rule.dsgcfgrule", &json!({}))];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall")]).unwrap(),
        1
    );
    assert_eq!(attach_roots(&mut documents, Vec::new()).unwrap(), 0);
}
