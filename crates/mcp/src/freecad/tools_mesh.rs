//! Import/export, meshes and reverse engineering (mesh → solid).

use serde_json::{Value, json};

use super::tools_app::{bodies_of, body_names, doc_name, is_mesh, resolve};
use super::tools_model::{bbox, new_bodies, unique_body};
use super::{Cx, R, ToolError, b, f, req, s, te};

fn ext(path: &str) -> String {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}
fn temp(name: &str) -> String {
    std::env::temp_dir().join(format!("solidcraft_aibridge_{}_{name}", std::process::id())).to_string_lossy().into_owned()
}

/// Bring a file into the design (or open a design file); returns the new bodies.
fn import(cx: &mut Cx, path: &str, doc: Option<&str>) -> Result<Vec<String>, ToolError> {
    if !std::path::Path::new(path).exists() {
        return Err(te("no_file", format!("File not found: {path}")));
    }
    cx.leave_sketch();
    if let Some(d) = doc.filter(|d| !d.is_empty())
        && cx.use_doc(Some(d)).is_err()
    {
        cx.run("file.new", json!({"name": d}))?;
    }
    let before = body_names(cx);
    match ext(path).as_str() {
        "solidcraft" | "json" => {
            cx.run("file.new", json!({}))?;
            cx.run("doc.open", json!({"path": path}))?;
            return Ok(body_names(cx));
        }
        "step" | "stp" | "iges" | "igs" | "fcstd" | "3dm" => drop(cx.run("file.insert_step", json!({"path": path}))?),
        "3mf" | "stl" | "obj" | "glb" | "gltf" | "plasticity" => drop(cx.run("file.insert_mesh", json!({"path": path}))?),
        e => return Err(te("unsupported_format", format!("Unsupported format: .{e}"))),
    }
    Ok(new_bodies(cx, &before))
}

fn export_bodies(cx: &mut Cx, a: &Value) -> Result<Vec<String>, ToolError> {
    cx.use_doc(s(a, "doc"))?;
    let names: Vec<String> = serde_json::from_value(a.get("objects").cloned().unwrap_or_default()).unwrap_or_default();
    if names.is_empty() {
        let ins = super::tools_app::inspect(cx);
        return Ok(ins
            .get("bodies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|b| b.get("visible").and_then(Value::as_bool) != Some(false))
            .filter_map(|b| b.get("name").and_then(Value::as_str).map(str::to_string))
            .collect());
    }
    let mut out = Vec::new();
    for n in &names {
        let o = resolve(cx, n, None)?;
        out.extend(bodies_of(cx, &o));
    }
    Ok(out)
}

fn mesh_body(cx: &mut Cx, name: &str, doc: Option<&str>) -> Result<String, ToolError> {
    let o = resolve(cx, name, doc)?;
    let body = bodies_of(cx, &o).into_iter().next().ok_or_else(|| te("no_shape", format!("Object '{name}' has no Shape")))?;
    Ok(body)
}

/// A body's triangles (via an ASCII STL).
fn triangles(cx: &mut Cx, body: &str) -> Result<Vec<[[f64; 3]; 3]>, ToolError> {
    let path = temp("slice.stl");
    cx.run("file.save_mesh", json!({"path": path, "bodies": [body], "format": "stl", "ascii": true}))?;
    let text = std::fs::read_to_string(&path).map_err(|e| te("io_error", e.to_string()))?;
    let _ = std::fs::remove_file(&path);
    let mut v: Vec<[f64; 3]> = Vec::new();
    for l in text.lines() {
        if let Some(r) = l.trim().strip_prefix("vertex") {
            let n: Vec<f64> = r.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if n.len() == 3 {
                v.push([n[0], n[1], n[2]]);
            }
        }
    }
    Ok(v.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect())
}

/// The plane sketches use for slices along an axis: (axis index, u index, v index).
fn axis_frame(axis: &str) -> (usize, usize, usize) {
    match axis {
        "x" => (0, 1, 2),
        "y" => (1, 2, 0),
        _ => (2, 0, 1),
    }
}
fn plane_json(axis: &str, h: f64) -> Value {
    match axis {
        "x" => json!({"origin": [h, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}),
        "y" => json!({"origin": [0, h, 0], "x_dir": [0, 0, 1], "y_dir": [1, 0, 0]}),
        _ => json!({"origin": [0, 0, h], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}),
    }
}
/// Points where the triangles cross the plane `p[k] = h`, in plane coordinates.
fn slice(tris: &[[[f64; 3]; 3]], axis: &str, h: f64) -> Vec<[f64; 2]> {
    let (k, u, v) = axis_frame(axis);
    let mut out = Vec::new();
    for t in tris {
        for i in 0..3 {
            let (p, q) = (t[i], t[(i + 1) % 3]);
            let (dp, dq) = (p[k] - h, q[k] - h);
            if (dp < 0.0) != (dq < 0.0) && (dp - dq).abs() > 1e-12 {
                let s = dp / (dp - dq);
                out.push([p[u] + (q[u] - p[u]) * s, p[v] + (q[v] - p[v]) * s]);
            }
        }
    }
    out
}
/// `n` points evenly spaced along a closed ring, the first of them its first point.
///
/// A loft runs through its sections point by point, so every section has to carry the same
/// count: the hull of a slice has however many points the mesh happens to give it, and sections
/// that disagree make a surface that misses its own edges by millimetres.
///
/// Spacing by perimeter, not by angle from the middle. Angle is the better answer for sections
/// around one axis, because the same direction means the same side of the part in each of
/// them; it is the worse answer here, because each slice's middle is its own, and on a scan
/// whose outline wanders off the axis -- a microscope's tube over its foot -- the middle moves
/// from slice to slice and takes every point with it, until the loft's control net doubles back
/// on itself.
fn resample(ring: &[[f64; 2]], n: usize) -> Vec<[f64; 2]> {
    let m = ring.len();
    if m < 3 || n < 3 {
        return ring.to_vec();
    }
    let side = |i: usize| {
        let (a, b) = (ring[i], ring[(i + 1) % m]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    let total: f64 = (0..m).map(side).sum();
    if !total.is_finite() || total <= 0.0 {
        return ring.to_vec();
    }
    let mut out = Vec::with_capacity(n);
    let (mut i, mut walked) = (0usize, 0.0);
    for k in 0..n {
        let want = total * k as f64 / n as f64;
        while i + 1 < m && walked + side(i) < want {
            walked += side(i);
            i += 1;
        }
        let (a, b) = (ring[i], ring[(i + 1) % m]);
        let t = if side(i) > 0.0 { ((want - walked) / side(i)).clamp(0.0, 1.0) } else { 0.0 };
        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out
}

/// Convex hull (counter-clockwise), resampled to exactly `max` points: that is what
/// `points_per_slice` asks for, and the hull of a slice has however many points the mesh
/// happens to give it, which differs from slice to slice.
fn hull(mut pts: Vec<[f64; 2]>, max: usize) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f64; 2]> = Vec::new();
    for pass in 0..2 {
        let start = h.len();
        let iter: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while h.len() >= start + 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 1e-12 {
                h.pop();
            }
            h.push(p);
        }
        h.pop();
    }
    if h.len() < 3 { h } else { resample(&h, max) }
}

/// Loft convex slices of a mesh along one axis; returns the new body.
fn loft_axis(
    cx: &mut Cx,
    tris: &[[[f64; 3]; 3]],
    lo: f64,
    hi: f64,
    axis: &str,
    a: &Value,
    step: Option<f64>,
    n_default: usize,
    label: &str,
) -> Result<(String, Vec<String>), ToolError> {
    let margin = f(a, "margin", 1.5).max(0.0);
    let (lo, hi) = (lo + margin, hi - margin);
    if hi - lo <= 1e-6 {
        return Err(te("too_thin", format!("The mesh is too thin along {axis} for margin {margin} mm")));
    }
    let n = match step {
        Some(st) if st > 0.0 => (((hi - lo) / st).floor() as usize + 1).clamp(2, 400),
        _ => a.get("n_slices").and_then(Value::as_u64).map(|n| n as usize).unwrap_or(n_default).clamp(2, 400),
    };
    let max_pts = a.get("points_per_slice").and_then(Value::as_u64).unwrap_or(40).clamp(12, 200) as usize;
    let mut sketches = Vec::new();
    for i in 0..n {
        let h = lo + (hi - lo) * i as f64 / (n - 1) as f64;
        let ring = hull(slice(tris, axis, h), max_pts);
        if ring.len() < 3 {
            continue;
        }
        let r = cx.run("sketch.create", json!({"plane": plane_json(axis, h), "name": format!("{label} slice {}{}", axis.to_uppercase(), i + 1)}))?;
        let sk = r.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        cx.run("sketch.line", json!({"points": ring, "closed": true}))?;
        cx.run("sketch.finish", json!({}))?;
        sketches.push(sk);
    }
    if sketches.len() < 2 {
        return Err(te("no_sections", "Fewer than two usable cross-sections"));
    }
    let before = body_names(cx);
    let sections: Vec<Value> = sketches.iter().map(|s| json!({"sketch": s})).collect();
    cx.run("solid.loft", json!({"sections": sections, "operation": "new", "name": label, "body_name": label}))?;
    let body = new_bodies(cx, &before).into_iter().next().ok_or_else(|| te("loft_failed", "The loft made no body"))?;
    let _ = cx.cmd("browser.visibility", json!({"visible": false, "items": &sketches}));
    Ok((body, sketches))
}

/// The component (id) that holds a body, unless it's the design's root.
fn component_of(cx: &mut Cx, body: &str) -> Option<Value> {
    let comps = cx.cmd("component.list", json!({})).ok()?.get("components")?.as_array()?.clone();
    comps
        .into_iter()
        .find(|c| {
            c.get("parent").is_some_and(|p| !p.is_null())
                && c.get("bodies").and_then(Value::as_array).is_some_and(|b| b.iter().any(|x| x.as_str() == Some(body)))
        })
        .and_then(|c| c.get("id").cloned())
}

fn hide_body(cx: &mut Cx, body: &str) {
    let _ = cx.cmd("browser.visibility", json!({"visible": false, "items": [body]}));
}
fn rename_body(cx: &mut Cx, body: &str, want: &str) -> String {
    if body == want {
        return body.to_string();
    }
    let n = unique_body(cx, want);
    match cx.cmd("body.rename", json!({"body": body, "name": n})) {
        Ok(_) => n,
        Err(_) => body.to_string(),
    }
}

/// The tools of this file; `None` for a name that isn't one of them.
pub fn call(cx: &mut Cx, name: &str, a: &Value) -> Option<R> {
    Some(match name {
        "io_import" | "mesh_import" | "mesh_import_file" => (|| {
            let path = req(a, "path")?;
            let made = import(cx, path, s(a, "doc"))?;
            let doc = doc_name(cx);
            Ok(match name {
                "io_import" => json!({"doc": doc, "path": path, "objects": made}),
                _ => {
                    let first = made.first().cloned().unwrap_or_default();
                    json!({"Name": first, "Label": first, "TypeId": "Mesh::Feature", "doc": doc, "objects": made})
                }
            })
        })(),
        "io_export" | "mesh_export" => (|| {
            let path = req(a, "path")?;
            let bodies = export_bodies(cx, a)?;
            if bodies.is_empty() {
                return Err(te("nothing_to_export", "No objects to export"));
            }
            cx.leave_sketch();
            if name == "mesh_export" && matches!(ext(path).as_str(), "stl" | "3mf") {
                cx.run("file.save_mesh", json!({"path": path, "bodies": bodies}))?;
            } else {
                cx.run("file.export", json!({"path": path, "bodies": bodies}))?;
            }
            Ok(json!({"path": path, "exported": bodies}))
        })(),
        "shape_to_mesh" => (|| {
            let obj = req(a, "object")?;
            let body = mesh_body(cx, obj, s(a, "doc"))?;
            cx.leave_sketch();
            let want = s(a, "label").map(str::to_string).unwrap_or_else(|| format!("{obj}_mesh"));
            let path = temp("shape.stl");
            cx.run("file.save_mesh", json!({"path": path, "bodies": [body], "format": "stl"}))?;
            let before = body_names(cx);
            let r = cx.run("file.insert_mesh", json!({"path": path, "name": want}));
            let _ = std::fs::remove_file(&path);
            r?;
            let made = new_bodies(cx, &before);
            let m = made.first().cloned().ok_or_else(|| te("mesh_failed", "No mesh was made"))?;
            let n = rename_body(cx, &m, &want);
            let tris = triangles(cx, &n).map(|t| t.len()).unwrap_or(0);
            Ok(json!({"Name": n, "Label": n, "TypeId": "Mesh::Feature", "facets": tris}))
        })(),
        "mesh_to_shape" | "mesh_rebuild_cad" => (|| {
            let obj = req(a, "object")?;
            let body = mesh_body(cx, obj, s(a, "doc"))?;
            if !is_mesh(cx, &body) {
                return Err(te("not_a_mesh", format!("'{obj}' is not a mesh")));
            }
            cx.leave_sketch();
            let tol = f(a, "tolerance", if name == "mesh_to_shape" { 0.1 } else { 0.25 });
            // FreeCAD keeps the mesh and adds a shape: convert a copy (Convert Mesh works in place)
            let before = body_names(cx);
            cx.run("solid.move", json!({"bodies": [body], "translate": [0, 0, 0], "copy": true}))?;
            let copy = new_bodies(cx, &before).into_iter().next().ok_or_else(|| te("convert_failed", "Could not copy the mesh"))?;
            let want = s(a, "label").map(str::to_string).unwrap_or_else(|| format!("{obj}_solid"));
            let copy = rename_body(cx, &copy, &want);
            // Convert Mesh works in the mesh body's own component
            let home = component_of(cx, &copy);
            if let Some(c) = &home {
                cx.run("component.activate", json!({"component": c}))?;
            }
            let r = cx.run("mesh.convert", json!({"body": copy, "tolerance": format!("{tol} mm")}));
            if home.is_some() {
                let _ = cx.cmd("component.activate", json!({"component": "root"}));
            }
            if let Err(e) = r {
                let _ = cx.cmd("selection.delete", json!({"items": [{"type": "body", "name": copy}]}));
                return Err(e);
            }
            let out = new_bodies(cx, &before).into_iter().find(|n| !is_mesh(cx, n)).unwrap_or(copy);
            let hide = if name == "mesh_to_shape" { b(a, "hide_mesh", true) } else { b(a, "hide_source", false) };
            if hide && out != body && body_names(cx).contains(&body) {
                hide_body(cx, &body);
            }
            let m = cx
                .cmd("inspect.measure", json!({"bodies": [out]}))
                .ok()
                .and_then(|v| v.get("bodies").and_then(|b| b.get(0)).cloned())
                .unwrap_or_default();
            Ok(
                json!({"Name": out, "Label": out, "TypeId": "Part::Feature", "faces": m.get("faces"), "volume": m.get("volume_mm3"), "isSolid": !is_mesh(cx, &out)}),
            )
        })(),
        "mesh_loft_solid" => (|| {
            let obj = req(a, "mesh_object")?;
            let body = mesh_body(cx, obj, s(a, "doc"))?;
            cx.leave_sketch();
            let axis = s(a, "axis").unwrap_or("z").to_string();
            let tris = triangles(cx, &body)?;
            let (lo, hi) = bbox(cx, std::slice::from_ref(&body)).ok_or_else(|| te("no_shape", "The mesh is empty"))?;
            let k = axis_frame(&axis).0;
            let label = s(a, "result_label").unwrap_or("BodyFromMesh").to_string();
            let step = a.get("resolution_mm").and_then(Value::as_f64);
            let (made, sketches) = loft_axis(cx, &tris, lo[k], hi[k], &axis, a, step, 16, &label)?;
            let made = rename_body(cx, &made, &label);
            if !b(a, "keep_intermediate", false) {
                let _ = cx.cmd("browser.visibility", json!({"visible": false, "items": &sketches}));
            }
            if b(a, "hide_mesh_after", true) {
                hide_body(cx, &body);
            }
            let m = cx
                .cmd("inspect.measure", json!({"bodies": [made]}))
                .ok()
                .and_then(|v| v.get("bodies").and_then(|b| b.get(0)).cloned())
                .unwrap_or_default();
            Ok(json!({"Name": made, "Label": made, "axis": axis, "slices": sketches.len(), "volume": m.get("volume_mm3"), "isValid": true}))
        })(),
        "mesh_to_solid_multi_axis" => (|| {
            let obj = req(a, "mesh_object")?;
            let body = mesh_body(cx, obj, s(a, "doc"))?;
            cx.leave_sketch();
            let axes: Vec<String> = serde_json::from_value(a.get("axes").cloned().unwrap_or(json!(["z"]))).unwrap_or_else(|_| vec!["z".into()]);
            let tris = triangles(cx, &body)?;
            let (lo, hi) = bbox(cx, std::slice::from_ref(&body)).ok_or_else(|| te("no_shape", "The mesh is empty"))?;
            let label = s(a, "result_label").unwrap_or("BodyMulti").to_string();
            let step = Some(f(a, "resolution_mm", 5.0));
            let mut lofts = Vec::new();
            for ax in &axes {
                let k = axis_frame(ax).0;
                let (m, _) = loft_axis(cx, &tris, lo[k], hi[k], ax, a, step, 16, &format!("{label}_{}", ax.to_uppercase()))?;
                lofts.push(m);
            }
            let mut note = Value::Null;
            let result = if lofts.len() == 1 {
                lofts[0].clone()
            } else {
                let keep = b(a, "keep_per_axis_lofts", false);
                let tools: Vec<String> = lofts[1..].to_vec();
                match cx.run("solid.combine", json!({"target": lofts[0], "tools": tools, "operation": "intersect", "keep_tools": keep})) {
                    Ok(_) if keep => drop(cx.cmd("browser.visibility", json!({"visible": false, "items": tools}))),
                    Ok(_) => {}
                    // the kernel can't intersect two freeform lofts everywhere: keep the first
                    // axis's loft and the others (hidden) rather than lose the work
                    Err(e) => {
                        let _ = cx.cmd("browser.visibility", json!({"visible": false, "items": tools}));
                        note = json!(format!(
                            "The per-axis lofts could not be intersected ({}); the result is the {} loft, the others are kept hidden.",
                            e.message, axes[0]
                        ));
                    }
                }
                lofts[0].clone()
            };
            let result = rename_body(cx, &result, &label);
            if b(a, "hide_mesh_after", true) {
                hide_body(cx, &body);
            }
            let m = cx
                .cmd("inspect.measure", json!({"bodies": [result]}))
                .ok()
                .and_then(|v| v.get("bodies").and_then(|b| b.get(0)).cloned())
                .unwrap_or_default();
            let mut out = json!({"Name": result, "Label": result, "axes": axes, "volume": m.get("volume_mm3"), "isValid": true});
            if !note.is_null() {
                out["intersected"] = json!(false);
                out["note"] = note;
            }
            Ok(out)
        })(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_slices_to_a_square_hull() {
        let c = |x: f64, y: f64, z: f64| [x, y, z];
        // two triangles of each of the cube's four side walls
        let mut tris = Vec::new();
        for (a, b) in [((0., 0.), (1., 0.)), ((1., 0.), (1., 1.)), ((1., 1.), (0., 1.)), ((0., 1.), (0., 0.))] {
            tris.push([c(a.0, a.1, 0.), c(b.0, b.1, 0.), c(b.0, b.1, 1.)]);
            tris.push([c(a.0, a.1, 0.), c(b.0, b.1, 1.), c(a.0, a.1, 1.)]);
        }
        let h = hull(slice(&tris, "z", 0.5), 40);
        // Every section carries the count asked for, so a loft's sections match each other.
        assert_eq!(h.len(), 40);
        assert!(h.iter().all(|p| on_edge_of(p, 0.0, 1.0)), "{h:?}");
        // and they go right round the square, not all along one side
        for side in [0usize, 1] {
            assert!(h.iter().any(|p| (p[side] - 0.0).abs() < 1e-9), "nothing on side {side} = 0: {h:?}");
            assert!(h.iter().any(|p| (p[side] - 1.0).abs() < 1e-9), "nothing on side {side} = 1: {h:?}");
        }
    }

    /// Whether a point sits on the boundary of the axis-aligned square from `lo` to `hi`.
    fn on_edge_of(p: &[f64; 2], lo: f64, hi: f64) -> bool {
        let e = 1e-9;
        let inside = |v: f64| (lo - e..=hi + e).contains(&v);
        let at_end = |v: f64| (v - lo).abs() < e || (v - hi).abs() < e;
        (at_end(p[0]) && inside(p[1])) || (at_end(p[1]) && inside(p[0]))
    }

    #[test]
    fn a_resampled_ring_keeps_its_shape_and_its_count() {
        let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        for n in [12, 17, 40] {
            let r = resample(&square, n);
            assert_eq!(r.len(), n);
            // every point is on the square's boundary
            assert!(r.iter().all(|p| on_edge_of(p, 0.0, 2.0)), "{r:?}");
            assert_eq!(r[0], [0.0, 0.0], "it starts where the ring starts");
            // evenly spaced: consecutive points are the perimeter over n apart, bar the corners
            let step = 8.0 / n as f64;
            for w in r.windows(2) {
                let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
                assert!(d <= step + 1e-9, "{d} apart, more than {step}");
            }
        }
        // However odd a count, the ring is still gone round once and not overshot.
        assert_eq!(resample(&square, 3).len(), 3);
        // a ring with no length is left alone, not reduced to n copies of one point
        assert_eq!(resample(&[[1.0, 1.0], [1.0, 1.0]], 8).len(), 2);
    }
}
