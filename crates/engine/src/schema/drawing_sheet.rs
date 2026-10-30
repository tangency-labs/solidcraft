//! Contracts and result types for the commands in `cmd/drawing_sheet.rs` (#3875).

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::check::whole;

/// A sheet named by its id or, when only one has it, its name.
pub(super) fn sheet_ref<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a sheet's id or name")),
    }
}

/// [`sheet_ref`], for a key that may be absent (`null` is refused).
fn some_sheet_ref<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    sheet_ref(d).map(Some)
}

fn text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(Some(s)),
        _ => Err(serde::de::Error::custom("must be text")),
    }
}

/// A paper size, as `SheetSize` reads it.
fn size<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    match serde_json::from_value::<solidcraft_geom::drawing_sheet::SheetSize>(v.clone()) {
        Ok(_) => Ok(Some(v)),
        Err(_) => Err(serde::de::Error::custom(whole("`size` is {\"standard\": \"a3\"} or {\"custom\": {\"sides_mm\": [w, h]}}"))),
    }
}

fn orientation<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if matches!(s.as_str(), "landscape" | "portrait") => Ok(Some(s)),
        _ => Err(serde::de::Error::custom(whole("`orientation` is landscape or portrait"))),
    }
}

/// A title block's definition, as `TitleBlock` reads it, or `null` for none.
fn title_block<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    if !v.is_null() {
        serde_json::from_value::<solidcraft_geom::title_block::TitleBlock>(v.clone())
            .map_err(|e| serde::de::Error::custom(whole(format!("`title_block`: {e}"))))?;
    }
    Ok(Some(v))
}

/// A finite number below 1e9 in size, as `params::num` reads it.
fn number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)?.as_f64().and_then(crate::params::finite) {
        Some(x) => Ok(Some(x)),
        None => Err(serde::de::Error::custom("must be a number")),
    }
}

/// What a sheet documents: `{source, component?}`, or `null` for nothing.
fn documents<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    let v = Value::deserialize(d)?;
    if v.is_null() {
        return Ok(Some(v));
    }
    if v.get("source").and_then(Value::as_str).and_then(solidcraft_doc::ids::decode).is_none() {
        return Err(serde::de::Error::custom(whole("`documents.source` must be a source's id")));
    }
    if v.get("component").is_some_and(|c| c.as_u64().is_none()) {
        return Err(serde::de::Error::custom(whole("`documents.component` must be a component id")));
    }
    Ok(Some(v))
}

/// What a sheet leaves out: an object of `count` and `print`, each true or false.
fn excluded<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Map<String, Value>>, D::Error> {
    let shape = || serde::de::Error::custom(whole("`excluded` is {count?: true|false, print?: true|false}"));
    let Value::Object(o) = Value::deserialize(d)? else { return Err(shape()) };
    if !o.keys().all(|k| matches!(k.as_str(), "count" | "print")) {
        return Err(shape());
    }
    for key in ["count", "print"] {
        if o.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(serde::de::Error::custom(whole(format!("`excluded.{key}` is true or false"))));
        }
    }
    Ok(Some(o))
}

/// Every sheet's id once, in a new order.
fn every_sheet<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let every = || serde::de::Error::custom(whole("`sheets` must list every sheet's id once"));
    let Value::Array(ids) = Value::deserialize(d)? else { return Err(every()) };
    ids.into_iter()
        .map(|id| id.as_str().filter(|s| solidcraft_doc::ids::decode(s).is_some()).map(str::to_string))
        .collect::<Option<_>>()
        .ok_or_else(every)
}

/// Add a sheet after another: it takes that sheet's paper, border, title block and resolution,
/// unless told otherwise, and a name of its own; it documents nothing and leaves nothing out unless
/// told.
/// One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SheetAddParams {
    /// The sheet it follows, by id or name. Default: the last.
    #[serde(default, deserialize_with = "some_sheet_ref", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub after: Option<String>,
    #[serde(flatten, deserialize_with = "super::flattened")]
    pub settings: SheetSettings,
}

/// Change a sheet's settings, the rest kept: one undo step, none when nothing changes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SheetSetParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    #[serde(flatten, deserialize_with = "super::flattened")]
    pub settings: SheetSettings,
}

/// A sheet's settings, each optional.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SheetSettings {
    /// Its name: 1 to 256 bytes, not only spaces. Names may repeat (a sheet is then named by id).
    #[serde(default, deserialize_with = "text", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    /// Its paper: `{"standard": "a3"}` (ISO A0 to A4, B and C sizes, US letter and the like) or
    /// `{"custom": {"sides_mm": [w, h]}}`, each side above 0 and at most 5000 mm. Its border
    /// must lay out on it.
    #[serde(default, deserialize_with = "size", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub size: Option<Value>,
    /// `landscape` or `portrait`.
    #[serde(default, deserialize_with = "orientation", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub orientation: Option<String>,
    /// Its title block: a definition (`size_mm`, `outline_line_mm`, `cell_line_mm`, `cells`) that
    /// sits on the drawing border's lower-right corner and must fit inside it, or `null` for none.
    #[serde(default, deserialize_with = "title_block", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub title_block: Option<Value>,
    /// The resolution its pictures are rendered at: 25 to 2400 dots per inch.
    #[serde(default, deserialize_with = "number", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub dpi: Option<f64>,
    /// The component it documents, `{"source": id, "component": id}` (component 0, the
    /// default, is the design itself), or `null` for none.
    #[serde(default, deserialize_with = "documents", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Value")]
    pub documents: Option<Value>,
    /// What it leaves out, `{"count"?: true|false, "print"?: true|false}`: from the bill of
    /// materials' counts, from printing. A flag not given is kept.
    #[serde(default, deserialize_with = "excluded", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Map<String, Value>")]
    pub excluded: Option<Map<String, Value>>,
}

/// Put the sheets in a new order. One undo step, none when the order is the same.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SheetReorderParams {
    /// Every sheet's id, once each, in the new order.
    #[serde(deserialize_with = "every_sheet")]
    pub sheets: Vec<String>,
}

/// What becomes of what is on a deleted sheet: `delete`.
fn contents<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if s == "delete" => Ok(Some(s)),
        _ => Err(serde::de::Error::custom(whole("`contents` is delete: the sheet goes with everything on it"))),
    }
}

/// Delete a sheet; a drawing keeps at least one. A sheet with anything on it is refused, naming
/// what, unless `contents` is `delete`. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SheetDeleteParams {
    /// The sheet, by id or name.
    #[serde(deserialize_with = "sheet_ref")]
    pub sheet: String,
    /// `delete`: the views on it and their dimensions go with it, in the same undo step.
    #[serde(default, deserialize_with = "contents", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub contents: Option<String>,
}

/// One of the drawing's sheets.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetName {
    pub id: String,
    pub name: String,
}

/// What the sheet commands return: the drawing's sheets in order, the new sheet's id from
/// `drawing.sheet.add`, and the views a change of paper leaves outside from `drawing.sheet.set`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SheetsResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub sheets: Vec<SheetName>,
    /// After a change of paper, the sheet's views whose boundary leaves its drawing space. None is
    /// moved: what goes where stays the drawing's author's choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside: Vec<super::drawing_view::OutsideResult>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_sheet_tests.rs"]
mod tests;
