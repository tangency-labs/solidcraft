use crate::scenario::Harness;
use serde_json::json;

/// `ui.at {"plane": …}` gives a point that a click picks that plane at, even where another
/// origin plane hides the middle of its square (seen from the front right and a little lower
/// than iso, the XZ square covers the middle of the XY one).
#[test]
fn clicking_an_origin_plane_target_sketches_on_that_plane() {
    for (yaw, pitch) in [(-0.785, 0.615), (-1.1, 0.5), (-0.45, 0.5), (2.4, 0.6), (-2.4, 0.4)] {
        for (plane, normal) in [("XY", [0.0, 0.0, 1.0]), ("XZ", [0.0, 1.0, 0.0]), ("YZ", [1.0, 0.0, 0.0])] {
            let mut h = Harness::new();
            h.app.cam.yaw = yaw;
            h.app.cam.pitch = pitch;
            h.step(&json!({"start":"sketch.create"})).unwrap();
            h.step(&json!({"click":{"plane":plane}})).unwrap();
            let state = h.app.session.world_state().unwrap();
            let n = state.sketches.first().map(|s| s.plane.normal()).unwrap_or_default();
            let along = (n.x * normal[0] + n.y * normal[1] + n.z * normal[2]).abs();
            assert!(along > 0.999, "yaw {yaw} pitch {pitch} {plane}: sketched on a plane with normal {n:?}");
        }
    }
}
