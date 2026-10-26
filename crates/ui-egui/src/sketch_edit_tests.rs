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
