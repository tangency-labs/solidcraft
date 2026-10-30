//! Contracts and result types for the commands in `cmd/drawing_table.rs` (#3874).
//!
//! A table's definition is the record's own words: `columns`, `filter` and `overrides` as a drawing
//! saves them, checked by the record's rules, so nothing here says them twice.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A table, by its id as `drawing.inspect` lists it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableIdParam(pub u64);

impl<'de> Deserialize<'de> for TableIdParam {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(TableIdParam).ok_or_else(|| serde::de::Error::custom("must be a table's id")),
            _ => Err(serde::de::Error::custom("must be a table's id")),
        }
    }
}

impl JsonSchema for TableIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingTableId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A table's id: 13 characters of lowercase Crockford base32.", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

/// What a table lists: `revision`, or `parts_list`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KindParam {
    Revision,
    PartsList,
}

/// The view a parts list lists, as `assembly.bom` takes it: each left out stays as it is, or is the
/// default on a new list.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct ListSet {
    /// Id or name. A new list takes the component its sheet documents, else the design.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub component: Option<Value>,
    /// `flat`, `top` or `all`.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub levels: Option<Value>,
    /// `part_number` or `definition`.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub group_by: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub group_children: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub show_purchased_contents: Option<Value>,
    /// How it shows a mass: `{unit: g | kg | lb, decimals: 0 to 6}` (default kg to 2).
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub mass: Option<Value>,
}

/// What a table reads like and where it sits: each left out stays as it is.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct TableSet {
    /// The sheet, by id or name.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub sheet: Option<String>,
    /// A parts list's view: refused on a revision table.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ListSet")]
    pub list: Option<ListSet>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub columns: Option<Value>,
    /// `top` or `bottom`.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub heading: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub row_height_mm: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub cap_mm: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Value>,
    /// `{anchor: {corner: top_left | top_right | bottom_left | bottom_right, offset_mm?: [x, y]}}`,
    /// or `{at: [x, y]}`, its outline's top left from the sheet's lower-left corner.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    pub place: Option<Value>,
}

/// What `drawing.table.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "super::drawing_sheet::sheet_ref")]
    pub sheet: String,
    pub kind: KindParam,
    #[serde(default)]
    pub set: TableSet,
}

/// What `drawing.table.edit` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub table: TableIdParam,
    pub set: TableSet,
}

/// What `drawing.table.delete` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DeleteParams {
    pub table: TableIdParam,
}

/// A table on a sheet.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TableResult {
    pub table: String,
    pub sheet: String,
    /// The record, as the drawing saves it.
    pub record: Value,
    /// Its view's item numbers couldn't be saved, so they can still move, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// What `drawing.table.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteResult {
    pub table: String,
}

#[cfg(test)]
#[path = "drawing_table_tests.rs"]
mod tests;
