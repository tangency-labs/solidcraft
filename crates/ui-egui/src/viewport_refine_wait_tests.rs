//! Display refinement waits for the camera to stand still (#3595): swapping a refinement in
//! rebuilds and uploads the whole model scene, so it is not done in the middle of an orbit.

use super::{CameraRest, REFINE_STILL_MS};

#[test]
fn a_camera_that_changes_every_frame_never_gets_refinement() {
    let mut rest = CameraRest::default();
    for frame in 0..600_u32 {
        let now = 1000.0 + f64::from(frame) * 4.0;
        assert!(rest.wait(u64::from(frame) + 1, now) > 0.0, "frame {frame}: a moving camera must wait");
    }
}

#[test]
fn refinement_is_allowed_once_the_camera_has_stood_still() {
    let mut rest = CameraRest::default();
    assert!(rest.wait(7, 1000.0) > 0.0, "the first frame with a new camera waits");
    let left = rest.wait(7, 1000.0 + REFINE_STILL_MS / 2.0);
    assert!(left > 0.0 && left < REFINE_STILL_MS, "half way: some of the wait is left, got {left}");
    assert_eq!(rest.wait(7, 1000.0 + REFINE_STILL_MS), 0.0);
    assert_eq!(rest.wait(7, 1000.0 + 5.0 * REFINE_STILL_MS), 0.0);
}

#[test]
fn moving_again_starts_the_wait_over() {
    let mut rest = CameraRest::default();
    rest.wait(7, 1000.0);
    assert_eq!(rest.wait(7, 2000.0), 0.0);
    assert_eq!(rest.wait(8, 2010.0), REFINE_STILL_MS);
    assert_eq!(rest.wait(8, 2010.0 + REFINE_STILL_MS), 0.0);
}
