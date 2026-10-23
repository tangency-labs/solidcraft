//! UI macros: record commands as Rhai, play them in the engine (one undo, rollback on failure).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::CommandSpec;
use crate::frames::TopLevel;
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

/// The most commands one recording holds. Recording stops at it.
pub const MAX_STEPS: usize = 10_000;
const MAX_MACRO_BYTES: usize = 4 << 20;
const MAX_LISTED: usize = 200;
/// Rhai's runaway-loop guard for one macro play. Generous because a macro that computes its
/// own geometry (a gear's involute is ~30k steps per flank pass) is doing real work; even at
/// this cap a `while true {}` stops in well under a second.
const MAX_OPS: u64 = 1_000_000;
/// Rhai's own default (64) is too tight for real generated macros — a gear's involute loop
/// trips it — but the limit stays bounded so hostile input still fails cleanly at parse time.
const MAX_EXPR_DEPTH: usize = 512;

/// A recorded or loaded Rhai macro.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Macro {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<MacroParam>,
    /// Rhai source `macro.play` evaluates.
    #[serde(default)]
    pub source: String,
}

/// What kind of value a `@param` asks for: how Play checks it and which control the UI shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    /// Free text (a macro with no kind token).
    #[default]
    Text,
    /// A whole number; Play shows a whole-number field and Rhai sees an integer.
    Int,
    /// A decimal number; Rhai sees a number (so `nx * spacing` just works).
    Number,
    /// A length expression (`5 mm`, `width + 2`); commands get it as they would from the UI.
    Length,
    /// An angle expression (`90 deg`).
    Angle,
    /// A toggle; `yes`/`no` (or `true`/`false`) become a Rhai bool.
    Bool,
    /// One of the listed options; Play shows a dropdown.
    Choice,
}

impl ParamKind {
    fn is_text(&self) -> bool {
        *self == Self::Text
    }
}

/// A `@if name=v1,v2` visibility gate on a parameter: the Play dialog hides the row until
/// the watched parameter holds one of the listed values. The macro body guards itself, so
/// the engine still binds a gated-out parameter from its default.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamGate {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacroParam {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The declared kind (`text` when a macro does not say).
    #[serde(default, skip_serializing_if = "ParamKind::is_text")]
    pub kind: ParamKind,
    /// The allowed values of a `choice` parameter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// The `@if` gate that hides this parameter until another one holds a listed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<ParamGate>,
}

#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub recording: bool,
    pub paused: bool,
    pub playing: bool,
    pub captured: Macro,
    pub loaded: Option<Macro>,
}

pub fn recordable(id: &str) -> bool {
    !(id.starts_with("macro.")
        || id.starts_with("file.")
        || id.starts_with("doc.")
        || id.starts_with("view.")
        || id.starts_with("ui.")
        || matches!(id, "edit.undo" | "edit.redo"))
}

impl Recorder {
    pub(crate) fn note(&mut self, id: &str, params: &Value) {
        if !self.recording || self.paused || self.playing || !recordable(id) {
            return;
        }
        if run_count(&self.captured.source) >= MAX_STEPS {
            self.recording = false;
            self.paused = false;
            if run_count(&self.captured.source) > 0 {
                self.loaded = Some(self.captured.clone());
            }
            return;
        }
        let params = match params.as_object() {
            Some(obj) => {
                let mut p = obj.clone();
                p.remove("live");
                if p.is_empty() { Value::Null } else { Value::Object(p) }
            }
            None => Value::Null,
        };
        append_run(&mut self.captured.source, id, &params);
    }

    pub fn steps(&self) -> usize {
        let n = run_count(&self.captured.source);
        if n > 0 { n } else { self.loaded.as_ref().map(|m| run_count(&m.source)).unwrap_or(0) }
    }
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("macro.record", "Record Macro", record)
        .at("MACRO", "RECORD")
        .icon("record")
        .noundo()
        .doc_only()
        .enabled(idle)
        .params("name?: the macro's name → {recording, paused, steps}"),
    CommandSpec::new("macro.pause", "Pause Recording", pause)
        .at("MACRO", "RECORD")
        .icon("pause")
        .noundo()
        .doc_only()
        .enabled(capturing)
        .params("→ {recording, paused: true, steps}"),
    CommandSpec::new("macro.resume", "Resume Recording", resume)
        .at("MACRO", "RECORD")
        .icon("record")
        .noundo()
        .doc_only()
        .enabled(paused)
        .params("→ {recording, paused: false, steps}"),
    CommandSpec::new("macro.stop", "Stop Recording", stop)
        .at("MACRO", "RECORD")
        .icon("stop")
        .noundo()
        .doc_only()
        .enabled(recording)
        .params("→ {recording: false, steps}: the recording is loaded as Rhai"),
    CommandSpec::new("macro.play", "Play Macro", play)
        .at("MACRO", "PLAYBACK")
        .icon("play")
        .enabled(ready)
        .params("source?: Rhai text | path?: .rhai file | name?: file in the macros folder | params?: {name: value} (default: the loaded macro; values are checked against each @param's kind: text, int, number, length, angle, bool, choice). Host: run(command, params), list_dir(path), write_file(path, text). One undo; a failure rolls back"),
    CommandSpec::new("macro.open", "Open Macro", open)
        .at("MACRO", "PLAYBACK")
        .icon("open")
        .noundo()
        .doc_only()
        .enabled(idle)
        .params("path: a .rhai file → name, steps, params"),
    CommandSpec::new("macro.save", "Save Macro", save)
        .at("MACRO", "PLAYBACK")
        .icon("export")
        .noundo()
        .doc_only()
        .enabled(has_steps)
        .params("name?: file in the macros folder ending in .rhai, path?: any .rhai file → {path, steps}"),
    CommandSpec::new("macro.clear", "Clear Macro", clear_macro)
        .at("MACRO", "PLAYBACK")
        .icon("delete")
        .noundo()
        .doc_only()
        .enabled(has_steps)
        .params("→ {cleared: true}"),
    CommandSpec::new("macro.list", "List Macros", list).icon("folder").noundo().doc_only().params("→ {recording, paused, steps, dir, macros}"),
    CommandSpec::new("macro.delete", "Delete Macro", delete).icon("delete").noundo().doc_only().params("name: a saved .rhai name"),
];

fn idle(s: &Session) -> std::result::Result<(), String> {
    if s.macros.recording {
        Err("already recording; stop the recording first".into())
    } else if s.macros.playing {
        Err("a macro is playing".into())
    } else {
        Ok(())
    }
}

fn recording(s: &Session) -> std::result::Result<(), String> {
    if s.macros.recording { Ok(()) } else { Err("nothing is being recorded".into()) }
}

fn capturing(s: &Session) -> std::result::Result<(), String> {
    if s.macros.recording && !s.macros.paused {
        Ok(())
    } else if s.macros.paused {
        Err("recording is paused; resume it first".into())
    } else {
        Err("nothing is being recorded".into())
    }
}

fn paused(s: &Session) -> std::result::Result<(), String> {
    if s.macros.paused { Ok(()) } else { Err("recording is not paused".into()) }
}

fn ready(s: &Session) -> std::result::Result<(), String> {
    idle(s).map_err(|why| match why.as_str() {
        "already recording; stop the recording first" => "stop the recording before playing a macro".into(),
        other => other.into(),
    })
}

fn has_steps(s: &Session) -> std::result::Result<(), String> {
    let loaded = s.macros.loaded.as_ref().is_some_and(|m| run_count(&m.source) > 0);
    if run_count(&s.macros.captured.source) == 0 && !loaded { Err("nothing has been recorded yet".into()) } else { Ok(()) }
}

fn record(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "macro.record";
    if let Some(v) = p.get("name")
        && !v.is_null()
        && str_(p, "name").is_none()
    {
        return Err(bad(cmd, "`name` must be text"));
    }
    let name = str_(p, "name").map(str::to_string).or_else(|| Some(s.doc.name.clone()));
    let mut source = String::new();
    if let Some(n) = name.as_ref().filter(|n| !n.is_empty()) {
        source.push_str("// ");
        source.push_str(n);
        source.push('\n');
    }
    s.macros.captured = Macro { name, source, ..Default::default() };
    s.macros.recording = true;
    s.macros.paused = false;
    Ok(json!({"recording": true, "paused": false, "steps": 0}))
}

fn pause(s: &mut Session, _p: &Value) -> Result<Value> {
    s.macros.paused = true;
    Ok(json!({"recording": true, "paused": true, "steps": s.macros.steps()}))
}

fn resume(s: &mut Session, _p: &Value) -> Result<Value> {
    s.macros.paused = false;
    Ok(json!({"recording": true, "paused": false, "steps": s.macros.steps()}))
}

fn stop(s: &mut Session, _p: &Value) -> Result<Value> {
    s.macros.recording = false;
    s.macros.paused = false;
    let steps = run_count(&s.macros.captured.source);
    if steps > 0 {
        s.macros.loaded = Some(s.macros.captured.clone());
    }
    Ok(json!({"recording": false, "paused": false, "steps": steps}))
}

fn clear_macro(s: &mut Session, _p: &Value) -> Result<Value> {
    s.macros.captured = Macro::default();
    s.macros.loaded = None;
    Ok(json!({"cleared": true}))
}

pub fn macros_dir() -> Option<PathBuf> {
    crate::recovery::data_dir().map(|d| d.join("macros"))
}

fn named(cmd: &str, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 128
        || !name.ends_with(".rhai")
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
    {
        return Err(bad(cmd, "`name` must be one plain file name ending in .rhai (letters, numbers, spaces, -_.)"));
    }
    let dir = macros_dir().ok_or_else(|| bad(cmd, "no macros folder on this system; give a path instead"))?;
    Ok(dir.join(name))
}

fn is_param_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn read_macro_file(cmd: &str, path: &Path) -> Result<String> {
    let text =
        solidcraft_io::vfs::read(&path.display().to_string(), MAX_MACRO_BYTES).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    String::from_utf8(text).map_err(|_| bad(cmd, format!("{} is not text", path.display())))
}

/// The kind token of a `@param` line, if the word names one.
fn param_kind(word: &str) -> Option<ParamKind> {
    Some(match word {
        "text" => ParamKind::Text,
        "int" => ParamKind::Int,
        "number" => ParamKind::Number,
        "length" => ParamKind::Length,
        "angle" => ParamKind::Angle,
        "bool" => ParamKind::Bool,
        "choice" => ParamKind::Choice,
        _ => return None,
    })
}

/// Split a trailing `@if name=v1,v2` visibility gate off a `@param` fragment. The gate is
/// whitespace-delimited so it never eats a label or default that merely contains `@if`.
fn split_gate(cmd: &str, text: &str) -> Result<(String, Option<ParamGate>)> {
    let Some((at, _)) = text.match_indices("@if").find(|(i, _)| *i == 0 || text[..*i].ends_with(char::is_whitespace)) else {
        return Ok((text.to_string(), None));
    };
    let head = text[..at].trim().to_string();
    let (gname, gvals) = text[at + 3..].trim().split_once('=').ok_or_else(|| bad(cmd, "an `@if` gate needs `@if name=value`"))?;
    let gname = gname.trim();
    if !is_param_name(gname) {
        return Err(bad(cmd, "an `@if` gate needs a parameter name to watch"));
    }
    let values: Vec<String> = gvals.split(',').map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).collect();
    if values.is_empty() || values.len() > 64 || values.iter().any(|v| v.chars().count() > 128 || v.chars().any(|c| c.is_control())) {
        return Err(bad(cmd, format!("the `@if` gate watching `{gname}` needs 1 to 64 plain values")));
    }
    Ok((head, Some(ParamGate { name: gname.to_string(), values })))
}

/// Check a value against a parameter's kind and turn it into the JSON type Rhai sees.
/// Lengths and angles stay expressions ("5 mm", `width + 2`): the command that receives
/// them evaluates and unit-checks them, exactly as if the value came from the UI.
fn coerce_param(kind: ParamKind, options: &[String], v: &Value) -> std::result::Result<Value, String> {
    match kind {
        ParamKind::Text => match v {
            Value::String(_) | Value::Number(_) | Value::Bool(_) => Ok(v.clone()),
            _ => Err("needs text".into()),
        },
        ParamKind::Int => match v {
            Value::Number(n) if n.is_i64() => Ok(v.clone()),
            Value::String(s) => s.trim().parse::<i64>().map(|i| json!(i)).map_err(|_| format!("`{s}` is not a whole number")),
            _ => Err("needs a whole number".into()),
        },
        ParamKind::Number => match v {
            Value::Number(_) => Ok(v.clone()),
            Value::String(s) => {
                s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map(|x| json!(x)).ok_or_else(|| format!("`{s}` is not a number"))
            }
            _ => Err("needs a number".into()),
        },
        ParamKind::Length | ParamKind::Angle => match v {
            Value::Number(_) => Ok(v.clone()),
            Value::String(s) => {
                let t = s.trim();
                if t.is_empty() || t.chars().count() > 128 {
                    return Err(if kind == ParamKind::Length { "needs a length, like `5 mm`" } else { "needs an angle, like `90 deg`" }.into());
                }
                // A plain number is the design's own unit: keep it a number so a macro can
                // compute with it; an expression ("5 mm", `width + 2`) stays text for the
                // command that receives it to evaluate.
                match t.parse::<f64>() {
                    Ok(x) if x.is_finite() => Ok(json!(x)),
                    _ => Ok(json!(s)),
                }
            }
            _ => Err(if kind == ParamKind::Length { "needs a length, like `5 mm`" } else { "needs an angle, like `90 deg`" }.into()),
        },
        ParamKind::Bool => match v {
            Value::Bool(_) => Ok(v.clone()),
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => Ok(json!(true)),
                "false" | "no" | "0" | "off" => Ok(json!(false)),
                other => Err(format!("`{other}` is not yes or no")),
            },
            _ => Err("needs yes or no".into()),
        },
        ParamKind::Choice => match v.as_str() {
            Some(s) if options.iter().any(|o| o == s) => Ok(v.clone()),
            Some(s) => Err(format!("`{s}` is not one of: {}", options.join(", "))),
            None => Err("needs one of the listed values".into()),
        },
    }
}

pub(crate) fn parse_rhai_macro(cmd: &str, text: &str) -> Result<Macro> {
    if text.len() > MAX_MACRO_BYTES {
        return Err(bad(cmd, "the macro file is too large"));
    }
    if text.trim().is_empty() {
        return Err(bad(cmd, "the macro holds no source"));
    }
    let mut params = Vec::new();
    let mut name = None;
    let mut description = None;
    for line in text.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("//") else { continue };
        let rest = rest.trim();
        if let Some(spec) = rest.strip_prefix("@param") {
            let spec = spec.trim();
            // A trailing `@if name=v1,v2` gate hides the parameter in the Play dialog until
            // the watched parameter holds one of the listed values; it rides after the default.
            let (left, default, gate) = match spec.split_once('|') {
                Some((l, r)) => {
                    let (d, g) = split_gate(cmd, r.trim())?;
                    (l.trim().to_string(), Some(d), g)
                }
                None => {
                    let (l, g) = split_gate(cmd, spec)?;
                    (l, None, g)
                }
            };
            // `name [label words] [kind] [choice, list]`: the option list is a trailing bracket
            // group (spaces inside allowed) — `[a, b]`, or the `{a, b}` shape community macros
            // use — and the kind token sits just before it, so `nx Count in X int` and
            // `mat Material choice [steel, brass]` both parse.
            // Older macros (no kind token) keep working as text.
            let open = match (left.ends_with(']'), left.ends_with('}')) {
                (true, _) => left.find('['),
                (_, true) => left.rfind('{'),
                (false, false) => None,
            };
            let (left, options) = match open {
                Some(from) if from + 1 < left.len() => {
                    let list = &left[from + 1..left.len() - 1];
                    let options = list.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
                    (left[..from].trim().to_string(), options)
                }
                _ => (left, Vec::new()),
            };
            let mut bits = left.split_whitespace();
            let n = bits.next().unwrap_or("").trim();
            if !is_param_name(n) {
                return Err(bad(cmd, "every @param needs a name (letters, digits, `_` and `-`)"));
            }
            if params.iter().any(|q: &MacroParam| q.name == n) {
                return Err(bad(cmd, format!("parameter `{n}` is given twice")));
            }
            let mut tail: Vec<&str> = bits.collect();
            // Options make it a choice, and a literal `choice` token is then just the kind
            // word, not part of the label. Without options a trailing kind word (`int`,
            // `length`, …) types the parameter — but a label word like "Pressure angle"
            // must survive untouched.
            let mut kind = if options.is_empty() { ParamKind::Text } else { ParamKind::Choice };
            if options.is_empty() {
                if let Some(k) = tail.last().copied().and_then(param_kind) {
                    kind = k;
                    tail.pop();
                }
            } else if tail.last().copied() == Some("choice") {
                tail.pop();
            }
            if kind == ParamKind::Choice && options.is_empty() {
                return Err(bad(cmd, format!("choice parameter `{n}` needs its options: `choice [a, b, …]`")));
            }
            if options.len() > 64 || options.iter().any(|o| o.chars().count() > 128 || o.chars().any(|c| c.is_control())) {
                return Err(bad(cmd, format!("choice parameter `{n}` needs 1 to 64 plain options")));
            }
            {
                let mut sorted = options.clone();
                sorted.sort();
                sorted.dedup();
                if sorted.len() != options.len() {
                    return Err(bad(cmd, format!("choice parameter `{n}` lists the same option twice")));
                }
            }
            let label = tail.join(" ");
            let default = default
                .map(|d| coerce_param(kind, &options, &Value::String(d)))
                .transpose()
                .map_err(|why| bad(cmd, format!("parameter `{n}` default: {why}")))?;
            params.push(MacroParam { name: n.to_string(), label: if label.is_empty() { None } else { Some(label) }, kind, options, default, gate });
            if params.len() > 100 {
                return Err(bad(cmd, "a macro asks for at most 100 parameters"));
            }
        } else if name.is_none() && !rest.is_empty() && !rest.starts_with('@') {
            name = Some(rest.to_string());
        } else if description.is_none() && !rest.is_empty() && !rest.starts_with('@') {
            description = Some(rest.to_string());
        }
    }
    Ok(Macro { name, description, params, source: text.to_string() })
}

fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "macro.open";
    let path = str_(p, "path").ok_or_else(|| bad(cmd, "`path` must be a .rhai file"))?;
    if !path.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("rhai")) {
        return Err(bad(cmd, "`path` must end in .rhai"));
    }
    let text = read_macro_file(cmd, Path::new(path))?;
    let m = parse_rhai_macro(cmd, &text)?;
    let out = json!({"name": m.name.clone().unwrap_or_default(), "steps": m.source.matches("run(").count(), "params": json!(m.params)});
    s.macros.loaded = Some(m);
    Ok(out)
}

fn play(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "macro.play";
    if s.macros.playing {
        return Err(bad(cmd, "a macro is already playing"));
    }
    let loaded = match (str_(p, "source"), str_(p, "name"), str_(p, "path")) {
        (Some(src), None, None) => parse_rhai_macro(cmd, src)?,
        (None, Some(name), None) => parse_rhai_macro(cmd, &read_macro_file(cmd, &named(cmd, name)?)?)?,
        (None, None, Some(path)) => parse_rhai_macro(cmd, &read_macro_file(cmd, Path::new(path))?)?,
        (None, None, None) => s
            .macros
            .loaded
            .clone()
            .filter(|m| run_count(&m.source) > 0)
            .ok_or_else(|| bad(cmd, "nothing has been recorded; give source, a path or a saved macro's name"))?,
        _ => return Err(bad(cmd, "give source, name or path — one of them")),
    };
    let given = match p.get("params") {
        None | Some(Value::Null) => &Value::Null,
        Some(v @ Value::Object(_)) => v,
        Some(_) => return Err(bad(cmd, "`params` is an object of {name: value}")),
    };
    let mut missing: Vec<&str> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    let mut bound: Vec<(String, Value)> = Vec::new();
    for mp in &loaded.params {
        match given.get(&mp.name).or(mp.default.as_ref()) {
            Some(v) => match coerce_param(mp.kind, &mp.options, v) {
                Ok(v) => bound.push((mp.name.clone(), v)),
                Err(why) => invalid.push(format!("`{}`: {why}", mp.name)),
            },
            None => missing.push(mp.name.as_str()),
        }
    }
    if !missing.is_empty() {
        return Err(bad(cmd, format!("macro `{}` needs values for: {}", loaded.name.as_deref().unwrap_or("macro"), missing.join(", "))));
    }
    if !invalid.is_empty() {
        return Err(bad(cmd, format!("macro `{}` got bad values — {}", loaded.name.as_deref().unwrap_or("macro"), invalid.join("; "))));
    }
    let mark = (s.doc.clone(), s.active_sketch, s.pending_moves.clone(), s.redo.clone(), s.undo.len());
    s.macros.playing = true;
    let _top = TopLevel::enter();
    let outcome = eval_rhai(s, &loaded.source, &bound);
    s.macros.playing = false;
    match outcome {
        Ok(()) => {
            // One undo step for the whole macro: drop the snapshots its own commands pushed and
            // keep a single one holding the design as it was before Play, so one Ctrl+Z undoes
            // the macro (docs/macros.md). A macro that changed nothing adds no undo step.
            s.undo.truncate(mark.4);
            if !std::sync::Arc::ptr_eq(&s.doc, &mark.0) || s.active_sketch != mark.1 || s.pending_moves != mark.2 {
                let label = loaded.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "Play Macro".to_string());
                s.undo.push(crate::Snapshot { label, doc: mark.0, active_sketch: mark.1, pending_moves: mark.2 });
                if s.undo.len() > crate::MAX_UNDO {
                    s.undo.remove(0);
                }
            }
            Ok(json!({"ran": true, "steps": loaded.source.matches("run(").count()}))
        }
        Err(e) => {
            s.doc = mark.0;
            s.active_sketch = mark.1;
            s.pending_moves = mark.2;
            s.redo = mark.3;
            s.undo.truncate(mark.4);
            s.refresh();
            Err(e)
        }
    }
}

fn eval_rhai(s: &mut Session, source: &str, bound: &[(String, Value)]) -> Result<()> {
    let shared = Rc::new(RefCell::new(std::mem::take(s)));
    let eval = {
        let mut engine = Engine::new();
        engine.set_max_operations(MAX_OPS);
        engine.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_EXPR_DEPTH);
        let sh = Rc::clone(&shared);
        engine.register_fn("run", move |cmd: ImmutableString, params: Dynamic| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
            let json_params: Value = rhai::serde::from_dynamic(&params).unwrap_or(Value::Null);
            let v = sh.borrow_mut().execute(cmd.as_str(), &json_params).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))?;
            rhai::serde::to_dynamic(&v).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))
        });
        let sh = Rc::clone(&shared);
        engine.register_fn("run", move |cmd: ImmutableString| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
            let v = sh.borrow_mut().execute(cmd.as_str(), &Value::Null).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))?;
            rhai::serde::to_dynamic(&v).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))
        });
        engine.register_fn("list_dir", list_dir_host);
        engine.register_fn("write_file", write_file_host);
        engine.register_fn("file_stem", file_stem_host);
        let mut scope = Scope::new();
        for (name, v) in bound {
            match rhai::serde::to_dynamic(v) {
                Ok(d) => {
                    scope.push_dynamic(name.clone(), d);
                }
                Err(e) => return Err(EngineError::Other(e.to_string())),
            }
        }
        engine.eval_with_scope::<Dynamic>(&mut scope, source).map(|_| ()).map_err(|e| EngineError::Other(format!("macro: {e}")))
    };
    match Rc::try_unwrap(shared) {
        Ok(cell) => *s = cell.into_inner(),
        Err(_) => return Err(EngineError::Other("macro still holds the session".into())),
    }
    eval
}

fn list_dir_host(path: ImmutableString) -> std::result::Result<Dynamic, Box<EvalAltResult>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        return Err(Box::new(EvalAltResult::from("list_dir is not supported in the browser")));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if path.is_empty() || path.len() > 4096 || path.contains("..") {
            return Err(Box::new(EvalAltResult::from("list_dir: bad path")));
        }
        let mut names: Vec<Dynamic> = Vec::new();
        let entries = std::fs::read_dir(path.as_str()).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))?;
        for e in entries.flatten().take(1000) {
            let p = e.path();
            if p.is_file() {
                names.push(Dynamic::from(p.to_string_lossy().into_owned()));
            }
        }
        names.sort_by_key(ToString::to_string);
        Ok(Dynamic::from_array(names))
    }
}

fn file_stem_host(path: ImmutableString) -> ImmutableString {
    Path::new(path.as_str()).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().into()
}

fn write_file_host(path: ImmutableString, text: ImmutableString) -> std::result::Result<Dynamic, Box<EvalAltResult>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, text);
        return Err(Box::new(EvalAltResult::from("write_file is not supported in the browser")));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if path.is_empty() || path.len() > 4096 || path.contains("..") {
            return Err(Box::new(EvalAltResult::from("write_file: bad path")));
        }
        if text.len() > MAX_MACRO_BYTES {
            return Err(Box::new(EvalAltResult::from("write_file: too large")));
        }
        if let Some(parent) = Path::new(path.as_str()).parent().filter(|p| !p.as_os_str().is_empty())
            && let Err(e) = solidcraft_io::vfs::create_dir_all(&parent.display().to_string())
        {
            return Err(Box::new(EvalAltResult::from(e.to_string())));
        }
        solidcraft_io::vfs::write(path.as_str(), text.as_bytes()).map_err(|e| Box::new(EvalAltResult::from(e.to_string())))?;
        Ok(Dynamic::UNIT)
    }
}

fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "macro.save";
    let name = p.get("name").filter(|v| !v.is_null());
    let path = p.get("path").filter(|v| !v.is_null());
    if name.is_some() && path.is_some() {
        return Err(bad(cmd, "give a name or a path, not both"));
    }
    let m = if run_count(&s.macros.captured.source) > 0 {
        s.macros.captured.clone()
    } else if let Some(m) = s.macros.loaded.clone().filter(|m| run_count(&m.source) > 0) {
        m
    } else {
        return Err(bad(cmd, "nothing has been recorded yet"));
    };
    let path = match (name, path) {
        (Some(name), None) => {
            let name = name.as_str().ok_or_else(|| bad(cmd, "`name` must be text"))?;
            named(cmd, name)?.display().to_string()
        }
        (None, Some(path)) => path.as_str().ok_or_else(|| bad(cmd, "`path` must be a file path"))?.to_string(),
        _ => {
            let stem = m.name.as_deref().filter(|n| !n.is_empty()).unwrap_or("macro");
            named(cmd, &format!("{stem}.rhai"))?.display().to_string()
        }
    };
    if !path.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("rhai")) {
        return Err(bad(cmd, "`path` must end in .rhai"));
    }
    if let Some(parent) = Path::new(&path).parent().filter(|p| !p.as_os_str().is_empty())
        && let Err(e) = solidcraft_io::vfs::create_dir_all(&parent.display().to_string())
    {
        return Err(EngineError::Other(format!("{}: {e}", parent.display())));
    }
    solidcraft_io::vfs::write(&path, m.source.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(json!({"path": path, "steps": m.source.matches("run(").count()}))
}

fn saved_macros() -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let Some(dir) = macros_dir() else { return out };
    let Ok(entries) = std::fs::read_dir(&dir) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".rhai")
            && entry.path().is_file()
            && let Ok(len) = entry.path().metadata().map(|m| m.len())
        {
            out.push((name, len));
        }
    }
    out.sort();
    out.truncate(MAX_LISTED);
    out
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    let dir = macros_dir().map(|d| d.display().to_string());
    let macros: Vec<Value> = saved_macros().into_iter().map(|(name, bytes)| json!({"name": name, "bytes": bytes})).collect();
    Ok(json!({"recording": s.macros.recording, "paused": s.macros.paused, "steps": s.macros.steps(), "dir": dir, "macros": macros}))
}

fn delete(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "macro.delete";
    let name = str_(p, "name").ok_or_else(|| bad(cmd, "`name` is required (see macro.list)"))?;
    let path = named(cmd, name)?;
    solidcraft_io::vfs::remove(&path.display().to_string()).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    Ok(json!({"deleted": name}))
}

fn run_count(src: &str) -> usize {
    src.matches("run(").count()
}

fn append_run(source: &mut String, id: &str, params: &Value) {
    source.push_str("run(\"");
    source.push_str(id);
    if params.is_null() || params.as_object().is_some_and(|o| o.is_empty()) {
        source.push_str("\");\n");
    } else {
        source.push_str("\", ");
        source.push_str(&json_to_rhai(params));
        source.push_str(");\n");
    }
}

fn json_to_rhai(v: &Value) -> String {
    match v {
        Value::Null => "#{ }".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(json_to_rhai).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(o) => {
            let parts: Vec<String> = o
                .iter()
                .map(|(k, val)| {
                    let key = if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { k.clone() } else { format!("\"{k}\"") };
                    format!("{key}: {}", json_to_rhai(val))
                })
                .collect();
            format!("#{{{}}}", parts.join(", "))
        }
    }
}

impl Session {
    pub fn macro_state(&self) -> Value {
        json!({"recording": self.macros.recording, "paused": self.macros.paused, "steps": self.macros.steps()})
    }
}
