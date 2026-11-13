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
//! saved ([`restore`]), as they came. What keeps them is the shared `solidcraft_format::newer`; what
//! is a design's (its features kept as placeholders, its parameters left out, the sections a design
//! can't open without) is here. Saving keeps the file's own format number and change list:
//! the file still holds what the newer build wrote, and a lower number would make that build
//! upgrade its own data as if it were old. What can't be put back because the design changed around
//! it ([`not_kept`]) is said in the reply to the save.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use solidcraft_format::newer::{leave_out_unreadable, looks_default, read_around, short};

use crate::format::{self, DESIGN, Newer};
use crate::{DocError, Document, Result};

pub use solidcraft_format::newer::NewerFile;

/// The app, as a newer file's notice names it.
const APP: &str = "SolidCraft";
/// How a [`RawFeature`] is written while it is inside this build; [`restore`] unwraps it.
const FOREIGN_TAG: &str = "newer_file_feature";
/// Sections a design can't open without: filled in when the file's can't be read.
const CORE: [&str; 3] = ["name", "params", "features"];
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

/// What a design `newer` says a newer build saved holds that this build doesn't read: nothing
/// left out of it yet (a design container's reader adds what it leaves out).
pub fn newer_file(newer: &Newer) -> NewerFile {
    NewerFile::new(APP, newer)
}

/// Read a design a newer build saved, or say why it truly can't be read.
pub(crate) fn read(mut raw: Value, version: u32, applied: Option<Vec<String>>, newer: Newer) -> Result<Document> {
    let applied_names = applied.as_deref();
    let mut file = newer_file(&newer);
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
    file.compare(&upgraded, &known, FOREIGN_TAG, &CONTAINER_ONLY);
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
                let note = format!("parameter `{name}` ({})", short(&why.to_string(), "design"));
                file.keep_element("params", param.clone(), note);
                false
            }
        });
    }
    // Any other section, by itself.
    leave_out_unreadable(top, file, "design", piece);
    if !top.get("name").is_some_and(Value::is_string) {
        top.insert("name".into(), json!("Untitled"));
    }
    for key in ["params", "features"] {
        top.entry(key).or_insert_with(|| json!([]));
    }
    read_around(top, file, &CORE, "design", |top: &Map<String, Value>| format::decode(Value::Object(top.clone()), version, applied))
}

fn feature_note(feature: &Value, why: &str) -> String {
    let name = feature.get("name").and_then(Value::as_str).unwrap_or("unnamed");
    let kind = feature.get("type").and_then(Value::as_str).unwrap_or("unnamed");
    let why = short(why, "design");
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

/// Write back what the design holds of the file a newer build saved, into `value`, the design's
/// JSON. Returns what couldn't go back ([`not_kept`]).
pub fn restore(doc: &Document, value: &mut Value) -> Vec<String> {
    let Some(file) = &doc.newer_file else { return Vec::new() };
    if let Some(Value::Array(features)) = value.get_mut("features") {
        for feature in features.iter_mut().filter(|f| f.get("type").and_then(Value::as_str) == Some(FOREIGN_TAG)) {
            unwrapped(feature);
        }
    }
    file.restore_into(value)
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
