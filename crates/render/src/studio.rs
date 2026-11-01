//! Original fixed direct-light studio used by opaque product preview consumers.
//! Relative linear radiance, not a calibrated illuminance or an HDRI environment.

use solidcraft_geom::Vec3;

use crate::surface::{OpaqueSurface, SurfaceError, unit_direction};

/// Fixed, neutral directional lights in the project's world frame (Z up).
/// Directions point towards each light and are analytically unit length.
pub const LIGHTS: [(Vec3, f64); 3] = [(Vec3::new(0.6, 0.0, 0.8), 3.0), (Vec3::new(-0.8, 0.0, 0.6), 1.0), (Vec3::new(0.0, -0.6, 0.8), 1.5)];

/// Sum BRDF * incident radiance * light cosine, without display transfer.
/// There are no cast shadows, indirect bounces, ambient term or emission.
pub fn shade(surface: OpaqueSurface, normal: Vec3, view: Vec3) -> Result<Vec3, SurfaceError> {
    shade_weighted(surface, normal, view, [1.0; 3])
}

pub fn shade_weighted(surface: OpaqueSurface, normal: Vec3, view: Vec3, weights: [f64; 3]) -> Result<Vec3, SurfaceError> {
    if weights.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value)) {
        return Err(SurfaceError::Radiance);
    }
    let n = unit_direction(normal)?;
    let mut radiance = Vec3::ZERO;
    for ((light, intensity), weight) in LIGHTS.into_iter().zip(weights) {
        radiance += surface.brdf(n, view, light)? * (intensity * weight.clamp(0.0, 1.0) * n.dot(light).max(0.0));
    }
    Ok(radiance)
}

/// Per-channel Reinhard preview tone map, still in linear sRGB.
/// Consumers encode exactly once after applying this to nonnegative finite radiance.
pub fn tone_map(radiance: Vec3) -> Result<Vec3, SurfaceError> {
    if !radiance.is_finite() || radiance.x < 0.0 || radiance.y < 0.0 || radiance.z < 0.0 {
        return Err(SurfaceError::Radiance);
    }
    let map = |c: f64| c / (1.0 + c);
    Ok(Vec3::new(map(radiance.x), map(radiance.y), map(radiance.z)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn render_lights_studio_visibility_preserves_default_and_zero_weights_remove_all_sources() {
        direct_white_conductor_matches_closed_form_and_is_tone_mapped_once();
        let surface = OpaqueSurface::new(Vec3::new(0.8, 0.5, 0.25), 0.0, 1.0, 1.5).unwrap();
        assert_eq!(shade(surface, Vec3::Z, Vec3::Z).unwrap(), shade_weighted(surface, Vec3::Z, Vec3::Z, [1.0; 3]).unwrap());
        assert_eq!(shade_weighted(surface, Vec3::Z, Vec3::Z, [0.0; 3]).unwrap(), Vec3::ZERO);
        assert!(shade_weighted(surface, Vec3::Z, Vec3::Z, [f64::NAN, 1.0, 1.0]).is_err());
        assert!(shade_weighted(surface, Vec3::Z, Vec3::Z, [2.0, 1.0, 1.0]).is_err());
    }

    #[test]
    fn direct_white_conductor_matches_closed_form_and_is_tone_mapped_once() {
        let s = OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0, 1.5).unwrap();
        let c = shade(s, Vec3::Z, Vec3::Z).unwrap();
        let expected = (4.5 * 0.8 / 1.8 + 0.6 / 1.6) / (2.0 * PI);
        assert!((c - Vec3::new(expected, expected, expected)).len() < 1e-14);
        let mapped = tone_map(c).unwrap();
        assert!((mapped.x - expected / (1.0 + expected)).abs() < 1e-14);
        assert_eq!(tone_map(Vec3::ZERO).unwrap(), Vec3::ZERO);
        assert!(tone_map(Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
        assert!(tone_map(Vec3::new(-1.0, 0.0, 0.0)).is_err());
        assert!(shade(s, Vec3::ZERO, Vec3::Z).is_err());
    }
}
