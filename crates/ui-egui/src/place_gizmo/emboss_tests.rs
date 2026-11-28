//! The Emboss handles (#20386): the marker stands where the picked profiles are built on the model
//! and drags them across it as Offset X and Offset Y, the grip turns them as Angle, and the box
//! round the profiles is outlined where it is built. On a cylinder the marker is held to what OK
//! builds.

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::prelude::*;

use super::{angle_of, nearer};
use crate::dialogs::Kind;
use crate::scenario::Harness;

/// The middle of the plate's top: clear of the profile, and of the view compass over the plate's
/// far corner, which a press there would turn. The handles are pressed on the near side of it.
const ON_THE_PLATE: [f64; 3] = [40.0, 25.0, 10.0];
/// The radius of the cylinders below, and how far out a wall raised by the default depth of 1
/// reaches at least: the probes tell raised from not by it.
const RADIUS: f64 = 20.0;
const RAISED: f64 = 20.5;

/// An 80 × 50 × 10 plate seen from the top, with the handles on.
fn plate() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.box", json!({"length": 80, "width": 50, "height": 10})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

/// A 20 × 8 rectangle drawn on `plane`, from (10, 10) to (30, 18): its middle is at (20, 14).
fn badge(h: &mut Harness, plane: serde_json::Value) -> u64 {
    let sketch = h.app.run("sketch.create", json!({"plane": plane, "name": "Badge"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 18]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    sketch
}

/// The plate with that rectangle on a plane `height` above the origin, along the world's X and Y,
/// and Emboss open on it with the plate's top as its Target Face.
fn plate_emboss(height: f64) -> Harness {
    let mut h = plate();
    let sketch = badge(&mut h, json!({"origin": [0, 0, height], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}));
    emboss(&mut h, sketch, Some(ON_THE_PLATE));
    h
}

/// A cylinder of radius 20 about the world's Z, and a 12 × 6 rectangle on a plane `off` its wall
/// (0: touching it), Emboss open on it with the wall as its Target Face, picked `pick` degrees
/// round from the world's X. The sketch's x is the world's Y and its y the world's Z, it looks
/// along -X, and the rectangle's middle is at (8, 0) in it: 8 mm across the axis from the line of
/// the wall that faces the sketch. The view looks at the wall from between X and Y.
fn wall(off: f64, pick: f64) -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.cylinder", json!({"radius": RADIUS, "height": 60})).unwrap();
    let plane = json!({"origin": [RADIUS + off, 0, 30], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]});
    let sketch = h.app.run("sketch.create", json!({"plane": plane, "name": "Badge"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [2, -3], "p1": [14, 3]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.cam = h.app.cam.looking_from(Vec3::new(0.82, 0.57, 0.2));
    h.app.cam.target = Vec3::new(17.0, 9.0, 30.0);
    h.app.cam.distance = 150.;
    h.frames(3);
    emboss(&mut h, sketch, Some(on_wall(pick, 15.0)));
    h
}

/// The point of a wall of that radius about the world's Z, `degrees` round from the world's X at
/// the height `z`.
fn on_wall(degrees: f64, z: f64) -> [f64; 3] {
    [RADIUS * degrees.to_radians().cos(), RADIUS * degrees.to_radians().sin(), z]
}

/// How far from the world's Z the first body reaches, `degrees` round it at the height `z`.
fn reach_at(h: &Harness, degrees: f64, z: f64) -> f64 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let st = h.app.session.model.state();
    let body = &st.bodies.first().expect("a body").body;
    let hit = body.fire_ray(Vec3::new(100.0 * cos, 100.0 * sin, z), Vec3::new(-cos, -sin, 0.0)).unwrap().expect("the body is there");
    hit.point.x.hypot(hit.point.y)
}

/// Emboss open on the sketch's first profile; with `target`, that point of the model is clicked
/// as its Target Face.
fn emboss(h: &mut Harness, sketch: u64, target: Option<[f64; 3]>) {
    h.app.session.selection = vec![Sel::Profile { sketch, index: 0 }];
    h.app.start("solid.emboss");
    h.frames(2);
    if let Some(at) = target {
        h.call("ui.dialogInput", json!({"index": 1}));
        h.step(&json!({"click": {"world": at}})).unwrap();
        let d = h.app.dialog.as_ref().expect("Emboss is open");
        assert!(matches!(d.inputs.get(1).and_then(|i| i.items.first()), Some(Sel::Face { .. })), "the click picked the Target Face");
    }
    h.settle();
}

/// OK, which must build the Emboss and close the dialog.
fn ok(h: &mut Harness) {
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK built it and closed: {:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    h.step(&json!({"expect": {"errors": 0, "bodies": 1}})).unwrap();
}

/// Where the marker stands, and the way the surface faces there.
fn place(h: &Harness) -> (Vec3, Vec3) {
    let places = super::places(&h.app, h.app.dialog.as_ref().expect("Emboss is open"));
    let [p] = places.as_slice() else { panic!("one place, not {}", places.len()) };
    (p.base, p.n)
}

/// The footprint outlined at the marker.
fn outline(h: &Harness) -> Vec<Vec3> {
    let (base, n) = place(h);
    super::footprint(&h.app, h.app.dialog.as_ref().expect("Emboss is open"), base, n)
}

/// The open Emboss dialog's Offset X, Offset Y and Angle, as its fields hold them.
fn fields(h: &Harness) -> [String; 3] {
    let Some(Kind::Emboss { offset_x, offset_y, angle, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no Emboss open") };
    [offset_x.clone(), offset_y.clone(), angle.clone()]
}

fn set_fields(h: &mut Harness, values: [&str; 3]) {
    let Some(Kind::Emboss { offset_x, offset_y, angle, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("no Emboss open") };
    [*offset_x, *offset_y, *angle] = values.map(str::to_owned);
    h.frames(2);
}

fn number(text: &str, unit: &str) -> f64 {
    text.trim_end_matches(unit).trim().parse().unwrap_or_else(|_| panic!("'{text}' is not a number of {unit}"))
}

fn drag_marker(h: &mut Harness, to: [f64; 3]) {
    h.step(&json!({"drag": [{"handle": "emboss:position"}, {"world": to}], "steps": 12})).unwrap();
    h.settle();
}

/// Where the grip is on screen, and where it is once turned about the marker by `degrees`,
/// counter-clockwise as the view shows it.
fn grip_turned(h: &mut Harness, degrees: f64) -> ([f64; 2], [f64; 2]) {
    let ball = h.at(&json!({"handle": "emboss:position"})).unwrap();
    let grip = h.at(&json!({"handle": "emboss:rotate"})).unwrap();
    let (dx, dy) = (grip[0] - ball[0], grip[1] - ball[1]);
    assert!(dx.hypot(dy) > 20.0, "the grip stands off the marker: {grip:?} {ball:?}");
    // The screen's y runs down, so counter-clockwise there is clockwise in these numbers.
    let (s, c) = degrees.to_radians().sin_cos();
    (grip, [ball[0] + dx * c + dy * s, ball[1] - dx * s + dy * c])
}

fn turn_grip(h: &mut Harness, degrees: f64) {
    let (grip, to) = grip_turned(h, degrees);
    h.step(&json!({"drag": [grip, to], "steps": 16})).unwrap();
    h.settle();
}

/// The grip pressed and turned by `degrees`, and held there: the frame a turn under way paints.
/// Returns where the pointer is, to let go there.
fn hold_grip_turned(h: &mut Harness, degrees: f64) -> egui::Pos2 {
    let (grip, to) = grip_turned(h, degrees);
    let at = |t: f64| egui::pos2((grip[0] + (to[0] - grip[0]) * t) as f32, (grip[1] + (to[1] - grip[1]) * t) as f32);
    h.frame_with(vec![egui::Event::PointerMoved(at(0.0))]);
    h.frame_with(vec![egui::Event::PointerButton {
        pos: at(0.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    for step in 1..=8 {
        h.frame_with(vec![egui::Event::PointerMoved(at(f64::from(step) / 8.0))]);
    }
    h.frames(2);
    at(1.0)
}

fn let_go(h: &mut Harness, at: egui::Pos2) {
    h.frame_with(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    h.settle();
}

/// Is the footprint painted, as a filled shape whose corners are where the box round the profiles
/// is now?
fn outlined(h: &mut Harness) -> bool {
    let on_screen: Vec<[f64; 2]> = outline(h).iter().map(|c| h.at(&json!({"world": [c.x, c.y, c.z]})).unwrap()).collect();
    let near = |v: &egui::epaint::Vertex, p: &[f64; 2]| (f64::from(v.pos.x) - p[0]).hypot(f64::from(v.pos.y) - p[1]) < 1.0;
    let boxed = |m: &egui::Mesh| m.vertices.len() == on_screen.len() && m.vertices.iter().zip(&on_screen).all(|(v, p)| near(v, p));
    h.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if boxed(m)))
}

/// The height of the model's top under the world point (`x`, `y`).
fn top_at(h: &Harness, x: f64, y: f64) -> f64 {
    let st = h.app.session.model.state();
    let body = &st.bodies.first().expect("the plate").body;
    body.fire_ray(Vec3::new(x, y, 100.0), Vec3::new(0.0, 0.0, -1.0)).unwrap().expect("the plate is under the point").point.z
}

#[test]
fn the_marker_stands_where_the_profiles_land_on_the_target_face() {
    let mut h = plate_emboss(30.0);
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(20.0, 14.0, 10.0)) < 1e-6, "under the middle of the profile, on the plate's top and not up in the sketch: {base:?}");
    assert!(n.dist(Vec3::Z) < 1e-9, "the way the face faces, so the ring lies on it: {n:?}");
    let marker = h.at(&json!({"handle": "emboss:position"})).expect("the marker is published");
    let landed = h.at(&json!({"world": [20, 14, 10]})).unwrap();
    assert!((marker[0] - landed[0]).hypot(marker[1] - landed[1]) < 1.5, "drawn there: {marker:?} {landed:?}");
    assert!(h.at(&json!({"handle": "emboss:rotate"})).is_ok(), "with the grip that turns the profiles");
}

#[test]
fn the_marker_stands_on_the_target_face_with_the_body_between_it_and_the_sketch() {
    // The sketch lies under the plate: the build projects the profile through it onto the top.
    let h = plate_emboss(0.0);
    assert!(place(&h).0.dist(Vec3::new(20.0, 14.0, 10.0)) < 1e-6, "on the plate's top: {:?}", place(&h).0);
}

#[test]
fn the_handles_are_off_without_the_preference() {
    let mut h = plate();
    h.app.preferences.placement_handles = false;
    let sketch = badge(&mut h, json!({"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}));
    emboss(&mut h, sketch, Some(ON_THE_PLATE));
    assert!(h.at(&json!({"handle": "emboss:position"})).is_err(), "Emboss is placed by its fields only, as before");
}

#[test]
fn the_marker_drags_the_profiles_across_the_face_and_ok_builds_them_there() {
    let mut h = plate_emboss(30.0);
    let before = h.app.session.doc.to_json();
    drag_marker(&mut h, [50.0, 34.0, 10.0]);
    let [x, y, angle] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm")), (30.0, 20.0), "the offsets that land the middle under the pointer: '{x}', '{y}'");
    assert_eq!(angle, "", "the move leaves the angle alone");
    assert!(place(&h).0.dist(Vec3::new(50.0, 34.0, 10.0)) < 1e-6, "the marker goes with the profiles: {:?}", place(&h).0);
    assert_eq!(h.app.session.doc.to_json(), before, "nothing is built before OK");

    ok(&mut h);
    let f = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Emboss { .. })).expect("an Emboss was made");
    let FeatureKind::Emboss { offset_x, offset_y, angle, .. } = &f.kind else { panic!("an Emboss: {:?}", f.kind) };
    assert!(
        offset_x.is_some() && offset_y.is_some() && angle.is_none(),
        "the dragged offsets are saved, the untouched angle is left out: {:?}",
        f.kind
    );
    assert!((top_at(&h, 50.0, 34.0) - 11.0).abs() < 1e-6, "the profile is raised where the marker was dropped");
    assert!((top_at(&h, 20.0, 14.0) - 10.0).abs() < 1e-6, "and not where the sketch has it");
}

/// A place on the plate's top 0.37 of the view's step past (50, 34) along X and 0.21 of it along
/// Y: off the steps both ways, and nearer the steps at (50, 34) than any other.
fn off_the_steps(h: &Harness) -> ([f64; 3], f64) {
    let step = crate::canvas::snap_step(h.app.cam.half_height());
    ([50.0 + 0.37 * step, 34.0 + 0.21 * step, 10.0], step)
}

#[test]
fn a_drag_takes_the_offsets_to_a_round_step_of_the_view() {
    let mut h = plate_emboss(30.0);
    let (to, step) = off_the_steps(&h);
    drag_marker(&mut h, to);
    let [x, y, _] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm")), (30.0, 20.0), "each offset on the nearest step of {step} mm: '{x}', '{y}'");
    assert!(place(&h).0.dist(Vec3::new(50.0, 34.0, 10.0)) < 1e-6, "the marker stands on the step, not under the pointer");
}

#[test]
fn with_ctrl_held_a_drag_puts_the_profiles_right_under_the_pointer() {
    let mut h = plate_emboss(30.0);
    let (to, step) = off_the_steps(&h);
    h.step(&json!({"drag": [{"handle": "emboss:position"}, {"world": to}], "steps": 12, "ctrl": true})).unwrap();
    h.settle();
    let [x, y, _] = fields(&h);
    assert!(
        (number(&x, "mm") - 30.0 - 0.37 * step).abs() < 1e-3 && (number(&y, "mm") - 20.0 - 0.21 * step).abs() < 1e-3,
        "off the steps of {step} mm: '{x}', '{y}'"
    );
}

#[test]
fn a_drag_along_one_offset_leaves_what_is_typed_in_the_other() {
    let mut h = plate_emboss(30.0);
    // Off every step of the view, and written as no handle writes it.
    set_fields(&mut h, ["", "2.50 mm", "15.0 deg"]);
    drag_marker(&mut h, [50.0, 16.5, 10.0]);
    let [x, y, angle] = fields(&h);
    assert_eq!(number(&x, "mm"), 30.0, "'{x}'");
    assert_eq!((y.as_str(), angle.as_str()), ("2.50 mm", "15.0 deg"), "the drag went along X only");
}

#[test]
fn a_press_on_a_handle_changes_nothing() {
    let mut h = plate_emboss(30.0);
    // Off the steps of the view and of the ring.
    set_fields(&mut h, ["3.30 mm", "2.50 mm", "12.0 deg"]);
    let at = hold_grip_turned(&mut h, 0.0);
    let_go(&mut h, at);
    let marker = h.at(&json!({"handle": "emboss:position"})).unwrap();
    h.step(&json!({"drag": [marker, marker], "steps": 2})).unwrap();
    h.settle();
    assert_eq!(fields(&h), ["3.30 mm", "2.50 mm", "12.0 deg"].map(str::to_owned), "neither handle was moved");
}

#[test]
fn the_grip_turns_the_profiles_as_the_angle_in_five_degree_steps() {
    let mut h = plate_emboss(30.0);
    turn_grip(&mut h, 90.0);
    let [x, y, angle] = fields(&h);
    assert_eq!(number(&angle, "deg"), 90.0, "counter-clockwise seen from above is a positive Angle here: '{angle}'");
    assert_eq!((x.as_str(), y.as_str()), ("", ""), "the turn leaves the offsets alone");
    assert!(place(&h).0.dist(Vec3::new(20.0, 14.0, 10.0)) < 1e-6, "the profiles turn about the marker");
    turn_grip(&mut h, 37.0);
    assert_eq!(number(&fields(&h)[2], "deg"), 125.0, "127° is taken to the nearest 5°");
}

/// The texts painted in the last frame.
fn painted_texts(h: &Harness) -> Vec<String> {
    h.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect()
}

#[test]
fn the_readout_beside_the_grip_is_the_angle() {
    let mut h = plate_emboss(30.0);
    let at = hold_grip_turned(&mut h, 30.0);
    let texts = painted_texts(&h);
    assert!(texts.iter().any(|t| t.starts_with("30°")), "the Angle the field holds, not where the grip stands on the face: {texts:?}");
    let_go(&mut h, at);
    assert_eq!(number(&fields(&h)[2], "deg"), 30.0);

    // Past a half turn too: turned on from 200° the field holds 230°, and so does the readout.
    set_fields(&mut h, ["", "", "200 deg"]);
    let at = hold_grip_turned(&mut h, 30.0);
    let texts = painted_texts(&h);
    assert!(texts.iter().any(|t| t.starts_with("230°")), "{texts:?}");
    let_go(&mut h, at);
    assert_eq!(number(&fields(&h)[2], "deg"), 230.0);
}

#[test]
fn the_box_is_outlined_while_the_grip_turns_it() {
    let mut h = plate_emboss(30.0);
    assert!(!outlined(&mut h), "no outline while nothing is dragged");
    let at = hold_grip_turned(&mut h, 30.0);
    assert!(outlined(&mut h), "where the turn puts the box, before the preview gets there");
    let_go(&mut h, at);
    assert!(!outlined(&mut h), "and no longer once the grip is let go");
}

#[test]
fn the_footprint_is_the_box_round_the_profiles_where_it_is_built() {
    let mut h = plate_emboss(30.0);
    let close = |got: &[Vec3], want: [[f64; 2]; 4]| got.len() == 4 && got.iter().zip(want).all(|(p, [x, y])| p.dist(Vec3::new(x, y, 10.0)) < 1e-6);
    assert!(close(&outline(&h), [[10.0, 10.0], [30.0, 10.0], [30.0, 18.0], [10.0, 18.0]]), "on the plate's top: {:?}", outline(&h));
    // Turned a quarter about its middle (20, 14), then moved by (30, 20).
    set_fields(&mut h, ["30 mm", "20 mm", "90 deg"]);
    assert!(
        close(&outline(&h), [[54.0, 24.0], [54.0, 44.0], [46.0, 44.0], [46.0, 24.0]]),
        "turned and moved as the build places the profiles: {:?}",
        outline(&h)
    );
}

#[test]
fn profiles_moved_off_the_face_keep_their_handles_level_with_it() {
    let mut h = plate_emboss(30.0);
    // 55 mm along X the rectangle reaches x = 85, past the plate's 80.
    set_fields(&mut h, ["55 mm", "", ""]);
    let want = [[65.0, 10.0], [85.0, 10.0], [85.0, 18.0], [65.0, 18.0]];
    assert!(outline(&h).iter().zip(want).all(|(p, [x, y])| p.dist(Vec3::new(x, y, 10.0)) < 1e-6), "the box, in the face's plane: {:?}", outline(&h));
    // 20 mm back along Y its middle is past the plate too: the marker must stay, to drag it back
    // by.
    set_fields(&mut h, ["", "-20 mm", ""]);
    assert!(place(&h).0.dist(Vec3::new(20.0, -6.0, 10.0)) < 1e-6, "{:?}", place(&h).0);
    drag_marker(&mut h, [50.0, 34.0, 10.0]);
    assert!(place(&h).0.dist(Vec3::new(50.0, 34.0, 10.0)) < 1e-6, "dragged back onto the plate: {:?}", place(&h).0);
}

#[test]
fn without_a_target_face_the_marker_stands_in_the_sketch_and_moves_the_profiles_in_it() {
    // The sketch lies on the plate's top, and no Target Face is picked: the plain emboss.
    let mut h = plate();
    let sketch = badge(&mut h, json!({"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}));
    emboss(&mut h, sketch, None);
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::new(20.0, 14.0, 10.0)) < 1e-9 && n.dist(Vec3::Z) < 1e-9, "at the middle of the profile, in its sketch: {base:?} {n:?}");
    drag_marker(&mut h, [50.0, 34.0, 10.0]);
    let [x, y, _] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm")), (30.0, 20.0), "'{x}', '{y}'");
    let d = h.app.dialog.as_ref().unwrap();
    assert!(d.inputs.get(1).is_some_and(|i| i.items.is_empty()), "the drag picks no Target Face");
}

/// With the profile moved by `fields`, the marker must stand `round` mm round the wall from the
/// world's X at the height 30, and OK must build the profile about that very place: the wall
/// raised 2° inside where a 6 mm half width ends either way, and not 2° outside. `wrapped` is
/// whether the profile is bent round the wall, its cap reaching its ends, or laid flat against it.
fn built_round_the_wall_where_the_marker_stands(mut h: Harness, offsets: [&str; 3], round: f64, wrapped: bool) {
    set_fields(&mut h, offsets);
    let middle = (round / RADIUS).to_degrees();
    let (base, n) = place(&h);
    assert!(base.dist(Vec3::from(on_wall(middle, 30.0))) < 1e-6, "as far round the wall as across the axis in the sketch: {base:?}");
    assert!(n.dist(Vec3::from(on_wall(middle, 0.0)) * (1.0 / RADIUS)) < 1e-6, "facing out from the axis there, so the ring lies on the wall: {n:?}");
    ok(&mut h);
    // A flat profile's side walls end the cap at asin(6 / 21), 16.6°; a bent one's at 6 / 20 rad,
    // 17.2°.
    for (off, raised) in [(0.0, true), (-15.0, true), (15.0, true), (-19.5, false), (19.5, false)] {
        assert_eq!(reach_at(&h, middle + off, 30.0) > RAISED, raised, "{off}° round the wall from the marker at {middle}°");
    }
    assert_eq!(reach_at(&h, middle + 16.9, 30.0) > RADIUS + 0.9, wrapped, "between the two: only a profile bent round the wall has its cap here");
}

#[test]
fn on_a_cylinder_the_marker_stands_where_the_profiles_are_built() {
    built_round_the_wall_where_the_marker_stands(wall(20.0, 50.0), ["-16 mm", "", ""], -8.0, false);
}

#[test]
fn on_a_cylinder_the_marker_follows_a_sketch_that_touches_the_wall() {
    built_round_the_wall_where_the_marker_stands(wall(0.0, 50.0), ["-16 mm", "", ""], -8.0, true);
}

#[test]
fn a_sketch_that_touches_the_wall_has_no_handles_while_its_far_side_is_picked() {
    // Wrapped, the profile starts from where the sketch touches; laid, from the picked side.
    // Which it is goes by where the faces of the wall end, so the handles keep away; the fields
    // still place it.
    let mut h = wall(0.0, 100.0);
    assert!(super::places(&h.app, h.app.dialog.as_ref().expect("Emboss is open")).is_empty());
    set_fields(&mut h, ["-16 mm", "", ""]);
    ok(&mut h);
}

#[test]
fn a_sketch_off_the_wall_is_followed_round_the_side_that_is_picked() {
    // Picked past a quarter turn from where the wall faces the sketch, the profiles go round the
    // far side: as far past the world's -X as they were short of its X.
    built_round_the_wall_where_the_marker_stands(wall(20.0, 100.0), ["-16 mm", "", ""], std::f64::consts::PI * RADIUS + 8.0, false);
}

#[test]
fn on_a_cylinder_a_turned_profile_is_built_turned_about_the_marker() {
    let mut h = wall(20.0, 50.0);
    set_fields(&mut h, ["-16 mm", "", "90 deg"]);
    let middle = (-8.0f64 / RADIUS).to_degrees();
    assert!(place(&h).0.dist(Vec3::from(on_wall(middle, 30.0))) < 1e-6, "the turn leaves the marker where it was: {:?}", place(&h).0);
    let corners = outline(&h);
    assert!(corners.iter().all(|c| ((c.z - 30.0).abs() - 6.0).abs() < 1e-6), "the box now stands 12 along the axis: {corners:?}");
    ok(&mut h);
    // The 12 × 6 rectangle now stands 12 along the axis and 6 round the wall, about the marker.
    for (round, up, raised) in
        [(0.0, 0.0, true), (6.5, 0.0, true), (-6.5, 0.0, true), (11.0, 0.0, false), (0.0, 5.0, true), (0.0, -5.0, true), (0.0, 7.0, false)]
    {
        assert_eq!(reach_at(&h, middle + round, 30.0 + up) > RAISED, raised, "{round}° round and {up} mm up from the marker");
    }
}

#[test]
fn the_marker_is_dragged_round_a_cylinder() {
    let mut h = wall(20.0, 50.0);
    // Dropped 18 mm round the wall from where it faces the sketch and 10 mm up: (18, 10) of the
    // sketch, which is (10, 10) from the middle.
    let dropped = Vec3::from(on_wall((18.0 / RADIUS).to_degrees(), 40.0));
    drag_marker(&mut h, [dropped.x, dropped.y, dropped.z]);
    let [x, y, _] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm")), (10.0, 10.0), "'{x}', '{y}'");
    assert!(place(&h).0.dist(dropped) < 1e-6, "the marker is on the wall where it was dropped: {:?}", place(&h).0);
    let want =
        [(12.0, 37.0), (24.0, 37.0), (24.0, 43.0), (12.0, 43.0)].map(|(round, z): (f64, f64)| Vec3::from(on_wall((round / RADIUS).to_degrees(), z)));
    assert!(
        outline(&h).len() == 4 && outline(&h).iter().zip(want).all(|(p, w)| p.dist(w) < 1e-6),
        "the box is laid round the wall with it: {:?}",
        outline(&h)
    );
}

#[test]
fn on_a_part_of_a_cylinder_the_marker_is_dragged_to_where_the_profile_is_built() {
    // The half with y above 0 of a cylinder of radius 20 and height 30, and a 6 × 5 rectangle on a
    // plane 10 clear of its wall, its x round the wall towards -X and its y along the axis: its
    // middle is at (0, 7.5), where the wall faces the sketch.
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.cylinder", json!({"radius": RADIUS, "height": 30, "body_name": "Half"})).unwrap();
    h.app
        .run("solid.box", json!({"corner": [-25, -50, -5], "length": 50, "width": 50, "height": 40, "operation": "cut", "targets": ["Half"]}))
        .unwrap();
    let plane = json!({"origin": [0, 30, 0], "x_dir": [-1, 0, 0], "y_dir": [0, 0, 1]});
    let sketch = h.app.run("sketch.create", json!({"plane": plane, "name": "Mark"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-3, 5], "p1": [3, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.cam = h.app.cam.looking_from(Vec3::new(0.3, 0.9, 0.3));
    h.app.cam.target = Vec3::new(0.0, 20.0, 15.0);
    h.app.cam.distance = 120.;
    h.frames(3);
    emboss(&mut h, sketch, Some(on_wall(60.0, 22.0)));
    assert!(place(&h).0.dist(Vec3::new(0.0, 20.0, 7.5)) < 1e-6, "on the wall where it faces the sketch: {:?}", place(&h).0);

    // Dropped 10 mm round the wall towards -X and 10 mm up: (10, 10) from the middle.
    let middle = 90.0 + (10.0 / RADIUS).to_degrees();
    drag_marker(&mut h, on_wall(middle, 17.5));
    let [x, y, _] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm")), (10.0, 10.0), "'{x}', '{y}'");
    ok(&mut h);
    // A 3 mm half width laid flat ends at asin(3 / 21), 8.2°.
    for (off, raised) in [(0.0, true), (-6.5, true), (6.5, true), (-11.0, false), (11.0, false)] {
        assert_eq!(reach_at(&h, middle + off, 17.5) > RAISED, raised, "{off}° round the wall from where the marker was dropped");
    }
}

#[test]
fn the_grip_points_the_way_the_profiles_run_round_a_cylinder() {
    let mut h = wall(20.0, 50.0);
    let (_, n) = place(&h);
    // Laid round the wall the profiles keep their shape: a direction of the sketch at the angle A
    // from its x runs on the wall at A from the way round it, towards the axis' way.
    let round = Vec3::new(-n.y, n.x, 0.0);
    turned_freely_the_grip_stands_where_asked(&mut h, n, |a| angle_of(round, n) + a);
}

#[test]
fn the_grip_stands_where_it_is_turned_past_half_the_way_round_a_cylinder() {
    let mut h = wall(20.0, 50.0);
    // 8 mm across the axis and 62 more: past the half turn, which is 62.8 mm round this wall.
    set_fields(&mut h, ["62 mm", "", ""]);
    let (_, n) = place(&h);
    let round = Vec3::new(-n.y, n.x, 0.0);
    turned_freely_the_grip_stands_where_asked(&mut h, n, |a| angle_of(round, n) + a);
}

#[test]
fn the_grip_points_the_way_the_profiles_run_on_a_face_tilted_to_the_sketch() {
    // A sketch tilted 30° about the world's X over the plate: its y climbs as it goes.
    let mut h = plate();
    let (sin, cos) = 30.0f64.to_radians().sin_cos();
    let sketch = badge(&mut h, json!({"origin": [0, 0, 30], "x_dir": [1, 0, 0], "y_dir": [0, cos, sin]}));
    emboss(&mut h, sketch, Some(ON_THE_PLATE));
    let (base, n) = place(&h);
    assert!((base.z - 10.0).abs() < 1e-9 && n.dist(Vec3::Z) < 1e-9, "on the plate's top: {base:?} {n:?}");
    // Projected along the sketch's normal onto the plate, the sketch's x stays the world's X and
    // its y is stretched by 1 / cos 30° along the world's Y.
    turned_freely_the_grip_stands_where_asked(&mut h, n, |a| angle_of(Vec3::X, n) + a.sin().atan2(a.cos() * cos));
}

/// Turned freely to an angle about `n`, the grip must stand at that angle and the Angle written
/// must be the one that runs the profiles there: `on_the_model` gives where a direction of the
/// sketch at an angle from its x runs on the model.
fn turned_freely_the_grip_stands_where_asked(h: &mut Harness, n: Vec3, on_the_model: impl Fn(f64) -> f64) {
    for raw in [0.4f64, 1.3, 2.5, -2.0, -0.7] {
        let mut d = h.app.dialog.take().expect("Emboss is open");
        let taken = super::set(&h.app, &mut d, n, raw, true);
        h.app.dialog = Some(d);
        // The Angle is written to a hundredth of a degree.
        let written = number(&fields(h)[2], "deg").to_radians();
        assert!(
            nearer(on_the_model(written) - raw).abs() < 5e-4,
            "turned to {raw}, the Angle {written} runs the profiles at {}",
            on_the_model(written)
        );
        assert!(nearer(taken - raw).abs() < 5e-4, "and the grip stands where the profiles run, {raw}, not at {taken}");
    }
}

#[test]
fn editing_an_emboss_puts_the_handles_where_it_was_built() {
    let mut h = plate_emboss(30.0);
    // Turned a quarter the rectangle stands 8 wide and 20 high, about (30, 20).
    set_fields(&mut h, ["10 mm", "6 mm", "90 deg"]);
    ok(&mut h);
    let id = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Emboss { .. })).expect("an Emboss was made").id;
    h.app.edit_feature(id);
    h.settle();
    assert!(place(&h).0.dist(Vec3::new(30.0, 20.0, 10.0)) < 1e-6, "on the plate under the built profile's middle: {:?}", place(&h).0);
    drag_marker(&mut h, [40.0, 24.0, 10.0]);
    let [x, y, angle] = fields(&h);
    assert_eq!((number(&x, "mm"), number(&y, "mm"), number(&angle, "deg")), (20.0, 10.0, 90.0), "'{x}', '{y}', '{angle}'");
    ok(&mut h);
    assert!((top_at(&h, 40.0, 24.0) - 11.0).abs() < 1e-6, "the profile is rebuilt where the marker was dropped");
    assert!((top_at(&h, 30.0, 12.0) - 10.0).abs() < 1e-6, "and no longer where it was");
}

#[test]
fn editing_an_emboss_of_every_profile_places_what_was_drawn() {
    // A sketch on the plate's top has the top's own outline in it, which closes a second profile
    // round the rectangle drawn: an Emboss of every profile builds the rectangle alone.
    let mut h = plate();
    let sketch = badge(&mut h, json!({"face": ON_THE_PLATE}));
    let profiles = h.app.session.model.state().sketch(sketch).expect("the sketch").profiles.len();
    assert!(profiles > 1, "the outline closes a profile of its own: {profiles}");
    let body = h.app.session.model.state().bodies.first().expect("the plate").name.clone();
    let made =
        h.app.run("solid.emboss", json!({"sketch": "Badge", "profiles": "all", "face": ON_THE_PLATE, "depth": "1mm", "targets": [body]})).unwrap();
    h.settle();
    h.app.edit_feature(made["feature"].as_u64().expect("the feature made"));
    h.settle();
    assert!(place(&h).0.dist(Vec3::new(20.0, 14.0, 10.0)) < 1e-6, "at the middle of the rectangle, which is what was built: {:?}", place(&h).0);
}

#[test]
fn editing_an_emboss_made_on_another_copy_of_its_part_puts_the_marker_on_that_copy() {
    // The parts of `preselect_tests`' edit of a cylinder Emboss: a sleeve and the part the sketch
    // is drawn in are each placed twice, and the Emboss is made on the second of each. Edited from
    // the root, the sketch is shown on the first copy of its part.
    let mut h = Harness::new();
    h.app.preferences.placement_handles = true;
    let source = h.app.run("component.create", json!({"name": "Source", "grounded": false})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Sleeve"})).unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Source:1", "axis": [0, 0, 1], "angle": "90 deg", "translate": [50, 0, 0]})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component": source, "translate": [80, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let author = h.app.run("component.create", json!({"name": "Author"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("occurrence.move", json!({"occurrence": "Author:1", "axis": [1, 0, 0], "angle": "90 deg", "translate": [-100, 40, 30]})).unwrap();
    let author_second = h.app.run("occurrence.copy", json!({"component": author, "translate": [0, 100, 0]})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("component.activate", json!({"component": author, "occurrence": author_second})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"origin": [120, 0, 0], "x_dir": [0, -1, 0], "y_dir": [0, 0, 1]}, "name": "Badge"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [-2, 5], "p1": [2, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let target = h.app.session.occurrence_bodies(&[second], false).unwrap().into_iter().next().unwrap();
    let made =
        h.app.run("solid.emboss", json!({"sketch": "Badge", "profiles": "all", "face": [120, 0, 7.5], "depth": "1mm", "targets": [target]})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.edit_feature(made["feature"].as_u64().expect("the feature made"));
    h.frames(2);
    // The rectangle's middle is on the line where the sketch touches the second sleeve.
    assert!(place(&h).0.dist(Vec3::new(120.0, 0.0, 7.5)) < 1e-6, "on the copy it was made on: {:?}", place(&h).0);
}
