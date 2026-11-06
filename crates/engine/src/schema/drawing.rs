//! Contracts and result types for the commands in `cmd/drawing.rs` (#3875): making a drawing,
//! its own settings, and the templates a drawing starts from.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::drawing::{Units, Updates};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_geom::revision::Numbering;

use super::check::whole;
use super::drawing_view::LineGroupParam;
use super::types::present;

/// Make a drawing of the design from a template, with ids of its own, and open it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DrawingCreateParams {
    /// The drawing's name (the design's when not given).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    /// A built-in's name (`drawing.template.list`) or a .json template file
    /// (`drawing.template.save`): the design's kind's built-in when not given.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub template: Option<String>,
    /// Its content language, a BCP 47 tag (the template's when not given).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub language: Option<String>,
    #[serde(default, deserialize_with = "units", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "units_schema")]
    pub units: Option<Units>,
    #[serde(default, deserialize_with = "projection", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "projection_schema")]
    pub projection: Option<Projection>,
    /// The ISO 128 line group its lines are drawn in (the template's when not given).
    #[serde(default, deserialize_with = "line_group", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LineGroupParam")]
    pub line_group: Option<LineGroup>,
    /// The font family its text is set in, by name, never a file ("": the built-in font).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub font: Option<String>,
}

/// Change the open drawing's own settings: one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DrawingSetParams {
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    /// Its content language, a BCP 47 tag.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub language: Option<String>,
    #[serde(default, deserialize_with = "units", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "units_schema")]
    pub units: Option<Units>,
    /// Which every projected view follows.
    #[serde(default, deserialize_with = "projection", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "projection_schema")]
    pub projection: Option<Projection>,
    #[serde(default, deserialize_with = "updates", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "updates_schema")]
    pub updates: Option<Updates>,
    /// The ISO 128 line group its lines are drawn in; a sheet or a view may choose another.
    #[serde(default, deserialize_with = "line_group", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "LineGroupParam")]
    pub line_group: Option<LineGroup>,
    /// The font family its text is set in, by name, never a file ("": the built-in font).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub font: Option<String>,
    #[serde(default, deserialize_with = "revision_numbering", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "revision_numbering_schema")]
    pub revision_numbering: Option<Numbering>,
}

/// `units` as given: `follow_model`, or a length unit the drawing's rules check.
fn units<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Units>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(u) if u == "follow_model" => Ok(Some(Units::FollowModel)),
        Value::String(unit) => Ok(Some(Units::Fixed { unit })),
        _ => Err(D::Error::custom(whole("`units` is \"follow_model\" or a length unit such as \"mm\""))),
    }
}

fn units_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "\"follow_model\" (the model's unit), or one length unit such as \"mm\".", "type": "string"})
}

/// A `line_group` as given, in the words a sheet's is refused with.
fn line_group<'de, D: Deserializer<'de>>(d: D) -> Result<Option<LineGroup>, D::Error> {
    crate::cmd::line_group_of(&Value::deserialize(d)?).map(Some).map_err(|e| D::Error::custom(whole(e)))
}

/// `projection` as given: first or third angle.
fn projection<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Projection>, D::Error> {
    serde_json::from_value(Value::deserialize(d)?).map(Some).map_err(|_| D::Error::custom(whole("`projection` is first or third (angle)")))
}

fn projection_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "First or third angle projection.", "type": "string", "enum": ["first", "third"]})
}

/// `updates` as given: when the views follow a changed model.
fn updates<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Updates>, D::Error> {
    serde_json::from_value(Value::deserialize(d)?).map(Some).map_err(|_| D::Error::custom(whole("`updates` is automatic or deferred")))
}

fn updates_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "When its views follow a changed model: automatic, or deferred (only on drawing.update).",
        "type": "string",
        "enum": ["automatic", "deferred"]
    })
}

/// How new revisions are to be numbered. A start the record's rules refuse is refused with them.
fn revision_numbering<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Numbering>, D::Error> {
    serde_json::from_value(Value::deserialize(d)?).map(Some).map_err(|_| D::Error::custom(whole("`revision_numbering` is {start: 0 or 1}")))
}

fn revision_numbering_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The number new revisions count from: rows already there stay as typed.",
        "type": "object",
        "properties": {"start": {"type": "integer", "enum": [0, 1]}},
        "required": ["start"],
        "additionalProperties": false
    })
}

/// Give each sheet of the drawing open the paper, drawing border and title block of a template's
/// sheet, moving nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TemplateApplyParams {
    /// A built-in's name, as `drawing.template.list` lists it, or a .json template file
    /// (`drawing.template.save`). Its sheet in each sheet's place is applied, its last for the
    /// sheets past its own.
    pub template: String,
    /// True to answer what would change without changing anything. Default: false.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub dry_run: Option<bool>,
}

/// A sheet a template changes, and what of it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetTemplateChange {
    pub sheet: String,
    /// It takes the template sheet's paper.
    pub paper: bool,
    /// It takes the template sheet's drawing border.
    pub border: bool,
    /// It takes the template sheet's title block.
    pub title_block: bool,
    /// The cells whose typed text is dropped: the new block has no text cell of their name.
    pub lost: Vec<String>,
}

/// What `drawing.template.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateApplyResult {
    /// Each sheet that changes, in order.
    pub changed: Vec<SheetTemplateChange>,
    /// The views on a restyled sheet that fall outside its drawing space or over its title block.
    /// None is moved.
    pub outside: Vec<super::drawing_view::OutsideResult>,
    /// Whether the call only answered, changing nothing.
    pub dry_run: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Write the drawing open as a template file: its sheets, borders, title blocks and the text typed
/// over their cells, its projection, units, language and properties, and nothing it draws or
/// documents.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TemplateSaveParams {
    /// The file to write: a .json path, at most 4096 bytes long.
    pub path: String,
}

/// What `drawing.template.save` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateSaveResult {
    /// The file written.
    pub path: String,
    /// How many bytes it holds.
    pub bytes: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List the templates a new drawing can start from.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TemplateListParams {}

/// One template a drawing can start from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateEntry {
    /// Its name, as `drawing.create`'s `template` takes it.
    pub name: String,
    /// What the app calls it.
    pub title: String,
    /// Whether it is an assembly's: the general tolerances it carries are an assembly's.
    pub assembly: bool,
    /// Whether `drawing.create` starts from it for this design when no template is named.
    pub default: bool,
}

/// What `drawing.template.list` returns: the built-in templates, ISO A4 to A0 for a part and for
/// an assembly.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TemplateListResult {
    pub templates: Vec<TemplateEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
