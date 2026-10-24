//! Portable built-in surface intent. Targets use definition/feature/occurrence identity,
//! never a captured mesh ordinal. Frames are definition-local millimetres. Encoded map
//! hashes pin the installed built-in resource revision; no paths or device objects are saved.
use crate::{DocError, Result, render_resources::HdrId};
use serde::{Deserialize, Serialize};
use solidcraft_render::job::textures::PlanarFrame;
use std::{collections::BTreeSet, fmt};

pub const MAX_SURFACE_BINDINGS: usize = 128;
pub const MAX_SURFACE_PATH: usize = 64;
fn invalid(message: impl Into<String>) -> DocError {
    DocError::Invalid(message.into())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderBodyTarget {
    pub definition: String,
    pub feature: u64,
    pub component: u64,
    #[serde(deserialize_with = "occurrence_path")]
    pub occurrence_path: Vec<u64>,
}
impl RenderBodyTarget {
    pub fn validate(&self) -> Result<()> {
        if self.definition.is_empty()
            || self.definition.len() > 1024
            || self.definition.chars().any(char::is_control)
            || self.feature == 0
            || self.occurrence_path.len() > MAX_SURFACE_PATH
            || self.occurrence_path.capacity() > MAX_SURFACE_PATH
            || self.occurrence_path.contains(&0)
            || self.occurrence_path.iter().collect::<BTreeSet<_>>().len() != self.occurrence_path.len()
            || (self.component == 0) != self.occurrence_path.is_empty()
        {
            return Err(invalid("invalid render surface definition/feature/occurrence identity"));
        }
        Ok(())
    }
}
fn occurrence_path<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<u64>, D::Error> {
    bounded_sequence::<D, u64, MAX_SURFACE_PATH>(d)
}

/// A material's four encoded SHA-256 receipts, in explicit role order. Height is
/// intentionally excluded: an eight-bit surface is not a displacement resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinSurfaceSource {
    pub id: String,
    pub version: String,
    pub base_color_sha256: HdrId,
    pub normal_sha256: HdrId,
    pub roughness_sha256: HdrId,
    pub metallic_sha256: HdrId,
}
impl BuiltinSurfaceSource {
    pub fn validate(&self) -> Result<()> {
        let valid_id = self.id.strip_prefix("SURF-").is_some_and(|n| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()));
        if !valid_id
            || self.version.is_empty()
            || self.version.len() > 32
            || !self.version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return Err(invalid("bounded versioned SURF-nnn source required"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSurface {
    pub target: RenderBodyTarget,
    pub source: BuiltinSurfaceSource,
    pub local_frame: PlanarFrame,
    pub normal_strength: f64,
}
impl SavedSurface {
    pub fn validate(&self) -> Result<()> {
        self.target.validate()?;
        self.source.validate()?;
        self.local_frame.validate().map_err(|e| invalid(e.to_string()))?;
        if !self.normal_strength.is_finite() || !(0.0..=1.0).contains(&self.normal_strength) {
            return Err(invalid("saved surface normal strength must be finite in 0..1"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SurfaceBindings(Vec<SavedSurface>);
impl SurfaceBindings {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &SavedSurface> {
        self.0.iter()
    }
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > MAX_SURFACE_BINDINGS || self.0.capacity() > MAX_SURFACE_BINDINGS {
            return Err(invalid("at most 128 saved surface bindings"));
        }
        let mut seen = BTreeSet::new();
        for binding in &self.0 {
            binding.validate()?;
            if !seen.insert(&binding.target) {
                return Err(invalid("duplicate saved surface target"));
            }
        }
        Ok(())
    }
    /// Validate before mutation; explicit replacement never transfers to another target.
    pub fn set(&mut self, binding: SavedSurface, replace: bool) -> Result<()> {
        binding.validate()?;
        if let Some(old) = self.0.iter_mut().find(|old| old.target == binding.target) {
            if old == &binding {
                return Ok(());
            }
            if !replace {
                return Err(invalid("surface target already has a binding; explicit replacement required"));
            }
            *old = binding;
        } else {
            if self.len() >= MAX_SURFACE_BINDINGS {
                return Err(invalid("at most 128 saved surface bindings"));
            }
            self.0.try_reserve_exact(1).map_err(|_| invalid("could not reserve saved surface binding"))?;
            if self.0.capacity() > MAX_SURFACE_BINDINGS {
                return Err(invalid("saved surface capacity exceeds admission"));
            }
            self.0.push(binding);
        }
        Ok(())
    }
    pub fn remove(&mut self, target: &RenderBodyTarget) -> bool {
        let before = self.0.len();
        self.0.retain(|binding| &binding.target != target);
        self.0.len() != before
    }
}
impl<'de> Deserialize<'de> for SurfaceBindings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let result = Self(bounded_sequence::<D, SavedSurface, MAX_SURFACE_BINDINGS>(d)?);
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}
pub(crate) fn bounded_sequence<'de, D, T, const N: usize>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Sequence<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> serde::de::Visitor<'de> for Sequence<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "an array of at most {N} entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = seq.next_element()? {
                if entries.len() == N {
                    return Err(serde::de::Error::custom(format!("at most {N} entries")));
                }
                entries.try_reserve_exact(1).map_err(serde::de::Error::custom)?;
                entries.push(entry);
            }
            Ok(entries)
        }
    }
    d.deserialize_seq(Sequence::<T, N>(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> SavedSurface {
        SavedSurface {
            target: RenderBodyTarget { definition: "Panel".into(), feature: 7, component: 0, occurrence_path: vec![] },
            source: BuiltinSurfaceSource {
                id: "SURF-001".into(),
                version: "1.0.0".into(),
                base_color_sha256: HdrId::of(b"base"),
                normal_sha256: HdrId::of(b"normal"),
                roughness_sha256: HdrId::of(b"rough"),
                metallic_sha256: HdrId::of(b"metal"),
            },
            local_frame: PlanarFrame { origin: [1., 2., 3.], u: [100., 0., 0.], v: [0., 100., 0.] },
            normal_strength: 0.75,
        }
    }
    #[test]
    fn saved_surface_origin_bounds_match_the_document_roundtrip() {
        use crate::render_settings::{GpuJob, RenderJob, RenderSetup};
        for coordinate in [-1e9, 1e9] {
            let mut binding = fixture();
            binding.local_frame.origin = [coordinate, 0.0, 0.0];
            let mut setup = RenderSetup {
                job: Some(RenderJob::GpuPath { transparent_background: false, options: Box::new(GpuJob::default()) }),
                ..Default::default()
            };
            setup.surfaces.set(binding, false).unwrap();
            let mut document = crate::Document::default();
            std::sync::Arc::make_mut(&mut document.render).setups.insert("Frame".into(), setup);
            document.render.validate().unwrap();
            assert_eq!(crate::Document::from_json(&document.to_json()).unwrap(), document);
        }
        for coordinate in [-5e9, 5e9, 1e9_f64.next_up()] {
            let mut binding = fixture();
            binding.local_frame.origin = [coordinate, 0.0, 0.0];
            assert!(binding.validate().is_err());
        }
    }

    #[test]
    fn saved_surfaces_roundtrip_without_paths_or_ordinals() {
        let mut bindings = SurfaceBindings::default();
        bindings.set(fixture(), false).unwrap();
        let wire = serde_json::to_value(&bindings).unwrap();
        let restored: SurfaceBindings = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(bindings, restored);
        assert!(wire[0].get("body_index").is_none());
        let mut changed = fixture();
        changed.normal_strength = 0.2;
        assert!(bindings.set(changed.clone(), false).is_err());
        assert_eq!(bindings, restored);
        bindings.set(changed, true).unwrap();
        assert_ne!(bindings, restored);
    }
    #[test]
    fn malformed_and_duplicate_surface_intent_is_rejected() {
        let valid = serde_json::to_value(fixture()).unwrap();
        for (path, value) in
            [("normal_strength", json!(1.1)), ("body_index", json!(0)), ("local_frame", json!({"origin":[0,0,0],"u":[0,0,0],"v":[0,1,0]}))]
        {
            let mut wire = valid.clone();
            wire[path] = value;
            assert!(serde_json::from_value::<SurfaceBindings>(json!([wire])).is_err());
        }
        assert!(serde_json::from_value::<SurfaceBindings>(json!([valid.clone(), valid.clone()])).is_err());
        assert!(serde_json::from_value::<SurfaceBindings>(json!(vec![valid; 129])).is_err());
        let mut target = fixture().target;
        target.component = 1;
        target.occurrence_path = vec![2, 2];
        assert!(target.validate().is_err());
    }
}
