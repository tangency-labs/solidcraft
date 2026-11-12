//! The open CAM job: the one job file (docs/cam-jobs.md) a session edits through `cam.job.*`
//! (`cmd/cam_job_edit.rs`), its own undo history, and what was last read from or written to its
//! file (to notice edits made outside SolidCraft).
//!
//! The job is kept as the file's JSON value, never as the parsed job: a key left out stays left
//! out (a default), a key given is an override, and saving writes back exactly the keys there
//! are, in a fixed order ([`write_job`]) with the file's own indent, so a job an agent wrote and
//! a person edited reads the same in git. Nothing here is part of the design or its history.

use std::hash::{Hash, Hasher};

use serde_json::{Map, Value};

/// Most steps kept in a job's undo history.
pub const MAX_HISTORY: usize = 200;
/// The indent a job file is written with when it has none of its own (a new job, one line).
pub const DEFAULT_INDENT: usize = 2;

/// One step of the job's history: its label and the job as it was before.
#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub before: Value,
}

/// What was last read from or written to the job's file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Disk {
    /// The file's modification time and size (`None` where the platform keeps no times).
    pub stamp: Option<(std::time::SystemTime, u64)>,
    /// A hash of its bytes.
    pub hash: u64,
}

/// Whether the job's file still holds what was last read or written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outside {
    /// Unchanged (or the job has no file yet).
    None,
    /// Changed by something else.
    Changed,
    /// Deleted or moved.
    Gone,
}

impl Outside {
    pub fn key(self) -> &'static str {
        match self {
            Outside::None => "none",
            Outside::Changed => "changed",
            Outside::Gone => "gone",
        }
    }
}

/// The open job (see the module notes).
#[derive(Clone, Debug)]
pub struct OpenJob {
    /// The job file; `None` until a new job of an unsaved design is saved somewhere.
    pub path: Option<String>,
    /// The job: the file's JSON object.
    pub value: Value,
    /// The job as last read or written; `None` when it never was.
    saved: Option<Value>,
    /// The file as last read or written.
    pub disk: Option<Disk>,
    /// The file's indent (spaces).
    pub indent: usize,
    pub undo: Vec<Step>,
    pub redo: Vec<Step>,
    /// Counts up with every change of `value`.
    pub revision: u64,
}

impl OpenJob {
    /// A job just read from (or about to be written to) `path`.
    pub fn new(path: Option<String>, value: Value, saved: bool, disk: Option<Disk>, indent: usize) -> OpenJob {
        OpenJob { path, saved: saved.then(|| value.clone()), value, disk, indent, undo: Vec::new(), redo: Vec::new(), revision: 1 }
    }

    /// A copy for a scratch session: the job, without the history.
    pub fn scratch(&self) -> OpenJob {
        OpenJob { undo: Vec::new(), redo: Vec::new(), ..self.clone() }
    }

    /// Whether the job differs from its file (or was never saved).
    pub fn dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.value)
    }

    /// The folder relative paths of the job are read from.
    pub fn folder(&self) -> Option<std::path::PathBuf> {
        self.path.as_deref().and_then(|p| std::path::Path::new(p).parent()).map(std::path::Path::to_path_buf)
    }

    /// Keep `next` as the job, one undo step labelled `label`.
    pub fn change(&mut self, label: impl Into<String>, next: Value) {
        let before = std::mem::replace(&mut self.value, next);
        self.undo.push(Step { label: label.into(), before });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
    }

    /// Undo the last step; its label.
    pub fn undo(&mut self) -> Option<String> {
        let step = self.undo.pop()?;
        let now = std::mem::replace(&mut self.value, step.before);
        self.redo.push(Step { label: step.label.clone(), before: now });
        self.revision += 1;
        Some(step.label)
    }

    /// Redo the last step undone; its label.
    pub fn redo(&mut self) -> Option<String> {
        let step = self.redo.pop()?;
        let now = std::mem::replace(&mut self.value, step.before);
        self.undo.push(Step { label: step.label.clone(), before: now });
        self.revision += 1;
        Some(step.label)
    }

    /// The job was written to (or read from) its file: `bytes` are what the file holds now.
    pub fn mark_saved(&mut self, bytes: &[u8]) {
        self.saved = Some(self.value.clone());
        self.disk = Some(Disk { stamp: self.path.as_deref().and_then(solidcraft_io::vfs::stamp), hash: hash_bytes(bytes) });
    }

    /// Whether the file still holds what was last read or written. Natively only its time and
    /// size are looked at while they match; the bytes are read and hashed when they moved, and
    /// always where the platform keeps no times (the web).
    pub fn outside(&mut self) -> Outside {
        let (Some(path), Some(disk)) = (self.path.as_deref(), self.disk.as_mut()) else { return Outside::None };
        let stamp = solidcraft_io::vfs::stamp(path);
        if stamp.is_some() && stamp == disk.stamp {
            return Outside::None;
        }
        match solidcraft_io::vfs::read(path, MAX_FILE_BYTES) {
            Ok(bytes) if hash_bytes(&bytes) == disk.hash => {
                // Touched but the same: remember the new time, so it is not read again.
                disk.stamp = stamp;
                Outside::None
            }
            Ok(_) => Outside::Changed,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Outside::Gone,
            Err(_) => {
                if solidcraft_io::vfs::exists(path) {
                    Outside::Changed
                } else {
                    Outside::Gone
                }
            }
        }
    }

    /// The job's operations.
    pub fn operations(&self) -> &[Value] {
        self.value.get("operations").and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }
}

/// Largest job file read (bytes).
pub const MAX_FILE_BYTES: usize = 16 << 20;

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Whether an operation's JSON is turned on (`enabled` absent or true), and for an indexed
/// one the operation inside it too.
pub fn op_enabled(op: &Value) -> bool {
    let on = |v: &Value| v.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    on(op) && op.get("operation").is_none_or(on)
}

/// An operation's name: its own, or its inner operation's (`indexed`, `indexed_pattern`).
pub fn op_name(op: &Value) -> String {
    let own = op.get("name").and_then(Value::as_str).filter(|n| !n.is_empty());
    let inner = op.get("operation").and_then(|o| o.get("name")).and_then(Value::as_str);
    own.or(inner).unwrap_or_default().to_string()
}

/// The indent of a JSON file: the spaces before its second line's first key (1 to 8), or the
/// default for a file on one line or indented with tabs.
pub fn detect_indent(text: &str) -> usize {
    text.lines()
        .skip(1)
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .filter(|n| (1..=8).contains(n))
        .unwrap_or(DEFAULT_INDENT)
}

/// Keys of a job, in the order they are written.
const JOB_KEYS: &[&str] = &[
    "name",
    "profile_path",
    "machine",
    "machine_path",
    "a_reversed",
    "a_limits_deg",
    "post",
    "post_path",
    "work_offset",
    "origin",
    "origin_from",
    "orientation",
    "bodies",
    "fixtures",
    "tolerance",
    "material",
    "stock",
    "breakthrough",
    "tools",
    "tool_library_path",
    "tool_library_paths",
    "tool_library",
    "default_heights",
    "default_feeds",
    "machine_rates",
    "machine_rates_path",
    "machine_model_path",
    "workholding_path",
    "workholding",
    "tool_change",
    "operations",
];
/// Keys of an operation that come first, in this order.
const OP_FIRST: &[&str] = &[
    "name",
    "type",
    "enabled",
    "tool",
    "a_deg",
    "count",
    "start_deg",
    "picks",
    "sketch",
    "profiles",
    "curve",
    "polygon",
    "points",
    "holes",
    "centers",
    "feature",
    "features",
    "heights",
    "stepdown",
    "stepover",
];
/// Keys of an operation that come last, in this order; the rest go between, by name.
const OP_LAST: &[&str] = &["spindle_rpm", "feeds", "coolant", "gouge_check", "operation"];
const HEIGHTS_KEYS: &[&str] = &["clearance", "retract", "top", "bottom"];
const FEEDS_KEYS: &[&str] = &["cut", "plunge", "descent"];
const STOCK_KEYS: &[&str] = &["box", "cylinder", "around_part", "simulated"];
const BOX_KEYS: &[&str] = &["min", "max"];
const CYLINDER_KEYS: &[&str] = &["radius", "x_min", "x_max"];
const AROUND_KEYS: &[&str] = &["margin", "top", "bottom"];
const BOUNDARY_KEYS: &[&str] = &["rect", "polygon"];
const TOOL_KEYS: &[&str] = &["number", "kind", "diameter", "description"];

/// Where in a job an object is, for its key order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Job,
    Op,
    Heights,
    Feeds,
    Stock,
    Box,
    Cylinder,
    Around,
    Boundary,
    Tool,
    Other,
}

fn child(place: Place, key: &str) -> Place {
    match (place, key) {
        (Place::Job, "operations") => Place::Op,
        (Place::Job, "stock") => Place::Stock,
        (Place::Job, "tools") => Place::Tool,
        (Place::Op, "operation") => Place::Op,
        (Place::Op, "heights") => Place::Heights,
        (Place::Op, "feeds") => Place::Feeds,
        (Place::Op, "boundary") => Place::Boundary,
        (Place::Stock, "box") => Place::Box,
        (Place::Stock, "cylinder") => Place::Cylinder,
        (Place::Stock, "around_part") => Place::Around,
        _ => Place::Other,
    }
}

/// `map`'s keys in the order they are written at `place`.
fn ordered(map: &Map<String, Value>, place: Place) -> Vec<&String> {
    let (first, last): (&[&str], &[&str]) = match place {
        Place::Job => (JOB_KEYS, &[]),
        Place::Op => (OP_FIRST, OP_LAST),
        Place::Heights => (HEIGHTS_KEYS, &[]),
        Place::Feeds => (FEEDS_KEYS, &[]),
        Place::Stock => (STOCK_KEYS, &[]),
        Place::Box => (BOX_KEYS, &[]),
        Place::Cylinder => (CYLINDER_KEYS, &[]),
        Place::Around => (AROUND_KEYS, &[]),
        Place::Boundary => (BOUNDARY_KEYS, &[]),
        Place::Tool => (TOOL_KEYS, &[]),
        Place::Other => (&[], &[]),
    };
    let rank = |k: &str| {
        if let Some(i) = first.iter().position(|f| *f == k) {
            (0, i)
        } else if let Some(i) = last.iter().position(|f| *f == k) {
            (2, i)
        } else {
            (1, 0)
        }
    };
    // `Map` iterates by name, so keys of the same rank stay in name order (a stable sort).
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|k| rank(k));
    keys
}

/// The job as its file holds it: `indent` spaces a level, every key in the job's order (keys
/// it does not order, by name), a final newline. A job written by this and read back is written
/// the same, byte for byte.
pub fn write_job(value: &Value, indent: usize) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Place::Job, 0, indent.clamp(1, 8));
    out.push('\n');
    out
}

fn write_value(out: &mut String, value: &Value, place: Place, depth: usize, indent: usize) {
    let pad = |out: &mut String, depth: usize| out.extend(std::iter::repeat_n(' ', depth * indent));
    match value {
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            out.push_str("{\n");
            let keys = ordered(map, place);
            for (i, key) in keys.iter().enumerate() {
                pad(out, depth + 1);
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push_str(": ");
                if let Some(v) = map.get(*key) {
                    write_value(out, v, child(place, key), depth + 1, indent);
                }
                if i + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, depth);
            out.push('}');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(out, depth + 1);
                write_value(out, item, place, depth + 1, indent);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, depth);
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// The deepest nesting of `value` (a scalar: 0), counted no further than `limit`.
pub fn depth(value: &Value, limit: usize) -> usize {
    if limit == 0 {
        return 0;
    }
    match value {
        Value::Object(m) => 1 + m.values().map(|v| depth(v, limit - 1)).max().unwrap_or(0),
        Value::Array(a) => 1 + a.iter().map(|v| depth(v, limit - 1)).max().unwrap_or(0),
        _ => 0,
    }
}

/// A short account of how job `new` differs from `old`, one line per change: job keys set,
/// changed or removed, operations added, removed or moved (by name), and an operation's keys
/// changed ("operation 3 'side pocket': stepdown 1.5 -> 2").
pub fn changes(old: &Value, new: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let empty = Map::new();
    let (a, b) = (old.as_object().unwrap_or(&empty), new.as_object().unwrap_or(&empty));
    key_changes(a, b, "", &["operations"], &mut out);
    let ops = |v: &Value| v.get("operations").and_then(Value::as_array).cloned().unwrap_or_default();
    let (oa, ob) = (ops(old), ops(new));
    let names_a: Vec<String> = oa.iter().map(op_name).collect();
    let names_b: Vec<String> = ob.iter().map(op_name).collect();
    for (i, op) in ob.iter().enumerate() {
        let name = &names_b[i];
        match names_a.iter().position(|n| n == name) {
            None => out.push(format!("added operation {} '{name}'", i + 1)),
            Some(j) => {
                if j != i && names_a.get(i) != Some(name) {
                    out.push(format!("moved operation '{name}' from {} to {}", j + 1, i + 1));
                }
                if let (Some(x), Some(y)) = (oa[j].as_object(), op.as_object()) {
                    key_changes(x, y, &format!("operation {} '{name}': ", i + 1), &[], &mut out);
                }
            }
        }
    }
    for (j, name) in names_a.iter().enumerate() {
        if !names_b.contains(name) {
            out.push(format!("removed operation {} '{name}'", j + 1));
        }
    }
    out
}

fn key_changes(a: &Map<String, Value>, b: &Map<String, Value>, prefix: &str, skip: &[&str], out: &mut Vec<String>) {
    let short = |v: &Value| {
        let text = v.to_string();
        if text.chars().count() > 40 { format!("{}…", text.chars().take(40).collect::<String>()) } else { text }
    };
    for (k, v) in b {
        if skip.contains(&k.as_str()) {
            continue;
        }
        match a.get(k) {
            None => out.push(format!("{prefix}{k} set to {}", short(v))),
            Some(old) if old != v => out.push(format!("{prefix}{k} {} -> {}", short(old), short(v))),
            _ => {}
        }
    }
    for k in a.keys() {
        if !skip.contains(&k.as_str()) && !b.contains_key(k) {
            out.push(format!("{prefix}{k} removed"));
        }
    }
}

#[cfg(test)]
#[path = "cam_job_state_tests.rs"]
mod tests;
