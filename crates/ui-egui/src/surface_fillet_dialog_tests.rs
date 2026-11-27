//! The Surface Fillet and Surface Chamfer dialogs (#769): a point on each of two surface bodies,
//! the radius or the chamfer's distances or angle, Trim and Extend become `surface.fillet` or
//! `surface.chamfer` with the same stored fields the command makes; editing either feature
//! reopens its dialog with the stored values and picks; what the command refuses shows in the
//! dialog in its words and leaves the design as it was.
use crate::dialogs::Kind;
use crate::dialogs_surface::Surf;
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;

/// Two 20 x 20 sheets crossing at right angles along the X axis: A in the XY plane, B in XZ.
fn crossing(h: &mut Harness) {
    for (plane, name) in [("XY", "A"), ("XZ", "B")] {
        let sk = format!("{name} sketch");
        h.app.run("sketch.create", json!({"plane": plane, "name": sk})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("surface.patch", json!({"sketch": sk, "body_name": name})).unwrap();
    }
    h.frames(2);
}

/// A pick on the one face of a sheet at `p`.
fn face(body: &str, p: [f64; 3]) -> Sel {
    Sel::Face { body: body.into(), index: 0, point: Vec3::new(p[0], p[1], p[2]) }
}

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget": "OK"})).unwrap();
    // An edit is applied off the UI thread: give it the frames it takes.
    for _ in 0..400 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "{:?} {:?} {:?}", h.app.preview.error, h.app.dialog.as_ref().map(|d| (&d.error, &d.inputs)), h.app.session.log);
}

/// The preview has run (or failed).
fn settle(h: &mut Harness) {
    let began = std::time::Instant::now();
    h.frames(3);
    while h.app.preview.busy && began.elapsed() < std::time::Duration::from_secs(300) {
        h.frame();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    h.frames(2);
}

fn area(h: &Harness, body: &str) -> f64 {
    solidcraft_engine::kernel::measure(&h.app.session.world_state().unwrap().body(body).unwrap().body).unwrap().area
}

/// The last feature's stored fields, as saved.
fn last_kind(h: &Harness) -> Value {
    serde_json::to_value(&h.app.session.doc.features.last().unwrap().kind).unwrap()
}

/// The stored fields `command` makes from `params` on the crossing sheets, run as a script would.
fn by_command(command: &str, params: Value) -> Value {
    let mut h = Harness::new();
    crossing(&mut h);
    h.app.run(command, params).unwrap();
    last_kind(&h)
}

/// Start `command`'s dialog with A picked at (0, 5, 0) and B at (0, 0, 5).
fn start_on_crossing(h: &mut Harness, command: &str) {
    h.step(&json!({"call": "ui.start", "params": {"command": command}})).unwrap();
    h.frames(3);
    let d = h.app.dialog.as_mut().unwrap();
    assert_eq!(d.inputs.len(), 2, "{:?}", d.inputs);
    d.inputs[0].items = vec![face("A", [0.0, 5.0, 0.0])];
    d.inputs[1].items = vec![face("B", [0.0, 0.0, 5.0])];
}

/// Surface Fillet: OK builds the feature `surface.fillet` builds from the same values (the
/// crossing sheets rounded by 2: two flats of 20 x 8 and a quarter cylinder of radius 2 and
/// length 20, one body); editing it reopens the dialog with the radius, Trim and both picks, and
/// Trim None leaves both sheets whole beside a round alone.
#[test]
fn fillet_dialog_builds_what_the_command_builds_and_comes_back_in_an_edit() {
    let mut h = Harness::new();
    crossing(&mut h);
    start_on_crossing(&mut h, "surface.fillet");
    assert!(h.widget("Radius").is_some() && h.widget("Trim").is_some(), "the radius and trim rows");
    let Some(Kind::Surface(Surf::Fillet { radius, trim: 0, extend: false })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else {
        panic!("{:?}", h.app.dialog.as_ref().map(|d| &d.kind))
    };
    assert_eq!(radius, "2 mm", "the default radius");
    *radius = "2 mm".into();
    ok(&mut h);
    let want =
        by_command("surface.fillet", json!({"a": {"body": "A", "point": [0, 5, 0]}, "b": {"body": "B", "point": [0, 0, 5]}, "radius": "2 mm"}));
    assert_eq!(last_kind(&h), want, "the dialog stores what the command stores");
    let flats_and_round = 2.0 * 20.0 * 8.0 + std::f64::consts::FRAC_PI_2 * 2.0 * 20.0;
    assert!((area(&h, "A") - flats_and_round).abs() < 1e-3 * flats_and_round, "{} vs {flats_and_round}", area(&h, "A"));
    assert!(h.app.session.world_state().unwrap().body("B").is_none(), "B is sewn into A");
    // Edit: the dialog comes back with what was stored; Trim None and radius 1.5 rebuild it
    // (both sheets whole, and a quarter cylinder of radius 1.5 and length 20 alone).
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_mut().unwrap();
    assert!(matches!(&d.kind, Kind::Surface(Surf::Fillet { radius, trim: 0, extend: false }) if radius == "2 mm"), "{:?}", d.kind);
    let picked: Vec<(String, Vec3)> = d
        .inputs
        .iter()
        .flat_map(|i| i.items.iter())
        .filter_map(|s| if let Sel::Face { body, point, .. } = s { Some((body.clone(), *point)) } else { None })
        .collect();
    assert_eq!(picked, [("A".to_string(), Vec3::new(0.0, 5.0, 0.0)), ("B".to_string(), Vec3::new(0.0, 0.0, 5.0))]);
    let Kind::Surface(Surf::Fillet { radius, trim, .. }) = &mut d.kind else { panic!() };
    *radius = "1.5 mm".into();
    *trim = 2;
    ok(&mut h);
    match &h.app.session.doc.find_feature(&name).unwrap().kind {
        FeatureKind::SurfaceFillet { radius, trim, .. } => {
            assert_eq!((radius.to_string().as_str(), *trim), ("1.5 mm", solidcraft_engine::doc::FilletTrim::None));
        }
        k => panic!("{k:?}"),
    }
    assert!((area(&h, "A") - 400.0).abs() < 1e-6 && (area(&h, "B") - 400.0).abs() < 1e-6, "both sheets whole");
    let st = h.app.session.world_state().unwrap();
    let round: Vec<_> = st.bodies.iter().filter(|b| b.name != "A" && b.name != "B").map(|b| b.name.clone()).collect();
    assert_eq!(round.len(), 1, "the round alone is a new body");
    let quarter = std::f64::consts::FRAC_PI_2 * 1.5 * 20.0;
    assert!((area(&h, &round[0]) - quarter).abs() < 1e-3 * quarter, "{} vs {quarter}", area(&h, &round[0]));
}

/// Surface Chamfer: Two Distances (2 along A, 3 along B) builds what `surface.chamfer` builds
/// (flats 20 x 8 and 20 x 7 and a bevel sqrt(2² + 3²) wide); an edit reopens it as Two Distances,
/// and Distance and Angle replaces the second distance by the angle (never both).
#[test]
fn chamfer_dialog_builds_what_the_command_builds_and_comes_back_in_an_edit() {
    let mut h = Harness::new();
    crossing(&mut h);
    start_on_crossing(&mut h, "surface.chamfer");
    assert!(h.widget("Chamfer Type").is_some() && h.widget("Distance").is_some());
    let Some(Kind::Surface(Surf::Chamfer { ctype, distance2, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!() };
    *ctype = 1;
    *distance2 = "3 mm".into();
    h.frames(2);
    assert!(h.widget("Distance 1").is_some() && h.widget("Distance 2").is_some() && h.widget("Angle").is_none());
    ok(&mut h);
    let want = by_command(
        "surface.chamfer",
        json!({"a": {"body": "A", "point": [0, 5, 0]}, "b": {"body": "B", "point": [0, 0, 5]}, "distance": "2 mm", "distance2": "3 mm"}),
    );
    assert_eq!(last_kind(&h), want, "the dialog stores what the command stores");
    let expected = 20.0 * 8.0 + 20.0 * 7.0 + 13.0_f64.sqrt() * 20.0;
    assert!((area(&h, "A") - expected).abs() < 1e-3 * expected, "{} vs {expected}", area(&h, "A"));
    let name = h.app.session.doc.features.last().unwrap().name.clone();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.frames(8);
    let d = h.app.dialog.as_mut().unwrap();
    assert!(
        matches!(&d.kind, Kind::Surface(Surf::Chamfer { distance, ctype: 1, distance2, flip: false, trim: 0, extend: false, .. })
            if distance == "2 mm" && distance2 == "3 mm"),
        "{:?}",
        d.kind
    );
    assert_eq!(d.inputs.iter().map(|i| i.items.len()).collect::<Vec<_>>(), [1, 1]);
    let Kind::Surface(Surf::Chamfer { ctype, angle, flip, .. }) = &mut d.kind else { panic!() };
    *ctype = 2;
    *angle = "30 deg".into();
    *flip = true;
    ok(&mut h);
    match &h.app.session.doc.find_feature(&name).unwrap().kind {
        FeatureKind::SurfaceChamfer { distance2, angle, flip, .. } => {
            assert!(distance2.is_none(), "the angle replaces the second distance");
            assert_eq!(angle.as_ref().map(|a| a.to_string()), Some("30 deg".to_string()));
            assert!(*flip);
        }
        k => panic!("{k:?}"),
    }
    assert_eq!(h.app.session.world_state().unwrap().body("A").unwrap().body.face_count(), 3, "two flats and the bevel");
}

/// What the command refuses shows in the dialog, in the command's words, while the values are
/// being chosen; OK then changes nothing. Without both picks the dialog says which is missing.
#[test]
fn a_refused_fillet_shows_the_commands_message_and_leaves_the_design_unchanged() {
    let mut h = Harness::new();
    crossing(&mut h);
    let before = h.app.session.doc.to_json();
    h.step(&json!({"call": "ui.start", "params": {"command": "surface.fillet"}})).unwrap();
    h.frames(3);
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[0].items = vec![face("A", [0.0, 5.0, 0.0])];
    }
    let missing = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(missing.contains("second surface"), "{missing}");
    // A ball far larger than the sheets leave room for.
    let refused = {
        let mut h2 = Harness::new();
        crossing(&mut h2);
        h2.app
            .session
            .execute("surface.fillet", &json!({"a": {"body": "A", "point": [0, 5, 0]}, "b": {"body": "B", "point": [0, 0, 5]}, "radius": "500 mm"}))
            .unwrap_err()
            .to_string()
    };
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.inputs[1].items = vec![face("B", [0.0, 0.0, 5.0])];
        let Kind::Surface(Surf::Fillet { radius, .. }) = &mut d.kind else { panic!() };
        *radius = "500 mm".into();
    }
    settle(&mut h);
    let shown = h.app.preview.error.clone().or_else(|| h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    let shown = shown.unwrap_or_default();
    let core = refused.split(": ").last().unwrap_or(&refused).trim();
    assert!(!core.is_empty() && shown.contains(core), "the dialog shows the command's refusal: {shown:?} vs {refused:?}");
    // OK does not build it: the dialog stays with the message, and the design is as it was.
    h.step(&json!({"widget": "OK"})).unwrap();
    for _ in 0..200 {
        if h.app.applying.is_none() {
            break;
        }
        h.frames(1);
    }
    h.frames(2);
    assert_eq!(h.app.session.doc.to_json(), before, "a refusal leaves the design unchanged");
    assert!(h.app.session.world_state().unwrap().body("B").is_some());
}
