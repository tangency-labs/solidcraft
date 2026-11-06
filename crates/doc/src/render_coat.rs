//! Portable opaque clear-coat assignments. Targets are stable definition/feature/
//! component/occurrence identities; captured mesh ordinals never enter a design.
use crate::{
    DocError, Result,
    render_surface::{RenderBodyTarget, bounded_sequence},
};
use serde::{Deserialize, Serialize};
use solidcraft_render::clear_coat::ClearCoat;
use std::collections::BTreeSet;

/// Matches the existing runtime clear-coat override bound for either executor.
pub const MAX_COAT_BINDINGS: usize = 64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoatMaterial {
    pub weight: f64,
    pub roughness: f64,
    pub ior: f64,
}
impl PartialEq for CoatMaterial {
    fn eq(&self, other: &Self) -> bool {
        self.weight.to_bits() == other.weight.to_bits()
            && self.roughness.to_bits() == other.roughness.to_bits()
            && self.ior.to_bits() == other.ior.to_bits()
    }
}
impl Default for CoatMaterial {
    fn default() -> Self {
        Self { weight: 1.0, roughness: 0.12, ior: 1.5 }
    }
}
impl CoatMaterial {
    pub fn validate(self) -> Result<()> {
        self.to_runtime().map(|_| ())
    }
    pub fn to_runtime(self) -> Result<ClearCoat> {
        ClearCoat::new(self.weight, self.roughness, self.ior).map_err(|e| DocError::Invalid(e.to_string()))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedCoat {
    pub target: RenderBodyTarget,
    pub coat: CoatMaterial,
}
impl SavedCoat {
    pub fn validate(&self) -> Result<()> {
        self.target.validate()?;
        self.coat.validate()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CoatBindings(Vec<SavedCoat>);
impl CoatBindings {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &SavedCoat> {
        self.0.iter()
    }
    pub fn validate(&self) -> Result<()> {
        if self.len() > MAX_COAT_BINDINGS || self.0.capacity() > MAX_COAT_BINDINGS {
            return Err(DocError::Invalid("at most 64 saved clear-coat targets".into()));
        }
        let mut seen = BTreeSet::new();
        for binding in self.iter() {
            binding.validate()?;
            if !seen.insert(&binding.target) {
                return Err(DocError::Invalid("duplicate saved clear-coat target".into()));
            }
        }
        Ok(())
    }
    /// Validate before mutation; replacement is explicit and stays on the same target.
    pub fn set(&mut self, binding: SavedCoat, replace: bool) -> Result<()> {
        binding.validate()?;
        if let Some(old) = self.0.iter_mut().find(|old| old.target == binding.target) {
            if old == &binding {
                return Ok(());
            }
            if !replace {
                return Err(DocError::Invalid("clear-coat target already has a binding; explicit replacement required".into()));
            }
            *old = binding;
        } else {
            if self.len() >= MAX_COAT_BINDINGS {
                return Err(DocError::Invalid("at most 64 saved clear-coat targets".into()));
            }
            self.0.try_reserve_exact(1).map_err(|_| DocError::Invalid("could not reserve saved clear-coat binding".into()))?;
            if self.0.capacity() > MAX_COAT_BINDINGS {
                return Err(DocError::Invalid("saved clear-coat capacity exceeds admission".into()));
            }
            self.0.push(binding);
        }
        Ok(())
    }
    pub fn remove(&mut self, target: &RenderBodyTarget) -> bool {
        let before = self.len();
        self.0.retain(|binding| &binding.target != target);
        self.len() != before
    }
}
impl<'de> Deserialize<'de> for CoatBindings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let result = Self(bounded_sequence::<D, SavedCoat, MAX_COAT_BINDINGS>(d)?);
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}
