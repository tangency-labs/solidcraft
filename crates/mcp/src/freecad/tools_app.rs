//! Documents, objects, selection, view, tree, workbench, GUI commands, console, preferences,
//! scripting, spreadsheets, assemblies and display.

use serde_json::{Value, json};

use super::{Cx, R, ToolError, b, obj_xyz, req, s, te};

// ---------- objects ----------

/// What a FreeCAD object name stands for here.
#[derive(Clone, Debug)]
pub enum Obj {
    Body(String),
    Feature { id: u64, name: String, kind: String },
    Sketch { id: u64, name: String },
    Component { id: u64, name: String },
    Sheet(String),
}

impl Obj {
    pub fn name(&self) -> &str {
        match self {
            Obj::Body(n) | Obj::Sheet(n) => n,
            Obj::Feature { name, .. } | Obj::Sketch { name, .. } | Obj::Component { name, .. } => name,
        }
    }
}

pub fn inspect(cx: &mut Cx) -> Value {
    cx.b.call("document.inspect", json!({})).unwrap_or_default()
}
/// The active design's name, as `document.inspect` reports it.
pub fn doc_name(cx: &mut Cx) -> String {
    inspect(cx).get("name").and_then(Value::as_str).unwrap_or_default().to_string()
}
/// A timeline feature's name by id, from an `document.inspect` reply already in hand.
pub fn feature_name(ins: &Value, id: u64) -> Option<String> {
    ins.get("timeline")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|f| f.get("id").and_then(Value::as_u64) == Some(id))
        .and_then(|f| f.get("name").and_then(Value::as_str).map(str::to_string))
}
/// One command's row from `engine.commands`, which carries its shortcut and whether it can
/// run on the design as it stands.
fn command_row(cx: &mut Cx, id: &str) -> Value {
    let reply = cx.b.call("engine.commands", json!({})).unwrap_or_default();
    let rows = reply.get("commands").or(Some(&reply)).and_then(Value::as_array).cloned().unwrap_or_default();
    rows.into_iter().find(|r| r.get("id").and_then(Value::as_str) == Some(id)).unwrap_or(Value::Null)
}
pub fn body_names(cx: &mut Cx) -> Vec<String> {
    bodies_json(cx).iter().filter_map(|b| b.get("name").and_then(Value::as_str).map(str::to_string)).collect()
}
pub fn is_mesh(cx: &mut Cx, name: &str) -> bool {
    bodies_json(cx).iter().any(|b| b.get("name").and_then(Value::as_str) == Some(name) && b.get("mesh").and_then(Value::as_bool) == Some(true))
}
/// Every body, as `document.inspect` reports it (`name`, `mesh`, `visible`...).
fn bodies_json(cx: &mut Cx) -> Vec<Value> {
    inspect(cx).get("bodies").and_then(Value::as_array).cloned().unwrap_or_default()
}
/// Whether the design has a parameter by that name.
fn has_param(cx: &mut Cx, name: &str) -> bool {
    inspect(cx).get("params").and_then(Value::as_array).into_iter().flatten().any(|p| p.get("name").and_then(Value::as_str) == Some(name))
}
fn components(cx: &mut Cx) -> Vec<Value> {
    cx.cmd("component.list", json!({})).ok().and_then(|v| v.get("components").cloned()).and_then(|c| c.as_array().cloned()).unwrap_or_default()
}

/// Resolve a FreeCAD object name: a body, a timeline feature or sketch, a component or a
/// spreadsheet (by name, or its position in the timeline as a number).
pub fn resolve(cx: &mut Cx, name: &str, doc: Option<&str>) -> Result<Obj, ToolError> {
    cx.use_doc(doc)?;
    if body_names(cx).iter().any(|b| b == name) && !cx.st.bodies.iter().any(|b| b == name) {
        return Ok(Obj::Body(name.to_string()));
    }
    if cx.st.sheets.contains_key(name) {
        return Ok(Obj::Sheet(name.to_string()));
    }
    let ins = inspect(cx);
    for f in ins.get("timeline").and_then(Value::as_array).into_iter().flatten() {
        if f.get("name").and_then(Value::as_str) == Some(name) {
            let id = f.get("id").and_then(Value::as_u64).unwrap_or(0);
            let kind = f.get("type").and_then(Value::as_str).unwrap_or("").to_string();
            return Ok(if kind == "Sketch" { Obj::Sketch { id, name: name.into() } } else { Obj::Feature { id, name: name.into(), kind } });
        }
    }
    for c in components(cx) {
        if c.get("name").and_then(Value::as_str) == Some(name) {
            return Ok(Obj::Component { id: c.get("id").and_then(Value::as_u64).unwrap_or(0), name: name.into() });
        }
    }
    if body_names(cx).iter().any(|b| b == name) {
        return Ok(Obj::Body(name.to_string()));
    }
    Err(te("no_object", format!("Object '{name}' not found in document '{}'", doc_name(cx))))
}

/// The bodies an object stands for: the body, a feature's bodies, a component's bodies.
pub fn bodies_of(cx: &mut Cx, o: &Obj) -> Vec<String> {
    match o {
        Obj::Body(n) => vec![n.clone()],
        Obj::Feature { id, .. } => {
            let ins = inspect(cx);
            let made: Vec<String> = ins
                .get("bodies")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|b| b.get("feature").and_then(Value::as_u64) == Some(*id))
                .filter_map(|b| b.get("name").and_then(Value::as_str).map(str::to_string))
                .collect();
            if !made.is_empty() {
                return made;
            }
            // a feature that changes bodies (a pocket, a fillet): the bodies of its component
            let comp = components(cx)
                .into_iter()
                .find(|c| c.get("features").and_then(Value::as_array).is_some_and(|fs| fs.iter().any(|f| f.as_str() == Some(o.name()))));
            comp.and_then(|c| c.get("bodies").cloned()).and_then(|b| serde_json::from_value(b).ok()).unwrap_or_default()
        }
        Obj::Component { name, .. } => components(cx)
            .into_iter()
            .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|c| c.get("bodies").cloned())
            .and_then(|b| serde_json::from_value(b).ok())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
/// The one body an object stands for (the first).
pub fn body_of(cx: &mut Cx, name: &str, doc: Option<&str>) -> Result<String, ToolError> {
    let o = resolve(cx, name, doc)?;
    bodies_of(cx, &o).into_iter().next().ok_or_else(|| te("no_shape", format!("Object '{name}' has no Shape")))
}

pub fn type_id(cx: &mut Cx, o: &Obj) -> String {
    match o {
        Obj::Body(n) => if is_mesh(cx, n) { "Mesh::Feature" } else { "Part::Feature" }.into(),
        Obj::Sheet(_) => "Spreadsheet::Sheet".into(),
        Obj::Sketch { .. } => "Sketcher::SketchObject".into(),
        Obj::Component { name, .. } => {
            if cx.st.bodies.contains(name) {
                "PartDesign::Body".into()
            } else if cx.st.assemblies.contains(name) {
                "Assembly::AssemblyObject".into()
            } else {
                "App::Part".into()
            }
        }
        Obj::Feature { kind, .. } => match kind.as_str() {
            "ExtrudeFeature" => "PartDesign::Pad".into(),
            "RevolveFeature" => "PartDesign::Revolution".into(),
            "FilletFeature" => "PartDesign::Fillet".into(),
            "ChamferFeature" => "PartDesign::Chamfer".into(),
            "BoxFeature" => "Part::Box".into(),
            "CylinderFeature" => "Part::Cylinder".into(),
            "SphereFeature" => "Part::Sphere".into(),
            "TorusFeature" => "Part::Torus".into(),
            "CombineFeature" => "Part::Boolean".into(),
            "MoveFeature" => "Part::Transform".into(),
            "MirrorFeature" => "Part::Mirroring".into(),
            "MeshFeature" | "MeshImport" => "Mesh::Feature".into(),
            "LoftFeature" => "Part::Loft".into(),
            other => format!("App::FeaturePython/{other}"),
        },
    }
}

fn visible(cx: &mut Cx, o: &Obj) -> bool {
    let ins = inspect(cx);
    let find = |k: &str, n: &str| {
        ins.get(k)
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(n)).and_then(|x| x.get("visible")).and_then(Value::as_bool))
    };
    match o {
        Obj::Body(n) => find("bodies", n).unwrap_or(true),
        Obj::Sketch { name, .. } => find("sketches", name).unwrap_or(true),
        _ => true,
    }
}

fn serialize(cx: &mut Cx, o: &Obj, props: bool) -> Value {
    let mut out = json!({"Name": o.name(), "Label": o.name(), "TypeId": type_id(cx, o), "Document": doc_name(cx), "Visibility": visible(cx, o)});
    if props {
        let p = match o {
            Obj::Body(n) => {
                let m = cx
                    .cmd("inspect.measure", json!({"bodies": [n]}))
                    .ok()
                    .and_then(|v| v.get("bodies").and_then(|b| b.get(0)).cloned())
                    .unwrap_or_default();
                let look = cx.cmd("appearance.list", json!({"body": n})).unwrap_or_default();
                let place = cx.st.place.get(n).copied().unwrap_or(IDENTITY);
                json!({"Label": n, "Visibility": visible(cx, o), "Shape": m, "Appearance": look, "Placement": placement_json(&place), "Mesh": is_mesh(cx, n)})
            }
            Obj::Feature { name, .. } => cx.cmd("timeline.inspect", json!({"feature": name})).unwrap_or_default(),
            Obj::Sketch { name, .. } => cx.cmd("sketch.inspect", json!({"sketch": name})).unwrap_or_default(),
            Obj::Component { name, .. } => {
                components(cx).into_iter().find(|c| c.get("name").and_then(Value::as_str) == Some(name)).unwrap_or_default()
            }
            Obj::Sheet(n) => json!({"Cells": cx.st.sheets.get(n).cloned().unwrap_or_default()}),
        };
        out["Properties"] = p;
    }
    out
}

// ---------- placements (4×4 row-major) ----------
pub const IDENTITY: [f64; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

pub fn mat_mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut m = [0.0; 16];
    for r in 0..4 {
        for c in 0..4 {
            m[r * 4 + c] = (0..4).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
    }
    m
}
/// Rotation about `axis` (any length) by `deg`, then translation `t`.
pub fn mat_from(axis: [f64; 3], deg: f64, t: [f64; 3]) -> [f64; 16] {
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let (x, y, z) = if l > 1e-12 { (axis[0] / l, axis[1] / l, axis[2] / l) } else { (0., 0., 1.) };
    let (sn, cs) = deg.to_radians().sin_cos();
    let c1 = 1.0 - cs;
    [
        cs + x * x * c1,
        x * y * c1 - z * sn,
        x * z * c1 + y * sn,
        t[0],
        y * x * c1 + z * sn,
        cs + y * y * c1,
        y * z * c1 - x * sn,
        t[1],
        z * x * c1 - y * sn,
        z * y * c1 + x * sn,
        cs + z * z * c1,
        t[2],
        0.,
        0.,
        0.,
        1.,
    ]
}
/// Inverse of a rigid transform.
pub fn mat_inv(m: &[f64; 16]) -> [f64; 16] {
    let rt = [m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]];
    let t = [m[3], m[7], m[11]];
    let it =
        [-(rt[0] * t[0] + rt[1] * t[1] + rt[2] * t[2]), -(rt[3] * t[0] + rt[4] * t[1] + rt[5] * t[2]), -(rt[6] * t[0] + rt[7] * t[1] + rt[8] * t[2])];
    [rt[0], rt[1], rt[2], it[0], rt[3], rt[4], rt[5], it[1], rt[6], rt[7], rt[8], it[2], 0., 0., 0., 1.]
}
/// Axis (unit), angle (degrees) and translation of a rigid transform.
pub fn mat_parts(m: &[f64; 16]) -> ([f64; 3], f64, [f64; 3]) {
    let tr = m[0] + m[5] + m[10];
    let ang = ((tr - 1.0) / 2.0).clamp(-1.0, 1.0).acos();
    let mut axis = [m[9] - m[6], m[2] - m[8], m[4] - m[1]];
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if l > 1e-9 {
        axis = [axis[0] / l, axis[1] / l, axis[2] / l];
    } else if ang > 1.0 {
        // a half turn: the axis from the diagonal
        let d = [((m[0] + 1.0) / 2.0).max(0.0).sqrt(), ((m[5] + 1.0) / 2.0).max(0.0).sqrt(), ((m[10] + 1.0) / 2.0).max(0.0).sqrt()];
        axis = [d[0], if m[1] < 0.0 { -d[1] } else { d[1] }, if m[2] < 0.0 { -d[2] } else { d[2] }];
    } else {
        axis = [0., 0., 1.];
    }
    (axis, ang.to_degrees(), [m[3], m[7], m[11]])
}
pub fn placement_json(m: &[f64; 16]) -> Value {
    let (axis, deg, t) = mat_parts(m);
    json!({"Base": obj_xyz(t), "Rotation": {"Axis": obj_xyz(axis), "Angle": deg}})
}

/// Move bodies by a rigid transform: a rotation about the origin, then a translation (each a
/// Move feature), and track their placements.
pub fn apply_transform(cx: &mut Cx, bodies: &[String], m: &[f64; 16]) -> Result<(), ToolError> {
    cx.leave_sketch();
    let (axis, deg, t) = mat_parts(m);
    if deg.abs() > 1e-9 {
        cx.run("solid.move", json!({"bodies": bodies, "axis": axis, "angle": format!("{deg} deg")}))?;
    }
    if t.iter().any(|x| x.abs() > 1e-12) {
        cx.run("solid.move", json!({"bodies": bodies, "translate": t}))?;
    }
    for name in bodies {
        let old = cx.st.place.get(name).copied().unwrap_or(IDENTITY);
        cx.st.place.insert(name.clone(), mat_mul(m, &old));
    }
    Ok(())
}

// ---------- the tools ----------

fn doc_arg(a: &Value) -> Option<&str> {
    s(a, "doc")
}

fn doc_entry(cx: &mut Cx, i: usize, name: &str, dirty: bool, active: bool) -> Value {
    let _ = i;
    let (file, count) = if active {
        let ins = inspect(cx);
        (ins.get("path").cloned().unwrap_or(Value::Null), json!(ins.get("bodies").and_then(Value::as_array).map(Vec::len).unwrap_or(0)))
    } else {
        (Value::Null, Value::Null)
    };
    json!({"Name": name, "Label": name, "FileName": file.as_str().unwrap_or(""), "Modified": dirty, "ObjectCount": count, "Active": active})
}

fn console_level(error: bool) -> &'static str {
    if error { "error" } else { "message" }
}

/// FreeCAD view orientation names → SolidCraft views.
fn view_name(o: &str) -> Option<&'static str> {
    Some(match o {
        "front" => "front",
        "back" => "back",
        "left" => "left",
        "right" => "right",
        "top" => "top",
        "bottom" => "bottom",
        "iso" | "isometric" | "axo" | "axonometric" | "dimetric" | "trimetric" | "default" => "iso",
        _ => return None,
    })
}

/// An Inventor camera string as FreeCAD's view_get_camera gives, with SolidCraft's own camera on
/// a comment line so view_set_camera restores it exactly.
fn camera_string(cam: &Value) -> String {
    let g = |k: &str| cam.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let t: Vec<f64> =
        cam.get("target").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_else(|| vec![0.0; 3]);
    let (yaw, pitch, dist) = (g("yaw"), g("pitch"), g("distance"));
    // yaw 0 looks along +Y; the eye is behind the target along the view direction
    let dir = [yaw.sin() * pitch.cos(), yaw.cos() * pitch.cos(), -pitch.sin()];
    let pos = [
        t.first().copied().unwrap_or(0.0) - dir[0] * dist,
        t.get(1).copied().unwrap_or(0.0) - dir[1] * dist,
        t.get(2).copied().unwrap_or(0.0) - dir[2] * dist,
    ];
    let ortho = g("fov") <= 0.0;
    format!(
        "#Inventor V2.1 ascii\n\n# solidcraft {}\n{} {{\n  viewportMapping ADJUST_CAMERA\n  position {} {} {}\n  orientation 0 0 1  {}\n  nearDistance {}\n  farDistance {}\n  aspectRatio 1\n  focalDistance {}\n  {}\n\n}}\n",
        cam,
        if ortho { "OrthographicCamera" } else { "PerspectiveCamera" },
        pos[0],
        pos[1],
        pos[2],
        yaw,
        dist * 0.01,
        dist * 100.0,
        dist,
        if ortho { format!("height {}", 2.0 * dist * (0.35f64).tan()) } else { format!("heightAngle {}", g("fov").to_radians()) }
    )
}
fn parse_camera(s: &str) -> Result<Value, ToolError> {
    if let Some(line) = s.lines().find_map(|l| l.trim().strip_prefix("# solidcraft ")) {
        let mut v: Value = serde_json::from_str(line).map_err(|e| te("bad_camera", e.to_string()))?;
        if let Some(o) = v.as_object_mut() {
            o.retain(|k, _| matches!(k.as_str(), "target" | "yaw" | "pitch" | "roll" | "distance"));
        }
        return Ok(v);
    }
    // a FreeCAD camera: position, focalDistance and the orientation's view direction
    let nums = |k: &str| -> Option<Vec<f64>> {
        let l = s.lines().find(|l| l.trim_start().starts_with(k))?;
        Some(l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect())
    };
    let pos = nums("position").filter(|p| p.len() >= 3).ok_or_else(|| te("bad_camera", "camera needs a position"))?;
    let o = nums("orientation").filter(|p| p.len() >= 4).unwrap_or_else(|| vec![0., 0., 1., 0.]);
    let focal = nums("focalDistance").and_then(|v| v.first().copied()).unwrap_or(100.0);
    // the camera looks along its -Z, rotated by the orientation
    let m = mat_from([o[0], o[1], o[2]], o[3].to_degrees(), [0.0; 3]);
    let dir = [-m[2], -m[6], -m[10]];
    let target = [pos[0] + dir[0] * focal, pos[1] + dir[1] * focal, pos[2] + dir[2] * focal];
    Ok(json!({"target": target, "yaw": dir[0].atan2(dir[1]), "pitch": (-dir[2]).clamp(-1.0, 1.0).asin(), "distance": focal}))
}

const SC_GROUP: &str = "User parameter:BaseApp/Preferences/SolidCraft";

fn pref_coerce(ty: &str, v: &Value) -> Value {
    match ty {
        "bool" => json!(v.as_bool().unwrap_or_else(|| v.as_str() == Some("true") || v.as_f64().is_some_and(|x| x != 0.0))),
        "int" | "uint" => json!(v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0).trunc() as i64),
        "float" => json!(v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)),
        _ => json!(v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())),
    }
}
fn pref_default(ty: &str) -> Value {
    match ty {
        "bool" => json!(false),
        "string" => json!(""),
        "float" => json!(0.0),
        _ => json!(0),
    }
}

/// Spreadsheet cell parameter name.
fn cell_param(cx: &Cx, sheet: &str, cell: &str) -> String {
    cx.st
        .sheets
        .get(sheet)
        .and_then(|m| m.get(&cell.to_ascii_uppercase()))
        .cloned()
        .unwrap_or_else(|| format!("{sheet}_{}", cell.to_ascii_uppercase()))
}
/// A FreeCAD cell value as a parameter expression: `=A1*2` refers to other cells.
fn cell_expression(cx: &Cx, sheet: &str, value: &str) -> String {
    let Some(expr) = value.trim().strip_prefix('=') else { return value.trim().to_string() };
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let is_cell = word.len() >= 2 && word.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && {
            let letters = word.chars().take_while(char::is_ascii_alphabetic).count();
            letters <= 2 && word[letters..].chars().all(|c| c.is_ascii_digit()) && letters < word.len()
        };
        if is_cell {
            out.push_str(&cell_param(cx, sheet, word));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for ch in expr.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}
fn sheet_of<'a>(cx: &Cx, name: &'a str) -> Result<&'a str, ToolError> {
    if cx.st.sheets.contains_key(name) { Ok(name) } else { Err(te("no_object", format!("Spreadsheet '{name}' not found"))) }
}

/// Run a SolidCraft command script: JSON ([{command, params}] or {commands}) or one command per
/// line (`id {json}`, `#` comments).
fn run_script(cx: &mut Cx, code: &str) -> (bool, String, Option<String>) {
    let mut steps: Vec<(String, Value)> = Vec::new();
    let trimmed = code.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        let list = v.get("commands").cloned().unwrap_or(v);
        for st in list.as_array().cloned().unwrap_or_default() {
            if let Some(c) = st.get("command").and_then(Value::as_str) {
                steps.push((c.to_string(), st.get("params").cloned().unwrap_or(json!({}))));
            }
        }
    } else {
        for (n, line) in trimmed.lines().enumerate() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with("//") {
                continue;
            }
            let (cmd, rest) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            let params = if rest.trim().is_empty() {
                json!({})
            } else {
                match serde_json::from_str(rest.trim()) {
                    Ok(p) => p,
                    Err(e) => return (false, String::new(), Some(format!("line {}: bad JSON: {e}", n + 1))),
                }
            };
            steps.push((cmd.to_string(), params));
        }
    }
    let mut out = String::new();
    for (cmd, params) in steps {
        match cx.cmd(&cmd, params) {
            Ok(v) => out.push_str(&format!("{cmd}: {v}\n")),
            Err(e) => return (false, out, Some(format!("{cmd}: {e}"))),
        }
    }
    (true, out, None)
}

fn eval_expression(cx: &mut Cx, e: &str) -> Result<Value, ToolError> {
    use solidcraft_engine::doc::expr;
    // `document.inspect` reports a parameter's value in millimetres, or degrees for an angle
    // (#10812); the expression evaluator works in radians, so an angle is converted here.
    let vals: std::collections::BTreeMap<String, expr::Value> = inspect(cx)
        .get("params")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?.to_string();
            let v = p.get("value")?.as_f64()?;
            let q = match p.get("unit").and_then(Value::as_str).unwrap_or("") {
                "deg" | "rad" => expr::Value { v: v.to_radians(), len: 0, ang: 1 },
                "" | "num" | "-" => expr::Value::num(v),
                _ => expr::Value::length(v),
            };
            Some((name, q))
        })
        .collect();
    let lookup = |n: &str| vals.get(n).copied().ok_or_else(|| solidcraft_engine::doc::DocError::Unknown(format!("parameter `{n}`")));
    let v = expr::eval_with(e, &lookup).map_err(|err| te("eval_failed", format!("Eval failed: {err}")))?;
    Ok(if v.len == 0 && v.ang == 0 {
        json!(v.v)
    } else if v.ang == 1 && v.len == 0 {
        json!({"value": v.v.to_degrees(), "unit": "deg"})
    } else if v.len == 1 && v.ang == 0 {
        json!({"value": v.v, "unit": "mm"})
    } else {
        json!({"value": v.v, "length_power": v.len, "angle_power": v.ang})
    })
}

/// The tools of this file; `None` for a name that isn't one of them.
pub fn call(cx: &mut Cx, name: &str, a: &Value) -> Option<R> {
    Some(match name {
        // ======================= documents =======================
        "doc_list" => (|| {
            let (tabs, act) = cx.docs()?;
            let rows: Vec<Value> = tabs
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let n = t.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let dirty = t.get("dirty").and_then(Value::as_bool).unwrap_or(false);
                    doc_entry(cx, i, &n, dirty, i == act || tabs.len() <= 1)
                })
                .collect();
            Ok(json!(rows))
        })(),
        "doc_new" => (|| {
            cx.leave_sketch();
            cx.run("file.new", json!({"name": s(a, "name")}))?;
            if let Some(n) = s(a, "name") {
                let _ = n;
            }
            let n = doc_name(cx);
            Ok(json!({"Name": n, "Label": n}))
        })(),
        "doc_open" => (|| {
            let path = req(a, "path")?;
            cx.leave_sketch();
            cx.run("file.new", json!({}))?;
            cx.run("doc.open", json!({"path": path}))?;
            let n = doc_name(cx);
            Ok(json!({"Name": n, "Label": n, "FileName": path}))
        })(),
        "doc_save" => (|| {
            cx.use_doc(s(a, "name"))?;
            if inspect(cx).get("path").and_then(Value::as_str).is_none() {
                return Err(te("no_path", "Document has no file path; use doc_save_as"));
            }
            cx.leave_sketch();
            let r = cx.run("file.save", json!({}))?;
            Ok(json!({"Name": doc_name(cx), "FileName": r.get("path").cloned().unwrap_or(Value::Null)}))
        })(),
        "doc_save_as" => (|| {
            cx.use_doc(s(a, "name"))?;
            cx.leave_sketch();
            let path = req(a, "path")?;
            cx.run("file.save_as", json!({"path": path}))?;
            Ok(json!({"Name": doc_name(cx), "FileName": path}))
        })(),
        "doc_close" => (|| {
            cx.use_doc(s(a, "name"))?;
            let closed = doc_name(cx);
            if b(a, "save", false) && inspect(cx).get("path").and_then(Value::as_str).is_some() {
                cx.run("file.save", json!({}))?;
            }
            cx.ctl("ui.documents", json!({"action": "close", "force": true}))?;
            Ok(json!({"closed": closed}))
        })(),
        "doc_set_active" => (|| {
            let n = req(a, "name")?;
            cx.use_doc(Some(n))?;
            Ok(json!({"active": n}))
        })(),
        "doc_recompute" => (|| {
            cx.use_doc(s(a, "name"))?;
            cx.leave_sketch();
            cx.run("timeline.compute_all", json!({}))?;
            Ok(json!({"recomputed_objects": inspect(cx).get("timeline").and_then(Value::as_array).map(Vec::len).unwrap_or(0)}))
        })(),
        "doc_undo" => cx.run("edit.undo", json!({})).map(|_| json!({"ok": true})),
        "doc_redo" => cx.run("edit.redo", json!({})).map(|_| json!({"ok": true})),
        "doc_get_properties" => (|| {
            cx.use_doc(s(a, "name"))?;
            let ins = inspect(cx);
            let meta = cx.cmd("document.metadata", json!({})).unwrap_or_default();
            Ok(json!({
                "Label": doc_name(cx), "Name": doc_name(cx),
                "FileName": ins.get("path").cloned().unwrap_or(Value::Null), "Units": ins.get("units"),
                "Modified": ins.get("dirty"), "Metadata": meta, "Parameters": ins.get("params"),
                "Bodies": ins.get("bodies").and_then(Value::as_array).map(Vec::len), "Features": ins.get("timeline").and_then(Value::as_array).map(Vec::len),
                "UndoSteps": ins.get("undo"), "RedoSteps": ins.get("redo"),
            }))
        })(),

        // ======================= objects =======================
        "obj_list" => (|| {
            cx.use_doc(doc_arg(a))?;
            let filter = s(a, "type_filter").unwrap_or("");
            let mut out = Vec::new();
            let mut objs: Vec<Obj> = Vec::new();
            for c in components(cx) {
                if c.get("parent").is_some_and(|p| !p.is_null())
                    || cx.st.bodies.iter().chain(&cx.st.assemblies).any(|n| Some(n.as_str()) == c.get("name").and_then(Value::as_str))
                {
                    objs.push(Obj::Component {
                        id: c.get("id").and_then(Value::as_u64).unwrap_or(0),
                        name: c.get("name").and_then(Value::as_str).unwrap_or("").into(),
                    });
                }
            }
            let ins = inspect(cx);
            for f in ins.get("timeline").and_then(Value::as_array).into_iter().flatten() {
                let (id, n, k) = (
                    f.get("id").and_then(Value::as_u64).unwrap_or(0),
                    f.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    f.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                );
                objs.push(if k == "Sketch" { Obj::Sketch { id, name: n } } else { Obj::Feature { id, name: n, kind: k } });
            }
            for n in body_names(cx) {
                objs.push(Obj::Body(n));
            }
            for n in cx.st.sheets.keys().cloned().collect::<Vec<_>>() {
                objs.push(Obj::Sheet(n));
            }
            for o in objs {
                let v = serialize(cx, &o, false);
                if v["TypeId"].as_str().is_some_and(|t| t.contains(filter)) {
                    out.push(v);
                }
            }
            Ok(json!(out))
        })(),
        "obj_get" => (|| {
            let o = resolve(cx, req(a, "name")?, doc_arg(a))?;
            Ok(serialize(cx, &o, true))
        })(),
        "obj_set_property" => (|| {
            let name = req(a, "name")?;
            let prop = req(a, "property")?;
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            let o = resolve(cx, name, doc_arg(a))?;
            cx.leave_sketch();
            match prop {
                "Label" => rename(cx, &o, value.as_str().unwrap_or(""))?,
                "Visibility" => set_visible(cx, &o, value.as_bool().unwrap_or(true))?,
                "ShapeColor" | "Color" | "DiffuseColor" => {
                    let color = match &value {
                        Value::Array(c) => {
                            let k = if c.iter().filter_map(Value::as_f64).all(|x| x <= 1.0) { 255.0 } else { 1.0 };
                            json!(c.iter().take(3).map(|x| (x.as_f64().unwrap_or(0.0) * k).round()).collect::<Vec<_>>())
                        }
                        v => v.clone(),
                    };
                    let bodies = bodies_of(cx, &o);
                    cx.run("appearance.assign", json!({"bodies": bodies, "color": color}))?;
                }
                "Placement" => {
                    let p = &value;
                    let base = super::xyz(p.get("Base"), [0.0; 3]);
                    let rot = p.get("Rotation");
                    let axis = super::xyz(rot.and_then(|r| r.get("Axis")), [0., 0., 1.]);
                    let deg = rot.and_then(|r| r.get("Angle")).and_then(Value::as_f64).unwrap_or(0.0);
                    let list = bodies_of(cx, &o);
                    set_placement(cx, &list, &mat_from(axis, deg, base))?;
                }
                _ => {
                    // a feature input (Length, Radius…): change the parameter behind it
                    let info = cx
                        .run("timeline.inspect", json!({"feature": o.name()}))
                        .map_err(|_| te("no_property", format!("Object '{name}' has no property '{prop}'")))?;
                    let input = info.get("inputs").and_then(Value::as_array).and_then(|ins| {
                        ins.iter().find(|i| {
                            i.get("label")
                                .and_then(Value::as_str)
                                .is_some_and(|l| l.eq_ignore_ascii_case(prop) || l.replace(' ', "").eq_ignore_ascii_case(prop))
                        })
                    });
                    let Some(param) = input.and_then(|i| i.get("param")).and_then(Value::as_str) else {
                        return Err(te("no_property", format!("Object '{name}' has no property '{prop}'")));
                    };
                    let expr = match &value {
                        Value::String(s) => s.clone(),
                        v => v.to_string(),
                    };
                    cx.run("parameters.change", json!({"name": param, "expression": expr}))?;
                }
            }
            Ok(json!({"ok": true, "value": value}))
        })(),
        "obj_delete" => (|| {
            let name = req(a, "name")?;
            let o = resolve(cx, name, doc_arg(a))?;
            cx.leave_sketch();
            match &o {
                Obj::Body(n) => drop(cx.run("selection.delete", json!({"items": [{"type": "body", "name": n}]}))?),
                Obj::Feature { name, .. } | Obj::Sketch { name, .. } => drop(cx.run("timeline.delete", json!({"feature": name}))?),
                Obj::Component { id, .. } => drop(cx.run("component.delete", json!({"component": id}))?),
                Obj::Sheet(n) => {
                    let params: Vec<String> = cx.st.sheets.remove(n).unwrap_or_default().into_values().collect();
                    for p in params {
                        let _ = cx.cmd("parameters.delete", json!({"name": p}));
                    }
                }
            }
            Ok(json!({"deleted": name, "doc": doc_name(cx)}))
        })(),
        "obj_rename" => (|| {
            let o = resolve(cx, req(a, "name")?, doc_arg(a))?;
            let label = req(a, "label")?;
            cx.leave_sketch();
            rename(cx, &o, label)?;
            Ok(json!({"name": label, "label": label}))
        })(),
        "obj_set_visibility" => (|| {
            let o = resolve(cx, req(a, "name")?, doc_arg(a))?;
            let v = b(a, "visible", true);
            set_visible(cx, &o, v)?;
            Ok(json!({"name": o.name(), "visible": v}))
        })(),
        "obj_shape_info" => (|| {
            let name = req(a, "name")?;
            let body = body_of(cx, name, doc_arg(a))?;
            let m = cx.run("inspect.measure", json!({"bodies": [body]}))?;
            let m = m.get("bodies").and_then(|b| b.get(0)).cloned().unwrap_or_default();
            let edges = cx.cmd("model.edges", json!({"body": body})).ok();
            let length: f64 = edges
                .as_ref()
                .and_then(|e| e.get("edges"))
                .and_then(Value::as_array)
                .map(|e| e.iter().filter_map(|x| x.get("length").and_then(Value::as_f64)).sum())
                .unwrap_or(0.0);
            let bb = m.get("bbox").cloned().unwrap_or_default();
            let c = |k: &str, i: usize| bb.get(k).and_then(|v| v.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
            Ok(json!({
                "Name": name, "isValid": true, "isClosed": !is_mesh(cx, &body),
                "Volume": m.get("volume_mm3"), "Area": m.get("area_mm2"), "Length": length,
                "Vertexes": m.get("vertices"), "Edges": m.get("edges"), "Faces": m.get("faces"),
                "BoundBox": {"XMin": c("min", 0), "YMin": c("min", 1), "ZMin": c("min", 2), "XMax": c("max", 0), "YMax": c("max", 1), "ZMax": c("max", 2),
                             "XLength": c("max", 0) - c("min", 0), "YLength": c("max", 1) - c("min", 1), "ZLength": c("max", 2) - c("min", 2)},
            }))
        })(),

        // ======================= selection =======================
        "selection_get" => {
            let mut by: Vec<(String, Vec<String>)> = Vec::new();
            let ins = inspect(cx);
            let sel = ins.get("selection").cloned().unwrap_or_default();
            for it in sel.as_array().into_iter().flatten() {
                let ty = it.get("type").and_then(Value::as_str).unwrap_or("");
                let (obj, sub) = match ty {
                    "body" => (it.get("name").and_then(Value::as_str).unwrap_or("").to_string(), None),
                    "face" => (
                        it.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                        it.get("index").and_then(Value::as_u64).map(|i| format!("Face{}", i + 1)),
                    ),
                    "edge" => (
                        it.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
                        it.get("index").and_then(Value::as_u64).map(|i| format!("Edge{}", i + 1)),
                    ),
                    "feature" => {
                        let id = it.get("id").and_then(Value::as_u64);
                        let name = id.and_then(|id| feature_name(&ins, id)).unwrap_or_default();
                        (name, None)
                    }
                    _ => continue,
                };
                match by.iter_mut().find(|(o, _)| *o == obj) {
                    Some((_, subs)) => subs.extend(sub),
                    None => by.push((obj, sub.into_iter().collect())),
                }
            }
            let doc = doc_name(cx);
            Ok(json!(
                by.into_iter()
                    .map(|(o, subs)| {
                        let t = resolve(cx, &o, None).map(|x| type_id(cx, &x)).unwrap_or_default();
                        json!({"Document": doc, "Object": o, "Label": o, "TypeId": t, "SubElementNames": subs})
                    })
                    .collect::<Vec<_>>()
            ))
        }
        "selection_clear" => cx.run("select.clear", json!({})).map(|_| json!({"ok": true})),
        "selection_add" => (|| {
            let name = req(a, "object")?;
            let o = resolve(cx, name, doc_arg(a))?;
            let item = match s(a, "sub") {
                Some(sub) => {
                    let body = bodies_of(cx, &o).into_iter().next().ok_or_else(|| te("no_shape", format!("'{name}' has no shape")))?;
                    super::tools_model::sub_item(cx, &body, sub)?
                }
                None => match &o {
                    Obj::Body(n) => json!({"type": "body", "name": n}),
                    Obj::Feature { id, .. } | Obj::Sketch { id, .. } => json!({"type": "feature", "id": id}),
                    other => {
                        let bodies = bodies_of(cx, other);
                        let items: Vec<Value> = bodies.iter().map(|n| json!({"type": "body", "name": n})).collect();
                        return cx.run("select.set", json!({"items": items, "add": true})).map(|_| json!({"ok": true}));
                    }
                },
            };
            cx.run("select.set", json!({"items": [item], "add": true}))?;
            Ok(json!({"ok": true}))
        })(),
        "selection_remove" => (|| {
            let name = req(a, "object")?;
            let sel = inspect(cx).get("selection").cloned().unwrap_or_default();
            let keep: Vec<Value> = sel
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|it| it.get("name").and_then(Value::as_str) != Some(name) && it.get("body").and_then(Value::as_str) != Some(name))
                .collect();
            if keep.is_empty() {
                cx.run("select.clear", json!({}))?;
            } else {
                cx.run("select.set", json!({"items": keep}))?;
            }
            Ok(json!({"ok": true}))
        })(),

        // ======================= view =======================
        "view_set_orientation" => (|| {
            let o = req(a, "orientation")?.to_ascii_lowercase();
            let v = view_name(&o).ok_or_else(|| te("bad_orientation", format!("Unknown orientation: {o}")))?;
            cx.ctl("ui.view", json!({"view": v}))?;
            Ok(json!({"orientation": o}))
        })(),
        "view_fit_all" => cx.ctl("ui.camera", json!({"fit": true})).map(|_| json!({"ok": true})),
        "view_fit_selection" => (|| {
            let bodies: Vec<String> = inspect(cx)
                .get("selection")
                .cloned()
                .unwrap_or_default()
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|it| it.get("name").or_else(|| it.get("body")).and_then(Value::as_str).map(str::to_string))
                .collect();
            if bodies.is_empty() {
                cx.ctl("ui.camera", json!({"fit": true})).map(|_| json!({"ok": true}))
            } else {
                cx.ctl("ui.view", json!({"view": "fit", "animate": true}))?;

                Ok(json!({"ok": true}))
            }
        })(),
        "view_screenshot" => (|| {
            let w = a.get("width").and_then(Value::as_u64).unwrap_or(1024);
            let h = a.get("height").and_then(Value::as_u64).unwrap_or(768);
            let format = s(a, "format").unwrap_or("PNG").to_string();
            let path = match s(a, "path") {
                Some(p) => p.to_string(),
                None => std::env::temp_dir().join(format!("solidcraft_aibridge_{}.png", std::process::id())).to_string_lossy().into_owned(),
            };
            cx.ctl("ui.render", json!({"path": path, "width": w, "height": h}))?;
            let mut out = json!({"path": path, "width": w, "height": h, "format": format});
            if b(a, "return_base64", true)
                && let Ok(bytes) = std::fs::read(&path)
            {
                out["image_base64"] = json!(base64(&bytes));
            }
            Ok(out)
        })(),
        "view_get_camera" => cx.ctl("ui.camera", json!({})).map(|c| json!({"camera": camera_string(&c)})),
        "view_set_camera" => (|| {
            let cam = parse_camera(req(a, "camera")?)?;
            cx.ctl("ui.camera", cam)?;
            Ok(json!({"ok": true}))
        })(),
        "view_redraw" => {
            // nothing to ask for: the app redraws on its own.
            Ok(json!({"ok": true}))
        }

        // ======================= tree =======================
        "tree_get" => (|| {
            cx.use_doc(doc_arg(a))?;
            let depth = a.get("max_depth").and_then(Value::as_u64).unwrap_or(8);
            let comps = components(cx);
            fn node(cx: &mut Cx, comps: &[Value], c: &Value, depth: u64) -> Value {
                let name = c.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let o = Obj::Component { id: c.get("id").and_then(Value::as_u64).unwrap_or(0), name: name.clone() };
                let mut n = json!({"Name": name, "Label": name, "TypeId": type_id(cx, &o), "Visible": true});
                if depth > 0 {
                    let mut kids = Vec::new();
                    for k in ["sketches", "features"] {
                        for f in c.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                            if k == "features" && c.get("sketches").and_then(Value::as_array).is_some_and(|s| s.iter().any(|x| x.as_str() == Some(f)))
                            {
                                continue;
                            }
                            if let Ok(o) = resolve(cx, f, None) {
                                kids.push(serialize(cx, &o, false));
                            }
                        }
                    }
                    for bname in c.get("bodies").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                        kids.push(serialize(cx, &Obj::Body(bname.into()), false));
                    }
                    let id = c.get("id").and_then(Value::as_u64);
                    for child in comps.iter().filter(|x| x.get("parent").and_then(Value::as_u64) == id && id.is_some()) {
                        kids.push(node(cx, comps, child, depth - 1));
                    }
                    if !kids.is_empty() {
                        n["Children"] = json!(kids);
                    }
                }
                n
            }
            let roots: Vec<Value> = comps.iter().filter(|c| c.get("parent").is_none_or(Value::is_null)).cloned().collect();
            Ok(json!(roots.iter().map(|r| node(cx, &comps, r, depth)).collect::<Vec<_>>()))
        })(),
        "tree_expand" => (|| {
            // The object still has to exist, as it does in FreeCAD, even though what follows
            // expands the whole tree rather than that one row.
            let _ = resolve(cx, req(a, "object")?, doc_arg(a))?;
            let exp = b(a, "expanded", true);
            if exp {
                cx.run("browser.expand_all", json!({}))?;
            } else {
                cx.run("browser.collapse_all", json!({}))?;
            }
            Ok(json!({"ok": true, "expanded": exp, "note": "best-effort; actual TreeView state is implementation-dependent"}))
        })(),

        // ======================= workbench =======================
        "workbench_list" => Ok(json!(["DesignWorkspace", "RenderWorkspace"])),
        "workbench_active" => (|| {
            let render = cx.ctl("ui.workspace", json!({}))?.get("workspace").and_then(Value::as_str) == Some("render");
            Ok(json!({"name": if render { "RenderWorkspace" } else { "DesignWorkspace" }, "menuText": if render { "Render" } else { "Design" }}))
        })(),
        "workbench_activate" => (|| {
            let n = req(a, "name")?;
            let w = match n {
                "DesignWorkspace" | "PartDesignWorkbench" | "PartWorkbench" | "SketcherWorkbench" | "PlasticityWorkbench" => "design",
                "RenderWorkspace" => "render",
                _ => return Err(te("unknown_workbench", format!("Unknown workbench: {n}"))),
            };
            cx.ctl("ui.workspace", json!({"workspace": w}))?;
            Ok(json!({"active": n}))
        })(),

        // ======================= gui commands =======================
        "gui_list_commands" => {
            let filt = s(a, "filter").unwrap_or("").to_lowercase();
            let mut specs: Vec<_> = solidcraft_engine::command_specs()
                .into_iter()
                .filter(|c| filt.is_empty() || c.id.to_lowercase().contains(&filt) || c.label.to_lowercase().contains(&filt))
                .collect();
            specs.sort_by_key(|c| c.id);
            Ok(if b(a, "with_info", false) {
                json!(specs.iter().map(|c| json!({"name": c.id, "menuText": c.label, "toolTip": c.params})).collect::<Vec<_>>())
            } else {
                json!(specs.iter().map(|c| c.id).collect::<Vec<_>>())
            })
        }
        "gui_run_command" => (|| {
            let n = req(a, "name")?;
            let spec = solidcraft_engine::find_command(n).ok_or_else(|| te("unknown_command", format!("Unknown GUI command: {n}")))?;
            cx.ctl("ui.start", json!({"command": spec.id}))?;
            Ok(json!({"ran": spec.id, "item": a.get("item").and_then(Value::as_i64).unwrap_or(0)}))
        })(),
        "gui_command_info" => (|| {
            let n = req(a, "name")?;
            let c = solidcraft_engine::find_command(n).ok_or_else(|| te("command_info_failed", format!("Cannot get info for '{n}'")))?;
            let row = command_row(cx, c.id);
            let key = row.get("shortcut").and_then(Value::as_str).unwrap_or_default().to_string();
            let enabled = row.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            Ok(
                json!({"menuText": c.label, "toolTip": c.params, "whatsThis": c.id, "statusTip": c.label, "pixmap": c.icon, "shortcut": key, "enabled": enabled}),
            )
        })(),

        // ======================= console =======================
        "console_tail" => (|| {
            let limit = a.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 2000) as usize;
            let level = s(a, "level").unwrap_or("all");
            let mut items: Vec<Value> = cx
                .ctl("ui.messages", json!({}))?
                .get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|m| {
                    let error = m.get("error").and_then(Value::as_bool).unwrap_or(false);
                    let text = m.get("text").and_then(Value::as_str).unwrap_or("");
                    json!({"level": console_level(error), "message": format!("{text}\n")})
                })
                .collect();
            items.extend(cx.st.console.iter().map(|(l, m)| json!({"level": l, "message": m})));
            let items: Vec<Value> = items.into_iter().filter(|m| level == "all" || m["level"] == level).collect();
            let n = items.len();
            Ok(json!(items.into_iter().skip(n.saturating_sub(limit)).collect::<Vec<_>>()))
        })(),
        "console_clear" => (|| {
            cx.ctl("ui.messages", json!({"clear": true}))?;
            cx.st.console.clear();
            Ok(json!({"ok": true}))
        })(),
        "console_print" => {
            let msg = s(a, "message").unwrap_or("").to_string();
            let level = s(a, "level").unwrap_or("message").to_string();
            if matches!(level.as_str(), "message" | "error" | "warning") {
            } else {
                cx.st.console.push((level, if msg.ends_with('\n') { msg } else { format!("{msg}\n") }));
            }
            Ok(json!({"ok": true}))
        }

        // ======================= preferences =======================
        "pref_get" => (|| {
            let (group, key, ty) = (req(a, "group")?, req(a, "key")?, req(a, "type")?);
            let dflt = a.get("default").cloned().filter(|d| !d.is_null()).unwrap_or_else(|| pref_default(ty));
            if group.trim_end_matches('/') == SC_GROUP {
                let all = cx.prefs(json!({}))?;
                return Ok(all.get(key).map(|v| pref_coerce(ty, v)).unwrap_or(dflt));
            }
            Ok(cx.st.params.get(group).and_then(|g| g.get(key)).map(|v| pref_coerce(ty, v)).unwrap_or(dflt))
        })(),
        "pref_set" => (|| {
            let (group, key, ty) = (req(a, "group")?, req(a, "key")?, req(a, "type")?);
            let v = pref_coerce(ty, a.get("value").unwrap_or(&Value::Null));
            if group.trim_end_matches('/') == SC_GROUP {
                cx.prefs(json!({ key: v }))?;
            } else {
                cx.st.params.entry(group.to_string()).or_default().insert(key.to_string(), v);
            }
            Ok(json!({"ok": true}))
        })(),
        "pref_list" => (|| {
            let group = req(a, "group")?.trim_end_matches('/').to_string();
            let entries: Vec<(String, Value)> = if group == SC_GROUP {
                cx.prefs(json!({}))?.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
            } else {
                cx.st.params.get(&group).map(|g| g.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
            };
            let pick = |f: fn(&Value) -> bool| entries.iter().filter(|(_, v)| f(v)).map(|(k, _)| k.clone()).collect::<Vec<_>>();
            let mut subgroups: Vec<String> = cx
                .st
                .params
                .keys()
                .filter_map(|k| k.strip_prefix(&format!("{group}/")).map(|r| r.split('/').next().unwrap_or(r).to_string()))
                .collect();
            if SC_GROUP.starts_with(&format!("{group}/")) {
                subgroups.push(SC_GROUP[group.len() + 1..].split('/').next().unwrap_or("SolidCraft").to_string());
            }
            subgroups.sort();
            subgroups.dedup();
            Ok(
                json!({"subgroups": subgroups, "ints": pick(|v| v.is_i64() || v.is_u64()), "floats": pick(|v| v.is_f64()), "bools": pick(Value::is_boolean), "strings": pick(Value::is_string)}),
            )
        })(),

        // ======================= scripting =======================
        "freecad_python_exec" => {
            let code = s(a, "code").unwrap_or("").to_string();
            let (success, stdout, exception) = run_script(cx, &code);
            Ok(json!({"success": success, "stdout": stdout, "stderr": "", "exception": exception}))
        }
        "freecad_python_eval" => (|| eval_expression(cx, req(a, "expression")?))(),

        // ======================= spreadsheet =======================
        "ss_create" => (|| {
            cx.use_doc(doc_arg(a))?;
            let base = s(a, "label").unwrap_or("Spreadsheet").to_string();
            let mut n = base.clone();
            let mut i = 1;
            while cx.st.sheets.contains_key(&n) {
                n = format!("{base}{i:03}");
                i += 1;
            }
            cx.st.sheets.insert(n.clone(), Default::default());
            Ok(json!({"Name": n, "Label": n}))
        })(),
        "ss_set" => (|| {
            cx.use_doc(doc_arg(a))?;
            let sheet = sheet_of(cx, req(a, "sheet")?)?;
            let cell = req(a, "cell")?.to_ascii_uppercase();
            let value = match a.get("value") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            let param = cell_param(cx, sheet, &cell);
            let expr = cell_expression(cx, sheet, &value);
            let exists = has_param(cx, &param);
            if exists {
                cx.run("parameters.change", json!({"name": param, "expression": expr}))?;
            } else {
                cx.run("parameters.add", json!({"name": param, "expression": expr, "comment": format!("{sheet}.{cell}")}))?;
            }
            cx.st.sheets.entry(sheet.to_string()).or_default().insert(cell.clone(), param);
            Ok(json!({"sheet": sheet, "cell": cell, "value": value}))
        })(),
        "ss_get" => (|| {
            cx.use_doc(doc_arg(a))?;
            let sheet = sheet_of(cx, req(a, "sheet")?)?;
            let cell = req(a, "cell")?;
            // a cell or an alias
            let param =
                cx.st.sheets.get(sheet).and_then(|m| m.values().find(|p| p.as_str() == cell).cloned()).unwrap_or_else(|| cell_param(cx, sheet, cell));
            let list = cx.run("parameters.list", json!({}))?;
            let p = list
                .get("parameters")
                .and_then(Value::as_array)
                .and_then(|l| l.iter().find(|p| p.get("name").and_then(Value::as_str) == Some(param.as_str())).cloned());
            let mut out = json!({"cell": cell});
            if let Some(p) = p {
                out["expression"] = p.get("expression").cloned().unwrap_or(Value::Null);
                out["value"] = p.get("value").cloned().unwrap_or(Value::Null);
            } else {
                out["value"] = Value::Null;
            }
            Ok(out)
        })(),
        "ss_set_alias" => (|| {
            cx.use_doc(doc_arg(a))?;
            let sheet = sheet_of(cx, req(a, "sheet")?)?.to_string();
            let cell = req(a, "cell")?.to_ascii_uppercase();
            let alias = req(a, "alias")?.to_string();
            let old = cell_param(cx, &sheet, &cell);
            if has_param(cx, &old) {
                cx.run("parameters.rename", json!({"name": old, "new_name": alias}))?;
            } else {
                cx.run("parameters.add", json!({"name": alias, "expression": "0", "comment": format!("{sheet}.{cell}")}))?;
            }
            cx.st.sheets.entry(sheet.clone()).or_default().insert(cell.clone(), alias.clone());
            Ok(json!({"sheet": sheet, "cell": cell, "alias": alias}))
        })(),

        // ======================= assembly =======================
        "assembly_create" => (|| {
            cx.use_doc(doc_arg(a))?;
            cx.leave_sketch();
            let label = s(a, "label").unwrap_or("Assembly");
            let r = cx.run("component.create", json!({"name": label, "activate": false}))?;
            let n = r.get("name").and_then(Value::as_str).unwrap_or(label).to_string();
            cx.st.assemblies.push(n.clone());
            Ok(json!({"Name": n, "Label": n}))
        })(),
        "assembly_add_part" => (|| {
            let asm = req(a, "assembly")?;
            let part = req(a, "part")?;
            let comp = resolve(cx, asm, doc_arg(a))?;
            let Obj::Component { id, .. } = comp else { return Err(te("no_object", "Object not found")) };
            let p = resolve(cx, part, doc_arg(a))?;
            cx.leave_sketch();
            let bodies = bodies_of(cx, &p);
            if bodies.is_empty() {
                return Err(te("no_object", "Object not found"));
            }
            cx.run("component.move_bodies", json!({"bodies": bodies, "component": id}))?;
            Ok(json!({"assembly": asm, "added": part}))
        })(),

        // ======================= display =======================
        "display_refine" => (|| {
            let msaa = a.get("msaa").and_then(Value::as_u64).unwrap_or(8);
            if ![2, 4, 6, 8].contains(&msaa) {
                return Err(te("bad_value", "msaa must be 2, 4, 6 or 8"));
            }
            let before = cx.prefs(json!({}))?;
            if cx.st.display_backup.is_none() {
                cx.st.display_backup = Some(json!({
                    "msaa": before.get("msaa"), "ambient_occlusion": before.get("ambient_occlusion"), "display_quality": before.get("display_quality"),
                    "supersampling": before.get("supersampling"), "supersampling_adaptive": before.get("supersampling_adaptive"),
                }));
            }
            // SolidCraft offers 1, 2, 4 or 8 samples
            let samples = if msaa == 6 { 4 } else { msaa };
            cx.prefs(json!({"msaa": samples}))?;
            if b(a, "effects", true) {
                cx.run("view.graphics_preset", json!({"preset": "quality"}))?;
            }
            Ok(
                json!({"preferences_changed": if b(a, "effects", true) { 5 } else { 1 }, "shapes": body_names(cx).len(), "meshes": 0, "note": "Anti-aliasing applies after a restart; the quality preset (ambient occlusion, Fine tessellation, supersampling) applies now."}),
            )
        })(),
        "display_restore" => (|| {
            let backup = cx.st.display_backup.take().ok_or_else(|| te("no_backup", "No saved display settings to restore"))?;
            let mut set = serde_json::Map::new();
            for (k, v) in backup.as_object().into_iter().flatten() {
                if !v.is_null() {
                    set.insert(k.clone(), v.clone());
                }
            }
            let n = set.len();
            cx.prefs(Value::Object(set))?;
            Ok(json!({"restored": n, "note": "Restart SolidCraft for anti-aliasing to change back."}))
        })(),
        _ => return None,
    })
}

fn rename(cx: &mut Cx, o: &Obj, label: &str) -> Result<(), ToolError> {
    if label.is_empty() {
        return Err(te("bad_value", "Label must not be empty"));
    }
    match o {
        Obj::Body(n) => {
            cx.run("body.rename", json!({"body": n, "name": label}))?;
            if let Some(p) = cx.st.place.remove(n) {
                cx.st.place.insert(label.to_string(), p);
            }
        }
        Obj::Feature { name, .. } | Obj::Sketch { name, .. } => drop(cx.run("timeline.rename", json!({"feature": name, "name": label}))?),
        Obj::Component { id, name } => {
            cx.run("component.rename", json!({"component": id, "name": label}))?;
            for list in [&mut cx.st.bodies, &mut cx.st.assemblies] {
                for x in list.iter_mut().filter(|x| *x == name) {
                    *x = label.to_string();
                }
            }
        }
        Obj::Sheet(n) => {
            if let Some(cells) = cx.st.sheets.remove(n) {
                cx.st.sheets.insert(label.to_string(), cells);
            }
        }
    }
    Ok(())
}

fn set_visible(cx: &mut Cx, o: &Obj, v: bool) -> Result<(), ToolError> {
    let items: Vec<String> = match o {
        Obj::Sketch { name, .. } => vec![name.clone()],
        other => bodies_of(cx, other),
    };
    if items.is_empty() {
        return Err(te("visibility_failed", format!("Cannot set visibility of '{}'", o.name())));
    }
    cx.run("browser.visibility", json!({"visible": v, "items": items}))?;
    Ok(())
}

/// Set bodies' absolute placement (from the tracked one).
pub fn set_placement(cx: &mut Cx, bodies: &[String], np: &[f64; 16]) -> Result<(), ToolError> {
    for body in bodies {
        let old = cx.st.place.get(body).copied().unwrap_or(IDENTITY);
        let delta = mat_mul(np, &mat_inv(&old));
        apply_transform(cx, std::slice::from_ref(body), &delta)?;
        cx.st.place.insert(body.clone(), *np);
    }
    Ok(())
}

/// Standard base64 (for screenshots).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= c.len() {
                out.push(T[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
