// SPDX-License-Identifier: Apache-2.0
//! Typed rows and cells of a configuration-table document, in authored row order.

use serde_json::{Map, Value};
use std::collections::HashMap;

const ASSET_PREFIX: &str = "autodesk.configuration:assets.";
const COMPONENT_PREFIX: &str = "autodesk.configuration:components.";

/// Tables of one document and the count of members that no table typing assigns.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AdmittedTables {
    pub(crate) tables: Vec<ConfigurationTable>,
    pub(crate) unassigned: usize,
}

/// One table with its rows in `rowOrder` order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigurationTable {
    pub(crate) id: String,
    pub(crate) rows: Vec<ConfigurationRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigurationRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) cells: Vec<ConfigurationCell>,
}

/// A cell whose value matches the target of its column.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConfigurationCell {
    pub(crate) column_id: String,
    pub(crate) value: ConfiguredValue,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConfiguredValue {
    /// Value of a document property with an empty property set.
    Property { key: String, text: String },
    /// Source expression of a document parameter record.
    Parameter {
        record_index: u32,
        expression: String,
    },
    /// Source expression of the parameter of a parameter owner record.
    OwnedParameter {
        owner_record_index: u32,
        expression: String,
    },
    /// Whether the feature of a parameter scope record is computed.
    Feature { record_index: u32, computed: bool },
}

#[derive(Debug, Clone, PartialEq)]
enum ColumnTarget {
    Property(String),
    Parameter(u32),
    OwnedParameter(u32),
    Feature(u32),
}

impl AdmittedTables {
    /// Type every table of a table document. A member, table, column, row
    /// flag, or cell outside the typed form is counted, not refused.
    pub(crate) fn admit(payload: &Map<String, Value>) -> Self {
        let mut admitted = Self::default();
        let versioned = payload.get("asset").is_some_and(is_versioned_document);
        for (name, value) in payload {
            let assigned = match name.as_str() {
                "asset" => versioned,
                "tables" if versioned => match value {
                    Value::Array(tables) => {
                        for value in tables {
                            let mut unassigned = 0;
                            match table(value, &mut unassigned) {
                                Some(table) => {
                                    admitted.tables.push(table);
                                    admitted.unassigned += unassigned;
                                }
                                None => admitted.unassigned += 1,
                            }
                        }
                        true
                    }
                    _ => false,
                },
                "cachedRevIds" => match value {
                    Value::Object(ids) => {
                        admitted.unassigned += ids.len();
                        true
                    }
                    _ => false,
                },
                "testRowData" => value
                    .as_array()
                    .is_some_and(|cells| cells.iter().all(Value::is_string)),
                _ => false,
            };
            admitted.unassigned += usize::from(!assigned);
        }
        admitted
    }
}

fn typed<'a>(value: &'a Value, kind: &str) -> Option<&'a Value> {
    let value = value.as_object()?;
    (value.get("type")?.as_str()? == kind).then_some(value.get("value")?)
}

fn typed_component<'a>(value: &'a Value, component: &str) -> Option<&'a Map<String, Value>> {
    let kind = value.as_object()?.get("type")?.as_str()?;
    if kind.strip_prefix(COMPONENT_PREFIX)? != component {
        return None;
    }
    typed(value, kind)?.as_object()
}

fn string<'a>(members: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    typed(members.get(name)?, "String")?.as_str()
}

fn boolean(members: &Map<String, Value>, name: &str) -> Option<bool> {
    typed(members.get(name)?, "Bool")?.as_bool()
}

fn asset<'a>(value: &'a Value, kind: &str) -> Option<&'a Map<String, Value>> {
    let asset = value.as_object()?;
    (asset.get("type")?.as_str()?.strip_prefix(ASSET_PREFIX)? == kind).then_some(asset)
}

fn attribute<'a>(asset: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    asset.get("attributes")?.as_object()?.get(name)?.as_str()
}

/// Properties of the one component with `name` and type `component`.
fn component<'a>(
    asset: &'a Map<String, Value>,
    name: &str,
    component: &str,
) -> Option<&'a Map<String, Value>> {
    let mut matches = asset
        .get("components")?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .filter(|candidate| candidate.get("name").and_then(Value::as_str) == Some(name));
    let found = matches.next()?;
    if matches.next().is_some()
        || found
            .get("type")?
            .as_str()?
            .strip_prefix(COMPONENT_PREFIX)?
            != component
    {
        return None;
    }
    found.get("properties")?.as_object()
}

fn is_versioned_document(value: &Value) -> bool {
    asset(value, "configuration-1.0.0")
        .and_then(|asset| component(asset, "tableVersion", "tableData-1.0.0"))
        .and_then(|version| typed(version.get("tableVersion")?, "Int32")?.as_u64())
        == Some(1)
}

fn table(value: &Value, unassigned: &mut usize) -> Option<ConfigurationTable> {
    let members = value.as_object()?;
    let table_asset = asset(members.get("asset")?, "configurationTable-1.1.0")?;
    let id = attribute(table_asset, "pim:configuration:tableId")?;
    let definition = component(table_asset, "tableDefinition", "tableDefinition-1.1.0")?;
    if string(definition, "id")? != id {
        return None;
    }
    let columns = typed(
        definition.get("columnData")?,
        "map<autodesk.configuration:components.columnData-1.1.0>",
    )?
    .as_object()?;
    let mut targets = HashMap::new();
    for (column_id, column) in columns {
        let column = typed_component(column, "columnData-1.1.0")?;
        if string(column, "id")? != column_id {
            return None;
        }
        match column_target(column) {
            Some(target) => {
                targets.insert(column_id.as_str(), target);
            }
            None => *unassigned += 1,
        }
    }
    let view = asset(
        members.get("tableView")?.as_object()?.get("asset")?,
        "tableView-1.0.0",
    )?;
    let view = component(view, "viewData", "tableViewData-1.0.0")?;
    let order = typed(view.get("rowOrder")?, "String")?.as_array()?;
    let mut rows = HashMap::new();
    for value in members.get("rows")?.as_array()? {
        let row = row(value, columns, &targets, unassigned)?;
        let id = row.id.clone();
        if rows.insert(id, row).is_some() {
            return None;
        }
    }
    if order.len() != rows.len() {
        return None;
    }
    let mut ordered = Vec::new();
    for id in order {
        ordered.push(rows.remove(id.as_str()?)?);
    }
    *unassigned += members
        .keys()
        .filter(|name| !matches!(name.as_str(), "asset" | "rows" | "tableView"))
        .count();
    Some(ConfigurationTable {
        id: id.to_owned(),
        rows: ordered,
    })
}

fn column_target(column: &Map<String, Value>) -> Option<ColumnTarget> {
    if boolean(column, "isCustom")? {
        return None;
    }
    let object = column.get("object")?;
    if let Some(property) = typed_component(object, "configuredProperty-1.0.0") {
        if string(property, "type")? != "Property" || !string(property, "propertySet")?.is_empty() {
            return None;
        }
        return Some(ColumnTarget::Property(
            string(property, "propertyKey")?.to_owned(),
        ));
    }
    let object = typed_component(object, "configuredObject-1.0.0")?;
    let record = string(object, "id")?;
    if record.is_empty() || !record.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let record = record.parse().ok()?;
    match string(object, "type")? {
        "UserParameterValue" => Some(ColumnTarget::Parameter(record)),
        "ParameterDoubleValue" => Some(ColumnTarget::OwnedParameter(record)),
        "DcFeature" => Some(ColumnTarget::Feature(record)),
        _ => None,
    }
}

fn row(
    value: &Value,
    columns: &Map<String, Value>,
    targets: &HashMap<&str, ColumnTarget>,
    unassigned: &mut usize,
) -> Option<ConfigurationRow> {
    let row_asset = asset(value.as_object()?.get("asset")?, "row-1.0.0")?;
    let id = attribute(row_asset, "pim:configuration:rowId")?;
    let name = attribute(row_asset, "pim:configuration:rowName")?;
    let data = component(row_asset, "rowData", "rowData-1.0.0")?;
    let definition = component(row_asset, "rowDefinition", "rowDefinition-1.0.0")?;
    if string(definition, "id")? != id {
        return None;
    }
    string(data, "configRevisionId")?;
    *unassigned += usize::from(boolean(data, "default")?);
    let cells = typed(
        data.get("rowCells")?,
        "map<autodesk.configuration:components.rowCell-1.0.0>",
    )?
    .as_object()?;
    let mut typed_cells = Vec::new();
    for (column_id, cell) in cells {
        let cell = typed_component(cell, "rowCell-1.0.0")?;
        if string(cell, "columnId")? != column_id || !columns.contains_key(column_id) {
            return None;
        }
        let value = targets
            .get(column_id.as_str())
            .and_then(|target| cell_value(target, cell.get("data")?));
        match value {
            Some(value) => typed_cells.push(ConfigurationCell {
                column_id: column_id.clone(),
                value,
            }),
            None => *unassigned += 1,
        }
    }
    Some(ConfigurationRow {
        id: id.to_owned(),
        name: name.to_owned(),
        cells: typed_cells,
    })
}

fn cell_value(target: &ColumnTarget, data: &Value) -> Option<ConfiguredValue> {
    match target {
        ColumnTarget::Property(key) => {
            let value = typed_component(data, "variant.string-1.0.0")?;
            Some(ConfiguredValue::Property {
                key: key.clone(),
                text: string(value, "value")?.to_owned(),
            })
        }
        ColumnTarget::Parameter(record) => Some(ConfiguredValue::Parameter {
            record_index: *record,
            expression: expression(data)?,
        }),
        ColumnTarget::OwnedParameter(record) => Some(ConfiguredValue::OwnedParameter {
            owner_record_index: *record,
            expression: expression(data)?,
        }),
        ColumnTarget::Feature(record) => {
            let value = typed_component(data, "variant.bool-1.0.0")?;
            Some(ConfiguredValue::Feature {
                record_index: *record,
                computed: boolean(value, "value")?,
            })
        }
    }
}

/// Source expression of an expression cell whose evaluated value is a number.
fn expression(data: &Value) -> Option<String> {
    let value = typed_component(data, "variant.expression-1.0.0")?;
    let evaluated = typed_component(value.get("value")?, "variant.float64-1.0.0")?;
    typed(evaluated.get("value")?, "Float64")?.as_f64()?;
    Some(string(value, "expression")?.to_owned())
}

#[cfg(test)]
mod tests;
