//! Runtime thin-lens camera settings. Focus is an axial distance from the eye,
//! not distance along an off-axis ray. No document or camera-navigation changes.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThinLens {
    pub aperture_radius_mm: f64,
    pub focus_distance_mm: f64,
}

impl ThinLens {
    /// The shared physical camera domain used by both native ray-traced methods.
    /// A zero-radius lens preserves pinhole/orthographic behavior exactly.
    pub fn validate_camera(self, camera: &solidcraft_viewport::Camera) -> Result<()> {
        self.validate()?;
        if self.aperture_radius_mm > 0.0
            && (!camera.is_valid()
                || !(0.0..std::f64::consts::PI).contains(&camera.fov)
                || camera.fov == 0.0
                || !camera.half_height().is_finite()
                || camera.half_height() > 1e12
                || !(1e-6..=1e9).contains(&camera.distance)
                || !(1e-6..=1e6).contains(&(camera.half_height() / camera.distance)))
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "thin-lens depth of field requires a bounded perspective camera (distance >= 0.000001 mm and tan(fov/2) in 0.000001..1000000)",
            ));
        }
        Ok(())
    }
    pub fn validate(self) -> Result<()> {
        if !self.aperture_radius_mm.is_finite()
            || !(0.0..=1e7).contains(&self.aperture_radius_mm)
            || !self.focus_distance_mm.is_finite()
            || !(1e-6..=1e9).contains(&self.focus_distance_mm)
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "lens aperture radius must be 0..10000000 mm and axial focus distance 0.000001..1000000000 mm",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lens_admission_rejects_nonfinite_and_unbounded_values() {
        for aperture in [f64::NAN, f64::INFINITY, -1.0, 1e7 + 1.0] {
            assert!(ThinLens { aperture_radius_mm: aperture, focus_distance_mm: 10.0 }.validate().is_err());
        }
        for focus in [f64::NAN, f64::INFINITY, 0.0, -1.0, 1e9 + 1.0] {
            assert!(ThinLens { aperture_radius_mm: 1.0, focus_distance_mm: focus }.validate().is_err());
        }
        assert!(ThinLens { aperture_radius_mm: 0.0, focus_distance_mm: 1.0 }.validate().is_ok());
        assert!(serde_json::from_str::<ThinLens>(r#"{"aperture_radius_mm":1,"focus_distance_mm":10,"f_stop":2}"#).is_err());
    }
    #[test]
    fn active_lens_camera_domain_is_explicit_and_zero_aperture_stays_compatible() {
        let active = ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 10.0 };
        let zero = ThinLens { aperture_radius_mm: 0.0, ..active };
        let mut camera = solidcraft_viewport::Camera::default();
        assert!(active.validate_camera(&camera).is_err());
        zero.validate_camera(&camera).unwrap();
        camera.fov = 0.7;
        active.validate_camera(&camera).unwrap();
        for fov in [f64::NAN, f64::INFINITY, -0.7, std::f64::consts::PI, 1e-12] {
            assert!(active.validate_camera(&solidcraft_viewport::Camera { fov, ..camera }).is_err());
        }
        for distance in [1e-7, 1e9 + 1.0] {
            assert!(active.validate_camera(&solidcraft_viewport::Camera { distance, ..camera }).is_err());
        }
        let fixed = super::super::RaytraceOptions { lens: Some(active), ..Default::default() };
        assert!(fixed.validate().is_err());
        super::super::RaytraceOptions { lens: Some(zero), ..fixed.clone() }.validate().unwrap();
        super::super::RaytraceOptions { progressive: Some(Default::default()), ..fixed }.validate().unwrap();
    }
}
