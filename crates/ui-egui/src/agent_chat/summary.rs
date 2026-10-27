//! A tool call in words, for its card (#1457): "Extrude Sketch1 by 10 mm", "Box 40 × 30 × 20 mm,
//! then Fillet 4 edges (r 4 mm)", "Saved block.solidcraft".
//!
//! A first pass built from the command's translated label, its main parameters (lengths in the
//! design's units) and the bodies the result names. Anything it doesn't know (another agent's own
//! tools, a call too large or malformed to read) gives `None`, and the card keeps its title.
//! The arguments and the result are the agent's: read with `get`, quoted short, never trusted.

use serde_json::{Map, Value};

use crate::i18n::{format as trf, tr, tr_label};

/// Steps of a batch named before "and N more".
const MAX_STEPS: usize = 4;
/// Created bodies named after the arrow.
const MAX_BODIES: usize = 3;
/// Longest name, path or expression quoted from a call (characters).
const MAX_WORD: usize = 40;
/// Longest summary (characters).
const MAX_SUMMARY: usize = 240;
/// Largest result read for its bodies or measurements (bytes).
const MAX_RESULT: usize = 64 * 1024;

/// How lengths are shown: the design's units and the length precision.
#[derive(Clone, Copy, Debug)]
pub struct Units<'a> {
    pub units: &'a str,
    pub decimals: u8,
}

/// The call in words, or `None` when it is not one of SolidCraft's tools or says too little.
/// `title`: the call's title as the agent gave it; `input`: its arguments; `output`: what it
/// returned (text); `done`: it completed (a finished save reads "Saved …", one still running
/// or failed "Save …").
pub fn summary(title: &str, input: Option<&Value>, output: Option<&str>, done: bool, u: Units) -> Option<String> {
    let tool = tool_name(title)?;
    let result = output.filter(|o| o.len() <= MAX_RESULT).and_then(|o| serde_json::from_str::<Value>(o).ok());
    let text = tool_summary(tool, input.and_then(Value::as_object), result.as_ref(), done, u, true)?;
    Some(clip(&text, MAX_SUMMARY))
}

/// SolidCraft's tool a title names: `mcp__<server>__<tool>` (an agent names MCP tools after their
/// server, which for SolidCraft's has `solidcraft` in it), or the in-app client's own `measure`,
/// `execute: solid.box`, `batch: …`.
pub fn tool_name(title: &str) -> Option<&str> {
    let head = title.trim();
    let name = head.split(|c: char| c == ':' || c.is_whitespace()).next()?;
    let parts: Vec<&str> = name.split("__").take(4).collect();
    match parts.as_slice() {
        ["mcp", server, tool] if server.to_ascii_lowercase().contains("solidcraft") && !tool.is_empty() => Some(*tool),
        [bare] if !bare.is_empty() && head.strip_prefix(*bare).is_some_and(|rest| rest.is_empty() || rest.starts_with(':')) => Some(*bare),
        _ => None,
    }
}

/// One tool's words. `nest`: a batch may hold steps (batches don't nest).
fn tool_summary(tool: &str, a: Option<&Map<String, Value>>, result: Option<&Value>, done: bool, u: Units, nest: bool) -> Option<String> {
    let s = |k: &str| a.and_then(|a| a.get(k)).and_then(Value::as_str);
    match tool {
        "execute" => command(s("command")?, a.and_then(|a| a.get("params")).and_then(Value::as_object), result, u),
        "batch" if nest => batch(a?.get("commands")?.as_array()?, done, u),
        "measure" => measure(s("body"), result, done, u),
        "save" => {
            let file = s("path").or_else(|| result.and_then(|r| r.get("path")).and_then(Value::as_str)).and_then(file_name);
            match file {
                Some(f) if done => Some(trf("ui.ai_tool_sum_saved", &[("file", &f)])),
                f => Some(words([Some(label("file.save")?.to_string()), f])),
            }
        }
        "export" => Some(words([Some(label("file.export")?.to_string()), s("path").and_then(file_name)])),
        "open" => Some(words([Some(label("doc.open")?.to_string()), s("path").and_then(file_name)])),
        // The control channel's own calls: the picture is the point, never where it was left.
        "ui.screenshot" | "ui_screenshot" | "screenshot" => Some(tr("ui.ai_tool_sum_screenshot").to_string()),
        "new_design" => Some(words([Some(label("file.new")?.to_string()), s("name").map(word)])),
        "set_parameter" => {
            let value = a.and_then(|a| a.get("value").or_else(|| a.get("expression"))).and_then(|v| length(v, u));
            let what = s("name").map(word).map(|n| match value {
                Some(v) => format!("{n} = {v}"),
                None => n,
            });
            Some(words([Some(label("parameters.change")?.to_string()), what]))
        }
        "inspect_design" => label("document.inspect").map(str::to_string),
        "undo" => label("edit.undo").map(str::to_string),
        "redo" => label("edit.redo").map(str::to_string),
        "crash_reports" => label("diagnostics.crash_reports").map(str::to_string),
        _ => None,
    }
}

/// An engine command: its label, what it works on, its sizes, and the bodies it made.
fn command(id: &str, p: Option<&Map<String, Value>>, result: Option<&Value>, u: Units) -> Option<String> {
    let mut text = label(id)?.to_string();
    if let Some(p) = p {
        let size = size(p, u).or_else(|| p.get("distance").and_then(|v| length(v, u)).map(|d| trf("ui.ai_tool_sum_by", &[("value", &d)])));
        text = words([Some(text), subject(p), size]);
        let mut extras = Vec::new();
        if let Some(r) = p.get("radius").and_then(|v| length(v, u)) {
            extras.push(trf("ui.ai_tool_sum_radius", &[("value", &r)]));
        }
        if let Some(d) = p.get("diameter").and_then(|v| length(v, u)) {
            extras.push(format!("⌀ {d}"));
        }
        if let Some(a) = p.get("angle").and_then(angle) {
            extras.push(a);
        }
        if !extras.is_empty() {
            text = format!("{text} ({})", extras.join(", "));
        }
    }
    let created: Vec<String> = result
        .and_then(|r| r.get("created_bodies"))
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(Value::as_str).take(MAX_BODIES + 1).map(word).collect())
        .unwrap_or_default();
    if !created.is_empty() {
        let shown: Vec<&str> = created.iter().take(MAX_BODIES).map(String::as_str).collect();
        let more = if created.len() > MAX_BODIES { ", …" } else { "" };
        text = format!("{text} → {}{more}", shown.join(", "));
    }
    Some(text)
}

/// A batch: its first steps in order, "A, then B", and how many more.
fn batch(steps: &[Value], done: bool, u: Units) -> Option<String> {
    let mut parts = steps.iter().take(MAX_STEPS).filter_map(|step| {
        let id = step.get("command").or_else(|| step.get("id")).and_then(Value::as_str)?;
        let params = step.get("params").and_then(Value::as_object);
        let text = if id.contains('.') { command(id, params, None, u) } else { tool_summary(id, params, None, done, u, false) };
        Some(text.unwrap_or_else(|| word(id)))
    });
    let mut text = parts.next()?;
    for next in parts {
        text = trf("ui.ai_tool_sum_then", &[("first", &text), ("next", &next)]);
    }
    if steps.len() > MAX_STEPS {
        text = trf("ui.ai_tool_sum_more", &[("steps", &text), ("count", &(steps.len() - MAX_STEPS).to_string())]);
    }
    Some(text)
}

/// `measure`: once done, the body's size and volume, or the bodies' total volume.
fn measure(body: Option<&str>, result: Option<&Value>, done: bool, u: Units) -> Option<String> {
    let plain = || Some(words([Some(label("inspect.measure")?.to_string()), body.map(word)]));
    if !done {
        return plain();
    }
    let bodies = result.and_then(|r| r.get("bodies")).and_then(Value::as_array);
    let what = match bodies.map(Vec::as_slice) {
        Some([one]) => {
            let name = one.get("name").and_then(Value::as_str).or(body).map(word)?;
            let facts: Vec<String> =
                [bbox_size(one, u), one.get("volume_mm3").and_then(Value::as_f64).and_then(|v| volume(v, u))].into_iter().flatten().collect();
            if facts.is_empty() { name } else { format!("{name}: {}", facts.join(", ")) }
        }
        Some(many) if !many.is_empty() => {
            let count = plural(BODIES, many.len());
            match result.and_then(|r| r.get("total")).and_then(|t| t.get("volume_mm3")).and_then(Value::as_f64).and_then(|v| volume(v, u)) {
                Some(v) => format!("{count}: {v}"),
                None => count,
            }
        }
        _ => match body {
            Some(b) => word(b),
            None => return plain(),
        },
    };
    Some(trf("ui.ai_tool_sum_measured", &[("what", &what)]))
}

/// What a command works on: a sketch, body or target by name, or how many edges, faces or bodies.
fn subject(p: &Map<String, Value>) -> Option<String> {
    for k in ["sketch", "body", "target"] {
        if let Some(name) = p.get(k).and_then(Value::as_str) {
            return Some(word(name));
        }
    }
    for (k, key) in [("edges", EDGES), ("faces", FACES), ("bodies", BODIES), ("features", FEATURES)] {
        match p.get(k).and_then(Value::as_array).map(Vec::as_slice) {
            Some([one]) if one.is_string() => return one.as_str().map(word),
            Some(list) if !list.is_empty() => return Some(plural(key, list.len())),
            _ => {}
        }
    }
    None
}

/// "40 × 30 × 20 mm" from two or more of length, width and height.
fn size(p: &Map<String, Value>, u: Units) -> Option<String> {
    let given: Vec<&Value> = ["length", "width", "height"].iter().filter_map(|k| p.get(*k)).collect();
    if given.len() < 2 {
        return None;
    }
    if let Some(mm) = given.iter().map(|v| v.as_f64().filter(|x| x.is_finite())).collect::<Option<Vec<f64>>>() {
        return Some(lengths(&mm, u));
    }
    let each: Vec<String> = given.iter().filter_map(|v| length(v, u)).collect();
    (each.len() == given.len()).then(|| each.join(" × "))
}

/// A body's size along X, Y and Z from its bounding box.
fn bbox_size(body: &Value, u: Units) -> Option<String> {
    let corner = |k: &str| -> Option<Vec<f64>> {
        let a = body.get("bbox")?.get(k)?.as_array()?;
        let v: Vec<f64> = a.iter().take(3).filter_map(Value::as_f64).filter(|x| x.is_finite()).collect();
        (v.len() == 3).then_some(v)
    };
    let (min, max) = (corner("min")?, corner("max")?);
    Some(lengths(&min.iter().zip(&max).map(|(a, b)| (b - a).abs()).collect::<Vec<_>>(), u))
}

/// Millimetre lengths as "40 × 30 × 20 mm" in the design's units.
fn lengths(mm: &[f64], u: Units) -> String {
    let unit = crate::prefs::unit_scale(u.units).1;
    let numbers: Vec<String> = mm.iter().map(|v| compact(&crate::prefs::length_number(*v, u.units, u.decimals))).collect();
    format!("{} {unit}", numbers.join(" × "))
}

/// A length parameter: a number (mm) in the design's units, or the expression as written.
fn length(v: &Value, u: Units) -> Option<String> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(|mm| lengths(&[mm], u)),
        Value::String(s) if !s.trim().is_empty() => Some(word(s)),
        _ => None,
    }
}

/// An angle parameter: a number (degrees), or the expression as written.
fn angle(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(|d| format!("{}°", compact(&format!("{d:.2}")))),
        Value::String(s) if !s.trim().is_empty() => Some(word(s)),
        _ => None,
    }
}

/// A volume in mm³: cm³ for a millimetre design (as people read a part's size), otherwise the
/// design's unit cubed; three significant figures.
fn volume(mm3: f64, u: Units) -> Option<String> {
    if !mm3.is_finite() {
        return None;
    }
    let (scale, unit) = crate::prefs::unit_scale(u.units);
    let (v, unit) = if unit == "mm" { (mm3 / 1000.0, "cm") } else { (mm3 * scale.powi(3), unit) };
    let places = match v.abs() {
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        a if a >= 1.0 => 2,
        _ => 3,
    };
    Some(format!("{} {unit}³", compact(&format!("{v:.places$}"))))
}

/// "10.00" → "10", "2.50" → "2.5", "-0.00" → "0".
fn compact(number: &str) -> String {
    let s = if number.contains('.') { number.trim_end_matches('0').trim_end_matches('.') } else { number };
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

/// A count's messages: one, and more.
const EDGES: (&str, &str) = ("ui.ai_tool_sum_edges_one", "ui.ai_tool_sum_edges_other");
const FACES: (&str, &str) = ("ui.ai_tool_sum_faces_one", "ui.ai_tool_sum_faces_other");
const BODIES: (&str, &str) = ("ui.ai_tool_sum_bodies_one", "ui.ai_tool_sum_bodies_other");
const FEATURES: (&str, &str) = ("ui.ai_tool_sum_features_one", "ui.ai_tool_sum_features_other");

/// "1 edge", "4 edges".
fn plural((one, other): (&str, &str), count: usize) -> String {
    trf(if count == 1 { one } else { other }, &[("count", &count.to_string())])
}

/// A command's label in the interface language.
fn label(id: &str) -> Option<&'static str> {
    solidcraft_engine::find_command(id).map(|c| tr_label(c.label))
}

/// A file's name without its folders; `None` for a temporary file (the agent's scratch, not
/// something to name).
fn file_name(path: &str) -> Option<String> {
    if is_temp(path) {
        return None;
    }
    Some(word(path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path)))
}

/// A path in a temporary folder (`/tmp`, `%TEMP%`, macOS `/var/folders`) or one of SolidCraft's
/// own scratch folders.
fn is_temp(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    ["/tmp/", "/temp/", "/var/folders/", "/appdata/local/temp", "solidcraft-mcp-", "/dev/shm/"].iter().any(|m| p.contains(m))
        || p.ends_with("/tmp")
        || p.ends_with("/temp")
}

/// Whether a word starts an absolute path: a Windows drive (`C:\`, `C:/`), a UNC share (`\\server`),
/// `~` or a Unix path with a folder (`/home/x`, not a lone `/`).
fn path_start(w: &str) -> bool {
    let b = w.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let windows = drive || w.starts_with("\\\\");
    windows || w == "~" || w.starts_with("~/") || (w.starts_with('/') && w[1..].contains(|c: char| c != '/'))
}

/// A card's title with every path cut to its file name and temporary files left out:
/// "ui.screenshot: C:\\Users\\me\\Temp\\shot-0.png" reads "ui.screenshot", "Read /home/me/a.step"
/// "Read a.step". The full title stays in the card's tooltip.
pub fn plain_title(title: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut words = title.split_whitespace().peekable();
    while let Some(first) = words.next() {
        // Quotes and brackets around a path stay out of it.
        let lead = first.len() - first.trim_start_matches(['"', '\'', '(', '[', '`']).len();
        let core = &first[lead..];
        if !path_start(core) {
            out.push(first.to_string());
            continue;
        }
        let mut path = core.to_string();
        let windows = !core.starts_with('/') && !core.starts_with('~');
        // A Windows folder may hold spaces ("Jane Doe"): carry on while the last part has no extension.
        while windows && words.peek().is_some() {
            let last = path.rsplit(['/', '\\']).next().unwrap_or("");
            if last.contains('.') {
                break;
            }
            path.push(' ');
            path.push_str(words.next().unwrap_or_default());
        }
        let trail_at = path.trim_end_matches(['"', '\'', ')', ']', '`', ',', ';', '.', ':']).len();
        let (body, trail) = path.split_at(trail_at);
        let trail = trail.trim_start_matches(['"', '\'', ')', ']', '`']);
        match file_name(body) {
            Some(name) => out.push(format!("{}{name}{trail}", &first[..lead])),
            None => {
                if let Some(prev) = out.last_mut() {
                    // The colon or "to" that led to the path leads nowhere now.
                    if prev.ends_with(':') {
                        prev.pop();
                    }
                }
            }
        }
    }
    let text = out.join(" ");
    clip(&text, MAX_SUMMARY)
}

/// A name or expression from the call on one line, cut short.
fn word(s: &str) -> String {
    let line: String = s.chars().take(MAX_WORD * 4).map(|c| if c.is_control() { ' ' } else { c }).collect();
    clip(line.trim(), MAX_WORD)
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max.saturating_sub(1)).collect::<String>() + "…" }
}

/// The parts that are there, with spaces between.
fn words<const N: usize>(parts: [Option<String>; N]) -> String {
    parts.into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}
