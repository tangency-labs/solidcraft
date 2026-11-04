// SPDX-License-Identifier: Apache-2.0

use super::{AdmittedTables, ConfiguredValue};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, object_target, property_target, string_cell, table_document,
};
use serde_json::{json, Value};

const DEFINITION: &str = "/tables/0/asset/components/0/properties";
const COLUMNS: &str = "/tables/0/asset/components/0/properties/columnData/value";
const ROW_ORDER: &str = "/tables/0/tableView/asset/components/0/properties/rowOrder/value";

fn row_data(row: usize) -> String {
    format!("/tables/0/rows/{row}/asset/components/0/properties")
}

fn cell(row: usize, column: &str) -> String {
    format!("{}/rowCells/value/{column}/value", row_data(row))
}

/// Rows `Tall`, `Configuration 1`, `NoHole` in array order; the authored order
/// is `c1`, `tall`, `nohole`.
fn document() -> Value {
    let cells = |height: &str, computed: bool| {
        vec![
            ("P", string_cell("")),
            ("H", expression_cell(height, 1.0)),
            ("D", expression_cell("10.00 mm", 1.0)),
            ("F", bool_cell(computed)),
        ]
    };
    table_document(
        "1",
        &[
            ("P", property_target("Part Number")),
            ("H", object_target("202", "UserParameterValue")),
            ("D", object_target("330", "ParameterDoubleValue")),
            ("F", object_target("314", "DcFeature")),
        ],
        &[
            ("tall", "Tall", cells("35 mm", false)),
            ("c1", "Configuration 1", cells("25 mm", true)),
            ("nohole", "NoHole", cells("10 mm", true)),
        ],
        &["c1", "tall", "nohole"],
    )
}

fn admit(document: &Value) -> AdmittedTables {
    AdmittedTables::admit(document.as_object().unwrap())
}

fn set(document: &mut Value, pointer: &str, value: Value) {
    *document
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("{pointer}")) = value;
}

#[test]
fn table_rows_follow_row_order_and_type_each_cell() {
    let admitted = admit(&document());
    assert_eq!(admitted.unassigned, 0);
    let [table] = admitted.tables.as_slice() else {
        panic!("one table");
    };
    assert_eq!(table.id, "1");
    let rows = table
        .rows
        .iter()
        .map(|row| (row.id.as_str(), row.name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("c1", "Configuration 1"),
            ("tall", "Tall"),
            ("nohole", "NoHole")
        ]
    );
    let tall = table.rows[1]
        .cells
        .iter()
        .map(|cell| (cell.column_id.as_str(), cell.value.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        tall,
        [
            (
                "D",
                ConfiguredValue::OwnedParameter {
                    owner_record_index: 330,
                    expression: "10.00 mm".into()
                }
            ),
            (
                "F",
                ConfiguredValue::Feature {
                    record_index: 314,
                    computed: false
                }
            ),
            (
                "H",
                ConfiguredValue::Parameter {
                    record_index: 202,
                    expression: "35 mm".into()
                }
            ),
            (
                "P",
                ConfiguredValue::Property {
                    key: "Part Number".into(),
                    text: String::new()
                }
            ),
        ]
    );
}

#[test]
fn table_document_outside_the_versioned_envelope_assigns_no_table() {
    for (pointer, value) in [
        (
            "/asset/components/0/properties/tableVersion/value",
            json!(2),
        ),
        (
            "/asset/components/0/properties/tableVersion/type",
            json!("Int64"),
        ),
        (
            "/asset/type",
            json!("autodesk.configuration:assets.configuration-2.0.0"),
        ),
        ("/asset/components/0/name", json!("version")),
    ] {
        let mut document = document();
        set(&mut document, pointer, value);
        let admitted = admit(&document);
        assert!(admitted.tables.is_empty(), "{pointer}");
        assert_eq!(admitted.unassigned, 2, "{pointer}");
    }
    let admitted = admit(&json!({}));
    assert!(admitted.tables.is_empty());
    assert_eq!(admitted.unassigned, 0);
}

#[test]
fn table_identity_conflicts_leave_the_table_unassigned() {
    let column = |member: &str| format!("{COLUMNS}/H/value/{member}");
    let cases: Vec<(String, Value)> = vec![
        (format!("{DEFINITION}/id/value"), json!("2")),
        (column("id/value"), json!("H2")),
        (
            "/tables/0/rows/0/asset/components/1/properties/id/value".into(),
            json!("other"),
        ),
        (format!("{}/columnId/value", cell(0, "H")), json!("D")),
        (ROW_ORDER.into(), json!(["c1", "tall"])),
        (ROW_ORDER.into(), json!(["c1", "tall", "tall"])),
        (ROW_ORDER.into(), json!(["c1", "tall", "nohole", "extra"])),
        (ROW_ORDER.into(), json!(["c1", "tall", 3])),
        (
            "/tables/0/rows/2/asset/attributes/pim:configuration:rowId".into(),
            json!("c1"),
        ),
        (
            "/tables/0/asset/type".into(),
            json!("autodesk.configuration:assets.configurationTable-1.2.0"),
        ),
        ("/tables/0/rows".into(), json!({})),
        (format!("{}/configRevisionId/value", row_data(0)), json!(7)),
        (format!("{}/default/type", row_data(0)), json!("String")),
        (
            "/tables/0/asset/components/0/type".into(),
            json!("autodesk.configuration:components.tableDefinition-1.2.0"),
        ),
    ];
    for (pointer, value) in cases {
        let mut document = document();
        set(&mut document, &pointer, value.clone());
        if pointer.ends_with("/rowId") {
            set(
                &mut document,
                "/tables/0/rows/2/asset/components/1/properties/id/value",
                json!("c1"),
            );
        }
        let admitted = admit(&document);
        assert!(admitted.tables.is_empty(), "{pointer} = {value}");
        assert_eq!(admitted.unassigned, 1, "{pointer} = {value}");
    }
}

#[test]
fn table_rows_with_one_id_leave_the_table_unassigned() {
    let mut document = document();
    for pointer in [
        "/tables/0/rows/2/asset/attributes/pim:configuration:rowId",
        "/tables/0/rows/2/asset/components/1/properties/id/value",
    ] {
        set(&mut document, pointer, json!("c1"));
    }
    set(&mut document, ROW_ORDER, json!(["c1", "tall"]));
    let admitted = admit(&document);
    assert!(admitted.tables.is_empty());
    assert_eq!(admitted.unassigned, 1);
}

#[test]
fn table_cell_of_an_absent_column_leaves_the_table_unassigned() {
    let mut document = document();
    let cells = document
        .pointer_mut(&format!("{}/rowCells/value", row_data(0)))
        .unwrap()
        .as_object_mut()
        .unwrap();
    let mut moved = cells.remove("H").unwrap();
    moved["value"]["columnId"]["value"] = json!("X");
    cells.insert("X".into(), moved);
    let admitted = admit(&document);
    assert!(admitted.tables.is_empty());
    assert_eq!(admitted.unassigned, 1);
}

#[test]
fn table_counts_members_without_a_typed_target() {
    let column = |name: &str, member: &str| format!("{COLUMNS}/{name}/value/{member}");
    let cases: Vec<(String, Value, usize)> = vec![
        ("/unknown".into(), json!(0), 1),
        ("/cachedRevIds".into(), json!({"a": "1", "b": "2"}), 2),
        ("/cachedRevIds".into(), json!([]), 1),
        ("/testRowData".into(), json!(["{}", 1]), 1),
        ("/tables/0/unknown".into(), json!(0), 1),
        (format!("{}/default/value", row_data(1)), json!(true), 1),
        (column("H", "isCustom/value"), json!(true), 4),
        (
            column("P", "object/value/propertySet/value"),
            json!("set"),
            4,
        ),
        (column("P", "object/value/type/value"), json!("Other"), 4),
        (
            column("H", "object/value/type/value"),
            json!("ModelParameterValue"),
            4,
        ),
        (column("H", "object/value/id/value"), json!("+202"), 4),
        (column("H", "object/value/id/value"), json!(""), 4),
        (column("H", "object/value/id/value"), json!("20a"), 4),
        (column("H", "object/value/id/value"), json!("4294967296"), 4),
        (format!("{}/data", cell(0, "H")), bool_cell(true), 1),
        (format!("{}/data", cell(0, "F")), string_cell("true"), 1),
        (format!("{}/data", cell(0, "P")), bool_cell(true), 1),
        (
            format!("{}/data/value/value/value/value/value", cell(0, "H")),
            json!("1"),
            1,
        ),
    ];
    for (pointer, value, unassigned) in cases {
        let mut document = document();
        if pointer.matches('/').count() == 1 {
            document
                .as_object_mut()
                .unwrap()
                .insert(pointer[1..].into(), value.clone());
        } else if pointer == "/tables/0/unknown" {
            document["tables"][0]
                .as_object_mut()
                .unwrap()
                .insert("unknown".into(), value.clone());
        } else {
            set(&mut document, &pointer, value.clone());
        }
        let admitted = admit(&document);
        assert_eq!(admitted.tables.len(), 1, "{pointer} = {value}");
        assert_eq!(admitted.unassigned, unassigned, "{pointer} = {value}");
        assert_eq!(admitted.tables[0].rows.len(), 3, "{pointer} = {value}");
    }
}

#[test]
fn table_components_are_selected_by_one_name() {
    let mut duplicated = document();
    let definition = duplicated["tables"][0]["asset"]["components"][0].clone();
    duplicated["tables"][0]["asset"]["components"]
        .as_array_mut()
        .unwrap()
        .push(definition);
    let admitted = admit(&duplicated);
    assert!(admitted.tables.is_empty());
    assert_eq!(admitted.unassigned, 1);

    let mut versioned = document();
    set(
        &mut versioned,
        "/asset/components/0/type",
        json!("autodesk.configuration:components.tableData-2.0.0"),
    );
    let admitted = admit(&versioned);
    assert!(admitted.tables.is_empty());
    assert_eq!(admitted.unassigned, 2);
}
