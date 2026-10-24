//! Portable render-only optical assignments with stable definition/occurrence
//! identity. These are authored optical numbers, not engineering material cards.
use crate::{
    DocError, Result,
    render_surface::{RenderBodyTarget, bounded_sequence},
};
use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;
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
    pub ior: f64,
    pub absorption_per_mm: [f64; 3],
    pub boundary: OpticalBoundary,
    pub thickness_mm: f64,
    pub shadow_transmission: bool,
    pub subsurface: Option<Subsurface>,
}
impl Default for OpticalMaterial {
    fn default() -> Self {
        Self { ior: 1.5, absorption_per_mm: [0.0; 3], boundary: Default::default(), thickness_mm: 0.0, shadow_transmission: false, subsurface: None }
    }
}
impl OpticalMaterial {
    pub fn validate(self) -> Result<()> {
        let error = |message: String| DocError::Invalid(message);
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
}
