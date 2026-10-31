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

/// #6645: a clicked end with `clockwise` sweeps from the start the other way round, keeps the
/// start's radius and its `.start`/`.end` names, with and without a typed radius.
#[test]
fn a_clockwise_centre_arc_runs_from_its_start_to_its_end_the_other_way_round() {
    for dimensions in [None, Some(json!({"radius": 10}))] {
        let mut s = session();
        // The end is off the radius, as a cursor is: it is pulled onto the start's circle.
        let params = json!({"center":[0,0],"start":[10,0],"end":[0,4],"clockwise":true,"id":"a1","connect":false});
        match &dimensions {
            Some(d) => draw(&mut s, "sketch.arc.center_point", params, d.clone()),
            None => run(&mut s, "sketch.arc.center_point", params),
        };
        let sk = sketch(&s);
        let arc = &sk.curves[sk.curve_index("a1").unwrap()];
        assert!(arc.reversed, "{dimensions:?}");
        let at = |r: &str| sk.points[sk.resolve_point(r).unwrap()].pos;
        let (start, end) = (at("a1.start"), at("a1.end"));
        close(start.x, 10.0);
        close(start.y, 0.0);
        close(end.x, 0.0);
        close(end.y, 10.0);
        // Clockwise from 0° to 90° is 270° of arc through the bottom (-90°), not the top.
        let solidcraft_sketch::CurveKind::Arc { c, a, b } = arc.kind else { panic!("not an arc") };
        let (c, a, b) = (sk.points[c].pos, sk.points[a].pos, sk.points[b].pos);
        close(((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU).to_degrees(), 270.0);
        if dimensions.is_some() {
            close(measure(&s, |k| matches!(k, ConstraintKind::Radius { .. })), 10.0);
        }
    }
    // Without the flag the same clicks keep the counter-clockwise quarter.
    let mut s = session();
    run(&mut s, "sketch.arc.center_point", json!({"center":[0,0],"start":[10,0],"end":[0,4],"id":"a1"}));
    assert!(!sketch(&s).curves[sketch(&s).curve_index("a1").unwrap()].reversed);
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
    let world = s.world_state();
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

/// The expression of the parameter driving the first dimension `kind` matches.
fn driving(s: &Session, kind: fn(&ConstraintKind) -> bool) -> String {
    let sk = sketch(s);
    let c = sk.constraints.iter().find(|c| kind(&c.kind)).unwrap();
    s.doc.param(c.param.as_deref().unwrap()).unwrap().expr.clone()
}

/// A typed box value `name = expression` defines the user parameter `name`, which drives the
/// dimension, as the Dimension tool's value does (#7567).
#[test]
fn a_typed_definition_defines_the_parameter_that_drives_the_dimension() {
    let is_diameter = |k: &ConstraintKind| matches!(k, ConstraintKind::Diameter { .. });
    let mut s = session();
    let undo = s.undo.len();
    let out = draw(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":3}), json!({"diameter":"width = 20"}));
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(out["defined"], json!(["width"]), "{out}");
    close(measure(&s, is_diameter), 20.0);
    let width = s.doc.param("width").unwrap();
    assert_eq!((width.expr.as_str(), width.unit.as_str(), width.model), ("20", "mm", false));
    assert_eq!(driving(&s, is_diameter), "width");
    run(&mut s, "edit.undo", json!({}));
    assert!(sketch(&s).curves.is_empty() && s.doc.param("width").is_none());

    // A bare number is in the design's unit, in the size checks too.
    let mut s = session();
    run(&mut s, "document.units", json!({"units":"in"}));
    draw(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":3}), json!({"diameter":"#w = 2"}));
    close(measure(&s, is_diameter), 50.8);
    assert_eq!(s.doc.param("w").unwrap().unit, "in");
    let mut s = session();
    run(&mut s, "document.units", json!({"units":"in"}));
    let slot = json!({"p0":[0,0],"p1":[60,0],"width":10,"connect":false});
    draw(&mut s, "sketch.slot.overall", slot.clone(), json!({"length":"len = 2","width":"0.5"}));
    close(measure(&s, |k| matches!(k, ConstraintKind::SlotOverallLength { .. })), 50.8);

    // Both boxes of a rectangle may define a name.
    let mut s = session();
    let out = draw(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[5,5]}), json!({"width":"w = 30","height":"h=10"}));
    assert_eq!(out["defined"], json!(["h", "w"]), "{out}");
    let is_length = |k: &ConstraintKind| matches!(k, ConstraintKind::Length { .. });
    let sk = sketch(&s);
    let mut lengths: Vec<_> =
        sk.constraints.iter().filter(|c| is_length(&c.kind)).map(|c| solidcraft_sketch::measure_dimension(&sk, &c.kind).unwrap()).collect();
    lengths.sort_by(f64::total_cmp);
    assert_eq!(lengths.len(), 2);
    close(lengths[0], 10.0);
    close(lengths[1], 30.0);
}

#[test]
fn a_refused_typed_definition_changes_nothing() {
    let mut s = session();
    run(&mut s, "parameters.add", json!({"name":"taken","expression":"5","unit":"mm"}));
    let circle = |value: &str| json!({"command":"sketch.circle.center","params":{"center":[0,0],"radius":3},"dimensions":{"diameter":value}});
    let rect =
        |w: &str, h: &str| json!({"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[5,5]},"dimensions":{"width":w,"height":h}});
    for (params, says) in [
        (circle("taken = 20"), "already a parameter"),
        (circle("w ="), "needs a value"),
        (circle("2 = 3"), ""),
        (circle("w == 3"), ""),
        (circle("w = -4"), "invalid diameter"),
        (circle(&format!("w = {}", "1".repeat(4096))), "bounded"),
        // The same name twice: the second box finds it taken.
        (rect("w = 30", "w = 10"), "already a parameter"),
        // A box's value can't use a name the other box defines: neither exists yet.
        (rect("w = 30", "h = w / 2"), ""),
    ] {
        let snapshot = serde_json::to_value(&*s.doc).unwrap();
        let undo = s.undo.len();
        let error = s.execute("sketch.draw.dimensioned", &params).expect_err(&params.to_string()).to_string();
        assert!(error.contains(says), "{params}: {error}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), snapshot, "{params}");
        assert_eq!(s.undo.len(), undo);
    }
}

/// A drawing for each creator `dimension_roles` accepts. A creator added there needs one here,
/// or `as_drawn_dimensions_every_size_of_every_creator_where_it_was_drawn` fails.
fn sample(creator: &str) -> Option<Value> {
    Some(match creator {
        "sketch.line" => json!({"points":[[0,0],[6,8]]}),
        "sketch.line.midpoint" => json!({"mid":[3,4],"end":[9,12]}),
        "sketch.rectangle.two_point" => json!({"p0":[0,0],"p1":[30,20]}),
        "sketch.rectangle.center" => json!({"center":[0,0],"corner":[15,10]}),
        "sketch.rectangle.three_point" => json!({"p0":[0,0],"p1":[30,10],"p2":[25,25]}),
        "sketch.rectangle.rounded" => json!({"p0":[0,0],"p1":[30,20],"radius":3}),
        "sketch.circle.center" => json!({"center":[0,0],"radius":5}),
        "sketch.circle.two_point" => json!({"p0":[0,0],"p1":[10,0]}),
        "sketch.circle.three_point" => json!({"p0":[0,0],"p1":[10,0],"p2":[5,5]}),
        "sketch.arc.center_point" => json!({"center":[0,0],"start":[10,0],"end":[0,10]}),
        "sketch.slot.center_to_center" | "sketch.slot.overall" => json!({"p0":[2,3],"p1":[22,3],"width":4}),
        "sketch.slot.center_point" => json!({"center":[12,3],"end":[22,3],"width":4}),
        "sketch.slot.arc_center" => json!({"center":[0,0],"start":[20,0],"end":[0,20],"width":6}),
        "sketch.slot.arc_three_point" => json!({"start":[20,0],"through":[14.142136,14.142136],"end":[0,20],"width":6}),
        "sketch.polygon.inscribed" | "sketch.polygon.circumscribed" => json!({"center":[4,5],"radius":10,"sides":6}),
        "sketch.polygon.edge" => json!({"p0":[0,0],"p1":[10,0],"sides":6}),
        _ => return None,
    })
}

fn points(s: &Session) -> Vec<(f64, f64)> {
    sketch(s).points.iter().map(|p| (p.pos.x, p.pos.y)).collect()
}

/// With `as_drawn`, every creator gets every size it has (one of radius and diameter) as driving
/// dimensions, in one undo step, and the shape stays exactly where it was drawn.
#[test]
fn as_drawn_dimensions_every_size_of_every_creator_where_it_was_drawn() {
    let creators: Vec<&str> = crate::command_specs().iter().map(|c| c.id).filter(|id| super::dimension_roles(id).is_some()).collect();
    assert!(creators.len() >= 17, "{creators:?}");
    for creator in creators {
        let params = sample(creator).unwrap_or_else(|| panic!("{creator}: add a sample drawing for it"));
        let mut plain = session();
        run(&mut plain, creator, params.clone());
        let mut s = session();
        let undo = s.undo.len();
        let out = run(&mut s, "sketch.draw.dimensioned", json!({"command":creator,"params":params,"as_drawn":true}));
        assert_eq!(s.undo.len(), undo + 1, "{creator}: one undo step");
        let placed = out["dimensions"].as_object().unwrap();
        for role in super::dimension_roles(creator).unwrap() {
            let alternative = super::ALTERNATIVES.iter().find(|pair| pair.contains(role)).and_then(|pair| pair.iter().find(|r| r != &role));
            assert!(
                placed.contains_key(*role) || alternative.is_some_and(|other| placed.contains_key(*other)),
                "{creator}: no {role} dimension in {placed:?}"
            );
            if let Some(other) = alternative {
                assert!(!(placed.contains_key(*role) && placed.contains_key(*other)), "{creator}: both {role} and {other}");
            }
        }
        for (role, result) in placed {
            assert_ne!(result["driven"], json!(true), "{creator}: {role} is driven");
        }
        for ((x, y), (px, py)) in points(&s).into_iter().zip(points(&plain)) {
            assert!((x - px).abs() < 1e-5 && (y - py).abs() < 1e-5, "{creator}: moved from ({px}, {py}) to ({x}, {y})");
        }
        run(&mut s, "edit.undo", json!({}));
        assert!(sketch(&s).curves.is_empty(), "{creator}: undo left geometry");
        assert!(s.doc.params.iter().all(|p| !p.model), "{creator}: undo left a dimension parameter");
    }
}

/// Typed values win over the size as drawn; a typed radius stands in for the diameter; a size the
/// sketch already determines (a line inferred horizontal has no angle to give) is left out.
#[test]
fn as_drawn_keeps_typed_values_alternatives_and_leaves_out_determined_sizes() {
    let mut s = session();
    let out = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[30,20]},"dimensions":{"width":40},"as_drawn":true}),
    );
    let mut placed: Vec<&String> = out["dimensions"].as_object().unwrap().keys().collect();
    placed.sort();
    assert_eq!(placed, ["height", "width"]);
    let lengths: Vec<f64> = sketch(&s)
        .constraints
        .iter()
        .filter(|c| matches!(c.kind, ConstraintKind::Length { .. }))
        .map(|c| solidcraft_sketch::measure_dimension(&sketch(&s), &c.kind).unwrap())
        .collect();
    assert_eq!(lengths.len(), 2);
    assert!(lengths.iter().any(|l| (l - 40.0).abs() < 1e-6) && lengths.iter().any(|l| (l - 20.0).abs() < 1e-6), "{lengths:?}");

    let mut s = session();
    let out = run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command":"sketch.circle.center","params":{"center":[0,0],"radius":5},"dimensions":{"radius":"4 mm"},"as_drawn":true}),
    );
    assert_eq!(out["dimensions"].as_object().unwrap().keys().collect::<Vec<_>>(), ["radius"]);
    close(measure(&s, |k| matches!(k, ConstraintKind::Radius { .. })), 4.0);

    let mut s = session();
    let out =
        run(&mut s, "sketch.draw.dimensioned", json!({"command":"sketch.line","params":{"points":[[0,0],[10,0]],"infer":true},"as_drawn":true}));
    assert_eq!(out["dimensions"].as_object().unwrap().keys().collect::<Vec<_>>(), ["length"]);
    assert!(sketch(&s).constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Horizontal { .. })), "the inferred horizontal stays");
    close(measure(&s, |k| matches!(k, ConstraintKind::Length { .. })), 10.0);

    // Without as_drawn the call is unchanged: only what is typed.
    let mut s = session();
    let out = draw(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[30,20]}), json!({"width":40}));
    assert_eq!(out["dimensions"].as_object().unwrap().keys().collect::<Vec<_>>(), ["width"]);
}

/// Every `[x, y]` point of a sample moved by `(dx, dy)`; sizes and counts stay.
fn shifted(v: &Value, dx: f64, dy: f64) -> Value {
    match v {
        Value::Array(xy) if xy.len() == 2 && xy.iter().all(Value::is_number) => {
            json!([xy[0].as_f64().unwrap() + dx, xy[1].as_f64().unwrap() + dy])
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| shifted(item, dx, dy)).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(k, item)| (k.clone(), shifted(item, dx, dy))).collect()),
        other => other.clone(),
    }
}

fn dof(s: &mut Session) -> u64 {
    run(s, "sketch.solve", json!({}))["dof"].as_u64().unwrap()
}

/// With `as_drawn` and `locate`, every creator ends fully constrained wherever it is drawn: at its
/// sample place (several touch the origin, which anchors them) and moved away from the origin.
/// Nothing moves, and it is one undo step.
#[test]
fn locate_fully_constrains_every_creator_where_it_was_drawn() {
    let creators: Vec<&str> = crate::command_specs().iter().map(|c| c.id).filter(|id| super::dimension_roles(id).is_some()).collect();
    for creator in creators {
        let sample = sample(creator).unwrap_or_else(|| panic!("{creator}: add a sample drawing for it"));
        for params in [sample.clone(), shifted(&sample, 13.0, 7.0)] {
            let mut plain = session();
            run(&mut plain, creator, params.clone());
            let mut s = session();
            let undo = s.undo.len();
            let out = run(&mut s, "sketch.draw.dimensioned", json!({"command":creator,"params":params,"as_drawn":true,"locate":true}));
            assert_eq!(dof(&mut s), 0, "{creator} {params}: not fully constrained; located {}", out["located"]);
            assert_eq!(s.undo.len(), undo + 1, "{creator}: one undo step");
            for ((x, y), (px, py)) in points(&s).into_iter().zip(points(&plain)) {
                assert!((x - px).abs() < 1e-5 && (y - py).abs() < 1e-5, "{creator} {params}: moved from ({px}, {py}) to ({x}, {y})");
            }
            for placed in out["located"].as_array().unwrap() {
                assert_ne!(placed["driven"], json!(true), "{creator}: a driven position {placed}");
            }
            run(&mut s, "edit.undo", json!({}));
            assert!(sketch(&s).curves.is_empty() && sketch(&s).constraints.is_empty(), "{creator}: undo left something");
        }
    }
}

/// A shape anchored to the origin gets no position; a point on an axis goes onto the axis rather
/// than taking a 0 distance; one off both axes gets a horizontal and a vertical distance.
#[test]
fn locate_adds_only_what_the_shape_lacks() {
    let located = |center: [f64; 2]| -> (Value, Session) {
        let mut s = session();
        let out = run(
            &mut s,
            "sketch.draw.dimensioned",
            json!({"command":"sketch.circle.center","params":{"center":center,"radius":5},"as_drawn":true,"locate":true}),
        );
        (out["located"].clone(), s)
    };
    let (anchored, mut s) = located([0.0, 0.0]);
    assert_eq!(anchored, json!([]), "a circle on the origin is already placed");
    assert_eq!(dof(&mut s), 0);

    let (on_axis, mut s) = located([20.0, 0.0]);
    assert_eq!(on_axis.as_array().unwrap().len(), 2, "{on_axis}");
    assert_eq!(dof(&mut s), 0);
    let zero_distance = sketch(&s).constraints.iter().any(|c| match &c.kind {
        ConstraintKind::DistanceX { value, .. } | ConstraintKind::DistanceY { value, .. } => value.abs() < 1e-9,
        _ => false,
    });
    assert!(!zero_distance, "a 0 distance was placed instead of the axis");

    let (free, mut s) = located([20.0, 12.0]);
    assert_eq!(free.as_array().unwrap().len(), 2, "{free}");
    assert_eq!(dof(&mut s), 0);
    close(measure(&s, |k| matches!(k, ConstraintKind::DistanceX { .. })), 20.0);
    close(measure(&s, |k| matches!(k, ConstraintKind::DistanceY { .. })), 12.0);
}
