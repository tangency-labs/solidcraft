// SPDX-License-Identifier: Apache-2.0
//! Synthetic configuration-table documents and Design configuration root records.
#![allow(clippy::unwrap_used)]

use serde_json::{json, Map, Value};

use crate::test_support::{indexed_header, lp_ascii, lp_utf16};

fn typed(kind: &str, value: Value) -> Value {
    json!({"type": kind, "value": value})
}

fn component_value(component: &str, members: Value) -> Value {
    typed(
        &format!("autodesk.configuration:components.{component}"),
        members,
    )
}

fn asset(kind: &str, attributes: Value, components: Value) -> Value {
    json!({
        "id": "", "revisionId": "",
        "type": format!("autodesk.configuration:assets.{kind}"),
        "attributes": attributes, "components": components
    })
}

fn component(name: &str, kind: &str, properties: Value) -> Value {
    json!({
        "name": name,
        "type": format!("autodesk.configuration:components.{kind}"),
        "properties": properties
    })
}

/// The `object` value of a document-property column.
pub(crate) fn property_target(key: &str) -> Value {
    component_value(
        "configuredProperty-1.0.0",
        json!({
            "id": typed("String", json!("00000000-0000-0000-0000-000000000000")),
            "propertyKey": typed("String", json!(key)),
            "propertySet": typed("String", json!("")),
            "type": typed("String", json!("Property"))
        }),
    )
}

/// The `object` value of a column that names Design record `record` of `kind`.
pub(crate) fn object_target(record: &str, kind: &str) -> Value {
    component_value(
        "configuredObject-1.0.0",
        json!({"id": typed("String", json!(record)), "type": typed("String", json!(kind))}),
    )
}

/// The `data` value of a parameter cell.
pub(crate) fn expression_cell(expression: &str, value: f64) -> Value {
    component_value(
        "variant.expression-1.0.0",
        json!({
            "expression": typed("String", json!(expression)),
            "value": component_value("variant.float64-1.0.0", json!({"value": typed("Float64", json!(value))}))
        }),
    )
}

/// The `data` value of a feature cell.
pub(crate) fn bool_cell(value: bool) -> Value {
    component_value(
        "variant.bool-1.0.0",
        json!({"value": typed("Bool", json!(value))}),
    )
}

/// The `data` value of a property cell.
pub(crate) fn string_cell(text: &str) -> Value {
    component_value(
        "variant.string-1.0.0",
        json!({"value": typed("String", json!(text))}),
    )
}

/// One table row: id, name, and `(column id, data)` cells.
pub(crate) type Row<'a> = (&'a str, &'a str, Vec<(&'a str, Value)>);

/// A table document with one table `table_id`, the `(column id, object)`
/// columns, `rows` in array order, and the authored `row_order`.
pub(crate) fn table_document(
    table_id: &str,
    columns: &[(&str, Value)],
    rows: &[Row<'_>],
    row_order: &[&str],
) -> Value {
    let column_data = columns
        .iter()
        .map(|(id, object)| {
            (
                (*id).to_owned(),
                component_value(
                    "columnData-1.1.0",
                    json!({
                        "id": typed("String", json!(id)),
                        "isCustom": typed("Bool", json!(false)),
                        "object": object,
                        "title": typed("String", json!(id))
                    }),
                ),
            )
        })
        .collect::<Map<_, _>>();
    let rows = rows
        .iter()
        .map(|(id, name, cells)| {
            let cells = cells
                .iter()
                .map(|(column, data)| {
                    (
                        (*column).to_owned(),
                        component_value(
                            "rowCell-1.0.0",
                            json!({"columnId": typed("String", json!(column)), "data": data}),
                        ),
                    )
                })
                .collect::<Map<_, _>>();
            json!({"asset": asset(
                "row-1.0.0",
                json!({"pim:configuration:rowId": id, "pim:configuration:rowName": name}),
                json!([
                    component("rowData", "rowData-1.0.0", json!({
                        "configRevisionId": typed("String", json!(format!("revision-{id}"))),
                        "default": typed("Bool", json!(false)),
                        "rowCells": typed("map<autodesk.configuration:components.rowCell-1.0.0>", Value::Object(cells))
                    })),
                    component("rowDefinition", "rowDefinition-1.0.0", json!({"id": typed("String", json!(id))}))
                ]),
            )})
        })
        .collect::<Vec<_>>();
    let column_order = columns.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    json!({
        "asset": asset("configuration-1.0.0", json!({}), json!([
            component("tableVersion", "tableData-1.0.0", json!({"tableVersion": typed("Int32", json!(1))}))
        ])),
        "tables": [{
            "asset": asset(
                "configurationTable-1.1.0",
                json!({"pim:configuration:tableId": table_id}),
                json!([component("tableDefinition", "tableDefinition-1.1.0", json!({
                    "columnData": typed("map<autodesk.configuration:components.columnData-1.1.0>", Value::Object(column_data)),
                    "id": typed("String", json!(table_id)),
                    "tableName": typed("String", json!("Configurations")),
                    "tableType": typed("String", json!("Configurations"))
                }))]),
            ),
            "rows": rows,
            "tableView": {"asset": asset("tableView-1.0.0", json!({}), json!([
                component("viewData", "tableViewData-1.0.0", json!({
                    "columnOrder": typed("String", json!(column_order)),
                    "rowOrder": typed("String", json!(row_order))
                }))
            ]))}
        }],
        "cachedRevIds": {},
        "testRowData": []
    })
}

/// A Design `DesignConfigurationRoot` record that names `row_id` of `table_id`.
pub(crate) fn root_record(
    class_tag: [u8; 3],
    record_index: u32,
    table_id: &str,
    row_id: &str,
) -> Vec<u8> {
    let mut record = Vec::new();
    indexed_header(&mut record, class_tag, record_index);
    record.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut record, "DesignConfigurationRoot");
    record.extend_from_slice(&[0; 11]);
    record.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut record, table_id);
    lp_utf16(&mut record, row_id);
    record.extend_from_slice(&0u32.to_le_bytes());
    record
}

/// Indexed-record index of the root record that [`with_root_segment`] stores.
pub(crate) const ROOT_RECORD: u32 = 349;

/// `archive` with a `FusionDesignSegmentType1` segment whose one primary record
/// is `root`, which must carry class tag `256` and index [`ROOT_RECORD`].
pub(crate) fn with_root_segment(archive: Vec<u8>, root: Vec<u8>) -> Vec<u8> {
    use std::io::{Read, Write};

    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "11111111-2222-3333-4444-555555555555",
            "",
            1,
            "Design",
            &[ROOT_RECORD.into()],
        )],
        &[(ROOT_RECORD.into(), 0)],
    );
    let mut source = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut payload = Vec::new();
        entry.read_to_end(&mut payload).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&payload).unwrap();
    }
    for (name, payload) in [("BulkStream.dat", root), ("MetaStream.dat", metadata)] {
        zip.start_file(
            format!("FusionAssetName[Active]/FusionDesignSegmentType1/{name}"),
            stored,
        )
        .unwrap();
        zip.write_all(&payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The neutral configurations that the native configuration documents of `ir`
/// project against its native and neutral records.
pub(crate) fn project(ir: &cadmpeg_ir::CadIr) -> Vec<cadmpeg_ir::features::DesignConfiguration> {
    let native = crate::test_support::native_test::f3d_native(ir);
    crate::design::configurations::project_configurations(
        &native.design_configurations,
        &crate::design::configurations::ConfigurationTargets {
            parameters: &native.design_parameters,
            owners: &native.design_parameter_owners,
            scopes: &native.design_parameter_scopes,
            neutral_parameters: &ir.model.parameters,
            neutral_features: &ir.model.features,
        },
    )
    .expect("configuration projection")
}

/// Attach the active row `row_id` of table `1` to the first native
/// configuration document and project again.
pub(crate) fn set_active_row(ir: &mut cadmpeg_ir::CadIr, row_id: &str) {
    crate::test_support::native_test::update_f3d_native(ir, |native| {
        native.design_configurations[0]
            .set_active_row(
                crate::records::configuration::DesignConfigurationActiveRow {
                    record_index: ROOT_RECORD,
                    table_id: "1".into(),
                    row_id: row_id.into(),
                },
            )
            .unwrap();
    });
    ir.model.configurations = project(ir);
}
