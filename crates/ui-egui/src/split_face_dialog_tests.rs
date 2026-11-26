//! The Split Face dialog with 3D curves as its tool (#22414): the 3D curves picked (each one
//! splits) and an optional direction become `solid.split_face`'s `wires` and `direction`, the
//! feature it makes is the one the command makes, Edit Feature brings the picks and a saved
//! direction back, and a split the command refuses leaves the design as it was. The planar
//! sketch, plane and face tools are covered where they always were (`preselect_tests`).
//!
//! The sheet: a circular arc 40 mm wide with an 8 mm rise (radius 29 mm, centre (20, -21)) in the
//! XY plane, extruded 30 mm along Z, as in the engine's own tests (`split_face_tests`). Its area
//! is 29 · 2 asin(20/29) · 30 = 1324.17 mm²; face areas come from meshes, so they are checked to
//! 0.1 %.
use crate::dialogs::{Dialog, Kind, apply_commands};
use crate::dialogs_surface::Surf;
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;

const R: f64 = 29.0;

/// Half the arc's angle, asin(20/29).
fn half() -> f64 {
    (20.0 / R).asin()
}

/// The whole sheet's area, 29 · 2 asin(20/29) · 30.
fn whole() -> f64 {
    R * 2.0 * half() * 30.0
}

/// The strip of the sheet past x = 30 (or short of x = 10): 29 · (asin(20/29) − asin(10/29)) · 30.
fn side() -> f64 {
    R * (half() - (10.0 / R).asin()) * 30.0
}

/// The sheet's height at `x`: y = sqrt(29² − (x − 20)²) − 21.
fn height(x: f64) -> f64 {
    (R * R - (x - 20.0) * (x - 20.0)).sqrt() - 21.0
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * b.abs()
}

/// A point on the top of the sheet.
const ON_SHEET: Vec3 = Vec3 { x: 20.0, y: 8.0, z: 15.0 };

/// The sheet, and the sketch `Marks` (3D Sketch on) holding the curves `draw` makes, finished.
/// Returns the sketch's id and the ids of its 3D curves, in the order drawn.
fn sheet_with(h: &mut Harness, draw: &[(&str, Value)]) -> (u64, Vec<String>) {
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Arc"})).unwrap();
    h.app.run("sketch.arc.three_point", json!({"start": [0, 0], "end": [40, 0], "through": [20, 8]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("surface.extrude", json!({"sketch": "Arc", "curves": ["a1"], "distance": 30, "body_name": "Sheet"})).unwrap();
    marks(h, draw)
}

/// The sketch `Marks` (3D Sketch on) with the curves `draw` makes, finished: its id and the ids of
/// its 3D curves, in the order drawn.
fn marks(h: &mut Harness, draw: &[(&str, Value)]) -> (u64, Vec<String>) {
    let sketch = h.app.run("sketch.create", json!({"plane": "XY", "name": "Marks"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.options", json!({"sketch_3d": true})).unwrap();
    for (command, params) in draw {
        h.app.run(command, params.clone()).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    (sketch, sketch_curves(h, sketch).0)
}

/// The ids of a sketch's 3D curves and of its planar curves.
fn sketch_curves(h: &Harness, sketch: u64) -> (Vec<String>, Vec<String>) {
    let st = crate::open_meshing::session_world(&h.app.session);
    let ss = st.sketch(sketch).unwrap();
    (ss.sketch.wires.iter().map(|w| w.id.clone()).collect(), ss.sketch.curves.iter().map(|c| c.id.clone()).collect())
}

/// A straight 3D line lying on the sheet along its length at `x`, from edge to edge.
fn line_on_sheet(x: f64) -> (&'static str, Value) {
    ("sketch.line3d", json!({"points": [[x, height(x), 0.0], [x, height(x), 30.0]]}))
}

/// The areas of the faces of `body`, smallest first.
fn areas(h: &mut Harness, body: &str) -> Vec<f64> {
    let f = h.app.run("model.faces", json!({"body": body})).unwrap();
    let mut a: Vec<f64> = f["faces"].as_array().unwrap().iter().map(|x| x["area"].as_f64().unwrap()).collect();
    a.sort_by(f64::total_cmp);
    a
}

/// 3D curve `id` of `sketch` as a pick.
fn wire(sketch: u64, id: &str) -> Sel {
    Sel::ExternalSketchCurve { sketch, id: id.to_string() }
}

/// The Split Face dialog on the face at `face` with `tool` picked (and `direction`, if any).
fn split_dialog(h: &Harness, face: Vec3, tool: Vec<Sel>, direction: Option<Sel>) -> Dialog {
    let mut d = Dialog::for_command(&h.app, "solid.split_face").unwrap();
    d.inputs[0].items = vec![crate::dialogs::face_sel(&h.app.session, face).expect("a face at the point")];
    d.inputs[1].items = tool;
    d.inputs[2].items = direction.into_iter().collect();
    d
}

/// Press OK and wait until the dialog has closed.
fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget":"OK"})).unwrap();
    for _ in 0..200 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "{:?} {:?}", h.app.preview.error, h.app.dialog.as_ref().map(|d| (&d.error, &d.inputs)));
}

/// Open the Edit Feature dialog of feature `name`.
fn edit(h: &mut Harness, name: &str) {
    h.step(&json!({"call":"ui.editFeature","params":{"feature":name}})).unwrap();
    h.frames(4);
}

/// The kind of the feature called `name`.
fn kind(h: &Harness, name: &str) -> FeatureKind {
    h.app.session.doc.find_feature(name).unwrap().kind.clone()
}

/// The last feature: its name and kind.
fn last(h: &Harness) -> (String, FeatureKind) {
    let f = h.app.session.doc.features.last().unwrap();
    (f.name.clone(), f.kind.clone())
}

/// A 3D line on the sheet picked as the tool: the dialog sends the line by id, with no `loops`
/// and no direction, and makes the feature the command makes from the same values. The line,
/// swept along the sheet's normal there (the default), cuts it along itself: the piece past
/// x = 30 spans asin(10/29) to asin(20/29) of the arc, 29 · (asin(20/29) − asin(10/29)) · 30 =
/// 355.82 mm², and the rest is 968.35 mm².
#[test]
fn a_3d_curve_picked_as_the_tool_makes_the_commands_split() {
    let mut h = Harness::new();
    let (sketch, wires) = sheet_with(&mut h, &[line_on_sheet(30.0)]);
    let [line] = wires.as_slice() else { panic!("{wires:?}") };
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, line)], None);
    let commands = apply_commands(&h.app, &d).unwrap();
    assert_eq!(commands.len(), 1);
    let (command, p) = &commands[0];
    assert_eq!(command, "solid.split_face");
    assert_eq!((&p["sketch"], &p["wires"], &p["body"]), (&json!(sketch), &json!([line]), &json!("Sheet")), "{p}");
    assert!(p.get("loops").is_none() && p.get("direction").is_none(), "{p}");
    h.app.dialog = Some(d);
    ok(&mut h);
    let (_, made) = last(&h);
    let a = areas(&mut h, "Sheet");
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(close(a[0], side()) && close(a[1], whole() - side()), "{a:?} against {} and {}", side(), whole() - side());

    // The command from the command line, on the same design.
    let mut c = Harness::new();
    let (_, wires) = sheet_with(&mut c, &[line_on_sheet(30.0)]);
    c.app.run("solid.split_face", json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks", "wires": wires})).unwrap();
    assert_eq!(made, last(&c).1, "the dialog stores what the command stores");
}

/// Several 3D curves of one sketch split at once, each one picked. Lines along the sheet at
/// x = 10 and x = 30 leave three strips: two of 355.82 mm² and the middle one,
/// 29 · 2 asin(10/29) · 30 = 612.53 mm².
#[test]
fn several_3d_curves_picked_split_with_each_one() {
    let mut h = Harness::new();
    let (sketch, wires) = sheet_with(&mut h, &[line_on_sheet(30.0), line_on_sheet(10.0)]);
    let [first, second] = wires.as_slice() else { panic!("{wires:?}") };
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, first), wire(sketch, second)], None);
    let (_, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    assert_eq!(p["wires"], json!([first, second]), "{p}");
    h.app.dialog = Some(d);
    ok(&mut h);
    let a = areas(&mut h, "Sheet");
    let middle = R * 2.0 * (10.0 / R).asin() * 30.0;
    assert_eq!(a.len(), 3, "{a:?}");
    assert!(close(a[0], side()) && close(a[1], side()) && close(a[2], middle), "{a:?} against {}, {} and {middle}", side(), side());
    assert!(close(a.iter().sum(), whole()));
}

/// A direction picked (the Y axis) is sent, and Edit Feature brings the curve and the axis back:
/// OK on the unchanged dialog leaves the feature as it was. A control point spline across the
/// sheet (control points (0, 0, 10), (20, 16, 0), (40, 0, 10): z = 5 + (x − 20)²/80 seen along
/// Y), swept along +Y, leaves on the z = 0 side
/// 29 · (10 asin(20/29) + (29² asin(20/29) − 20 · 21) / 80) = 300.45 mm² (the mesh of a face
/// trimmed by a free-form edge sags more: checked to 0.2 %).
#[test]
fn a_picked_direction_is_sent_and_comes_back_in_an_edit() {
    let mut h = Harness::new();
    let (sketch, wires) = sheet_with(&mut h, &[("sketch.spline3d.control_point", json!({"points": [[0, 0, 10], [20, 16, 0], [40, 0, 10]]}))]);
    let [spline] = wires.as_slice() else { panic!("{wires:?}") };
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, spline)], Some(Sel::Axis { name: "Y".into() }));
    let (_, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    assert_eq!((&p["wires"], &p["direction"]), (&json!([spline]), &json!([0.0, 1.0, 0.0])), "{p}");
    h.app.dialog = Some(d);
    ok(&mut h);
    let (name, made) = last(&h);
    let FeatureKind::SplitFace { wires, direction, .. } = &made else { panic!("{made:?}") };
    assert_eq!((wires.clone(), direction.map(|d| d.0)), (vec![spline.clone()], Some(Vec3::Y)));
    let a = areas(&mut h, "Sheet");
    let near = R * (10.0 * half() + (R * R * half() - 20.0 * 21.0) / 80.0);
    assert_eq!(a.len(), 2, "{a:?}");
    assert!((a[0] - near).abs() <= 2e-3 * near && (a[1] - (whole() - near)).abs() <= 2e-3 * (whole() - near), "{a:?} against {near}");

    edit(&mut h, &name);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.kind, Kind::Surface(Surf::SplitFace { direction: None, .. })), "{:?}", d.kind);
    assert_eq!(d.inputs[1].items, vec![wire(sketch, spline)]);
    assert_eq!(d.inputs[2].items, vec![Sel::Axis { name: "Y".into() }]);
    assert!(h.widget("Keep the saved direction").is_none(), "an axis is shown as the pick");
    ok(&mut h);
    assert_eq!(kind(&h, &name), made, "OK on the unchanged edit keeps the feature");
}

/// A saved direction along no axis (made by the command) comes back as saved: the dialog shows
/// it and sends it unchanged while "Keep the saved direction" is ticked, and unticked the split
/// goes back to the default direction. On a 40 × 30 × 20 box a 3D line across the top at y = 15
/// splits the top into two halves of 600 mm² either way (the box's 30 × 20 ends are 600 mm² too).
#[test]
fn a_saved_direction_is_kept_until_unticked() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"})).unwrap();
    let sixes = |h: &mut Harness| areas(h, "B").into_iter().filter(|a| close(*a, 600.0)).count();
    assert_eq!(sixes(&mut h), 2, "the two ends");
    marks(&mut h, &[("sketch.line3d", json!({"points": [[5, 15, 20], [35, 15, 20]]}))]);
    h.app
        .run("solid.split_face", json!({"faces": [[20, 10, 20]], "body": "B", "sketch": "Marks", "direction": [0.0, 0.6, 0.8], "name": "Slanted"}))
        .unwrap();
    let made = kind(&h, "Slanted");
    assert_eq!(sixes(&mut h), 4, "the ends and the two halves of the top");

    edit(&mut h, "Slanted");
    let d = h.app.dialog.as_ref().unwrap();
    assert!(
        matches!(d.kind, Kind::Surface(Surf::SplitFace { direction: Some(v), keep_direction: true }) if v == Vec3::new(0.0, 0.6, 0.8)),
        "{:?}",
        d.kind
    );
    assert!(d.inputs[2].items.is_empty());
    assert!(h.widget("Keep the saved direction").is_some());
    ok(&mut h);
    assert_eq!(kind(&h, "Slanted"), made, "kept as saved");

    edit(&mut h, "Slanted");
    let d = h.app.dialog.as_mut().unwrap();
    let Kind::Surface(Surf::SplitFace { keep_direction, .. }) = &mut d.kind else { panic!("{:?}", d.kind) };
    *keep_direction = false;
    ok(&mut h);
    let FeatureKind::SplitFace { direction, .. } = kind(&h, "Slanted") else { panic!() };
    assert_eq!(direction, None, "unticked: the default direction");
    assert_eq!(sixes(&mut h), 4);
}

/// A split saved with every 3D curve of a sketch that has no planar curves (none named, as the
/// command makes it) comes back with those curves picked, and OK keeps it naming none.
#[test]
fn a_split_with_every_3d_curve_stays_so_in_an_edit() {
    let mut h = Harness::new();
    let (sketch, wires) = sheet_with(&mut h, &[line_on_sheet(30.0)]);
    h.app.run("solid.split_face", json!({"faces": [[20, 8, 15]], "body": "Sheet", "sketch": "Marks", "name": "Every"})).unwrap();
    let made = kind(&h, "Every");
    assert!(matches!(&made, FeatureKind::SplitFace { wires, .. } if wires.is_empty()), "{made:?}");
    edit(&mut h, "Every");
    let picked: Vec<Sel> = wires.iter().map(|id| wire(sketch, id)).collect();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items, picked);
    ok(&mut h);
    assert_eq!(kind(&h, "Every"), made);
    assert_eq!(areas(&mut h, "Sheet").len(), 2);
}

/// What the dialog refuses itself, in the user's words: a plane picked with a direction (a plane
/// cuts along itself), a direction that is no axis, line or edge, 3D curves picked with a curve of
/// a planar sketch, and a plane among curves.
#[test]
fn the_dialog_refuses_tools_it_cannot_send() {
    let mut h = Harness::new();
    let (sketch, wires) = sheet_with(&mut h, &[line_on_sheet(30.0), ("sketch.line", json!({"points": [[0, -5], [40, -5]]}))]);
    let [line] = wires.as_slice() else { panic!("{wires:?}") };
    let planar = sketch_curves(&h, sketch).1;
    let [flat] = planar.as_slice() else { panic!("{planar:?}") };
    let refusal = |d: &Dialog| apply_commands(&h.app, d).unwrap_err();

    let d = split_dialog(&h, ON_SHEET, vec![Sel::Plane { name: "YZ".into() }], Some(Sel::Axis { name: "Y".into() }));
    assert_eq!(refusal(&d), h.app.tr("ui.split_face_direction_needs_curves"));
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, line)], Some(Sel::Plane { name: "XY".into() }));
    assert_eq!(refusal(&d), h.app.tr("ui.split_face_direction_pick"));
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, line), wire(sketch, flat)], None);
    assert_eq!(refusal(&d), h.app.tr("ui.split_face_mixed_curves"));
    // A plane among curves never reaches OK from the view (the last pick replaces it), but is
    // refused if it does.
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, line), Sel::Plane { name: "YZ".into() }], None);
    assert_eq!(refusal(&d), h.app.tr("ui.split_face_one_tool"));
    // The planar curve alone is the planar sketch tool, as before: the sketch, its loops joined.
    let d = split_dialog(&h, ON_SHEET, vec![wire(sketch, flat)], None);
    let (_, p) = apply_commands(&h.app, &d).unwrap().remove(0);
    assert_eq!((&p["sketch"], &p["loops"]), (&json!(sketch), &json!(true)), "{p}");
    assert!(p.get("wires").is_none(), "{p}");
}

/// The tool keeps what one split can take: a plane or a face picked replaces the other picks, and
/// a curve picked replaces a plane, a face or curves of another sketch.
#[test]
fn a_pick_replaces_tool_picks_that_cannot_go_with_it() {
    let plane = Sel::Plane { name: "XY".into() };
    let tidy = |mut picks: Vec<Sel>| {
        crate::dialogs_surface::tidy_split_tool(&mut picks);
        picks
    };
    assert_eq!(tidy(vec![wire(3, "w1"), wire(3, "w2")]), vec![wire(3, "w1"), wire(3, "w2")]);
    assert_eq!(tidy(vec![wire(3, "w1"), plane.clone()]), vec![plane.clone()]);
    assert_eq!(tidy(vec![plane, wire(3, "w1")]), vec![wire(3, "w1")]);
    assert_eq!(tidy(vec![wire(3, "w1"), wire(5, "w1")]), vec![wire(5, "w1")]);
    assert_eq!(tidy(Vec::new()), Vec::new());
}

/// A split the command refuses leaves the design unchanged and says why in the dialog: on a box,
/// a 3D line across the top swept along Y lies in the top all along, so it cannot cut across it.
#[test]
fn a_refused_split_leaves_the_design_unchanged() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"})).unwrap();
    let (sketch, wires) = marks(&mut h, &[("sketch.line3d", json!({"points": [[5, 15, 20], [35, 15, 20]]}))]);
    let [line] = wires.as_slice() else { panic!("{wires:?}") };
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let d = split_dialog(&h, Vec3::new(20.0, 10.0, 20.0), vec![wire(sketch, line)], Some(Sel::Axis { name: "Y".into() }));
    h.app.dialog = Some(d);
    h.frames(4);
    // OK may be held back while the preview fails; pressed or not, nothing is made.
    let _ = h.step(&json!({"widget":"OK"}));
    for _ in 0..50 {
        h.frames(1);
    }
    let d = h.app.dialog.as_ref().expect("the dialog stays open");
    let said = d.error.clone().or_else(|| h.app.preview.error.clone()).unwrap_or_default();
    assert!(said.contains("tangent"), "the command's refusal is shown: {said:?}");
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
}
