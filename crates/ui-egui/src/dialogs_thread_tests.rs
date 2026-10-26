use serde_json::json;
use solidcraft_engine::geom::Vec3;

use super::*;
use crate::dialogs::{Kind, apply_commands, edge_sel};
use crate::preselect_tests::thread_face;
use crate::scenario::{self, Harness};
use crate::theme::Tokens;

/// A thread dialog on an 8 mm shaft 10 mm long, the face picked, in a window tall enough for
/// every row without scrolling the fields.
fn shaft() -> Harness {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1400}));
    h.app.home.open = false;
    h.app.run("solid.cylinder", json!({"radius":4,"height":10})).unwrap();
    h.app.session.selection = vec![thread_face(&h.app, "Body1")];
    h.step(&json!({"start":"solid.thread"})).unwrap();
    h.frames(2);
    h
}

fn click(h: &mut Harness, handles: &[&str]) {
    for handle in handles {
        h.step(&json!({"click":{"handle":handle}})).unwrap_or_else(|e| panic!("{handle}: {e}"));
    }
    h.frames(2);
}

fn choose(h: &mut Harness, row: &str, choice: &str) {
    click(h, &[&format!("dialog:{row}"), &format!("dialog:{row}:{choice}")]);
}

fn type_in(h: &mut Harness, row: &str, text: &str) {
    click(h, &[&format!("dialog:{row}")]);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":text})).unwrap();
    h.frames(2);
}

fn params(h: &Harness) -> serde_json::Value {
    apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0).1
}

fn options(h: &Harness) -> Options {
    match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Thread { options, .. } => options.clone(),
        k => panic!("{k:?}"),
    }
}

/// A dense row of the dialog, the same height as every other row.
fn shown(row: &str) -> bool {
    let rect = scenario::rect_of(&format!("dialog:row:{row}"));
    if let Some(rect) = rect {
        assert_eq!(rect.height(), Tokens::get().control_height, "{row}");
    }
    rect.is_some()
}

/// Every option of the command, set in the dialog, reaches it under the command's names.
#[test]
fn every_option_reaches_the_command() {
    let mut h = shaft();
    let untouched = params(&h);
    assert_eq!(untouched.as_object().unwrap().keys().collect::<Vec<_>>(), ["face"], "defaults stay out of the command: {untouched}");
    choose(&mut h, "Form", "Trapezoidal (ISO 2904)");
    choose(&mut h, "Size", "20 mm");
    assert_eq!(params(&h)["designation"], "Tr20x4", "a new size takes its standard pitch");
    type_in(&mut h, "Designation", "Tr20x2");
    choose(&mut h, "Pitch", "4 mm");
    choose(&mut h, "Hand", "Left");
    choose(&mut h, "Starts", "2");
    choose(&mut h, "Extent", "Length");
    type_in(&mut h, "Length", "6 mm");
    choose(&mut h, "Ease In", "Standard (2.5 P)");
    choose(&mut h, "Ease Out", "Length");
    type_in(&mut h, "Ease Out Length", "1.5 mm");
    choose(&mut h, "Tip", "Chamfer");
    type_in(&mut h, "Chamfer Angle", "30 deg");
    type_in(&mut h, "Chamfer Depth", "0.5 mm");
    click(&mut h, &["dialog:Modeled"]);
    let mut p = params(&h);
    p.as_object_mut().unwrap().remove("face");
    assert_eq!(
        p,
        json!({
            "designation": "Tr20x4", "form": "trapezoidal", "hand": "left", "starts": 2, "extent": "length", "length": "6 mm",
            "ease_in": "standard", "ease_out": "1.5 mm", "tip": "chamfer", "chamfer_angle": "30 deg", "chamfer_depth": "0.5 mm",
            "modeled": true,
        })
    );
    choose(&mut h, "Ease In", "Short (1.25 P)");
    choose(&mut h, "Hand", "Both");
    let p = params(&h);
    assert_eq!((&p["ease_in"], &p["hand"]), (&json!("short"), &json!("both")));
}

/// Buttress rows only for the buttress forms, the length only with a Length extent, the Up To
/// pick only with an Up To extent, the chamfer only with a chamfered tip, sizes only for the
/// forms with a size list.
#[test]
fn only_the_fields_that_apply_are_shown() {
    let mut h = shaft();
    for row in ["Form", "Size", "Pitch", "Designation", "Hand", "Starts", "Extent", "Ease In", "Ease Out", "Tip"] {
        assert!(shown(row), "{row}");
    }
    for row in ["Steep Flank", "Length", "Ease In Length", "Ease Out Length", "Chamfer Angle", "Chamfer Depth"] {
        assert!(!shown(row), "{row}");
    }
    let inputs = |h: &Harness| h.app.dialog.as_ref().unwrap().inputs.iter().map(|i| i.label).collect::<Vec<_>>();
    assert_eq!(inputs(&h), ["Cylindrical face"], "a full thread with plain ends starts at no end in particular");

    choose(&mut h, "Form", "Buttress (DIN 513)");
    assert!(shown("Steep Flank") && shown("Size"));
    choose(&mut h, "Steep Flank", "Start");
    assert_eq!(params(&h)["steep_flank"], "start");
    choose(&mut h, "Form", "Buttress (ASME B1.9)");
    assert!(shown("Steep Flank") && !shown("Size") && !shown("Pitch"), "no size list for the ASME buttress");
    assert_eq!(params(&h)["steep_flank"], "start");
    choose(&mut h, "Form", "Acme (ASME B1.5)");
    assert!(!shown("Steep Flank") && shown("Size") && !shown("Pitch"), "an Acme size names its threads per inch");
    assert!(params(&h).get("steep_flank").is_none(), "the steep flank is a buttress option");

    choose(&mut h, "Extent", "Up to");
    assert_eq!(inputs(&h), ["Cylindrical face", START_END, UP_TO]);
    assert!(!shown("Length"));
    choose(&mut h, "Extent", "Length");
    assert_eq!(inputs(&h), ["Cylindrical face", START_END]);
    assert!(shown("Length"));
    choose(&mut h, "Extent", "Full");
    assert_eq!(inputs(&h), ["Cylindrical face"]);
    choose(&mut h, "Ease In", "Length");
    assert_eq!(inputs(&h), ["Cylindrical face", START_END], "a runout starts at an end");
    assert!(shown("Ease In Length") && !shown("Ease Out Length"));
    choose(&mut h, "Tip", "Chamfer");
    assert!(shown("Chamfer Angle") && shown("Chamfer Depth"));
    let p = params(&h);
    assert!(p.get("chamfer_angle").is_none() && p.get("chamfer_depth").is_none(), "empty values take ISO 4753's: {p}");
}

/// The picks: the end the thread starts from, and a plane or a face to run up to.
#[test]
fn the_start_end_and_up_to_picks_reach_the_command() {
    let mut h = shaft();
    choose(&mut h, "Extent", "Length");
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!((d.inputs[1].label, d.active), (START_END, 0), "the optional pick does not take the next click");
    let top = edge_sel(&h.app.session, Vec3::new(4.0, 0.0, 10.0)).expect("the top edge");
    let Sel::Edge { point, .. } = top else { panic!("{top:?}") };
    h.app.dialog.as_mut().unwrap().inputs[1].items = vec![top];
    assert_eq!(params(&h)["from"], json!([point.x, point.y, point.z]));

    choose(&mut h, "Extent", "Up to");
    let p = params(&h);
    assert_eq!(p["extent"], "up_to");
    assert!(p.get("up_to").is_none() && p.get("length").is_none());
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(d.inputs[d.active].label, UP_TO, "the new pick is the active one");
    d.inputs[2].items = vec![Sel::Plane { name: "XY".into() }];
    assert_eq!(params(&h)["up_to"], "XY");
    let face = crate::dialogs::face_sel(&h.app.session, Vec3::new(0.0, 0.0, 0.0)).expect("the bottom face");
    let Sel::Face { point, .. } = face.clone() else { panic!("{face:?}") };
    h.app.dialog.as_mut().unwrap().inputs[2].items = vec![face];
    assert_eq!(params(&h)["up_to"], json!([point.x, point.y, point.z]));
    choose(&mut h, "Extent", "Full");
    let p = params(&h);
    assert!(p.get("extent").is_none() && p.get("up_to").is_none() && p.get("from").is_none(), "{p}");
}

/// A form change keeps a manual size's diameter where the new form lists it, and the other
/// options; a form without that size, or a size the face chose, goes back to the face's size.
#[test]
fn a_form_change_keeps_what_still_applies() {
    let mut h = shaft();
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Thread { designation, .. } if designation == "M8"));
    choose(&mut h, "Hand", "Left");
    choose(&mut h, "Starts", "3");
    choose(&mut h, "Extent", "Length");
    type_in(&mut h, "Length", "4 mm");
    choose(&mut h, "Ease Out", "Short (1.25 P)");
    let kept = options(&h);

    choose(&mut h, "Form", "Trapezoidal (ISO 2904)");
    assert!(params(&h).get("designation").is_none(), "the face's metric size is not carried to another form");
    choose(&mut h, "Size", "20 mm");
    choose(&mut h, "Form", "Buttress (DIN 513)");
    assert_eq!(params(&h)["designation"], "S20x4", "20 mm is a buttress size too");
    choose(&mut h, "Form", "Trapezoidal (ISO 2904)");
    choose(&mut h, "Size", "8 mm");
    choose(&mut h, "Form", "Buttress (DIN 513)");
    assert!(params(&h).get("designation").is_none(), "8 mm is not a buttress size");
    choose(&mut h, "Form", "ISO metric");
    h.frames(2);
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Thread { designation, .. } if designation == "M8"), "the face's size again");
    let now = options(&h);
    assert_eq!(Options { form: kept.form, ..now.clone() }, kept);
    let p = params(&h);
    assert_eq!((&p["hand"], &p["starts"], &p["length"], &p["ease_out"]), (&json!("left"), &json!(3), &json!("4 mm"), &json!("short")));
}

/// The dialog shows the command's own error text for options it cannot build.
#[test]
fn the_command_error_is_what_the_dialog_shows() {
    let mut h = shaft();
    type_in(&mut h, "Designation", "Q7");
    let (command, params) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    let saved = h.app.session.doc.to_json();
    let expected = h.app.session.execute(&command, &params).expect_err("Q7 is no thread").to_string();
    assert_eq!(h.app.session.doc.to_json(), saved);
    h.step(&json!({"widget":"OK"})).unwrap();
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("the dialog stays open");
    assert!(d.error.as_deref().is_some_and(|e| expected.contains(e) || e.contains(&expected)), "{:?} vs {expected}", d.error);
}

#[test]
fn the_forms_are_the_documents_and_square_is_not_offered() {
    assert!(FORMS.iter().all(|(_, f)| f.refused().is_none()), "a form the command refuses is not offered");
    assert_eq!(FORMS.len(), ThreadForm::ALL.len() - 1);
    assert_eq!(parse(ThreadForm::Trapezoidal, "Tr40x14(P7)"), Some((40.0, 7.0)), "a two-start lead 14 names pitch 7");
    assert_eq!(parse(ThreadForm::Trapezoidal, "M10x1"), None);
    assert_eq!(pitches(ThreadForm::Metric, 8.0, 1.25), [1.25, 1.0, 0.75, 0.5, 0.35, 0.25, 0.2]);
    assert_eq!(example(ThreadForm::ButtressAsme), "2.5-8 BUTT");
    assert_eq!(example(ThreadForm::Trapezoidal), "Tr20x4");
}

/// The face's size follows the form.
#[test]
fn the_face_size_follows_the_form() {
    let mut h = shaft();
    let shown_size = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Thread { designation, .. } => designation.clone(),
        k => panic!("{k:?}"),
    };
    assert_eq!(shown_size(&h), "M8");
    choose(&mut h, "Form", "Trapezoidal (ISO 2904)");
    h.frames(2);
    assert_eq!(shown_size(&h), "Tr8x1.5");
    assert!(params(&h).get("designation").is_none(), "the face's size stays automatic");
}

/// Editing a saved thread reopens every option it was made with, and OK gives them back.
#[test]
fn a_saved_thread_reopens_with_every_option() {
    for (made, picks) in [
        (
            json!({"designation":"S20x4","steep_flank":"start","hand":"left","starts":2,"length":"12 mm","from":[10,0,20],"modeled":true,
                "ease_in":"standard","ease_out":"1 mm","tip":"chamfer","chamfer_angle":"30 deg","chamfer_depth":"0.5 mm"}),
            vec!["Cylindrical face", START_END],
        ),
        (json!({"designation":"M20","from":[10,0,20],"up_to":[10,0,5]}), vec!["Cylindrical face", START_END, UP_TO]),
        (json!({"designation":"M20","up_to":"XY","from":[10,0,20]}), vec!["Cylindrical face", START_END, UP_TO]),
    ] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        h.app.run("solid.cylinder", json!({"radius":10,"height":20})).unwrap();
        let Sel::Face { point, .. } = thread_face(&h.app, "Body1") else { panic!("a face") };
        let mut command = made.clone();
        command["face"] = json!([point.x, point.y, point.z]);
        let id = h.app.run("solid.thread", command).unwrap_or_else(|e| panic!("{made}: {e}"))["feature"].as_u64().unwrap();
        h.app.edit_feature(id);
        h.frames(2);
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs.iter().map(|i| i.label).collect::<Vec<_>>(), picks, "{made}");
        let p = params(&h)["params"].clone();
        for (key, value) in made.as_object().unwrap() {
            match key.as_str() {
                "from" | "up_to" if value.is_array() => {
                    let near = |a: &Value, b: &Value| (0..3).all(|i| (a[i].as_f64().unwrap() - b[i].as_f64().unwrap()).abs() < 0.5);
                    assert!(near(&p[key], value), "{key}: {} vs {value}", p[key]);
                }
                _ => assert_eq!(&p[key], value, "{key} in {p}"),
            }
        }
        let extent = if made.get("length").is_some() { "length" } else { "up_to" };
        assert_eq!(p["extent"], extent);
        if made["designation"] == "S20x4" {
            assert_eq!(p["form"], "buttress", "the form is saved resolved");
            assert!(shown("Steep Flank") && shown("Chamfer Angle") && shown("Ease Out Length"));
        }
        let (cmd, params) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        h.app.session.execute(&cmd, &params).unwrap_or_else(|e| panic!("{made}: OK on the reopened dialog: {e}"));
    }
}
