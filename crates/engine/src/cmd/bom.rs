//! Part properties and the bill of materials. `component.metadata` and
//! `component.metadata.set` read and change a component definition's part number, description,
//! revision and free properties (shared by every occurrence, saved with the component);
//! `occurrence.bom_type` lets one occurrence count as another bill of materials type.
//!
//! `assembly.bom` presents the rows of a bill of materials view of the design, or of one
//! component, as JSON, optionally written to a CSV or JSON file. `assembly.bom.renumber` and
//! `assembly.bom.item.set` number a view, which saves its item numbers; [`record_item_numbers`]
//! saves a numbered view's new rows' numbers after each change. The rows and every rule behind
//! them (bill of materials types, folding, materials, mass, item numbers) are
//! `solidcraft_doc::bom`'s.

use serde_json::{Map, Value, json};
use solidcraft_doc::BomType;
use solidcraft_doc::bom::{self, Bom, Group, MAX_ITEM, Mass, Numbering, Order, Quantity, Request, Row, Tree, Uncounted, View};
use solidcraft_doc::metadata::{BOM_QUANTITY, BOM_TYPE, DESCRIPTION, MATERIAL, MAX_KEYS, MAX_TEXT_BYTES, PART_NUMBER, REVISION, VIRTUAL, WELL_KNOWN};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::bom::{OccurrenceBomTypeParams, OccurrenceBomTypeResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const OCCURRENCE_BOM_TYPE_CONTRACT: TypedContract<OccurrenceBomTypeParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.metadata", "Part Properties", read).icon("settings").noundo().doc_only().params(
        "component: id or name (0 or \"root\": the design's own properties) → component, name, part_number, description, revision, \
         bom_type, material (its static material or null), bom_quantity (or null) and virtual (components only), metadata (as saved, other tools' keys too), \
         properties: {name: text}",
    ),
    CommandSpec::new("component.metadata.set", "Change Part Properties", set).icon("settings").doc_only().params(
        "component: id or name (not the root: use document.metadata.set); part_number?, description?, revision?: text (empty or null \
         removes); bom_type?: normal|phantom|reference|purchased|inseparable (empty or null: normal); material?: text, a static \
         material: what its bill of materials rows say it is made of, in place of its bodies' materials (it doesn't change the \
         mass; empty or null clears it); bom_quantity?: an expression, what one placement means in a bill of materials: a \
         number of pieces (\"4\") or a length (\"2.5 m\", \"2 * span + 50 mm\"), evaluated with the design's parameters, above \
         0 (empty or null removes it: one piece); virtual?: true|false (a component with no geometry, such as grease or a \
         label: it can't be the active component or hold bodies, sketches or components); values?: {key: value} (SolidCraft's \
         keys project, part_number, revision, author, organization, description, license, bom_type, material, bom_quantity, \
         virtual, or a reverse-DNS key holding an object); properties?: \
         {name: text, …} (free properties; empty or null removes). All or nothing, one undo step → component, metadata, \
         properties, changed: [keys]",
    ),
    CommandSpec::new("occurrence.bom_type", "Bill of Materials Type", crate::checked!("occurrence.bom_type", OCCURRENCE_BOM_TYPE_CONTRACT, occurrence_type))
        .icon("bom")
        .doc_only()
        .contract(&OCCURRENCE_BOM_TYPE_CONTRACT)
        .output(output_schema_for::<OccurrenceBomTypeResult>)
        .params(
        "occurrence: id or name; path?: [occurrence ids] (checked when given); bom_type: normal|phantom|reference|purchased|inseparable \
         (this occurrence counts as that type in every bill of materials, in place of its component's; an occurrence inside a \
         subassembly belongs to its definition, so the type holds wherever the subassembly is placed) | same_as_component or null \
         (as its component again). One undo step → occurrence, bom_type (its own, or same_as_component), effective (the type it \
         counts as), changed",
    ),
    CommandSpec::new("assembly.bom", "Bill of Materials", bom).at("SOLID", "INSPECT").icon("bom").noundo().params(
        "component?: id or name (default the design); levels?: flat (default: every part at any depth, counted through its \
         subassemblies) | top (the component's direct children, subassemblies as one row each) | all (as top, each subassembly's \
         own rows listed after it, to any depth: items 1.1, 1.1.1, as in the CSV); show_purchased_contents?: true (in all, a \
         purchased part's contents listed under it too); group_by?: [\"part_number\"] \
         (default; components without a part number get a row per definition) | [\"definition\"]; group_children?: true (a component \
         with a part number is one row, the parts placed inside it listed under it as children: [{item, part_number, description, \
         revision, each, count, definition, material, mass_g?}], not as rows of their own); mass?: true (measures each part: \
         mass_g, what one of the row's parts weighs in grams with what is placed inside it, except what the list shows as rows of \
         their own beside it (in parts only a part's contents are rows of their own, but not a folded part's or a purchased one's, \
         and of an inseparable one's only its purchased parts are), so the rows add up to the assembly; a reference placement \
         adds nothing; null when nothing in it has a body, a body can't be measured or the row's definitions differ); path?: write \
         a .csv (columns item, part_number, description, revision, count, definition, material, quantity) or .json file (children as \
         items 1.1, 1.2…); format?: csv|json (default from the path). Each placement counts as \
         its occurrence's bom_type, else its component's: phantom has no row (what it holds counts here; its own bodies, \
         if it has any, are a row of their own), reference counts nowhere, purchased and inseparable are one row (in flat, an inseparable one's purchased parts are listed too); placements \
         of one component that count as different types are different rows → rows: [{item, part_number, description, revision, \
         bom_type, count, definition, material (the physical materials of the definitions' own bodies, \"; \" between them; a \
         row that weighs what is placed inside it shows the one material all of that shares, else \"\"), mass_g?, quantity? \
         (when its definitions have a bom_quantity: what its placements add up to, a number with quantity_unit pieces or mm, a \
         placement without one counting 1 piece; null with quantity_unknown mixed (pieces and a length) or invalid (a quantity \
         that no longer evaluates)), components, \
         properties, children?, static? ([\"material\"]: a value typed in for the component, shown in place of the worked-out \
         one)}] sorted by part number, total_occurrences (children included), total_mass_g (with mass: \
         what the assembly weighs, measured from the design rather than added up from rows; null when unknown), numbered \
         (the view's item numbers are saved: assembly.bom.renumber). In a numbered view each level is listed by item number \
         and a row says provisional (its number not saved yet: a row added since shows the next number at its level, saved \
         by the next change), locked and duplicate when true",
    ),
    CommandSpec::new("assembly.bom.renumber", "Renumber Items", renumber).icon("bom").params(
        "component?, levels?, group_by?, group_children?, show_purchased_contents?: the view, as assembly.bom takes them; \
         start?: 1; step?: 1 (whole numbers, 1 to 999999); order?: list (default: by part number, as an unnumbered view \
         lists) | item (in item-number order, closing the gaps). Numbers every row from start by step, each level on its \
         own (in all levels, within its parent); locked rows keep their numbers and the others skip them; rows that are \
         gone are forgotten. Saves the view's numbers, with these list options: after it, a row added takes the highest \
         number at its level + 1 and a row removed leaves a gap until the next renumber. One undo step → the view as \
         assembly.bom gives it",
    ),
    CommandSpec::new("assembly.bom.item.set", "Change Item Number", item_set).icon("bom").params(
        "component?, levels?, group_by?, group_children?, show_purchased_contents?: the view, as assembly.bom takes them; \
         row: the row's item as the view shows it now (\"3\", or \"1.2\" under row 1); item?: 1 to 999999 (a number \
         another row has is allowed: both are marked duplicate); locked?: true|false (a renumber keeps a locked number). \
         A view not numbered yet is numbered first, every row keeping the number it shows. One undo step → the view as \
         assembly.bom gives it, with duplicates: [item numbers used twice at the row's level]",
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
        "revision": m.revision(), "bom_type": m.bom_type().as_str(), "material": m.material(), "bom_quantity": m.bom_quantity(),
        "virtual": m.is_virtual(), "metadata": m, "properties": c.properties}))
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
    for k in [PART_NUMBER, DESCRIPTION, REVISION, BOM_TYPE, MATERIAL, BOM_QUANTITY, VIRTUAL] {
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
            format!(
                "give part_number, description, revision, bom_type, material, bom_quantity, virtual, values or properties ({}, {BOM_TYPE}, \
                 {MATERIAL}, {BOM_QUANTITY}, {VIRTUAL})",
                WELL_KNOWN.join(", ")
            ),
        ));
    }
    let mut changed: Vec<String> = Vec::new();
    for (k, v) in entries {
        if metadata.set(&k, v).map_err(|e| bad(cmd, e.to_string()))? && !changed.contains(&k) {
            changed.push(k);
        }
    }
    if changed.iter().any(|k| k == BOM_QUANTITY)
        && let Some(text) = metadata.bom_quantity()
    {
        bom::amount(&s.doc, text).map_err(|e| bad(cmd, format!("`{BOM_QUANTITY}`: {e}")))?;
    }
    // What a virtual component may not hold is `Session::execute`'s check, for every command.
    if metadata.is_virtual() && s.active_component == id {
        return Err(bad(cmd, "the active component can't be virtual: activate another one first"));
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
pub(crate) const SAME_AS_COMPONENT: &str = "same_as_component";

/// `occurrence.bom_type`'s `bom_type`: a type, or `None` for the component's (`same_as_component`
/// or null); the handler's message for anything else, or for none given.
pub(crate) fn occurrence_bom_type(v: Option<&Value>) -> std::result::Result<Option<BomType>, String> {
    let words = || format!("{}, or {SAME_AS_COMPONENT}", BomType::ALL.map(BomType::as_str).join(", "));
    match v {
        Some(Value::Null) => Ok(None),
        Some(Value::String(w)) if w == SAME_AS_COMPONENT => Ok(None),
        Some(Value::String(w)) => BomType::parse(w).map(Some).ok_or_else(|| format!("`bom_type` is one of {}", words())),
        _ => Err(format!("give `bom_type`: {}", words())),
    }
}

fn occurrence_type(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.bom_type";
    let id = super::component::occurrence(s, p, cmd)?;
    if p.get("path").is_some() {
        super::component::occurrence_path(s, p, id, cmd)?;
    }
    let want = occurrence_bom_type(p.get("bom_type")).map_err(|e| bad(cmd, e))?;
    let changed = s.doc.occurrences.iter().find(|o| o.id == id).ok_or_else(|| bad(cmd, "no occurrence"))?.bom_type != want;
    // Only a real change touches the design (an undo step, unsaved changes).
    if changed && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
        o.bom_type = want;
    }
    let effective = s.doc.occurrences.iter().zip(s.doc.occurrence_bom_types()).find(|(o, _)| o.id == id).map(|(_, t)| t).unwrap_or_default();
    let reply = OccurrenceBomTypeResult {
        occurrence: id,
        bom_type: want.map_or(SAME_AS_COMPONENT, BomType::as_str).to_string(),
        effective: effective.as_str().to_string(),
        changed,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
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
    Ok(Request { source, view, group, fold: flag("group_children")?, mass: flag("mass")?, purchased_contents: flag("show_purchased_contents")? })
}

fn group_key(g: &str, cmd: &str) -> Result<Group> {
    Group::parse(g).ok_or_else(|| bad(cmd, format!("can't group by `{g}` (part_number or definition)")))
}

/// A mass as `assembly.bom` gives it: grams, or null when it isn't known.
fn grams(mass: Mass) -> Value {
    match mass {
        Mass::Grams(grams) => json!(grams),
        Mass::NotAsked | Mass::Unknown(_) => Value::Null,
    }
}

/// The rows as `assembly.bom` gives them. In all levels they come one after another, each followed
/// by the rows under it (items 1, 1.1, 1.1.1), as the CSV lists them: nested, 64 levels would be
/// two containers a level, deeper than JSON readers take (serde_json stops at 128). Otherwise a
/// row's children (a folded part's contents, one level) are listed in it.
fn rows_json(list: &Bom, view: View, mass: bool) -> Vec<Value> {
    if view == View::AllLevels {
        list.rows_deep().iter().map(|(item, row)| row_json(row, item, mass, list.numbered, false)).collect()
    } else {
        list.rows.iter().map(|row| row_json(row, &[row.item], mass, list.numbered, true)).collect()
    }
}

/// One row, `item` its item numbers from the top (`[2, 1]` reads 2.1), with its children in it when
/// `nested`. A row listed under another reads its item as "2.1" and gives `each`, and leaves out
/// its components and properties (the output #1888 set). Its numbering flags are given when true;
/// provisional only in a `numbered` view (in one never numbered every number is).
fn row_json(row: &Row, item: &[u32], mass: bool, numbered: bool, nested: bool) -> Value {
    let mut v = json!({"part_number": row.part_number, "description": row.description, "revision": row.revision,
        "bom_type": row.bom_type.as_str(), "count": row.count, "definition": row.names.join("; "), "material": row.materials.join("; ")});
    if item.len() <= 1 {
        v["item"] = json!(row.item);
        v["components"] = json!(row.components);
        v["properties"] = json!(row.properties);
    } else {
        v["item"] = json!(bom::item_text(item));
        v["each"] = json!(row.each);
    }
    if mass {
        v["mass_g"] = grams(row.mass);
    }
    match row.quantity {
        Quantity::Count => {}
        Quantity::Pieces(n) => (v["quantity"], v["quantity_unit"]) = (json!(n), json!("pieces")),
        Quantity::Length(mm) => (v["quantity"], v["quantity_unit"]) = (json!(mm), json!("mm")),
        Quantity::Unknown(why) => {
            v["quantity"] = Value::Null;
            v["quantity_unknown"] = json!(match why {
                Uncounted::Mixed => "mixed",
                Uncounted::Invalid => "invalid",
            });
        }
    }
    for (flag, on) in [("provisional", numbered && row.provisional), ("locked", row.locked), ("duplicate", row.duplicate)] {
        if on {
            v[flag] = json!(true);
        }
    }
    if !row.statics.is_empty() {
        v["static"] = json!(row.statics.iter().map(|s| s.key()).collect::<Vec<_>>());
    }
    if nested && !row.children.is_empty() {
        let mut under = item.to_vec();
        under.push(0);
        v["children"] = row
            .children
            .iter()
            .map(|c| {
                if let Some(last) = under.last_mut() {
                    *last = c.item;
                }
                row_json(c, &under, mass, numbered, true)
            })
            .collect();
    }
    v
}

/// A CSV field, always quoted.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The bill of materials as CSV: fixed columns, every field quoted, each row followed by the rows
/// listed under it. A column added later goes at the end, so the others keep their places.
fn csv(list: &Bom) -> String {
    let mut out = ["item", "part_number", "description", "revision", "count", "definition", "material", "quantity"].map(quote).join(",");
    out.push('\n');
    for (item, r) in list.rows_deep() {
        let (item, count, definition, material) = (bom::item_text(&item), r.count.to_string(), r.names.join("; "), r.materials.join("; "));
        let quantity = match r.quantity {
            Quantity::Pieces(n) => n.to_string(),
            Quantity::Length(mm) => format!("{mm} mm"),
            Quantity::Count | Quantity::Unknown(_) => String::new(),
        };
        let fields: [&str; 8] = [&item, &r.part_number, &r.description, &r.revision, &count, &definition, &material, &quantity];
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
    let rows = rows_json(&list, request.view, request.mass);
    let mut out = Map::new();
    out.insert("component".into(), json!(request.source));
    out.insert("levels".into(), json!(request.view.as_str()));
    out.insert("group_by".into(), json!([request.group.as_str()]));
    out.insert("group_children".into(), json!(request.fold));
    out.insert("show_purchased_contents".into(), json!(request.purchased_contents));
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
            "json" => {
                let mut file = json!({"rows": rows, "total_occurrences": list.total.occurrences});
                if request.mass {
                    file["total_mass_g"] = grams(list.total.mass);
                }
                serde_json::to_string_pretty(&file).map_err(|e| bad(cmd, e.to_string()))?
            }
            other => return Err(bad(cmd, format!("unknown format `{other}` (csv or json)"))),
        };
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        out.insert("path".into(), json!(path));
        out.insert("format".into(), json!(format));
    }
    out.insert("rows".into(), Value::Array(rows));
    out.insert("total_occurrences".into(), json!(list.total.occurrences));
    if request.mass {
        out.insert("total_mass_g".into(), grams(list.total.mass));
    }
    out.insert("numbered".into(), json!(list.numbered));
    Ok(Value::Object(out))
}

/// A whole number from 1 to [`MAX_ITEM`] (`default` when not given).
fn item_number(p: &Value, k: &str, default: Option<u32>, cmd: &str) -> Result<Option<u32>> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=MAX_ITEM).contains(n))
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("`{k}` is a whole number from 1 to {MAX_ITEM}"))),
    }
}

/// The view numbered as `how` says, its numbers saved (only a real change touches the design), and
/// the view as `assembly.bom` gives it.
fn renumbered(s: &mut Session, p: &Value, how: Numbering, cmd: &str) -> Result<(Value, Bom)> {
    let request = Request { mass: false, ..request(s, p, cmd)? };
    let state = s.model.state();
    let numbers = bom::number(&mut Tree::new(&s.doc, &state), &request, how).map_err(|e| bad(cmd, e.to_string()))?;
    if let Some(numbers) = numbers {
        let saved = &mut s.doc_mut().assembly.item_numbers;
        match saved.iter_mut().find(|n| n.source == numbers.source && n.view == numbers.view) {
            Some(old) => *old = numbers,
            None => saved.push(numbers),
        }
    }
    let list = bom::list(&mut Tree::new(&s.doc, &state), &request).map_err(|e| bad(cmd, e.to_string()))?;
    let reply = json!({"component": request.source, "levels": request.view.as_str(), "numbered": list.numbered,
        "rows": rows_json(&list, request.view, false), "total_occurrences": list.total.occurrences});
    Ok((reply, list))
}

fn renumber(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "assembly.bom.renumber";
    let start = item_number(p, "start", Some(1), cmd)?.unwrap_or(1);
    let step = item_number(p, "step", Some(1), cmd)?.unwrap_or(1);
    let order = match p.get("order") {
        None | Some(Value::Null) => Order::List,
        Some(Value::String(o)) if o == "list" => Order::List,
        Some(Value::String(o)) if o == "item" => Order::Item,
        Some(_) => return Err(bad(cmd, "`order` is list or item")),
    };
    Ok(renumbered(s, p, Numbering::Renumber { start, step, order }, cmd)?.0)
}

fn item_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "assembly.bom.item.set";
    let shown = || bad(cmd, "`row` is the row's item as the view shows it: \"3\", or \"1.2\" under row 1");
    let row: Vec<u32> = match p.get("row") {
        Some(Value::Number(n)) => vec![n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(shown)?],
        Some(Value::String(text)) if text.len() <= 1024 => {
            text.split('.').map(|n| n.trim().parse::<u32>().map_err(|_| shown())).collect::<Result<_>>()?
        }
        _ => return Err(shown()),
    };
    let item = item_number(p, "item", None, cmd)?;
    let locked = match p.get("locked") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(bad(cmd, "`locked` is true or false")),
    };
    if item.is_none() && locked.is_none() {
        return Err(bad(cmd, "give `item`, `locked` or both"));
    }
    let (mut reply, list) = renumbered(s, p, Numbering::Set { row: row.clone(), item, locked }, cmd)?;
    // The rows listed with it: under the row above it (the top level for a row of its own).
    let above = row.split_last().map_or(&[][..], |(_, above)| above);
    let mut level: &[Row] = &list.rows;
    for &i in above {
        level = level.iter().find(|r| r.item == i).map_or(&[][..], |r| r.children.as_slice());
    }
    let mut duplicates: Vec<u32> = level.iter().filter(|r| r.duplicate).map(|r| r.item).collect();
    duplicates.dedup();
    reply["duplicates"] = json!(duplicates);
    Ok(reply)
}

/// After a change, each numbered view's new rows get their numbers in the design ([`bom::record`]),
/// so they are saved in the change's undo step. A design with no view numbered lists nothing.
/// Whether the design changed.
pub(crate) fn record_item_numbers(s: &mut Session) -> bool {
    if s.doc.assembly.item_numbers.is_empty() {
        return false;
    }
    let state = s.model.state();
    let Some(numbers) = bom::record(&mut Tree::new(&s.doc, &state)) else { return false };
    s.doc_mut().assembly.item_numbers = numbers;
    true
}

#[cfg(test)]
#[path = "bom_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
