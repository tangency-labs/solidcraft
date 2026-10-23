//! Save history stored in a design file.
//!
//! `format` (`solidcraft/N`) is the schema version the migrations understand. This object is
//! the file's own record of that number and of how many times it has been saved:
//! `format_version` is the `N` the file was written with, `revision` counts saves by a build
//! that records history (0 until the first), and `entries` is that record, oldest first.
//!
//! A `saved` entry can carry a `snapshot`: the parametric document (features, parameters,
//! sketches, assembly) as of that save, without this history object and without an evaluated
//! boundary representation. Restore reads that snapshot. Keeping snapshots is opt-in
//! ([`RevisionHistoryOptions`] defaults to off). When it is on, the newest
//! [`SnapshotLimits::DEFAULT_VERSIONS`] revisions keep one, and the bytes those snapshots add
//! stay within [`SnapshotLimits::DEFAULT_BYTES`] (128 MiB). A preference can lower either cap
//! and cannot raise them past that ceiling. STEP text, mesh arrays and canvas images that are
//! already in the live document, or in a newer snapshot that was kept, count as a short digest:
//! a container stores each distinct payload once by hash (see `docs/design-container.md`). A
//! payload that is new counts in full, so a changed import cannot grow the file without bound.
//! A plain JSON save then drops the oldest snapshot bodies once their real encoded size passes
//! the same cap. An entry whose snapshot was dropped keeps its revision, format and action;
//! restoring it is an error.
//!
//! A file from before the field existed has none of it. Opening one records revision 0 and a
//! single `opened` entry at the format number the file carried, with no snapshot. Geometry is
//! not changed. Unknown keys inside the object are ignored, so a later build can add history
//! fields without this build refusing a file whose schema version it already understands.
//!
//! This is not the design property `metadata.revision` (a part-number text such as `"A"`), and
//! it is not the timeline or the undo history.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most entries kept. Older ones are dropped on open and on save. History is informational;
/// dropping it still opens the design.
pub const MAX_ENTRIES: usize = 64;
/// Default number of newest revisions that keep a model snapshot. A preference can keep fewer,
/// down to one, or more, up to [`MAX_ENTRIES`]. Older entries keep their revision, format and
/// action, and `snapshot` is absent.
pub const MAX_SNAPSHOTS: usize = 16;
/// Hard ceiling on snapshot payload bytes: 128 MiB. This is the default size cap and the most a
/// preference or a hostile file is allowed to keep. Sixteen revisions at the previous 8 MiB
/// parametric budget is the same figure.
pub const MAX_SNAPSHOT_BYTES: usize = 128 << 20;
/// Largest format number or save count accepted in this object. A larger value is treated as
/// a damaged history and replaced with the migration record, and the design still opens.
const MAX_FORMAT: u32 = 1_000_000;
const MAX_REVISION: u64 = 1_000_000_000;
const MAX_ACTION: usize = 32;
/// A string this long is a payload the container will hash, not parametric JSON.
const HEAVY_TEXT: usize = 1024;
/// An array this long is mesh data the container will hash.
const HEAVY_ARRAY: usize = 64;

/// How many snapshots a save may keep, and how many bytes they may add.
///
/// [`Self::clamp`] refuses a version count above [`MAX_ENTRIES`] or a byte count above
/// [`MAX_SNAPSHOT_BYTES`]. Opening a file uses [`Self::ceiling`], so a preference that asks for
/// more versions than the default does not lose them on the next open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotLimits {
    /// Newest revisions that keep a model. Zero keeps none.
    pub max_versions: usize,
    /// Snapshot payload bytes kept, newest first.
    pub max_bytes: usize,
}

impl SnapshotLimits {
    pub const DEFAULT_VERSIONS: usize = MAX_SNAPSHOTS;
    pub const DEFAULT_BYTES: usize = MAX_SNAPSHOT_BYTES;

    /// The preference defaults: 16 revisions and 128 MiB.
    pub fn standard() -> Self {
        Self { max_versions: Self::DEFAULT_VERSIONS, max_bytes: Self::DEFAULT_BYTES }
    }

    /// The most a file may hold when it is opened, before a tighter preference is applied on save.
    pub fn ceiling() -> Self {
        Self { max_versions: MAX_ENTRIES, max_bytes: Self::DEFAULT_BYTES }
    }

    pub fn clamp(self) -> Self {
        Self { max_versions: self.max_versions.min(MAX_ENTRIES), max_bytes: self.max_bytes.min(MAX_SNAPSHOT_BYTES) }
    }
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self::standard()
    }
}

/// Whether saves keep model snapshots, and the caps applied when history is pruned.
///
/// Off by default. A save still records the revision; it stores a model only when `enabled`.
/// The caps are enforced on every save, so a file cannot keep more than the configured
/// revisions or bytes even after the preference is turned off. Neither cap can exceed
/// [`SnapshotLimits::ceiling`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionHistoryOptions {
    pub enabled: bool,
    pub max_bytes: usize,
    pub max_versions: usize,
}

impl Default for RevisionHistoryOptions {
    fn default() -> Self {
        Self { enabled: false, max_bytes: SnapshotLimits::DEFAULT_BYTES, max_versions: SnapshotLimits::DEFAULT_VERSIONS }
    }
}

impl RevisionHistoryOptions {
    pub fn limits(self) -> SnapshotLimits {
        SnapshotLimits { max_versions: self.max_versions, max_bytes: self.max_bytes }.clamp()
    }
}

/// One recorded open or save.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentRevision {
    pub revision: u64,
    pub format_version: u32,
    /// `opened` for a file that had no history; `saved` for a save. Other short tokens are
    /// kept as a later build wrote them.
    #[serde(default = "saved_action", skip_serializing_if = "is_saved_action")]
    pub action: String,
    /// Parametric document as of this revision, without `document_history`. Absent for an
    /// `opened` entry, a save from before snapshots, or a revision pruned past the caps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Value>,
}

fn saved_action() -> String {
    "saved".into()
}

fn is_saved_action(action: &String) -> bool {
    action == "saved"
}

/// The file's format number and save history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentHistory {
    /// `solidcraft/N` number this file was saved as. After an older file is opened this stays
    /// the number the file carried, until the next save writes this build's number.
    pub format_version: u32,
    /// Saves recorded by a build that writes this object. 0 means it has not been saved by one.
    pub revision: u64,
    /// Oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<DocumentRevision>,
}

impl Default for DocumentHistory {
    fn default() -> Self {
        Self::fresh()
    }
}

impl DocumentHistory {
    /// A design created by this build and not saved yet.
    pub fn fresh() -> Self {
        Self { format_version: crate::format::FORMAT, revision: 0, entries: Vec::new() }
    }

    /// An older file that did not carry a history. `origin` is the `solidcraft/N` it was saved as.
    pub fn migrated(origin: u32) -> Self {
        Self {
            format_version: origin,
            revision: 0,
            entries: vec![DocumentRevision { revision: 0, format_version: origin, action: "opened".into(), snapshot: None }],
        }
    }

    /// The history to keep for a file saved as format `origin`. A missing or damaged value
    /// becomes [`Self::migrated`]; a readable one is kept, with the oldest entries dropped past
    /// [`MAX_ENTRIES`]. `shared` is the live document's heavy payloads (see [`shared_payloads`]),
    /// which do not count again toward the byte ceiling.
    pub fn adopt(value: Option<&Value>, origin: u32, shared: &HashSet<u64>) -> Self {
        let Some(value) = value else {
            return Self::migrated(origin);
        };
        let Ok(mut history) = serde_json::from_value::<Self>(value.clone()) else {
            return Self::migrated(origin);
        };
        if !history.fields_in_range() {
            return Self::migrated(origin);
        }
        if history.entries.len() > MAX_ENTRIES {
            let extra = history.entries.len() - MAX_ENTRIES;
            history.entries.drain(..extra);
        }
        history.prune_snapshots(SnapshotLimits::ceiling(), shared);
        history
    }

    /// Record one save by this build, with no model snapshot. The count saturates at
    /// [`MAX_REVISION`] instead of wrapping. Existing snapshots are pruned to the default caps.
    pub fn record_save(&mut self) {
        self.record_save_with(None, SnapshotLimits::standard());
    }

    /// Record one save and attach `snapshot` (the parametric document, from [`Self::model_snapshot`]).
    /// `limits` drop the oldest snapshot bodies; this entry's revision, format and action stay.
    pub fn record_save_with(&mut self, snapshot: Option<Value>, limits: SnapshotLimits) {
        self.record_save_shared(snapshot, limits, &HashSet::new());
    }

    /// [`Self::record_save_with`] with the live document's heavy payloads already paid for.
    pub fn record_save_shared(&mut self, snapshot: Option<Value>, limits: SnapshotLimits, shared: &HashSet<u64>) {
        self.revision = self.revision.saturating_add(1).min(MAX_REVISION);
        self.format_version = crate::format::FORMAT;
        self.entries.push(DocumentRevision { revision: self.revision, format_version: self.format_version, action: saved_action(), snapshot });
        if self.entries.len() > MAX_ENTRIES {
            let extra = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..extra);
        }
        self.prune_snapshots(limits, shared);
    }

    /// The parametric document `doc` is right now, without this history object. `None` when it
    /// cannot be encoded or the parametric JSON it would add is already over `max_bytes` (the
    /// save still counts; that revision cannot be restored). Heavy payloads already in `doc` are
    /// not charged: the file stores them once.
    pub fn model_snapshot(doc: &crate::Document, max_bytes: usize) -> Option<Value> {
        let mut value = serde_json::to_value(doc).ok()?;
        if !value.is_object() {
            return None;
        }
        scrub_snapshot(&mut value);
        let max_bytes = max_bytes.min(MAX_SNAPSHOT_BYTES);
        let mut seen = shared_payloads(&value);
        (snapshot_cost(&value, &mut seen) <= max_bytes).then_some(value)
    }

    /// Drop snapshot bodies past `limits`, oldest first. `shared` payloads are already in the
    /// file and count as a digest. A snapshot that is not a JSON object is dropped. Nested
    /// `document_history` is removed so a snapshot does not contain another copy of every snapshot.
    /// A body that is dropped does not mark its payloads as stored.
    pub fn prune_snapshots(&mut self, limits: SnapshotLimits, shared: &HashSet<u64>) {
        let limits = limits.clamp();
        let mut seen = shared.clone();
        let mut kept = 0usize;
        let mut bytes = 0usize;
        for entry in self.entries.iter_mut().rev() {
            let Some(snapshot) = entry.snapshot.as_mut() else { continue };
            if !snapshot.is_object() {
                entry.snapshot = None;
                continue;
            }
            scrub_snapshot(snapshot);
            let mut trial = seen.clone();
            let weight = snapshot_cost(snapshot, &mut trial);
            if limits.max_versions == 0 || kept >= limits.max_versions || weight > limits.max_bytes || bytes.saturating_add(weight) > limits.max_bytes
            {
                entry.snapshot = None;
                continue;
            }
            seen = trial;
            kept += 1;
            bytes = bytes.saturating_add(weight);
        }
    }

    /// Plain JSON has no blob store, so count the encoded snapshot. Drop the oldest bodies once
    /// the total passes `max_bytes` (and never past [`MAX_SNAPSHOT_BYTES`]). Container saves do
    /// not call this: identical STEP, mesh and canvas bytes are stored once in the package.
    pub fn prune_inline_snapshots(&mut self, max_bytes: usize) {
        let max_bytes = max_bytes.min(MAX_SNAPSHOT_BYTES);
        let mut bytes = 0usize;
        for entry in self.entries.iter_mut().rev() {
            let Some(snapshot) = entry.snapshot.as_ref() else { continue };
            let weight = match serde_json::to_vec(snapshot) {
                Ok(buf) => buf.len(),
                Err(_) => usize::MAX,
            };
            if weight == usize::MAX || weight > max_bytes || bytes.saturating_add(weight) > max_bytes {
                entry.snapshot = None;
            } else {
                bytes = bytes.saturating_add(weight);
            }
        }
    }

    fn fields_in_range(&self) -> bool {
        self.format_version <= MAX_FORMAT
            && self.revision <= MAX_REVISION
            && self.entries.iter().all(|entry| {
                entry.format_version <= MAX_FORMAT
                    && entry.revision <= MAX_REVISION
                    && !entry.action.is_empty()
                    && entry.action.len() <= MAX_ACTION
                    && entry.action.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
    }
}

fn scrub_snapshot(snapshot: &mut Value) {
    if let Some(obj) = snapshot.as_object_mut() {
        obj.remove("document_history");
    }
}

/// Heavy payloads already stored with the live document: top-level features and canvases, not
/// the snapshots inside `document_history`.
pub fn shared_payloads(doc: &Value) -> HashSet<u64> {
    let mut seen = HashSet::new();
    let Some(obj) = doc.as_object() else { return seen };
    if let Some(features) = obj.get("features") {
        collect_heavy(features, &mut seen);
    }
    if let Some(canvases) = obj.get("canvases") {
        collect_heavy(canvases, &mut seen);
    }
    seen
}

fn collect_heavy(value: &Value, seen: &mut HashSet<u64>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_heavy(item, seen);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                if key == "document_history" {
                    continue;
                }
                if heavy_charge(key, child).is_some() {
                    seen.insert(payload_id(child));
                } else {
                    collect_heavy(child, seen);
                }
            }
        }
        _ => {}
    }
}

/// Bytes this value adds. A long `step` or canvas `data` string, and a long mesh `positions`
/// or `triangles` array, count in full the first time their bytes are seen and as a digest
/// after that.
fn snapshot_cost(value: &Value, seen: &mut HashSet<u64>) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(v) => {
            if *v {
                4
            } else {
                5
            }
        }
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => s.len().saturating_add(2),
        Value::Array(items) => items.iter().fold(2usize, |n, item| n.saturating_add(1).saturating_add(snapshot_cost(item, seen))),
        Value::Object(map) => {
            let mut n = 2usize;
            for (key, child) in map {
                n = n.saturating_add(key.len()).saturating_add(3);
                if let Some(real) = heavy_charge(key, child) {
                    if seen.insert(payload_id(child)) {
                        n = n.saturating_add(real);
                    } else {
                        n = n.saturating_add(64);
                    }
                } else {
                    n = n.saturating_add(snapshot_cost(child, seen));
                }
            }
            n
        }
    }
}

fn heavy_charge(key: &str, child: &Value) -> Option<usize> {
    match (key, child) {
        ("step" | "data", Value::String(s)) if s.len() > HEAVY_TEXT => Some(s.len()),
        ("positions" | "triangles", Value::Array(items)) if items.len() > HEAVY_ARRAY => Some(snapshot_weight(child)),
        _ => None,
    }
}

fn payload_id(value: &Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hash_json(&mut hasher, value);
    hasher.finish()
}

fn hash_json(hasher: &mut impl Hasher, value: &Value) {
    match value {
        Value::Null => 0u8.hash(hasher),
        Value::Bool(v) => {
            1u8.hash(hasher);
            v.hash(hasher);
        }
        Value::Number(n) => {
            2u8.hash(hasher);
            n.to_string().hash(hasher);
        }
        Value::String(s) => {
            3u8.hash(hasher);
            s.hash(hasher);
        }
        Value::Array(items) => {
            4u8.hash(hasher);
            items.len().hash(hasher);
            for item in items {
                hash_json(hasher, item);
            }
        }
        Value::Object(map) => {
            5u8.hash(hasher);
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                key.hash(hasher);
                if let Some(child) = map.get(key) {
                    hash_json(hasher, child);
                }
            }
        }
    }
}

/// Bytes of parametric JSON. A long `step` or canvas `data` string, and a long mesh
/// `positions` or `triangles` array, count as a digest: the container stores those bytes once.
fn snapshot_weight(value: &Value) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(v) => {
            if *v {
                4
            } else {
                5
            }
        }
        Value::Number(n) => n.to_string().len(),
        Value::String(s) => s.len().saturating_add(2),
        Value::Array(items) => items.iter().fold(2usize, |n, item| n.saturating_add(1).saturating_add(snapshot_weight(item))),
        Value::Object(map) => {
            let mut n = 2usize;
            for (key, child) in map {
                n = n.saturating_add(key.len()).saturating_add(3);
                let heavy = match (key.as_str(), child) {
                    ("step" | "data", Value::String(s)) if s.len() > HEAVY_TEXT => Some(64),
                    ("positions" | "triangles", Value::Array(items)) if items.len() > HEAVY_ARRAY => Some(64),
                    _ => None,
                };
                n = n.saturating_add(heavy.unwrap_or_else(|| snapshot_weight(child)));
            }
            n
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use crate::format::{FORMAT, format_string};
    use serde_json::json;

    fn minimal(version: u32, name: &str) -> Value {
        json!({
            "format": format!("solidcraft/{version}"),
            "name": name,
            "units": "mm",
            "params": [],
            "features": [],
            "next_id": 1
        })
    }

    #[test]
    fn a_new_file_round_trips_its_version_metadata() {
        let doc = Document::new("Bracket");
        assert_eq!(doc.document_history, DocumentHistory::fresh());
        assert_eq!(doc.document_history.format_version, FORMAT);
        assert_eq!(doc.document_history.revision, 0);
        assert!(doc.document_history.entries.is_empty());
        let text = doc.to_json();
        let raw: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw["format"], format_string(FORMAT));
        assert_eq!(raw["document_history"]["format_version"], json!(FORMAT));
        assert_eq!(raw["document_history"]["revision"], json!(0));
        assert!(raw["document_history"].get("entries").is_none(), "an unsaved design has no entries");
        let opened = Document::from_json(&text).unwrap();
        assert_eq!(opened.document_history, doc.document_history);
        assert_eq!(opened.format, format_string(FORMAT));
    }

    #[test]
    fn record_save_increments_and_round_trips() {
        let mut doc = Document::new("Bracket");
        doc.document_history.record_save();
        doc.document_history.record_save();
        assert_eq!(doc.document_history.revision, 2);
        assert_eq!(doc.document_history.format_version, FORMAT);
        assert_eq!(doc.document_history.entries.len(), 2);
        assert!(doc.document_history.entries.iter().all(|e| e.action == "saved" && e.format_version == FORMAT));
        assert_eq!(doc.document_history.entries[1].revision, 2);
        let opened = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(opened.document_history, doc.document_history);
    }

    #[test]
    fn an_older_file_without_history_opens_at_revision_zero() {
        let raw = minimal(15, "old");
        assert!(raw.get("document_history").is_none());
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.format, format_string(FORMAT));
        assert_eq!(opened.name, "old");
        assert_eq!(opened.document_history, DocumentHistory::migrated(15));
        let again = Document::from_json(&opened.to_json()).unwrap();
        assert_eq!(again.document_history, opened.document_history);
    }

    #[test]
    fn a_damaged_history_does_not_stop_an_older_file_opening() {
        let mut raw = minimal(9, "damaged");
        raw["document_history"] = json!("nope");
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.document_history, DocumentHistory::migrated(9));

        raw["document_history"] = json!({"format_version": "9", "revision": -1});
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.document_history, DocumentHistory::migrated(9));

        let mut entries = Vec::new();
        for i in 0..MAX_ENTRIES + 10 {
            entries.push(json!({"revision": i, "format_version": 9, "action": "saved"}));
        }
        raw["document_history"] = json!({"format_version": 9, "revision": 80, "entries": entries});
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.document_history.entries.len(), MAX_ENTRIES);
        assert_eq!(opened.document_history.entries.last().unwrap().revision, (MAX_ENTRIES + 9) as u64);
        assert_eq!(opened.name, "damaged");
    }

    #[test]
    fn unknown_history_fields_are_ignored_and_a_newer_schema_version_is_refused() {
        let mut doc = Document::new("Forward");
        doc.document_history.record_save();
        let mut raw: Value = serde_json::from_str(&doc.to_json()).unwrap();
        raw["document_history"]["future_field"] = json!(true);
        raw["document_history"]["entries"] = json!([{
            "revision": 1,
            "format_version": FORMAT,
            "action": "autosaved",
            "extra": {"kept_by_a_later_build": true}
        }]);
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.document_history.revision, 1);
        assert_eq!(opened.document_history.format_version, FORMAT);
        assert_eq!(opened.document_history.entries[0].action, "autosaved");
        let saved = serde_json::from_str::<Value>(&opened.to_json()).unwrap();
        assert!(saved["document_history"].get("future_field").is_none());
        assert!(saved["document_history"]["entries"][0].get("extra").is_none());

        raw["format"] = json!(format_string(FORMAT + 1));
        let err = Document::from_json(&raw.to_string()).unwrap_err().to_string();
        assert!(err.contains("newer SolidCraft"), "{err}");
    }

    #[test]
    fn saving_a_migrated_file_keeps_the_opened_entry_and_appends() {
        let mut doc = Document::from_json(&minimal(4, "then-saved").to_string()).unwrap();
        assert_eq!(doc.document_history, DocumentHistory::migrated(4));
        doc.document_history.record_save();
        assert_eq!(doc.document_history.revision, 1);
        assert_eq!(doc.document_history.format_version, FORMAT);
        assert_eq!(doc.document_history.entries[0].action, "opened");
        assert_eq!(doc.document_history.entries[0].format_version, 4);
        assert_eq!(doc.document_history.entries[1].action, "saved");
        assert_eq!(doc.document_history.entries[1].revision, 1);
        assert_eq!(Document::from_json(&doc.to_json()).unwrap().document_history, doc.document_history);
    }

    #[test]
    fn a_snapshot_round_trips_and_does_not_nest_history() {
        let mut doc = Document::new("Bracket");
        let snap = DocumentHistory::model_snapshot(&doc, SnapshotLimits::DEFAULT_BYTES).unwrap();
        assert!(snap.get("document_history").is_none());
        assert_eq!(snap["name"], "Bracket");
        doc.document_history.record_save_with(Some(snap), SnapshotLimits::standard());
        assert_eq!(doc.document_history.revision, 1);
        assert!(doc.document_history.entries[0].snapshot.as_ref().unwrap().get("document_history").is_none());
        let opened = Document::from_json(&doc.to_json()).unwrap();
        assert_eq!(opened.document_history, doc.document_history);
    }

    #[test]
    fn a_snapshot_metadata_number_outside_geometry_range_still_opens() {
        let mut doc = Document::new("Meta");
        let mut snap = DocumentHistory::model_snapshot(&doc, SnapshotLimits::DEFAULT_BYTES).unwrap();
        snap["metadata"] = json!({"foreign": 1.5e300});
        doc.document_history.record_save_with(Some(snap), SnapshotLimits::standard());
        let opened = Document::from_json(&doc.to_json()).unwrap();
        let kept = opened.document_history.entries[0].snapshot.as_ref().unwrap();
        assert_eq!(kept["metadata"]["foreign"], json!(1.5e300));
    }

    #[test]
    fn snapshots_keep_the_newest_and_a_damaged_payload_does_not_drop_the_count() {
        let mut history = DocumentHistory::fresh();
        for i in 0..MAX_SNAPSHOTS + 4 {
            history.record_save_with(Some(json!({"name": format!("rev-{i}"), "features": []})), SnapshotLimits::standard());
        }
        assert_eq!(history.entries.len(), MAX_SNAPSHOTS + 4);
        assert_eq!(history.revision, (MAX_SNAPSHOTS + 4) as u64);
        let with_model = history.entries.iter().filter(|e| e.snapshot.is_some()).count();
        assert_eq!(with_model, MAX_SNAPSHOTS);
        assert!(history.entries.first().unwrap().snapshot.is_none());
        assert!(history.entries.last().unwrap().snapshot.is_some());
        assert_eq!(history.entries.first().unwrap().revision, 1);

        let mut raw = minimal(9, "damaged");
        raw["document_history"] = json!({
            "format_version": 9,
            "revision": 2,
            "entries": [
                {"revision": 1, "format_version": 9, "action": "saved", "snapshot": "not-an-object"},
                {"revision": 2, "format_version": 9, "action": "saved", "snapshot": {"name": "kept", "document_history": {"revision": 99}}}
            ]
        });
        let opened = Document::from_json(&raw.to_string()).unwrap();
        assert_eq!(opened.document_history.revision, 2);
        assert!(opened.document_history.entries[0].snapshot.is_none());
        let kept = opened.document_history.entries[1].snapshot.as_ref().unwrap();
        assert_eq!(kept["name"], "kept");
        assert!(kept.get("document_history").is_none());
    }

    #[test]
    fn inline_prune_drops_the_oldest_snapshot_body_past_the_byte_cap() {
        let heavy = "x".repeat(HEAVY_TEXT + 32);
        let limits = SnapshotLimits { max_versions: 8, max_bytes: heavy.len() + 200 };
        let mut history = DocumentHistory::fresh();
        // The same `step` is stored once, so both revisions keep a body under the container cap.
        history.record_save_with(Some(json!({"step": heavy.clone()})), limits);
        history.record_save_with(Some(json!({"step": heavy.clone()})), limits);
        assert_eq!(history.entries.iter().filter(|e| e.snapshot.is_some()).count(), 2);
        history.prune_inline_snapshots(heavy.len() + 40);
        assert!(history.entries[0].snapshot.is_none(), "plain JSON counts the real STEP text");
        assert!(history.entries[1].snapshot.is_some());
        assert_eq!(history.revision, 2);

        let mut notes = DocumentHistory::fresh();
        let tight = SnapshotLimits { max_versions: 8, max_bytes: heavy.len() + 80 };
        notes.record_save_with(Some(json!({"note": heavy.clone()})), tight);
        notes.record_save_with(Some(json!({"note": heavy})), tight);
        assert!(notes.entries[0].snapshot.is_none(), "parametric JSON over the cap drops the older body");
        assert!(notes.entries[1].snapshot.is_some());
        assert_eq!(notes.revision, 2);
    }

    #[test]
    fn a_changed_import_counts_toward_the_byte_cap_and_a_repeat_does_not() {
        let first = "a".repeat(HEAVY_TEXT + 40);
        let second = "b".repeat(HEAVY_TEXT + 40);
        let one = SnapshotLimits { max_versions: 8, max_bytes: first.len() + 80 };
        let mut changed = DocumentHistory::fresh();
        changed.record_save_with(Some(json!({"step": first.clone()})), one);
        changed.record_save_with(Some(json!({"step": second})), one);
        assert!(changed.entries[0].snapshot.is_none(), "a different STEP is new bytes and drops the older body");
        assert!(changed.entries[1].snapshot.is_some());

        let both = SnapshotLimits { max_versions: 8, max_bytes: first.len() + 200 };
        let mut same = DocumentHistory::fresh();
        same.record_save_with(Some(json!({"step": first.clone()})), both);
        same.record_save_with(Some(json!({"step": first})), both);
        assert_eq!(same.entries.iter().filter(|e| e.snapshot.is_some()).count(), 2, "the same STEP is stored once");
    }

    #[test]
    fn a_live_import_is_not_charged_against_the_snapshot_cap() {
        let step = "s".repeat(HEAVY_TEXT + 80);
        let limits = SnapshotLimits { max_versions: 4, max_bytes: 400 };
        let body = json!({"features": [{"type": "import", "step": step}]});
        let mut charged = DocumentHistory::fresh();
        charged.record_save_with(Some(body.clone()), limits);
        assert!(charged.entries[0].snapshot.is_none(), "without the live document, the STEP is new bytes");
        let mut shared_with_live = DocumentHistory::fresh();
        let shared = shared_payloads(&body);
        shared_with_live.record_save_shared(Some(body), limits, &shared);
        assert!(shared_with_live.entries[0].snapshot.is_some(), "the file already stores this STEP");
    }

    #[test]
    fn history_options_default_off_with_the_128_mb_ceiling_and_16_versions() {
        let options = RevisionHistoryOptions::default();
        assert!(!options.enabled);
        assert_eq!(options.max_versions, 16);
        assert_eq!(options.max_bytes, 128 << 20);
        assert_eq!(options.limits(), SnapshotLimits::standard());
        let raised = RevisionHistoryOptions { enabled: true, max_bytes: usize::MAX, max_versions: 10_000 };
        assert_eq!(raised.limits().max_bytes, MAX_SNAPSHOT_BYTES);
        assert_eq!(raised.limits().max_versions, MAX_ENTRIES);
        assert!(DocumentHistory::model_snapshot(&Document::new("Bracket"), 8).is_none());
        assert!(DocumentHistory::model_snapshot(&Document::new("Bracket"), SnapshotLimits::DEFAULT_BYTES).is_some());
    }

    #[test]
    fn version_cap_drops_older_snapshots_when_pruning() {
        let mut history = DocumentHistory::fresh();
        let limits = SnapshotLimits { max_versions: 2, max_bytes: MAX_SNAPSHOT_BYTES };
        for i in 0..4 {
            history.record_save_with(Some(json!({"name": format!("rev-{i}")})), limits);
        }
        assert_eq!(history.revision, 4);
        assert_eq!(history.entries.iter().filter(|e| e.snapshot.is_some()).count(), 2);
        assert!(history.entries[0].snapshot.is_none());
        assert!(history.entries[1].snapshot.is_none());
        assert_eq!(history.entries[3].snapshot.as_ref().unwrap()["name"], "rev-3");
    }
}
