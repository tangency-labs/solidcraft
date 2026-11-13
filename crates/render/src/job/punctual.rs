//! Bounded point/spot intent, also embedded by saved CPU render setups.
use serde::{Deserialize, Serialize};

use super::{ErrorCode, JobError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpotConeOptions {
    /// World direction FROM the source, never camera-local.
    pub emitted_axis: [f64; 3],
    pub inner_half_angle_radians: f64,
    pub outer_half_angle_radians: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PunctualLightOptions {
    pub position_mm: [f64; 3],
    pub color: [f64; 3],
    pub intensity: f64,
    pub reference_distance_mm: f64,
    pub minimum_distance_mm: f64,
    /// Omitted means a point source with no directional cone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cone: Option<SpotConeOptions>,
}

/// GPU spot feathers narrower than this cosine interval are explicitly refused.
/// This keeps f32 unit-vector/dot rounding small compared with the authored
/// feather instead of widening two distinct f64 bounds into the same f32 cone.
pub const MIN_GPU_SPOT_COSINE_GAP: f64 = 1.0 / 1024.0;
pub const MAX_PUNCTUAL_LIGHTS: usize = 8;

/// Copyable bounded GPU source intent. CPU requests keep their existing DTO Vec.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PunctualLights {
    entries: [PunctualLightOptions; MAX_PUNCTUAL_LIGHTS],
    len: usize,
}
impl Default for PunctualLights {
    fn default() -> Self {
        Self {
            entries: [PunctualLightOptions {
                position_mm: [0.0; 3],
                color: [0.0; 3],
                intensity: 0.0,
                reference_distance_mm: 1.0,
                minimum_distance_mm: 0.001,
                cone: None,
            }; MAX_PUNCTUAL_LIGHTS],
            len: 0,
        }
    }
}
impl PunctualLights {
    pub fn new(lights: &[PunctualLightOptions]) -> Result<Self> {
        if lights.len() > MAX_PUNCTUAL_LIGHTS {
            return Err(JobError::new(ErrorCode::InvalidRequest, "at most eight point and spot lights combined"));
        }
        let mut result = Self::default();
        for (slot, light) in result.entries.iter_mut().zip(lights) {
            light.validate_gpu()?;
            *slot = *light;
        }
        result.len = lights.len();
        Ok(result)
    }
    pub fn iter(&self) -> impl Iterator<Item = &PunctualLightOptions> {
        self.entries.iter().take(self.len)
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
impl Serialize for PunctualLights {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
impl<'de> Deserialize<'de> for PunctualLights {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Lights;
        impl<'de> serde::de::Visitor<'de> for Lights {
            type Value = PunctualLights;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("at most eight GPU point and spot lights combined")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
                let mut lights = PunctualLights::default();
                while let Some(light) = seq.next_element::<PunctualLightOptions>()? {
                    let slot =
                        lights.entries.get_mut(lights.len).ok_or_else(|| serde::de::Error::custom("at most eight GPU point and spot lights"))?;
                    light.validate_gpu().map_err(serde::de::Error::custom)?;
                    *slot = light;
                    lights.len += 1;
                }
                Ok(lights)
            }
        }
        deserializer.deserialize_seq(Lights)
    }
}

impl PunctualLightOptions {
    /// GPU precision is a separate bounded domain; original CPU intent is never
    /// clamped or rewritten when it cannot be represented by this consumer.
    pub fn validate_gpu(&self) -> Result<()> {
        self.validate()?;
        if let Some(cone) = self.cone {
            let gap = cone.inner_half_angle_radians.cos() - cone.outer_half_angle_radians.cos();
            if gap < MIN_GPU_SPOT_COSINE_GAP {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "GPU spot feather is too narrow for reliable precision; increase its angle range or use CPU",
                ));
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        let invalid = || JobError::new(ErrorCode::InvalidRequest, "invalid bounded point/spot light");
        if !self.position_mm.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
            || !self.color.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !self.intensity.is_finite()
            || !(0.0..=64.0).contains(&self.intensity)
            || !self.reference_distance_mm.is_finite()
            || !(0.001..=1e6).contains(&self.reference_distance_mm)
            || !self.minimum_distance_mm.is_finite()
            || !(0.001..=self.reference_distance_mm).contains(&self.minimum_distance_mm)
        {
            return Err(invalid());
        }
        if let Some(cone) = self.cone {
            let inner = cone.inner_half_angle_radians;
            let outer = cone.outer_half_angle_radians;
            if !cone.emitted_axis.iter().all(|v| v.is_finite() && v.abs() <= 1e9)
                || !cone.emitted_axis.iter().any(|v| *v != 0.0)
                || !inner.is_finite()
                || !outer.is_finite()
                || inner < 0.0
                || inner >= outer
                || outer > std::f64::consts::FRAC_PI_2
                || inner.cos() <= outer.cos()
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    #[cfg(feature = "raytrace-cpu")]
    pub(crate) fn light(self) -> crate::raytrace::Result<crate::raytrace::punctual::PunctualLight> {
        use crate::raytrace::punctual::{PunctualLight, SpotCone};
        use solidcore_record::Vec3;
        let [x, y, z] = self.position_mm;
        let [r, g, b] = self.color;
        let cone = self
            .cone
            .map(|c| {
                let [x, y, z] = c.emitted_axis;
                SpotCone::new(Vec3::new(x, y, z), c.inner_half_angle_radians, c.outer_half_angle_radians)
            })
            .transpose()?;
        PunctualLight::new(Vec3::new(x, y, z), Vec3::new(r, g, b), self.intensity, self.reference_distance_mm, self.minimum_distance_mm, cone)
    }
}

pub(super) fn bounded_punctual<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Vec<PunctualLightOptions>>, D::Error> {
    struct Lights;
    impl<'de> serde::de::Visitor<'de> for Lights {
        type Value = Vec<PunctualLightOptions>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most eight point/spot sources")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut lights = Vec::new();
            while let Some(light) = seq.next_element()? {
                if lights.len() == 8 {
                    return Err(serde::de::Error::custom("at most eight point/spot sources"));
                }
                lights.try_reserve_exact(1).map_err(serde::de::Error::custom)?;
                lights.push(light);
            }
            if lights.capacity() > 8 {
                return Err(serde::de::Error::custom("point/spot capacity exceeds eight"));
            }
            Ok(lights)
        }
    }
    struct Optional;
    impl<'de> serde::de::Visitor<'de> for Optional {
        type Value = Option<Vec<PunctualLightOptions>>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("optional bounded point/spot sources")
        }
        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<Self::Value, D::Error> {
            d.deserialize_seq(Lights).map(Some)
        }
    }
    d.deserialize_option(Optional)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::RaytraceOptions;

    fn light() -> PunctualLightOptions {
        PunctualLightOptions {
            position_mm: [0.0, 0.0, 100.0],
            color: [1.0; 3],
            intensity: 1.0,
            reference_distance_mm: 100.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }
    }

    #[test]
    fn omitted_empty_point_and_spot_roundtrip_without_null_sequence() {
        let mut spot = light();
        spot.cone = Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.5 });
        for lights in [None, Some(Vec::new()), Some(vec![light(), spot])] {
            let options = RaytraceOptions { punctual_lights: lights, ..Default::default() };
            let json = serde_json::to_value(&options).unwrap();
            assert_eq!(serde_json::from_value::<RaytraceOptions>(json).unwrap(), options);
        }
    }

    #[test]
    fn counts_original_capacity_and_rejects_ninth_source_before_runtime() {
        let options = RaytraceOptions { punctual_lights: Some(vec![light(); 8]), ..Default::default() };
        assert_eq!(options.retained_bytes().unwrap(), (8 * std::mem::size_of::<PunctualLightOptions>()) as u64);
        assert!(options.validate().is_ok());
        let nine = RaytraceOptions { punctual_lights: Some(vec![light(); 9]), ..Default::default() };
        assert!(nine.validate().is_err());
        assert!(serde_json::from_value::<RaytraceOptions>(serde_json::to_value(nine).unwrap()).is_err());
        let spare = RaytraceOptions { punctual_lights: Some(Vec::with_capacity(9)), ..Default::default() };
        assert!(spare.retained_bytes().is_err());
    }

    #[test]
    fn directionals_keep_their_own_eight_slots_when_punctual_slots_are_full() {
        let options = RaytraceOptions {
            lights: Some(vec![crate::job::DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 1.0 }; 8]),
            punctual_lights: Some(vec![light(); 8]),
            ..Default::default()
        };
        assert!(options.validate().is_ok());
    }

    #[test]
    fn invalid_cone_and_nonfinite_inputs_refuse() {
        let mut input = light();
        input.cone = Some(SpotConeOptions { emitted_axis: [0.0; 3], inner_half_angle_radians: 0.0, outer_half_angle_radians: 0.5 });
        assert!(input.validate().is_err());
        input = light();
        input.position_mm[0] = f64::NAN;
        assert!(input.validate().is_err());
        input = light();
        input.minimum_distance_mm = 0.0;
        assert!(input.validate().is_err());
        input = light();
        input.reference_distance_mm = f64::INFINITY;
        assert!(input.validate().is_err());
    }
}

/// Runtime-only counters: scene-dependent singular light samples contribute zero,
/// but remain visible to the caller. These are not saved design fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PunctualDiagnostics {
    pub near_source: u64,
    pub radiance_limit: u64,
    pub shadow_offset_crossed: u64,
}
impl PunctualDiagnostics {
    pub fn is_empty(self) -> bool {
        self.near_source == 0 && self.radiance_limit == 0 && self.shadow_offset_crossed == 0
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    fn point() -> PunctualLightOptions {
        PunctualLightOptions {
            position_mm: [1e9, 0.0, 1.0],
            color: [0.25, 0.5, 1.0],
            intensity: 2.0,
            reference_distance_mm: 10.0,
            minimum_distance_mm: 0.001,
            cone: None,
        }
    }
    #[test]
    fn gpu_punctual_copy_list_preserves_exact_authored_values_and_rejects_hostile_ninth() {
        let mut spot = point();
        spot.cone = Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1e-200], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.5 });
        let list = PunctualLights::new(&[point(), spot, point(), spot, point(), spot, point(), spot]).unwrap();
        assert_eq!(list.iter().count(), 8);
        assert_eq!(serde_json::from_value::<PunctualLights>(serde_json::to_value(list).unwrap()).unwrap(), list);
        assert!(PunctualLights::new(&[point(); 9]).is_err());
        assert!(serde_json::from_value::<PunctualLights>(serde_json::json!(vec![point(); 9])).is_err());
        assert!(serde_json::to_value(super::super::StudioLighting::default()).unwrap().get("punctual_lights").is_none());
        for mut invalid in [point(); 3] {
            invalid.minimum_distance_mm = 0.0;
            assert!(PunctualLights::new(&[invalid]).is_err());
        }
    }
    #[test]
    fn gpu_spot_precision_refusal_never_widens_or_rewrites_cpu_valid_cones() {
        for (inner, outer) in [(0.2, 0.2000000001), (0.0, 0.0001), (1.0, 1.000000001)] {
            let mut spot = point();
            spot.cone = Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: inner, outer_half_angle_radians: outer });
            let before = spot;
            spot.validate().unwrap();
            assert!(spot.validate_gpu().unwrap_err().message.contains("precision"));
            assert_eq!(spot, before);
        }
        let mut spot = point();
        spot.cone = Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.2, outer_half_angle_radians: 0.5 });
        assert!(spot.validate_gpu().is_ok());
        let cone = spot.cone.unwrap();
        let inner = cone.inner_half_angle_radians.cos() as f32;
        let outer = cone.outer_half_angle_radians.cos() as f32;
        assert!(inner > outer && f64::from(inner - outer) > MIN_GPU_SPOT_COSINE_GAP);
    }
}
