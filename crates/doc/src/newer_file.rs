//! A design a newer SolidCraft saved: it opens, with a warning, and saving it loses nothing.
//!
//! A newer format number, or a change the file names that this build doesn't have, says a newer
//! build saved the file. It may use nothing this build lacks, so it is read as usual. When part of
//! it can't be read, that part stays out of the way and the rest opens:
//! - a feature this build can't read keeps its place on the timeline as
//!   [`FeatureKind::NewerFileFeature`](crate::FeatureKind::NewerFileFeature), which reports an error when evaluated;
//! - a parameter it can't read is left out;
//! - any other top-level section it can't read is left out whole;
//! - a field it doesn't know, which typed reading would drop, is noticed by comparing the file
//!   with what was read.
//!
//! All of these are named in [`NewerFile::not_understood`] and go back into the file when it is
//! saved ([`restore`]), as they came. Saving keeps the file's own format number and change list:
//! the file still holds what the newer build wrote, and a lower number would make that build
//! upgrade its own data as if it were old. What can't be put back because the design changed around
//! it ([`not_kept`]) is said in the reply to the save.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::format::{self, DESIGN, Newer};
use crate::{DocError, Document, Result};

/// How a [`RawFeature`] is written while it is inside this build; [`restore`] unwraps it.
const FOREIGN_TAG: &str = "newer_file_feature";
/// Sections a design can't open without: filled in when the file's can't be read.
const CORE: [&str; 3] = ["name", "params", "features"];
/// The most sections left out one at a time to find the one a design can't be read with.
const MAX_SECTION_TRIES: usize = 64;
/// Fields a design container adds beside what a design holds; not unknown to this build.
const CONTAINER_ONLY: [&str; 6] = ["step_blob", "mesh_blob", "image_blob", "points_blob", "hdr_blob", "texture_blob"];

/// A feature as a newer build wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawFeature(pub Box<Value>);

impl RawFeature {
    /// The kind of feature it says it is.
    pub fn kind(&self) -> &str {
        self.0.get("type").and_then(Value::as_str).unwrap_or("unnamed")
    }

    /// Why it doesn't build here.
    pub fn why(&self) -> String {
        format!("saved by a newer SolidCraft: this version can't read a `{}` feature. Update SolidCraft to build it; saving keeps it", self.kind())
    }
}

/// What the file says and holds that this build doesn't read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewerFile {
    /// The number the file says.
    pub format: u32,
    /// The number this build reads up to.
    pub reads_up_to: u32,
    /// The changes the file names that this build doesn't have.
    pub unknown_changes: Vec<String>,
    /// What this build couldn't read, by name.
    pub not_understood: Vec<String>,
    /// Fields known records lost in typed reading.
    kept: Option<Kept>,
    /// Top-level sections left out whole.
    sections: Vec<Section>,
    /// Elements left out of a top-level list: its key and the element.
    elements: Vec<(String, Value)>,
    /// The parts of a design container this build has no use for: name and bytes. A blob only one
    /// of the kept pieces refers to is one of them.
    pub parts: Vec<(String, Arc<[u8]>)>,
    /// The file's `container.json`, when this build doesn't know what it says.
    pub container: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
struct Section {
    key: String,
    value: Value,
    /// This build's record of the section, without the file's: while it is unchanged, the file's
    /// goes back.
    empty: Option<Value>,
}

/// The fields of a record that typed reading dropped.
#[derive(Clone, Debug, PartialEq)]
enum Kept {
    Field(Value),
    Fields(BTreeMap<String, Kept>),
    Items(Vec<(Anchor, Kept)>),
}

/// Which element of a list some kept fields belong to.
#[derive(Clone, Debug, PartialEq)]
enum Anchor {
    Id(u64),
    Name(String),
    /// Its place, good only while the list keeps its length.
    Index {
        at: usize,
        of: usize,
    },
}

impl NewerFile {
    /// What to tell the user, whatever else is said.
    pub fn notice(&self) -> String {
        let reads = self.reads_up_to;
        let format = self.format;
        match self.unknown_changes.first() {
            Some(change) if format <= reads => {
                format!(
                    "Saved by a newer SolidCraft (format {format} with `{change}`, which this version doesn't know). If something's missing, update."
                )
            }
            _ => format!("Saved by a newer SolidCraft (format {format}, this version reads up to {reads}). If something's missing, update."),
        }
    }

    /// Every line to report: the notice, then what couldn't be read, by name.
    pub fn warnings(&self) -> Vec<String> {
        let unread = self.not_understood.iter().map(|what| format!("Not read: {what}. Saving keeps it."));
        std::iter::once(self.notice()).chain(unread).collect()
    }

    /// What a reply says about it.
    pub fn report(&self) -> Value {
        json!({
            "format": self.format,
            "reads_up_to": self.reads_up_to,
            "unknown_changes": self.unknown_changes,
            "not_understood": self.not_understood,
            "notice": self.notice(),
        })
    }

    /// Whether the next save needs a container to keep what the file holds: plain JSON has no room
    /// for parts beside the design.
    pub fn needs_container(&self) -> bool {
        !self.parts.is_empty()
    }

    /// Whether anything kept back names this text (a blob's name in a container).
    pub fn mentions(&self, text: &str) -> bool {
        fn value(v: &Value, text: &str) -> bool {
            match v {
                Value::String(s) => s == text,
                Value::Array(a) => a.iter().any(|x| value(x, text)),
                Value::Object(o) => o.values().any(|x| value(x, text)),
                _ => false,
            }
        }
        fn kept(k: &Kept, text: &str) -> bool {
            match k {
                Kept::Field(v) => value(v, text),
                Kept::Fields(m) => m.values().any(|x| kept(x, text)),
                Kept::Items(l) => l.iter().any(|(_, x)| kept(x, text)),
            }
        }
        self.kept.as_ref().is_some_and(|k| kept(k, text))
            || self.sections.iter().any(|s| value(&s.value, text))
            || self.elements.iter().any(|(_, v)| value(v, text))
    }

    /// Say that a section the container reader couldn't finish reading is left out.
    /// `empty` is what the design writes for the section while it holds nothing, if anything.
    pub fn keep_section(&mut self, key: &str, value: Value, empty: Option<Value>, why: &str) {
        self.not_understood.push(format!("section `{key}` ({why})"));
        self.sections.push(Section { key: key.into(), value, empty });
    }
}

/// Read a design a newer build saved, or say why it truly can't be read.
pub(crate) fn read(mut raw: Value, version: u32, applied: Option<Vec<String>>, newer: Newer) -> Result<Document> {
    let applied_names = applied.as_deref();
    let mut file =
        NewerFile { format: newer.format, reads_up_to: newer.reads_up_to, unknown_changes: newer.unknown_changes.clone(), ..NewerFile::default() };
    let (mut doc, read_as) = match format::decode(raw.clone(), version, applied_names) {
        Ok(doc) => (doc, raw),
        Err(_) => {
            let mut foreign = BTreeMap::new();
            let mut doc = salvage(&mut raw, version, applied_names, &mut file, &mut foreign).map_err(|why| {
                DocError::Invalid(format!(
                    "this design was saved by a newer SolidCraft (format {}, this version reads up to {}) and this version can't read it: {why}. Update SolidCraft to open it",
                    newer.format, newer.reads_up_to
                ))
            })?;
            fill_foreign(&mut doc, foreign);
            (doc, raw)
        }
    };
    let mut upgraded = read_as;
    // Compared with what was read, the file's own shape after the same upgrade.
    DESIGN.upgrade(&mut upgraded, version, applied_names)?;
    let known = serde_json::to_value(&doc).map_err(|e| DocError::Invalid(format!("design file: {e}")))?;
    let mut paths = Vec::new();
    file.kept = vanished(&upgraded, &known, "", &mut paths);
    file.not_understood.extend(paths);
    for section in &mut file.sections {
        section.empty = known.get(&section.key).cloned();
    }
    if newer.format > newer.reads_up_to {
        // The file's own number and list: it still holds what that build wrote.
        doc.format = DESIGN.format_string(newer.format);
        doc.format_pending = applied.unwrap_or_default();
    } else {
        // Upgraded to this build's format; the changes it doesn't know are still in it.
        doc.format_pending.extend(newer.unknown_changes.iter().cloned());
    }
    doc.newer_file = Some(Arc::new(file));
    Ok(doc)
}

/// Read the design around what can't be read, which moves to `file`. `raw` ends as what was read.
fn salvage(
    raw: &mut Value,
    version: u32,
    applied: Option<&[String]>,
    file: &mut NewerFile,
    foreign: &mut BTreeMap<u64, Value>,
) -> std::result::Result<Document, String> {
    let piece = |key: &str, value: &Value| {
        let mut stub = json!({"name": "", "params": [], "features": []});
        stub[key] = value.clone();
        format::check_piece(stub, version, applied)
    };
    let Some(top) = raw.as_object_mut() else { return Err("expected a JSON object".into()) };
    // A feature it can't read keeps its place; a parameter it can't read stays out.
    if let Some(Value::Array(features)) = top.get_mut("features") {
        let mut free = features.iter().filter_map(|f| f.get("id").and_then(Value::as_u64)).max().unwrap_or(0).saturating_add(1);
        for feature in features.iter_mut() {
            if let Err(why) = piece("features", &json!([feature])) {
                file.not_understood.push(feature_note(feature, &why.to_string()));
                *feature = wrapped(std::mem::take(feature), &mut free, foreign);
            }
        }
    }
    if let Some(Value::Array(params)) = top.get_mut("params") {
        params.retain(|param| match piece("params", &json!([param])) {
            Ok(()) => true,
            Err(why) => {
                let name = param.get("name").and_then(Value::as_str).unwrap_or("unnamed");
                file.not_understood.push(format!("parameter `{name}` ({})", short(&why.to_string())));
                file.elements.push(("params".into(), param.clone()));
                false
            }
        });
    }
    // Any other section, by itself.
    let keys: Vec<String> = top.keys().cloned().collect();
    for key in &keys {
        let value = top.get(key).cloned().unwrap_or_default();
        if let Err(why) = piece(key, &value) {
            move_out(top, key, &why.to_string(), file);
        }
    }
    if !top.get("name").is_some_and(Value::is_string) {
        top.insert("name".into(), json!("Untitled"));
    }
    for key in ["params", "features"] {
        top.entry(key).or_insert_with(|| json!([]));
    }
    let attempt = |top: &Map<String, Value>| format::decode(Value::Object(top.clone()), version, applied);
    let first = match attempt(top) {
        Ok(doc) => return Ok(doc),
        Err(why) => why,
    };
    // Sections that are fine by themselves but not together with the rest.
    let keys: Vec<String> = top.keys().filter(|k| !CORE.contains(&k.as_str())).cloned().collect();
    // Each try reads the whole design: a file of many sections goes straight to the last resort.
    for key in keys.iter().filter(|_| keys.len() <= MAX_SECTION_TRIES) {
        let mut without = top.clone();
        without.remove(key);
        if let Ok(doc) = attempt(&without) {
            move_out(top, key, &first.to_string(), file);
            return Ok(doc);
        }
    }
    let mut bare = top.clone();
    bare.retain(|key, _| CORE.contains(&key.as_str()));
    match attempt(&bare) {
        Ok(doc) => {
            for key in &keys {
                move_out(top, key, &first.to_string(), file);
            }
            Ok(doc)
        }
        Err(_) => Err(first.to_string()),
    }
}

/// Leave a section out and keep it. A section the design needs is left empty.
fn move_out(top: &mut Map<String, Value>, key: &str, why: &str, file: &mut NewerFile) {
    let Some(value) = top.remove(key) else { return };
    file.not_understood.push(format!("section `{key}` ({})", short(why)));
    file.sections.push(Section { key: key.into(), value, empty: None });
}

/// The first line of a reader's error, without the list of every kind it knows.
fn short(why: &str) -> &str {
    let why = why.split(", expected").next().unwrap_or(why);
    why.strip_prefix("design file: ").unwrap_or(why)
}

fn feature_note(feature: &Value, why: &str) -> String {
    let name = feature.get("name").and_then(Value::as_str).unwrap_or("unnamed");
    let kind = feature.get("type").and_then(Value::as_str).unwrap_or("unnamed");
    let why = short(why);
    if why.starts_with("unknown variant") {
        format!("feature `{name}`, a `{kind}` feature this version doesn't know")
    } else {
        format!("feature `{name}`, a `{kind}` feature ({why})")
    }
}

/// A feature this build can't read as this build reads it: the fields every feature has, which stay
/// editable. Its `raw` is filled in after reading ([`fill_foreign`]), so nothing in a feature this
/// build can't read is judged by this build's rules. `free` gives an id to one the file left
/// without.
fn wrapped(raw: Value, free: &mut u64, foreign: &mut BTreeMap<u64, Value>) -> Value {
    let id = raw.get("id").and_then(Value::as_u64).unwrap_or_else(|| {
        let id = *free;
        *free = free.saturating_add(1);
        id
    });
    let name = raw.get("name").and_then(Value::as_str).unwrap_or("Unknown feature").to_string();
    let suppressed = raw.get("suppressed").and_then(Value::as_bool).unwrap_or(false);
    let component = raw.get("component").and_then(Value::as_u64).unwrap_or(0);
    foreign.insert(id, raw);
    json!({"id": id, "name": name, "suppressed": suppressed, "component": component, "type": FOREIGN_TAG, "raw": null})
}

/// Give the features [`wrapped`] left without one what the file said.
fn fill_foreign(doc: &mut Document, mut foreign: BTreeMap<u64, Value>) {
    for feature in &mut doc.features {
        if let crate::FeatureKind::NewerFileFeature { raw } = &mut feature.kind
            && let Some(value) = foreign.remove(&feature.id)
        {
            *raw.0 = value;
        }
    }
}

/// A feature back as the file had it, with the changes this build can make to any feature.
fn unwrapped(feature: &mut Value) {
    let Some(wrapper) = feature.as_object_mut() else { return };
    let Some(Value::Object(mut raw)) = wrapper.remove("raw") else { return };
    for key in ["id", "name", "suppressed", "component"] {
        match wrapper.get(key) {
            // Left out as a writer leaves it out, unless the file said it.
            Some(v) if key != "id" && !raw.contains_key(key) && looks_default(v) => {}
            Some(v) => {
                raw.insert(key.into(), v.clone());
            }
            None => {
                raw.remove(key);
            }
        }
    }
    *feature = Value::Object(raw);
}

/// A value a writer leaves out without losing anything: not worth telling anyone about.
fn looks_default(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// What `raw` has that `known`, what was read of it, doesn't. `paths` gets those fields' names.
fn vanished(raw: &Value, known: &Value, path: &str, paths: &mut Vec<String>) -> Option<Kept> {
    match (raw, known) {
        (Value::Object(raw), Value::Object(known)) => {
            if known.get("type").and_then(Value::as_str) == Some(FOREIGN_TAG) {
                return None;
            }
            let mut kept = BTreeMap::new();
            for (key, value) in raw {
                if CONTAINER_ONLY.contains(&key.as_str()) || (path.is_empty() && matches!(key.as_str(), "format" | "format_pending")) {
                    continue;
                }
                let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match known.get(key) {
                    None => {
                        if !looks_default(value) {
                            paths.push(format!("field `{inner}`"));
                        }
                        kept.insert(key.clone(), Kept::Field(value.clone()));
                    }
                    Some(known) => {
                        if let Some(k) = vanished(value, known, &inner, paths) {
                            kept.insert(key.clone(), k);
                        }
                    }
                }
            }
            (!kept.is_empty()).then_some(Kept::Fields(kept))
        }
        (Value::Array(raw), Value::Array(known)) => {
            let mut items = Vec::new();
            for (at, element) in raw.iter().enumerate() {
                let anchor = anchor_of(element).unwrap_or(Anchor::Index { at, of: raw.len() });
                let Some(partner) = find(known, &anchor) else { continue };
                let label = match (element.get("name").and_then(Value::as_str), &anchor) {
                    (Some(name), _) => format!("{path}[{name}]"),
                    (None, Anchor::Id(id)) => format!("{path}[{id}]"),
                    _ => format!("{path}[{at}]"),
                };
                if let Some(k) = vanished(element, partner, &label, paths) {
                    items.push((anchor, k));
                }
            }
            (!items.is_empty()).then_some(Kept::Items(items))
        }
        _ => None,
    }
}

fn anchor_of(element: &Value) -> Option<Anchor> {
    if let Some(id) = element.get("id").and_then(Value::as_u64) {
        return Some(Anchor::Id(id));
    }
    element.get("name").and_then(Value::as_str).map(|name| Anchor::Name(name.to_string()))
}

fn find<'a>(list: &'a [Value], anchor: &Anchor) -> Option<&'a Value> {
    match anchor {
        Anchor::Id(id) => list.iter().find(|x| x.get("id").and_then(Value::as_u64) == Some(*id)),
        Anchor::Name(name) => list.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(name)),
        Anchor::Index { at, of } => (list.len() == *of).then(|| list.get(*at)).flatten(),
    }
}

fn find_mut<'a>(list: &'a mut [Value], anchor: &Anchor) -> Option<&'a mut Value> {
    match anchor {
        Anchor::Id(id) => list.iter_mut().find(|x| x.get("id").and_then(Value::as_u64) == Some(*id)),
        Anchor::Name(name) => list.iter_mut().find(|x| x.get("name").and_then(Value::as_str) == Some(name)),
        Anchor::Index { at, of } => (list.len() == *of).then(|| list.get_mut(*at)).flatten(),
    }
}

/// Put the kept fields back. An element that is gone was deleted here, its fields with it; one
/// whose place can't be told any more, and a field whose record is gone, go into `lost`.
fn apply(kept: &Kept, value: &mut Value, path: &str, lost: &mut Vec<String>) {
    match kept {
        Kept::Field(_) => {}
        Kept::Fields(fields) => {
            let Some(object) = value.as_object_mut() else {
                lost.push(format!("field `{path}`"));
                return;
            };
            for (key, sub) in fields {
                let inner = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match (sub, object.get_mut(key)) {
                    (Kept::Field(v), None) => {
                        object.insert(key.clone(), v.clone());
                    }
                    (Kept::Field(_), Some(_)) => {}
                    (sub, Some(inner_value)) => apply(sub, inner_value, &inner, lost),
                    // The list it was in is empty and not written: its elements were deleted.
                    (Kept::Items(_), None) => {}
                    (_, None) => lost.push(format!("field `{inner}`")),
                }
            }
        }
        Kept::Items(items) => {
            let Some(list) = value.as_array_mut() else {
                lost.push(format!("field `{path}`"));
                return;
            };
            for (anchor, sub) in items {
                match (find_mut(list, anchor), anchor) {
                    (Some(element), _) => apply(sub, element, path, lost),
                    (None, Anchor::Index { at, .. }) => lost.push(format!("element {at} of `{path}`")),
                    (None, _) => {}
                }
            }
        }
    }
}

/// Write back what the design holds of the file a newer build saved, into `value`, the design's
/// JSON. Returns what couldn't go back ([`not_kept`]).
pub fn restore(doc: &Document, value: &mut Value) -> Vec<String> {
    let mut lost = Vec::new();
    let Some(file) = &doc.newer_file else { return lost };
    if let Some(Value::Array(features)) = value.get_mut("features") {
        for feature in features.iter_mut().filter(|f| f.get("type").and_then(Value::as_str) == Some(FOREIGN_TAG)) {
            unwrapped(feature);
        }
    }
    if let Some(kept) = &file.kept {
        apply(kept, value, "", &mut lost);
    }
    if let Some(top) = value.as_object_mut() {
        for section in &file.sections {
            if top.get(&section.key) == section.empty.as_ref() {
                top.insert(section.key.clone(), section.value.clone());
            } else {
                lost.push(format!("section `{}`, replaced by what was edited here", section.key));
            }
        }
        for (key, element) in &file.elements {
            let name = element.get("name");
            match top.entry(key.clone()).or_insert_with(|| json!([])) {
                Value::Array(list) if name.is_none() || !list.iter().any(|x| x.get("name") == name) => list.push(element.clone()),
                _ => lost.push(format!("`{key}` element {}, replaced by what was made here", name.and_then(Value::as_str).unwrap_or("unnamed"))),
            }
        }
    }
    lost
}

/// What of the file a newer build saved the next save can't keep, because the design was changed
/// around it. Empty almost always.
pub fn not_kept(doc: &Document) -> Vec<String> {
    let Ok(mut value) = serde_json::to_value(doc) else { return Vec::new() };
    restore(doc, &mut value)
}

#[cfg(test)]
#[path = "newer_file_tests.rs"]
mod tests;
