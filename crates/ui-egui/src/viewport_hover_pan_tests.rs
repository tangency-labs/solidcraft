//! #2130: the face highlight must stay on while the view is panned.
//!
//! The reporter hovered a face and then panned the view with two fingers on a trackpad: the
//! white highlight began to flicker. The cause is in the model viewport's frame loop, which
//! drops the hover pick for the whole navigation gesture: `viewport.rs` computes
//! `navigating` (which is true while `pan != egui::Vec2::ZERO`) and then filters the hover
//! pick out with `!navigating`, so `app.viewport.hover` is set to `None` for as long as the
//! view moves. The face under the cursor stops lighting up, and it comes back only when the
//! pan stops — the flicker the reporter saw.
//!
//! The request (from the issue's triage) is the one Fusion has: keep the highlight of the
//! surface under the cursor while the view is being rotated or panned.

use serde_json::json;

use super::Vec3;
use crate::scenario::Harness;

/// A big flat plate with the camera looking straight down at it, so the whole model render
/// is over its top face and a pan keeps the pointer over that same face.
fn top_view() -> Harness {
    let mut h = Harness::new();
    h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 400, "width": 400, "height": 20}}));
    h.app.cam = solidcraft_engine::render::Camera {
        target: Vec3::new(200.0, 200.0, 10.0),
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        roll: 0.0,
        distance: 100.0,
        fov: 0.0,
        up: Default::default(),
    };
    h.frames(3);
    h
}

/// A pixel over the model's top face, found by picking.
fn face_pixel(h: &Harness) -> egui::Pos2 {
    let rect = h.app.viewport.layout.as_ref().unwrap().drawable;
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    for y in 2..18 {
        for x in 2..18 {
            let at = rect.min + egui::vec2(rect.width() * x as f32 / 20.0, rect.height() * y as f32 / 20.0);
            if crate::viewport::pick(&h.app, &proj, at).iter().any(|hit| matches!(hit, crate::viewport::Hit::Face { .. })) {
                return at;
            }
        }
    }
    panic!("the model view must offer a visible face");
}

/// The highlight under the pointer stays on while the view is panned (#2130).
///
/// The pointer sits over a face, so the face lights up. Then the view is panned with two
/// fingers (a trackpad pan, as on the reporter's MacBook) while the pointer stays over that
/// same face. The highlight must stay: the face under the cursor does not change when the
/// camera moves.
#[test]
fn the_face_under_the_pointer_stays_lit_while_the_view_is_panned() {
    let mut h = top_view();
    let at = face_pixel(&h);

    // The pointer over the face: the face lights up.
    h.call("ui.move", json!({"x": at.x, "y": at.y}));
    h.frames(2);
    assert!(
        matches!(h.app.viewport.hover, Some(crate::viewport::Hit::Face { .. })),
        "the face under the pointer must light up: {:?}",
        h.app.viewport.hover
    );

    // Pan the view with the trackpad while the pointer stays over the face. The camera moves.
    let before = h.app.cam;
    let gesture = |phase| egui::Event::MouseWheel {
        source: egui::MouseWheelSource::Unknown,
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(60.0, -40.0),
        modifiers: egui::Modifiers::NONE,
        phase,
    };
    h.app.synthetic.push(gesture(egui::TouchPhase::Start));
    h.frame();
    h.app.synthetic.push(gesture(egui::TouchPhase::Move));
    h.frame();
    assert_ne!(h.app.cam.target, before.target, "the pan must have moved the view");

    // The pointer is still over the same face...
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    assert!(
        crate::viewport::pick(&h.app, &proj, at).iter().any(|hit| matches!(hit, crate::viewport::Hit::Face { .. })),
        "the pointer must still be over a face after the pan"
    );
    // ...so the face under it must still be lit.
    assert!(
        matches!(h.app.viewport.hover, Some(crate::viewport::Hit::Face { .. })),
        "the highlight under the pointer must stay on while the view is panned (#2130): {:?}",
        h.app.viewport.hover
    );
}
