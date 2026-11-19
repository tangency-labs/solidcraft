//! Regression tests: editing a sketch from the Browser or timeline can always be left again.

use crate::SolveApp;
use crate::test_support::sample_app;
use solidcraft_engine::doc::FeatureKind;

fn first_sketch(app: &SolveApp) -> u64 {
    app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap()
}

fn plate_and_cylinder() -> crate::scenario::Harness {
    use serde_json::json;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    for (command, params) in [
        ("sketch.create", json!({"plane": "XY", "name": "Sketch1"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"name": "Plate", "distance": 10})),
        ("solid.cylinder", json!({"name": "Cyl", "base": [60, 15, 0], "radius": 8, "height": 20, "operation": "new"})),
    ] {
        h.app.run(command, params).unwrap();
    }
    h
}

#[test]
fn finishing_an_edited_sketch_undoes_the_edit_with_the_complete_design_visible() {
    use serde_json::json;
    for engine_finish in [false, true] {
        let mut h = plate_and_cylinder();
        let before = h.app.session.doc.to_json();
        let id = first_sketch(&h.app);
        h.app.edit_sketch(id);
        h.app.run("sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [20, 20]})).unwrap();
        if engine_finish {
            h.app.run("sketch.finish", json!({})).unwrap();
        } else {
            h.app.finish_sketch();
        }
        assert_eq!(h.app.session.doc.marker, None);
        let edited = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.marker, None, "Undo must not expose the temporary sketch rollback");
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.session.doc.to_json(), before, "one Undo takes back the sketch edit");
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
        let volume = h.app.run("inspect.measure", json!({"bodies": ["Body1"]})).unwrap();
        assert!((volume["total"]["volume_mm3"].as_f64().unwrap() - 12000.0).abs() < 1e-6);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), edited);
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
    }
}

#[test]
fn engine_finish_restores_the_marker_from_a_ui_sketch_edit() {
    use serde_json::json;
    for marker in [None, Some(0), Some(2)] {
        let mut h = plate_and_cylinder();
        h.app.run("timeline.roll_to", json!({"position": marker})).unwrap();
        let before = h.app.session.doc.to_json();
        let bodies = h.app.session.model.state().bodies.len();
        let id = first_sketch(&h.app);
        h.app.edit_sketch(id);
        // Deliberately bypass the window's completion callback: the engine owns restoration.
        h.app.session.execute("sketch.finish", &json!({})).unwrap();
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.model.state().bodies.len(), bodies);
    }
}

#[test]
fn unchanged_sketch_edit_keeps_undo_and_redo() {
    use serde_json::json;
    let mut h = plate_and_cylinder();
    h.app.run("edit.undo", json!({})).unwrap();
    let before = h.app.session.doc.to_json();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let id = first_sketch(&h.app);
    h.app.edit_sketch(id);
    h.app.finish_sketch();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    h.app.run("edit.redo", json!({})).unwrap();
    assert_eq!(h.app.session.model.state().bodies.len(), 2);
}

#[test]
fn edit_sketch_rolls_back_and_finish_restores() {
    let mut app = sample_app();
    let marker = app.session.doc.marker;
    let id = first_sketch(&app);
    app.edit_sketch(id);
    assert_eq!(app.session.active_sketch, Some(id));
    let idx = app.session.doc.feature_index(id).unwrap();
    assert_eq!(app.session.doc.marker, Some(idx + 1), "later features are rolled back");
    app.finish_sketch();
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.doc.marker, marker, "marker restored");
}

/// #2716: with the history marker before a sketch, editing it rolls the marker forward to it
/// (it is built, its palette shows), and Finish puts the marker back.
#[test]
fn edit_sketch_rolls_forward_from_before_it_and_finish_restores() {
    let mut app = sample_app();
    let id = first_sketch(&app);
    let idx = app.session.doc.feature_index(id).unwrap();
    app.run("timeline.roll_to", serde_json::json!({ "position": idx })).unwrap();
    let marker = app.session.doc.marker;
    assert!(app.session.model.state().sketch(id).is_none(), "the sketch is past the marker");
    app.edit_sketch(id);
    assert_eq!(app.session.active_sketch, Some(id));
    assert_eq!(app.session.doc.marker, Some(idx + 1), "rolled forward to just after the sketch");
    assert!(app.session.model.state().sketch(id).is_some(), "the sketch is built");
    app.finish_sketch();
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.doc.marker, marker, "marker restored");
}

#[test]
fn editing_twice_does_not_nest() {
    let mut app = sample_app();
    let marker = app.session.doc.marker;
    let id = first_sketch(&app);
    app.edit_sketch(id);
    app.edit_sketch(id);
    app.edit_feature(id);
    app.finish_sketch();
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.doc.marker, marker);
}

#[test]
fn editing_another_sketch_switches_cleanly() {
    let mut app = sample_app();
    let marker = app.session.doc.marker;
    let ids: Vec<u64> = app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).collect();
    app.edit_sketch(ids[0]);
    app.edit_sketch(ids[1]);
    assert_eq!(app.session.active_sketch, Some(ids[1]));
    app.finish_sketch();
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.doc.marker, marker);
}

#[test]
fn toolbar_finish_and_other_feature_edit_leave_the_sketch() {
    let mut app = sample_app();
    let marker = app.session.doc.marker;
    let id = first_sketch(&app);
    app.edit_sketch(id);
    app.start("sketch.finish");
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.doc.marker, marker);
    // Double-clicking a solid feature while sketching finishes the sketch first.
    app.edit_sketch(id);
    let extrude = app.session.doc.features.iter().find(|f| !matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap();
    app.edit_feature(extrude);
    assert_eq!(app.session.active_sketch, None);
}

fn two_finished_sketches() -> (crate::scenario::Harness, Vec<u64>) {
    use serde_json::json;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let mut ids = Vec::new();
    for name in ["First", "Second"] {
        let created = h.app.run("sketch.create", json!({"plane": "XY", "name": name})).unwrap();
        ids.push(created["sketch"].as_u64().unwrap());
        h.app.run("sketch.line", json!({"points": [[0, 0], [20, 10]]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.settle();
    (h, ids)
}

#[test]
fn ribbon_edit_sketch_opens_the_selected_sketch_and_finish_restores_the_marker() {
    use serde_json::json;
    let (mut h, ids) = two_finished_sketches();
    let marker = h.app.session.doc.marker;
    h.app.run("select.set", json!({"items": [{"type": "feature", "id": ids[0]}]})).unwrap();
    h.step(&json!({"start": "sketch.edit"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.active_sketch, Some(ids[0]));
    assert_eq!(h.app.session.doc.marker, Some(1));
    h.step(&json!({"start": "sketch.finish"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.active_sketch, None);
    assert_eq!(h.app.session.doc.marker, marker);
}

#[test]
fn ribbon_edit_sketch_offers_a_choice_without_a_unique_selection() {
    use serde_json::json;
    for selected in [false, true] {
        let (mut h, ids) = two_finished_sketches();
        if selected {
            h.app
                .run(
                    "select.set",
                    json!({"items": [
                        {"type": "feature", "id": ids[0]}, {"type": "feature", "id": ids[1]}
                    ]}),
                )
                .unwrap();
        }
        h.step(&json!({"start": "sketch.edit"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None, "do not choose an arbitrary sketch");
        assert!(h.app.viewport.context_menu.is_some(), "offer the sketches to edit");
        h.step(&json!({"call": "ui.menuPick", "params": {"item": "Second"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, Some(ids[1]));
        h.step(&json!({"start": "sketch.finish"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, None);
    }
}

#[test]
fn ribbon_edit_sketch_uses_the_owner_of_selected_sketch_geometry() {
    use serde_json::json;
    use solidcraft_engine::Sel;
    let (mut h, ids) = two_finished_sketches();
    h.app.session.selection =
        vec![Sel::ExternalSketchCurve { sketch: ids[1], id: "l1".into() }, Sel::ExternalSketchPoint { sketch: ids[1], id: "p1".into() }];
    h.step(&json!({"start": "sketch.edit"})).unwrap();
    h.settle();
    assert_eq!(h.app.session.active_sketch, Some(ids[1]));
}

#[test]
fn ribbon_edit_sketch_handles_an_empty_design_without_running_a_command() {
    use serde_json::json;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.step(&json!({"start": "sketch.edit"})).unwrap();
    h.settle();
    let items = crate::context_menu::items(&h.app, &h.app.menu.target);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "No sketches to edit");
    assert!(!items[0].enabled);
    assert_eq!(h.app.session.active_sketch, None);
    assert!(h.app.session.doc.features.is_empty());
    assert!(h.app.session.execute("sketch.edit", &json!({})).is_err(), "the programmatic command still requires a sketch");
}

/// Leaving a sketch any way leaves the camera exactly where it is; entering still faces the plane.
mod exit_keeps_view {
    use crate::scenario::Harness;
    use serde_json::json;
    use solidcraft_viewport::Camera;

    /// Start a sketch on XY from the iso view, then orbit and pan into a 3D view of the user's own.
    fn sketching_in_3d() -> (Harness, Camera) {
        let mut h = Harness::new();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(3);
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        crate::scenario::check(&mut h, &json!({"sketching": true, "camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam.pan(25.0, 15.0, 800.0);
        h.app.cam_anim = None;
        let cam = h.app.cam;
        (h, cam)
    }

    #[test]
    fn finish_keeps_the_camera() {
        let (mut h, cam) = sketching_in_3d();
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
        h.app.finish_sketch();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.cam, cam);
    }

    #[test]
    fn double_escape_keeps_the_camera_with_or_without_geometry() {
        for line in [true, false] {
            let (mut h, cam) = sketching_in_3d();
            if line {
                h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
            }
            h.step(&json!({"key": "Escape"})).unwrap();
            h.step(&json!({"key": "Escape"})).unwrap();
            h.settle();
            assert_eq!(h.app.session.active_sketch, None, "line: {line}");
            assert_eq!(h.app.session.doc.features.len(), usize::from(line), "a blank sketch is discarded, one with a line kept");
            assert_eq!(h.app.cam, cam, "line: {line}");
        }
    }

    #[test]
    fn cancelling_the_new_sketch_dialog_keeps_the_camera() {
        let mut h = Harness::new();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(3);
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam_anim = None;
        let cam = h.app.cam;
        crate::dialogs::cancel(&mut h.app);
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.cam, cam);
    }

    #[test]
    fn editing_a_sketch_faces_it_and_finishing_leaves_the_view() {
        let mut app = crate::test_support::sample_app();
        let id = super::first_sketch(&app);
        let before = app.cam;
        app.edit_sketch(id);
        assert!(app.cam_anim.is_some_and(|a| a.to != before), "entering turns the view to face the sketch");
        app.cam_anim = None;
        app.cam.orbit(30.0, 20.0);
        let cam = app.cam;
        app.finish_sketch();
        assert_eq!(app.cam, cam);
        assert!(app.cam_anim.is_none(), "no animation back");
    }
}

/// OK and Cancel in the view's corner (#6709): in a sketch without a dialog, the check is Finish
/// Sketch and the cross Cancel Sketch, as on the ribbon; with a dialog open in the sketch, the pair
/// answers the dialog and the sketch stays open.
mod view_corner {
    use crate::scenario::{self, Harness};
    use serde_json::json;

    fn sketching() -> Harness {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"view_confirm_corner": true}));
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_some());
        h
    }

    #[test]
    fn the_check_finishes_and_the_cross_cancels_the_sketch() {
        let mut h = sketching();
        assert!(scenario::rect_of("view:confirm").is_some(), "the pair while a sketch is open");
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(1.0));
        h.step(&json!({"click": {"handle": "view:confirm:ok"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None, "finished");
        assert_eq!(h.app.session.doc.features.len(), 1, "the sketch is kept");
        assert!(scenario::rect_of("view:confirm").is_none());

        let mut h = sketching();
        h.step(&json!({"click": {"handle": "view:confirm:cancel"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None, "cancelled");
        assert!(h.app.session.doc.features.is_empty(), "a new sketch goes with what was drawn in it");
    }

    /// With a line half drawn, the cross takes the same steps as Esc: the placed point, then the
    /// tool, and only then the sketch, which keeps what was drawn until that last click.
    #[test]
    fn the_cross_cancels_an_unfinished_tool_before_the_sketch() {
        let mut h = sketching();
        h.step(&json!({"start": "sketch.line"})).unwrap();
        assert!(h.app.tool.is_some(), "the line tool is out");
        h.step(&json!({"click": {"sketch": [10, 20]}})).unwrap();
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.pts.len()), Some(1), "the line's first point is down");
        let cross = |h: &mut Harness| {
            h.step(&json!({"click": {"handle": "view:confirm:cancel"}})).unwrap();
            h.settle();
        };

        cross(&mut h);
        assert_eq!(h.app.tool.as_ref().map(|tool| tool.pts.len()), Some(0), "the first click takes the point back");
        assert!(h.app.session.active_sketch.is_some());
        cross(&mut h);
        assert!(h.app.tool.is_none(), "the second puts the tool down");
        assert!(h.app.session.active_sketch.is_some(), "and keeps the sketch open");
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), Some(1.0));
        cross(&mut h);
        assert_eq!(h.app.session.active_sketch, None, "with nothing unfinished it cancels the sketch");
        assert!(h.app.session.doc.features.is_empty());
    }

    #[test]
    fn a_refused_cancel_shows_its_error_and_keeps_the_sketch() {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"view_confirm_corner": true}));
        for (command, params) in [
            ("sketch.create", json!({"plane": "XY", "name": "S"})),
            ("sketch.line", json!({"points": [[0, 0], [10, 0]]})),
            ("sketch.finish", json!({})),
            ("sketch.edit", json!({"sketch": "S"})),
            ("sketch.line", json!({"points": [[0, 5], [10, 5]]})),
            ("edit.undo", json!({})),
            ("edit.undo", json!({})),
        ] {
            h.app.run(command, params).unwrap();
        }
        h.settle();
        let design = h.app.session.doc.to_json();
        h.step(&json!({"click": {"handle": "view:confirm:cancel"}})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_some(), "the refused cancel leaves the sketch open");
        assert_eq!(h.app.session.doc.to_json(), design);
        let (message, _, error) = h.app.status.clone().expect("the refusal is shown");
        assert!(error && message.contains("Undo"), "{message}");
    }

    #[test]
    fn a_dialog_in_the_sketch_takes_the_pair() {
        let mut h = sketching();
        h.app.start("sketch.offset");
        h.settle();
        assert!(h.app.dialog.is_some());
        assert_eq!(scenario::count_of("view:confirm:ok:enabled"), scenario::count_of("dialog:ok:enabled"));
        h.step(&json!({"click": {"handle": "view:confirm:cancel"}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "the cross cancels the dialog");
        assert!(h.app.session.active_sketch.is_some(), "and leaves the sketch open");
        assert!(scenario::rect_of("view:confirm").is_some(), "the pair answers the sketch again");
    }
}

/// The "When a sketch ends" preference (#7038): back to the view from before the sketch, or back
/// only when the sketch view was panned or zoomed and not turned. Keep, the default, is above.
mod exit_view_preference {
    use crate::prefs::SketchExitView;
    use crate::scenario::Harness;
    use serde_json::json;
    use solidcraft_viewport::Camera;

    /// A sketch on XY started from the iso view with `choice` set, and that iso view.
    fn sketching(choice: SketchExitView) -> (Harness, Camera) {
        let mut h = Harness::new();
        h.app.preferences.sketch_exit_view = choice;
        h.call("ui.view", json!({"view": "iso"}));
        h.settle();
        let iso = h.app.cam;
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        crate::scenario::check(&mut h, &json!({"sketching": true, "camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
        (h, iso)
    }

    fn pan_and_zoom(h: &mut Harness) {
        h.app.cam.pan(25.0, 15.0, 800.0);
        h.app.cam.zoom_at(0.5, None);
        h.app.cam_anim = None;
    }

    fn finish(h: &mut Harness) {
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
        h.app.finish_sketch();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None);
    }

    #[test]
    fn keep_is_the_default() {
        assert_eq!(crate::prefs::Prefs::default().sketch_exit_view, SketchExitView::Keep);
    }

    #[test]
    fn previous_goes_back_after_a_turn() {
        let (mut h, iso) = sketching(SketchExitView::Previous);
        h.app.cam.orbit(60.0, -40.0);
        pan_and_zoom(&mut h);
        finish(&mut h);
        assert_eq!(h.app.cam, iso);
    }

    #[test]
    fn unless_rotated_goes_back_after_only_panning_and_zooming() {
        let (mut h, iso) = sketching(SketchExitView::UnlessRotated);
        pan_and_zoom(&mut h);
        finish(&mut h);
        assert_eq!(h.app.cam, iso);
    }

    #[test]
    fn unless_rotated_keeps_a_turned_view() {
        let (mut h, _) = sketching(SketchExitView::UnlessRotated);
        h.app.cam.orbit(60.0, -40.0);
        pan_and_zoom(&mut h);
        let cam = h.app.cam;
        finish(&mut h);
        assert_eq!(h.app.cam, cam);
    }

    #[test]
    fn double_escape_and_cancel_go_back_too() {
        let (mut h, iso) = sketching(SketchExitView::Previous);
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam_anim = None;
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.cam, iso, "double Esc");

        let (mut h, iso) = sketching(SketchExitView::Previous);
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam_anim = None;
        h.app.cancel_sketch();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(h.app.cam, iso, "Cancel");
    }

    #[test]
    fn editing_a_sketch_goes_back_to_the_view_before_the_edit() {
        let mut app = crate::test_support::sample_app();
        app.preferences.sketch_exit_view = SketchExitView::Previous;
        let id = super::first_sketch(&app);
        let before = app.cam;
        app.edit_sketch(id);
        let facing = app.cam_anim.take().map(|a| a.to).unwrap();
        app.cam = facing;
        app.finish_sketch();
        assert_eq!(app.settled_cam(), before);
    }

    #[test]
    fn a_sketch_opened_by_a_command_saves_its_own_owner_camera() {
        let (mut h, _) = sketching(SketchExitView::Previous);
        // A programmatic finish clears this sketch's saved view too. A later sketch opened
        // entirely by commands saves its own entry view.
        h.app.run("sketch.finish", json!({})).unwrap();
        assert!(h.app.pre_sketch_cam.is_none());
        h.settle();
        let before = h.app.cam;
        h.app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam_anim = None;
        finish(&mut h);
        assert_eq!(h.app.cam, before, "programmatic starts follow the owner camera preference too");
    }

    #[test]
    fn going_back_keeps_the_current_projection() {
        let before = Camera::default();
        let facing = Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, ..before };
        let now = Camera { fov: 0.6, distance: 80.0, ..facing };
        for choice in [SketchExitView::Previous, SketchExitView::UnlessRotated] {
            assert_eq!(choice.view_after(before, facing, now), Some(Camera { fov: 0.6, ..before }), "{choice:?}");
        }
        assert_eq!(SketchExitView::Keep.view_after(before, facing, now), None);
        let turned = Camera { yaw: 0.2, ..now };
        assert_eq!(SketchExitView::UnlessRotated.view_after(before, facing, turned), None);
    }
}

/// Preferences › Design › Extrude's view (#9176): with the first preference on, the view turns to
/// isometric each time Extrude opens; with the second as well, it goes back to the view from
/// before when Extrude closes, by OK, Cancel or Esc. Both are off by default: the view stays.
mod extrude_view_preference {
    use crate::scenario::Harness;
    use serde_json::json;
    use solidcraft_viewport::{Camera, StandardView};

    /// A finished sketch on XY, seen from the top, with the two preferences set; and that view.
    fn from_the_top(isometric: bool, back: bool) -> (Harness, Camera) {
        let mut h = Harness::new();
        h.app.preferences.extrude_view_isometric = isometric;
        h.app.preferences.extrude_view_return = back;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        crate::scenario::check(&mut h, &json!({"camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
        let top = h.app.cam;
        (h, top)
    }

    fn open_extrude(h: &mut Harness) {
        h.step(&json!({"start": "solid.extrude"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_some(), "Extrude is open");
    }

    fn iso_of(cam: Camera) -> Camera {
        let mut iso = cam;
        iso.set_view(StandardView::Iso);
        iso
    }

    #[test]
    fn both_are_off_by_default_and_the_view_stays() {
        let prefs = crate::prefs::Prefs::default();
        assert!(!prefs.extrude_view_isometric && !prefs.extrude_view_return);
        let (mut h, top) = from_the_top(false, false);
        open_extrude(&mut h);
        assert_eq!(h.app.cam, top);
        // The second one alone does nothing either.
        let (mut h, top) = from_the_top(false, true);
        open_extrude(&mut h);
        assert_eq!(h.app.cam, top);
    }

    #[test]
    fn extrude_turns_the_view_to_isometric_and_leaves_it_there() {
        let (mut h, top) = from_the_top(true, false);
        open_extrude(&mut h);
        assert_eq!(h.app.cam, iso_of(top), "isometric when Extrude opens");
        h.step(&json!({"widget": "Cancel"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.cam, iso_of(top), "without the second preference it stays isometric");
    }

    #[test]
    fn ok_cancel_and_escape_go_back_to_the_view_from_before() {
        for close in ["OK", "Cancel", "Escape"] {
            let (mut h, top) = from_the_top(true, true);
            open_extrude(&mut h);
            assert_eq!(h.app.cam, iso_of(top), "{close}: isometric while Extrude is open");
            match close {
                "Escape" => h.step(&json!({"key": "Escape"})).unwrap(),
                other => h.step(&json!({"widget": other})).unwrap(),
            };
            h.settle();
            assert!(h.app.dialog.is_none(), "{close}");
            assert_eq!(h.app.session.doc.features.len(), if close == "OK" { 2 } else { 1 }, "{close}: OK adds the extrude");
            assert_eq!(h.app.cam, top, "{close}: back to the view from before");
            // Each time it opens, not only the first.
            open_extrude(&mut h);
            assert_eq!(h.app.cam, iso_of(top), "{close}: isometric again");
        }
    }

    /// #10571: Extrude clicked with the sketch still open ends the sketch on its way, and that
    /// end turns the view back where it opened (Preferences › Sketch). The turn must not become
    /// the view Extrude goes back to: it is the view the command was clicked in.
    #[test]
    fn extrude_from_an_open_sketch_goes_back_to_the_view_it_was_clicked_in() {
        let mut h = Harness::new();
        h.app.preferences.extrude_view_isometric = true;
        h.app.preferences.extrude_view_return = true;
        h.app.preferences.sketch_exit_view = crate::prefs::SketchExitView::Previous;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        let top = h.app.cam;
        open_extrude(&mut h);
        assert_eq!(h.app.cam, iso_of(top), "isometric while Extrude is open");
        h.step(&json!({"widget": "Cancel"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.active_sketch, None, "Extrude ended the sketch");
        assert_eq!(h.app.cam, top, "back to the sketch view it was clicked in");
    }
}

/// #14128: Extrude clicked while an edited sketch is still open ends the sketch on its way. The
/// sketch is already used by an earlier extrude, so once it is finished it is hidden, and its new
/// rectangle could not be picked: the Extrude dialog showed no profiles to click. The sketch stays
/// shown while the Extrude it started is open, a click inside the new rectangle picks it, and OK
/// builds a solid from it, the same as after Finish Sketch. Closing the dialog hides it again.
#[test]
fn extrude_from_an_open_used_sketch_picks_its_new_profile() {
    use serde_json::json;
    for close in ["OK", "Cancel"] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        // The rectangle by its curves, as the Extrude dialog stores it: the new one stays a profile.
        h.app.run("solid.extrude", json!({"distance": 10, "profiles": [0]})).unwrap();
        assert!(!solidcraft_engine::view::sketch_shown(&h.app.session, sketch), "{close}: the extrude hides the sketch it uses");
        h.app.edit_feature(sketch);
        h.settle();
        assert_eq!(h.app.session.active_sketch, Some(sketch), "{close}");
        h.app.run("sketch.rectangle.two_point", json!({"p0": [60, 0], "p1": [80, 30]})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        let features = h.app.session.doc.features.len();
        h.step(&json!({"start": "solid.extrude"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, None, "{close}: Extrude ended the sketch");
        assert!(h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Extrude { .. })), "{close}: Extrude is open");
        assert!(solidcraft_engine::view::sketch_shown(&h.app.session, sketch), "{close}: the sketch stays shown while its Extrude is open");
        h.step(&json!({"click": {"world": [70, 15, 0]}})).unwrap();
        h.settle();
        let picked = h.app.dialog.as_ref().and_then(|d| d.inputs.first()).map(|i| i.items.clone()).unwrap_or_default();
        let new_profile = crate::open_meshing::session_world(&h.app.session)
            .sketch(sketch)
            .and_then(|ss| ss.profiles.iter().position(|p| p.contains(solidcore_record::Vec2::new(70.0, 15.0))))
            .unwrap();
        assert_eq!(picked, vec![solidcraft_engine::Sel::Profile { sketch, index: new_profile }], "{close}: the click picks the new rectangle");
        h.step(&json!({"widget": close})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{close}");
        assert!(!solidcraft_engine::view::sketch_shown(&h.app.session, sketch), "{close}: hidden again once the dialog closes");
        if close == "Cancel" {
            assert_eq!(h.app.session.doc.features.len(), features, "Cancel adds nothing");
            continue;
        }
        assert_eq!(h.app.session.doc.features.len(), features + 1, "OK adds the extrude");
        let Some(FeatureKind::Extrude { thin, .. }) = h.app.session.doc.features.last().map(|f| &f.kind) else { panic!("an extrude") };
        assert!(thin.is_none(), "a solid from the profile, not a thin wall along its lines");
        let volume = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        assert!((volume - (40.0 * 30.0 * 10.0 + 20.0 * 30.0 * 10.0)).abs() < 1e-6, "both blocks are solid: {volume}");
    }
}

/// #8127: a sketch inside an inserted fastener, opened from its row in the browser, activates the
/// fastener's component while it is open; Finish and Cancel make the design active again, so the
/// other bodies are no longer drawn faded. Editing it with the fastener already active leaves the
/// fastener active, and so does activating another component while the sketch is open.
#[test]
fn a_sketch_opened_inside_a_component_gives_the_active_component_back() {
    let mut app = sample_app();
    let bolt = app.run("parts.insert", serde_json::json!({"family": "socket_head_cap_screw", "size": "M6", "point": [0, 0, 0]})).unwrap();
    let comp = bolt["component"].as_u64().unwrap();
    let path = app.session.doc.first_path(comp);
    let sketch = app.session.doc.features.iter().find(|f| f.component == comp && matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap();
    let marker = app.session.doc.marker;
    for leave in [SolveApp::finish_sketch as fn(&mut SolveApp), SolveApp::cancel_sketch] {
        app.edit_sketch_in(sketch, Some((comp, &path)));
        assert_eq!(
            (app.session.active_sketch, app.session.active_component),
            (Some(sketch), comp),
            "the fastener is active while its sketch is open"
        );
        leave(&mut app);
        assert_eq!((app.session.active_sketch, app.session.active_component), (None, 0), "the design is active again");
        assert_eq!(app.session.active_occurrence_path, None);
        assert_eq!(app.session.doc.marker, marker);
    }
    // Switching to a sketch of the design from the fastener's leaves nothing of the fastener active.
    app.edit_sketch_in(sketch, Some((comp, &path)));
    app.edit_sketch(first_sketch(&app));
    assert_eq!(app.session.active_component, 0);
    app.finish_sketch();
    assert_eq!(app.session.active_component, 0);
    // The fastener was active already: it stays active.
    app.run("component.activate", serde_json::json!({"component": comp})).unwrap();
    app.edit_sketch_in(sketch, Some((comp, &path)));
    app.finish_sketch();
    assert_eq!(app.session.active_component, comp);
    // Another component activated while the sketch is open stays active.
    app.run("component.activate", serde_json::json!({"component": 0})).unwrap();
    let washer = app.run("parts.insert", serde_json::json!({"family": "washer", "size": "M6", "point": [20, 0, 0]})).unwrap();
    let washer = washer["component"].as_u64().unwrap();
    app.edit_sketch_in(sketch, Some((comp, &path)));
    app.run("component.activate", serde_json::json!({"component": washer})).unwrap();
    app.finish_sketch();
    assert_eq!(app.session.active_component, washer, "the component activated meanwhile stays active");
}

/// "Turn to face a sketch when it opens": off, creating or editing a sketch leaves the view as it
/// is, and Look At still faces the sketch when asked.
mod open_view_preference {
    use crate::scenario::Harness;
    use serde_json::json;

    #[test]
    fn turning_is_the_default() {
        assert!(crate::prefs::Prefs::default().sketch_open_look_at);
    }

    #[test]
    fn off_a_new_sketch_keeps_the_view_and_look_at_still_turns() {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"sketch_open_look_at": false}));
        assert!(!h.app.preferences.sketch_open_look_at, "set through the control channel");
        h.call("ui.view", json!({"view": "iso"}));
        h.settle();
        let iso = h.app.cam;
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_some());
        assert_eq!(h.app.cam, iso, "the view stays where it was");
        crate::dialogs::look_at_sketch_again(&mut h.app);
        h.settle();
        crate::scenario::check(&mut h, &json!({"sketching": true, "camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
    }

    #[test]
    fn off_editing_a_sketch_keeps_the_view() {
        let mut app = crate::test_support::sample_app();
        app.preferences.sketch_open_look_at = false;
        let id = super::first_sketch(&app);
        let before = app.cam;
        app.edit_sketch(id);
        assert!(app.session.active_sketch.is_some());
        assert_eq!(app.settled_cam(), before);
    }
}

#[test]
fn sketch_mode_belongs_to_one_tile_and_focus_never_finishes_it() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.preferences.look_at_anim = false;
    app.run("tab.view.layout", json!({"layout":"side_by_side"})).unwrap();
    let owner = app.docs.views.active().id;
    let other = app.docs.views.ids().find(|id| *id != owner).unwrap();
    let reply = app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let sketch = reply["sketch"].as_u64().unwrap();
    assert_eq!(reply["sketching_view"], owner);
    assert_eq!(app.docs.views.get(owner).unwrap().editing.unwrap().sketch, sketch);
    assert_eq!(crate::sketch_view::active_sketch(&app), Some(sketch));
    assert!(crate::documents::focus_view(&mut app, other));
    assert_eq!(app.session.active_sketch, Some(sketch));
    assert_eq!(app.session.sketching_view, Some(owner));
    assert_eq!(crate::sketch_view::active_sketch(&app), None);
    crate::documents::with_view_look(&mut app, owner, |app| {
        assert_eq!(crate::sketch_view::active_sketch(app), Some(sketch));
    });
    assert_eq!(app.session.active_sketch, Some(sketch), "painting a passive tile never clears the receiver");
}

#[test]
fn sketch_camera_round_trip_changes_only_its_owner_even_with_linked_cameras() {
    use crate::prefs::SketchExitView;
    use serde_json::json;
    for choice in [SketchExitView::Keep, SketchExitView::Previous, SketchExitView::UnlessRotated] {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.look_at_anim = false;
        app.preferences.sketch_exit_view = choice;
        app.run("tab.view.layout", json!({"layout":"side_by_side"})).unwrap();
        let owner = app.docs.views.active().id;
        let other = app.docs.views.ids().find(|id| *id != owner).unwrap();
        let before = app.cam;
        let other_before = app.docs.views.get(other).unwrap().spec;
        app.docs.views.set_camera_link(solidcraft_views::CameraLinkMode::Both);
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let facing = app.settled_cam();
        crate::documents::sync_linked_active(&mut app);
        assert_eq!(app.docs.views.get(other).unwrap().spec, other_before);
        assert!(crate::documents::focus_view(&mut app, other));
        let passive = app.cam;
        app.run("sketch.finish", json!({})).unwrap();
        assert_eq!(app.cam, passive, "finishing from another tile preserves its camera");
        let owner_after = app.docs.views.get(owner).unwrap().spec.unwrap().camera;
        let expected = choice.view_after(before, facing, facing).unwrap_or(facing);
        assert_eq!(owner_after, expected);
        assert!(app.docs.views.views().iter().all(|view| view.editing.is_none()));
    }
}

#[test]
fn model_sketch_fallback_preserves_render_and_sheet_ground_views() {
    use solidcraft_views::ViewKind;
    for kind in [ViewKind::Render, ViewKind::Sheet] {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let first = app.docs.views.active().id;
        let ground = app.docs.views.open(kind, (kind == ViewKind::Sheet).then(|| "sheet".into()), false);
        assert!(app.docs.views.close(first));
        if kind == ViewKind::Sheet {
            let before = app.docs.views.to_json();
            assert!(crate::sketch_view::prepare_start(&mut app, None).is_err());
            assert_eq!(app.docs.views.to_json(), before);
            assert_eq!(app.session.sketching_view, None);
            continue;
        }
        let target = crate::sketch_view::prepare_start(&mut app, None).unwrap();
        assert_ne!(target, ground);
        assert_eq!(app.docs.views.active().id, ground);
        assert_eq!(app.docs.views.get(ground).unwrap().kind, kind);
        assert_eq!(app.docs.views.get(target).unwrap().kind, ViewKind::Model);
        if kind == ViewKind::Render {
            assert!(app.docs.views.tiles().unwrap().contains(target));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn moving_a_sketching_view_to_another_window_keeps_its_owner_and_camera_return() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.preferences.look_at_anim = false;
    app.preferences.sketch_exit_view = crate::prefs::SketchExitView::Previous;
    let owner = app.docs.views.active().id;
    crate::documents::view_duplicate(&mut app, owner).unwrap();
    crate::documents::focus_view(&mut app, owner);
    let before = app.cam;
    app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    app.run("tab.view.move_to_window", json!({"view":owner})).unwrap();
    let window = app.docs.views.window_of(owner).unwrap();
    assert_ne!(window, 0);
    assert_eq!(app.session.sketching_view, Some(owner));
    assert!(app.docs.views.views_of(window).unwrap().get(owner).unwrap().editing.is_some());
    let main_camera = app.cam;
    app.run("sketch.finish", json!({})).unwrap();
    assert_eq!(app.cam, main_camera);
    assert_eq!(app.docs.views.views_of(window).unwrap().get(owner).unwrap().spec.unwrap().camera, before);
}

#[test]
fn closing_the_sketch_owner_finishes_before_removing_its_runtime_identity() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let owner = app.docs.views.active().id;
    crate::documents::view_duplicate(&mut app, owner).unwrap();
    crate::documents::focus_view(&mut app, owner);
    app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    assert!(crate::documents::view_close(&mut app, owner));
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.sketching_view, None);
    crate::sketch_view::sync(&mut app).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn starting_a_sketch_in_another_window_changes_only_the_destination_camera() {
    use serde_json::json;
    use solidcraft_viewport::{Camera, ViewSpec};
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.preferences.look_at_anim = false;
    app.preferences.sketch_exit_view = crate::prefs::SketchExitView::Previous;
    let caller = app.docs.views.active().id;
    let owner = crate::documents::view_duplicate(&mut app, caller).unwrap();
    app.run("tab.view.move_to_window", json!({"view":owner})).unwrap();
    let window = app.docs.views.window_of(owner).unwrap();
    let before = Camera { yaw: 0.7, pitch: 0.4, distance: 80.0, ..app.cam };
    app.docs.views.window_mut(window).unwrap().views.set_spec(owner, ViewSpec::new(before));
    let caller_camera = app.cam;
    let reply = app.run("sketch.create", json!({"plane":"XY","view":owner})).unwrap();
    assert_eq!(reply["sketching_view"], owner);
    assert_eq!(app.docs.views.active().id, caller);
    assert_eq!(app.cam, caller_camera);
    let destination = app.docs.views.views_of(window).unwrap().get(owner).unwrap().spec.unwrap().camera;
    assert_ne!(destination, before, "the destination enters the sketch plane");
    assert_eq!(app.pre_sketch_cam.unwrap().before, before);
    assert_eq!(app.pre_sketch_cam.unwrap().view, owner);
    app.run("sketch.finish", json!({})).unwrap();
    assert_eq!(app.cam, caller_camera);
    assert_eq!(app.docs.views.views_of(window).unwrap().get(owner).unwrap().spec.unwrap().camera, before);
}

#[test]
fn a_full_view_set_refuses_an_invisible_sketch_fallback_atomically() {
    use solidcraft_views::ViewKind;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let first = app.docs.views.active().id;
    let ground = app.docs.views.open(ViewKind::Render, None, false);
    assert!(app.docs.views.close(first));
    for _ in 1..solidcraft_views::MAX_VIEWS {
        app.docs.views.open(ViewKind::Render, None, false);
    }
    app.docs.views.activate(ground);
    let before = app.docs.views.to_json();
    assert!(crate::sketch_view::prepare_start(&mut app, None).is_err());
    assert_eq!(app.docs.views.to_json(), before);
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.session.sketching_view, None);
}

#[test]
fn malformed_explicit_sketch_view_is_never_replaced_by_a_fallback() {
    use serde_json::json;
    use solidcraft_engine::schema::ParamCheck;
    for check in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        for view in [json!("1"), json!(-1), json!(1.5)] {
            let mut session = solidcraft_engine::Session::default();
            session.param_check = check;
            let mut app = SolveApp::new(session, Default::default());
            let before = app.docs.views.to_json();
            let document = app.session.doc.to_json();
            assert!(app.run("sketch.create", json!({"plane":"XY", "view":view})).is_err());
            assert_eq!(app.docs.views.to_json(), before);
            assert_eq!(app.session.doc.to_json(), document);
            assert_eq!(app.session.active_sketch, None);
        }
    }
}

#[test]
fn render_grid_fallback_fills_an_empty_cell_without_moving_the_ground() {
    use solidcraft_views::{Preset, ViewKind};
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let first = app.docs.views.active().id;
    let ground = app.docs.views.open(ViewKind::Render, None, false);
    assert!(app.docs.views.close(first));
    app.docs.views.set_layout_padded(Preset::Grid);
    let target = crate::sketch_view::prepare_start(&mut app, None).unwrap();
    assert_eq!(app.docs.views.active().id, ground);
    assert_eq!(app.docs.views.tiles().unwrap().len(), 4);
    assert!(app.docs.views.tiles().unwrap().contains(target));
    assert!(app.docs.views.tiles().unwrap().contains(ground));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn refused_new_edit_finishes_and_notifies_the_original_window_exactly_once() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.preferences.look_at_anim = false;
    app.preferences.sketch_exit_view = crate::prefs::SketchExitView::Previous;
    let caller = app.docs.views.active().id;
    let owner = crate::documents::view_duplicate(&mut app, caller).unwrap();
    app.run("tab.view.move_to_window", json!({"view":owner})).unwrap();
    let window = app.docs.views.window_of(owner).unwrap();
    let before = app.docs.views.views_of(window).unwrap().get(owner).unwrap().spec.unwrap().camera;
    let sketch = app.run("sketch.create", json!({"plane":"XY","view":owner})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    let caller_camera = app.cam;
    assert!(app.run("solid.extrude", json!({"distance":0})).is_err());
    assert_eq!((app.session.active_sketch, app.session.sketching_view), (None, None));
    assert_eq!(app.cam, caller_camera);
    assert_eq!(app.docs.views.views_of(window).unwrap().get(owner).unwrap().spec.unwrap().camera, before);
    assert!(app.sketch_notices.latest_for(app.session.uid, 0).is_none());
    let delivered = app.sketch_notices.latest_for(app.session.uid, window).unwrap().clone();
    assert_eq!(delivered.sketch, sketch);
    assert_eq!(delivered.view, Some(owner));
    app.cam.yaw += 0.2;
    let moved = app.cam;
    crate::sketch_view::consume_finishes(&mut app);
    crate::sketch_view::consume_finishes(&mut app);
    assert_eq!(app.cam, moved, "draining again cannot replay camera restoration");
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, window).unwrap(), &delivered);
}

#[test]
fn a_finished_sketch_notice_survives_its_dialog_commit_then_dismisses_on_the_next_edit() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    app.preferences.look_at_anim = false;
    let first = app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let later = app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0":[40,0],"p1":[50,10]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.edit_sketch(first);
    app.start("solid.extrude");
    assert_eq!(app.session.active_sketch, None);
    let dialog = app.dialog.take().unwrap();
    assert_eq!(dialog.started_from_sketch, Some(first), "the earlier editor's source wins over the last sketch");
    assert_ne!(first, later);
    let event = app.sketch_notices.latest_for(app.session.uid, 0).unwrap().clone();
    app.run("select.clear", json!({})).unwrap();
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, 0).unwrap(), &event);
    app.run("solid.extrude", json!({"sketch":first,"distance":5})).unwrap();
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, 0).unwrap(), &event, "submitting the same already-open editor is not a second edit");
    app.run("solid.box", json!({"length":2,"width":3,"height":4})).unwrap();
    assert!(app.sketch_notices.latest_for(app.session.uid, 0).is_none());
}

#[test]
fn accepted_worker_events_are_presented_once_and_rejected_scratch_events_never_escape() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let sketch = app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    let bad = vec![("solid.box".into(), json!({"length":2,"width":3,"height":4})), ("solid.box".into(), json!({"length":-1}))];
    assert!(app.session.scratch().compute_commands(&bad, None).is_err());
    assert!(app.command_completed("plugin.run", Err("transaction refused".into())).is_err());
    assert_eq!(app.session.active_sketch, Some(sketch));
    assert!(app.sketch_notices.latest_for(app.session.uid, 0).is_none());
    let commands = vec![("solid.box".into(), json!({"length":2,"width":3,"height":4}))];
    let prepared = app.session.scratch().compute_commands(&commands, None).unwrap();
    app.session.accept_commands(prepared).unwrap();
    app.command_completed("plugin.run", Ok(json!({}))).unwrap();
    let event = app.sketch_notices.latest_for(app.session.uid, 0).unwrap().clone();
    assert_eq!(event.sketch, sketch);
    app.command_completed("plugin.run", Ok(json!({}))).unwrap();
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, 0).unwrap(), &event);
    assert!(app.session.take_sketch_finish_event().is_none());
}

#[test]
fn malformed_new_sketch_target_keeps_the_previous_sketch_finished() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let first = app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    assert!(app.run("sketch.create", json!({"plane":"XY","view":"invalid"})).is_err());
    assert_eq!(app.session.active_sketch, None);
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, 0).unwrap().sketch, first);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn closing_local_siblings_never_finishes_a_sketch_in_another_window() {
    use serde_json::json;
    for right_only in [false, true] {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let first = app.docs.views.active().id;
        let owner = crate::documents::view_duplicate(&mut app, first).unwrap();
        let local = crate::documents::view_duplicate(&mut app, first).unwrap();
        app.run("tab.view.move_to_window", json!({"view":owner})).unwrap();
        let window = app.docs.views.window_of(owner).unwrap();
        let sketch = app.run("sketch.create", json!({"plane":"XY","view":owner})).unwrap()["sketch"].as_u64().unwrap();
        if right_only {
            crate::documents::view_close_right_of(&mut app, first);
        } else {
            crate::documents::view_close_others(&mut app, first);
        }
        assert!(app.docs.views.get(local).is_none());
        assert!(app.docs.views.views_of(window).unwrap().get(owner).is_some());
        assert_eq!((app.session.active_sketch, app.session.sketching_view), (Some(sketch), Some(owner)));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn changing_an_unfocused_window_layout_keeps_the_live_sketch_registry_and_focus() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let owner = app.docs.views.active().id;
    let other = crate::documents::view_duplicate(&mut app, owner).unwrap();
    app.run("tab.view.move_to_window", json!({"view":other})).unwrap();
    let window = app.docs.views.window_of(other).unwrap();
    app.run("tab.view.activate", json!({"id":owner,"window":0})).unwrap();
    let sketch = app.run("sketch.create", json!({"plane":"XY","view":owner})).unwrap()["sketch"].as_u64().unwrap();
    let focused = app.session.view_context.focused();
    assert_eq!(crate::view_windows::focused_view(&app), focused);
    let recent = app.session.view_context.recent().to_vec();
    app.run("tab.view.layout", json!({"window":window,"layout":"side_by_side"})).unwrap();
    assert_eq!((app.session.active_sketch, app.session.sketching_view), (Some(sketch), Some(owner)));
    assert_eq!(app.session.view_context.focused(), focused);
    assert_eq!(crate::view_windows::focused_view(&app), focused, "layout's internal activation must not change committed native focus");
    assert_eq!(app.session.view_context.recent(), recent);
    assert!(app.session.view_context.descriptors().iter().any(|view| view.id == owner));
    assert!(app.docs.views.views_of(window).unwrap().is_tiled());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn reopening_a_notice_follows_its_moved_owner_even_from_a_foreign_child() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let owner = app.docs.views.active().id;
    let other = crate::documents::view_duplicate(&mut app, owner).unwrap();
    let sketch = app.run("sketch.create", json!({"plane":"XY", "view":owner})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    assert!(app.run("solid.extrude", json!({"distance":0})).is_err());
    let event = app.sketch_notices.latest_for(app.session.uid, 0).unwrap().clone();
    app.run("tab.view.move_to_window", json!({"view":other})).unwrap();
    let child = app.docs.views.window_of(other).unwrap();
    crate::view_windows::with_strip(&mut app, Some(child), |app| {
        crate::sketch_view::reopen_finished(app, event.clone());
        assert_eq!(app.session.sketching_view, Some(owner), "queued foreign focus must not retarget the edit to this child");
        Ok(())
    })
    .unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    let sentinel = crate::documents::view_duplicate(&mut app, owner).unwrap();
    assert_eq!(app.docs.views.window_of(sentinel), Some(0), "keep a Model view in Main while its original owner moves");
    app.run("tab.view.move_to_window", json!({"view":owner})).unwrap();
    crate::sketch_view::reopen_finished(&mut app, event);
    assert_eq!(app.session.sketching_view, Some(owner), "the owner ID follows a moved view");
    assert_eq!(app.session.active_sketch, Some(sketch));
}

#[test]
fn reopening_a_notice_with_a_closed_owner_resolves_a_live_model_view() {
    use serde_json::json;
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let owner = app.docs.views.active().id;
    let fallback = crate::documents::view_duplicate(&mut app, owner).unwrap();
    let sketch = app.run("sketch.create", json!({"plane":"XY", "view":owner})).unwrap()["sketch"].as_u64().unwrap();
    app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    assert!(crate::documents::view_close(&mut app, owner));
    let event = app.sketch_notices.latest_for(app.session.uid, 0).unwrap().clone();
    crate::sketch_view::reopen_finished(&mut app, event);
    assert_eq!(app.session.sketching_view, Some(fallback));
    assert_eq!(app.session.active_sketch, Some(sketch));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn closing_an_earlier_sketch_refreshes_the_restored_model_without_another_tool() {
    use serde_json::json;
    let mut h = plate_and_cylinder();
    let sketch = first_sketch(&h.app);
    let original = h.app.docs.views.active().id;
    let duplicate = crate::documents::view_duplicate(&mut h.app, original).unwrap();
    h.app.edit_sketch(sketch);
    let owner = h.app.session.sketching_view.unwrap();
    assert_eq!(owner, duplicate, "the editor starts in the focused duplicate");
    h.app.run("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[10,10]})).unwrap();
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    assert!(crate::documents::view_close(&mut h.app, owner));
    assert_eq!(h.app.session.doc.marker, None);
    assert!(h.app.session.needs_sketch_finish_evaluation());
    assert!(h.app.deferred.is_none(), "catch-up never synthesizes a history command");
    assert!(h.app.start_after_roll.is_none(), "closing the owner starts no next tool");
    let uid = h.app.session.uid;
    let job = h.app.sketch_refreshes.remove(&uid).unwrap().completed();
    h.app.sketch_refreshes.insert(uid, job);
    crate::sketch_view::poll_refreshes(&mut h.app, &h.ctx);
    assert!(!h.app.session.needs_sketch_finish_evaluation());
    assert_eq!(h.app.session.model.state().bodies.len(), 2, "the complete suffix is restored by cache adoption");
    assert!(h.app.sketch_refreshes.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_refused_drawing_tool_refreshes_a_finished_model_sketch_without_a_history_command() {
    use serde_json::json;
    let mut h = plate_and_cylinder();
    let sketch = first_sketch(&h.app);
    h.app.edit_sketch(sketch);
    h.app.run("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[10,10]})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    assert_eq!(h.app.session.active_sketch, Some(sketch));
    let drawing = h.app.session.open_drawing;
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    assert!(h.app.run("drawing.note.add", json!({})).is_err());
    assert_eq!(h.app.session.active_sketch, None);
    assert_eq!(h.app.session.doc.marker, None);
    assert_eq!(h.app.session.open_drawing, drawing);
    assert!(h.app.session.needs_sketch_finish_evaluation());
    assert!(h.app.deferred.is_none());
    assert!(h.app.start_after_roll.is_none());
    let uid = h.app.session.uid;
    let job = h.app.sketch_refreshes.remove(&uid).unwrap().completed();
    h.app.sketch_refreshes.insert(uid, job);
    crate::sketch_view::poll_refreshes(&mut h.app, &h.ctx);
    assert!(!h.app.session.needs_sketch_finish_evaluation());
    assert_eq!(h.app.session.model.state().bodies.len(), 2);
    assert_eq!(h.app.session.open_drawing, drawing, "model catch-up keeps Drawing focus");
    assert!(h.app.sketch_notices.latest_for(uid, 0).is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_new_edit_supersedes_a_tool_waiting_for_sketch_model_catch_up() {
    use serde_json::json;
    let mut h = plate_and_cylinder();
    let sketch = first_sketch(&h.app);
    h.app.edit_sketch(sketch);
    h.app.run("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[10,10]})).unwrap();
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    h.app.start("solid.extrude");
    assert!(h.app.start_after_roll.is_some());
    let fresh = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    assert_ne!(fresh, sketch);
    assert!(h.app.start_after_roll.is_none(), "the previous tool cannot open after a newer edit starts");
    let uid = h.app.session.uid;
    let job = h.app.sketch_refreshes.remove(&uid).unwrap().completed();
    h.app.sketch_refreshes.insert(uid, job);
    h.app.poll_deferred(&h.ctx);
    assert_eq!(h.app.session.active_sketch, Some(fresh));
    assert!(h.app.dialog.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopped_sketch_catch_up_rejects_even_an_already_finished_worker_result() {
    use serde_json::json;
    let mut h = plate_and_cylinder();
    let sketch = first_sketch(&h.app);
    let original = h.app.docs.views.active().id;
    let duplicate = crate::documents::view_duplicate(&mut h.app, original).unwrap();
    h.app.edit_sketch(sketch);
    let owner = h.app.session.sketching_view.unwrap();
    assert_eq!(owner, duplicate, "the editor starts in the focused duplicate");
    h.app.run("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[10,10]})).unwrap();
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    assert!(crate::documents::view_close(&mut h.app, owner));
    let uid = h.app.session.uid;
    let job = h.app.sketch_refreshes.remove(&uid).unwrap().completed();
    let progress = job.progress.clone();
    h.app.sketch_refreshes.insert(uid, job);
    let before = h.app.session.model.state();
    crate::operations::cancel(&mut h.app, "sketch_refresh").unwrap();
    assert!(progress.is_cancelled());
    crate::sketch_view::poll_refreshes(&mut h.app, &h.ctx);
    assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.model.state()), "Stop wins over an already queued successful result");
    assert!(h.app.session.needs_sketch_finish_evaluation());
    assert!(h.app.sketch_refreshes.is_empty(), "Stop does not immediately restart the same rebuild");
    assert!(!h.app.session.stop_token().is_stopped(), "stopping one refresh leaves the document usable");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sketch_model_refreshes_are_bounded_and_current_design_gets_the_first_slot() {
    use serde_json::json;
    fn pending() -> solidcraft_engine::Session {
        let mut session = solidcraft_engine::Session::default();
        session.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        session.prepare_edit_start("solid.box", &json!({})).unwrap();
        assert!(session.needs_sketch_finish_evaluation());
        session
    }
    let mut app = SolveApp::new(pending(), Default::default());
    app.apply_off_thread = Some(std::time::Duration::ZERO);
    for _ in 0..5 {
        crate::documents::open_tab_behind(&mut app, pending()).unwrap();
    }
    let uid = app.session.uid;
    let (job, _held) = crate::sketch_view::ModelRefresh::held();
    app.sketch_refreshes.insert(uid, job);
    crate::sketch_view::poll_refreshes(&mut app, &egui::Context::default());
    assert_eq!(app.sketch_refreshes.len(), 2, "the other pending documents remain queued");
    assert!(app.sketch_refreshes.contains_key(&uid), "current document owns a slot before backgrounds are considered");
    assert!(app.docs.background().filter(|session| session.needs_sketch_finish_evaluation()).count() >= 4);
}

#[test]
fn drawing_toolbar_entry_finishes_a_sheet_sketch_started_through_the_engine() {
    use serde_json::json;
    let mut app = SolveApp::new(Default::default(), Default::default());
    app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    let made = app.run("drawing.create", json!({})).unwrap();
    crate::workspace::enter(&mut app, "drawing").unwrap();
    let host = app.docs.views.active().clone();
    assert_eq!(host.kind, solidcraft_views::ViewKind::Sheet);
    assert_eq!(host.subject, Some(format!("{}/{}", made["id"].as_str().unwrap(), made["sheets"][0]["id"].as_str().unwrap())));
    assert_eq!(app.session.view_context.focused(), host.id);
    assert!(!app.session.model_focus, "the toolbar fixture commits a real Sheet receiver");
    let entry = app.session.execute("drawing.sketch.create", &json!({"owner":made["sheets"][0]["id"]})).unwrap();
    let sketch = solidcraft_engine::doc::ids::decode(entry["sketch"].as_str().unwrap()).unwrap();
    app.session.execute("sketch.line", &json!({"points":[[10,10],[20,10]],"infer":false})).unwrap();
    assert!(app.session.active_sheet_sketch.is_some());
    app.start("drawing.note");
    assert!(app.session.active_sheet_sketch.is_none());
    assert!(crate::drawing_workspace::placing_note(&app));
    assert_eq!(app.docs.views.active().id, host.id, "the Drawing toolbar keeps its committed Sheet host");
    assert_eq!(app.sketch_notices.latest_for(app.session.uid, 0).unwrap().sketch, sketch);
}

#[test]
fn sketch_finish_notice_uses_window_safe_area_instead_of_the_last_ground_tile() {
    use egui::{Pos2, RawInput, Rect, pos2, vec2};
    use serde_json::json;
    let mut app = SolveApp::new(Default::default(), Default::default());
    app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    assert!(app.run("solid.extrude", json!({"distance":0})).is_err());
    app.preferences.navigation_bar.edge = crate::ui_kit::NavigationEdge::Bottom;
    let t = solidcraft_theme::Tokens::get();
    let navigation_height = crate::ui_kit::NavigationMetrics::for_size(app.preferences.navigation_bar.size).height();
    let navigation_clearance = navigation_height + crate::ui_kit::NAV_BOTTOM + t.space_8;
    let ctx = crate::fonts::test_context();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
    let handle = format!("sketch:finished:{}:0", app.session.uid);
    for bottom_obstacle in [false, true] {
        let mut previous = None;
        for right_ground in [false, true] {
            app.viewport.window = None;
            app.viewport.rect =
                Some(Rect::from_min_max(pos2(if right_ground { 650.0 } else { 0.0 }, 150.0), pos2(if right_ground { 1200.0 } else { 550.0 }, 700.0)));
            let mut host = Rect::NOTHING;
            let _ = ctx.run_ui(RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                host = ui.max_rect();
                // A tall right editor ends above the bottom notice band. It must not shift
                // a window-centred notice left, regardless of the last ground tile.
                let mut obstacles =
                    vec![Rect::from_min_max(pos2(host.right() - 320.0, host.top() + 100.0), pos2(host.right(), host.bottom() - 150.0))];
                // The left tile's real navigation row is inset from the host by space_8.
                let navigation_top = host.bottom() - t.space_8 - crate::ui_kit::NAV_BOTTOM - navigation_height;
                obstacles.push(Rect::from_min_size(pos2(host.left() + t.space_8, navigation_top), vec2(300.0, navigation_height)));
                if bottom_obstacle {
                    obstacles.push(Rect::from_min_max(pos2(host.left(), host.bottom() - 80.0), host.max));
                }
                app.viewport.host_windows = Some(obstacles);
                crate::sketch_view::show_notice(&mut app, ui);
            });
            let notice = crate::scenario::rect_of(&handle).unwrap();
            assert!(
                (notice.center().x - host.center().x).abs() < 0.5,
                "notice centres in the window, independent of left/right ground: {notice:?} in {host:?}"
            );
            let bottom = host.bottom() - navigation_clearance.max(if bottom_obstacle { 80.0 } else { 0.0 }) - t.space_8;
            assert!((notice.bottom() - bottom).abs() < 0.5, "notice stays above navigation and bottom obstacles");
            let navigation_top = host.bottom() - t.space_8 - crate::ui_kit::NAV_BOTTOM - navigation_height;
            assert!(notice.bottom() + t.space_8 <= navigation_top + 0.5, "notice clears the navigation row");
            assert!(host.contains_rect(notice));
            if let Some(previous) = previous {
                assert_eq!(notice, previous, "changing only the ground tile cannot move a window notice");
            }
            previous = Some(notice);
        }
    }
}
