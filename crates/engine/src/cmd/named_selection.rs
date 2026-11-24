//! Named selections (#18716): a selection kept in the design under a name, so many items (the
//! parts of an imported board, the tiles of a floor) can be selected again in one click and then
//! hidden, coloured or moved together. Kept in the design like named views: saved, and each
//! change is an undo step; applying one only changes the selection.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::named_selection::{
    SelectionApplied, SelectionApplyParams, SelectionDeleteParams, SelectionDeleted, SelectionListParams, SelectionListResult, SelectionRenameParams,
    SelectionRenamed, SelectionSaveParams, SelectionSaved, SelectionUpdateParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Sel, Session};

const SAVE_CONTRACT: TypedContract<SelectionSaveParams> = TypedContract::new();
const APPLY_CONTRACT: TypedContract<SelectionApplyParams> = TypedContract::new();
const UPDATE_CONTRACT: TypedContract<SelectionUpdateParams> = TypedContract::new();
const RENAME_CONTRACT: TypedContract<SelectionRenameParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<SelectionDeleteParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<SelectionListParams> = TypedContract::new();

/// The most named selections a design keeps, and the most items one holds.
const MAX_SELECTIONS: usize = 1000;
const MAX_ITEMS: usize = 10_000;
/// The longest name, in characters, once surrounding spaces are cut.
const MAX_NAME_CHARS: usize = 128;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("named_selection.save", "Save Named Selection", crate::checked!("named_selection.save", save))
        .icon("select")
        .contract(&SAVE_CONTRACT)
        .output(output_schema_for::<SelectionSaved>)
        .params("name?; items?: [as select.set takes them] (default: the current selection) → {selection, count}"),
    CommandSpec::new("named_selection.apply", "Select Named Selection", crate::checked!("named_selection.apply", apply))
        .icon("select")
        .noundo()
        .contract(&APPLY_CONTRACT)
        .output(output_schema_for::<SelectionApplied>)
        .params("selection: name → {selected, skipped}: its items become the selection; items that no longer exist are skipped"),
    CommandSpec::new("named_selection.update", "Update Named Selection", crate::checked!("named_selection.update", update))
        .icon("select")
        .contract(&UPDATE_CONTRACT)
        .output(output_schema_for::<SelectionSaved>)
        .params("selection: name; items?: [as select.set takes them] (default: the current selection)"),
    CommandSpec::new("named_selection.rename", "Rename Named Selection", crate::checked!("named_selection.rename", rename))
        .icon("rename")
        .contract(&RENAME_CONTRACT)
        .output(output_schema_for::<SelectionRenamed>)
        .params("selection: name, name"),
    CommandSpec::new("named_selection.delete", "Delete Named Selection", crate::checked!("named_selection.delete", delete))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<SelectionDeleted>)
        .params("selection: name (its items stay in the design)"),
    CommandSpec::new("named_selection.list", "List Named Selections", crate::checked!("named_selection.list", list))
        .icon("list")
        .noundo()
        .read_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<SelectionListResult>)
        .doc_only(),
];

fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// The items a save or update keeps: those given, or the current selection's model items
/// (placements, bodies, faces, edges and features; sketch geometry and drawing items are left
/// out, as they only mean something while their sketch or drawing is open).
fn items_of(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Value>> {
    let items: Vec<Value> = match p.get("items") {
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(bad(cmd, "`items` must be a list")),
        None => s
            .selection
            .iter()
            .filter(|x| matches!(x, Sel::Occurrence { .. } | Sel::Body { .. } | Sel::Face { .. } | Sel::Edge { .. } | Sel::Feature { .. }))
            .filter_map(|x| serde_json::to_value(x).ok())
            .collect(),
    };
    if items.is_empty() {
        return Err(bad(cmd, "nothing to keep: select something first, or give `items`"));
    }
    if items.len() > MAX_ITEMS {
        return Err(bad(cmd, format!("a named selection holds at most {MAX_ITEMS} items")));
    }
    let items = items.into_iter().map(|it| completed(s, it, cmd)).collect::<Result<Vec<_>>>()?;
    Ok(with_references(s, items))
}

/// A face or edge given by only its index or only a point, completed as `select.set` would pick
/// it, so it can be named below; anything else as given.
fn completed(s: &Session, it: Value, cmd: &str) -> Result<Value> {
    let partial = matches!(it.get("type").and_then(Value::as_str), Some("face" | "edge")) && (it.get("index").is_none() || it.get("point").is_none());
    if !partial {
        return Ok(it);
    }
    let sel = super::edit::selection_item(s, it).map_err(|e| bad(cmd, e.to_string()))?;
    serde_json::to_value(sel).map_err(|e| bad(cmd, e.to_string()))
}

/// Each body, face and edge kept with the feature that made its body (`made_by`), so applying
/// does not select another body given the name after this one was deleted; each face and edge
/// of a solid also keeps its persistent name (what made it, see `naming`), so applying finds the
/// same face after an edit renumbers the body's faces rather than whichever face now has the
/// saved index.
fn with_references(s: &Session, items: Vec<Value>) -> Vec<Value> {
    let Ok(state) = s.world_state() else { return items };
    items
        .into_iter()
        .map(|mut it| {
            let kind = it.get("type").and_then(Value::as_str).unwrap_or_default();
            let body_key = match kind {
                "body" => "name",
                "face" | "edge" => "body",
                _ => return it,
            };
            let Some(b) = it.get(body_key).and_then(Value::as_str).and_then(|b| state.body(b)) else { return it };
            let index = it.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
            let name = match (kind, index) {
                _ if b.body.is_mesh() || it.get("point").is_none() || it.get("name").is_some() => None,
                ("face", Some(i)) => solidcraft_doc::naming::face_names(b).get(i).cloned(),
                ("edge", Some(i)) => solidcraft_doc::naming::edge_names(b).get(i).cloned(),
                _ => None,
            };
            if let Some(o) = it.as_object_mut() {
                o.insert("made_by".into(), Value::from(b.feature));
                if let Some(name) = name {
                    o.insert("name".into(), Value::String(name));
                }
            }
            it
        })
        .collect()
}

/// The item to select for a saved one (`None`: it is gone). A body, face or edge whose body name
/// now belongs to a body another feature made is gone, as is one marked `gone` (another body was
/// renamed to its name after it went, see `Document::rename_body_keys`). A named face or edge is found again by
/// its name near its saved point; one of a solid with no name has nothing to be found by, and is
/// gone too. A face or edge of a mesh body, and anything else, is selected as saved.
fn found(state: &solidcraft_doc::ModelState, mut it: Value) -> Option<Value> {
    let kind = it.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
    let body_key = match kind.as_str() {
        "body" => "name",
        "face" | "edge" => "body",
        _ => return Some(it),
    };
    if it.get("gone").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let made_by = it.as_object_mut().and_then(|o| o.remove("made_by"));
    let body_name = it.get(body_key).and_then(Value::as_str)?.to_string();
    let b = state.body(&body_name)?;
    if made_by.is_some_and(|f| f.as_u64() != Some(b.feature)) {
        return None;
    }
    if kind == "body" {
        return Some(it);
    }
    let Some(name) = it.get("name").and_then(Value::as_str) else {
        return b.body.is_mesh().then_some(it);
    };
    let near: solidcore_record::Vec3 = serde_json::from_value(it.get("point")?.clone()).ok()?;
    if kind == "face" {
        let (index, point) = solidcraft_doc::naming::face_on_body(b, name, near)?;
        Some(json!({"type": "face", "body": body_name, "index": index, "point": point}))
    } else {
        let point = solidcraft_doc::naming::find_edge(b, name, near).ok()?;
        Some(json!({"type": "edge", "body": body_name, "point": point}))
    }
}

/// What the items are, in words: "3 components, 2 bodies, 1 face".
fn summary(s: &Session, items: &[Value]) -> String {
    // components, bodies, faces, edges, sketches, features, other
    let mut n = [0usize; 7];
    for it in items {
        let k = match it.get("type").and_then(Value::as_str) {
            Some("occurrence") => 0,
            Some("body") => 1,
            Some("face") => 2,
            Some("edge") => 3,
            Some("feature")
                if it
                    .get("id")
                    .and_then(Value::as_u64)
                    .and_then(|id| s.doc.feature(id))
                    .is_some_and(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })) =>
            {
                4
            }
            Some("feature") => 5,
            _ => 6,
        };
        n[k] += 1;
    }
    let words = [
        ("component", "components"),
        ("body", "bodies"),
        ("face", "faces"),
        ("edge", "edges"),
        ("sketch", "sketches"),
        ("feature", "features"),
        ("item", "items"),
    ];
    n.iter()
        .zip(words)
        .filter(|(c, _)| **c > 0)
        .map(|(c, (one, many))| format!("{c} {}", if *c == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The default name for the `k`th selection holding `what`.
fn default_name(k: usize, what: &str) -> String {
    format!("Selection{k} – {what}")
}

/// The number in a name in the default form ("Selection3 – 4 faces").
fn default_number(name: &str) -> Option<usize> {
    let (head, _) = name.split_once(" – ")?;
    head.strip_prefix("Selection")?.parse().ok()
}

fn index_of(s: &Session, name: &str, cmd: &str) -> Result<usize> {
    s.doc.named_selections.iter().position(|x| x.name == name).ok_or_else(|| bad(cmd, format!("no named selection `{name}`")))
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "named_selection.save";
    let items = items_of(s, p, cmd)?;
    if s.doc.named_selections.len() >= MAX_SELECTIONS {
        return Err(bad(cmd, "too many named selections"));
    }
    let taken = |n: &str| s.doc.named_selections.iter().any(|x| x.name == n);
    let (name, generated) = match p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.chars().count() > MAX_NAME_CHARS => return Err(bad(cmd, "`name` is too long")),
        Some(n) if taken(n) => return Err(bad(cmd, format!("another named selection is called `{n}`"))),
        Some(n) => (n.to_string(), false),
        None => {
            let what = summary(s, &items);
            ((s.doc.named_selections.len() + 1..).map(|k| default_name(k, &what)).find(|n| !taken(n)).unwrap_or_default(), true)
        }
    };
    let count = items.len();
    s.doc_mut().named_selections.push(solidcraft_doc::NamedSelection { name: name.clone(), items, generated });
    reply(cmd, SelectionSaved { selection: name, count, param_warnings: Vec::new() })
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "named_selection.apply";
    let name = p.get("selection").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`selection` must be a name"))?;
    let items = s.doc.named_selections.get(index_of(s, name, cmd)?).map(|x| x.items.clone()).unwrap_or_default();
    s.selection.clear();
    // One item at a time, so an item that is gone (a deleted body or placement, a face an edit
    // removed) is skipped and the rest are still selected.
    let state = s.world_state()?;
    let mut skipped = 0;
    for it in items {
        let picked = found(&state, it).map(|it| super::edit::select_set(s, &json!({"items": [it], "add": true})));
        if !matches!(picked, Some(Ok(_))) {
            skipped += 1;
        }
    }
    s.refresh_display();
    reply(cmd, SelectionApplied { selection: name.to_string(), selected: s.selection.len(), skipped, param_warnings: Vec::new() })
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "named_selection.update";
    let name = p.get("selection").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`selection` must be a name"))?.to_string();
    let i = index_of(s, &name, cmd)?;
    let items = items_of(s, p, cmd)?;
    let count = items.len();
    // A name SolidCraft made says what the selection holds, so it follows the update (keeping
    // its number) unless another selection already has the new name. A name the user typed
    // stays, even one in the same shape ("Selection7 – 2 faces").
    let generated = s.doc.named_selections.get(i).is_some_and(|x| x.generated);
    let renamed = default_number(&name)
        .filter(|_| generated)
        .map(|k| default_name(k, &summary(s, &items)))
        .filter(|n| !s.doc.named_selections.iter().enumerate().any(|(j, x)| j != i && x.name == *n));
    let name = renamed.unwrap_or(name);
    if let Some(x) = s.doc_mut().named_selections.get_mut(i) {
        x.items = items;
        x.name = name.clone();
    }
    reply(cmd, SelectionSaved { selection: name, count, param_warnings: Vec::new() })
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "named_selection.rename";
    let old = p.get("selection").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`selection` must be a name"))?;
    let i = index_of(s, old, cmd)?;
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS)
        .ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?
        .to_string();
    if s.doc.named_selections.iter().enumerate().any(|(j, x)| j != i && x.name == name) {
        return Err(bad(cmd, format!("another named selection is called `{name}`")));
    }
    if let Some(x) = s.doc_mut().named_selections.get_mut(i) {
        x.name = name.clone();
        x.generated = false;
    }
    reply(cmd, SelectionRenamed { selection: name, param_warnings: Vec::new() })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "named_selection.delete";
    let name = p.get("selection").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`selection` must be a name"))?;
    let i = index_of(s, name, cmd)?;
    let gone = s.doc_mut().named_selections.remove(i);
    reply(cmd, SelectionDeleted { deleted: gone.name, param_warnings: Vec::new() })
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    reply("named_selection.list", SelectionListResult { selections: s.doc.named_selections.clone(), param_warnings: Vec::new() })
}

#[cfg(test)]
#[path = "named_selection_tests.rs"]
mod tests;
