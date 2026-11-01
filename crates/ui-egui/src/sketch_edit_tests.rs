//! Regression tests: editing a sketch from the Browser or timeline can always be left again.

use crate::SolveApp;
use crate::test_support::sample_app;
use solidcraft_engine::doc::FeatureKind;

fn first_sketch(app: &SolveApp) -> u64 {
    app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).unwrap()
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
    use solidcraft_engine::render::Camera;

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
    use solidcraft_engine::render::Camera;

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
    fn a_sketch_opened_by_a_command_leaves_the_view_alone() {
        let (mut h, _) = sketching(SketchExitView::Previous);
        // Finished by the command, so the views saved for this sketch stay behind.
        h.app.run("sketch.finish", json!({})).unwrap();
        assert!(h.app.pre_sketch_cam.is_some());
        h.app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
        h.app.cam.orbit(60.0, -40.0);
        h.app.cam_anim = None;
        let cam = h.app.cam;
        finish(&mut h);
        assert_eq!(h.app.cam, cam, "the views saved for the earlier sketch don't apply");
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
    use solidcraft_engine::render::{Camera, StandardView};

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

    /// Extrude started from an open sketch goes back to the sketch's view, the one on screen when
    /// Extrude was asked for, also when "When a sketch ends" turns the view back to the one from
    /// before the sketch: that turn used to be remembered instead, so nothing went back.
    #[test]
    fn from_an_open_sketch_ok_cancel_and_escape_go_back_to_the_sketch_view() {
        use crate::prefs::SketchExitView;
        for exit in SketchExitView::ALL {
            for close in ["OK", "Cancel", "Escape"] {
                let mut h = Harness::new();
                h.app.preferences.extrude_view_isometric = true;
                h.app.preferences.extrude_view_return = true;
                h.app.preferences.sketch_exit_view = exit;
                h.call("ui.view", json!({"view": "iso"}));
                h.settle();
                h.step(&json!({"start": "sketch.create"})).unwrap();
                h.step(&json!({"click": {"plane": "XY"}})).unwrap();
                h.settle();
                h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
                crate::scenario::check(&mut h, &json!({"sketching": true, "camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
                let sketch_view = h.app.cam;
                open_extrude(&mut h);
                assert_eq!(h.app.session.active_sketch, None, "{exit:?} {close}: Extrude ended the sketch");
                crate::scenario::check(&mut h, &json!({"camera": {"yaw_deg": -45, "pitch_deg": 35}}))
                    .unwrap_or_else(|e| panic!("{exit:?} {close}: isometric while Extrude is open: {e}"));
                match close {
                    "Escape" => h.step(&json!({"key": "Escape"})).unwrap(),
                    other => h.step(&json!({"widget": other})).unwrap(),
                };
                h.settle();
                assert!(h.app.dialog.is_none(), "{exit:?} {close}");
                assert_eq!(h.app.session.doc.features.len(), if close == "OK" { 2 } else { 1 }, "{exit:?} {close}: OK adds the extrude");
                assert_eq!(h.app.cam, sketch_view, "{exit:?} {close}: back to the sketch's view");
            }
        }
    }

    /// Only a view square to the sketch (its 2D view) turns: a view already turned in 3D stays
    /// where it is when Extrude opens and when it closes, from a finished sketch or an open one.
    #[test]
    fn a_view_already_in_3d_stays() {
        let (mut h, _) = from_the_top(true, true);
        h.app.cam.orbit(30.0, -20.0);
        h.app.cam_anim = None;
        let turned = h.app.cam;
        open_extrude(&mut h);
        assert_eq!(h.app.cam, turned, "a finished sketch seen in 3D: the view stays");
        h.step(&json!({"widget": "Cancel"})).unwrap();
        h.settle();
        assert_eq!(h.app.cam, turned, "and stays after Cancel");

        let mut h = Harness::new();
        h.app.preferences.extrude_view_isometric = true;
        h.app.preferences.extrude_view_return = true;
        h.step(&json!({"start": "sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]})).unwrap();
        h.app.cam.orbit(30.0, -20.0);
        h.app.cam_anim = None;
        let turned = h.app.cam;
        open_extrude(&mut h);
        assert_eq!(h.app.cam, turned, "an open sketch turned in 3D: the view stays");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert_eq!(h.app.cam, turned, "and stays after Esc");
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
