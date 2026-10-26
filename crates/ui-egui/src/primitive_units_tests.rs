//! #630: the Box, Cylinder, Sphere, Torus and Coil dialogs show the unit of their sizes, as the
//! other dialogs do ("10 mm"), in the design's units. OK makes what the bare numbers made before,
//! and a bare number typed into a field still means the design's units.

use serde_json::{Value, json};
use solidcraft_engine::Session;

use crate::dialogs::{Kind, apply_commands};
use crate::scenario::Harness;

/// The primitives with their sizes as a bare number reads them: the command and fields.
const PRIMITIVES: [(&str, &[(&str, &str)]); 5] = [
    ("solid.box", &[("length", "20"), ("width", "20"), ("height", "20")]),
    ("solid.cylinder", &[("diameter", "20"), ("height", "20")]),
    ("solid.sphere", &[("diameter", "20")]),
    ("solid.torus", &[("major", "20"), ("minor", "5")]),
    ("solid.coil", &[("diameter", "40"), ("revolutions", "4"), ("pitch", "8"), ("section_size", "3")]),
];

/// A design in `units`, with nothing in it.
fn design(units: &str) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    if units != "mm" {
        h.app.run("document.units", json!({ "units": units })).unwrap();
    }
    h.frames(2);
    h
}

/// The open dialog's fields.
fn fields(h: &Harness) -> Vec<(&'static str, String)> {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { fields, .. }) => fields.clone(),
        other => panic!("no primitive dialog: {other:?}"),
    }
}

/// Every field shows its text in its row, as a person sees it.
fn assert_shown(h: &Harness, fields: &[(&str, String)]) {
    for (label, text) in fields {
        let nodes = &h.output.accesskit_update.as_ref().unwrap().nodes;
        assert!(
            nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::TextInput && n.label() == Some(*label) && n.value() == Some(text)),
            "the {label} editor shows {text:?}"
        );
    }
}

/// Replace a field's text by typing, as a person does.
fn type_into(h: &mut Harness, label: &str, text: &str) {
    h.step(&json!({"click": {"handle": format!("dialog:{label}")}})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": text})).unwrap();
}

fn ok(h: &mut Harness) {
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none(), "OK closes the dialog: {:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
}

/// The volume of everything in the design (mm³).
fn volume(s: &mut Session) -> f64 {
    s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap()
}

/// The size of the design's only body (mm).
fn size(s: &mut Session) -> [f64; 3] {
    let r = s.execute("inspect.measure", &json!({})).unwrap();
    let b = &r["bodies"][0]["bbox"];
    std::array::from_fn(|i| b["max"][i].as_f64().unwrap() - b["min"][i].as_f64().unwrap())
}

fn assert_size(s: &mut Session, want: [f64; 3]) {
    let got = size(s);
    assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6), "{got:?}, want {want:?}");
}

/// The volume the command makes from the bare numbers in a design in `units` (what the dialog
/// made before its fields showed units).
fn bare_volume(units: &str, cmd: &str, sizes: &[(&str, &str)]) -> f64 {
    let mut s = Session::default();
    s.execute("document.units", &json!({ "units": units })).unwrap();
    let params: serde_json::Map<String, Value> = sizes.iter().map(|(k, v)| ((*k).to_string(), json!(v))).collect();
    s.execute(cmd, &Value::Object(params)).unwrap();
    volume(&mut s)
}

#[test]
fn primitive_dialogs_show_millimetres_and_make_the_same_sizes() {
    for (cmd, sizes) in PRIMITIVES {
        let mut h = design("mm");
        h.app.start(cmd);
        h.frames(3);
        let shown = fields(&h);
        let want: Vec<(&str, String)> =
            sizes.iter().map(|(k, v)| (*k, if *k == "revolutions" { (*v).to_string() } else { format!("{v} mm") })).collect();
        assert_eq!(shown, want, "{cmd}");
        assert_shown(&h, &shown);
        // OK sends the fields' texts, units and all.
        let (id, params) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        assert_eq!(id, cmd);
        for (k, v) in &want {
            assert_eq!(params[*k], json!(v), "{cmd} {k}");
        }
        ok(&mut h);
        let made = volume(&mut h.app.session);
        let before = bare_volume("mm", cmd, sizes);
        assert!(made > 0.0 && (made - before).abs() < 1e-6 * before, "{cmd}: {made} mm³, the bare numbers made {before} mm³");
    }
    let mut h = design("mm");
    h.app.start("solid.box");
    h.frames(3);
    ok(&mut h);
    assert_size(&mut h.app.session, [20.0, 20.0, 20.0]);
}

#[test]
fn in_an_inch_design_primitive_dialogs_show_inches_and_read_bare_numbers_as_inches() {
    for (cmd, sizes) in PRIMITIVES {
        let mut h = design("in");
        h.app.start(cmd);
        h.frames(3);
        let shown = fields(&h);
        let want: Vec<(&str, String)> =
            sizes.iter().map(|(k, v)| (*k, if *k == "revolutions" { (*v).to_string() } else { format!("{v} in") })).collect();
        assert_eq!(shown, want, "{cmd}");
        assert_shown(&h, &shown);
        ok(&mut h);
        // The same size the bare numbers made in an inch design.
        let made = volume(&mut h.app.session);
        let before = bare_volume("in", cmd, sizes);
        assert!(made > 0.0 && (made - before).abs() < 1e-6 * before, "{cmd}: {made} mm³, the bare numbers made {before} mm³");
    }
    // A bare number typed into a field is in the design's units; a typed unit is honoured.
    let mut h = design("in");
    h.app.start("solid.box");
    h.frames(3);
    type_into(&mut h, "width", "2");
    type_into(&mut h, "height", "10 mm");
    h.frames(2);
    let (_, params) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    assert_eq!((&params["length"], &params["width"], &params["height"]), (&json!("20 in"), &json!("2 in"), &json!("10 mm")));
    ok(&mut h);
    assert_size(&mut h.app.session, [508.0, 50.8, 10.0]);
}

#[test]
fn a_centimetre_design_shows_centimetres() {
    let mut h = design("cm");
    h.app.start("solid.sphere");
    h.frames(3);
    assert_eq!(fields(&h), vec![("diameter", "20 cm".to_string())]);
    ok(&mut h);
    assert_size(&mut h.app.session, [200.0, 200.0, 200.0]);
}

/// The sizes a primitive feature keeps, as stored.
fn stored(h: &Harness, id: u64) -> Vec<String> {
    use solidcraft_engine::doc::FeatureKind;
    match h.app.session.doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Box { length, width, height, .. }) => vec![length.to_string(), width.to_string(), height.to_string()],
        Some(FeatureKind::Cylinder { radius, height, .. }) => vec![radius.to_string(), height.to_string()],
        Some(FeatureKind::Sphere { radius, .. }) => vec![radius.to_string()],
        Some(FeatureKind::Torus { major, minor, .. }) => vec![major.to_string(), minor.to_string()],
        other => panic!("not a primitive: {other:?}"),
    }
}

#[test]
fn editing_a_primitive_after_the_design_goes_to_inches_keeps_its_millimetres() {
    // Made by a script in a millimetre design, then the design goes to inches: bare sizes are
    // still millimetres, and Edit shows them so; OK without a change keeps the part. Sizes with
    // a unit, expressions and parameters show as they are kept.
    let mut h = design("mm");
    h.app.run("parameters.add", json!({"name": "w", "expression": "6 mm"})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": "w", "height": 20})).unwrap();
    h.app.run("solid.cylinder", json!({"diameter": "10 mm", "height": 12, "base": [60, 0, 0], "operation": "new"})).unwrap();
    h.app.run("solid.sphere", json!({"radius": 4, "center": [0, 60, 0], "operation": "new"})).unwrap();
    h.app.run("solid.torus", json!({"major": 10, "minor": "2 mm", "center": [60, 60, 0], "operation": "new"})).unwrap();
    h.app.run("document.units", json!({"units": "in"})).unwrap();
    h.frames(2);
    let before = volume(&mut h.app.session);
    let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
    let want: [&[(&str, &str)]; 4] = [
        &[("length", "40 mm"), ("width", "w"), ("height", "20 mm")],
        &[("radius", "(10 mm) / 2"), ("height", "12 mm")],
        &[("radius", "4 mm")],
        &[("major", "10 mm"), ("minor", "2 mm")],
    ];
    assert_eq!(ids.len(), want.len());
    for (id, want) in ids.into_iter().zip(want) {
        h.app.edit_feature(id);
        h.frames(3);
        let shown = fields(&h);
        assert_eq!(shown, want.iter().map(|(k, v)| (*k, (*v).to_string())).collect::<Vec<_>>());
        assert_shown(&h, &shown);
        ok(&mut h);
        assert_eq!(stored(&h, id), want.iter().map(|(_, v)| (*v).to_string()).collect::<Vec<_>>());
        let after = volume(&mut h.app.session);
        assert!((after - before).abs() < 1e-6 * before, "the part is unchanged: {after} mm³, was {before} mm³");
    }
    // A size typed bare in the edit is in the design's units, as anywhere else.
    let first = h.app.session.doc.features.first().map(|f| f.id).unwrap();
    h.app.edit_feature(first);
    h.frames(3);
    type_into(&mut h, "height", "1");
    h.frames(2);
    ok(&mut h);
    let r = h.app.session.execute("inspect.measure", &json!({})).unwrap();
    let height = r["bodies"].as_array().unwrap().iter().map(|b| b["bbox"]["max"][2].as_f64().unwrap()).fold(f64::MIN, f64::max);
    assert!((height - 25.4).abs() < 1e-6, "1 in tall: {height}");
}

#[test]
fn editing_in_a_millimetre_design_keeps_the_sizes_as_they_are_kept() {
    // A bare number already means millimetres here: Edit shows and sends it as kept, so OK
    // without a change leaves the feature (and the active configuration's match) alone.
    let mut h = design("mm");
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    let (id, length) = h.app.session.doc.features.first().map(|f| (f.id, f.param_names[0].clone())).unwrap();
    h.app.run("config.configure", json!({ "params": [length] })).unwrap();
    h.app.run("config.activate", json!({"row": "Default"})).unwrap();
    assert!(h.app.session.doc.configuration_matches());
    h.app.edit_feature(id);
    h.frames(3);
    let shown = fields(&h);
    assert_eq!(shown, vec![("length", "40".to_string()), ("width", "30".to_string()), ("height", "20".to_string())]);
    ok(&mut h);
    assert_eq!(stored(&h, id), ["40", "30", "20"]);
    assert!(h.app.session.doc.configuration_matches(), "an edit without a change keeps the configuration matched");
    assert_size(&mut h.app.session, [40.0, 30.0, 20.0]);
}

#[test]
fn editing_a_coil_reopens_its_dialog_filled_in_and_ok_redefines_it() {
    // #3862: Edit Feature on a coil opens the Coil dialog with every setting it was made with,
    // not the command line; OK changes what was edited and keeps the rest.
    use solidcraft_engine::doc::{CoilSection, FeatureKind};
    let mut h = design("mm");
    h.app
        .run(
            "solid.coil",
            json!({"diameter": 40, "revolutions": 4, "pitch": 8, "section_size": 3, "section_position": "outside",
                   "clockwise": true, "start_angle": "30 deg", "base": [5, 0, 0], "operation": "new"}),
        )
        .unwrap();
    let id = h.app.session.doc.features.first().map(|f| f.id).unwrap();
    h.app.edit_feature(id);
    h.frames(3);
    let shown = fields(&h);
    let want = [("diameter", "40"), ("revolutions", "4"), ("pitch", "8"), ("section_size", "3")];
    assert_eq!(shown, want.iter().map(|(k, v)| (*k, (*v).to_string())).collect::<Vec<_>>());
    assert_shown(&h, &shown);
    let d = h.app.dialog.as_mut().unwrap();
    assert!(d.editing.is_some_and(|(edited, _)| edited == id));
    assert_eq!((&d.extra["section_position"], &d.extra["clockwise"], &d.extra["start_angle"]), (&json!("outside"), &json!(true), &json!("30 deg")));
    assert!(!d.extra.contains_key("section"), "a circular section is the default");
    d.extra.insert("section".into(), json!("square"));
    type_into(&mut h, "pitch", "10");
    h.frames(2);
    ok(&mut h);
    assert_eq!(h.app.session.doc.features.len(), 1, "OK redefines the coil instead of adding one");
    match h.app.session.doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Coil { base, pitch, turns, section, position, clockwise, start_angle, .. }) => {
            assert_eq!((pitch.to_string(), turns.to_string()), ("10".to_string(), "4".to_string()));
            assert_eq!((*section, *position, *clockwise), (CoilSection::Square, 1, true));
            assert_eq!(start_angle.as_ref().map(|a| a.to_string()).as_deref(), Some("30 deg"));
            assert_eq!(base.iter().map(|e| e.to_string()).collect::<Vec<_>>(), ["5", "0", "0"]);
        }
        other => panic!("not a coil: {other:?}"),
    }
}

/// #3271: cover secondary and optional lengths as well as each tool's primary canvas value.
#[test]
fn operation_length_defaults_follow_every_design_unit() {
    const CASES: &[(&str, &[&str])] = &[
        ("solid.extrude", &["10", "10", "", "", "2", "1"]),
        ("solid.fillet", &["2", "2"]),
        ("solid.chamfer", &["1", "1"]),
        ("solid.shell", &["2"]),
        ("solid.hole", &["5", "10", "9", "3", "10"]),
        ("solid.rib", &["2", ""]),
        ("solid.web", &["2", ""]),
        ("solid.emboss", &["1"]),
        ("solid.offset_face", &["2"]),
        ("solid.pattern.rectangular", &["20", "20"]),
        ("solid.pattern.path", &["20"]),
        ("solid.pipe", &["5", ""]),
        ("solid.bounding_solid", &["2"]),
        ("solid.move", &["0", "0", "0"]),
        ("construct.plane.offset", &["0"]),
        ("inspect.section", &["0"]),
        ("surface.extrude", &["10"]),
        ("surface.offset", &["2"]),
        ("surface.ruled", &["10"]),
        ("surface.thicken", &["2"]),
        ("surface.extend", &["10"]),
        ("surface.stitch", &[""]),
        ("sheet.flange", &["10", "", "20", ""]),
        ("sheet.hem", &["5", ""]),
        ("sheet.fold", &[""]),
        ("plastic.boss", &["6", "10", "3", "", "", "1", "3", "1"]),
        ("plastic.lip", &["1.5", "2", ""]),
        ("plastic.snap_fit", &["10", "1.5", "5", "1", "2"]),
        ("plastic.rest", &["10", "15", "2", ""]),
        ("joint.create", &["0", "0", "0"]),
        ("joint.gear", &["10", "20"]),
        ("joint.rack_pinion", &["10", ""]),
        ("joint.screw", &["1.5", ""]),
        ("joint.cam", &["5"]),
        ("sketch.offset", &["2", "2"]),
        ("sketch.move", &["0", "0"]),
        ("sketch.pattern.rectangular", &["20", "20"]),
        ("sketch.fillet", &["2", "2"]),
        ("sketch.chamfer.two_distance", &["2", "2"]),
    ];
    for units in ["mm", "cm", "m", "in", "ft"] {
        let h = design(units);
        for &(command, numbers) in CASES {
            let mut d = crate::dialogs::Dialog::for_command(&h.app, command).unwrap_or_else(|| panic!("no dialog for {command}"));
            let values: Vec<_> = crate::length_input::fields(&h.app.session.doc, &mut d.kind).into_iter().map(|v| v.clone()).collect();
            let expected: Vec<_> = numbers.iter().map(|n| if n.is_empty() { String::new() } else { format!("{n} {units}") }).collect();
            assert_eq!(values, expected, "{command} in {units}");
        }
    }
}

#[test]
fn length_input_expressions_and_computed_values_round_trip_without_display_rounding() {
    use solidcraft_engine::doc::expr::Kind as K;
    for (units, scale) in [("mm", 1.0), ("cm", 10.0), ("m", 1000.0), ("in", 25.4), ("ft", 304.8)] {
        let mut h = design(units);
        h.app.run("parameters.add", json!({"name":"span","expression":"12 mm"})).unwrap();
        for (text, expected) in [
            ("0.25", 0.25 * scale),
            ("1 / 4", 0.25 * scale),
            ("0,25", 0.25 * scale),
            ("0,25 + max(1,2)", 2.25 * scale),
            ("2 mm", 2.0),
            ("span / 2", 6.0),
            ("span + 2 mm", 14.0),
        ] {
            let value = crate::length_input::eval(&h.app.session.doc, text, K::Length).unwrap();
            assert!((value - expected).abs() < 1e-9, "{units}: {text} -> {value}");
        }
        for mm in [0.0, -12.3456789123, 0.0000123456789, 25.4, 123456.789123] {
            let text = crate::length_input::model(&h.app.session.doc, mm);
            assert!(text.ends_with(units), "{text}");
            let value = crate::length_input::eval(&h.app.session.doc, &text, K::Length).unwrap();
            assert!((value - mm).abs() < 1e-10 * mm.abs().max(1.0), "{text} -> {value}, wanted {mm}");
        }
        for text in ["", "NaN", "1e400", "2 deg", "missing_parameter"] {
            assert!(crate::length_input::eval(&h.app.session.doc, text, K::Length).is_err(), "{text}");
        }
        assert_eq!(crate::length_input::expression(&h.app.session.doc, "45", K::Angle), "45");
        assert_eq!(crate::length_input::expression(&h.app.session.doc, "3", K::Unitless), "3");
        assert_eq!(crate::length_input::expression(&h.app.session.doc, "span / 2", K::Length), "span / 2");
    }
}

#[test]
fn inch_extrude_secondary_lengths_and_unchanged_edit_keep_their_physical_sizes() {
    let mut h = design("mm");
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("solid.extrude", json!({"distance":10,"distance2":5})).unwrap();
    let feature = h.app.session.doc.features.last().unwrap().id;
    let before = volume(&mut h.app.session);
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.app.edit_feature(feature);
    h.frames(3);
    let Kind::Extrude { distance, distance2, .. } = &h.app.dialog.as_ref().unwrap().kind else { panic!("no extrude") };
    assert_eq!((distance.as_str(), distance2.as_str()), ("10 mm", "5 mm"));
    ok(&mut h);
    assert!((volume(&mut h.app.session) - before).abs() < 1e-6);
    h.app.edit_feature(feature);
    if let Kind::Extrude { distance, distance2, start, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *distance = "0.25".into();
        *distance2 = "2 mm".into();
        *start = "0.125".into();
    }
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let p = &commands[0].1["params"];
    assert_eq!((&p["distance"], &p["distance2"], &p["start_offset"]), (&json!("0.25 in"), &json!("2 mm"), &json!("0.125 in")));
    h.frames(3);
    ok(&mut h);
    let measurements = h.app.session.execute("inspect.measure", &json!({})).unwrap();
    let bbox = &measurements["bodies"][0]["bbox"];
    assert!((bbox["max"][2].as_f64().unwrap() - 9.525).abs() < 1e-6);
    let reopened = Session::new(solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap());
    assert_eq!(reopened.doc.length_inputs(), h.app.session.doc.length_inputs());
}

/// Somebody driving the app never sees a command's reply, so the status line says when an
/// input's plain numbers were read as mm (#5887, raised in review of #6017).
#[test]
fn the_status_line_says_when_an_expression_is_read_as_mm() {
    let mut h = design("in");
    h.app.run("parameters.add", json!({"name": "w", "expression": "2 in"})).unwrap();
    // A bare number takes the design's unit, so there is nothing to say.
    h.app.run("solid.box", json!({"length": 3, "width": 10, "height": 10, "corner": [0, 0, 0]})).unwrap();
    assert!(h.app.status.as_ref().is_none_or(|s| !s.0.contains("read as mm")), "a bare number takes the design unit: {:?}", h.app.status);
    // An expression keeps the mm default for its plain numbers, and the status line says so.
    h.app.run("parameters.change", json!({"name": "d1", "expression": "w + 1"})).unwrap();
    let said = h.app.status.clone().map(|s| s.0).unwrap_or_default();
    assert!(said.contains("read as mm") && said.contains("51.8") && said.contains("76.2"), "{said}");
    assert!(h.app.status.as_ref().is_some_and(|s| !s.2), "it is a warning, not an error: {:?}", h.app.status);
}
