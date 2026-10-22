use crate::Session;
use serde_json::{Value, json};
use solidcraft_sketch::ConstraintKind;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command} {params}: {error}"))
}
fn session() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    s
}
fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}
fn draw(s: &mut Session, command: &str, params: Value, dimensions: Value) -> Value {
    run(s, "sketch.draw.dimensioned", json!({"command":command,"params":params,"dimensions":dimensions}))
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}
fn measure(s: &Session, kind: fn(&ConstraintKind) -> bool) -> f64 {
    let sk = sketch(s);
    let c = sk.constraints.iter().find(|c| kind(&c.kind)).unwrap();
    solidcraft_sketch::measure_dimension(&sk, &c.kind).unwrap()
}

#[test]
fn typed_line_angle_survives_quadrants_move_reopen_and_one_undo() {
    let mut s = session();
    run(&mut s, "parameters.add", json!({"name":"heading","expression":"-45 deg","unit":"deg"}));
    let undo = s.undo.len();
    let out = draw(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]],"infer":true}), json!({"length":"10 mm","angle":"heading"}));
    assert_eq!(s.undo.len(), undo + 1);
    assert!(!sketch(&s).constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Horizontal { .. } | ConstraintKind::Vertical { .. })));
    close(measure(&s, |k| matches!(k, ConstraintKind::LineAngle { .. })), -45f64.to_radians());
    for degrees in [135.0_f64, 270.0, -135.0, 0.0, 90.0] {
        let result = run(&mut s, "parameters.change", json!({"name":"heading","expression":format!("{degrees} deg")}));
        assert_eq!(result["errors"], json!([]));
        close(measure(&s, |k| matches!(k, ConstraintKind::LineAngle { .. })), degrees.to_radians());
        close(measure(&s, |k| matches!(k, ConstraintKind::Length { .. })), 10.0);
    }
    let encoded = serde_json::to_value(&*s.doc).unwrap();
    let doc: solidcraft_doc::Document = serde_json::from_value(encoded).unwrap();
    let mut reopened = Session { doc: std::sync::Arc::new(doc), active_sketch: s.active_sketch, ..Session::default() };
    reopened.refresh();
    close(measure(&reopened, |k| matches!(k, ConstraintKind::LineAngle { .. })), 90f64.to_radians());
    let before_move = reopened.doc.clone();
    for rotate_constraints in [false, true] {
        reopened.doc = before_move.clone();
        reopened.refresh();
        let result =
            run(&mut reopened, "sketch.move", json!({"entities":out["curves"],"translate":[4,3],"angle":20,"rotate_constraints":rotate_constraints}));
        assert_eq!(result["sketch"]["solved"], true);
        let expected = if rotate_constraints { 110.0_f64 } else { 90.0 };
        close(measure(&reopened, |k| matches!(k, ConstraintKind::LineAngle { .. })), expected.to_radians());
        close(measure(&reopened, |k| matches!(k, ConstraintKind::Length { .. })), 10.0);
        assert_eq!(reopened.doc.param("heading").unwrap().expr, "90 deg");
        let sk = sketch(&reopened);
        let c = sk.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::LineAngle { .. })).unwrap();
        assert_eq!(reopened.doc.param(c.param.as_deref().unwrap()).unwrap().expr, if rotate_constraints { "(heading) + 20 deg" } else { "heading" });
        run(&mut reopened, "edit.undo", json!({}));
        close(measure(&reopened, |k| matches!(k, ConstraintKind::LineAngle { .. })), 90f64.to_radians());
    }
    let mut atomic = session();
    draw(&mut atomic, "sketch.line", json!({"points":[[0,0],[5,5]]}), json!({"length":10,"angle":45}));
    run(&mut atomic, "edit.undo", json!({}));
    assert!(sketch(&atomic).curves.is_empty());
    assert!(atomic.doc.params.iter().all(|p| !p.model));
}

#[test]
fn signed_arc_sweep_retains_radius_and_logical_endpoint_identity() {
    let mut s = session();
    run(&mut s, "parameters.add", json!({"name":"turn","expression":"-90 deg","unit":"deg"}));
    let out = draw(&mut s, "sketch.arc.center_point", json!({"center":[0,0],"start":[10,0],"end":[0,-10]}), json!({"radius":10,"sweep":"turn"}));
    let id = out["curves"][0].as_str().unwrap().to_string();
    let original = sketch(&s).resolve_point(&format!("{id}.start")).unwrap();
    for degrees in [-90.0_f64, -270.0, 90.0, 270.0, -45.0] {
        let result = run(&mut s, "parameters.change", json!({"name":"turn","expression":format!("{degrees} deg")}));
        assert_eq!(result["errors"], json!([]));
        let sk = sketch(&s);
        assert_eq!(sk.resolve_point(&format!("{id}.start")), Some(original));
        assert_eq!(sk.curves[sk.curve_index(&id).unwrap()].reversed, degrees < 0.0);
        close(measure(&s, |k| matches!(k, ConstraintKind::ArcSweep { .. })), degrees.to_radians());
        close(measure(&s, |k| matches!(k, ConstraintKind::Radius { .. })), 10.0);
    }
}

#[test]
fn slots_and_polygon_keep_permanent_typed_dimensions() {
    for command in ["sketch.slot.center_to_center", "sketch.slot.overall", "sketch.slot.center_point"] {
        let mut s = session();
        let out = draw(
            &mut s,
            command,
            json!({"p0":[2,3],"p1":[22,3],"center":[12,3],"end":[22,3],"width":4,"connect":false}),
            json!({"length":20,"width":4}),
        );
        let length = out["dimensions"]["length"]["param"].as_str().unwrap();
        let result = run(&mut s, "parameters.change", json!({"name":length,"expression":30}));
        assert_eq!(result["errors"], json!([]));
        close(measure(&s, |k| matches!(k, ConstraintKind::Length { .. } | ConstraintKind::SlotOverallLength { .. })), 30.0);
        let width = out["dimensions"]["width"]["param"].as_str().unwrap();
        let result = run(&mut s, "parameters.change", json!({"name":width,"expression":6}));
        assert_eq!(result["errors"], json!([]));
        close(measure(&s, |k| matches!(k, ConstraintKind::Diameter { .. })), 6.0);
        close(measure(&s, |k| matches!(k, ConstraintKind::Length { .. } | ConstraintKind::SlotOverallLength { .. })), 30.0);
    }
    for command in ["sketch.polygon.inscribed", "sketch.polygon.circumscribed"] {
        let mut s = session();
        let out = draw(&mut s, command, json!({"center":[4,5],"radius":10,"sides":6,"connect":false}), json!({"radius":10}));
        let support = out["support_circle"].as_str().unwrap();
        assert!(sketch(&s).curves[sketch(&s).curve_index(support).unwrap()].construction);
        let radius = out["dimensions"]["radius"]["param"].as_str().unwrap();
        let result = run(&mut s, "parameters.change", json!({"name":radius,"expression":15}));
        assert_eq!(result["errors"], json!([]));
        close(measure(&s, |k| matches!(k, ConstraintKind::Radius { .. })), 15.0);
    }
}

#[test]
fn arc_slot_dimensions_survive_parameter_edits_and_reopening() {
    let mut s = session();
    let out = draw(
        &mut s,
        "sketch.slot.arc_center",
        json!({"center":[0,0],"start":[20,0],"end":[0,20],"width":6}),
        json!({"radius":20,"sweep":90,"width":6}),
    );
    for (role, value) in [("radius", 25), ("sweep", 120), ("width", 8)] {
        let name = out["dimensions"][role]["param"].as_str().unwrap();
        let result = run(&mut s, "parameters.change", json!({"name":name,"expression":value}));
        assert_eq!(result["errors"], json!([]), "{role}: {result}");
    }
    let doc = serde_json::from_value(serde_json::to_value(s.design()).unwrap()).unwrap();
    let mut reopened = Session::new(doc);
    reopened.active_sketch = s.active_sketch;
    close(measure(&reopened, |k| matches!(k, ConstraintKind::Radius { .. })), 25.0);
    close(measure(&reopened, |k| matches!(k, ConstraintKind::ArcSweep { .. })), 120f64.to_radians());
    close(measure(&reopened, |k| matches!(k, ConstraintKind::Diameter { .. })), 8.0);
}

#[test]
fn invalid_dimensioned_shapes_are_atomic_including_late_constraint_failure() {
    let mut s = session();
    let before = serde_json::to_value(&*s.doc).unwrap();
    let history = s.undo.len();
    for params in [
        json!({"command":"edit.undo","params":{},"dimensions":{}}),
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":"missing+1"}}),
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":-1}}),
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":"1".repeat(4097)}}),
        json!({"command":"sketch.arc.center_point","params":{"center":[0,0],"start":[10,0]},"dimensions":{"sweep":360}}),
        json!({"command":"sketch.polygon.inscribed","params":{"center":[0,0],"radius":10,"sides":3.5},"dimensions":{"radius":10}}),
        json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]]},"dimensions":{"length":20,"angle":0}}),
    ] {
        // The last case connects both endpoints to fixed existing points and fails only after creation.
        if params["dimensions"]["length"] == 20 {
            run(&mut s, "sketch.point", json!({"point":[10,0]}));
            run(&mut s, "sketch.constraint.fix", json!({"entity":"p1","fixed":true}));
        }
        let snapshot = serde_json::to_value(&*s.doc).unwrap();
        let undo = s.undo.len();
        // Preserve a live redo branch, which scratch-only dimension failure must not consume.
        run(&mut s, "sketch.point", json!({"point":[100,100]}));
        run(&mut s, "edit.undo", json!({}));
        s.selection = vec![crate::Sel::SketchPoint { id: "origin".into() }];
        let redo = s.redo.len();
        let selection = s.selection.clone();
        let pending = s.pending_moves.clone();
        let active = (s.active_sketch, s.active_component, s.active_occurrence_path.clone());
        assert!(s.execute("sketch.draw.dimensioned", &params).is_err(), "{params}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), snapshot);
        assert_eq!(s.undo.len(), undo);
        assert_eq!(s.redo.len(), redo);
        assert_eq!(s.selection, selection);
        assert_eq!(s.pending_moves, pending);
        assert_eq!((s.active_sketch, s.active_component, s.active_occurrence_path.clone()), active);
    }
    assert_eq!(before["features"].as_array().unwrap().len(), 1);
    assert!(s.undo.len() >= history);
}

#[test]
fn a_dimensioned_midpoint_line_carries_its_length_and_angle() {
    let mut s = session();
    let out = draw(&mut s, "sketch.line.midpoint", json!({"mid":[0,0],"end":[10,0]}), json!({"length":20,"angle":0}));
    assert_eq!(out["curves"].as_array().map(|c| c.len()), Some(1), "{out}");
    close(measure(&s, |k| matches!(k, ConstraintKind::Length { .. })), 20.0);
    close(measure(&s, |k| matches!(k, ConstraintKind::LineAngle { .. })), 0.0);
    let sk = sketch(&s);
    let line = sk.curve_index(out["curves"][0].as_str().unwrap()).unwrap();
    let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[line].kind else { panic!("a line") };
    // The clicked middle is the line's middle.
    close((sk.points[a].pos.x + sk.points[b].pos.x) * 0.5, 0.0);
    close((sk.points[a].pos.y + sk.points[b].pos.y) * 0.5, 0.0);
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { l, .. } if l == line)),
        "the middle is constrained: {:?}",
        sk.constraints
    );
}

#[test]
fn dimensioned_authoring_uses_selected_occurrence_and_keeps_local_geometry() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Shared"}))["component"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"component":component,"translate":[20,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":component,"occurrence":second,"path":[second]}));
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[20,0,0],"operation":"new"}));
    s.auto_project = false;
    run(&mut s, "sketch.create", json!({"plane":{"face":[21,1,2]}}));
    let out = draw(&mut s, "sketch.line", json!({"points":[[0,0],[5,5]]}), json!({"length":5,"angle":45}));
    let model = s.model.state();
    let local = model.sketch(s.active_sketch.unwrap()).unwrap();
    close(local.plane.origin.x, 0.0);
    close(local.plane.origin.z, 2.0);
    let world = s.world_state().unwrap();
    let displayed = world.sketch(s.active_sketch.unwrap()).unwrap();
    close(displayed.plane.origin.x, 20.0);
    close(displayed.plane.origin.z, 2.0);
    assert_eq!(s.active_occurrence_path, Some(vec![second]));
    let sk = sketch(&s);
    let line = sk.curve_index(out["curves"][0].as_str().unwrap()).unwrap();
    let solidcraft_sketch::CurveKind::Line { a, b } = sk.curves[line].kind else { panic!("line") };
    close(sk.points[a].pos.x, 0.0);
    close(sk.points[b].pos.x, 5.0 / std::f64::consts::SQRT_2);
}
