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
        self.camera.is_valid()
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
}
