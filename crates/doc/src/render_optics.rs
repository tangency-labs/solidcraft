//! Portable render-only optical assignments with stable definition/occurrence
//! identity. These are authored optical numbers, not engineering material cards.
use crate::{
    DocError, Result,
    render_surface::{RenderBodyTarget, bounded_sequence},
};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_render::{
    surface::OpaqueSurface,
    transmission::{Transmission, subsurface::Subsurface},
};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpticalBoundary {
    ThinSheet,
    ClosedBox,
    #[default]
    ClosedMesh,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpticalMaterial {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<solidcraft_render::glass_model::GlassModel>,
    pub ior: f64,
    pub absorption_per_mm: [f64; 3],
    pub boundary: OpticalBoundary,
    pub thickness_mm: f64,
    pub shadow_transmission: bool,
    pub subsurface: Option<Subsurface>,
}
impl Default for OpticalMaterial {
    fn default() -> Self {
        Self {
            model: None,
            ior: 1.5,
            absorption_per_mm: [0.0; 3],
            boundary: Default::default(),
            thickness_mm: 0.0,
            shadow_transmission: false,
            subsurface: None,
        }
    }
}
impl OpticalMaterial {
    /// The CPU consumer is a certified subset of the existing saved optical contract.
    /// Keep authored unsupported values intact and refuse them, never drop effects.
    pub fn validate_for_cpu(self) -> Result<()> {
        self.validate_for_cpu_path(false)
    }
    /// General closed boundaries require the explicit finite progressive path.
    pub fn validate_for_cpu_path(self, path: bool) -> Result<()> {
        self.validate()?;
        if self.model.is_some() && !path {
            return Err(DocError::Invalid("advanced Glass models require CPU Indirect lighting".into()));
        }
        if self.shadow_transmission && (!path || self.boundary != OpticalBoundary::ThinSheet) {
            return Err(DocError::Invalid(
                "CPU transmissive shadows require Indirect lighting and a thin sheet; closed refractive light connections remain unsupported".into(),
            ));
        }
        if self.subsurface.is_some() && !path {
            return Err(DocError::Invalid("CPU subsurface scattering requires Indirect lighting and a certified closed medium".into()));
        }
        if self.absorption_per_mm.into_iter().any(|v| !(0.0..=100.0).contains(&v)) || !(0.0..=10000.0).contains(&self.thickness_mm) {
            return Err(DocError::Invalid("CPU glass absorption must be 0..100 per mm and virtual sheet thickness 0..10000 mm".into()));
        }
        Ok(())
    }

    /// A closed optical boundary is accepted only if the CPU producer subsequently
    /// certifies the captured mesh as its exact axis-aligned-box subset. The saved
    /// boundary is not rewritten and no arbitrary closed-mesh support is implied.
    pub fn to_cpu(self, body_index: usize, body_name: &str) -> Result<solidcraft_render::job::DielectricOptions> {
        self.to_cpu_for_path(body_index, body_name, false)
    }
    pub fn to_cpu_for_path(self, body_index: usize, body_name: &str, path: bool) -> Result<solidcraft_render::job::DielectricOptions> {
        self.validate_for_cpu_path(path)?;
        if body_name.is_empty() || body_name.len() > 1024 || body_name.chars().any(char::is_control) {
            return Err(DocError::Invalid("CPU optical target requires a bounded captured body name".into()));
        }
        Ok(solidcraft_render::job::DielectricOptions {
            model: self.model,
            subsurface: self.subsurface,
            shadow_transmission: self.shadow_transmission,
            body_index,
            body_name: body_name.into(),
            boundary: match self.boundary {
                OpticalBoundary::ThinSheet => solidcraft_render::job::DielectricBoundary::ThinSheet,
                OpticalBoundary::ClosedMesh if path => solidcraft_render::job::DielectricBoundary::ClosedMesh,
                OpticalBoundary::ClosedBox | OpticalBoundary::ClosedMesh => solidcraft_render::job::DielectricBoundary::ClosedBox,
            },
            ior: self.ior,
            absorption_per_mm: self.absorption_per_mm,
            thickness_mm: self.thickness_mm,
        })
    }

    pub fn validate(self) -> Result<()> {
        let error = |message: String| DocError::Invalid(message);
        if let Some(model) = self.model {
            model.validate(self.ior).map_err(|e| error(e.to_string()))?;
            if self.boundary == OpticalBoundary::ThinSheet && self.shadow_transmission && model.roughness > 0.0 {
                return Err(error("rough thin-sheet Glass requires disabled straight transmitted shadows".into()));
            }
        }
        OpaqueSurface::new(Vec3::ZERO, 0.0, 0.5, self.ior).map_err(|e| error(e.to_string()))?;
        if self.boundary != OpticalBoundary::ThinSheet && self.thickness_mm != 0.0 {
            return Err(error("closed optical thickness comes from geometry; thickness_mm must be zero".into()));
        }
        let [r, g, b] = self.absorption_per_mm;
        let medium = Transmission::new(Vec3::new(r, g, b), (self.boundary == OpticalBoundary::ThinSheet).then_some(self.thickness_mm))
            .map_err(|e| error(e.to_string()))?
            .with_shadow_transmission(self.shadow_transmission);
        if let Some(scatter) = self.subsurface {
            medium.with_subsurface(scatter).map_err(|e| error(e.to_string()))?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedOptics {
    pub target: RenderBodyTarget,
    pub optics: OpticalMaterial,
}
impl SavedOptics {
    pub fn validate(&self) -> Result<()> {
        self.target.validate()?;
        self.optics.validate()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpticalBindings(Vec<SavedOptics>);
impl OpticalBindings {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &SavedOptics> {
        self.0.iter()
    }
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > 32 || self.0.capacity() > 32 {
            return Err(DocError::Invalid("at most 32 portable optical targets".into()));
        }
        let mut seen = BTreeSet::new();
        for binding in &self.0 {
            binding.validate()?;
            if !seen.insert(&binding.target) {
                return Err(DocError::Invalid("duplicate portable optical target".into()));
            }
        }
        Ok(())
    }
    pub fn validate_for_cpu(&self) -> Result<()> {
        self.validate_for_cpu_path(false)
    }
    pub fn validate_for_cpu_path(&self, path: bool) -> Result<()> {
        self.validate()?;
        if self.len() > 8 {
            return Err(DocError::Invalid("CPU supports at most eight certified optical targets".into()));
        }
        for binding in self.iter() {
            binding.optics.validate_for_cpu_path(path)?;
        }
        Ok(())
    }
    pub fn set(&mut self, binding: SavedOptics, replace: bool) -> Result<()> {
        binding.validate()?;
        if let Some(old) = self.0.iter_mut().find(|old| old.target == binding.target) {
            if old == &binding {
                return Ok(());
            }
            if !replace {
                return Err(DocError::Invalid("optical target already exists; explicit replacement consent required".into()));
            }
            *old = binding;
        } else {
            if self.0.len() >= 32 {
                return Err(DocError::Invalid("at most 32 portable optical targets".into()));
            }
            self.0.try_reserve_exact(1).map_err(|_| DocError::Invalid("optical binding reservation failed".into()))?;
            if self.0.capacity() > 32 {
                return Err(DocError::Invalid("optical capacity exceeds its bound".into()));
            }
            self.0.push(binding);
        }
        Ok(())
    }
    pub fn remove(&mut self, target: &RenderBodyTarget) -> bool {
        let n = self.0.len();
        self.0.retain(|b| &b.target != target);
        n != self.0.len()
    }
}
impl<'de> Deserialize<'de> for OpticalBindings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let result = Self(bounded_sequence::<D, SavedOptics, 32>(d)?);
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

#[cfg(test)]
mod advanced_model_contract_tests {
    use super::*;
    use solidcraft_render::glass_model::{Dispersion, GlassModel, ThinFilm};
    #[test]
    fn advanced_optics_keep_every_authored_number_through_saved_and_cpu_lowering() {
        let model = GlassModel {
            roughness: 0.27,
            dispersion: Some(Dispersion { abbe_number: 45.0 }),
            thin_film: Some(ThinFilm { ior: 1.36, thickness_nm: 420.0 }),
        };
        let optics = OpticalMaterial { model: Some(model), ..Default::default() };
        let wire = serde_json::to_value(optics).unwrap();
        assert_eq!(serde_json::from_value::<OpticalMaterial>(wire.clone()).unwrap(), optics);
        assert_eq!(optics.to_cpu_for_path(2, "Original Glass", true).unwrap().model, Some(model));
        assert!(optics.to_cpu(2, "Original Glass").is_err());
        assert_eq!(serde_json::to_value(optics).unwrap(), wire);
        assert!(serde_json::to_value(OpticalMaterial::default()).unwrap().get("model").is_none());
        let sheet = OpticalMaterial {
            model: Some(GlassModel { roughness: 0.0, ..model }),
            boundary: OpticalBoundary::ThinSheet,
            shadow_transmission: true,
            ..optics
        };
        sheet.validate_for_cpu_path(true).unwrap();
        assert!(OpticalMaterial { model: Some(model), ..sheet }.validate().is_err());
    }
    #[test]
    fn caustic_quality_is_portable_between_both_saved_jobs_and_runtime_requests() {
        use crate::render_job::{CpuJob, GpuJob};
        let caustics = Some(solidcraft_render::job::CausticOptions { photons: 8192, radius_mm: 0.4, max_depth: 12, seed: 381 });
        let cpu = CpuJob { caustics, path: Some(Default::default()), progressive: Some(Default::default()), ..Default::default() };
        let gpu = GpuJob { caustics, ..Default::default() };
        assert_eq!(cpu.to_runtime(0.0).unwrap().caustics, caustics);
        assert_eq!(gpu.to_runtime(0.0).unwrap().caustics, caustics);
        assert_eq!(serde_json::from_value::<CpuJob>(serde_json::to_value(&cpu).unwrap()).unwrap(), cpu);
        assert_eq!(serde_json::from_value::<GpuJob>(serde_json::to_value(&gpu).unwrap()).unwrap(), gpu);
        assert!(serde_json::to_value(CpuJob::default()).unwrap().get("caustics").is_none());
        assert!(serde_json::to_value(GpuJob::default()).unwrap().get("caustics").is_none());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_optics_roundtrip_with_explicit_replacement_and_no_runtime_ordinal() {
        let target = RenderBodyTarget { definition: "Diffuser".into(), feature: 1, component: 0, occurrence_path: vec![] };
        let mut bindings = OpticalBindings::default();
        let one = SavedOptics {
            target: target.clone(),
            optics: OpticalMaterial { subsurface: Some(Subsurface { scattering_per_mm: 0.8, anisotropy: 0.3 }), ..Default::default() },
        };
        bindings.set(one.clone(), false).unwrap();
        let wire = serde_json::to_value(&bindings).unwrap();
        assert!(wire[0].get("body_index").is_none());
        assert_eq!(serde_json::from_value::<OpticalBindings>(wire).unwrap(), bindings);
        let mut two = one;
        two.optics.ior = 1.2;
        assert!(bindings.set(two.clone(), false).is_err());
        bindings.set(two, true).unwrap();
        assert!(bindings.remove(&target));
        assert!(bindings.is_empty());
        assert!(OpticalMaterial { thickness_mm: 1.0, ..Default::default() }.validate().is_err());
        assert!(
            OpticalMaterial {
                boundary: OpticalBoundary::ThinSheet,
                subsurface: Some(Subsurface { scattering_per_mm: 1.0, anisotropy: 0.0 }),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            OpticalMaterial {
                shadow_transmission: true,
                subsurface: Some(Subsurface { scattering_per_mm: 1.0, anisotropy: 0.0 }),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn cpu_optical_consumer_preserves_saved_boundary_and_refuses_unrepresented_effects() {
        let closed = OpticalMaterial { ior: 1.51, absorption_per_mm: [0.1, 0.02, 0.0], ..Default::default() };
        let original = serde_json::to_value(closed).unwrap();
        let cpu = closed.to_cpu(3, "Glass@part-2").unwrap();
        assert_eq!(cpu.boundary, solidcraft_render::job::DielectricBoundary::ClosedBox);
        assert_eq!((cpu.body_index, cpu.body_name.as_str()), (3, "Glass@part-2"));
        assert_eq!((cpu.ior, cpu.absorption_per_mm), (1.51, [0.1, 0.02, 0.0]));
        assert_eq!(serde_json::to_value(closed).unwrap(), original, "CPU specialization never rewrites the authored closed boundary");
        let thin = OpticalMaterial { boundary: OpticalBoundary::ThinSheet, thickness_mm: 2.0, ..closed };
        assert_eq!(thin.to_cpu(0, "Sheet").unwrap().boundary, solidcraft_render::job::DielectricBoundary::ThinSheet);
        for (value, reason) in [
            (OpticalMaterial { shadow_transmission: true, ..thin }, "transmissive shadows"),
            (OpticalMaterial { subsurface: Some(Subsurface { scattering_per_mm: 0.0, anisotropy: 0.0 }), ..closed }, "subsurface"),
            (OpticalMaterial { absorption_per_mm: [101.0, 0.0, 0.0], ..closed }, "absorption"),
            (OpticalMaterial { thickness_mm: 10001.0, ..thin }, "thickness"),
        ] {
            let before = serde_json::to_value(value).unwrap();
            assert!(value.to_cpu(0, "Target").unwrap_err().to_string().contains(reason));
            assert_eq!(serde_json::to_value(value).unwrap(), before);
        }
        let mut optics = OpticalBindings::default();
        for feature in 1..=9 {
            optics
                .set(
                    SavedOptics {
                        target: RenderBodyTarget { definition: format!("Glass{feature}"), feature, component: 0, occurrence_path: vec![] },
                        optics: closed,
                    },
                    false,
                )
                .unwrap();
        }
        optics.validate().unwrap();
        assert!(optics.validate_for_cpu().is_err());
        let target = RenderBodyTarget { definition: "Glass".into(), feature: 1, component: 0, occurrence_path: vec![] };
        let mut setup = crate::render_settings::RenderSetup {
            job: Some(crate::render_job::RenderJob::CpuRaytrace { transparent_background: false, options: Default::default() }),
            ..Default::default()
        };
        setup.optics.set(SavedOptics { target, optics: closed }, false).unwrap();
        setup.validate().unwrap();
        let wire = serde_json::to_value(&setup).unwrap();
        assert_eq!(serde_json::from_value::<crate::render_settings::RenderSetup>(wire).unwrap(), setup);
    }
}

#[cfg(test)]
mod cpu_general_glass_contract_tests {
    use super::*;
    #[test]
    fn saved_general_glass_requires_explicit_path_and_preserves_unsupported_choices_and_old_wire() {
        let optics = OpticalMaterial { ior: 1.51, absorption_per_mm: [0.1, 0.2, 0.3], boundary: OpticalBoundary::ClosedMesh, ..Default::default() };
        assert_eq!(optics.to_cpu(0, "Glass").unwrap().boundary, solidcraft_render::job::DielectricBoundary::ClosedBox);
        let general = optics.to_cpu_for_path(0, "Glass", true).unwrap();
        assert_eq!(general.boundary, solidcraft_render::job::DielectricBoundary::ClosedMesh);
        assert_eq!((general.ior, general.absorption_per_mm), (optics.ior, optics.absorption_per_mm));
        let scattering = OpticalMaterial { subsurface: Some(Subsurface { scattering_per_mm: 0.1, anisotropy: 0.3 }), ..optics };
        let before = serde_json::to_value(scattering).unwrap();
        assert!(scattering.to_cpu(0, "Glass").is_err(), "the legacy consumer remains explicit");
        assert_eq!(scattering.to_cpu_for_path(0, "Glass", true).unwrap().subsurface, scattering.subsurface);
        assert_eq!(serde_json::to_value(scattering).unwrap(), before);
        let encoded = serde_json::to_value(&general).unwrap();
        assert!(encoded.get("shadow_transmission").is_none());
        assert_eq!(serde_json::from_value::<solidcraft_render::job::DielectricOptions>(encoded).unwrap(), general);
        let thin = OpticalMaterial { boundary: OpticalBoundary::ThinSheet, shadow_transmission: true, ..optics };
        assert!(thin.to_cpu(0, "Glass").is_err());
        assert!(thin.to_cpu_for_path(0, "Glass", true).unwrap().shadow_transmission);
        for value in [
            OpticalMaterial { shadow_transmission: true, ..optics },
            OpticalMaterial {
                boundary: OpticalBoundary::ThinSheet,
                subsurface: Some(Subsurface { scattering_per_mm: 0.1, anisotropy: 0.0 }),
                ..optics
            },
        ] {
            let before = value;
            assert!(value.to_cpu_for_path(0, "Glass", true).is_err());
            assert_eq!(value, before);
        }
    }
}
