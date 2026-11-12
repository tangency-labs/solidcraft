//! Drawing a primitive in the viewport: nothing appears until the shape has been said, each
//! click takes a point (snapped where one is in reach), and the dialog's numbers follow the
//! drawing and stay editable.

use std::f64::consts::FRAC_1_SQRT_2;

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;

use crate::dialogs::Kind;
use crate::scenario::Harness;

/// The app with the viewport settled and nothing in the design.
fn app() -> Harness {
    let mut h = Harness::new();
    // Drawing a primitive belongs to Plasticity's controls (`solid_draw::start`).
    crate::plasticity::apply(&mut h.app, crate::plasticity::SCHEME_PLASTICITY);
    h.app.set_origin_shown(false);
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.settle();
    h
}

/// The screen point of a world point.
fn at(h: &Harness, p: Vec3) -> egui::Pos2 {
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    proj.to_screen(p).unwrap_or_else(|| panic!("{p:?} is off screen"))
}

fn click(h: &mut Harness, p: Vec3) {
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let pos = at(h, p);
    crate::solid_draw::on_click(&mut h.app, &proj, pos);
}

fn move_to(h: &mut Harness, p: Vec3) {
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let pos = at(h, p);
    crate::solid_draw::on_move(&mut h.app, &proj, pos);
}

/// The dialog's fields, as the gesture leaves them.
fn fields(h: &Harness) -> Vec<(String, String)> {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { fields, .. }) => fields.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
        other => panic!("no primitive dialog: {other:?}"),
    }
}

fn field(h: &Harness, key: &str) -> String {
    fields(h).into_iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_default()
}

fn bodies(h: &Harness) -> usize {
    h.app.session.world_state().unwrap().bodies.len()
}

#[test]
fn starting_a_primitive_draws_nothing_until_it_has_been_said() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    assert!(h.app.solid_draw.is_some(), "the gesture is waiting for the first corner");
    assert_eq!(h.app.dialog.as_ref().map(|d| d.preview_enabled), Some(false), "nothing is previewed yet");
    assert_eq!(bodies(&h), 0, "and nothing is made");
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.hint()), Some("Click the first corner"));
}

#[test]
fn a_box_is_drawn_corner_to_corner_and_then_up() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(1), "the first corner is down");
    assert_eq!(bodies(&h), 0, "a corner is not a body");
    // The opposite corner sizes the base; the plane is the ground, so z stays 0.
    move_to(&mut h, Vec3::new(40.0, 30.0, 0.0));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!((d.width - 40.0).abs() < 1e-3 && (d.length - 30.0).abs() < 1e-3, "{d:?}");
    assert_eq!(h.app.dialog.as_ref().map(|d| d.preview_enabled), Some(false), "a base alone is no shape");
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(2), "now the height");
    assert_eq!(field(&h, "length"), "40 mm");
    assert_eq!(field(&h, "width"), "30 mm");
    assert_eq!(bodies(&h), 0, "still nothing until the height is set");
    // The height is read along the plane's normal.
    move_to(&mut h, Vec3::new(40.0, 30.0, 20.0));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!((d.height - 20.0).abs() < 0.5, "{d:?}");
    assert!(d.defined(), "{d:?}");
    assert_eq!(h.app.dialog.as_ref().map(|d| d.preview_enabled), Some(true), "now it previews");
}

#[test]
fn the_last_point_opens_the_handles_and_the_click_after_it_makes_the_body() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    move_to(&mut h, Vec3::new(40.0, 30.0, 20.0));
    click(&mut h, Vec3::new(40.0, 30.0, 20.0));
    h.settle();
    // The last point does not commit: the shape is there to be adjusted first.
    let d = h.app.solid_draw.as_ref().unwrap_or_else(|| panic!("still being adjusted"));
    assert!(d.editing, "{d:?}");
    assert_eq!(d.hint(), "Drag the handles to adjust, then click to confirm");
    assert_eq!(bodies(&h), 0, "nothing is committed yet");
    // The click after that is the one that makes it.
    click(&mut h, Vec3::new(40.0, 30.0, 20.0));
    h.settle();
    assert!(h.app.solid_draw.is_none(), "the gesture is over");
    assert_eq!(bodies(&h), 1, "the box is made");
    let b = &h.app.session.world_state().unwrap().bodies[0];
    let bb = b.mesh().bounds();
    assert!((bb.max.x - bb.min.x - 40.0).abs() < 0.5, "{bb:?}");
    assert!((bb.max.y - bb.min.y - 30.0).abs() < 0.5, "{bb:?}");
    assert!((bb.max.z - bb.min.z - 20.0).abs() < 0.5, "{bb:?}");
}

#[test]
fn a_cylinder_is_drawn_centre_then_radius_then_height() {
    let mut h = app();
    h.app.start("solid.cylinder");
    h.settle();
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.hint()), Some("Click the centre"));
    click(&mut h, Vec3::ZERO);
    move_to(&mut h, Vec3::new(10.0, 0.0, 0.0));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!((d.radius - 10.0).abs() < 1e-3, "{d:?}");
    click(&mut h, Vec3::new(10.0, 0.0, 0.0));
    assert_eq!(field(&h, "diameter"), "20 mm");
    move_to(&mut h, Vec3::new(10.0, 0.0, 25.0));
    click(&mut h, Vec3::new(10.0, 0.0, 25.0));
    click(&mut h, Vec3::new(10.0, 0.0, 25.0));
    h.settle();
    assert_eq!(bodies(&h), 1, "the cylinder is made");
    let bb = h.app.session.world_state().unwrap().bodies[0].mesh().bounds();
    assert!((bb.max.x - bb.min.x - 20.0).abs() < 1.0, "{bb:?}");
}

#[test]
fn a_sphere_needs_only_a_centre_and_a_radius() {
    let mut h = app();
    // What is measured here is the radius clicked out, so the grid is kept out of it.
    h.app.preferences.draw_grid_snap = false;
    h.app.start("solid.sphere");
    h.settle();
    click(&mut h, Vec3::ZERO);
    move_to(&mut h, Vec3::new(12.0, 0.0, 0.0));
    assert_eq!(h.app.dialog.as_ref().map(|d| d.preview_enabled), Some(true), "a sphere is said by its radius alone");
    click(&mut h, Vec3::new(12.0, 0.0, 0.0));
    click(&mut h, Vec3::new(12.0, 0.0, 0.0));
    h.settle();
    assert!(h.app.solid_draw.is_none(), "two points, then the click that confirms");
    assert_eq!(bodies(&h), 1);
    let bb = h.app.session.world_state().unwrap().bodies[0].mesh().bounds();
    assert!((bb.max.x - bb.min.x - 24.0).abs() < 1.0, "{bb:?}");
}

#[test]
fn a_box_drawn_the_other_way_is_still_a_box() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    // Drawn right-to-left and back-to-front: the sizes are what was swept, not negative numbers.
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    click(&mut h, Vec3::ZERO);
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!((d.width - 40.0).abs() < 1e-3 && (d.length - 30.0).abs() < 1e-3, "{d:?}");
    assert!(d.sx < 0.0 && d.sy < 0.0, "which way it was drawn is remembered: {d:?}");
    move_to(&mut h, Vec3::new(0.0, 0.0, 15.0));
    click(&mut h, Vec3::new(0.0, 0.0, 15.0));
    click(&mut h, Vec3::new(0.0, 0.0, 15.0));
    h.settle();
    assert_eq!(bodies(&h), 1);
    let bb = h.app.session.world_state().unwrap().bodies[0].mesh().bounds();
    assert!((bb.min.x).abs() < 0.5 && (bb.max.x - 40.0).abs() < 0.5, "it spans the two corners: {bb:?}");
    assert!((bb.min.y).abs() < 0.5 && (bb.max.y - 30.0).abs() < 0.5, "{bb:?}");
}

#[test]
fn starting_another_command_drops_a_half_drawn_shape() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    assert!(h.app.solid_draw.is_some());
    h.app.start("solid.cylinder");
    h.settle();
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(0), "the cylinder starts from nothing");
    assert_eq!(bodies(&h), 0, "the half-drawn box made nothing");
}

/// A size said in numbers outranks one said by hand: once a field is typed, the drawing stops
/// writing it and the shape follows what was typed.
#[test]
fn a_typed_size_is_not_written_over_by_the_drawing() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    move_to(&mut h, Vec3::new(40.0, 30.0, 0.0));
    assert_eq!(field(&h, "length"), "40 mm");
    assert!(!h.app.solid_draw.as_ref().unwrap().locked("length"), "nothing is typed yet");
    // Type a length, as the dialog's field does.
    if let Some(Kind::Primitive { fields, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind)
        && let Some(f) = fields.iter_mut().find(|f| f.0 == "length")
    {
        f.1 = "55 mm".into();
    }
    // Carrying on drawing leaves it alone, and still writes the one that was not typed.
    move_to(&mut h, Vec3::new(70.0, 50.0, 0.0));
    assert!(h.app.solid_draw.as_ref().unwrap().locked("length"), "the typed field is locked");
    assert_eq!(field(&h, "length"), "55 mm", "the typed length stands");
    assert_eq!(field(&h, "width"), "50 mm", "the width still follows the cursor");
}

#[test]
fn the_numbers_can_still_be_typed_over_afterwards() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    move_to(&mut h, Vec3::new(40.0, 30.0, 20.0));
    // The drawing wrote the fields; they are ordinary text and can be replaced.
    assert_eq!(field(&h, "length"), "40 mm");
    if let Some(Kind::Primitive { fields, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind)
        && let Some(f) = fields.iter_mut().find(|f| f.0 == "length")
    {
        f.1 = "55 mm".into();
    }
    assert_eq!(field(&h, "length"), "55 mm");
}

/// The handles write the dialog's fields, so a size put right while it is being adjusted is the
/// size the body is made at.
#[test]
fn a_size_changed_while_adjusting_is_the_size_it_is_made_at() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    move_to(&mut h, Vec3::new(40.0, 30.0, 20.0));
    click(&mut h, Vec3::new(40.0, 30.0, 20.0));
    assert!(h.app.solid_draw.as_ref().is_some_and(|d| d.editing));
    // A handle drag ends as a field written, which is what this stands in for.
    if let Some(Kind::Primitive { fields, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind)
        && let Some(f) = fields.iter_mut().find(|f| f.0 == "height")
    {
        f.1 = "7 mm".into();
    }
    // Moving the cursor no longer changes anything: the handles own the sizes now.
    move_to(&mut h, Vec3::new(90.0, 90.0, 90.0));
    assert_eq!(field(&h, "height"), "7 mm", "the cursor does not take it back");
    assert_eq!(field(&h, "length"), "40 mm");
    click(&mut h, Vec3::new(40.0, 30.0, 20.0));
    h.settle();
    assert_eq!(bodies(&h), 1);
    let bb = h.app.session.world_state().unwrap().bodies[0].mesh().bounds();
    assert!((bb.max.z - bb.min.z - 7.0).abs() < 0.5, "made at the adjusted height: {bb:?}");
    assert!((bb.max.x - bb.min.x - 40.0).abs() < 0.5, "{bb:?}");
}

#[test]
fn leaving_the_shape_being_adjusted_makes_nothing() {
    let mut h = app();
    h.app.start("solid.sphere");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(12.0, 0.0, 0.0));
    assert!(h.app.solid_draw.as_ref().is_some_and(|d| d.editing), "it waits to be confirmed");
    assert_eq!(bodies(&h), 0);
    // Starting something else drops it, as it drops a half-drawn one.
    h.app.start("solid.box");
    h.settle();
    assert_eq!(bodies(&h), 0, "nothing was committed");
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(0));
}

/// The grid the view draws is the one a drawn point goes to, as the preference asks. Under
/// Plasticity's controls the chord asks for the places on the model rather than taking them
/// away, so it leaves the grid as the preference has it, held or not.
#[test]
fn a_drawn_point_goes_to_the_grid_and_the_chord_turns_it_round() {
    let mut h = app();
    let step = solidcraft_engine::view::grid_step(h.app.cam.half_height()).0;
    assert!(step > 0.0, "the view draws a grid");
    // Off: the point lands where the ray does.
    h.app.preferences.draw_grid_snap = false;
    h.app.start("solid.box");
    h.settle();
    let off_grid = Vec3::new(step * 1.4, step * 0.6, 0.0);
    click(&mut h, off_grid);
    let loose = h.app.solid_draw.as_ref().unwrap().pts[0];
    assert!((loose.x - off_grid.x).abs() < 1e-3, "turned off: {loose:?}");
    // On, as it is by default, it goes to the nearest line either way.
    h.app.preferences.draw_grid_snap = true;
    h.app.start("solid.box");
    h.settle();
    click(&mut h, off_grid);
    let snapped = h.app.solid_draw.as_ref().unwrap().pts[0];
    assert!((snapped.x - step).abs() < 1e-6 && (snapped.y - step).abs() < 1e-6, "step {step}: {snapped:?}");
    // The chord leaves it alone: asking for the places on the model does not take the grid
    // away. It is set after settling, since a drawn frame reads the chord afresh from the keys
    // actually held.
    h.app.start("solid.box");
    h.settle();
    crate::drag_snap::set_suppressed(false);
    click(&mut h, off_grid);
    let held = h.app.solid_draw.as_ref().unwrap().pts[0];
    assert!((held.x - step).abs() < 1e-6, "the chord leaves the grid on: {held:?}");
    // And with the grid off it stays off, chord or no chord: nothing on the model is in reach
    // of an empty design, so the point lands where the ray does.
    h.app.preferences.draw_grid_snap = false;
    h.app.start("solid.box");
    h.settle();
    crate::drag_snap::set_suppressed(false);
    click(&mut h, off_grid);
    let loose_with_chord = h.app.solid_draw.as_ref().unwrap().pts[0];
    assert!((loose_with_chord.x - off_grid.x).abs() < 1e-3, "the grid stays off: {loose_with_chord:?}");
    crate::drag_snap::set_suppressed(false);
}

/// The face under the cursor is what a shape is drawn on, snap or no snap: under Plasticity's
/// controls nothing locks onto the model until the chord is held, and a shape drawn over a body
/// must still stand on its face rather than fall through to the ground below it.
#[test]
fn a_shape_drawn_over_a_body_stands_on_its_face_without_the_chord() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    crate::model_snap::clear();
    crate::drag_snap::set_suppressed(true);
    h.app.start("solid.cylinder");
    h.settle();
    crate::model_snap::clear();
    crate::drag_snap::set_suppressed(true);
    click(&mut h, Vec3::new(20.0, 15.0, 20.0));
    let plane = h.app.solid_draw.as_ref().expect("a shape being drawn").plane.expect("a plane to draw on");
    assert!((plane.origin.z - 20.0).abs() < 1e-6, "drawn on the box's top face, not the ground: {plane:?}");
    crate::drag_snap::set_suppressed(false);
}

#[test]
fn the_height_goes_to_the_grid_as_the_base_does() {
    let mut h = app();
    let step = solidcraft_engine::view::grid_step(h.app.cam.half_height()).0;
    h.app.preferences.draw_grid_snap = true;
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(step * 4.0, step * 3.0, 0.0));
    move_to(&mut h, Vec3::new(step * 4.0, step * 3.0, step * 2.4));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!((d.height - step * 2.0).abs() < 1e-6, "step {step}: {d:?}");
    h.app.preferences.draw_grid_snap = false;
}

/// What the shape will be made as is set while it is drawn, by the keys those letters carry
/// during a drawing and nowhere else.
#[test]
fn the_operation_is_set_by_its_keys_while_a_shape_is_drawn() {
    let mut h = app();
    // Nothing being drawn: the keys are not taken, so the commands that carry them still work.
    assert!(!crate::solid_draw::handle_key(&mut h.app, "Q"));
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.cylinder");
    h.settle();
    assert_eq!(operation(&h), Some(0), "a body of its own to begin with");
    for (key, op) in crate::solid_draw::OPERATION_KEYS {
        assert!(crate::solid_draw::handle_key(&mut h.app, key), "{key} is taken while drawing");
        assert_eq!(operation(&h), Some(op), "{key}");
    }
    // A key that means nothing here is left for whatever else wants it.
    assert!(!crate::solid_draw::handle_key(&mut h.app, "Z"));
    // It reaches the command: a cut takes material out rather than adding a body.
    crate::solid_draw::handle_key(&mut h.app, "W");
    assert_eq!(operation(&h), Some(2));
    // Drawn on the box's top face -- the face under the cursor is what a shape is drawn on,
    // chord or no chord -- and taken down through it.
    let before = volume(&h);
    click(&mut h, Vec3::new(20.0, 15.0, 20.0));
    click(&mut h, Vec3::new(26.0, 15.0, 20.0));
    move_to(&mut h, Vec3::new(26.0, 15.0, -10.0));
    click(&mut h, Vec3::new(26.0, 15.0, -10.0));
    click(&mut h, Vec3::new(26.0, 15.0, -10.0));
    h.settle();
    assert_eq!(bodies(&h), 1, "the cylinder went into the box rather than beside it");
    let after = volume(&h);
    assert!(after < before - 1000.0, "material came out: {before} -> {after}");
    // Once it is over the keys go back to meaning what they usually do.
    assert!(!crate::solid_draw::handle_key(&mut h.app, "Q"));
}

/// The volume of everything in the design.
fn volume(h: &Harness) -> f64 {
    h.app.session.world_state().unwrap().bodies.iter().filter_map(|b| solidcraft_engine::kernel::measure(&b.body).ok()).map(|m| m.volume).sum()
}

/// The operation a drawing is set to.
fn operation(h: &Harness) -> Option<usize> {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { operation, .. }) => Some(*operation),
        _ => None,
    }
}

/// Backspace takes the last point back a step at a time, rather than costing the whole shape.
#[test]
fn backspace_takes_the_last_point_back() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(40.0, 30.0, 0.0));
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(2));
    assert!(crate::solid_draw::handle_key(&mut h.app, "Backspace"));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert_eq!(d.stage, 1, "back to sizing the base");
    assert_eq!(d.pts.len(), 1, "{d:?}");
    assert!(crate::solid_draw::handle_key(&mut h.app, "Backspace"));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert_eq!(d.stage, 0, "back to the first corner");
    assert!(d.pts.is_empty() && d.plane.is_none(), "the plane is chosen afresh: {d:?}");
    // Nothing left to take back: it waits rather than ending.
    assert!(crate::solid_draw::handle_key(&mut h.app, "Backspace"));
    assert_eq!(h.app.solid_draw.as_ref().map(|d| d.stage), Some(0));
    assert_eq!(bodies(&h), 0);
}

/// Out of the adjusting stage, Backspace puts the shape back to being drawn.
#[test]
fn backspace_leaves_the_adjusting_stage() {
    let mut h = app();
    h.app.start("solid.sphere");
    h.settle();
    click(&mut h, Vec3::ZERO);
    click(&mut h, Vec3::new(12.0, 0.0, 0.0));
    assert!(h.app.solid_draw.as_ref().is_some_and(|d| d.editing));
    assert!(crate::solid_draw::handle_key(&mut h.app, "Backspace"));
    let d = h.app.solid_draw.as_ref().unwrap();
    assert!(!d.editing && d.stage == 2, "back to drawing: {d:?}");
    assert_eq!(bodies(&h), 0);
}

/// The strip names every key the drawing answers to, and marks the one that is set.
#[test]
fn the_hints_name_the_keys_and_mark_what_is_set() {
    let mut h = app();
    assert_eq!(crate::solid_draw::key_hints(&h.app), None, "nothing being drawn says nothing");
    h.app.start("solid.box");
    h.settle();
    let (line, hints) = crate::solid_draw::key_hints(&h.app).unwrap_or_else(|| panic!("a drawing speaks"));
    assert_eq!(line, "Click the first corner");
    for (key, _) in crate::solid_draw::OPERATION_KEYS {
        assert!(hints.iter().any(|x| x.key == key), "{key} is named: {hints:?}");
    }
    assert!(hints.iter().any(|x| x.key == "Backspace"), "{hints:?}");
    assert!(hints.iter().any(|x| x.key == "Esc"), "{hints:?}");
    // New Body to begin with, and the mark follows the key pressed.
    assert_eq!(hints.iter().find(|x| x.on).map(|x| x.key), Some("B"));
    crate::solid_draw::handle_key(&mut h.app, "W");
    let (_, hints) = crate::solid_draw::key_hints(&h.app).unwrap_or_else(|| panic!("a drawing speaks"));
    assert_eq!(hints.iter().find(|x| x.on).map(|x| (x.key, x.label)), Some(("W", "Cut")));
}

/// Escape closes the dialog, and the shape being drawn goes with it.
#[test]
fn cancelling_the_dialog_drops_the_drawing() {
    let mut h = app();
    h.app.start("solid.box");
    h.settle();
    click(&mut h, Vec3::ZERO);
    assert!(h.app.solid_draw.is_some());
    crate::dialogs::cancel(&mut h.app);
    h.settle();
    assert!(h.app.solid_draw.is_none(), "no drawing is left behind without its dialog");
    assert!(h.app.dialog.is_none());
    assert_eq!(bodies(&h), 0);
}

/// A rectangle can be drawn square, and measured from a corner or from its centre, while it is
/// being drawn rather than by picking a different command first.
#[test]
fn a_rectangle_can_be_squared_and_measured_from_its_centre() {
    use solidcore_record::Vec2;
    let mut h = app();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.start("sketch.rectangle.two_point");
    h.settle();
    crate::tools::place(&mut h.app, (Vec2::new(0.0, 0.0), None));
    // Square takes the shorter side out to the longer, keeping the way it was drawn.
    assert!(crate::tools::handle_key(&mut h.app, "Alt+S"));
    assert!(h.app.tool.as_ref().is_some_and(|t| t.square));
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let at = proj.to_screen(solidcore_record::Vec3::new(40.0, -10.0, 0.0)).unwrap();
    crate::tools::on_hover(&mut h.app, &proj, at, false);
    let hover = h.app.tool.as_ref().and_then(|t| t.hover.clone()).unwrap_or_else(|| panic!("a hover"));
    assert!((hover.0.x - 40.0).abs() < 0.2 && (hover.0.y + 40.0).abs() < 0.2, "square, and still down-right: {hover:?}");
    // Off again, and the cursor is followed as it was.
    assert!(crate::tools::handle_key(&mut h.app, "Alt+S"));
    crate::tools::on_hover(&mut h.app, &proj, at, false);
    let hover = h.app.tool.as_ref().and_then(|t| t.hover.clone()).unwrap_or_else(|| panic!("a hover"));
    assert!((hover.0.y + 10.0).abs() < 0.2, "{hover:?}");
    // The centre key swaps which point the rectangle is measured from.
    assert!(crate::tools::handle_key(&mut h.app, "Alt+C"));
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.rectangle.center"));
    assert!(crate::tools::handle_key(&mut h.app, "Alt+C"));
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.rectangle.two_point"));
    // Neither key is a rectangle's alone to keep: a circle leaves them.
    h.app.start("sketch.circle.center");
    h.settle();
    assert!(!crate::tools::handle_key(&mut h.app, "Alt+S"));
    assert!(!crate::tools::handle_key(&mut h.app, "Alt+C"));
}

/// Backspace takes a sketch point back, as it does a drawn shape's.
#[test]
fn backspace_takes_a_sketch_point_back() {
    use solidcore_record::Vec2;
    let mut h = app();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.start("sketch.line");
    h.settle();
    assert!(!crate::tools::handle_key(&mut h.app, "Backspace"), "nothing down yet");
    crate::tools::place(&mut h.app, (Vec2::new(0.0, 0.0), None));
    crate::tools::place(&mut h.app, (Vec2::new(20.0, 0.0), None));
    let before = h.app.tool.as_ref().map(|t| t.pts.len()).unwrap_or(0);
    assert!(before >= 1, "points are down");
    assert!(crate::tools::handle_key(&mut h.app, "Backspace"));
    assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(before - 1));
}

/// The operation the drawing has settled on.
fn drawn_op(h: &Harness) -> usize {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { operation, .. }) => *operation,
        other => panic!("no primitive dialog: {other:?}"),
    }
}

/// Hover a world point through the viewport, so the snap finds the face under it and the drawing
/// takes its plane from there (the helpers above go straight to the gesture and never do).
fn hover(h: &mut Harness, p: Vec3) {
    let at = h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap();
    h.step(&json!({ "move": at })).unwrap();
}

fn tap(h: &mut Harness, p: Vec3) {
    hover(h, p);
    let at = h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap();
    h.step(&json!({ "click": at })).unwrap();
}

/// The way a hole is made: draw a cylinder on a face and pull it into the solid, and it takes
/// material out instead of standing a body on top. Pull it back out and it is a body of its own
/// again, so the choice follows the gesture rather than being made once and kept.
#[test]
fn a_shape_pulled_into_the_solid_cuts_it() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.cylinder");
    h.settle();
    // Base on the top face: its centre, then a point on the rim.
    tap(&mut h, Vec3::new(20.0, 15.0, 20.0));
    tap(&mut h, Vec3::new(25.0, 15.0, 20.0));
    let plane = h.app.solid_draw.as_ref().and_then(|d| d.plane).unwrap_or_else(|| panic!("a plane"));
    assert!((plane.origin.z - 20.0).abs() < 1e-6, "drawn on the top face, not the ground: {plane:?}");
    assert_eq!(drawn_op(&h), 0, "nothing is cut until the pull says so");
    // Pulled down into the block.
    hover(&mut h, Vec3::new(20.0, 15.0, 8.0));
    assert_eq!(drawn_op(&h), 2, "pulled into the solid, it cuts");
    // Carried right through and out of the far side, it is still a cut.
    hover(&mut h, Vec3::new(20.0, 15.0, -30.0));
    assert_eq!(drawn_op(&h), 2, "a pull through the far side is still a cut");
    // Pulled back out above the face, it is a body of its own again.
    hover(&mut h, Vec3::new(20.0, 15.0, 32.0));
    assert_eq!(drawn_op(&h), 0, "pulled away from it, it stands on its own");
    // Taken in again and finished: the block keeps its one body, with a bore down the axis.
    let bottom = Vec3::new(20.0, 15.0, 8.0);
    tap(&mut h, bottom);
    tap(&mut h, bottom);
    h.settle();
    assert_eq!(bodies(&h), 1, "the cut stays one body, not a cylinder standing on a block");
    let solid = |p: Vec3| h.app.session.world_state().unwrap().bodies[0].mesh().contains(p);
    assert!(!solid(Vec3::new(20.0, 15.0, 18.0)), "the bore is empty on its axis");
    assert!(!solid(Vec3::new(23.0, 15.0, 18.0)), "and out to its wall");
    assert!(solid(Vec3::new(20.0, 15.0, 2.0)), "the block is whole below the bore");
    assert!(solid(Vec3::new(5.0, 5.0, 18.0)), "and beside it");
}

/// An operation key is the person saying what they want, so the pull stops choosing after it.
#[test]
fn an_operation_key_takes_the_choice_from_the_pull() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.cylinder");
    h.settle();
    tap(&mut h, Vec3::new(20.0, 15.0, 20.0));
    tap(&mut h, Vec3::new(25.0, 15.0, 20.0));
    assert!(crate::solid_draw::handle_key(&mut h.app, "Q"), "Join");
    assert_eq!(drawn_op(&h), 1);
    hover(&mut h, Vec3::new(20.0, 15.0, 8.0));
    assert_eq!(drawn_op(&h), 1, "pulled into the solid, it is still the join that was asked for");
}

/// A box follows the same rule as the cylinder: drawn on a face and pulled in, it takes a pocket
/// out of the solid rather than standing a block on it.
#[test]
fn a_box_pulled_into_the_solid_cuts_a_pocket() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.box");
    h.settle();
    tap(&mut h, Vec3::new(10.0, 8.0, 20.0));
    tap(&mut h, Vec3::new(30.0, 22.0, 20.0));
    assert_eq!(drawn_op(&h), 0, "nothing is cut until the pull says so");
    hover(&mut h, Vec3::new(20.0, 15.0, 10.0));
    assert_eq!(drawn_op(&h), 2, "pulled into the solid, it cuts");
    hover(&mut h, Vec3::new(20.0, 15.0, 30.0));
    assert_eq!(drawn_op(&h), 0, "pulled away from it, it stands on its own");
    // Taken in again and finished: one body with a pocket in its top.
    let floor = Vec3::new(20.0, 15.0, 10.0);
    tap(&mut h, floor);
    tap(&mut h, floor);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    assert_eq!(bodies(&h), 1, "the cut stays one body");
    let solid = |p: Vec3| h.app.session.world_state().unwrap().bodies[0].mesh().contains(p);
    assert!(!solid(Vec3::new(20.0, 15.0, 18.0)), "the pocket is empty");
    assert!(solid(Vec3::new(20.0, 15.0, 2.0)), "the block is whole below it");
    assert!(solid(Vec3::new(3.0, 3.0, 18.0)), "and beside it");
}

/// A box drawn on the ground beside a body is not pulled into anything, so it stays a body of its
/// own however far it is pulled.
#[test]
fn a_box_drawn_clear_of_everything_stays_its_own_body() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    h.app.start("solid.box");
    h.settle();
    tap(&mut h, Vec3::new(60.0, 0.0, 0.0));
    tap(&mut h, Vec3::new(80.0, 20.0, 0.0));
    hover(&mut h, Vec3::new(70.0, 10.0, 15.0));
    assert_eq!(drawn_op(&h), 0, "pulled up out of the ground, clear of the block");
    hover(&mut h, Vec3::new(70.0, 10.0, -15.0));
    assert_eq!(drawn_op(&h), 0, "and down through it, where there is still nothing to cut");
}

/// A sphere is said by its centre and its radius alone, so there is no pull for the drawing to
/// read an operation from: it says nothing, and the window's own automatic choice decides
/// (`op_auto`, #6901, which this base has and the fork did not). A sphere resting on the block
/// joins it, as that rule says; turn the choice off in Preferences and it stands on its own.
#[test]
fn a_sphere_has_no_pull_to_choose_by() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.settle();
    // With the window's own choice off, the drawing leaves the operation where it opened.
    h.app.preferences.auto_operation = false;
    h.app.start("solid.sphere");
    h.settle();
    tap(&mut h, Vec3::new(20.0, 15.0, 20.0));
    hover(&mut h, Vec3::new(30.0, 15.0, 20.0));
    assert_eq!(drawn_op(&h), 0, "the drawing reads no operation from a sphere");
    let at = h.at(&json!({"world": [30.0, 15.0, 20.0]})).unwrap();
    h.step(&json!({ "click": at })).unwrap();
    h.step(&json!({ "click": at })).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert_eq!(bodies(&h), 2, "the block and the sphere, both standing");
}

/// A box drawn on a face stands on it, whichever way that face lies. It used to be built square to
/// the world and so fell back to a level plane through the click on anything tilted, which put it
/// neither on the face nor along its normal (and decided the cut by the wrong direction with it).
#[test]
fn a_box_drawn_on_a_tilted_face_stands_on_it() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    // A 45-degree face, from a chamfer across the top edge at +x.
    h.app.run("solid.chamfer", json!({"edges": [[40, 15, 20]], "distance": 10})).unwrap();
    h.settle();
    // The chamfer's own face, found by its normal.
    let st = h.app.session.world_state().unwrap();
    let m = st.body("Body1").unwrap().mesh();
    let faces = m.tri_face.iter().copied().max().map_or(0, |f| f + 1);
    let at = (0..faces)
        .find_map(|f| {
            solidcraft_engine::geom::snap::planar_face_center(&m, f)
                .filter(|(_, n)| (n.z - FRAC_1_SQRT_2).abs() < 1e-2 && (n.x - FRAC_1_SQRT_2).abs() < 1e-2)
                .map(|(c, _)| c)
        })
        .unwrap_or_else(|| panic!("the chamfer's own face"));
    drop(st);
    h.app.start("solid.box");
    h.settle();
    tap(&mut h, at);
    let plane = h.app.solid_draw.as_ref().and_then(|d| d.plane).unwrap_or_else(|| panic!("a plane"));
    let n = plane.normal();
    assert!((n.x - FRAC_1_SQRT_2).abs() < 1e-2 && (n.z - FRAC_1_SQRT_2).abs() < 1e-2, "it is drawn on the tilted face, not on a level plane: {n:?}");
}

/// Keep Tools (`T`, as in Plasticity): the shape stays a body of its own as well as cutting the
/// solid it is drawn into. The strip names the key and marks it while it is on.
#[test]
fn the_keep_tools_key_leaves_the_shape_standing_as_well() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Slab"})).unwrap();
    h.settle();
    h.app.start("solid.box");
    h.settle();
    let keep = |h: &Harness| h.app.dialog.as_ref().and_then(|d| d.extra.get("keep_tools")).and_then(serde_json::Value::as_bool).unwrap_or(false);
    assert!(!keep(&h), "off to begin with");
    let (_, hints) = crate::solid_draw::key_hints(&h.app).unwrap_or_else(|| panic!("a drawing speaks"));
    let hint = hints.iter().find(|x| x.key == crate::solid_draw::KEEP_TOOLS_KEY).unwrap_or_else(|| panic!("{hints:?}"));
    assert_eq!((hint.label, hint.on), ("Keep Tools", false));
    assert!(crate::solid_draw::handle_key(&mut h.app, crate::solid_draw::KEEP_TOOLS_KEY), "the drawing takes it");
    assert!(keep(&h), "and it is on now");
    // Cut (`W`), drawn into the slab's top: the pocket is taken out and the box stays a body.
    crate::solid_draw::handle_key(&mut h.app, "W");
    tap(&mut h, Vec3::new(10.0, 8.0, 20.0));
    tap(&mut h, Vec3::new(30.0, 22.0, 20.0));
    let floor = Vec3::new(20.0, 15.0, 12.0);
    tap(&mut h, floor);
    tap(&mut h, floor);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    assert_eq!(bodies(&h), 2, "the pocket and the box that cut it");
    let state = h.app.session.world_state().unwrap();
    let slab = state.bodies.iter().find(|b| b.name == "Slab").unwrap_or_else(|| panic!("the slab is still there"));
    let taken = 40.0 * 30.0 * 20.0 - slab.mesh().measure().volume;
    assert!(taken > 100.0, "the slab really lost material: {taken} mm³");
    let kept = state.bodies.iter().find(|b| b.name != "Slab").unwrap_or_else(|| panic!("the box is kept"));
    // The box is kept whole, so it holds at least what the pocket took out of the slab.
    let left = kept.mesh().measure().volume;
    assert!(left >= taken - 0.05, "the kept box holds the pocket: {left} against {taken}");
}

/// A pocket is taken out where the rectangle was drawn, whichever way round it was dragged and
/// however the face lies. The frame used to be built by flipping its directions, so a box drawn
/// right to left spanned backwards out of its corner and one pulled into a tilted face ran off to
/// the side of it: half of this pocket fell outside the solid and only half the material went.
#[test]
fn a_pocket_drawn_right_to_left_on_a_tilted_face_is_cut_where_it_was_drawn() {
    let mut h = app();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Block"})).unwrap();
    // Not 45 degrees: a face square to the iso view's diagonal is edge-on, and nothing can be
    // drawn on what cannot be seen.
    h.app.run("solid.chamfer", json!({"edges": [[40, 15, 20]], "distance": 12, "distance2": 5})).unwrap();
    h.settle();
    let whole = h.app.session.world_state().unwrap().body("Block").unwrap().mesh().measure().volume;
    // The chamfer's own tilted face, found by its normal, as the tilted-face case above does.
    let st = h.app.session.world_state().unwrap();
    let m = st.body("Block").unwrap().mesh();
    let faces = m.tri_face.iter().copied().max().map_or(0, |f| f + 1);
    let (centre, normal) = (0..faces)
        .find_map(|f| {
            solidcraft_engine::geom::snap::planar_face_center(&m, f)
                .filter(|(_, n)| n.y.abs() < 1e-3 && n.x > 0.1 && n.z > 0.1 && (n.x - n.z).abs() > 0.1)
        })
        .unwrap_or_else(|| panic!("the chamfer's own face"));
    drop(st);
    // Two in-plane directions of that face: along the edge, and across it.
    let along = Vec3::new(0.0, 1.0, 0.0);
    let across = normal.cross(along);
    h.app.start("solid.box");
    h.settle();
    // Dragged backwards along both, the way that used to come out spanning the other way.
    tap(&mut h, centre + along * 9.0 + across * 3.0);
    tap(&mut h, centre - along * 9.0 - across * 3.0);
    let d = h.app.solid_draw.as_ref().unwrap_or_else(|| panic!("a drawing"));
    let (width, length) = (d.width, d.length);
    // Far enough past the face that the drag reads as a pull into the solid.
    let into = centre - normal * 18.0;
    hover(&mut h, into);
    assert_eq!(drawn_op(&h), 2, "pulled into the solid, it cuts");
    let depth = h.app.solid_draw.as_ref().map(|d| d.height).unwrap_or(0.0);
    tap(&mut h, into);
    tap(&mut h, into);
    h.step(&json!({"key": "Enter"})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    assert_eq!(bodies(&h), 1, "the cut stays one body");
    // The whole of the box came out of the solid, not the half of it that lay inside.
    let taken = whole - h.app.session.world_state().unwrap().body("Block").unwrap().mesh().measure().volume;
    let want = width * length * depth;
    assert!(want > 50.0, "the pull is worth measuring: {want}");
    assert!((taken - want).abs() < want * 0.02, "took {taken} mm³ of {want}");
}
