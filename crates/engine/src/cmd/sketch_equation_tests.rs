//! Equation curves (#8352): a fit point spline from formulas over the design's parameters.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::CurveKind;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn no_trouble(s: &Session) {
    for r in &s.model.results {
        assert!(r.error.is_none() && r.warning.is_none(), "{}: {:?} {:?}", r.name, r.error, r.warning);
    }
}

fn near(got: f64, want: f64, rel: f64) {
    assert!((got - want).abs() <= want.abs() * rel, "got {got}, want {want}");
}

/// The evaluated sketch `S`.
fn solved(s: &Session) -> solidcraft_sketch::Sketch {
    let f = s.doc.find_feature("S").unwrap().id;
    s.model.state().sketch(f).unwrap().sketch.clone()
}

/// The positions of curve `id`'s points in the evaluated sketch `S`.
fn curve_points(s: &Session, id: &str) -> Vec<Vec2> {
    let sk = solved(s);
    let c = sk.curves.iter().find(|c| c.id == id).unwrap();
    c.kind.point_ids().iter().map(|i| sk.points[*i].pos).collect()
}

/// A wave over a strip: y = h + 5 sin(2 pi x / 100) over x from 0 to 100, closed by three
/// lines down to the x axis. Its area is 100 h (the sine adds as much as it takes away).
fn wave(s: &mut Session) -> String {
    run(s, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(s, "sketch.equation_curve", json!({"explicit": true, "y": "h + 5 * sin(2 * pi * x / 100)", "start": 0, "end": "100 mm"}));
    let id = r["curve"].as_str().unwrap().to_string();
    assert_eq!(r["closed"], false, "{r}");
    assert!(r["deviation"].as_f64().unwrap() <= 0.01, "{r}");
    run(s, "sketch.line", json!({"points": [format!("{id}.end"), [100, 0], [0, 0], format!("{id}.start")]}));
    run(s, "sketch.finish", json!({}));
    id
}

#[test]
fn a_wave_from_a_formula_extrudes_and_follows_its_parameter() {
    let mut s = Session::default();
    let id = wave(&mut s);
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10}));
    no_trouble(&s);
    near(volume(&mut s), 100.0 * 10.0 * 10.0, 1e-4);
    let before = curve_points(&s, &id);
    assert!(before.iter().all(|p| (p.y - (10.0 + 5.0 * (2.0 * std::f64::consts::PI * p.x / 100.0).sin())).abs() < 1e-9), "{before:?}");
    // The curve follows the parameter: the strip is twice as tall.
    run(&mut s, "parameters.change", json!({"name": "h", "expression": "20 mm"}));
    no_trouble(&s);
    near(volume(&mut s), 100.0 * 20.0 * 10.0, 1e-4);
    let after = curve_points(&s, &id);
    assert_eq!(before.len(), after.len());
    assert!(before.iter().zip(&after).all(|(a, b)| (b.y - a.y - 10.0).abs() < 1e-9 && (a.x - b.x).abs() < 1e-9));
    // The parameter knows the curve uses it, and a rename reaches the formula.
    let users = run(&mut s, "parameters.users", json!({"name": "h"}));
    assert!(users.to_string().contains("Curve Y"), "{users}");
    run(&mut s, "parameters.rename", json!({"name": "h", "new_name": "height"}));
    let f = s.doc.find_feature("S").unwrap().id;
    assert_eq!(s.doc.sketch(f).unwrap().equation_of(&id).unwrap().y, "height + 5 * sin(2 * pi * x / 100)");
    no_trouble(&s);
    near(volume(&mut s), 100.0 * 20.0 * 10.0, 1e-4);
    // The design file keeps the formulas, and a reopened design follows them.
    assert!(s.doc.to_json().contains("\"equations\""));
    let mut r = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    no_trouble(&r);
    near(volume(&mut r), 100.0 * 20.0 * 10.0, 1e-4);
}

#[test]
fn the_variable_hides_a_parameter_of_its_name_and_a_rename_leaves_it_alone() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "1000 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "t", "y": "t * t", "start": -2, "end": 2}));
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    let pts = curve_points(&s, &id);
    assert!(pts.iter().all(|p| (p.y - p.x * p.x).abs() < 1e-9 && p.x.abs() <= 2.0 + 1e-9), "{pts:?}");
    run(&mut s, "parameters.rename", json!({"name": "t", "new_name": "u"}));
    let f = s.doc.find_feature("S").unwrap().id;
    let eq = s.doc.sketch(f).unwrap().equation_of(&id).unwrap().clone();
    assert_eq!((eq.x.as_deref(), eq.y.as_str()), (Some("t"), "t * t"));
}

#[test]
fn a_full_turn_closes_smoothly_and_extrudes_as_a_disc() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "20 * cos(t)", "y": "20 * sin(t)", "start": 0, "end": "360 deg"}));
    assert_eq!(r["closed"], true, "{r}");
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let f = s.doc.find_feature("S").unwrap().id;
    let sk = s.doc.sketch(f).unwrap();
    let Some(CurveKind::Spline { pts, periodic: true, control: false, .. }) = sk.curves.iter().find(|c| c.id == id).map(|c| &c.kind) else {
        panic!("not a closed fit spline: {:?}", sk.curves)
    };
    assert_eq!(pts.first(), pts.last());
    assert!(pts.iter().all(|p| sk.points[*p].fixed));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 5}));
    no_trouble(&s);
    near(volume(&mut s), std::f64::consts::PI * 400.0 * 5.0, 1e-3);
}

#[test]
fn a_profile_under_a_curve_revolves() {
    // y = 5 + 2 sin(x / 5) over x from 0 to 30, revolved about the x axis:
    // V = pi * integral of (5 + 2 sin(x/5))^2 = pi * (25 * 30 + 20 * 5 (1 - cos 6) + 4 (15 - 5/4 sin 12)).
    let want = std::f64::consts::PI * (750.0 + 100.0 * (1.0 - 6f64.cos()) + 4.0 * (15.0 - 1.25 * 12f64.sin()));
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"explicit": true, "y": "5 + 2 * sin(x / 5)", "start": 0, "end": 30}));
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.line", json!({"points": [format!("{id}.end"), [30, 0], [0, 0], format!("{id}.start")]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": "S", "axis": "x"}));
    no_trouble(&s);
    near(volume(&mut s), want, 2e-3);
}

#[test]
fn an_edit_changes_the_formulas_and_keeps_the_joined_ends() {
    let mut s = Session::default();
    let id = wave(&mut s);
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    let f = s.doc.find_feature("S").unwrap().id;
    let before = s.doc.sketch(f).unwrap().clone();
    let ends = |sk: &solidcraft_sketch::Sketch| {
        let c = sk.curves.iter().find(|c| c.id == id).unwrap();
        let (a, b) = c.kind.ends().unwrap();
        (sk.points[a].id.clone(), sk.points[b].id.clone())
    };
    // A tighter tolerance and a bigger wave: more points, the same ends.
    let r = run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "y": "h + 8 * sin(2 * pi * x / 100)", "tolerance": 0.002}));
    assert!(r["deviation"].as_f64().unwrap() <= 0.002, "{r}");
    let after = s.doc.sketch(f).unwrap().clone();
    assert_eq!(ends(&before), ends(&after));
    let count = |sk: &solidcraft_sketch::Sketch| sk.curves.iter().find(|c| c.id == id).unwrap().kind.point_ids().len();
    assert!(count(&after) > count(&before), "{} vs {}", count(&after), count(&before));
    assert_eq!(r["points"].as_u64(), Some(count(&after) as u64));
    let eq = after.equation_of(&id).unwrap();
    assert_eq!((eq.y.as_str(), eq.start.as_str(), eq.tolerance), ("h + 8 * sin(2 * pi * x / 100)", "0", 0.002));
    // No points left behind: the old in-between points went.
    assert_eq!(after.points.len(), before.points.len() - count(&before) + count(&after));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10}));
    no_trouble(&s);
    near(volume(&mut s), 100.0 * 10.0 * 10.0, 1e-4);
    // Undo puts the old formulas back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.sketch(f).unwrap().equation_of(&id).unwrap().y, "h + 5 * sin(2 * pi * x / 100)");
}

#[test]
fn an_edit_can_switch_between_the_two_forms() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"explicit": true, "y": "x / 2", "start": 0, "end": 10}));
    let id = r["curve"].as_str().unwrap().to_string();
    // To a closed parametric curve: the old end point goes, the start stays.
    let r = run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "x": "10 * cos(t)", "y": "10 * sin(t)", "start": 0, "end": "2 * pi"}));
    assert_eq!(r["closed"], true, "{r}");
    let f = s.doc.find_feature("S").unwrap().id;
    let sk = s.doc.sketch(f).unwrap();
    assert_eq!(sk.points.len(), 1 + r["points"].as_u64().unwrap() as usize, "the origin and the curve's own points");
    // Back to the explicit form, open again.
    let r = run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "explicit": true, "y": "x * x / 10", "start": -10, "end": 10}));
    assert_eq!(r["closed"], false, "{r}");
    let sk = s.doc.sketch(f).unwrap();
    assert!(sk.equation_of(&id).unwrap().x.is_none());
    let (a, b) = sk.curves.iter().find(|c| c.id == id).unwrap().kind.ends().unwrap();
    assert_eq!((sk.points[a].pos, sk.points[b].pos), (Vec2::new(-10.0, 10.0), Vec2::new(10.0, 10.0)));
}

#[test]
fn deleting_the_curve_forgets_its_formulas() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "t", "y": "sin(t)", "start": 0, "end": 6}));
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.delete", json!({"entities": [id]}));
    let f = s.doc.find_feature("S").unwrap().id;
    let sk = s.doc.sketch(f).unwrap();
    assert!(sk.equations.is_empty() && sk.curves.is_empty() && sk.points.len() == 1, "{sk:?}");
}

#[test]
fn bad_formulas_ranges_and_values_are_refused_and_change_nothing() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let before = s.doc.to_json();
    for p in [
        json!({}),
        json!({"y": "t", "start": 0, "end": 1}),
        json!({"x": "t", "y": "t", "start": 0}),
        json!({"x": "t", "explicit": true, "y": "x", "start": 0, "end": 1}),
        json!({"x": "t", "y": "sqrt(t)", "start": -1, "end": 1}),
        json!({"x": "t", "y": "1 / t", "start": -1, "end": 1}),
        json!({"x": "t", "y": "t", "start": 1, "end": 1}),
        json!({"x": "t", "y": "t", "start": 0, "end": "1e12"}),
        json!({"x": "t * 1 deg", "y": "t", "start": 0, "end": 1}),
        json!({"x": "nope", "y": "t", "start": 0, "end": 1}),
        json!({"x": "t", "y": "t", "start": 0, "end": 1, "tolerance": 0}),
        json!({"x": "t", "y": "t", "start": 0, "end": 1, "tolerance": "big"}),
        json!({"x": 5, "y": 5, "start": 0, "end": 1}),
        json!({"x": ["t"], "y": "t", "start": 0, "end": 1}),
        json!({"x": "t", "y": "sin(1e5 * t)", "start": 0, "end": 1000, "tolerance": 0.0001}),
        json!({"x": "1e300 * t", "y": "t", "start": 0, "end": 1}),
    ] {
        assert!(s.execute("sketch.equation_curve", &p).is_err(), "{p} should be refused");
        assert_eq!(s.doc.to_json(), before, "{p} changed the design");
    }
    for p in [json!({"curve": "nope", "y": "t"}), json!({"y": "t"}), json!({"curve": 7})] {
        assert!(s.execute("sketch.equation_curve.edit", &p).is_err(), "{p} should be refused");
    }
    let line = run(&mut s, "sketch.line", json!({"points": [[0, 0], [5, 5]]}))["curves"][0].as_str().unwrap().to_string();
    assert!(s.execute("sketch.equation_curve.edit", &json!({"curve": line, "y": "t"})).is_err());
}

#[test]
fn a_curve_its_parameters_break_is_an_error_not_a_crash() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "k", "expression": "1 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.equation_curve", json!({"x": "t", "y": "sqrt(k / 1 mm - t)", "start": 0, "end": 0.5}));
    run(&mut s, "sketch.finish", json!({}));
    no_trouble(&s);
    // Now the square root goes negative inside the range: the sketch reports it.
    run(&mut s, "parameters.change", json!({"name": "k", "expression": "0.25 mm"}));
    let r = s.model.results.iter().find(|r| r.name == "S").unwrap();
    assert!(r.error.as_deref().is_some_and(|e| e.contains("equation curve")), "{:?}", r.error);
    run(&mut s, "parameters.change", json!({"name": "k", "expression": "1 mm"}));
    no_trouble(&s);
}

/// The issue's curve (#9333): `a = 10 mm`, y = a^2 / (4 t) over t from a to 3a. Each point is on
/// y = a^2 / (4 x), from (a, a / 4) to (3a, a / 12).
fn assert_parabola(s: &Session, id: &str, a: f64) {
    let pts = curve_points(s, id);
    let (first, last) = (pts[0], *pts.last().unwrap());
    assert!((first - Vec2::new(a, a / 4.0)).len() < 1e-9, "{first:?}");
    assert!((last - Vec2::new(3.0 * a, a / 12.0)).len() < 1e-9, "{last:?}");
    assert!(pts.iter().all(|p| (p.y - a * a / (4.0 * p.x)).abs() < 1e-9), "{pts:?}");
}

fn range_unit_curve(s: &mut Session) -> String {
    run(s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(s, "sketch.equation_curve", json!({"x": "t", "y": "a ^ 2 / (4 * t)", "start": "a", "end": "3 * a", "variable_unit": "range"}));
    assert_eq!(r["closed"], false, "{r}");
    let id = r["curve"].as_str().unwrap().to_string();
    run(s, "sketch.finish", json!({}));
    no_trouble(s);
    id
}

#[test]
fn a_variable_in_its_ranges_unit_follows_the_parameter_and_the_design_unit() {
    let mut s = Session::default();
    // Without the flag this very formula is refused, as it always was.
    run(&mut s, "parameters.add", json!({"name": "b", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Off"}));
    let why = s.execute("sketch.equation_curve", &json!({"x": "t", "y": "b ^ 2 / (4 * t)", "start": "b", "end": "3 * b"})).unwrap_err().to_string();
    assert!(why.contains("length^2"), "{why}");
    run(&mut s, "sketch.finish", json!({}));

    let id = range_unit_curve(&mut s);
    assert_parabola(&s, &id, 10.0);
    run(&mut s, "parameters.change", json!({"name": "a", "expression": "20 mm"}));
    no_trouble(&s);
    assert_parabola(&s, &id, 20.0);
    // In millimetres the parameter's length does not depend on the design's unit, nor the curve.
    run(&mut s, "document.units", json!({"units": "in"}));
    no_trouble(&s);
    assert_parabola(&s, &id, 20.0);
    // A parameter given in inches, in an inch design.
    run(&mut s, "parameters.change", json!({"name": "a", "expression": "2 in"}));
    no_trouble(&s);
    assert_parabola(&s, &id, 50.8);
}

/// A plain end is in the design's unit, so it moves when the unit changes (#9333).
#[test]
fn a_plain_end_of_the_range_takes_the_design_unit() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "a", "expression": "100 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "t", "y": "t / 2", "start": 1, "end": "a", "variable_unit": "range"}));
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    assert!((curve_points(&s, &id)[0] - Vec2::new(1.0, 0.5)).len() < 1e-9);
    run(&mut s, "document.units", json!({"units": "in"}));
    no_trouble(&s);
    let pts = curve_points(&s, &id);
    assert!((pts[0] - Vec2::new(25.4, 12.7)).len() < 1e-9, "{:?}", pts[0]);
    assert!((*pts.last().unwrap() - Vec2::new(100.0, 50.0)).len() < 1e-9);
}

#[test]
fn the_refused_range_says_why_and_changes_nothing() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let before = s.doc.to_json();
    for (start, end) in [("a", "90 deg"), ("90 deg", "a"), ("0", "1")] {
        let p = json!({"x": "t", "y": "t", "start": start, "end": end, "variable_unit": "range"});
        let why = s.execute("sketch.equation_curve", &p).unwrap_err().to_string();
        assert!(
            why.contains("with the variable in its range's unit, start and end must both be lengths, or one a length and the other a plain number"),
            "{p}: {why}"
        );
        assert_eq!(s.doc.to_json(), before, "{p} changed the design");
    }
    for bad in [json!("mm"), json!(1), json!(true), json!({}), json!("Range")] {
        assert!(s.execute("sketch.equation_curve", &json!({"x": "t", "y": "t", "start": 0, "end": "a", "variable_unit": bad})).is_err(), "{bad}");
    }
}

/// A design file is hostile (#9333): a range the rule refuses is reported on the sketch, and a
/// value that means nothing refuses the file. Neither crashes.
#[test]
fn a_hostile_saved_flag_or_range_is_an_error_not_a_crash() {
    let mut s = Session::default();
    range_unit_curve(&mut s);
    let saved = s.doc.to_json();
    assert!(saved.contains("3 * a") && saved.contains("\"range\""), "{saved}");
    let angle = saved.replace("3 * a", "90 deg");
    let r = Session::new(solidcraft_doc::Document::from_json(&angle).unwrap());
    let res = r.model.results.iter().find(|r| r.name == "S").unwrap();
    assert!(res.error.as_deref().is_some_and(|e| e.contains("equation curve") && e.contains("must both be lengths")), "{:?}", res.error);
    assert!(solidcraft_doc::Document::from_json(&saved.replace("\"range\"", "\"furlong\"")).is_err());
}

#[test]
fn an_edit_keeps_the_flag_until_told_and_a_saved_design_keeps_it() {
    use solidcraft_sketch::VariableUnit;
    let mut s = Session::default();
    let id = range_unit_curve(&mut s);
    let f = s.doc.find_feature("S").unwrap().id;
    // The file says it, and reopening follows it.
    let saved = s.doc.to_json();
    assert!(saved.contains("variable_unit"), "not saved");
    let mut r = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    no_trouble(&r);
    assert_eq!(r.doc.sketch(f).unwrap().equation_of(&id).unwrap().variable_unit, VariableUnit::Range);
    assert_parabola(&r, &id, 10.0);
    run(&mut r, "parameters.change", json!({"name": "a", "expression": "20 mm"}));
    assert_parabola(&r, &id, 20.0);
    // An edit that leaves it out keeps it.
    run(&mut s, "sketch.edit", json!({"sketch": "S"}));
    run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "tolerance": 0.005}));
    assert_eq!(s.doc.sketch(f).unwrap().equation_of(&id).unwrap().variable_unit, VariableUnit::Range);
    // Off again, that formula is refused and the curve stays as it was.
    assert!(s.execute("sketch.equation_curve.edit", &json!({"curve": id, "variable_unit": "number"})).is_err());
    assert_eq!(s.doc.sketch(f).unwrap().equation_of(&id).unwrap().variable_unit, VariableUnit::Range);
    run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "y": "t / 2", "start": 0, "end": 10, "variable_unit": "number"}));
    assert_eq!(s.doc.sketch(f).unwrap().equation_of(&id).unwrap().variable_unit, VariableUnit::Number);
    assert!(!s.doc.to_json().contains("variable_unit"));
}

/// A design written without the flag (#9333): nothing in the file changes, and its curve is what
/// it was, with the variable a plain number over a range in length.
#[test]
fn a_design_saved_without_the_flag_reopens_the_same() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "t", "y": "t * t / 20", "start": "0 mm", "end": "40 mm"}));
    let id = r["curve"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let saved = s.doc.to_json();
    assert!(!saved.contains("variable_unit"), "a curve without the flag writes no new key");
    let want = curve_points(&s, &id);
    assert!((*want.last().unwrap() - Vec2::new(40.0, 80.0)).len() < 1e-9);
    // The format before this change, as the last release wrote it.
    let mut old: Value = serde_json::from_str(&saved).unwrap();
    old["format"] = json!(solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT - 1));
    if let Some(o) = old.as_object_mut() {
        o.remove("format_pending");
    }
    let r = Session::new(solidcraft_doc::Document::from_json(&old.to_string()).unwrap());
    no_trouble(&r);
    assert_eq!(curve_points(&r, &id), want);
}
