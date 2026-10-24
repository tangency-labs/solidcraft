//! Part properties and the bill of materials. `component.metadata` and
//! `component.metadata.set` read and change a component definition's part number, description,
//! revision and free properties (shared by every occurrence, saved with the component);
//! `occurrence.bom_type` lets one occurrence count as another bill of materials type.
//!
//! `assembly.bom` presents the rows of a bill of materials view of the design, or of one
//! component, as JSON, optionally written to a CSV or JSON file. The rows and every rule behind
//! them (bill of materials types, folding, materials, mass) are `solidcraft_doc::bom`'s.

use serde_json::{Map, Value, json};
use solidcraft_doc::BomType;
use solidcraft_doc::bom::{self, Bom, Group, Mass, Request, Row, Tree, View};
use solidcraft_doc::metadata::{BOM_TYPE, DESCRIPTION, MAX_KEYS, MAX_TEXT_BYTES, PART_NUMBER, REVISION, WELL_KNOWN};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.metadata", "Part Properties", read).icon("settings").noundo().doc_only().params(
        "component: id or name (0 or \"root\": the design's own properties) → component, name, part_number, description, revision, \
         bom_type (components only), metadata (as saved, other tools' keys too), properties: {name: text}",
    ),
    CommandSpec::new("component.metadata.set", "Change Part Properties", set).icon("settings").doc_only().params(
        "component: id or name (not the root: use document.metadata.set); part_number?, description?, revision?: text (empty or null \
         removes); bom_type?: normal|phantom|reference|purchased|inseparable (empty or null: normal); values?: {key: value} \
         (SolidCraft's keys project, part_number, revision, author, organization, description, license, bom_type, or a reverse-DNS \
         key holding an object); properties?: {name: text, …} (free properties; empty or null removes). All or nothing, one undo \
         step → component, metadata, properties, changed: [keys]",
    ),
    CommandSpec::new("occurrence.bom_type", "Bill of Materials Type", occurrence_type).icon("bom").doc_only().params(
        "occurrence: id or name; path?: [occurrence ids] (checked when given); bom_type: normal|phantom|reference|purchased|inseparable \
         (this occurrence counts as that type in every bill of materials, in place of its component's; an occurrence inside a \
         subassembly belongs to its definition, so the type holds wherever the subassembly is placed) | same_as_component or null \
         (as its component again). One undo step → occurrence, bom_type (its own, or same_as_component), effective (the type it \
         counts as), changed",
    ),
    CommandSpec::new("assembly.bom", "Bill of Materials", bom).at("SOLID", "INSPECT").icon("bom").noundo().params(
        "component?: id or name (default the design); levels?: flat (default: every part at any depth, counted through its \
         subassemblies) | top (the component's direct children, subassemblies as one row each); group_by?: [\"part_number\"] \
         (default; components without a part number get a row per definition) | [\"definition\"]; group_children?: true (a component \
         with a part number is one row, the parts placed inside it listed under it as children: [{item, part_number, description, \
         revision, each, count, definition, material, mass_g?}], not as rows of their own); mass?: true (measures each part: \
         mass_g, what one of the row's parts weighs in grams with what is placed inside it, except what the list shows as rows of \
         their own beside it (in parts only a part's contents are rows of their own, but not a folded part's or a purchased one's, \
         and of an inseparable one's only its purchased parts are), so the rows add up to the assembly; a reference placement \
         adds nothing; null when nothing in it has a body, a body can't be measured or the row's definitions differ); path?: write \
         a .csv or .json file (children as items 1.1, 1.2…); format?: csv|json (default from the path). Each placement counts as \
         its occurrence's bom_type, else its component's: phantom has no row (what it holds counts here), reference counts \
         nowhere, purchased and inseparable are one row (in flat, an inseparable one's purchased parts are listed too); placements \
         of one component that count as different types are different rows → rows: [{item, part_number, description, revision, \
         bom_type, count, definition, material (the physical materials of the definitions' own bodies, \"; \" between them; a \
         row that weighs what is placed inside it shows the one material all of that shares, else \"\"), mass_g?, components, \
         properties, children?}] sorted by part number, total_occurrences (children included)",
    ),
];

/// Longest free property name, in bytes.
const MAX_PROPERTY_NAME_BYTES: usize = 128;

fn key(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn component_id(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let k = key(p.get("component")).ok_or_else(|| bad(cmd, "`component` must be a component id or name"))?;
    s.doc.find_component(&k).ok_or_else(|| bad(cmd, format!("no component `{k}`")))
}

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.metadata";
    let id = component_id(s, p, cmd)?;
    if id == 0 {
        let m = &s.doc.metadata;
        return Ok(json!({"component": 0, "name": s.doc.name, "part_number": m.part_number(), "description": m.description(),
            "revision": m.revision(), "metadata": m, "properties": {}}));
    }
    let c = s.doc.components.iter().find(|c| c.id == id).ok_or_else(|| bad(cmd, format!("no component `{id}`")))?;
    let m = &c.metadata;
    Ok(json!({"component": id, "name": c.name, "part_number": m.part_number(), "description": m.description(),
        "revision": m.revision(), "bom_type": m.bom_type().as_str(), "metadata": m, "properties": c.properties}))
}

/// A free property as it may be saved (`None`: remove it), or why not.
fn property(name: &str, value: &Value, cmd: &str) -> Result<Option<String>> {
    let shown: String = name.chars().take(64).collect();
    if name.trim().is_empty() || name.len() > MAX_PROPERTY_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(bad(cmd, format!("property name `{shown}` must be 1…{MAX_PROPERTY_NAME_BYTES} bytes of text without control characters")));
    }
    let text = match value {
        Value::Null => return Ok(None),
        Value::String(t) if t.is_empty() => return Ok(None),
        Value::String(t) => t.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return Err(bad(cmd, format!("property `{shown}` takes text"))),
    };
    if text.len() > MAX_TEXT_BYTES {
        return Err(bad(cmd, format!("property `{shown}` is longer than {MAX_TEXT_BYTES} bytes")));
    }
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r')) {
        return Err(bad(cmd, format!("property `{shown}` has a control character")));
    }
    Ok(Some(text))
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.metadata.set";
    let id = component_id(s, p, cmd)?;
    if id == 0 {
        return Err(bad(cmd, "the root is the design: its properties are document.metadata.set's"));
    }
    let c = s.doc.components.iter().find(|c| c.id == id).ok_or_else(|| bad(cmd, format!("no component `{id}`")))?;
    let (mut metadata, mut properties) = (c.metadata.clone(), c.properties.clone());
    let mut entries: Vec<(String, Value)> = Vec::new();
    match p.get("values") {
        None => {}
        Some(Value::Object(v)) if v.len() <= MAX_KEYS => entries.extend(v.iter().map(|(k, v)| (k.clone(), v.clone()))),
        Some(Value::Object(_)) => return Err(bad(cmd, format!("`values` can hold at most {MAX_KEYS} keys"))),
        Some(_) => return Err(bad(cmd, "`values` must be an object: {key: value, …}")),
    }
    for k in [PART_NUMBER, DESCRIPTION, REVISION, BOM_TYPE] {
        if let Some(v) = p.get(k) {
            entries.push((k.to_string(), v.clone()));
        }
    }
    let props: Vec<(&String, &Value)> = match p.get("properties") {
        None => Vec::new(),
        Some(Value::Object(v)) if v.len() <= MAX_KEYS => v.iter().collect(),
        Some(Value::Object(_)) => return Err(bad(cmd, format!("`properties` can hold at most {MAX_KEYS} names"))),
        Some(_) => return Err(bad(cmd, "`properties` must be an object: {name: text, …}")),
    };
    if entries.is_empty() && props.is_empty() {
        return Err(bad(
            cmd,
            format!("give part_number, description, revision, bom_type, values or properties ({}, {BOM_TYPE})", WELL_KNOWN.join(", ")),
        ));
    }
    let mut changed: Vec<String> = Vec::new();
    for (k, v) in entries {
        if metadata.set(&k, v).map_err(|e| bad(cmd, e.to_string()))? && !changed.contains(&k) {
            changed.push(k);
        }
    }
    for (name, value) in props {
        let was = properties.get(name).cloned();
        match property(name, value, cmd)? {
            Some(text) => {
                properties.insert(name.clone(), text);
            }
            None => {
                properties.remove(name);
            }
        }
        if properties.get(name) != was.as_ref() {
            changed.push(format!("properties.{name}"));
        }
    }
    if properties.len() > MAX_KEYS {
        return Err(bad(cmd, format!("a component holds at most {MAX_KEYS} properties")));
    }
    // Only a real change touches the design (an undo step, unsaved changes).
    if !changed.is_empty()
        && let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id)
    {
        c.metadata = metadata;
        c.properties = properties;
    }
    let c = s.doc.components.iter().find(|c| c.id == id);
    Ok(json!({"component": id, "metadata": c.map(|c| &c.metadata), "properties": c.map(|c| &c.properties), "changed": changed}))
}

/// The word that clears an occurrence's own bill of materials type.
const SAME_AS_COMPONENT: &str = "same_as_component";

fn occurrence_type(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.bom_type";
    let id = super::component::occurrence(s, p, cmd)?;
    if p.get("path").is_some() {
        super::component::occurrence_path(s, p, id, cmd)?;
    }
    let words = || format!("{}, or {SAME_AS_COMPONENT}", BomType::ALL.map(BomType::as_str).join(", "));
    let want = match p.get("bom_type") {
        Some(Value::Null) => None,
        Some(Value::String(w)) if w == SAME_AS_COMPONENT => None,
        Some(Value::String(w)) => Some(BomType::parse(w).ok_or_else(|| bad(cmd, format!("`bom_type` is one of {}", words())))?),
        _ => return Err(bad(cmd, format!("give `bom_type`: {}", words()))),
    };
    let changed = s.doc.occurrences.iter().find(|o| o.id == id).ok_or_else(|| bad(cmd, "no occurrence"))?.bom_type != want;
    // Only a real change touches the design (an undo step, unsaved changes).
    if changed && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
        o.bom_type = want;
    }
    let effective = s.doc.occurrences.iter().zip(s.doc.occurrence_bom_types()).find(|(o, _)| o.id == id).map(|(_, t)| t).unwrap_or_default();
    Ok(json!({"occurrence": id, "bom_type": want.map_or(SAME_AS_COMPONENT, BomType::as_str), "effective": effective.as_str(),
        "changed": changed}))
}

/// What `assembly.bom`'s parameters ask for: the design's bill of materials unless `component`
/// names one.
fn request(s: &Session, p: &Value, cmd: &str) -> Result<Request> {
    let source = if p.get("component").is_some_and(|v| !v.is_null()) { component_id(s, p, cmd)? } else { 0 };
    let views = || format!("`levels` is {}", View::ALL.map(View::as_str).join(" or "));
    let view = match p.get("levels") {
        None | Some(Value::Null) => View::default(),
        Some(Value::String(l)) => View::parse(l).ok_or_else(|| bad(cmd, views()))?,
        Some(_) => return Err(bad(cmd, views())),
    };
    let group_by = || bad(cmd, "`group_by` is [\"part_number\"] or [\"definition\"]");
    let group = match p.get("group_by") {
        None | Some(Value::Null) => Group::default(),
        Some(Value::String(g)) => group_key(g, cmd)?,
        Some(Value::Array(a)) => match a.as_slice() {
            [Value::String(g)] => group_key(g, cmd)?,
            _ => return Err(group_by()),
        },
        Some(_) => return Err(group_by()),
    };
    let flag = |k: &str| match p.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(cmd, format!("`{k}` is true or false"))),
    };
    Ok(Request { source, view, group, fold: flag("group_children")?, mass: flag("mass")? })
}

fn group_key(g: &str, cmd: &str) -> Result<Group> {
    Group::parse(g).ok_or_else(|| bad(cmd, format!("can't group by `{g}` (part_number or definition)")))
}

/// A row as `assembly.bom` gives it, under the rows whose item numbers are `under` (none at the
/// top). A row listed under another reads its item as "1.2" and gives `each`, and leaves out its
/// components and properties (the output #1888 set).
fn row_json(row: &Row, under: &[u32], mass: bool) -> Value {
    let mut item = under.to_vec();
    item.push(row.item);
    let mut v = json!({"part_number": row.part_number, "description": row.description, "revision": row.revision,
        "bom_type": row.bom_type.as_str(), "count": row.count, "definition": row.names.join("; "), "material": row.materials.join("; ")});
    if under.is_empty() {
        v["item"] = json!(row.item);
        v["components"] = json!(row.components);
        v["properties"] = json!(row.properties);
    } else {
        v["item"] = json!(bom::item_text(&item));
        v["each"] = json!(row.each);
    }
    if mass {
        v["mass_g"] = match row.mass {
            Mass::Grams(grams) => json!(grams),
            Mass::NotAsked | Mass::Unknown(_) => Value::Null,
        };
    }
    if !row.children.is_empty() {
        v["children"] = row.children.iter().map(|c| row_json(c, &item, mass)).collect();
    }
    v
}

/// A CSV field, always quoted.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The bill of materials as CSV: fixed columns, every field quoted, each row followed by the rows
/// listed under it.
fn csv(list: &Bom) -> String {
    let mut out = ["item", "part_number", "description", "revision", "count", "definition"].map(quote).join(",");
    out.push('\n');
    for (item, r) in list.rows_deep() {
        let (item, count, definition) = (bom::item_text(&item), r.count.to_string(), r.names.join("; "));
        let fields: [&str; 6] = [&item, &r.part_number, &r.description, &r.revision, &count, &definition];
        out.push_str(&fields.map(quote).join(","));
        out.push('\n');
    }
    out
}

fn bom(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "assembly.bom";
    let request = request(s, p, cmd)?;
    let state = s.model.state();
    let list = bom::list(&mut Tree::new(&s.doc, &state), &request).map_err(|e| bad(cmd, e.to_string()))?;
    let rows: Vec<Value> = list.rows.iter().map(|r| row_json(r, &[], request.mass)).collect();
    let mut out = Map::new();
    out.insert("component".into(), json!(request.source));
    out.insert("levels".into(), json!(request.view.as_str()));
    out.insert("group_by".into(), json!([request.group.as_str()]));
    out.insert("group_children".into(), json!(request.fold));
    out.insert("mass".into(), json!(request.mass));
    if let Some(path) = str_(p, "path") {
        let format = match str_(p, "format").map(str::to_ascii_lowercase) {
            Some(f) => f,
            None if path.to_ascii_lowercase().ends_with(".json") => "json".into(),
            None if path.to_ascii_lowercase().ends_with(".csv") => "csv".into(),
            None => return Err(bad(cmd, "`path` must end in .csv or .json, or give `format`")),
        };
        let text = match format.as_str() {
            "csv" => csv(&list),
            "json" => serde_json::to_string_pretty(&json!({"rows": rows, "total_occurrences": list.total.occurrences}))
                .map_err(|e| bad(cmd, e.to_string()))?,
            other => return Err(bad(cmd, format!("unknown format `{other}` (csv or json)"))),
        };
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        out.insert("path".into(), json!(path));
        out.insert("format".into(), json!(format));
    }
    out.insert("rows".into(), Value::Array(rows));
    out.insert("total_occurrences".into(), json!(list.total.occurrences));
    Ok(Value::Object(out))
}

#[cfg(test)]
#[path = "bom_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
