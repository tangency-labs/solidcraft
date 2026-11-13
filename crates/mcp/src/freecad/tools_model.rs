//! Part primitives, booleans, transforms, alignment, direct edits, PartDesign, Sketcher, Draft
//! and measurement.

use serde_json::{Value, json};

use super::tools_app::{IDENTITY, Obj, apply_transform, bodies_of, body_names, feature_name, inspect, mat_from, mat_mul, resolve, set_placement};
use super::{Cx, R, ToolError, b, f, obj_xyz, req, s, te, xyz};

// ---------- helpers ----------

/// Bodies that exist now and didn't before.
pub fn new_bodies(cx: &mut Cx, before: &[String]) -> Vec<String> {
    body_names(cx).into_iter().filter(|n| !before.contains(n)).collect()
}
/// The last timeline entry (id, name).
pub fn last_feature(cx: &mut Cx) -> Option<(u64, String)> {
    let ins = inspect(cx);
    let f = ins.get("timeline").and_then(Value::as_array).and_then(|t| t.last()).cloned()?;
    Some((f.get("id").and_then(Value::as_u64)?, f.get("name").and_then(Value::as_str)?.to_string()))
}
/// A body name not in use: `base`, `base001`, `base002`… (FreeCAD's naming).
pub fn unique_body(cx: &mut Cx, base: &str) -> String {
    let names = body_names(cx);
    if !names.iter().any(|n| n == base) {
        return base.to_string();
    }
    (1..).map(|i| format!("{base}{i:03}")).find(|n| !names.contains(n)).unwrap_or_else(|| base.to_string())
}
/// Give a fresh body FreeCAD's name for it.
fn name_body(cx: &mut Cx, body: &str, want: &str) -> Result<String, ToolError> {
    if body == want {
        return Ok(body.to_string());
    }
    let n = unique_body(cx, want);
    cx.run("body.rename", json!({"body": body, "name": n}))?;
    Ok(n)
}
fn name_feature(cx: &mut Cx, want: Option<&str>) -> String {
    let Some((_, name)) = last_feature(cx) else { return want.unwrap_or("").to_string() };
    match want {
        Some(w) if w != name => match cx.cmd("timeline.rename", json!({"feature": name, "name": w})) {
            Ok(_) => w.to_string(),
            Err(_) => name,
        },
        _ => name,
    }
}
fn hide(cx: &mut Cx, items: &[String]) {
    if !items.is_empty() {
        let _ = cx.cmd("browser.visibility", json!({"visible": false, "items": items}));
    }
}
fn measure(cx: &mut Cx, body: &str) -> Value {
    cx.cmd("inspect.measure", json!({"bodies": [body]})).ok().and_then(|v| v.get("bodies").and_then(|b| b.get(0)).cloned()).unwrap_or_default()
}
fn arr3(v: Option<&Value>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}
/// Bounding box of bodies: (min, max).
pub fn bbox(cx: &mut Cx, bodies: &[String]) -> Option<([f64; 3], [f64; 3])> {
    let m = cx.cmd("inspect.measure", json!({"bodies": bodies})).ok()?;
    let mut out: Option<([f64; 3], [f64; 3])> = None;
    for b in m.get("bodies").and_then(Value::as_array).into_iter().flatten() {
        let (Some(lo), Some(hi)) = (arr3(b.pointer("/bbox/min")), arr3(b.pointer("/bbox/max"))) else { continue };
        out = Some(match out {
            None => (lo, hi),
            Some((a, c)) => ([a[0].min(lo[0]), a[1].min(lo[1]), a[2].min(lo[2])], [c[0].max(hi[0]), c[1].max(hi[1]), c[2].max(hi[2])]),
        });
    }
    out
}
fn targets(cx: &mut Cx, name: &str, doc: Option<&str>) -> Result<Vec<String>, ToolError> {
    let o = resolve(cx, name, doc)?;
    let list = bodies_of(cx, &o);
    if list.is_empty() {
        return Err(te("no_shape", format!("Object '{name}' has no geometry to transform")));
    }
    Ok(list)
}

/// FreeCAD placement `{Base, Rotation: {Axis, Angle}}` (Angle in degrees, as App.Rotation takes it).
fn placement(a: &Value) -> Option<[f64; 16]> {
    let p = a.get("placement").filter(|p| p.is_object())?;
    let rot = p.get("Rotation");
    Some(mat_from(
        xyz(rot.and_then(|r| r.get("Axis")), [0., 0., 1.]),
        rot.and_then(|r| r.get("Angle")).and_then(Value::as_f64).unwrap_or(0.0),
        xyz(p.get("Base"), [0.0; 3]),
    ))
}

// ---------- sub-elements ----------

fn faces(cx: &mut Cx, body: &str) -> Vec<Value> {
    cx.cmd("model.faces", json!({"body": body})).ok().and_then(|v| v.get("faces").and_then(Value::as_array).cloned()).unwrap_or_default()
}
fn edges(cx: &mut Cx, body: &str) -> Vec<Value> {
    cx.cmd("model.edges", json!({"body": body})).ok().and_then(|v| v.get("edges").and_then(Value::as_array).cloned()).unwrap_or_default()
}
fn sub_index(sub: &str, kinds: &[&str]) -> Option<(String, usize)> {
    let sub = sub.rsplit('.').next().unwrap_or(sub);
    for k in kinds {
        if let Some(n) = sub.strip_prefix(k).and_then(|n| n.parse::<usize>().ok()) {
            return n.checked_sub(1).map(|i| (k.to_string(), i));
        }
    }
    None
}
fn face_of(cx: &mut Cx, body: &str, sub: &str) -> Result<Value, ToolError> {
    let sub = if sub.chars().all(|c| c.is_ascii_digit()) { format!("Face{sub}") } else { sub.to_string() };
    let (_, i) = sub_index(&sub, &["Face", "Facet"]).ok_or_else(|| te("bad_subname", format!("'{sub}' is not a face")))?;
    faces(cx, body).get(i).cloned().ok_or_else(|| te("bad_subname", format!("'{body}' has no {sub}")))
}
fn edge_of(cx: &mut Cx, body: &str, sub: &str) -> Result<(usize, Value), ToolError> {
    let sub = if sub.chars().all(|c| c.is_ascii_digit()) { format!("Edge{sub}") } else { sub.to_string() };
    let (_, i) = sub_index(&sub, &["Edge"]).ok_or_else(|| te("bad_subname", format!("'{sub}' is not an edge")))?;
    edges(cx, body).get(i).cloned().map(|e| (i, e)).ok_or_else(|| te("bad_subname", format!("'{body}' has no {sub}")))
}
/// Vertices as FreeCAD numbers them: the distinct edge ends in edge order.
fn vertices(cx: &mut Cx, body: &str) -> Vec<([f64; 3], usize)> {
    let mut out: Vec<([f64; 3], usize)> = Vec::new();
    for (i, e) in edges(cx, body).iter().enumerate() {
        for k in ["start", "end"] {
            if let Some(p) = arr3(e.get(k))
                && !out.iter().any(|(q, _)| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() + (q[2] - p[2]).abs() < 1e-6)
            {
                out.push((p, i));
            }
        }
    }
    out
}
/// A selection item for `FaceN` / `EdgeN` / `VertexN` (the edge it ends) of a body.
pub fn sub_item(cx: &mut Cx, body: &str, sub: &str) -> Result<Value, ToolError> {
    if let Some((_, i)) = sub_index(sub, &["Vertex"]) {
        let (p, e) = vertices(cx, body).get(i).copied().ok_or_else(|| te("bad_subname", format!("'{body}' has no {sub}")))?;
        let _ = e;
        let edge = edges(cx, body).into_iter().enumerate().find(|(_, x)| [arr3(x.get("start")), arr3(x.get("end"))].contains(&Some(p)));
        let (idx, ex) = edge.ok_or_else(|| te("bad_subname", format!("'{body}' has no {sub}")))?;
        return Ok(json!({"type": "edge", "body": body, "index": idx, "point": ex.get("mid").cloned().unwrap_or(json!(p))}));
    }
    if sub_index(sub, &["Edge"]).is_some() {
        let (i, e) = edge_of(cx, body, sub)?;
        return Ok(json!({"type": "edge", "body": body, "index": i, "point": e.get("mid")}));
    }
    let fc = face_of(cx, body, sub)?;
    Ok(json!({"type": "face", "body": body, "index": fc.get("index"), "point": fc.get("point")}))
}

// ---------- sketches ----------

fn sketch_name(cx: &mut Cx, name: &str, doc: Option<&str>) -> Result<String, ToolError> {
    match resolve(cx, name, doc)? {
        Obj::Sketch { name, .. } => Ok(name),
        o => {
            let t = super::tools_app::type_id(cx, &o);
            Err(te("not_a_sketch", format!("Object '{name}' is not a Sketch (TypeId={t})")))
        }
    }
}
fn sketch_info(cx: &mut Cx, sketch: &str) -> Value {
    cx.cmd("sketch.inspect", json!({"sketch": sketch})).unwrap_or_default()
}
fn curve_ids(info: &Value) -> Vec<String> {
    info.get("curves")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}
/// Run `body` with the sketch open for editing, then finish it.
fn in_sketch(cx: &mut Cx, sketch: &str, body: impl FnOnce(&mut Cx) -> R) -> R {
    let ins = inspect(cx);
    let open = ins.get("active_sketch").and_then(Value::as_u64).and_then(|id| feature_name(&ins, id));
    if open.as_deref() != Some(sketch) {
        cx.leave_sketch();
        cx.run("sketch.edit", json!({"sketch": sketch}))?;
    }
    let r = body(cx);
    let _ = cx.cmd("sketch.finish", json!({}));
    r
}
/// Add curves with one command and report the new ones' geometry indices.
fn add_curves(cx: &mut Cx, sketch: &str, cmd: &str, params: Value) -> Result<Vec<usize>, ToolError> {
    let before = curve_ids(&sketch_info(cx, sketch));
    in_sketch(cx, sketch, |cx| cx.run(cmd, params))?;
    let after = curve_ids(&sketch_info(cx, sketch));
    Ok(after.iter().enumerate().filter(|(_, id)| !before.contains(id)).map(|(i, _)| i).collect())
}
/// The point of a curve: FreeCAD positions 1 start, 2 end, 3 centre.
fn point_ref(ids: &[String], g: &Value, pos: &Value) -> Result<String, ToolError> {
    let c = curve_ref(ids, g)?;
    Ok(match pos.as_i64() {
        Some(1) => format!("{c}.start"),
        Some(2) => format!("{c}.end"),
        Some(3) => format!("{c}.center"),
        _ => return Err(te("bad_args", format!("Bad point position {pos} (1 start, 2 end, 3 centre)"))),
    })
}
fn curve_ref(ids: &[String], g: &Value) -> Result<String, ToolError> {
    match g.as_i64() {
        Some(-1) => Ok("x".into()),
        Some(-2) => Ok("y".into()),
        Some(i) if i >= 0 => ids.get(i as usize).cloned().ok_or_else(|| te("bad_index", format!("Geometry index {i} out of range"))),
        _ => Err(te("bad_args", format!("Bad geometry index {g}"))),
    }
}
/// A FreeCAD Sketcher constraint as SolidCraft commands.
fn constraint(ids: &[String], ty: &str, a: &[Value], value: Option<f64>) -> Result<(&'static str, Value), ToolError> {
    let need = |n: usize| if a.len() < n { Err(te("bad_args", format!("{ty} needs {n} arguments"))) } else { Ok(()) };
    let val = || value.ok_or_else(|| te("bad_args", format!("{ty} needs a value")));
    let two_points = || -> Result<Value, ToolError> { Ok(json!([point_ref(ids, &a[0], &a[1])?, point_ref(ids, &a[2], &a[3])?])) };
    Ok(match ty {
        "Coincident" => {
            need(4)?;
            ("sketch.constraint.coincident", json!({"a": point_ref(ids, &a[0], &a[1])?, "b": point_ref(ids, &a[2], &a[3])?}))
        }
        "PointOnObject" => {
            need(3)?;
            ("sketch.constraint.coincident", json!({"a": point_ref(ids, &a[0], &a[1])?, "b": curve_ref(ids, &a[2])?}))
        }
        "Horizontal" | "Vertical" => {
            let mode = ty.to_lowercase();
            if a.len() >= 4 {
                ("sketch.constraint.horizontal_vertical", json!({"points": two_points()?, "mode": mode}))
            } else {
                need(1)?;
                ("sketch.constraint.horizontal_vertical", json!({"line": curve_ref(ids, &a[0])?, "mode": mode}))
            }
        }
        "Parallel" | "Perpendicular" | "Tangent" | "Equal" => {
            need(2)?;
            let cmd = match ty {
                "Parallel" => "sketch.constraint.parallel",
                "Perpendicular" => "sketch.constraint.perpendicular",
                "Tangent" => "sketch.constraint.tangent",
                _ => "sketch.constraint.equal",
            };
            (cmd, json!({"a": curve_ref(ids, &a[0])?, "b": curve_ref(ids, &a[1])?}))
        }
        "Distance" | "DistanceX" | "DistanceY" => {
            let kind = match ty {
                "DistanceX" => "horizontal",
                "DistanceY" => "vertical",
                _ => "distance",
            };
            let ents = match a.len() {
                0 => return Err(te("bad_args", format!("{ty} needs arguments"))),
                1 => json!([curve_ref(ids, &a[0])?]),
                2 => json!([point_ref(ids, &a[0], &a[1])?]),
                3 => json!([point_ref(ids, &a[0], &a[1])?, curve_ref(ids, &a[2])?]),
                _ => two_points()?,
            };
            let kind = if a.len() == 1 && kind == "distance" { "length" } else { kind };
            ("sketch.dimension", json!({"entities": ents, "type": kind, "value": val()?}))
        }
        "Radius" | "Diameter" => {
            need(1)?;
            ("sketch.dimension", json!({"entities": [curve_ref(ids, &a[0])?], "type": ty.to_lowercase(), "value": val()?}))
        }
        "Angle" => {
            let deg = val()?.to_degrees();
            let ents = if a.len() >= 2 {
                json!([curve_ref(ids, &a[0])?, curve_ref(ids, a.get(2).filter(|_| a.len() >= 4).unwrap_or(&a[1]))?])
            } else {
                json!([curve_ref(ids, &a[0])?, "x"])
            };
            ("sketch.dimension", json!({"entities": ents, "type": "angle", "value": format!("{deg} deg")}))
        }
        "Fixed" | "Block" | "Lock" => {
            need(1)?;
            let e = if a.len() >= 2 { point_ref(ids, &a[0], &a[1])? } else { curve_ref(ids, &a[0])? };
            ("sketch.constraint.fix", json!({"entity": e, "fixed": true}))
        }
        "Symmetric" => {
            need(5)?;
            let line = if a.len() >= 6 { point_ref(ids, &a[4], &a[5])? } else { curve_ref(ids, &a[4])? };
            ("sketch.constraint.symmetry", json!({"a": point_ref(ids, &a[0], &a[1])?, "b": point_ref(ids, &a[2], &a[3])?, "line": line}))
        }
        _ => {
            return Err(te(
                "unsupported_constraint",
                format!(
                    "Unsupported constraint type '{ty}' (Coincident, PointOnObject, Horizontal, Vertical, Parallel, Perpendicular, Tangent, Equal, Distance, DistanceX, DistanceY, Radius, Diameter, Angle, Fixed/Block, Symmetric)"
                ),
            ));
        }
    })
}

/// A sketch on a world plane through `z` (Draft objects): returns its name.
fn draft_sketch(cx: &mut Cx, base: &str, z: f64) -> Result<String, ToolError> {
    cx.leave_sketch();
    let r = cx.run("sketch.create", json!({"plane": {"origin": [0, 0, z], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": base}))?;
    Ok(r.get("name").and_then(Value::as_str).unwrap_or(base).to_string())
}
fn finish_draft(cx: &mut Cx, sketch: &str, type_id: &str) -> R {
    cx.run("sketch.finish", json!({}))?;
    Ok(json!({"Name": sketch, "Label": sketch, "TypeId": type_id}))
}
fn pt2(v: &Value) -> [f64; 2] {
    [v.get("x").and_then(Value::as_f64).unwrap_or(0.0), v.get("y").and_then(Value::as_f64).unwrap_or(0.0)]
}

// ---------- PartDesign ----------

/// The PartDesign body (component) to work in: the named one, the active one, or a new "Body".
fn pd_body(cx: &mut Cx, name: Option<&str>, doc: Option<&str>) -> Result<String, ToolError> {
    cx.use_doc(doc)?;
    if let Some(n) = name {
        return match resolve(cx, n, None)? {
            Obj::Component { name, .. } => Ok(name),
            _ => Err(te("no_object", format!("'{n}' is not a PartDesign body"))),
        };
    }
    if let Some(b) = cx.st.active_body.clone()
        && matches!(resolve(cx, &b, None), Ok(Obj::Component { .. }))
    {
        return Ok(b);
    }
    new_pd_body(cx, "Body")
}
fn new_pd_body(cx: &mut Cx, label: &str) -> Result<String, ToolError> {
    cx.leave_sketch();
    let r = cx.run("component.create", json!({"name": label, "activate": true}))?;
    let n = r.get("name").and_then(Value::as_str).unwrap_or(label).to_string();
    cx.st.bodies.push(n.clone());
    cx.st.active_body = Some(n.clone());
    Ok(n)
}
/// The component a sketch lives in, and that component's bodies.
fn sketch_home(cx: &mut Cx, sketch: &str) -> (Option<String>, Vec<String>) {
    let comps =
        cx.cmd("component.list", json!({})).ok().and_then(|v| v.get("components").cloned()).and_then(|c| c.as_array().cloned()).unwrap_or_default();
    for c in comps {
        let holds = c.get("sketches").and_then(Value::as_array).is_some_and(|s| s.iter().any(|x| x.as_str() == Some(sketch)));
        if holds && c.get("parent").is_some_and(|p| !p.is_null()) {
            let bodies = c.get("bodies").cloned().and_then(|b| serde_json::from_value(b).ok()).unwrap_or_default();
            return (c.get("name").and_then(Value::as_str).map(str::to_string), bodies);
        }
    }
    (None, Vec::new())
}
fn activate(cx: &mut Cx, comp: Option<&str>) {
    let _ = cx.cmd("component.activate", json!({"component": comp.unwrap_or("root")}));
}
fn pd_feature(cx: &mut Cx, a: &Value, cmd: &str, mut params: Value, cut: bool, base: &str, type_id: &str) -> R {
    let sketch = sketch_name(cx, req(a, "sketch")?, s(a, "doc"))?;
    cx.leave_sketch();
    let (comp, bodies) = sketch_home(cx, &sketch);
    if cut && bodies.is_empty() {
        return Err(te("no_base", format!("{base} needs a solid in the body to cut from")));
    }
    activate(cx, comp.as_deref());
    params["sketch"] = json!(sketch);
    if bodies.is_empty() {
        params["operation"] = json!("new");
        params["body_name"] = json!(comp.clone().unwrap_or_else(|| base.to_string()));
    } else {
        params["operation"] = json!(if cut { "cut" } else { "join" });
        params["targets"] = json!([bodies[0]]);
    }
    cx.run(cmd, params)?;
    let n = name_feature(cx, Some(s(a, "label").unwrap_or(base)));
    hide(cx, std::slice::from_ref(&sketch));
    Ok(json!({"Name": n, "Label": n, "TypeId": type_id}))
}
fn edge_items(cx: &mut Cx, body: &str, list: &[Value]) -> Result<Vec<Value>, ToolError> {
    list.iter()
        .map(|e| {
            let e = match e {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            let (i, _) = edge_of(cx, body, &e)?;
            Ok(json!({"body": body, "index": i}))
        })
        .collect()
}
fn blend(cx: &mut Cx, object: &str, list: &[Value], size: f64, chamfer: bool, doc: Option<&str>) -> Result<String, ToolError> {
    let body = super::tools_app::body_of(cx, object, doc)?;
    cx.leave_sketch();
    let items = edge_items(cx, &body, list)?;
    if chamfer {
        cx.run("solid.chamfer", json!({"edges": items, "distance": size, "body": body}))?;
    } else {
        cx.run("solid.fillet", json!({"edges": items, "radius": size, "body": body}))?;
    }
    Ok(body)
}
fn in_place(cx: &mut Cx, body: &str) -> Value {
    let m = measure(cx, body);
    json!({"Name": body, "Label": body, "in_place": true, "faces": m.get("faces"), "volume": m.get("volume_mm3")})
}

// ---------- primitives ----------

fn primitive(cx: &mut Cx, a: &Value, base: &str, type_id: &str, make: impl FnOnce(&mut Cx, &str) -> R) -> R {
    cx.use_doc(s(a, "doc"))?;
    cx.leave_sketch();
    let want = s(a, "label").unwrap_or(base).to_string();
    let before = body_names(cx);
    make(cx, &want)?;
    let made = new_bodies(cx, &before);
    let body = made.first().cloned().ok_or_else(|| te("bad_value", format!("{base}: invalid dimensions")))?;
    let body = name_body(cx, &body, &want)?;
    cx.st.place.insert(body.clone(), IDENTITY);
    if let Some(m) = placement(a) {
        apply_transform(cx, std::slice::from_ref(&body), &m)?;
    }
    Ok(json!({"Name": body, "Label": body, "TypeId": type_id}))
}
/// A solid of revolution about world Z from an (r, z) profile, through `angle` degrees.
fn revolve_z(cx: &mut Cx, name: &str, prof: &[[f64; 2]], angle: f64) -> R {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    for p in prof {
        if pts.last().is_none_or(|q: &[f64; 2]| (q[0] - p[0]).abs() + (q[1] - p[1]).abs() > 1e-9) {
            pts.push(*p);
        }
    }
    if pts.len() > 2 && pts.first() == pts.last() {
        pts.pop();
    }
    let sk = cx.run("sketch.create", json!({"plane": "XZ", "name": format!("{name} profile")}))?;
    let sk = sk.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    cx.run("sketch.line", json!({"points": pts, "closed": true}))?;
    cx.run("sketch.finish", json!({}))?;
    let r = cx.run(
        "solid.revolve",
        json!({"sketch": sk, "axis": "Z", "angle": format!("{} deg", angle.clamp(1e-3, 360.0)), "operation": "new", "name": name, "body_name": name}),
    );
    hide(cx, std::slice::from_ref(&sk));
    r
}
fn cylinder(cx: &mut Cx, n: &str, r: f64, h: f64, angle: f64) -> R {
    if !(r > 0.0 && h > 0.0) {
        return Err(te("bad_value", "Cylinder: radius and height must be positive"));
    }
    if angle >= 360.0 - 1e-9 {
        cx.run("solid.cylinder", json!({"radius": r, "height": h, "name": n, "body_name": n, "operation": "new"}))
    } else {
        revolve_z(cx, n, &[[0.0, 0.0], [r, 0.0], [r, h], [0.0, h]], angle)
    }
}
fn cone(cx: &mut Cx, n: &str, r1: f64, r2: f64, h: f64, angle: f64) -> R {
    if !(h > 0.0) || r1 < 0.0 || r2 < 0.0 || r1 + r2 <= 0.0 {
        return Err(te("bad_value", "Cone: invalid dimensions"));
    }
    revolve_z(cx, n, &[[0.0, 0.0], [r1, 0.0], [r2, h], [0.0, h]], angle)
}
fn plane(cx: &mut Cx, n: &str, l: f64, w: f64) -> R {
    if !(l > 0.0 && w > 0.0) {
        return Err(te("bad_value", "Plane: invalid dimensions"));
    }
    let sk = cx.run("sketch.create", json!({"plane": "XY", "name": format!("{n} outline")}))?;
    let sk = sk.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    cx.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [l, w]}))?;
    cx.run("sketch.finish", json!({}))?;
    let r = cx.run("surface.patch", json!({"sketch": sk}));
    hide(cx, std::slice::from_ref(&sk));
    r
}

// ---------- slicing helpers for booleans on copies ----------

fn copies(cx: &mut Cx, bodies: &[String]) -> Result<Vec<String>, ToolError> {
    let mut out = Vec::new();
    for b in bodies {
        let before = body_names(cx);
        cx.run("solid.move", json!({"bodies": [b], "translate": [0, 0, 0], "copy": true}))?;
        out.extend(new_bodies(cx, &before));
    }
    Ok(out)
}
fn boolean(cx: &mut Cx, names: &[String], op: &str, base: &str, label: Option<&str>, doc: Option<&str>) -> R {
    let mut inputs = Vec::new();
    for n in names {
        let o = resolve(cx, n, doc)?;
        let b = bodies_of(cx, &o).into_iter().next().ok_or_else(|| te("no_shape", format!("Object '{n}' has no Shape")))?;
        inputs.push(b);
    }
    if inputs.len() < 2 {
        return Err(te("bad_args", format!("{base} needs two objects")));
    }
    cx.leave_sketch();
    let c = copies(cx, &inputs)?;
    if c.len() != inputs.len() {
        return Err(te("boolean_failed", format!("{base}: could not copy the inputs")));
    }
    cx.run("solid.combine", json!({"target": c[0], "tools": &c[1..], "operation": op, "keep_tools": false}))?;
    if !body_names(cx).contains(&c[0]) {
        return Err(te("empty_result", format!("{base}: the result is empty")));
    }
    let n = name_body(cx, &c[0], label.unwrap_or(base))?;
    hide(cx, &inputs);
    Ok(json!({"Name": n, "Label": n, "TypeId": "Part::Boolean"}))
}

// ---------- measurement ----------

/// `Obj` or `Obj.Sub` as a measurement item.
fn measure_item(cx: &mut Cx, r: &str, doc: Option<&str>) -> Result<Value, ToolError> {
    let (name, sub) = match resolve(cx, r, doc) {
        Ok(_) => (r.to_string(), None),
        Err(e) => match r.rsplit_once('.') {
            Some((n, s)) => (n.to_string(), Some(s.to_string())),
            None => return Err(e),
        },
    };
    let o = resolve(cx, &name, doc)?;
    let body = bodies_of(cx, &o).into_iter().next().ok_or_else(|| te("no_shape", format!("Object '{name}' has no Shape")))?;
    match sub {
        None => Ok(json!({"type": "body", "name": body})),
        Some(s) => sub_item(cx, &body, &s),
    }
}

/// The tools of this file; `None` for a name that isn't one of them.
pub fn call(cx: &mut Cx, name: &str, a: &Value) -> Option<R> {
    let doc = s(a, "doc");
    Some(match name {
        "server_ping" => Ok(json!({"alive": true, "marshalled_to_main_thread": true})),

        // ======================= Part primitives =======================
        "part_box" => primitive(cx, a, "Box", "Part::Box", |cx, n| {
            cx.run("solid.box", json!({"length": f(a, "length", 10.0), "width": f(a, "width", 10.0), "height": f(a, "height", 10.0), "corner": [0, 0, 0], "name": n, "body_name": n, "operation": "new"}))
        }),
        "part_cylinder" => {
            primitive(cx, a, "Cylinder", "Part::Cylinder", |cx, n| cylinder(cx, n, f(a, "radius", 5.0), f(a, "height", 10.0), f(a, "angle", 360.0)))
        }
        "part_sphere" => primitive(cx, a, "Sphere", "Part::Sphere", |cx, n| {
            let r = f(a, "radius", 5.0);
            if r <= 0.0 {
                return Err(te("bad_value", "Sphere: radius must be positive"));
            }
            cx.run("solid.sphere", json!({"radius": r, "center": [0, 0, 0], "name": n, "body_name": n, "operation": "new"}))
        }),
        "part_cone" => primitive(cx, a, "Cone", "Part::Cone", |cx, n| {
            cone(cx, n, f(a, "radius1", 5.0), f(a, "radius2", 0.0), f(a, "height", 10.0), f(a, "angle", 360.0))
        }),
        "part_torus" => primitive(cx, a, "Torus", "Part::Torus", |cx, n| {
            cx.run("solid.torus", json!({"major": f(a, "radius1", 10.0), "minor": f(a, "radius2", 2.0), "center": [0, 0, 0], "name": n, "body_name": n, "operation": "new"}))
        }),
        "part_plane" => primitive(cx, a, "Plane", "Part::Plane", |cx, n| plane(cx, n, f(a, "length", 100.0), f(a, "width", 100.0))),
        "part_create" => (|| {
            let t = req(a, "type_id")?.to_string();
            let p = a.get("properties").cloned().unwrap_or(json!({}));
            let num = |k: &str, d: f64| {
                p.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim_end_matches(" mm").parse().ok()))).unwrap_or(d)
            };
            let base = t.rsplit("::").next().unwrap_or("Part").to_string();
            match t.as_str() {
                "Part::Box" => primitive(cx, a, &base, &t, |cx, n| {
                    cx.run("solid.box", json!({"length": num("Length", 10.0), "width": num("Width", 10.0), "height": num("Height", 10.0), "corner": [0, 0, 0], "name": n, "body_name": n, "operation": "new"}))
                }),
                "Part::Cylinder" => {
                    primitive(cx, a, &base, &t, |cx, n| cylinder(cx, n, num("Radius", 2.0), num("Height", 10.0), num("Angle", 360.0)))
                }
                "Part::Sphere" => primitive(cx, a, &base, &t, |cx, n| {
                    cx.run("solid.sphere", json!({"radius": num("Radius", 5.0), "name": n, "body_name": n, "operation": "new"}))
                }),
                "Part::Cone" => primitive(cx, a, &base, &t, |cx, n| {
                    cone(cx, n, num("Radius1", 2.0), num("Radius2", 4.0), num("Height", 10.0), num("Angle", 360.0))
                }),
                "Part::Torus" => primitive(cx, a, &base, &t, |cx, n| {
                    cx.run(
                        "solid.torus",
                        json!({"major": num("Radius1", 10.0), "minor": num("Radius2", 2.0), "name": n, "body_name": n, "operation": "new"}),
                    )
                }),
                "Part::Plane" => primitive(cx, a, &base, &t, |cx, n| plane(cx, n, num("Length", 10.0), num("Width", 10.0))),
                "Part::Prism" => primitive(cx, a, &base, &t, |cx, n| {
                    let sk = cx.run("sketch.create", json!({"plane": "XY", "name": format!("{n} profile")}))?;
                    let sk = sk.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let sides = num("Polygon", 6.0).round().max(3.0);
                    let r = num("Circumradius", 2.0);
                    let pts: Vec<[f64; 2]> = (0..sides as usize)
                        .map(|i| {
                            let t = i as f64 / sides * std::f64::consts::TAU;
                            [r * t.cos(), r * t.sin()]
                        })
                        .collect();
                    cx.run("sketch.line", json!({"points": pts, "closed": true}))?;
                    cx.run("sketch.finish", json!({}))?;
                    let out = cx
                        .run("solid.extrude", json!({"sketch": sk, "distance": num("Height", 10.0), "operation": "new", "name": n, "body_name": n}));
                    hide(cx, std::slice::from_ref(&sk));
                    out
                }),
                "Part::Ellipsoid" => primitive(cx, a, &base, &t, |cx, n| {
                    let (r1, r2) = (num("Radius1", 2.0), num("Radius2", 4.0));
                    let r3 = num("Radius3", 0.0);
                    let r3 = if r3 > 0.0 { r3 } else { r2 };
                    let before = body_names(cx);
                    cx.run("solid.sphere", json!({"radius": 1, "name": n, "body_name": n, "operation": "new"}))?;
                    let made = new_bodies(cx, &before);
                    cx.run("solid.scale", json!({"bodies": made, "factors": [r2, r3, r1], "center": [0, 0, 0]}))
                }),
                _ => Err(te(
                    "unsupported_type",
                    format!("Cannot create '{t}': SolidCraft supports Part::Box, Cylinder, Sphere, Cone, Torus, Plane, Prism and Ellipsoid"),
                )),
            }
        })(),

        // ======================= booleans =======================
        "bool_fuse" | "bool_common" => {
            let objs: Vec<String> = serde_json::from_value(a.get("objects").cloned().unwrap_or_default()).unwrap_or_default();
            let (op, base) = if name == "bool_fuse" { ("join", "Fusion") } else { ("intersect", "Common") };
            boolean(cx, &objs, op, base, s(a, "label"), doc)
        }
        "bool_cut" => (|| boolean(cx, &[req(a, "base")?.to_string(), req(a, "tool")?.to_string()], "cut", "Cut", s(a, "label"), doc))(),
        "part_compound" => (|| {
            let objs: Vec<String> = serde_json::from_value(a.get("objects").cloned().unwrap_or_default()).unwrap_or_default();
            let mut bodies = Vec::new();
            for n in &objs {
                let o = resolve(cx, n, doc)?;
                bodies.extend(bodies_of(cx, &o));
            }
            if bodies.is_empty() {
                return Err(te("no_object", "Nothing to combine"));
            }
            cx.leave_sketch();
            let label = s(a, "label").unwrap_or("Compound");
            let r = cx.run("component.create", json!({"name": label, "activate": false}))?;
            let id = r.get("component").or_else(|| r.get("id")).cloned().unwrap_or(json!(label));
            let n = r.get("name").and_then(Value::as_str).unwrap_or(label).to_string();
            cx.run("component.move_bodies", json!({"bodies": bodies, "component": id}))?;
            Ok(json!({"Name": n, "Label": n, "TypeId": "Part::Compound"}))
        })(),

        // ======================= transforms =======================
        "transform_translate" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            apply_transform(cx, &list, &mat_from([0., 0., 1.], 0.0, [f(a, "dx", 0.0), f(a, "dy", 0.0), f(a, "dz", 0.0)]))?;
            let p = cx.st.place.get(&list[0]).copied().unwrap_or(IDENTITY);
            Ok(json!({"name": obj, "Placement_Base": obj_xyz([p[3], p[7], p[11]])}))
        })(),
        "transform_rotate" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            let c = xyz(a.get("center"), [0.0; 3]);
            let r = mat_from(xyz(a.get("axis"), [0., 0., 1.]), f(a, "angle_deg", 0.0), [0.0; 3]);
            let m = mat_mul(&mat_from([0., 0., 1.], 0.0, c), &mat_mul(&r, &mat_from([0., 0., 1.], 0.0, [-c[0], -c[1], -c[2]])));
            apply_transform(cx, &list, &m)?;
            Ok(json!({"name": obj, "ok": true}))
        })(),
        "transform_set_placement" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            set_placement(cx, &list, &mat_from(xyz(a.get("axis"), [0., 0., 1.]), f(a, "angle_deg", 0.0), xyz(a.get("base"), [0.0; 3])))?;
            Ok(json!({"name": obj, "ok": true}))
        })(),
        "transform_copy" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            cx.leave_sketch();
            let made = copies(cx, &list)?;
            let want = s(a, "label").map(str::to_string).unwrap_or_else(|| format!("{obj}_copy"));
            let mut first = String::new();
            for (i, b) in made.iter().enumerate() {
                let n = name_body(cx, b, &want)?;
                let p = cx.st.place.get(&list[i.min(list.len() - 1)]).copied().unwrap_or(IDENTITY);
                cx.st.place.insert(n.clone(), p);
                if i == 0 {
                    first = n;
                }
            }
            Ok(json!({"Name": first, "Label": first}))
        })(),
        "transform_mirror" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            cx.leave_sketch();
            let made = copies(cx, &list)?;
            let o = xyz(a.get("base"), [0.0; 3]);
            let n = xyz(a.get("normal"), [1.0, 0.0, 0.0]);
            cx.run("solid.mirror", json!({"bodies": made, "plane": {"origin": o, "normal": n}}))?;
            let want = s(a, "label").map(str::to_string).unwrap_or_else(|| format!("{obj}_mirror"));
            let mut first = String::new();
            for (i, b) in made.iter().enumerate() {
                let nm = name_body(cx, b, &want)?;
                if i == 0 {
                    first = nm;
                }
            }
            Ok(json!({"Name": first, "Label": first}))
        })(),

        // ======================= align =======================
        "align_center" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            let (lo, hi) = bbox(cx, &list).ok_or_else(|| te("no_shape", format!("'{obj}' has no geometry")))?;
            let mut ctr = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
            if s(a, "reference") == Some("mass") {
                let m = cx.run("inspect.center_of_mass", json!({"bodies": list}))?;
                ctr = arr3(m.get("center")).ok_or_else(|| te("no_solid", format!("'{obj}' has no solid; use reference='bbox'")))?;
            }
            let dz = match s(a, "z").unwrap_or("center") {
                "floor" => -lo[2],
                "keep" => 0.0,
                _ => -ctr[2],
            };
            let off = [-ctr[0], -ctr[1], dz];
            apply_transform(cx, &list, &mat_from([0., 0., 1.], 0.0, off))?;
            let (lo, hi) = bbox(cx, &list).unwrap_or((lo, hi));
            Ok(json!({"moved": obj, "offset": off, "bbox": {"x": [lo[0], hi[0]], "y": [lo[1], hi[1]], "z": [lo[2], hi[2]]}}))
        })(),
        "align_lay_on_face" => (|| {
            let obj = req(a, "object")?;
            let list = targets(cx, obj, doc)?;
            let fc = face_of(cx, &list[0], req(a, "face")?)?;
            let n =
                arr3(fc.get("plane_normal")).ok_or_else(|| te("bad_subname", format!("'{}' is not a planar face", req(a, "face").unwrap_or(""))))?;
            // turn the face normal onto -Z (the face down)
            let d = -n[2];
            let (axis, deg) = if d < -1.0 + 1e-9 { ([1.0, 0.0, 0.0], 180.0) } else { ([n[1], -n[0], 0.0], d.clamp(-1.0, 1.0).acos().to_degrees()) };
            let (lo, hi) = bbox(cx, &list).ok_or_else(|| te("no_shape", "no geometry"))?;
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
            let m = mat_mul(
                &mat_from([0., 0., 1.], 0.0, c),
                &mat_mul(&mat_from(axis, deg, [0.0; 3]), &mat_from([0., 0., 1.], 0.0, [-c[0], -c[1], -c[2]])),
            );
            if deg.abs() > 1e-9 {
                apply_transform(cx, &list, &m)?;
            }
            if b(a, "drop_to_floor", true)
                && let Some((lo, _)) = bbox(cx, &list)
            {
                apply_transform(cx, &list, &mat_from([0., 0., 1.], 0.0, [0.0, 0.0, -lo[2]]))?;
            }
            let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-12);
            Ok(
                json!({"moved": obj, "rotation_deg": (deg * 1000.0).round() / 1000.0, "rotation_axis": if deg.abs() < 1e-9 { json!([0, 0, 1]) } else { json!([axis[0] / l, axis[1] / l, axis[2] / l]) }, "face_normal_before": n}),
            )
        })(),

        // ======================= direct edit =======================
        "direct_push_pull" => (|| {
            let body = super::tools_app::body_of(cx, req(a, "object")?, doc)?;
            let d = f(a, "distance", 0.0);
            cx.leave_sketch();
            let list: Vec<String> = serde_json::from_value(a.get("faces").cloned().unwrap_or_default()).unwrap_or_default();
            let mut planar = Vec::new();
            let mut curved = Vec::new();
            for fname in &list {
                let fc = face_of(cx, &body, fname)?;
                let p = fc.get("point").cloned().unwrap_or_default();
                if fc.get("plane_normal").is_some_and(|n| !n.is_null()) { planar.push(p) } else { curved.push(p) }
            }
            if !planar.is_empty() {
                cx.run("solid.offset_face", json!({"faces": planar, "distance": d, "body": body}))?;
            }
            for p in curved {
                cx.run("solid.press_pull", json!({"face": p, "distance": d}))?;
            }
            Ok(in_place(cx, &body))
        })(),
        "direct_fillet" | "direct_chamfer" => (|| {
            let edges = a.get("edges").and_then(Value::as_array).cloned().unwrap_or_default();
            let chamfer = name == "direct_chamfer";
            let size = if chamfer { f(a, "size", 1.0) } else { f(a, "radius", 1.0) };
            let body = blend(cx, req(a, "object")?, &edges, size, chamfer, doc)?;
            Ok(in_place(cx, &body))
        })(),

        // ======================= PartDesign =======================
        "pd_create_body" => (|| {
            cx.use_doc(doc)?;
            let n = new_pd_body(cx, s(a, "label").unwrap_or("Body"))?;
            Ok(json!({"Name": n, "Label": n, "TypeId": "PartDesign::Body"}))
        })(),
        "pd_active_body" => (|| {
            cx.use_doc(doc)?;
            Ok(match cx.st.active_body.clone() {
                Some(b) if matches!(resolve(cx, &b, None), Ok(Obj::Component { .. })) => json!({"Name": b, "Label": b, "TypeId": "PartDesign::Body"}),
                _ => Value::Null,
            })
        })(),
        "pd_create_sketch" => (|| {
            let body = pd_body(cx, s(a, "body"), doc)?;
            cx.leave_sketch();
            activate(cx, Some(&body));
            let pl = s(a, "plane").unwrap_or("XY");
            let up = pl.to_ascii_uppercase().replace(['_', ' '], "").replace("PLANE", "");
            let plane = match up.as_str() {
                "XY" | "XZ" | "YZ" => json!(up),
                _ => {
                    let (obj, sub) = pl
                        .rsplit_once('.')
                        .ok_or_else(|| te("bad_support", format!("Unknown sketch plane '{pl}' (use XY, XZ, YZ or Object.FaceN)")))?;
                    let sb = super::tools_app::body_of(cx, obj, None)?;
                    let fc = face_of(cx, &sb, sub)?;
                    if fc.get("plane_normal").is_none_or(Value::is_null) {
                        return Err(te("bad_support", format!("'{pl}' is not a planar face")));
                    }
                    json!({"face": fc.get("point"), "body": sb})
                }
            };
            let label = s(a, "label").unwrap_or("Sketch");
            let r = cx.run("sketch.create", json!({"plane": plane, "name": label}))?;
            let n = r.get("name").and_then(Value::as_str).unwrap_or(label).to_string();
            cx.run("sketch.finish", json!({}))?;
            Ok(json!({"Name": n, "Label": n, "TypeId": "Sketcher::SketchObject"}))
        })(),
        "pd_pad" => {
            let l = f(a, "length", 10.0);
            let dir = if b(a, "midplane", false) {
                "symmetric"
            } else if b(a, "reversed", false) {
                "negative"
            } else {
                "positive"
            };
            pd_feature(cx, a, "solid.extrude", json!({"distance": l, "direction": dir}), false, "Pad", "PartDesign::Pad")
        }
        "pd_pocket" => {
            let dir = if b(a, "reversed", false) { "positive" } else { "negative" };
            pd_feature(cx, a, "solid.extrude", json!({"distance": f(a, "length", 10.0), "direction": dir}), true, "Pocket", "PartDesign::Pocket")
        }
        "pd_revolution" => (|| {
            let axis = match s(a, "axis").unwrap_or("V_Axis") {
                "V_Axis" => "y",
                "H_Axis" => "x",
                other => return Err(te("bad_axis", format!("Unsupported axis '{other}' (V_Axis, H_Axis)"))),
            };
            pd_feature(
                cx,
                a,
                "solid.revolve",
                json!({"axis": axis, "angle": format!("{} deg", f(a, "angle_deg", 360.0).min(360.0))}),
                false,
                "Revolution",
                "PartDesign::Revolution",
            )
        })(),
        "pd_fillet" | "pd_chamfer" => (|| {
            let edges = a.get("edges").and_then(Value::as_array).cloned().unwrap_or_default();
            let chamfer = name == "pd_chamfer";
            let size = if chamfer { f(a, "size", 1.0) } else { f(a, "radius", 1.0) };
            blend(cx, req(a, "base_feature")?, &edges, size, chamfer, doc)?;
            let (base, ty) = if chamfer { ("Chamfer", "PartDesign::Chamfer") } else { ("Fillet", "PartDesign::Fillet") };
            let n = name_feature(cx, Some(s(a, "label").unwrap_or(base)));
            Ok(json!({"Name": n, "Label": n, "TypeId": ty}))
        })(),

        // ======================= Sketcher =======================
        "sketch_add_line" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let idx = add_curves(
                cx,
                &sk,
                "sketch.line",
                json!({"points": [[f(a, "x1", 0.0), f(a, "y1", 0.0)], [f(a, "x2", 0.0), f(a, "y2", 0.0)]], "construction": b(a, "construction", false)}),
            )?;
            Ok(json!({"sketch": sk, "geometry_index": idx.first()}))
        })(),
        "sketch_add_circle" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let r = f(a, "r", 0.0);
            if r <= 0.0 {
                return Err(te("bad_value", "Radius must be positive"));
            }
            let idx = add_curves(
                cx,
                &sk,
                "sketch.circle.center",
                json!({"center": [f(a, "cx", 0.0), f(a, "cy", 0.0)], "radius": r, "construction": b(a, "construction", false)}),
            )?;
            Ok(json!({"sketch": sk, "geometry_index": idx.first()}))
        })(),
        "sketch_add_arc" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let (c, r) = ([f(a, "cx", 0.0), f(a, "cy", 0.0)], f(a, "r", 0.0));
            if r <= 0.0 {
                return Err(te("bad_value", "Radius must be positive"));
            }
            let (a0, a1) = (f(a, "start_deg", 0.0), f(a, "end_deg", 90.0));
            let mut sweep = a1 - a0;
            while sweep <= 1e-9 {
                sweep += 360.0;
            }
            let start = [c[0] + r * a0.to_radians().cos(), c[1] + r * a0.to_radians().sin()];
            let idx = add_curves(
                cx,
                &sk,
                "sketch.arc.center_point",
                json!({"center": c, "start": start, "sweep": sweep, "construction": b(a, "construction", false)}),
            )?;
            Ok(json!({"sketch": sk, "geometry_index": idx.first()}))
        })(),
        "sketch_add_rectangle" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let (x, y) = (f(a, "x", 0.0), f(a, "y", 0.0));
            let idx =
                add_curves(cx, &sk, "sketch.rectangle.two_point", json!({"p0": [x, y], "p1": [x + f(a, "width", 0.0), y + f(a, "height", 0.0)]}))?;
            Ok(json!({"sketch": sk, "geometry_indices": idx}))
        })(),
        "sketch_add_constraint" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let info = sketch_info(cx, &sk);
            let ids = curve_ids(&info);
            let args = a.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
            let (cmd, params) = constraint(&ids, req(a, "type")?, &args, a.get("value").and_then(Value::as_f64))?;
            in_sketch(cx, &sk, |cx| cx.run(cmd, params))?;
            let n = sketch_info(cx, &sk).get("constraints").and_then(Value::as_array).map(Vec::len).unwrap_or(1);
            Ok(json!({"sketch": sk, "constraint_index": n.saturating_sub(1), "applied": true}))
        })(),
        "sketch_get_geometry" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let info = sketch_info(cx, &sk);
            let list = info.get("curves").and_then(Value::as_array).cloned().unwrap_or_default();
            Ok(json!(
                list.iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let ty = match c.get("type").and_then(Value::as_str).unwrap_or("") {
                            "line" => "LineSegment",
                            "circle" => "Circle",
                            "arc" => "ArcOfCircle",
                            "ellipse" => "Ellipse",
                            "spline" | "bspline" => "BSplineCurve",
                            other => other,
                        };
                        let mut g = json!({"index": i, "id": c.get("id"), "type": ty, "construction": c.get("construction").and_then(Value::as_bool).unwrap_or(false)});
                        let sa = c.get("start_at").cloned();
                        let ea = c.get("end_at").cloned();
                        if ty == "LineSegment" {
                            g["start"] = sa.clone().unwrap_or(Value::Null);
                            g["end"] = ea.clone().unwrap_or(Value::Null);
                        }
                        if let Some(r) = c.get("radius") {
                            let ctr = c.get("center_at").or_else(|| c.get("center")).cloned().unwrap_or(Value::Null);
                            g["center"] = ctr.clone();
                            g["radius"] = r.clone();
                            if ty == "ArcOfCircle" {
                                let ang = |p: &Option<Value>| -> Option<f64> {
                                    let p = p.as_ref()?.as_array()?;
                                    let cc = ctr.as_array()?;
                                    Some((p.get(1)?.as_f64()? - cc.get(1)?.as_f64()?).atan2(p.first()?.as_f64()? - cc.first()?.as_f64()?).to_degrees())
                                };
                                g["start_deg"] = json!(ang(&sa));
                                g["end_deg"] = json!(ang(&ea));
                            }
                        }
                        g
                    })
                    .collect::<Vec<_>>()
            ))
        })(),
        "sketch_delete_geometry" => (|| {
            let sk = sketch_name(cx, req(a, "sketch")?, doc)?;
            let ids = curve_ids(&sketch_info(cx, &sk));
            let idx: Vec<i64> = serde_json::from_value(a.get("indices").cloned().unwrap_or_default()).unwrap_or_default();
            let ents: Vec<String> = idx.iter().filter_map(|i| usize::try_from(*i).ok().and_then(|i| ids.get(i).cloned())).collect();
            if !ents.is_empty() {
                in_sketch(cx, &sk, |cx| cx.run("sketch.delete", json!({"entities": ents})))?;
            }
            Ok(json!({"sketch": sk, "deleted": idx}))
        })(),

        // ======================= Draft =======================
        "draft_line" => (|| {
            let (p1, p2) = (a.get("p1").cloned().unwrap_or_default(), a.get("p2").cloned().unwrap_or_default());
            let sk = draft_sketch(cx, "Line", f(&p1, "z", 0.0))?;
            cx.run("sketch.line", json!({"points": [pt2(&p1), pt2(&p2)]}))?;
            finish_draft(cx, &sk, "Part::Part2DObjectPython")
        })(),
        "draft_wire" => (|| {
            let pts = a.get("points").and_then(Value::as_array).cloned().unwrap_or_default();
            if pts.len() < 2 {
                return Err(te("bad_value", "A wire needs at least 2 points"));
            }
            let closed = b(a, "closed", false) || b(a, "face", false);
            let sk = draft_sketch(cx, "Wire", f(&pts[0], "z", 0.0))?;
            cx.run("sketch.line", json!({"points": pts.iter().map(pt2).collect::<Vec<_>>(), "closed": closed}))?;
            let r = finish_draft(cx, &sk, "Part::Part2DObjectPython")?;
            if closed && b(a, "face", false) {
                cx.run("surface.patch", json!({"sketch": sk}))?;
            }
            Ok(r)
        })(),
        "draft_circle" => (|| {
            let c = a.get("center").cloned().unwrap_or_default();
            let r = f(a, "radius", 0.0);
            if r <= 0.0 {
                return Err(te("bad_value", "Radius must be positive"));
            }
            let sk = draft_sketch(cx, "Circle", f(&c, "z", 0.0))?;
            cx.run("sketch.circle.center", json!({"center": pt2(&c), "radius": r}))?;
            let out = finish_draft(cx, &sk, "Part::Part2DObjectPython")?;
            if b(a, "face", false) {
                cx.run("surface.patch", json!({"sketch": sk}))?;
            }
            Ok(out)
        })(),
        "draft_text" => (|| {
            let p = a.get("position").cloned().unwrap_or_default();
            let sk = draft_sketch(cx, "Text", f(&p, "z", 0.0))?;
            cx.run("sketch.text", json!({"text": req(a, "text")?, "at": pt2(&p), "height": f(a, "size", 5.0)}))?;
            finish_draft(cx, &sk, "App::FeaturePython")
        })(),

        // ======================= measure =======================
        "measure_bounding_box" => (|| {
            let obj = req(a, "object")?;
            let o = resolve(cx, obj, doc)?;
            let list = bodies_of(cx, &o);
            let (lo, hi) = bbox(cx, &list).ok_or_else(|| te("no_shape", format!("Object '{obj}' has no Shape")))?;
            Ok(json!({"XMin": lo[0], "YMin": lo[1], "ZMin": lo[2], "XMax": hi[0], "YMax": hi[1], "ZMax": hi[2],
                      "XLength": hi[0] - lo[0], "YLength": hi[1] - lo[1], "ZLength": hi[2] - lo[2],
                      "Center": obj_xyz([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0])}))
        })(),
        "measure_mass_properties" => (|| {
            let obj = req(a, "object")?;
            let body = super::tools_app::body_of(cx, obj, doc)?;
            let m = measure(cx, &body);
            let length: f64 = edges(cx, &body).iter().filter_map(|e| e.get("length").and_then(Value::as_f64)).sum();
            // A body whose surface does not close has no volume and no centre of mass. The
            // fallback here used to be the origin, which is a place, and reads as one.
            let com = arr3(m.get("center_of_mass"));
            let mut out = json!({"Volume": m.get("volume_mm3"), "Area": m.get("area_mm2"), "Length": length,
                                 "CenterOfMass": com.map(obj_xyz)});
            if let Some(why) = m.get("warning") {
                out["Warning"] = why.clone();
            }
            Ok(out)
        })(),
        "measure_distance" => (|| {
            let ia = measure_item(cx, req(a, "a")?, doc)?;
            let ib = measure_item(cx, req(a, "b")?, doc)?;
            let m = cx.run("inspect.measure", json!({"items": [ia, ib]}))?;
            let d = m.get("distance_mm").and_then(Value::as_f64).ok_or_else(|| te("measure_failed", "No distance between these"))?;
            let p = arr3(m.get("from")).unwrap_or([0.0; 3]);
            let q = arr3(m.get("to")).unwrap_or([0.0; 3]);
            Ok(json!({"distance": d, "points": [{"a": obj_xyz(p), "b": obj_xyz(q)}]}))
        })(),
        _ => return None,
    })
}
