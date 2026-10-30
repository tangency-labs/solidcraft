//! Every command that adds sketch geometry returns `created: {points, curves, constraints,
//! dimensions}` with the real ids of what it added, so a script can dimension and constrain the
//! new geometry right away; ids are never reused after a delete.

use serde_json::{Value, json};
use solidcraft_sketch::{CurveKind, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sketch(s: &Session) -> Sketch {
    s.doc.sketch(s.active_sketch.unwrap()).unwrap().clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

/// The ids in `r.created` exist in the active sketch, and each curve and point takes a
/// dimension (driven, so it never conflicts) and a fix right away. `before`: the sketch before
/// the command. Returns `created`.
fn check(s: &mut Session, cmd: &str, r: &Value, before: &Sketch) -> Value {
    let c = r.get("created").cloned().unwrap_or_else(|| panic!("{cmd}: no `created` in {r}"));
    for k in ["points", "curves", "constraints", "dimensions"] {
        assert!(c[k].is_array(), "{cmd}: created.{k} in {c}");
    }
    let sk = sketch(s);
    let curves = strs(&c["curves"]);
    let points = strs(&c["points"]);
    assert!(!curves.is_empty() || !points.is_empty() || c.get("wires").is_some(), "{cmd}: nothing created: {c}");
    for id in &points {
        assert!(sk.point_index(id).is_some(), "{cmd}: point {id} not in the sketch");
        assert_ne!(id, "origin", "{cmd}");
    }
    for id in strs(&c["constraints"]).iter().chain(&strs(&c["dimensions"])) {
        assert!(sk.constraints.iter().any(|k| &k.id == id), "{cmd}: constraint {id} not in the sketch");
    }
    for id in strs(&c["dimensions"]) {
        assert!(sk.constraints.iter().any(|k| k.id == id && k.kind.is_dimension()), "{cmd}: {id} is not a dimension");
    }
    for id in strs(&c.get("wires").cloned().unwrap_or_default()) {
        assert!(sk.wire_index(&id).is_some(), "{cmd}: wire {id} not in the sketch");
    }
    // The new curves in the command's own list (where it has one) are in `created`.
    let wires = strs(&c.get("wires").cloned().unwrap_or_default());
    for id in strs(&r["curves"]) {
        let old = before.curve_index(&id).is_some() || before.wire_index(&id).is_some();
        assert!(old || curves.contains(&id) || wires.contains(&id), "{cmd}: returned curve {id} missing from created {c}");
    }
    for id in curves.iter().chain(&points) {
        assert!(before.curve_index(id).is_none() && before.point_index(id).is_none(), "{cmd}: {id} is not new");
    }
    for id in &curves {
        let i = sk.curve_index(id).unwrap_or_else(|| panic!("{cmd}: curve {id} not in the sketch"));
        let measurable = matches!(sk.curves[i].kind, CurveKind::Line { .. } | CurveKind::Circle { .. } | CurveKind::Arc { .. });
        if measurable {
            run(s, "sketch.dimension", json!({"entities": [id], "driven": true}));
        }
        run(s, "sketch.constraint.fix", json!({"entity": id, "fixed": true}));
        run(s, "edit.undo", json!({}));
        if measurable {
            run(s, "edit.undo", json!({}));
        }
    }
    for id in &points {
        run(s, "sketch.dimension", json!({"entities": [id, "origin"], "driven": true}));
        run(s, "edit.undo", json!({}));
    }
    c
}

/// Run `cmd` in `s` and check what it reports.
fn made(s: &mut Session, cmd: &str, p: Value) -> Value {
    let before = sketch(s);
    let r = run(s, cmd, p);
    check(s, cmd, &r, &before)
}

#[test]
fn drawing_commands_return_the_ids_they_create() {
    let cases: Vec<(&str, Value)> = vec![
        ("sketch.line", json!({"points": [[0, 0], [10, 0], [10, 5]]})),
        ("sketch.rectangle.two_point", json!({"p0": [1, 1], "p1": [21, 15]})),
        ("sketch.rectangle.three_point", json!({"p0": [0, 0], "p1": [20, 5], "p2": [18, 15]})),
        ("sketch.rectangle.center", json!({"center": [3, 2], "corner": [13, 9]})),
        ("sketch.circle.center", json!({"center": [5, 5], "diameter": 8})),
        ("sketch.circle.two_point", json!({"p0": [0, 0], "p1": [10, 0]})),
        ("sketch.circle.three_point", json!({"p0": [0, 0], "p1": [10, 0], "p2": [5, 6]})),
        ("sketch.arc.three_point", json!({"start": [0, 0], "end": [10, 0], "through": [5, 4]})),
        ("sketch.arc.center_point", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10]})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 10, "sides": 6})),
        ("sketch.polygon.circumscribed", json!({"center": [0, 0], "radius": 10, "sides": 5})),
        ("sketch.polygon.edge", json!({"p0": [0, 0], "p1": [10, 0], "sides": 4})),
        ("sketch.slot.center_to_center", json!({"p0": [0, 0], "p1": [20, 0], "width": 6})),
        ("sketch.slot.overall", json!({"p0": [0, 0], "p1": [20, 0], "width": 6})),
        ("sketch.slot.center_point", json!({"center": [0, 0], "end": [10, 0], "width": 4})),
        ("sketch.slot.arc_three_point", json!({"start": [0, 0], "through": [10, 5], "end": [20, 0], "width": 3})),
        ("sketch.slot.arc_center", json!({"center": [0, 0], "start": [10, 0], "end": [0, 10], "width": 3})),
        ("sketch.point", json!({"point": [3, 4]})),
        ("sketch.line.midpoint", json!({"mid": [0, 0], "end": [10, 5]})),
        ("sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], [5, 5], [10, 0]]})),
        ("sketch.spline.fit_point", json!({"points": [[0, 0], [5, 5], [10, 0]], "periodic": true})),
        ("sketch.spline.control_point", json!({"points": [[0, 0], [5, 5], [10, 0], [15, 5]]})),
        ("sketch.spline.control_point_5", json!({"points": [[0, 0], [3, 5], [6, 0], [9, 5], [12, 0], [15, 5]]})),
        ("sketch.conic", json!({"start": [0, 0], "end": [10, 0], "apex": [5, 6]})),
        ("sketch.text", json!({"text": "IT", "at": [0, 0], "height": 10})),
    ];
    for (cmd, p) in cases {
        let mut s = new_sketch();
        made(&mut s, cmd, p);
    }
}

#[test]
fn drawn_ids_take_driving_dimensions_and_constraints() {
    let mut s = new_sketch();
    let c = made(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "connect": false}));
    let l = strs(&c["curves"]);
    let pts = strs(&c["points"]);
    assert_eq!((l.len(), pts.len()), (1, 2), "{c}");
    run(&mut s, "sketch.constraint.coincident", json!({"a": pts[0], "b": "origin"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": l[0]}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": [l[0]], "value": 30}));
    assert_eq!(strs(&d["created"]["dimensions"]).len(), 1, "{d}");
    let sk = sketch(&s);
    let end = sk.point(sk.point_index(&pts[1]).unwrap()).unwrap();
    assert!((end.x - 30.0).abs() < 1e-6 && end.y.abs() < 1e-6, "{end:?}");

    let c = made(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 4}));
    let circle = strs(&c["curves"]);
    run(&mut s, "sketch.dimension", json!({"entities": [circle[0]], "type": "diameter", "value": 12}));
    let sk = sketch(&s);
    let i = sk.curve_index(&circle[0]).unwrap();
    assert!(matches!(sk.curves[i].kind, CurveKind::Circle { r, .. } if (r - 6.0).abs() < 1e-6));
}

#[test]
fn modify_commands_return_the_ids_they_create() {
    // Corner tools: an open L of two lines.
    for (cmd, extra) in [
        ("sketch.fillet", json!({"radius": 2})),
        ("sketch.chamfer.equal_distance", json!({"distance": 2})),
        ("sketch.chamfer.distance_angle", json!({"distance": 2, "angle": 40})),
        ("sketch.chamfer.two_distance", json!({"distance": 2, "distance2": 3})),
    ] {
        let mut s = new_sketch();
        let c = made(&mut s, "sketch.line", json!({"points": [[0, 10], [0, 0], [10, 0]]}));
        let l = strs(&c["curves"]);
        let mut p = extra;
        p["point"] = json!(format!("{}.end", l[0]));
        let c = made(&mut s, cmd, p);
        assert!(!strs(&c["curves"]).is_empty(), "{cmd}: {c}");
    }

    let mut s = new_sketch();
    let rect = strs(&made(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}))["curves"]);
    let c = made(&mut s, "sketch.offset", json!({"curves": [rect[0]], "distance": 2, "side": [10, 5]}));
    assert_eq!(strs(&c["curves"]).len(), 4, "{c}");
    made(&mut s, "sketch.move", json!({"entities": [rect[0]], "translate": [0, -20], "copy": true}));
    made(&mut s, "sketch.pattern.rectangular", json!({"entities": [rect[2]], "count": 3, "spacing": 30}));
    let circle = strs(&made(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 2}))["curves"]);
    made(&mut s, "sketch.pattern.circular", json!({"entities": [circle[0]], "center": [90, 0], "count": 4}));
    let axis = strs(&made(&mut s, "sketch.line", json!({"points": [[200, -10], [200, 10]]}))["curves"]);
    let sm = strs(&made(&mut s, "sketch.circle.center", json!({"center": [190, 0], "radius": 2}))["curves"]);
    made(&mut s, "sketch.mirror", json!({"entities": [sm[0]], "line": axis[0]}));

    // Break and trim split a curve where another crosses it.
    let mut s = new_sketch();
    let h = strs(&made(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"]);
    made(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 5]]}));
    made(&mut s, "sketch.break", json!({"curve": h[0], "at": [5, 0]}));
    let mut s = new_sketch();
    let h = strs(&made(&mut s, "sketch.line", json!({"points": [[0, 0], [30, 0]]}))["curves"]);
    made(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 5]]}));
    made(&mut s, "sketch.line", json!({"points": [[20, -5], [20, 5]]}));
    made(&mut s, "sketch.trim", json!({"curve": h[0], "at": [15, 0]}));

    // Tangent tools and blends start from existing curves.
    let mut s = new_sketch();
    let l = strs(&made(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))["curves"]);
    made(&mut s, "sketch.arc.tangent", json!({"start": format!("{}.end", l[0]), "end": [20, 10]}));
    let a = strs(&made(&mut s, "sketch.line", json!({"points": [[0, 30], [10, 30]]}))["curves"]);
    let b = strs(&made(&mut s, "sketch.line", json!({"points": [[20, 40], [30, 40]]}))["curves"]);
    made(&mut s, "sketch.blend_curve", json!({"a": format!("{}.end", a[0]), "b": format!("{}.start", b[0])}));
    let x = strs(&made(&mut s, "sketch.line", json!({"points": [[100, 0], [140, 0]], "connect": false}))["curves"]);
    let y = strs(&made(&mut s, "sketch.line", json!({"points": [[100, 0], [100, 40]], "connect": false}))["curves"]);
    made(&mut s, "sketch.circle.two_tangent", json!({"curves": [x[0], y[0]], "radius": 5, "near": [106, 4]}));
    let z = strs(&made(&mut s, "sketch.line", json!({"points": [[100, 40], [140, 40]], "connect": false}))["curves"]);
    made(&mut s, "sketch.circle.three_tangent", json!({"curves": [x[0], y[0], z[0]], "near": [120, 20]}));
}

#[test]
fn dimensioned_drawing_reports_its_geometry_and_driving_dimensions() {
    // The creator runs nested inside sketch.draw.dimensioned; `created` must still cover the
    // dimensions the wrapper adds afterwards, not just the nested creator's geometry.
    for (cmd, p, dims) in [
        ("sketch.line", json!({"points": [[0, 0], [5, 5]]}), json!({"length": 10, "angle": 45})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}), json!({"width": 30, "height": 15})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 3}), json!({"diameter": 12})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 10, "sides": 6}), json!({"radius": 8})),
    ] {
        let mut s = new_sketch();
        let c = made(&mut s, "sketch.draw.dimensioned", json!({"command": cmd, "params": p, "dimensions": dims}));
        let n = dims.as_object().map_or(0, |d| d.len());
        assert_eq!(strs(&c["dimensions"]).len(), n, "{cmd}: {c}");
        assert!(!strs(&c["curves"]).is_empty(), "{cmd}: {c}");
    }
}

#[test]
fn breaking_an_offset_association_creates_nothing() {
    let mut s = new_sketch();
    let e = strs(&made(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 5, "connect": false}))["curves"]);
    let c = made(&mut s, "sketch.offset", json!({"curves": [e[0]], "distance": -1, "chain": false}));
    let out = strs(&c["curves"]);
    let r = run(&mut s, "sketch.offset.break", json!({"curve": out[0]}));
    let c = &r["created"];
    for k in ["points", "curves", "constraints", "dimensions"] {
        assert_eq!(c[k], json!([]), "{r}");
    }
}

#[test]
fn text_edits_and_imports_return_the_ids_they_create() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.text", json!({"text": "I", "at": [0, 0], "height": 10}));
    check(&mut s, "sketch.text", &r, &Sketch::new());
    let before = sketch(&s);
    let link = r["link"].as_str().map(str::to_string).unwrap_or_else(|| panic!("{r}"));
    let r = run(&mut s, "sketch.edit_text", json!({"link": link, "text": "II"}));
    check(&mut s, "sketch.edit_text", &r, &before);

    let svg = r#"<svg width="40mm" height="40mm" viewBox="0 0 40 40"><path d="M0 0 H40 V40 H0 Z"/></svg>"#;
    let mut s = new_sketch();
    let c = made(&mut s, "sketch.insert_svg", json!({"text": svg}));
    assert_eq!(strs(&c["curves"]).len(), 4, "{c}");
    let dxf = run(&mut s, "sketch.export_dxf", json!({}))["dxf"].as_str().unwrap_or_default().to_string();
    let mut s = new_sketch();
    let c = made(&mut s, "sketch.insert_dxf", json!({"text": dxf}));
    assert_eq!(strs(&c["curves"]).len(), 4, "{c}");
}

#[test]
fn projections_and_3d_curves_return_the_ids_they_create() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 10}));
    let body = s.model.state().bodies.first().map(|b| b.name.clone()).unwrap_or_default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5}));
    made(&mut s, "sketch.project", json!({"refs": [{"edge": [10, 0, 10]}]}));
    made(&mut s, "sketch.intersect", json!({"refs": [{"body": body}]}));
    made(&mut s, "sketch.include_3d", json!({"refs": [{"edge": [20, 10, 10]}]}));
    made(&mut s, "sketch.fit_curves_to_section", json!({"body": body}));
    made(&mut s, "sketch.isoparametric_curve", json!({"face": [10, 10, 10]}));

    let mut s = new_sketch();
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let c = made(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 10], [10, 0, 10]]}));
    assert_eq!(strs(&c["wires"]).len(), 2, "{c}");
    made(&mut s, "sketch.spline3d", json!({"points": [[0, 0, 0], [5, 5, 5], [10, 0, 10]]}));
    made(&mut s, "sketch.point3d", json!({"point": [5, 5, 5]}));
}

#[test]
fn a_new_sketch_reports_its_projected_face_edges() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 10}));
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}}));
    let c = check(&mut s, "sketch.create", &r, &Sketch::new());
    assert_eq!(strs(&c["curves"]).len(), 4, "{c}");
}

#[test]
fn ids_are_never_reused_after_a_delete() {
    let mut s = new_sketch();
    let first = made(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    let circle = made(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 3}));
    let dim = run(&mut s, "sketch.dimension", json!({"entities": [strs(&circle["curves"])[0]], "value": 8}));
    let mut gone: Vec<String> = ["points", "curves"].iter().flat_map(|k| strs(&first[k]).into_iter().chain(strs(&circle[k]))).collect();
    gone.extend(strs(&dim["created"]["dimensions"]));
    // Deleting the curves takes their own end and centre points (and the dimension) with them.
    run(&mut s, "sketch.delete", json!({"entities": strs(&first["curves"]).into_iter().chain(strs(&circle["curves"])).collect::<Vec<_>>()}));
    let sk = sketch(&s);
    assert!(sk.curves.is_empty() && sk.points.len() == 1 && sk.constraints.is_empty(), "{:?}", sk.curves);
    // The same commands again: every id is new.
    let again = made(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    let circle2 = made(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 3}));
    let dim2 = run(&mut s, "sketch.dimension", json!({"entities": [strs(&circle2["curves"])[0]], "value": 8}));
    let mut fresh: Vec<String> = ["points", "curves"].iter().flat_map(|k| strs(&again[k]).into_iter().chain(strs(&circle2[k]))).collect();
    fresh.extend(strs(&dim2["created"]["dimensions"]));
    assert_eq!(fresh.len(), gone.len(), "{fresh:?} {gone:?}");
    for id in &fresh {
        assert!(!gone.contains(id), "id {id} reused: {gone:?} -> {fresh:?}");
    }
    // Undo of a delete brings the old ids back unchanged (they still name the same geometry).
    let line = strs(&again["curves"])[0].clone();
    run(&mut s, "sketch.delete", json!({"entities": [line]}));
    run(&mut s, "edit.undo", json!({}));
    assert!(sketch(&s).curve_index(&line).is_some());
}

/// The example in docs/control-protocol.md.
#[test]
fn documented_example_holds() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
    assert_eq!(r["curves"], json!(["l1"]));
    assert_eq!(r["created"], json!({"points": ["l1.start", "l1.end"], "curves": ["l1"], "constraints": ["k1"], "dimensions": []}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 50}));
}

#[test]
fn commands_that_add_nothing_report_empty_lists() {
    let mut s = new_sketch();
    let c = made(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 5]]}));
    let r = run(&mut s, "sketch.move_point", json!({"point": strs(&c["points"])[1], "to": [12, 6]}));
    assert_eq!(r["created"], json!({"points": [], "curves": [], "constraints": [], "dimensions": []}), "{r}");
}

/// Conic curves take the curve prefix `n`, not the constraints' `k` (#6173): however a conic
/// is created — drawn, a Move/Copy copy, a pattern copy — its id still says it is a curve.
#[test]
fn conic_curves_take_the_n_prefix_not_the_constraints_k() {
    let mut s = new_sketch();
    let drawn = run(&mut s, "sketch.conic", json!({"start": [80, 490], "end": [100, 490], "apex": [90, 505], "rho": 0.5}));
    let conic = strs(&drawn["curves"])[0].clone();
    assert_eq!(conic, "n1", "a drawn conic starts its own counter");

    // A constraint keeps its prefix, and it no longer shares a counter with conics.
    run(&mut s, "sketch.constraint.fix", json!({"entity": format!("{conic}.start")}));
    let sk = sketch(&s);
    assert!(sk.constraints.iter().any(|k| k.id == "k1"), "constraint ids: {:?}", sk.constraints.iter().map(|k| &k.id).collect::<Vec<_>>());

    let copy = run(&mut s, "sketch.move", json!({"entities": [conic], "translate": [40, 0], "copy": true}));
    assert_eq!(strs(&copy["curves"]), vec!["n2".to_string()], "a Move/Copy copy continues the conic counter");

    let pattern = run(&mut s, "sketch.pattern.rectangular", json!({"entities": [conic], "count": 2, "spacing": 40}));
    let made: Vec<String> = strs(&pattern["curves"]);
    assert!(!made.is_empty(), "{pattern}");
    for id in made {
        assert!(id.starts_with('n'), "a pattern copy keeps the conic prefix: {id}");
    }
}
