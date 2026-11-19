//! What one view of a design looks with: the data that makes two views of the same model differ.
//! Plain data with a saved form, so a front end keeps one per view and the commands read it.

use crate::Camera;
use serde::{Deserialize, Serialize};

/// How one view looks at the model. Today the camera; display style and analyses join it here, so
/// a view that keeps its own look needs no other state.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewSpec {
    pub camera: Camera,
}

impl ViewSpec {
    /// A spec looking from `camera`.
    pub fn new(camera: Camera) -> Self {
        ViewSpec { camera }
    }

    /// Whether every number is usable: a saved spec that is not is dropped, never drawn.
    pub fn is_valid(&self) -> bool {
        // Camera projection takes radians: zero is orthographic; a perspective lens must
        // stay below a half-turn so its focal length and visible height remain positive.
        self.camera.is_valid() && (0.0..std::f64::consts::PI).contains(&self.camera.fov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_round_trips_and_a_broken_camera_is_invalid() {
        let spec = ViewSpec::new(Camera::default());
        let back: ViewSpec = serde_json::from_value(serde_json::to_value(spec).unwrap()).unwrap();
        assert_eq!(back, spec);
        assert!(back.is_valid());
        let mut broken = spec;
        broken.camera.distance = f64::NAN;
        assert!(!broken.is_valid());
    }

    #[test]
    fn saved_field_of_view_is_zero_or_a_perspective_angle_below_pi_radians() {
        for fov in [0.0, 0.6, std::f64::consts::PI - 1e-6] {
            let camera = Camera { fov, ..Default::default() };
            assert!(ViewSpec::new(camera).is_valid(), "usable field of view {fov}");
            let projection = camera.proj(1.0, 0.1, 1000.0);
            assert!(projection.0[0][0] > 0.0 && projection.0[1][1] > 0.0);
            assert!(projection.0.iter().flatten().all(|value| value.is_finite()));
        }
        for fov in [-1.0, std::f64::consts::PI, std::f64::consts::TAU, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(!ViewSpec::new(Camera { fov, ..Default::default() }).is_valid(), "unusable field of view {fov}");
        }
    }
}
