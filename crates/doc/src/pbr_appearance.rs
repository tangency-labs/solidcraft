//! Persistable opaque optical parameters, independent of engineering material data.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_render::surface::{OpaqueSurface, SurfaceError, srgb8_to_linear};

/// Explicitly selected opaque response. Existing scalar looks have no such record.
/// Construction and deserialization share the renderer's finite admission bounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Parameters", into = "Parameters")]
pub struct PbrParameters {
    metallic: f64,
    roughness: f64,
    dielectric_ior: f64,
}

impl Default for PbrParameters {
    fn default() -> Self {
        Self { metallic: 0.0, roughness: 0.5, dielectric_ior: 1.5 }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    metallic: f64,
    roughness: f64,
    dielectric_ior: f64,
}

impl PbrParameters {
    pub fn new(metallic: f64, roughness: f64, dielectric_ior: f64) -> Result<Self, SurfaceError> {
        OpaqueSurface::new(Vec3::ZERO, metallic, roughness, dielectric_ior)?;
        Ok(Self { metallic, roughness, dielectric_ior })
    }

    /// Decode the containing look's sRGB8 base color once, for the actual consumer.
    pub fn surface(self, color: [u8; 3]) -> Result<OpaqueSurface, SurfaceError> {
        let [r, g, b] = color.map(srgb8_to_linear);
        OpaqueSurface::new(Vec3::new(r, g, b), self.metallic, self.roughness, self.dielectric_ior)
    }

    pub fn metallic(self) -> f64 {
        self.metallic
    }
    pub fn roughness(self) -> f64 {
        self.roughness
    }
    pub fn dielectric_ior(self) -> f64 {
        self.dielectric_ior
    }
}

impl TryFrom<Parameters> for PbrParameters {
    type Error = SurfaceError;
    fn try_from(p: Parameters) -> Result<Self, Self::Error> {
        Self::new(p.metallic, p.roughness, p.dielectric_ior)
    }
}
impl From<PbrParameters> for Parameters {
    fn from(p: PbrParameters) -> Self {
        Self { metallic: p.metallic, roughness: p.roughness, dielectric_ior: p.dielectric_ior }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn optical_parameters_share_strict_admission_and_round_trip() {
        let p = PbrParameters::new(1.0, 0.3, 1.5).unwrap();
        let saved = serde_json::to_value(p).unwrap();
        assert_eq!(serde_json::from_value::<PbrParameters>(saved.clone()).unwrap(), p);
        for (key, value) in [("metallic", json!(1.1)), ("roughness", json!(0)), ("dielectric_ior", json!(0.9)), ("extra", json!(true))] {
            let mut bad = saved.clone();
            bad[key] = value;
            assert!(serde_json::from_value::<PbrParameters>(bad).is_err());
        }
        assert!(PbrParameters::new(f64::NAN, 0.3, 1.5).is_err());
        assert!(PbrParameters::new(0.0, f64::INFINITY, 1.5).is_err());
        let surface = p.surface([128, 0, 255]).unwrap();
        assert!((surface.base_color().x - 0.21586050011389926).abs() < 1e-14);
        assert_eq!(surface.base_color().z, 1.0);
    }
}
