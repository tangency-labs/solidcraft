//! Portable render intent and immutable HDR references. Runtime paths, captured bodies and devices never enter a design.

use std::{fmt, marker::PhantomData};

use serde::{Deserialize, Serialize};
use solidcraft_render::{
    Camera,
    job::{
        CpuBudget, CpuProgressiveOptions, DirectionalLightOptions, JobOptions, LightingPreset, PunctualLightOptions, RaytraceEnvironmentOptions,
        RaytraceOptions, RectangleLightOptions, StudioLighting, ThinLens, ToneMap,
    },
    lighting::environment::SkyGradient,
};

use crate::{
    DocError, Result,
    render_resources::{CpuHdrEnvironment, GpuHdrEnvironment},
};

/// The selected method, not a collection of independent backend drafts.
/// Missing/null `RenderSetup.job` retains the legacy request protocol.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderJob {
    Raster {
        #[serde(default)]
        transparent_background: bool,
    },
    CpuRaytrace {
        #[serde(default)]
        options: CpuJob,
    },
    GpuPath {
        #[serde(default)]
        transparent_background: bool,
        // StudioLighting is a bounded four-light value; boxing keeps the other variants small.
        #[serde(default)]
        options: Box<GpuJob>,
    },
}

impl RenderJob {
    pub fn validate(&self, camera: &Camera, exposure_ev: f64) -> Result<()> {
        match self {
            Self::Raster { .. } if exposure_ev != 0.0 => Err(DocError::Invalid("saved raster rendering requires exposure_ev: 0".into())),
            Self::Raster { .. } => Ok(()),
            Self::CpuRaytrace { options } => options.to_runtime(exposure_ev).map(|_| ()),
            Self::GpuPath { options, transparent_background } => {
                if *transparent_background && options.backdrop.is_some() {
                    return Err(DocError::Invalid("choose a camera backdrop or transparent PNG, not both".into()));
                }
                options.validate(exposure_ev)?;
                if *transparent_background && options.finishing.bloom.is_some_and(|b| b.active()) {
                    return Err(DocError::Invalid("display bloom requires opaque output".into()));
                }
                if camera.fov == 0.0 && options.lens.is_some_and(|lens| lens.aperture_radius_mm > 0.0) {
                    return Err(DocError::Invalid("saved thin-lens depth of field requires a perspective camera".into()));
                }
                Ok(())
            }
        }
    }
}

/// CPU values have the runtime renderer's units and limits. Exposure belongs to RenderSetup.
/// These explicit fields exclude name/ordinal bindings and external paths; HDR refers to document bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CpuJob {
    #[serde(skip_serializing_if = "CpuBudget::is_compact")]
    pub budget: CpuBudget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progressive: Option<CpuProgressiveOptions>,
    pub primary_samples: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<LightingPreset>,
    #[serde(deserialize_with = "directional_lights", skip_serializing_if = "Option::is_none")]
    pub lights: Option<Vec<DirectionalLightOptions>>,
    #[serde(deserialize_with = "punctual_lights", skip_serializing_if = "Option::is_none")]
    pub punctual_lights: Option<Vec<PunctualLightOptions>>,
    #[serde(deserialize_with = "area_lights", skip_serializing_if = "Option::is_none")]
    pub area_lights: Option<Vec<RectangleLightOptions>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_samples: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    pub shadows: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<RaytraceEnvironmentOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr: Option<CpuHdrEnvironment>,
}

impl Default for CpuJob {
    fn default() -> Self {
        Self {
            budget: CpuBudget::Compact,
            progressive: None,
            primary_samples: 1,
            preset: None,
            lights: None,
            punctual_lights: None,
            area_lights: None,
            area_samples: None,
            seed: None,
            shadows: true,
            environment: None,
            hdr: None,
        }
    }
}

impl CpuJob {
    /// Pure lowering, valid on hosts without a CPU raytracer. Checks bounds before cloning lists.
    pub fn to_runtime(&self, exposure_ev: f64) -> Result<RaytraceOptions> {
        if let Some(hdr) = self.hdr {
            hdr.validate()?;
            if self.environment.is_some() {
                return Err(DocError::Invalid("choose either saved CPU HDR or constant environment".into()));
            }
        }
        bounded(&self.lights, 8, "directional lights")?;
        bounded(&self.punctual_lights, 8, "point/spot lights")?;
        bounded(&self.area_lights, 4, "rectangular lights")?;
        if self.area_samples.is_some_and(|n| !matches!(n, 1 | 4 | 16 | 64)) {
            return Err(DocError::Invalid("saved area_samples must be 1, 4, 16 or 64".into()));
        }
        // Preserve hidden editable intent in CpuJob, but do not submit tuning for
        // a light family that is explicitly absent. This does not rewrite the file.
        let active_area = self.area_lights.as_ref().is_some_and(|lights| !lights.is_empty());
        let options = RaytraceOptions {
            budget: self.budget,
            progressive: self.progressive,
            primary_samples: self.primary_samples,
            preset: self.preset,
            lights: self.lights.clone(),
            punctual_lights: self.punctual_lights.clone(),
            area_lights: self.area_lights.clone(),
            area_samples: if active_area { self.area_samples } else { None },
            seed: if active_area { self.seed } else { None },
            shadows: self.shadows,
            environment: self.environment,
            exposure_ev,
            ..Default::default()
        };
        options.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        Ok(options)
    }
}

/// Progressive values without a second exposure authority or captured GPU bindings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GpuJob {
    #[serde(skip_serializing_if = "crate::render_aging::SavedAgingStack::is_empty")]
    pub aging: crate::render_aging::SavedAgingStack,
    pub nested_media: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<solidcraft_render::job::volume::HomogeneousVolume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<solidcraft_render::job::adaptive::AdaptiveSampling>,
    /// Conditional Fresnel strata per primary sample; one keeps the old stream.
    pub transmission_samples: u32,

    #[serde(skip_serializing_if = "solidcraft_render::job::finishing::Finishing::is_identity")]
    pub finishing: solidcraft_render::job::finishing::Finishing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<solidcraft_render::job::finishing::Backdrop>,
    pub linear_exr: bool,
    pub samples: u32,
    pub max_depth: u32,
    pub seed: u32,
    pub tone_map: ToneMap,
    pub denoise: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens: Option<ThinLens>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lighting: Option<StudioLighting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<SkyEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr: Option<GpuHdrEnvironment>,
}

impl Default for GpuJob {
    fn default() -> Self {
        let options = JobOptions::default();
        Self {
            aging: Default::default(),
            nested_media: options.nested_media,
            volume: options.volume,
            adaptive: options.adaptive,
            transmission_samples: options.transmission_samples,
            finishing: options.finishing,
            backdrop: options.backdrop,
            linear_exr: options.linear_exr,
            samples: options.samples,
            max_depth: options.max_depth,
            seed: options.seed,
            tone_map: options.tone_map,
            denoise: options.denoise,
            lens: options.lens,
            lighting: options.lighting,
            environment: None,
            hdr: None,
        }
    }
}

impl GpuJob {
    /// Pure numerical validation; this does not resolve a saved occurrence.
    pub fn validate(&self, exposure_ev: f64) -> Result<()> {
        self.validated_parameters(exposure_ev).map(|_| ())
    }

    /// Untargeted intent is self-contained. Targeted aging requires the engine's
    /// current capture; callers cannot silently turn an unresolved target global.
    pub fn to_runtime(&self, exposure_ev: f64) -> Result<JobOptions> {
        if self.aging.iter().any(|layer| layer.target.is_some()) {
            return Err(DocError::Invalid("targeted aging requires stable occurrence resolution against the current scene".into()));
        }
        self.validated_parameters(exposure_ev)
    }

    fn validated_parameters(&self, exposure_ev: f64) -> Result<JobOptions> {
        if let Some(hdr) = self.hdr {
            hdr.validate()?;
            if self.environment.is_some() {
                return Err(DocError::Invalid("choose either saved GPU HDR or sky environment".into()));
            }
        }
        if let Some(environment) = self.environment {
            environment.validate()?;
        }
        let options = JobOptions {
            aging: self.aging.parameters_only()?,
            nested_media: self.nested_media,
            volume: self.volume,
            adaptive: self.adaptive,
            transmission_samples: self.transmission_samples,
            finishing: self.finishing,
            backdrop: self.backdrop,
            linear_exr: self.linear_exr,
            samples: self.samples,
            max_depth: self.max_depth,
            seed: self.seed,
            tone_map: self.tone_map,
            denoise: self.denoise,
            lens: self.lens,
            lighting: self.lighting,
            exposure_ev,
        };
        options.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        Ok(options)
    }
}

/// Only the self-contained procedural sky is portable in this slice. HDR paths are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkyEnvironment {
    Sky {
        #[serde(default)]
        gradient: SkyGradient,
        #[serde(default)]
        rotation_rad: f64,
        #[serde(default = "unit")]
        intensity: f64,
    },
}

impl SkyEnvironment {
    pub fn validate(&self) -> Result<()> {
        let Self::Sky { gradient, rotation_rad, intensity } = self;
        gradient.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        if !rotation_rad.is_finite() || rotation_rad.abs() > 1e6 || !intensity.is_finite() || !(0.0..=100.0).contains(intensity) {
            return Err(DocError::Invalid("saved sky rotation must be finite within +/-1000000 radians and intensity 0..100".into()));
        }
        Ok(())
    }
}

fn unit() -> f64 {
    1.0
}

fn bounded<T>(values: &Option<Vec<T>>, maximum: usize, label: &str) -> Result<()> {
    if values.as_ref().is_some_and(|values| values.len() > maximum || values.capacity() > maximum) {
        return Err(DocError::Invalid(format!("at most {maximum} saved {label} and reserved slots")));
    }
    Ok(())
}

// Do not deserialize unbounded Vecs and truncate them afterwards. Null and omission both
// mean unspecified; Some([]) remains an explicit empty list and is never converted to None.
fn optional_lights<'de, D, T, const N: usize>(deserializer: D) -> std::result::Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Lights<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> serde::de::Visitor<'de> for Lights<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "at most {N} saved lights")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            values.try_reserve_exact(N).map_err(serde::de::Error::custom)?;
            while let Some(value) = sequence.next_element()? {
                if values.len() == N {
                    return Err(serde::de::Error::custom(format!("at most {N} saved lights")));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    struct Optional<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> serde::de::Visitor<'de> for Optional<T, N> {
        type Value = Option<Vec<T>>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "null or at most {N} saved lights")
        }
        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error> {
            deserializer.deserialize_seq(Lights::<T, N>(PhantomData)).map(Some)
        }
    }
    deserializer.deserialize_option(Optional::<T, N>(PhantomData))
}

fn directional_lights<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Vec<DirectionalLightOptions>>, D::Error> {
    optional_lights::<D, DirectionalLightOptions, 8>(d)
}

fn punctual_lights<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Vec<PunctualLightOptions>>, D::Error> {
    optional_lights::<D, PunctualLightOptions, 8>(d)
}

fn area_lights<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Vec<RectangleLightOptions>>, D::Error> {
    optional_lights::<D, RectangleLightOptions, 4>(d)
}
