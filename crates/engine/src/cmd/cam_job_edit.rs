//! The open CAM job (a child of `cmd/cam.rs`; state in `crate::cam_job_state`): `cam.job.new`,
//! `open`, `save`, `reload`, `close`, `get`, `edit`, `undo` and `redo`.
//!
//! A session keeps at most one job open. Its edits are its own history, never the design's
//! (`.noundo()`): every successful edit is one step, and every edit is read with the job's
//! schema before it is kept, so a person cannot make a job an agent cannot run. A command that
//! would drop unsaved edits refuses without `discard`; one that would write over a file changed
//! outside SolidCraft refuses without `overwrite`. None opens a dialog.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::{CommandSpec, MAX_OPERATIONS, parse};
use crate::cam_job_state::{self, Disk, OpenJob, Outside, op_enabled, op_name};
use crate::params::bad;
use crate::schema::cam::CamJob;
use crate::schema::cam_job_edit::{
    CamJobCloseParams, CamJobCloseReply, CamJobEditParams, CamJobGetParams, CamJobGetReply, CamJobHistoryParams, CamJobHistoryReply, CamJobNewParams,
    CamJobOpenParams, CamJobReloadParams, CamJobReloadReply, CamJobSaveParams, CamJobSaveReply, JobSummary, OpSummary,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const NEW: TypedContract<CamJobNewParams> = TypedContract::new();
const OPEN: TypedContract<CamJobOpenParams> = TypedContract::new();
const SAVE: TypedContract<CamJobSaveParams> = TypedContract::new();
const RELOAD: TypedContract<CamJobReloadParams> = TypedContract::new();
const CLOSE: TypedContract<CamJobCloseParams> = TypedContract::new();
const GET: TypedContract<CamJobGetParams> = TypedContract::new();
const EDIT: TypedContract<CamJobEditParams> = TypedContract::new();
const HISTORY: TypedContract<CamJobHistoryParams> = TypedContract::new();

/// Longest name of a job (characters).
const MAX_NAME: usize = 200;
/// Deepest nesting of a job's JSON.
const MAX_DEPTH: usize = 32;
/// Most keys one edit sets or unsets.
const MAX_KEYS: usize = 200;
/// Most setups of one design whose job files are named after it (`.op2` to `.op99`).
const MAX_SETUPS: usize = 99;
/// The refusal of a command that needs an open job.
const NO_JOB: &str = "open or create a job first (cam.job.open, cam.job.new)";

macro_rules! job_cmd {
    ($id:literal, $label:literal, $run:path, $contract:expr, $reply:ty, $doc:literal) => {
        CommandSpec::new($id, $label, crate::checked!($id, $run))
            .noundo()
            .icon("cam")
            .doc_only()
            .contract($contract)
            .output(output_schema_for::<$reply>)
            .params($doc)
    };
}

pub(crate) static COMMANDS: &[CommandSpec] = &[
    job_cmd!(
        "cam.job.new",
        "New Machining Job",
        new_job,
        &NEW,
        JobSummary,
        "path?: .json (default <design>.cam.json beside the saved design, or <design>.op2.cam.json, .op3… when it is taken); name?; \
         discard?: drop an open job's unsaved edits → the job summary: {path?, name, dirty, revision, machine, post, tools, origin, \
         bodies, operations: [{index, name, type, tool?, enabled}]}. Nothing is written until cam.job.save; refused when a file is \
         already at path (open it instead)"
    ),
    job_cmd!(
        "cam.job.open",
        "Open Machining Job",
        open_job,
        &OPEN,
        JobSummary,
        "path: job .json; discard?: drop an open job's unsaved edits → the job summary (as cam.job.new). A file that does not read as a \
         job is refused with the key and line"
    ),
    job_cmd!(
        "cam.job.save",
        "Save Machining Job",
        save_job,
        &SAVE,
        CamJobSaveReply,
        "path?: save as this .json; overwrite?: write over a file changed outside SolidCraft (or another file at path) → path, bytes, \
         written. Keys in the job's order, the file's own indent, a final newline"
    ),
    job_cmd!(
        "cam.job.reload",
        "Reload Machining Job",
        reload_job,
        &RELOAD,
        CamJobReloadReply,
        "discard?: drop unsaved edits → summary, changes: [what changed, one line each]. One job undo step"
    ),
    job_cmd!("cam.job.close", "Close Machining Job", close_job, &CLOSE, CamJobCloseReply, "discard?: drop unsaved edits → closed"),
    job_cmd!(
        "cam.job.get",
        "Machining Job State",
        get_job,
        &GET,
        CamJobGetReply,
        "job?: include the JSON → open, outside: none | changed | gone, undo: [labels], redo: [labels], summary?, job?"
    ),
    job_cmd!(
        "cam.job.edit",
        "Edit Machining Job",
        edit_job,
        &EDIT,
        JobSummary,
        "set?: {key or dotted path: value} (job keys: machine, post, post_path, origin, tools, tool_library_path, tolerance, …); unset?: \
         [key or dotted path] → the summary. One job undo step; refused, changing nothing, when the job would not read"
    ),
    job_cmd!("cam.job.undo", "Undo Machining Job Edit", undo_job, &HISTORY, CamJobHistoryReply, "→ step: the label undone, summary"),
    job_cmd!("cam.job.redo", "Redo Machining Job Edit", redo_job, &HISTORY, CamJobHistoryReply, "→ step: the label redone, summary"),
];

/// The open job, or the refusal.
fn need<'a>(s: &'a mut Session, cmd: &str) -> Result<&'a mut OpenJob> {
    s.cam_job.as_deref_mut().ok_or_else(|| bad(cmd, NO_JOB))
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| EngineError::Other(e.to_string()))
}

/// Refused when a job is open with unsaved edits and the call does not say to drop them.
fn may_drop(s: &Session, cmd: &str, discard: Option<bool>) -> Result<()> {
    match s.cam_job.as_deref() {
        Some(open) if open.dirty() && !discard.unwrap_or(false) => {
            Err(bad(cmd, "the open job has unsaved changes: save it (cam.job.save) or give `discard: true` to drop them"))
        }
        _ => Ok(()),
    }
}

/// A job file's path: not empty, a `.json` file, in the folders the session may use.
fn job_path(cmd: &str, path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() || path.chars().count() > 4096 || path.chars().any(char::is_control) {
        return Err(bad(cmd, "`path` must be a file path"));
    }
    if !path.to_ascii_lowercase().ends_with(".json") {
        return Err(bad(cmd, "a job file is a `.json` file"));
    }
    solidcraft_io::vfs::check(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    Ok(path.to_string())
}

fn check_name(cmd: &str, what: &str, name: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(bad(cmd, format!("{what} must be 1 to {MAX_NAME} characters, without control characters")));
    }
    Ok(())
}

/// A job read from `path`: its JSON, what the file held and its indent. Refused, with the key
/// and line, when it does not read as a job.
fn read_job(cmd: &str, path: &str) -> Result<(Value, Disk, usize)> {
    let bytes = solidcraft_io::vfs::read(path, cam_job_state::MAX_FILE_BYTES).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| bad(cmd, format!("{path}: not UTF-8 text")))?;
    let value: Value = serde_json::from_str(text).map_err(|e| bad(cmd, format!("{path}: not valid JSON: {e}")))?;
    if !value.is_object() {
        return Err(bad(cmd, format!("{path}: a job is a JSON object")));
    }
    if super::picks::has_refs(&value) {
        // Picks and heights from faces are read without the model first.
        let plain = super::picks::placeholders(&value).map_err(|e| bad(cmd, format!("{path}: not a job (docs/cam-jobs.md): {e}")))?;
        if let Err(e) = serde_path_to_error::deserialize::<_, CamJob>(&plain) {
            let problem = job_problem(&plain, &e.path().to_string(), &e.inner().to_string());
            return Err(bad(cmd, format!("{path}: not a job (docs/cam-jobs.md): {problem}")));
        }
    } else {
        let mut de = serde_json::Deserializer::from_str(text);
        if let Err(e) = serde_path_to_error::deserialize::<_, CamJob>(&mut de) {
            let at = format!("line {} column {}", e.inner().line(), e.inner().column());
            let problem = job_problem(&value, &e.path().to_string(), &e.inner().to_string());
            return Err(bad(cmd, format!("{path}: not a job (docs/cam-jobs.md), {at}: {problem}")));
        }
    }
    check_job(cmd, &value)?;
    let disk = Disk { stamp: solidcraft_io::vfs::stamp(path), hash: cam_job_state::hash_bytes(&bytes) };
    Ok((value, disk, cam_job_state::detect_indent(text)))
}

/// Why operation `op` does not read, down to its key (the job's error stops at the operation,
/// as an operation is read by its `type`).
fn op_problem(op: &Value) -> Option<(String, String)> {
    use crate::schema::cam::{ContourOp, PocketOp};
    let ty = op.get("type")?.as_str()?;
    let mut body = op.as_object()?.clone();
    body.remove("type");
    let body = Value::Object(body);
    let err = match ty {
        "contour" => serde_path_to_error::deserialize::<_, ContourOp>(&body).err(),
        "pocket" => serde_path_to_error::deserialize::<_, PocketOp>(&body).err(),
        _ => None,
    }?;
    let path = err.path().to_string();
    Some((if path == "." { String::new() } else { path }, err.into_inner().to_string()))
}

/// The job's read error `path: why`, taken down to the key inside an operation.
fn job_problem(value: &Value, path: &str, why: &str) -> String {
    let k = path.strip_prefix("operations[").and_then(|r| r.strip_suffix(']')).and_then(|k| k.parse::<usize>().ok());
    match k.and_then(|k| value.get("operations")?.get(k)).and_then(op_problem) {
        Some((key, why)) if !key.is_empty() => format!("`{path}.{key}`: {why}"),
        Some((_, why)) => format!("`{path}`: {why}"),
        None => format!("`{path}`: {why}"),
    }
}

/// Refused when `value` is not a job that reads, or is too large.
fn check_job(cmd: &str, value: &Value) -> Result<()> {
    if cam_job_state::depth(value, MAX_DEPTH + 1) > MAX_DEPTH {
        return Err(bad(cmd, format!("the job is nested more than {MAX_DEPTH} levels deep")));
    }
    let n = value.get("operations").and_then(Value::as_array).map_or(0, Vec::len);
    if n > MAX_OPERATIONS {
        return Err(bad(cmd, format!("a job has at most {MAX_OPERATIONS} operations")));
    }
    if value.to_string().len() > cam_job_state::MAX_FILE_BYTES {
        return Err(bad(cmd, "the job would be larger than 16 MB"));
    }
    let plain;
    let value = if super::picks::has_refs(value) {
        plain = super::picks::placeholders(value).map_err(|e| bad(cmd, format!("the job would not read, so nothing changed: {e}")))?;
        &plain
    } else {
        value
    };
    if let Err(e) = serde_path_to_error::deserialize::<_, CamJob>(value) {
        let problem = job_problem(value, &e.path().to_string(), &e.inner().to_string());
        return Err(bad(cmd, format!("the job would not read, so nothing changed: {problem}")));
    }
    Ok(())
}

/// Keep `next` as the open job, one undo step `label`, once it reads as a job.
fn commit(s: &mut Session, cmd: &str, label: String, next: Value) -> Result<()> {
    check_job(cmd, &next)?;
    let open = need(s, cmd)?;
    if open.value != next {
        open.change(label, next);
    }
    Ok(())
}

/// The folder and name of the saved design, which its job files are named after.
fn design_stem(s: &Session) -> Option<(std::path::PathBuf, String)> {
    let path = Path::new(s.path.as_deref()?);
    let stem = path.file_stem()?.to_string_lossy().to_string();
    Some((path.parent()?.to_path_buf(), stem))
}

/// The first job file of `stem` in `dir` no file is at: `<stem>.cam.json`, then
/// `<stem>.op2.cam.json`, `.op3` and on.
fn free_job_path(dir: &Path, stem: &str) -> Option<String> {
    std::iter::once(format!("{stem}.cam.json"))
        .chain((2..=MAX_SETUPS).map(|n| format!("{stem}.op{n}.cam.json")))
        .map(|f| dir.join(f).to_string_lossy().to_string())
        .find(|p| !solidcraft_io::vfs::exists(p))
}

fn new_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.new";
    let params: CamJobNewParams = parse(cmd, p, "")?;
    may_drop(s, cmd, params.discard)?;
    let path = match &params.path {
        Some(given) => Some(job_path(cmd, given)?),
        None => match design_stem(s) {
            Some((dir, stem)) => Some(
                free_job_path(&dir, &stem)
                    .ok_or_else(|| bad(cmd, format!("{stem} has jobs up to its setup {MAX_SETUPS} beside it already: give the new one a `path`")))?,
            ),
            None => None,
        },
    };
    if let Some(path) = &path
        && solidcraft_io::vfs::exists(path)
    {
        return Err(bad(cmd, format!("a file is already at {path}: open it (cam.job.open) or give another `path`")));
    }
    let mut job = Map::new();
    if let Some(name) = &params.name {
        check_name(cmd, "`name`", name)?;
        job.insert("name".into(), json!(name));
    }
    job.insert("operations".into(), json!([]));
    let value = Value::Object(job);
    check_job(cmd, &value)?;
    s.cam_job = Some(Box::new(OpenJob::new(path, value, false, None, cam_job_state::DEFAULT_INDENT)));
    to_value(summary(s))
}

fn open_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.open";
    let params: CamJobOpenParams = parse(cmd, p, "")?;
    let path = job_path(cmd, &params.path)?;
    may_drop(s, cmd, params.discard)?;
    let (value, disk, indent) = read_job(cmd, &path)?;
    s.cam_job = Some(Box::new(OpenJob::new(Some(path), value, true, Some(disk), indent)));
    to_value(summary(s))
}

fn save_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.save";
    let params: CamJobSaveParams = parse(cmd, p, "")?;
    let overwrite = params.overwrite.unwrap_or(false);
    let given = params.path.as_deref().map(|p| job_path(cmd, p)).transpose()?;
    let open = need(s, cmd)?;
    let path = match (&given, &open.path) {
        (Some(p), _) | (None, Some(p)) => p.clone(),
        (None, None) => return Err(bad(cmd, "the job has no file yet: give `path`")),
    };
    let same = open.path.as_deref().is_some_and(|own| super::same_file(own, &path));
    if same {
        if open.outside() == Outside::Changed && !overwrite {
            return Err(bad(
                cmd,
                "the job file was changed outside SolidCraft since it was read: reload it (cam.job.reload {discard: true}) or give \
                 `overwrite: true` to keep these edits",
            ));
        }
    } else if solidcraft_io::vfs::exists(&path) && !overwrite {
        return Err(bad(cmd, format!("a file is already at {path}: give `overwrite: true` to replace it")));
    }
    let text = cam_job_state::write_job(&open.value, open.indent);
    let unchanged = same && !open.dirty() && open.disk.as_ref().is_some_and(|d| d.hash == cam_job_state::hash_bytes(text.as_bytes()));
    if !unchanged {
        super::write_file(&path, text.as_bytes()).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
    }
    open.path = Some(path.clone());
    open.mark_saved(text.as_bytes());
    to_value(CamJobSaveReply { path, bytes: text.len(), written: !unchanged })
}

fn reload_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.reload";
    let params: CamJobReloadParams = parse(cmd, p, "")?;
    may_drop(s, cmd, params.discard)?;
    let open = need(s, cmd)?;
    let Some(path) = open.path.clone() else { return Err(bad(cmd, "the job has no file to read again")) };
    let (value, disk, indent) = read_job(cmd, &path)?;
    let changes = cam_job_state::changes(&open.value, &value);
    if open.value != value {
        open.change("Reloaded: changed outside SolidCraft", value);
    }
    open.indent = indent;
    open.mark_saved(&[]);
    open.disk = Some(disk);
    to_value(CamJobReloadReply { summary: summary(s), changes })
}

fn close_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.close";
    let params: CamJobCloseParams = parse(cmd, p, "")?;
    may_drop(s, cmd, params.discard)?;
    let closed = s.cam_job.take().is_some();
    to_value(CamJobCloseReply { closed })
}

fn get_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.get";
    let params: CamJobGetParams = parse(cmd, p, "")?;
    let Some(open) = s.cam_job.as_deref_mut() else { return to_value(CamJobGetReply { outside: "none".into(), ..Default::default() }) };
    let reply = CamJobGetReply {
        open: true,
        outside: open.outside().key().to_string(),
        undo: open.undo.iter().rev().map(|st| st.label.clone()).collect(),
        redo: open.redo.iter().rev().map(|st| st.label.clone()).collect(),
        job: params.job.unwrap_or(false).then(|| open.value.clone()),
        summary: None,
    };
    to_value(CamJobGetReply { summary: Some(summary(s)), ..reply })
}

/// Checked dotted paths: each part a key (or, into an array, an index).
fn path_parts<'a>(cmd: &str, path: &'a str) -> Result<Vec<&'a str>> {
    let parts: Vec<&str> = path.split('.').collect();
    if path.is_empty() || path.len() > 256 || parts.len() > MAX_DEPTH || parts.iter().any(|p| p.is_empty() || p.chars().any(char::is_control)) {
        return Err(bad(cmd, format!("`{}` is not a key or a dotted path of keys", path.chars().take(64).collect::<String>())));
    }
    Ok(parts)
}

/// Sets `path` in `root` to `v`, making the objects on the way.
fn set_path(cmd: &str, root: &mut Value, path: &str, v: Value) -> Result<()> {
    let parts = path_parts(cmd, path)?;
    let mut at = root;
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        at = match at {
            Value::Object(map) => {
                if last {
                    map.insert((*part).to_string(), v);
                    return Ok(());
                }
                map.entry((*part).to_string()).or_insert_with(|| Value::Object(Map::new()))
            }
            Value::Array(items) => {
                let k: usize = part.parse().map_err(|_| bad(cmd, format!("`{path}`: `{part}` is not a place in a list")))?;
                let len = items.len();
                let slot = items.get_mut(k).ok_or_else(|| bad(cmd, format!("`{path}`: the list has {len} items")))?;
                if last {
                    *slot = v;
                    return Ok(());
                }
                slot
            }
            _ => return Err(bad(cmd, format!("`{path}`: `{part}` is inside a value that is not an object"))),
        };
    }
    Ok(())
}

/// Removes `path` from `root` (nothing when it is not there).
fn unset_path(cmd: &str, root: &mut Value, path: &str) -> Result<()> {
    let parts = path_parts(cmd, path)?;
    let Some((last, way)) = parts.split_last() else { return Ok(()) };
    let mut at = root;
    for part in way {
        at = match at {
            Value::Object(map) => match map.get_mut(*part) {
                Some(v) => v,
                None => return Ok(()),
            },
            Value::Array(items) => match part.parse::<usize>().ok().and_then(|k| items.get_mut(k)) {
                Some(v) => v,
                None => return Ok(()),
            },
            _ => return Ok(()),
        };
    }
    if let Value::Object(map) = at {
        map.remove(*last);
    }
    Ok(())
}

/// `set` and `unset` applied to `target`; keys under `guard` are refused.
fn apply_edits(cmd: &str, target: &mut Value, set: Option<&Map<String, Value>>, unset: Option<&[String]>, guard: &[&str]) -> Result<()> {
    let n = set.map_or(0, Map::len) + unset.map_or(0, <[String]>::len);
    if n > MAX_KEYS {
        return Err(bad(cmd, format!("one edit sets or unsets at most {MAX_KEYS} keys")));
    }
    for (path, v) in set.into_iter().flatten() {
        if guard.iter().any(|g| path.split('.').next() == Some(*g)) {
            return Err(bad(cmd, format!("`{path}` is not edited here")));
        }
        set_path(cmd, target, path, v.clone())?;
    }
    for path in unset.into_iter().flatten() {
        if guard.iter().any(|g| path.split('.').next() == Some(*g)) {
            return Err(bad(cmd, format!("`{path}` is not edited here")));
        }
        unset_path(cmd, target, path)?;
    }
    Ok(())
}

fn edit_job(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "cam.job.edit";
    let params: CamJobEditParams = parse(cmd, p, "")?;
    let open = need(s, cmd)?;
    let mut next = open.value.clone();
    apply_edits(cmd, &mut next, params.set.as_ref(), params.unset.as_deref(), &["operations"])?;
    let keys: Vec<String> = params.set.iter().flat_map(Map::keys).cloned().chain(params.unset.iter().flatten().cloned()).collect();
    let label = match keys.as_slice() {
        [one] => format!("Set {one}"),
        _ => "Edit the job".to_string(),
    };
    commit(s, cmd, label, next)?;
    to_value(summary(s))
}

fn history(s: &mut Session, cmd: &str, redo: bool) -> Result<Value> {
    let open = need(s, cmd)?;
    let step = if redo { open.redo() } else { open.undo() };
    to_value(CamJobHistoryReply { step, summary: summary(s) })
}

fn undo_job(s: &mut Session, p: &Value) -> Result<Value> {
    let _: CamJobHistoryParams = parse("cam.job.undo", p, "")?;
    history(s, "cam.job.undo", false)
}

fn redo_job(s: &mut Session, p: &Value) -> Result<Value> {
    let _: CamJobHistoryParams = parse("cam.job.redo", p, "")?;
    history(s, "cam.job.redo", true)
}

/// The open job in brief.
fn summary(s: &Session) -> JobSummary {
    let Some(open) = s.cam_job.as_deref() else { return JobSummary::default() };
    let v = &open.value;
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let file = |p: String| Path::new(&p).file_name().map_or(p.clone(), |f| f.to_string_lossy().to_string());
    let machine = match v.get("machine") {
        Some(Value::String(m)) => m.clone(),
        Some(Value::Object(m)) => m.get("name").and_then(Value::as_str).unwrap_or("custom").to_string(),
        _ => "none".to_string(),
    };
    let post = text("post").or_else(|| text("post_path").map(file)).unwrap_or_else(|| "none".to_string());
    let tools = match (v.get("tools").and_then(Value::as_array), text("tool_library_path")) {
        (Some(t), _) => format!("{} tools", t.len()),
        (None, Some(p)) => file(p),
        _ => "none".to_string(),
    };
    let origin = v.get("origin").and_then(Value::as_array).map_or([0.0; 3], |a| {
        let g = |k: usize| a.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        [g(0), g(1), g(2)]
    });
    let bodies =
        v.get("bodies").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let operations = open
        .operations()
        .iter()
        .enumerate()
        .map(|(index, op)| OpSummary {
            index,
            name: op_name(op),
            kind: op.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
            tool: op.get("tool").and_then(Value::as_u64).and_then(|t| u32::try_from(t).ok()),
            enabled: op_enabled(op),
        })
        .collect();
    JobSummary {
        path: open.path.clone(),
        name: text("name").unwrap_or_else(|| s.doc.name.clone()),
        dirty: open.dirty(),
        revision: open.revision,
        machine,
        post,
        tools,
        origin,
        bodies,
        operations,
    }
}

#[cfg(test)]
#[path = "cam_job_edit_tests.rs"]
mod tests;
