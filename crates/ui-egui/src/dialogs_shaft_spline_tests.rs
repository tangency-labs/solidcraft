use super::*;
use crate::scenario::{self, Harness, handle_at, rect_of};

/// The dialog's values, as the open dialog holds them.
fn spline(h: &mut Harness) -> &mut Spline {
    match h.app.dialog.as_mut().map(|d| &mut d.kind) {
        Some(Kind::ShaftSpline(s)) => s,
        other => panic!("no Shaft Spline dialog: {other:?}"),
    }
}

/// A copy of the dialog's values, for a call that reads the app as well.
fn values(h: &mut Harness) -> Spline {
    spline(h).clone()
}

/// Unfolds the section titled `title` when it is folded, which `shown` says by what the section draws only while
/// it is open.
fn unfold(h: &mut Harness, title: &str, shown: impl Fn() -> bool) {
    if !shown() {
        h.step(&json!({"click": {"handle": format!("dialog:section:{title}")}})).unwrap();
        h.frames(2);
    }
}

/// Unfolds the Master spline section when it is folded: its check box is then not drawn.
fn open_master(h: &mut Harness) {
    unfold(h, "shaft_spline.master", || handle_at("dialog:shaft_spline:master").is_some());
}

/// Unfolds the Tip corners section when it is folded: its Corners row is then not drawn.
fn open_corners(h: &mut Harness) {
    unfold(h, "shaft_spline.tip_corners", || rect_of("dialog:row:shaft_spline.corners").is_some());
}

/// Unfolds the Chamfers section when it is folded: the End chamfer row is then not drawn.
fn open_chamfers(h: &mut Harness) {
    unfold(h, "shaft_spline.chamfers", || handle_at("dialog:shaft_spline:end_chamfer").is_some());
}

fn open() -> Harness {
    let mut h = Harness::new();
    // Tall enough for the whole dialog: a row scrolled out of view can be neither found nor clicked.
    h.call("ui.resize", json!({"width": 1600, "height": 2000}));
    h.app.start(COMMAND);
    h.frames(3);
    h
}

fn bodies(h: &Harness) -> usize {
    h.app.session.model.state().bodies.len()
}

/// The preview the dialog draws now.
fn drawn(h: &mut Harness) -> Arc<SplinePreview> {
    spline(h).preview().cloned().unwrap_or_else(|| panic!("no preview: {:?}", spline(h).refusal()))
}

/// Shaft Spline from the palette, a shortcut or `ui.start` opens its dialog, with the drawing,
/// its caption and the rows of an involute spline; nothing is made before OK.
#[test]
fn the_dialog_opens_for_the_command() {
    let mut h = open();
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ShaftSpline(_))));
    assert!(rect_of("dialog:shaft_spline:drawing").is_some_and(|r| r.width() > 100.0 && r.height() > 75.0));
    assert!(rect_of("dialog:shaft_spline:caption").is_some());
    for key in ["teeth", "length", "module", "side_clearance"] {
        assert!(handle_at(&format!("dialog:shaft_spline:{key}")).is_some(), "{key}");
    }
    let p = drawn(&mut h);
    assert_eq!(p.numbers.teeth, 20);
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(1.0));
    assert_eq!(bodies(&h), 0, "nothing is made before OK");
}

/// Each kind of teeth shows its own rows and none of the others', and sends only those.
#[test]
fn each_type_shows_its_rows_and_hides_the_others() {
    let mut h = open();
    let rows = [
        ("module", [true, false, false]),
        ("pressure_angle", [true, false, false]),
        ("hub_addendum", [true, false, false]),
        ("tip_clearance", [true, false, false]),
        ("form_clearance", [true, false, false]),
        ("major_diameter", [false, true, true]),
        ("minor_diameter", [false, true, true]),
        ("tooth_width", [false, true, false]),
        ("flank_angle", [false, false, true]),
        ("root_round", [false, true, true]),
        ("major_clearance", [false, true, true]),
        ("minor_clearance", [false, true, true]),
        ("hub_tip_round", [false, true, true]),
        ("side_clearance", [true, true, true]),
        ("hub_root_round", [true, true, true]),
        ("length", [true, true, true]),
        // Plain corners have no size; no hub, no outside diameter; no master spline, no angle.
        ("tip_size", [false, false, false]),
        ("hub_diameter", [false, false, false]),
        ("master_angle", [false, false, false]),
    ];
    for kind in 0..3 {
        spline(&mut h).kind = kind;
        h.frames(3);
        // The Tip corners of straight-sided teeth and serrations start folded.
        if kind != 0 {
            open_corners(&mut h);
        }
        let now = values(&mut h);
        let sent = commands(&h.app, &now).unwrap_or_else(|e| panic!("kind {kind}: {e}"));
        let (_, p) = &sent[0];
        for (key, shown) in rows {
            assert_eq!(handle_at(&format!("dialog:shaft_spline:{key}")).is_some(), shown[kind], "kind {kind}: row {key}");
            assert_eq!(p.get(key).is_some(), shown[kind], "kind {kind}: {key} sent");
        }
        assert_eq!(rect_of("dialog:row:shaft_spline.corners").is_some(), kind != 0, "kind {kind}: the tip corners");
        assert_eq!(p["type"], json!(TYPES[kind].0));
        assert_eq!(drawn(&mut h).numbers.kind, serde_json::from_value(json!(TYPES[kind].0)).unwrap());
    }
    // A corner with a size, a hub and a master spline bring their rows, and their parameters.
    spline(&mut h).kind = 1;
    spline(&mut h).corner[1] = 1;
    spline(&mut h).make = 2;
    spline(&mut h).master = true;
    h.frames(3);
    open_master(&mut h);
    for key in ["tip_size", "hub_diameter", "master_angle"] {
        assert!(handle_at(&format!("dialog:shaft_spline:{key}")).is_some(), "row {key}");
    }
    let now = values(&mut h);
    let (_, p) = &commands(&h.app, &now).unwrap()[0];
    assert_eq!(
        (&p["tip_corner"], &p["tip_size"], &p["make"], &p["hub_diameter"]),
        (&json!("chamfered"), &json!("0.3 mm"), &json!("both"), &json!("40 mm"))
    );
}

/// The drawing follows the rows: more teeth, another outline.
#[test]
fn changing_a_row_changes_the_drawing() {
    let mut h = open();
    let before = drawn(&mut h);
    spline(&mut h).set("teeth", "24");
    h.frames(3);
    let after = drawn(&mut h);
    assert_eq!(after.numbers.teeth, 24);
    assert!((after.pitch - TAU_OF(24)).abs() < 1e-12 && (before.pitch - TAU_OF(20)).abs() < 1e-12);
    assert_ne!(before.outlines[0].points, after.outlines[0].points);
    // The pitch circle grows with the count of teeth: module times teeth.
    assert_eq!(after.numbers.pitch_diameter, Some(36.0));
}

#[allow(non_snake_case)]
fn TAU_OF(teeth: u32) -> f64 {
    std::f64::consts::TAU / f64::from(teeth)
}

/// A value the engine refuses: its words under the drawing, the drawing kept dimmed, the row
/// named, OK off, and Enter makes nothing.
#[test]
fn a_bad_value_shows_the_refusal_and_keeps_ok_from_running() {
    let mut h = open();
    let undo = h.app.session.undo.len();
    // Hub teeth that reach into the shaft's root fillets.
    spline(&mut h).set("hub_addendum", "0.85");
    h.frames(3);
    let refusal = spline(&mut h).refusal().cloned().expect("refused");
    assert_eq!(refusal.key.as_deref(), Some("hub_addendum"), "{refusal:?}");
    assert!(refusal.words.contains("`hub_addendum`"), "{refusal:?}");
    assert!(rect_of(&format!("dialog:status:{}", refusal.words)).is_some(), "the words are on the caption line");
    assert!(spline(&mut h).good.is_some(), "the last good drawing stays");
    // The command refuses it in the same words.
    let (_, p) = (COMMAND, values(&mut h).params(&h.app.session.doc).unwrap());
    let err = h.app.session.scratch().execute(COMMAND, &p).unwrap_err().to_string();
    assert!(err.contains(&refusal.words), "{err} / {refusal:?}");
    let now = values(&mut h);
    assert_eq!(commands(&h.app, &now).unwrap_err(), refusal.words);
    assert_eq!(scenario::count_of("dialog:ok:enabled"), Some(0.0));
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_some(), "a refused dialog stays open");
    assert_eq!((bodies(&h), h.app.session.undo.len()), (0, undo), "nothing was made");
    // A text that is no number names its row too.
    spline(&mut h).set("module", "lots");
    h.frames(2);
    assert_eq!(spline(&mut h).refusal().and_then(|b| b.key.clone()).as_deref(), Some("module"));
}

/// Enter runs Shaft Spline with the dialog's values: a shaft and its hub, taken away by one undo.
#[test]
fn enter_makes_the_bodies_and_undo_takes_them_away() {
    let mut h = open();
    spline(&mut h).kind = 1;
    spline(&mut h).make = 2;
    h.frames(3);
    let undo = h.app.session.undo.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    assert_eq!(bodies(&h), 2);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(bodies(&h), 0, "one undo takes both away");
}

/// Cancel and Escape add nothing.
#[test]
fn cancel_adds_nothing() {
    let mut h = open();
    h.step(&json!({"key": "Escape"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none());
    assert_eq!(bodies(&h), 0);
    assert!(h.app.session.doc.features.is_empty());
}

/// The master spline's rows come with its check box, and its form and angle reach the command.
#[test]
fn the_master_spline_rows_appear_with_the_check_box_and_reach_the_command() {
    let mut h = open();
    spline(&mut h).kind = 2;
    h.frames(3);
    open_master(&mut h);
    assert!(handle_at("dialog:shaft_spline:master").is_some());
    assert!(rect_of("dialog:row:shaft_spline.master_form").is_none() && handle_at("dialog:shaft_spline:master_angle").is_none());
    h.step(&json!({"click": {"handle": "dialog:shaft_spline:master"}})).unwrap();
    h.frames(3);
    assert!(spline(&mut h).master, "the check box sets it");
    assert!(rect_of("dialog:row:shaft_spline.master_form").is_some() && handle_at("dialog:shaft_spline:master_angle").is_some());
    spline(&mut h).form = 1;
    spline(&mut h).master_angle = "90 deg".into();
    h.frames(3);
    let now = values(&mut h);
    let (_, p) = &commands(&h.app, &now).unwrap()[0];
    assert_eq!((&p["master_spline"], &p["master_angle"]), (&json!("wide_tooth"), &json!("90 deg")));
    let preview = drawn(&mut h);
    let master = preview.master.as_ref().expect("a master spline");
    assert!((master.angle - 90.0).abs() < 1e-9 && preview.numbers.positions == 1, "{master:?}");
    // The Show choice offers the master spline now.
    spline(&mut h).show = 2;
    h.frames(2);
    assert_eq!(spline(&mut h).show, 2);
    spline(&mut h).master = false;
    h.frames(2);
    assert_eq!(spline(&mut h).show, 0, "no master spline, nothing to show of it");
}

/// The painter draws the outlines, the named circles and the master spline's arcs, and nothing
/// outside its rectangle, in every view.
#[test]
fn the_painter_draws_the_outline_circles_and_master_arc_inside_its_rectangle() {
    let doc = Document::default();
    let mut s = Spline::new(&doc);
    s.kind = 1;
    s.make = 2;
    s.master = true;
    let w = s.work(&doc);
    let Ok((_, preview)) = &w.result else { panic!("{:?}", w.result) };
    let ctx = egui::Context::default();
    let rect = Rect::from_min_size(pos2(20.0, 30.0), vec2(240.0, 180.0));
    for view in [View::Whole, View::Teeth, View::Master] {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui.painter(), rect, preview, view, &Look { dim: false, unit: ("mm", 1.0) }));
        out.textures_delta.clear();
        let mut closed = 0;
        let mut thick = 0;
        let mut dashes = 0;
        for c in &out.shapes {
            // The frame is drawn on the rectangle's own border; everything else is clipped to the rectangle.
            let inside =
                rect.contains_rect(c.clip_rect) || c.clip_rect.width() <= 0.0 || rect.expand(1.0).contains_rect(c.shape.visual_bounding_rect());
            assert!(inside, "{view:?}: drawn outside {:?}", c.clip_rect);
            match &c.shape {
                Shape::Path(path) if path.closed && path.points.len() > 20 => closed += 1,
                Shape::Path(path) if !path.closed && (path.stroke.width - 3.0).abs() < 1e-3 => thick += 1,
                Shape::LineSegment { .. } => dashes += 1,
                _ => {}
            }
        }
        assert_eq!(closed, 2, "{view:?}: the shaft's and the hub's outlines");
        assert_eq!(thick, 2, "{view:?}: the master spline's arcs on the shaft and in the hub");
        assert!(dashes > 20, "{view:?}: the dashed circles");
    }
}

/// A 30 mm shaft, 60 mm long, a pick on its outside near the top, and a pick on its top end, the face the teeth
/// start from.
fn picked_shaft() -> (Session, Sel, Sel) {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius": 15, "height": 60, "body_name": "Shaft"})).unwrap();
    let face = |index: usize, point: Vec3| Sel::Face { body: "Shaft".into(), index, point };
    (s, face(0, Vec3::new(15.0, 0.0, 55.0)), face(1, Vec3::new(5.0, 0.0, 60.0)))
}

/// The End chamfer goes with a shaft that is made or cut, the Entry chamfer with a hub or a bore, each only with a
/// size above 0, as typed.
#[test]
fn the_chamfer_rows_go_with_the_part_made_and_only_with_a_size() {
    let doc = Document::default();
    let typed = |text: &str| json!(crate::length_input::expression(&doc, text, ValueKind::Length));
    let mut s = Spline::new(&doc);
    s.kind = 1;
    s.end_chamfer = "1 mm".into();
    s.entry_chamfer = "0.8 mm".into();
    // Make: a shaft, a hub, both.
    for (make, end, entry) in [(0, true, false), (1, false, true), (2, true, true)] {
        s.make = make;
        let p = s.command_params(&doc, Picks::default()).unwrap_or_else(|b| panic!("make {make}: {b:?}"));
        assert_eq!(p.get("end_chamfer").cloned(), end.then(|| typed("1 mm")), "make {make}: {p}");
        assert_eq!(p.get("entry_chamfer").cloned(), entry.then(|| typed("0.8 mm")), "make {make}: {p}");
    }
    // A size of 0 is no chamfer: nothing is sent.
    s.end_chamfer = "0 mm".into();
    s.entry_chamfer = "0".into();
    let p = s.command_params(&doc, Picks::default()).unwrap();
    assert!(p.get("end_chamfer").is_none() && p.get("entry_chamfer").is_none(), "{p}");
    // With a face picked the pick says what is cut, whatever Make says, and no new part's keys are sent.
    let (session, sel, start) = picked_shaft();
    s.end_chamfer = "1 mm".into();
    s.entry_chamfer = "0.8 mm".into();
    let shaft = s.command_params(&session.doc, Picks { shaft: Some(&sel), shaft_start: Some(&start), ..Picks::default() }).unwrap();
    assert_eq!(shaft["shaft_face"], pt(Vec3::new(15.0, 0.0, 55.0)), "{shaft}");
    assert_eq!((shaft["shaft_start"].clone(), shaft.get("bore_start")), (pt(Vec3::new(5.0, 0.0, 60.0)), None), "{shaft}");
    assert_eq!((shaft.get("end_chamfer").cloned(), shaft.get("entry_chamfer")), (Some(typed("1 mm")), None), "{shaft}");
    assert!(shaft.get("make").is_none() && shaft.get("hub_diameter").is_none() && shaft.get("bore_face").is_none(), "{shaft}");
    assert_eq!(shaft.get("cutter_diameter").cloned(), Some(typed(&s.cutter_diameter)), "the cutter goes with a picked shaft: {shaft}");
    let bore = s.command_params(&session.doc, Picks { bore: Some(&sel), bore_start: Some(&start), ..Picks::default() }).unwrap();
    assert_eq!((bore.get("end_chamfer"), bore.get("entry_chamfer").cloned()), (None, Some(typed("0.8 mm"))), "{bore}");
    assert!(bore.get("make").is_none() && bore.get("cutter_diameter").is_none() && bore.get("shaft_face").is_none(), "{bore}");
    assert_eq!(bore["bore_face"], pt(Vec3::new(15.0, 0.0, 55.0)), "{bore}");
    assert_eq!((bore["bore_start"].clone(), bore.get("shaft_start")), (pt(Vec3::new(5.0, 0.0, 60.0)), None), "{bore}");
}

/// A picked part needs the face its teeth start from: without it the command is not made, in words, and nothing
/// names a row. A saved spline that shows no start face keeps the end it has, and its edit goes without one.
#[test]
fn a_picked_part_needs_the_face_its_teeth_start_from() {
    let (session, sel, start) = picked_shaft();
    let doc = &session.doc;
    let mut s = Spline::new(doc);
    s.kind = 1;
    s.cutter = false;
    for (picks, words) in [
        (Picks { shaft: Some(&sel), ..Picks::default() }, "shaft_spline.need_shaft_start"),
        (Picks { bore: Some(&sel), ..Picks::default() }, "shaft_spline.need_bore_start"),
        (Picks { shaft: Some(&sel), shaft_start: Some(&start), bore: Some(&sel), bore_start: None }, "shaft_spline.need_bore_start"),
    ] {
        let refused = s.command_params(doc, picks).expect_err("a picked part without its start face");
        assert_eq!((refused.key.as_deref(), refused.words.as_str()), (None, tr(words)), "{picks:?}");
        // The teeth are drawn all the same: the drawing does not wait for the start face.
        assert!(s.params_for(doc, picks).is_ok(), "{picks:?}");
    }
    let both = Picks { shaft: Some(&sel), shaft_start: Some(&start), bore: Some(&sel), bore_start: Some(&start) };
    let p = s.command_params(doc, both).unwrap();
    assert_eq!((p["shaft_start"].clone(), p["bore_start"].clone()), (pt(Vec3::new(5.0, 0.0, 60.0)), pt(Vec3::new(5.0, 0.0, 60.0))), "{p}");
    // A start face with no round face picked is not sent.
    let alone = s.command_params(doc, Picks { shaft_start: Some(&start), ..Picks::default() }).unwrap();
    assert!(alone.get("shaft_start").is_none() && alone.get("shaft_face").is_none(), "{alone}");
    s.start_optional = [true, false];
    let p = s.command_params(doc, Picks { shaft: Some(&sel), ..Picks::default() }).unwrap();
    assert!(p.get("shaft_start").is_none() && p.get("shaft_face").is_some(), "{p}");
    // A face in the Shaft row that is not round goes to the engine without a start face: the engine says what
    // is wrong with the pick, where asking for the start face would point at the wrong row.
    s.start_optional = [false; 2];
    s.not_round = [true, false];
    let p = s.command_params(doc, Picks { shaft: Some(&start), ..Picks::default() }).unwrap();
    assert!(p.get("shaft_start").is_none() && p.get("shaft_face").is_some(), "{p}");
    let refused = session.scratch().execute(COMMAND, &p).unwrap_err().to_string();
    assert!(refused.contains("no round face where the shaft was picked"), "{refused}");
}

/// The row that waits next is the start face of a part whose round face is picked, before any other free row,
/// and a face that lands in a row it does not belong in is moved to its own.
#[test]
fn the_pick_rows_wait_in_order_and_sort_their_faces() {
    let mut h = Harness::new();
    h.app.session.execute("solid.cylinder", &json!({"radius": 15, "height": 60, "body_name": "Shaft"})).unwrap();
    // A ring above the shaft, with a round bore.
    for (command, params) in [
        ("sketch.create", json!({"plane": "XY", "offset": 70, "name": "Hub section"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 25})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 16})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 20, "operation": "new", "body_name": "Hub"})),
    ] {
        h.app.session.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"));
    }
    let face = |body: &str, index: usize, point: Vec3| Sel::Face { body: body.into(), index, point };
    let outside = face("Shaft", 0, Vec3::new(15.0, 0.0, 55.0));
    let top = face("Shaft", 1, Vec3::new(5.0, 0.0, 60.0));
    let wall = face("Hub", 0, Vec3::new(16.0, 0.0, 80.0));
    let under = face("Hub", 1, Vec3::new(20.0, 0.0, 70.0));
    let mut rows = inputs();
    let mut s = Spline::new(&h.app.session.doc);
    let held = |rows: &[SelInput]| -> Vec<Option<Sel>> { rows.iter().map(|i| i.items.first().cloned()).collect() };
    assert_eq!(next_pick(&rows), Some(SHAFT), "nothing picked: the first row waits");
    // A bore lands in the first row, Shaft: it goes to Bore, and Bore start waits, not the free Shaft row.
    rows[SHAFT].items = vec![wall.clone()];
    assert_eq!(sort_picks(&h.app, &mut s, &mut rows), Some(true));
    assert_eq!(held(&rows), [None, None, Some(wall.clone()), None]);
    assert_eq!(next_pick(&rows), Some(BORE_START));
    assert_eq!(s.not_round, [false, false]);
    // A flat face alone in the Shaft row is no round face: the dialog knows it.
    let mut flat = inputs();
    flat[SHAFT].items = vec![top.clone()];
    let mut other = Spline::new(&h.app.session.doc);
    assert_eq!(sort_picks(&h.app, &mut other, &mut flat), Some(false));
    assert_eq!(other.not_round, [true, false]);
    // The hub's flat face lands in a round face's row: it is the face the hub's teeth start from.
    rows[SHAFT].items = vec![under.clone()];
    assert_eq!(sort_picks(&h.app, &mut s, &mut rows), Some(true));
    assert_eq!(held(&rows), [None, None, Some(wall.clone()), Some(under.clone())]);
    assert_eq!(next_pick(&rows), Some(SHAFT));
    // The shaft, then its start face in the row that waits.
    rows[SHAFT].items = vec![outside.clone()];
    assert_eq!(sort_picks(&h.app, &mut s, &mut rows), Some(false), "a shaft in Shaft stays");
    assert_eq!(sort_picks(&h.app, &mut s, &mut rows), None, "nothing changed since the last look");
    assert_eq!(next_pick(&rows), Some(SHAFT_START));
    rows[SHAFT_START].items = vec![top.clone()];
    assert_eq!(sort_picks(&h.app, &mut s, &mut rows), Some(false));
    assert_eq!(held(&rows), [Some(outside.clone()), Some(top.clone()), Some(wall.clone()), Some(under.clone())]);
    assert_eq!(next_pick(&rows), None, "every row holds a face");
    assert_eq!(shown_picks(&rows), [0..1, 1..2, 2..3, 3..4]);
    // The start rows show with their part: nothing picked, the two round faces' rows alone.
    assert_eq!(shown_picks(&inputs()), [0..1, 2..3]);
    // A shaft and a bore each in the other's row change places, their start faces with them.
    let mut crossed = inputs();
    crossed[SHAFT].items = vec![wall.clone()];
    crossed[SHAFT_START].items = vec![under.clone()];
    crossed[BORE].items = vec![outside.clone()];
    crossed[BORE_START].items = vec![top.clone()];
    let mut s = Spline::new(&h.app.session.doc);
    assert_eq!(sort_picks(&h.app, &mut s, &mut crossed), Some(true));
    assert_eq!(held(&crossed), [Some(outside), Some(top), Some(wall), Some(under)]);
}

/// A row that is not a value is refused naming its key: the rows the preview reads, and the rows that go to the
/// command only (the chamfers, the cutter), each only where it is sent.
#[test]
fn a_row_that_is_not_a_value_is_refused_with_its_key() {
    let doc = Document::default();
    let mut s = Spline::new(&doc);
    s.kind = 1;
    s.end_chamfer = "abc".into();
    let key = |r: Result<Value, Bad>| r.err().and_then(|b| b.key);
    assert_eq!(key(s.command_params(&doc, Picks::default())).as_deref(), Some("end_chamfer"));
    // Making a hub alone, the End chamfer is not sent, so it is not read.
    s.make = 1;
    assert!(s.command_params(&doc, Picks::default()).is_ok());
    s.make = 2;
    s.end_chamfer = "1 mm".into();
    s.entry_chamfer = String::new();
    assert_eq!(key(s.command_params(&doc, Picks::default())).as_deref(), Some("entry_chamfer"));
    // The cutter of a picked shaft, while its box is ticked.
    let (session, sel, start) = picked_shaft();
    let picks = Picks { shaft: Some(&sel), shaft_start: Some(&start), ..Picks::default() };
    s.entry_chamfer = "0".into();
    s.cutter_diameter = "wide".into();
    assert_eq!(key(s.command_params(&session.doc, picks)).as_deref(), Some("cutter_diameter"));
    s.cutter = false;
    let p = s.command_params(&session.doc, picks).unwrap();
    assert!(p.get("cutter_diameter").is_none(), "{p}");
    // The rows the preview reads.
    s.set("minor_diameter", "lots");
    assert_eq!(key(s.params(&doc)).as_deref(), Some("minor_diameter"));
    s.set("minor_diameter", "23 mm");
    s.set("teeth", "6.5");
    assert_eq!(key(s.params(&doc)).as_deref(), Some("teeth"));
}

/// A picked shaft sets the rows the dialog filled itself: the size that fits the shaft as it is, the length, the
/// cutter. A size the user typed stays. Keeping the shaft's diameter, the size is not sent, so the command takes
/// the shaft's own, and the command takes what the dialog sends.
#[test]
fn the_rows_start_from_a_picked_shaft_and_a_typed_size_stays() {
    let (session, sel, start) = picked_shaft();
    let doc = &session.doc;
    let shown = |mm: f64| crate::length_input::shown(doc, mm);
    let picks = Picks { shaft: Some(&sel), shaft_start: Some(&start), ..Picks::default() };
    let mut s = Spline::new(doc);
    s.sync_picks(&session, picks);
    // 30 mm across: the Major diameter is the shaft's own, and the Module the one that puts the tips of 20 teeth on
    // it (module × (teeth + 2 × addendum), with the addendum of 0.5 the dialog starts with).
    for kind in [1, 2] {
        assert_eq!(s.values[kind].get("major_diameter").map(String::as_str), Some(shown(30.0).as_str()), "kind {kind}");
    }
    assert_eq!(s.values[0].get("module").map(String::as_str), Some(shown(30.0 / 21.0).as_str()));
    // The length is the shaft's diameter, within the 60 mm face; the cutter twice the diameter.
    assert_eq!((s.length.as_str(), s.cutter_diameter.as_str()), (shown(30.0).as_str(), shown(60.0).as_str()));
    // The size row is a number like any other, sent as typed.
    s.kind = 1;
    s.cutter = false;
    let p = s.command_params(doc, picks).unwrap();
    assert_eq!(p["major_diameter"], json!(crate::length_input::expression(doc, &shown(30.0), ValueKind::Length)), "{p}");
    // Keeping the shaft's diameter: the row is left out for the command, and the preview gets the number that fits.
    s.fit = true;
    let p = s.command_params(doc, picks).unwrap();
    assert!(p.get("major_diameter").is_none(), "{p}");
    let preview = s.params_for(doc, picks).unwrap();
    assert!((preview["major_diameter"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9, "{preview}");
    // The command takes what the dialog sends: one Shaft Spline on the shaft, at the shaft's own diameter.
    let mut scratch = session.scratch();
    let r = scratch.execute(COMMAND, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!((r["shaft_major_diameter"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9, "{r}");
    assert!(matches!(scratch.doc.features.last().map(|f| &f.kind), Some(FeatureKind::ShaftSpline { .. })));
    // A size the user typed before the pick stays.
    let mut typed = Spline::new(doc);
    typed.kind = 1;
    typed.set("major_diameter", "26 mm");
    typed.sync_picks(&session, picks);
    assert_eq!(typed.values[1].get("major_diameter").map(String::as_str), Some("26 mm"));
}

/// A chamfer row that names a parameter or works something out is sent even where it comes to 0 today, so the
/// feature keeps the expression and a chamfer comes when the parameter grows; only a plain 0 is no chamfer. A
/// negative size is sent too, and the command refuses it.
#[test]
fn a_chamfer_given_by_an_expression_is_sent_even_where_it_comes_to_zero() {
    let mut session = Session::default();
    session.execute("parameters.add", &json!({"name": "c", "expression": "0 mm"})).unwrap();
    let doc = &session.doc;
    let typed = |text: &str| json!(crate::length_input::expression(doc, text, ValueKind::Length));
    let mut s = Spline::new(doc);
    s.kind = 1;
    s.make = 2;
    s.end_chamfer = "c".into();
    s.entry_chamfer = "c * 2".into();
    let p = s.command_params(doc, Picks::default()).unwrap();
    assert_eq!((p.get("end_chamfer").cloned(), p.get("entry_chamfer").cloned()), (Some(typed("c")), Some(typed("c * 2"))), "{p}");
    // The command takes it, keeps both expressions on the feature, and cuts no chamfer while they come to 0.
    let mut scratch = session.scratch();
    scratch.execute(COMMAND, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(
        matches!(scratch.doc.features.last().map(|f| &f.kind), Some(FeatureKind::ShaftSpline { end_chamfer: Some(_), entry_chamfer: Some(_), .. })),
        "the feature keeps the chamfers' expressions"
    );
    // A plain 0, with or without a unit, is no chamfer: nothing is sent.
    for zero in ["0", "0 mm", "0.0mm", " 0.00 "] {
        s.end_chamfer = zero.into();
        s.entry_chamfer = zero.into();
        let p = s.command_params(doc, Picks::default()).unwrap_or_else(|b| panic!("{zero:?}: {b:?}"));
        assert!(p.get("end_chamfer").is_none() && p.get("entry_chamfer").is_none(), "{zero:?}: {p}");
    }
    // A negative size goes to the command, which says what is wrong with it.
    s.end_chamfer = "-1 mm".into();
    let p = s.command_params(doc, Picks::default()).unwrap();
    assert_eq!(p.get("end_chamfer").cloned(), Some(typed("-1 mm")), "{p}");
    let refused = session.scratch().execute(COMMAND, &p).unwrap_err().to_string();
    assert!(refused.contains("`end_chamfer` must be from 0"), "{refused}");
}

/// A reopened spline whose start face is not shown sends the start it has saved while its own round face is
/// picked, so OK keeps the end of its teeth; a start picked in the row wins, and another round face drops it.
#[test]
fn a_reopened_spline_keeps_its_saved_start_until_another_face_is_picked() {
    let (session, sel, start) = picked_shaft();
    let doc = &session.doc;
    let shaft = Picks { shaft: Some(&sel), ..Picks::default() };
    // The saved start is on the shaft's bottom end, far from the pick near its top.
    let bottom = Vec3::new(5.0, 0.0, 0.0);
    let mut s = Spline::new(doc);
    s.kind = 1;
    s.cutter = false;
    s.start_optional = [true, false];
    s.saved_start = [Some((sel.clone(), bottom)), None];
    s.sync_picks(&session, shaft);
    s.fit = true;
    assert!(s.saved_start[0].is_some(), "the face reopened with is still picked");
    let p = s.command_params(doc, shaft).unwrap();
    assert_eq!(p.get("shaft_start"), Some(&pt(bottom)), "{p}");
    // The command keeps the start at the bottom, not at the end nearest the pick.
    let mut scratch = session.scratch();
    scratch.execute(COMMAND, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    match scratch.doc.features.last().map(|f| &f.kind) {
        Some(FeatureKind::ShaftSpline { shaft_start: Some(at), .. }) => assert_eq!(pt(at.0), pt(bottom)),
        other => panic!("no Shaft Spline with a start: {other:?}"),
    }
    // A start picked in the row wins.
    let p = s.command_params(doc, Picks { shaft_start: Some(&start), ..shaft }).unwrap();
    assert_eq!(p.get("shaft_start"), Some(&pt(Vec3::new(5.0, 0.0, 60.0))), "{p}");
    // Another round face picked: the saved start is not sent, and the dialog drops it.
    let other = Sel::Face { body: "Shaft".into(), index: 0, point: Vec3::new(-15.0, 0.0, 30.0) };
    let elsewhere = Picks { shaft: Some(&other), ..Picks::default() };
    s.fit = false;
    let p = s.command_params(doc, elsewhere).unwrap();
    assert!(p.get("shaft_start").is_none(), "{p}");
    s.sync_picks(&session, elsewhere);
    assert!(s.saved_start[0].is_none());
}

/// A value the engine refuses, left as it is, is not worked out again: the dialog asks for another frame only
/// while the values differ from the ones last worked out.
#[test]
fn a_refused_value_left_as_it_is_asks_for_no_more_frames() {
    let doc = Document::default();
    let picks = Picks::default();
    assert!(Spline::new(&doc).stale(&doc, picks), "nothing worked out yet");
    let mut s = Spline::new(&doc);
    // Hub teeth that reach into the shaft's root fillets.
    s.set("hub_addendum", "0.85");
    s.refresh(&doc, picks);
    assert!(s.worked.as_ref().is_some_and(|w| w.result.is_err()), "refused");
    assert!(!s.stale(&doc, picks), "nothing changed since it was refused");
    s.set("hub_addendum", "0.9");
    assert!(s.stale(&doc, picks), "a changed value is worked out again");
}

/// The whole view draws each circle whole, the largest too: its first and last points meet.
#[test]
fn the_whole_view_draws_every_circle_whole() {
    let doc = Document::default();
    let mut s = Spline::new(&doc);
    s.kind = 1;
    s.make = 2;
    let w = s.work(&doc);
    let Ok((_, preview)) = &w.result else { panic!("{:?}", w.result) };
    let rect = Rect::from_min_size(pos2(20.0, 30.0), vec2(240.0, 180.0));
    let at = place(rect, preview, View::Whole);
    assert!(preview.circles.iter().any(|(c, _)| *c == SplineCircle::HubOutside), "the hub's outside is drawn");
    for (which, r) in &preview.circles {
        let points = circle_points(&at, *r, rect);
        let ends = points.first().copied().zip(points.last().copied());
        assert!(ends.is_some_and(|(a, b)| a.distance(b) < 0.01), "{which:?}: {ends:?}");
    }
}

/// The size that fits a picked shaft is the kernel's: the module that puts the tips of the teeth on the shaft's
/// circle. A diameter that is no size makes none.
#[test]
fn the_size_that_fits_a_shaft_is_the_kernels_and_a_diameter_that_is_no_size_makes_none() {
    let doc = Document::default();
    let s = Spline::new(&doc);
    // 20 teeth and an addendum of 0.5: 30 mm across over 21 modules.
    let module = s.size_fitting(&doc, 0, 30.0);
    assert_eq!(module, solidcraft_engine::doc::shaft_spline::module_for_major_radius(20.0, 0.5, 0.0, 15.0));
    assert!(module.is_some_and(|m| (m - 30.0 / 21.0).abs() < 1e-12), "{module:?}");
    for diameter in [0.0, -30.0, f64::NAN, f64::INFINITY] {
        assert_eq!(s.size_fitting(&doc, 0, diameter), None, "{diameter}");
    }
}

/// The numbers that size the teeth are on top; the rest of the tooth's form and the chamfers fold away in groups of
/// their own. The Chamfers group starts folded and holds each chamfer of a part cut or made: its size, its Type,
/// and the row the type adds.
#[test]
fn the_chamfers_fold_in_a_group_of_their_own_with_a_type_and_its_row() {
    let mut h = open();
    for key in ["length", "module", "pressure_angle", "addendum", "thinning"] {
        assert!(handle_at(&format!("dialog:shaft_spline:{key}")).is_some(), "{key}");
    }
    // A shaft alone: its End chamfer, folded away until the group is opened.
    assert!(handle_at("dialog:shaft_spline:end_chamfer").is_none(), "the Chamfers group starts folded");
    open_chamfers(&mut h);
    let shown = |key: &str| handle_at(&format!("dialog:shaft_spline:{key}")).is_some();
    assert!(shown("end_chamfer") && shown("end_chamfer_type"));
    assert!(!shown("end_chamfer_distance2") && !shown("end_chamfer_angle"), "Equal Distance adds no row");
    assert!(!shown("entry_chamfer") && !shown("entry_chamfer_type"), "no hub, no entry chamfer");
    spline(&mut h).end_chamfer_type = 1;
    h.frames(2);
    assert!(shown("end_chamfer_distance2") && !shown("end_chamfer_angle"), "Two Distances: a second distance");
    spline(&mut h).end_chamfer_type = 2;
    h.frames(2);
    assert!(!shown("end_chamfer_distance2") && shown("end_chamfer_angle"), "Distance and Angle: an angle");
    // The shaft and its hub: both chamfers, each with its own Type.
    spline(&mut h).make = 2;
    spline(&mut h).entry_chamfer_type = 1;
    h.frames(2);
    assert!(shown("end_chamfer_angle") && shown("entry_chamfer") && shown("entry_chamfer_type") && shown("entry_chamfer_distance2"));
    // The angle starts where the Chamfer tool's does.
    assert_eq!(spline(&mut h).end_chamfer_angle, "45 deg");
}

/// Each chamfer sends the row its Type adds, and only with the chamfer itself: a second distance for Two
/// Distances, an angle for Distance and Angle, nothing more for Equal Distance. A row that is not a value is
/// refused naming its key while it is sent.
#[test]
fn each_chamfer_type_sends_its_own_row_and_only_with_its_chamfer() {
    let doc = Document::default();
    let length = |text: &str| json!(crate::length_input::expression(&doc, text, ValueKind::Length));
    let angle = |text: &str| json!(crate::length_input::expression(&doc, text, ValueKind::Angle));
    let mut s = Spline::new(&doc);
    s.kind = 1;
    s.make = 2;
    s.end_chamfer = "0.5 mm".into();
    s.entry_chamfer = "0.4 mm".into();
    s.end_chamfer_distance2 = "0.4 mm".into();
    s.end_chamfer_angle = "30 deg".into();
    s.entry_chamfer_distance2 = "0.3 mm".into();
    s.entry_chamfer_angle = "40 deg".into();
    let keys = ["end_chamfer_distance2", "end_chamfer_angle", "entry_chamfer_distance2", "entry_chamfer_angle"];
    for (kind, sent) in [
        (0, [None, None, None, None]),
        (1, [Some(length("0.4 mm")), None, Some(length("0.3 mm")), None]),
        (2, [None, Some(angle("30 deg")), None, Some(angle("40 deg"))]),
    ] {
        s.end_chamfer_type = kind;
        s.entry_chamfer_type = kind;
        let p = s.command_params(&doc, Picks::default()).unwrap_or_else(|b| panic!("type {kind}: {b:?}"));
        for (key, want) in keys.iter().zip(&sent) {
            assert_eq!(p.get(*key), want.as_ref(), "type {kind}: {key}: {p}");
        }
        assert_eq!((p.get("end_chamfer").cloned(), p.get("entry_chamfer").cloned()), (Some(length("0.5 mm")), Some(length("0.4 mm"))), "{p}");
    }
    // No chamfer, no row of its type: a plain 0, or a part not made.
    s.end_chamfer_type = 1;
    s.entry_chamfer_type = 2;
    s.end_chamfer = "0".into();
    s.make = 0;
    let p = s.command_params(&doc, Picks::default()).unwrap();
    assert!(keys.iter().all(|k| p.get(*k).is_none()), "{p}");
    // A row that is not a value is refused with its key, only while it is sent.
    let key = |r: Result<Value, Bad>| r.err().and_then(|b| b.key);
    s.end_chamfer = "0.5 mm".into();
    s.end_chamfer_distance2 = "far".into();
    assert_eq!(key(s.command_params(&doc, Picks::default())).as_deref(), Some("end_chamfer_distance2"));
    s.end_chamfer_type = 0;
    assert!(s.command_params(&doc, Picks::default()).is_ok(), "Equal Distance does not send the second distance");
    s.make = 2;
    s.entry_chamfer_angle = String::new();
    assert_eq!(key(s.command_params(&doc, Picks::default())).as_deref(), Some("entry_chamfer_angle"));
}

/// A saved spline opens again with each chamfer's Type the one its numbers say, its rows as saved, and sends
/// the same chamfers again.
#[test]
fn a_saved_spline_opens_again_with_each_chamfer_type() {
    for kind in 0..3 {
        let mut h = Harness::new();
        let doc = &h.app.session.doc;
        let mut s = Spline::new(doc);
        s.kind = 1;
        s.make = 2;
        s.end_chamfer = "0.5 mm".into();
        s.entry_chamfer = "0.4 mm".into();
        s.end_chamfer_distance2 = "0.4 mm".into();
        s.end_chamfer_angle = "30 deg".into();
        s.entry_chamfer_distance2 = "0.3 mm".into();
        s.entry_chamfer_angle = "40 deg".into();
        s.end_chamfer_type = kind;
        s.entry_chamfer_type = kind;
        let p = s.command_params(doc, Picks::default()).unwrap_or_else(|b| panic!("type {kind}: {b:?}"));
        h.app.run(COMMAND, p.clone()).unwrap_or_else(|e| panic!("type {kind}: {p}: {e}"));
        let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
        let reopened = crate::dialogs::for_feature(&h.app, id, None).unwrap();
        let Kind::ShaftSpline(k) = &reopened.kind else { panic!("not a Shaft Spline dialog: {:?}", reopened.kind) };
        assert_eq!((k.end_chamfer_type, k.entry_chamfer_type), (kind, kind), "type {kind}");
        let again = k.command_params(&h.app.session.doc, Picks::default()).unwrap_or_else(|b| panic!("type {kind}: {b:?}"));
        for key in ["end_chamfer", "entry_chamfer", "end_chamfer_distance2", "end_chamfer_angle", "entry_chamfer_distance2", "entry_chamfer_angle"] {
            assert_eq!(again.get(key), p.get(key), "type {kind}: {key}: {again} vs {p}");
        }
    }
}
