use serde_json::json;
use solidcore_record::Vec3;

use super::*;
use crate::dialogs::{Kind, apply_commands, edge_sel};
use crate::preselect_tests::thread_face;
use crate::scenario::{self, Harness};
use solidcraft_theme::Tokens;

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

#[test]
fn a_new_thread_leaves_the_flank_layout_out_and_an_edit_keeps_its_saved_layout() {
    for continuous in [false, true] {
        let mut h = shaft();
        h.app.session.selection = vec![thread_face(&h.app, "Body1")];
        h.app.dialog = crate::dialogs::Dialog::for_command(&h.app, "solid.thread");
        // Continuous flanks are the command's default, so a new thread's dialog never sends the field.
        assert_eq!(params(&h).get("continuous_flanks"), None);
        // A saved thread (an old design's has the historical layout) keeps its layout when edited.
        h.app.run("solid.thread", json!({"face": [4,0,5], "designation": "M8", "continuous_flanks": continuous})).unwrap();
        let saved = h.app.session.doc.features.last().unwrap().clone();
        let mut d = crate::dialogs::Dialog::for_command(&h.app, "solid.thread").unwrap();
        crate::dialogs_thread::reopen(&mut d, &h.app.session, &saved);
        assert_eq!(d.extra["continuous_flanks"], json!(continuous));
    }
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
    choose(&mut h, "End", "ui.fade");
    type_in(&mut h, "Fade Length", "1.5 mm");
    choose(&mut h, "Start", "Chamfer");
    type_in(&mut h, "Chamfer Angle", "30 deg");
    type_in(&mut h, "Chamfer Depth", "0.5 mm");
    click(&mut h, &["dialog:Modeled"]);
    let mut p = params(&h);
    p.as_object_mut().unwrap().remove("face");
    assert_eq!(
        p,
        json!({
            "designation": "Tr20x4", "form": "trapezoidal", "hand": "left", "starts": 2, "extent": "length", "length": "6 mm",
            "ease_out": "1.5 mm", "tip": "start", "chamfer_angle": "30 deg", "chamfer_depth": "0.5 mm",
            "modeled": true,
        })
    );
    // An empty fade length is ISO 3508's standard runout; a square end has none.
    click(&mut h, &["dialog:Fade Length"]);
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"key":"Backspace"})).unwrap();
    h.frames(2);
    choose(&mut h, "Hand", "Both");
    let p = params(&h);
    assert_eq!((&p["ease_out"], &p["hand"]), (&json!("standard"), &json!("both")));
    choose(&mut h, "End", "thread.end_square");
    assert!(params(&h).get("ease_out").is_none());
}

/// Buttress rows only for the buttress forms, the length only with a Length extent, the Up To
/// pick only with an Up To extent, the chamfer only with a chamfered tip, sizes only for the
/// forms with a size list.
#[test]
fn only_the_fields_that_apply_are_shown() {
    let mut h = shaft();
    for row in ["Form", "Size", "Pitch", "Designation", "Hand", "Starts", "Extent", "Start", "End"] {
        assert!(shown(row), "{row}");
    }
    for row in ["Steep Flank", "Length", "Fade Length", "Chamfer Angle", "Chamfer Depth"] {
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
    choose(&mut h, "End", "ui.fade");
    assert_eq!(inputs(&h), ["Cylindrical face", START_END], "a runout starts at an end");
    assert!(shown("Fade Length"));
    choose(&mut h, "Start", "Chamfer");
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
    choose(&mut h, "End", "ui.fade");
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
    assert_eq!((&p["hand"], &p["starts"], &p["length"], &p["ease_out"]), (&json!("left"), &json!(3), &json!("4 mm"), &json!("standard")));
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

/// The pipe threads: a G 1/2 shaft and an Rc 1/2 hole size themselves by the form, list their
/// sizes by designation without a pitch, and offer one start turning one way.
#[test]
fn pipe_forms_size_the_face_with_one_start_turning_one_way() {
    let size = |h: &Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Thread { designation, .. } => designation.clone(),
        k => panic!("{k:?}"),
    };
    let open = |hole: bool| {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        h.app.home.open = false;
        if hole {
            h.app.run("solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":12,"body_name":"Body1"})).unwrap();
            h.app.run("solid.cylinder", json!({"base":[0,0,-1],"radius":9.2,"height":14,"body_name":"Bore"})).unwrap();
            h.app.run("solid.combine", json!({"target":"Body1","tools":["Bore"],"operation":"cut"})).unwrap();
        } else {
            h.app.run("solid.cylinder", json!({"radius":20.955 / 2.0,"height":15})).unwrap();
        }
        h.app.session.selection = vec![thread_face(&h.app, "Body1")];
        h.step(&json!({"start":"solid.thread"})).unwrap();
        h.frames(2);
        h
    };
    let mut h = open(false);
    choose(&mut h, "Form", "thread.form_bspp");
    assert_eq!(size(&h), "G 1/2");
    assert!(shown("Size") && shown("Hand") && !shown("Pitch") && !shown("Starts"));
    choose(&mut h, "Hand", "Left");
    let p = params(&h);
    assert_eq!((&p["form"], &p["hand"]), (&json!("bspp"), &json!("left")));
    choose(&mut h, "Form", "thread.form_bspt");
    assert_eq!(size(&h), "R 1/2");
    choose(&mut h, "Form", "thread.form_npt");
    assert_eq!(params(&h)["form"], json!("npt"));
    // A hand of both, from a form that has it, turns right on a pipe thread.
    choose(&mut h, "Form", "ISO metric");
    choose(&mut h, "Hand", "Both");
    choose(&mut h, "Form", "thread.form_bspp");
    assert!(params(&h).get("hand").is_none(), "right, the default");

    let mut h = open(true);
    choose(&mut h, "Form", "thread.form_bspt");
    assert_eq!(size(&h), "Rc 1/2", "ISO 7-1 writes the internal taper thread Rc");
    choose(&mut h, "Form", "thread.form_rp");
    assert_eq!(size(&h), "Rp 1/2");
    let (cmd, p) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
    h.app.session.execute(&cmd, &p).unwrap_or_else(|e| panic!("Rp 1/2 in its hole: {e}"));
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
                "ease_out":"1 mm","tip":"start","chamfer_angle":"30 deg","chamfer_depth":"0.5 mm"}),
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
            assert!(shown("Steep Flank") && shown("Chamfer Angle") && shown("Fade Length"));
        }
        let (cmd, params) = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0);
        h.app.session.execute(&cmd, &params).unwrap_or_else(|e| panic!("{made}: OK on the reopened dialog: {e}"));
    }
}

/// Four separate cylindrical faces on an extruded boss and its circular-pattern copies.
fn patterned_shafts() -> (Harness, Vec<Sel>) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.cylinder", json!({"radius":30,"height":3,"body_name":"Plate"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY","offset":3,"name":"PostSketch"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[20,0],"radius":3})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance":10,"operation":"join","targets":["Plate"],"name":"Post"})).unwrap();
    h.app.run("solid.pattern.circular", json!({"features":["Post"],"axis":"Z","count":4,"name":"Posts"})).unwrap();
    let topology = h.app.run("model.faces", json!({"body":"Plate"})).unwrap();
    let picks = topology["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|face| face["cylinder"]["radius"].as_f64().is_some_and(|r| (r - 3.0).abs() < 1e-8))
        .map(|face| serde_json::from_value(json!({"type":"face","body":"Plate","index":face["index"],"point":face["point"]})).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(picks.len(), 4);
    (h, picks)
}

/// An empty or singly preselected Thread keeps the original viewport replacement behavior.
#[test]
fn ordinary_thread_face_clicks_replace_the_previous_face() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius":4,"height":10,"body_name":"First"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[30,0,0],"radius":10,"height":10,"body_name":"Second"})).unwrap();
    let first = thread_face(&h.app, "First");
    let second = thread_face(&h.app, "Second");
    for selection in [vec![], vec![first.clone()]] {
        h.app.session.selection = selection;
        h.app.start("solid.thread");
        for pick in [first.clone(), second.clone(), second.clone()] {
            let Sel::Face { body, index, point } = &pick else { panic!("face") };
            let hit = crate::viewport::Hit::Face { body: body.clone(), index: *index, point: *point };
            crate::viewport::dialog_pick(&mut h.app, &hit, pick.clone());
            let d = h.app.dialog.as_ref().unwrap();
            assert!(!d.inputs[0].multi);
            assert_eq!(d.inputs[0].items, vec![pick]);
        }
        let d = h.app.dialog.as_ref().unwrap();
        assert!(matches!(&d.kind, Kind::Thread { designation, .. } if designation == "M20"));
        let commands = apply_commands(&h.app, d).unwrap();
        assert!(commands[0].1.get("face").is_some());
        assert!(commands[0].1.get("faces").is_none());
        crate::dialogs::cancel(&mut h.app);
    }
}

/// A marquee containing several faces, or an explicit add, opts into the existing toggle behavior.
#[test]
fn thread_box_selection_explicitly_enables_multiple_faces() {
    let (mut h, picks) = patterned_shafts();
    h.app.session.selection.clear();
    for add in [false, true] {
        let mut d = crate::dialogs::Dialog::for_command(&h.app, "solid.thread").unwrap();
        assert!(!d.inputs[0].multi);
        d.take_box(&h.app.session, vec![picks[0].clone()], false);
        assert!(!d.inputs[0].multi, "a one-face marquee still replaces");
        if add {
            d.take_box(&h.app.session, vec![picks[1].clone()], true);
        } else {
            d.take_box(&h.app.session, picks[..2].to_vec(), false);
        }
        assert!(d.inputs[0].multi);
        assert_eq!(d.inputs[0].items, picks[..2]);
        d.pick(&h.app.session, picks[2].clone());
        assert_eq!(d.inputs[0].items, picks[..3]);
        d.pick(&h.app.session, picks[2].clone());
        assert_eq!(d.inputs[0].items, picks[..2]);
        let commands = apply_commands(&h.app, &d).unwrap();
        assert_eq!(commands[0].1["faces"].as_array().unwrap().len(), 2);
        d.take_box(&h.app.session, picks[2..].to_vec(), false);
        assert_eq!(d.inputs[0].items, picks[2..]);
        // Start/Up To inputs keep their existing single-selection contract.
        let Kind::Thread { options, .. } = &mut d.kind else { panic!("Thread") };
        options.extent = UP_TO_EXTENT;
        sync_inputs(&mut d.inputs, &mut d.active, options);
        let up_to = d.inputs.iter().position(|i| i.label == UP_TO).unwrap();
        d.active = up_to;
        d.take_box(&h.app.session, picks[..2].to_vec(), false);
        assert!(!d.inputs[up_to].multi);
        assert_eq!(d.inputs[up_to].items.len(), 1);
    }
}

/// Older saved groups have no target body domain but still keep every selected face when edited.
#[test]
fn a_legacy_multi_face_thread_reopens_in_group_selection_mode() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius":4,"height":10,"body_name":"First"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[30,0,0],"radius":4,"height":10,"body_name":"Second"})).unwrap();
    let faces = ["First", "Second"]
        .into_iter()
        .map(|body| match thread_face(&h.app, body) {
            Sel::Face { point, .. } => json!({"body":body,"point":point}),
            _ => panic!("face"),
        })
        .collect::<Vec<_>>();
    let id = h.app.run("solid.thread", json!({"faces":faces,"designation":"M8"})).unwrap()["feature"].as_u64().unwrap();
    let doc = std::sync::Arc::make_mut(&mut h.app.session.doc);
    let FeatureKind::Thread { target_bodies, .. } = &mut doc.feature_mut(id).unwrap().kind else { panic!("Thread") };
    target_bodies.clear();
    let saved = h.app.session.doc.to_json();
    h.app = crate::SolveApp::new(Session::new(Document::from_json(&saved).unwrap()), crate::Services::default());
    assert_eq!(h.app.session.model.state().threads.len(), 2);
    h.app.edit_feature(id);
    h.frames(2);
    let d = h.app.dialog.take().unwrap();
    assert!(d.inputs[0].multi);
    assert_eq!(d.inputs[0].items.len(), 2);
    assert!(h.app.session.model.state().threads.is_empty(), "editing shows the model before the Thread");
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1["params"]["faces"].as_array().unwrap().len(), 2);
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.model.state().threads.len(), 2);
    assert!(h.app.session.model.state().threads.iter().all(|thread| thread.feature == id));
}

/// The new dialog keeps every preselected boss and sends their shared options in one command.
#[test]
fn all_preselected_pattern_faces_receive_the_threads_options() {
    let (mut h, picks) = patterned_shafts();
    h.app.session.selection = picks.clone();
    h.app.start("solid.thread");
    let d = h.app.dialog.as_mut().unwrap();
    assert!(d.inputs[0].multi);
    assert_eq!(d.inputs[0].items.len(), 4);
    let Kind::Thread { designation, suggested, length, options, .. } = &mut d.kind else { panic!("Thread") };
    *designation = "M6x0.75".into();
    *suggested = None;
    *length = "6 mm".into();
    options.extent = LENGTH;
    options.hand = ThreadHand::Left;
    options.starts = 1;
    d.extra.insert("modeled".into(), json!(true));
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands.len(), 1);
    let (command, params) = &commands[0];
    assert_eq!(command, "solid.thread");
    assert!(params.get("face").is_none());
    assert_eq!(params["faces"].as_array().unwrap().len(), 4);
    for (face, pick) in params["faces"].as_array().unwrap().iter().zip(&picks) {
        let Sel::Face { body, point, .. } = pick else { panic!("face") };
        assert_eq!(*face, json!({"body":body,"point":[point.x, point.y, point.z]}));
    }
    assert_eq!(params["designation"], "M6x0.75");
    assert_eq!(params["length"], "6 mm");
    assert_eq!(params["hand"], "left");
    assert!(params.get("starts").is_none(), "the single-start default stays omitted");
    assert_eq!(params["modeled"], true);
}

/// One OK creates one saved feature for four threads, undone and redone together.
#[test]
fn a_thread_on_four_pattern_faces_applies_and_undoes_as_one_operation() {
    let (mut h, picks) = patterned_shafts();
    h.app.session.selection = picks;
    h.app.start("solid.thread");
    let mut d = h.app.dialog.take().unwrap();
    let Kind::Thread { designation, suggested, .. } = &mut d.kind else { panic!("Thread") };
    *designation = "M6x0.75".into();
    *suggested = None;
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let features = h.app.session.doc.features.len();
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.doc.features.len(), features + 1);
    let feature = h.app.session.doc.features.last().unwrap();
    let id = feature.id;
    let FeatureKind::Thread { additional_faces, target_bodies, .. } = &feature.kind else { panic!("Thread") };
    assert_eq!(additional_faces.len(), 3);
    assert_eq!(target_bodies.iter().map(|n| n.as_ref()).collect::<Vec<&str>>(), ["Plate"; 4]);
    let threads = h.app.session.model.state().threads.clone();
    assert_eq!(threads.len(), 4);
    for thread in &threads {
        assert_eq!(thread.designation, "M6x0.75");
        assert_eq!(thread.feature, id, "all markings belong to one timeline feature");
    }
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let saved = h.app.session.doc.to_json();
    let reopened = Session::new(Document::from_json(&saved).unwrap());
    assert_eq!(reopened.model.state().threads.len(), 4);
    assert_eq!(reopened.doc.features.len(), features + 1);
    let reopened_app = crate::SolveApp::new(reopened, crate::Services::default());
    let reopened_dialog = crate::dialogs::for_feature(&reopened_app, id, None).unwrap();
    assert_eq!(reopened_dialog.inputs[0].items.len(), 4);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.session.model.state().threads.is_empty());
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.model.state().threads.len(), 4);
}

/// A later invalid face leaves none of the preceding cosmetic threads in the document.
#[test]
fn an_invalid_face_cannot_partially_apply_a_thread_batch() {
    let (mut h, mut picks) = patterned_shafts();
    h.app.session.selection = picks.clone();
    h.app.start("solid.thread");
    let mut d = h.app.dialog.take().unwrap();
    let Kind::Thread { designation, suggested, .. } = &mut d.kind else { panic!("Thread") };
    *designation = "M6x0.75".into();
    *suggested = None;
    let topology = h.app.run("model.faces", json!({"body":"Plate"})).unwrap();
    let plate = topology["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|face| face["cylinder"]["radius"].as_f64().is_some_and(|r| (r - 30.0).abs() < 1e-8))
        .unwrap();
    picks.push(serde_json::from_value(json!({"type":"face","body":"Plate","index":plate["index"],"point":plate["point"]})).unwrap());
    d.inputs[0].items = picks;
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    assert!(crate::dialogs::run_dialog(&mut h.app, &d).is_err());
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.session.model.state().threads.is_empty());
}

/// Editing the grouped feature restores every face and can add or remove its members.
#[test]
fn editing_a_grouped_thread_reopens_adds_and_removes_faces() {
    let (mut h, picks) = patterned_shafts();
    h.app.session.selection = picks[..2].to_vec();
    h.app.start("solid.thread");
    let mut d = h.app.dialog.take().unwrap();
    let Kind::Thread { designation, suggested, .. } = &mut d.kind else { panic!("Thread") };
    *designation = "M6x0.75".into();
    *suggested = None;
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    let id = h.app.session.doc.features.last().unwrap().id;
    let features = h.app.session.doc.features.len();
    assert_eq!(h.app.session.model.state().threads.len(), 2);
    for selected in [picks.clone(), vec![picks[0].clone()]] {
        let previous = h.app.session.model.state().threads.len();
        let mut d = crate::dialogs::for_feature(&h.app, id, None).unwrap();
        assert!(d.inputs[0].multi);
        assert_eq!(d.inputs[0].items.len(), previous);
        d.take_box(&h.app.session, selected.clone(), false);
        let commands = apply_commands(&h.app, &d).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].0, "timeline.redefine");
        assert_eq!(commands[0].1["feature"], id);
        let params = &commands[0].1["params"];
        assert!(params.get("face").is_none(), "an edited body-qualified group retains its identity domain");
        assert_eq!(params["faces"].as_array().unwrap().len(), selected.len());
        h.app.session.execute(&commands[0].0, &commands[0].1).unwrap();
        assert_eq!(h.app.session.doc.features.len(), features);
        let state = h.app.session.model.state();
        assert_eq!(state.threads.len(), selected.len());
        assert!(state.threads.iter().all(|thread| thread.feature == id));
        let reopened = crate::dialogs::for_feature(&h.app, id, None).unwrap();
        assert!(reopened.inputs[0].multi, "a saved group keeps its selection mode with one member");
        assert_eq!(reopened.inputs[0].items.len(), selected.len());
    }
}

/// Repeated points on the same selected face create one feature, not duplicate markings.
#[test]
fn duplicate_face_picks_create_only_one_thread_command() {
    let mut h = shaft();
    let d = h.app.dialog.as_mut().unwrap();
    let mut duplicate = d.inputs[0].items[0].clone();
    if let Sel::Face { point, .. } = &mut duplicate {
        point.z = 5.0;
    }
    d.inputs[0].items.push(duplicate);
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands.len(), 1);
    assert!(commands[0].1.get("face").is_some());
    assert!(commands[0].1.get("faces").is_none());
}

/// A full history still retains the state before every thread of the operation.
#[test]
fn a_thread_batch_keeps_its_start_when_the_undo_history_is_full() {
    let (mut h, picks) = patterned_shafts();
    h.app.session.selection = picks;
    h.app.start("solid.thread");
    let mut d = h.app.dialog.take().unwrap();
    let Kind::Thread { designation, suggested, .. } = &mut d.kind else { panic!("Thread") };
    *designation = "M6x0.75".into();
    *suggested = None;
    let before = h.app.session.doc.to_json();
    let earlier = h.app.session.undo.last().unwrap().clone();
    // Exercise the engine's 200-step history without computing 200 extra features.
    h.app.session.undo.resize(200, earlier);
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.undo.len(), 200);
    assert_eq!(h.app.session.model.state().threads.len(), 4);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.session.model.state().threads.is_empty());
}

/// Editing after an upstream move resolves every saved face name, including the Up To slot.
#[test]
fn a_grouped_thread_reopens_named_faces_after_its_shafts_move() {
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name":"shift","expression":"0 mm"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":["shift",0,0],"radius":3,"height":10,"body_name":"First"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":["shift + 30 mm",0,0],"radius":3,"height":10,"body_name":"Second"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":["shift",30,0],"radius":3,"height":10,"body_name":"Limit"})).unwrap();
    let id = h
        .app
        .run(
            "solid.thread",
            json!({
                "faces":[{"body":"First","point":[3,0,2]},{"body":"Second","point":[33,0,2]}],"designation":"M6x0.75","from":[3,0,0],"up_to":[0,30,10]
            }),
        )
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    let names = h.app.session.doc.feature(id).unwrap().face_names.clone();
    assert_eq!(names.len(), 3, "primary face, Up To face, then the second shaft");
    assert!(names.iter().all(|name| !name.is_empty()));
    h.app.run("parameters.change", json!({"name":"shift","expression":"50 mm"})).unwrap();
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
    let feature = h.app.session.doc.feature(id).unwrap();
    assert_eq!(feature.face_names, names);
    let FeatureKind::Thread { face, additional_faces, .. } = &feature.kind else { panic!("Thread") };
    assert_eq!(face.0.x, 3.0, "the saved point remains at its original location");
    assert_eq!(additional_faces[0].0.x, 33.0);
    let saved = h.app.session.doc.to_json();
    h.app = crate::SolveApp::new(Session::new(Document::from_json(&saved).unwrap()), crate::Services::default());
    let features = h.app.session.doc.features.len();
    h.app.edit_feature(id);
    h.frames(2);
    let d = h.app.dialog.take().expect("the saved group can be edited");
    assert_eq!(d.inputs[0].items.len(), 2);
    for (pick, expected) in d.inputs[0].items.iter().zip(["First", "Second"]) {
        let Sel::Face { body, point, .. } = pick else { panic!("face") };
        assert_eq!(body, expected, "the Up To name cannot stand for an added shaft");
        assert!(point.x >= 47.0 - 1e-6, "the pick follows its moved source, not its saved coordinate");
    }
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0, "timeline.redefine");
    let params = &commands[0].1["params"];
    assert_eq!(params["faces"].as_array().unwrap().len(), 2);
    assert!(params["up_to"][0].as_f64().unwrap() >= 47.0 - 1e-6, "Up To follows its own name too");
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.doc.features.len(), features);
    let state = h.app.session.model.state();
    assert_eq!(state.threads.len(), 2);
    for (thread, (body, x)) in state.threads.iter().zip([("First", 50.0), ("Second", 80.0)]) {
        assert_eq!(thread.feature, id);
        assert_eq!(thread.body, body);
        assert!((thread.axis_point.x - x).abs() < 1e-6);
    }
}

/// Reopening an unchanged group preserves its exact saved picks through name reprojection.
#[test]
fn an_unchanged_grouped_thread_edit_keeps_its_exact_saved_parameters() {
    let mut h = Harness::new();
    h.app.run("solid.cylinder", json!({"radius":3,"height":10,"body_name":"First"})).unwrap();
    h.app.run("solid.cylinder", json!({"base":[30,0,0],"radius":3,"height":10,"body_name":"Second"})).unwrap();
    let points = ["First", "Second"]
        .into_iter()
        .map(|body| match thread_face(&h.app, body) {
            Sel::Face { point, .. } => json!({"body":body,"point":point}),
            other => panic!("expected a cylindrical face: {other:?}"),
        })
        .collect::<Vec<_>>();
    let id = h.app.run("solid.thread", json!({"faces":points,"designation":"M6x0.75"})).unwrap()["feature"].as_u64().unwrap();
    let before = h.app.session.doc.feature(id).unwrap().kind.clone();
    let saved = h.app.session.doc.to_json();
    h.app = crate::SolveApp::new(Session::new(Document::from_json(&saved).unwrap()), crate::Services::default());
    h.app.edit_feature(id);
    h.frames(2);
    let d = h.app.dialog.take().expect("the unchanged group can be edited");
    assert_eq!(d.inputs[0].items.len(), 2);
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.doc.feature(id).unwrap().kind, before, "a no-op edit preserves every saved floating-point value");
}

/// Cold edit and rename preserve each body's identity, even where two source bodies coincide.
#[test]
fn grouped_thread_dialog_preserves_coincident_body_identities_and_missing_slots() {
    let mut h = Harness::new();
    for body in ["A", "B"] {
        h.app.run("solid.cylinder", json!({"radius":4,"height":8,"body_name":body,"operation":"new"})).unwrap();
    }
    let id = h.app.run("solid.thread", json!({"faces":[{"body":"B","point":[4,0,2]},{"body":"A","point":[4,0,2]}],"designation":"M8"})).unwrap()["feature"].as_u64().unwrap();
    h.app.run("body.rename", json!({"body":"B","name":"Renamed"})).unwrap();
    let saved = h.app.session.doc.to_json();
    h.app = crate::SolveApp::new(Session::new(Document::from_json(&saved).unwrap()), crate::Services::default());
    h.app.edit_feature(id);
    h.frames(2);
    let mut d = h.app.dialog.take().unwrap();
    let commands = apply_commands(&h.app, &d).unwrap();
    let faces = commands[0].1["params"]["faces"].as_array().unwrap();
    assert_eq!(faces.iter().map(|p| p["body"].as_str().unwrap()).collect::<Vec<_>>(), ["Renamed", "A"]);
    crate::dialogs::run_dialog(&mut h.app, &d).unwrap();
    assert_eq!(h.app.session.model.state().threads.iter().map(|t| t.body.as_str()).collect::<Vec<_>>(), ["Renamed", "A"]);
    d.inputs[0].items.truncate(1);
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands[0].1["params"]["faces"].as_array().unwrap().len(), 1);
    assert_eq!(commands[0].1["params"]["faces"][0]["body"], "Renamed");
    // An unresolved saved slot remains visible and blocks Apply, rather than dropping a face.
    let mut broken: serde_json::Value = serde_json::from_str(&saved).unwrap();
    broken["features"].as_array_mut().unwrap().last_mut().unwrap()["face_names"][1] = json!("missing");
    let app = crate::SolveApp::new(Session::new(Document::from_json(&broken.to_string()).unwrap()), crate::Services::default());
    let d = crate::dialogs::for_feature(&app, id, None).unwrap();
    assert_eq!(d.inputs[0].items.len(), 2, "the failed saved reference is not silently dropped");
    assert!(apply_commands(&app, &d).is_err());
}
