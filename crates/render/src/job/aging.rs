//! Original ordered procedural weathering. These are authored optical layers,
//! not corrosion kinetics, engineering-property changes or displaced geometry.
//! Fixed-capacity intent remains copyable, bounded and portable with a saved shot.
use super::{ErrorCode, JobError, Result};
use crate::{ProductScene, surface::srgb8_to_linear};
use serde::{Deserialize, Serialize, de, ser::SerializeSeq};
use std::fmt;

pub const MAX_AGING_LAYERS: usize = 8;
const NAME_BYTES: usize = 256;
fn invalid(message: impl Into<String>) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BodyName {
    bytes: [u8; NAME_BYTES],
    length: u16,
}
impl BodyName {
    pub fn new(value: &str) -> Result<Self> {
        if value.is_empty() || value.len() > NAME_BYTES || value.chars().any(char::is_control) {
            return Err(invalid("aging target name must contain 1..256 UTF-8 bytes without controls"));
        }
        let mut result = Self { bytes: [0; NAME_BYTES], length: value.len() as u16 };
        if let Some(destination) = result.bytes.get_mut(..value.len()) {
            destination.copy_from_slice(value.as_bytes());
        }
        Ok(result)
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(self.bytes.get(..usize::from(self.length)).unwrap_or_default()).unwrap_or_default()
    }
}
impl fmt::Debug for BodyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}
impl Serialize for BodyName {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for BodyName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Name;
        impl<'de> de::Visitor<'de> for Name {
            type Value = BodyName;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded exact captured body name")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                BodyName::new(value).map_err(E::custom)
            }
        }
        d.deserialize_str(Name)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyTarget {
    pub body_index: u32,
    pub body_name: BodyName,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgingKind {
    #[default]
    Rust,
    Patina,
    Dust,
    Grime,
    Scratches,
    PaintChips,
    Fading,
}
impl AgingKind {
    pub const ALL: [Self; 7] = [Self::Rust, Self::Patina, Self::Dust, Self::Grime, Self::Scratches, Self::PaintChips, Self::Fading];
    pub fn code(self) -> u32 {
        match self {
            Self::Rust => 1,
            Self::Patina => 2,
            Self::Dust => 3,
            Self::Grime => 4,
            Self::Scratches => 5,
            Self::PaintChips => 6,
            Self::Fading => 7,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Rust => "render.aging_rust",
            Self::Patina => "render.aging_patina",
            Self::Dust => "render.aging_dust",
            Self::Grime => "render.aging_grime",
            Self::Scratches => "render.aging_scratches",
            Self::PaintChips => "render.aging_chips",
            Self::Fading => "render.aging_fading",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceClass {
    #[default]
    AnyOpaque,
    Metal,
    Dielectric,
}
impl SurfaceClass {
    pub fn code(self) -> u32 {
        match self {
            Self::AnyOpaque => 0,
            Self::Metal => 1,
            Self::Dielectric => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgingLayer {
    pub kind: AgingKind,
    pub enabled: bool,
    pub strength: f64,
    pub coverage: f64,
    pub scale_mm: f64,
    pub seed: u32,
    /// World-anchored field origin. Does not imply following a moving occurrence.
    pub origin_mm: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<BodyTarget>,
    pub surface_class: SurfaceClass,
    pub color_srgb: [u8; 3],
    pub roughness: f64,
    pub metallic: f64,
    /// Bias to upward-facing geometric normals; not occlusion/cavity detection.
    pub upward_bias: f64,
}
impl Default for AgingLayer {
    fn default() -> Self {
        Self::preset(AgingKind::Rust)
    }
}
impl AgingLayer {
    pub fn preset(kind: AgingKind) -> Self {
        let (color, roughness, metallic, coverage, upward) = match kind {
            AgingKind::Rust => ([145, 63, 27], 0.9, 0.0, 0.45, 0.0),
            AgingKind::Patina => ([49, 132, 112], 0.76, 0.0, 0.48, 0.0),
            AgingKind::Dust => ([182, 166, 130], 0.96, 0.0, 0.65, 0.88),
            AgingKind::Grime => ([39, 31, 23], 0.7, 0.0, 0.42, 0.0),
            AgingKind::Scratches => ([181, 184, 188], 0.28, 1.0, 0.65, 0.0),
            AgingKind::PaintChips => ([118, 123, 132], 0.45, 1.0, 0.28, 0.0),
            AgingKind::Fading => ([230, 222, 206], 0.78, 0.0, 0.68, 0.3),
        };
        Self {
            kind,
            enabled: true,
            strength: 0.8,
            coverage,
            scale_mm: 18.0,
            seed: 19,
            origin_mm: [0.0; 3],
            target: None,
            surface_class: SurfaceClass::AnyOpaque,
            color_srgb: color,
            roughness,
            metallic,
            upward_bias: upward,
        }
    }
    pub fn active(self) -> bool {
        self.enabled && self.strength > 0.0 && self.coverage > 0.0
    }
    pub fn validate(self) -> Result<()> {
        if [self.strength, self.coverage, self.metallic, self.upward_bias].iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !self.scale_mm.is_finite()
            || !(0.01..=100_000.0).contains(&self.scale_mm)
            || !self.roughness.is_finite()
            || !(0.02..=1.0).contains(&self.roughness)
            || self.origin_mm.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
            || self.seed > 65535
        {
            return Err(invalid(
                "aging needs strength/coverage/metallic/upward bias 0..1, scale 0.01..100000 mm, roughness 0.02..1, finite origin +/-1e9 mm and seed 0..65535",
            ));
        }
        if let Some(target) = self.target {
            if target.body_index as usize >= super::MAX_GPU_PATH_BODIES || target.body_name.as_str().starts_with("__render_stage__/") {
                return Err(invalid("aging cannot target an out-of-range body or a passive render stage"));
            }
            BodyName::new(target.body_name.as_str())?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AgingStack {
    slots: [Option<AgingLayer>; MAX_AGING_LAYERS],
    length: u8,
}
impl AgingStack {
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn len(&self) -> usize {
        usize::from(self.length)
    }
    pub fn active(&self) -> bool {
        self.iter().any(|layer| layer.active())
    }
    pub fn iter(&self) -> impl Iterator<Item = &AgingLayer> {
        self.slots.iter().take(self.len()).flatten()
    }
    pub fn get(&self, index: usize) -> Option<&AgingLayer> {
        self.slots.get(index).and_then(Option::as_ref)
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut AgingLayer> {
        self.slots.get_mut(index).and_then(Option::as_mut)
    }
    pub fn push(&mut self, layer: AgingLayer) -> Result<()> {
        layer.validate()?;
        let index = self.len();
        let slot = self.slots.get_mut(index).ok_or_else(|| invalid("at most eight aging layers"))?;
        *slot = Some(layer);
        self.length += 1;
        Ok(())
    }
    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.len() {
            return Err(invalid("aging layer index is not present"));
        }
        for current in index..self.len() - 1 {
            let next = self.slots.get(current + 1).copied().flatten();
            if let Some(slot) = self.slots.get_mut(current) {
                *slot = next;
            }
        }
        self.length -= 1;
        let end = self.len();
        if let Some(slot) = self.slots.get_mut(end) {
            *slot = None;
        }
        Ok(())
    }
    pub fn move_layer(&mut self, from: usize, to: usize) -> Result<()> {
        if from >= self.len() || to >= self.len() {
            return Err(invalid("aging reorder index is not present"));
        }
        self.slots.swap(from, to);
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        for layer in self.iter() {
            layer.validate()?;
        }
        Ok(())
    }
    pub fn validate_scene(&self, scene: &ProductScene) -> Result<()> {
        self.validate()?;
        for layer in self.iter().filter(|layer| layer.active()) {
            if let Some(target) = layer.target {
                let body = scene.meshes.get(target.body_index as usize).ok_or_else(|| invalid("aging target body is missing"))?;
                if body.name != target.body_name.as_str() {
                    return Err(invalid("aging target identity changed; review and select the target again"));
                }
                if body.transmission.is_some() || body.opacity != 1.0 || body.faces.iter().any(|f| f.transmission.is_some() || f.opacity != 1.0) {
                    return Err(invalid("aging supports opaque surfaces only; explicit glass targets are refused"));
                }
            }
            for (index, body) in scene.meshes.iter().enumerate() {
                if body.name.starts_with("__render_stage__/") || layer.target.is_some_and(|t| t.body_index as usize != index) {
                    continue;
                }
                let bounds = body.mesh.bounds();
                if !bounds.is_empty() && bounds.diagonal() / layer.scale_mm > 4096.0 {
                    return Err(invalid("aging field exceeds 4096 repeats across a body; increase the physical scale explicitly"));
                }
            }
        }
        Ok(())
    }
    /// Five aligned vec4s per layer: id/target/class/seed, mask/scale,
    /// rebased periodic origin, linear coat RGB/roughness, metallic/reserved.
    pub fn gpu_layers(&self, target: [f64; 3]) -> Result<[[f32; 20]; MAX_AGING_LAYERS]> {
        self.validate()?;
        if target.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return Err(invalid("invalid aging camera target"));
        }
        let mut result = [[0.0; 20]; MAX_AGING_LAYERS];
        for (out, layer) in result.iter_mut().zip(self.iter()) {
            if !layer.active() {
                continue;
            }
            out[0] = layer.kind.code() as f32;
            out[1] = layer.target.map_or(0, |t| t.body_index + 1) as f32;
            out[2] = layer.surface_class.code() as f32;
            out[3] = layer.seed as f32;
            out[4] = layer.strength as f32;
            out[5] = layer.coverage as f32;
            out[6] = layer.scale_mm.recip() as f32;
            out[7] = layer.upward_bias as f32;
            for axis in 0..3 {
                out[8 + axis] = ((target[axis] - layer.origin_mm[axis]) / layer.scale_mm).rem_euclid(4096.0) as f32;
            }
            for axis in 0..3 {
                out[12 + axis] = srgb8_to_linear(layer.color_srgb[axis]) as f32;
            }
            out[15] = layer.roughness as f32;
            out[16] = layer.metallic as f32;
        }
        Ok(result)
    }
}
impl Serialize for AgingStack {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.len()))?;
        for layer in self.iter() {
            seq.serialize_element(layer)?;
        }
        seq.end()
    }
}
impl<'de> Deserialize<'de> for AgingStack {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Layers;
        impl<'de> de::Visitor<'de> for Layers {
            type Value = AgingStack;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most eight ordered aging layers")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<AgingStack, A::Error> {
                let mut result = AgingStack::default();
                while let Some(layer) = seq.next_element()? {
                    result.push(layer).map_err(de::Error::custom)?;
                }
                Ok(result)
            }
        }
        d.deserialize_seq(Layers)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layers_are_bounded_ordered_and_exactly_portable() {
        let mut stack = AgingStack::default();
        assert!(!stack.active());
        for kind in AgingKind::ALL {
            stack.push(AgingLayer::preset(kind)).unwrap();
        }
        stack.move_layer(0, 3).unwrap();
        assert_eq!(stack.get(0).unwrap().kind, AgingKind::Grime);
        stack.remove(2).unwrap();
        assert_eq!(stack.len(), 6);
        let wire = serde_json::to_string(&stack).unwrap();
        assert_eq!(serde_json::from_str::<AgingStack>(&wire).unwrap(), stack);
        while stack.len() < 8 {
            stack.push(Default::default()).unwrap();
        }
        assert!(stack.push(Default::default()).is_err());
        let nine = format!("[{}]", ["{}"; 9].join(","));
        assert!(serde_json::from_str::<AgingStack>(&nine).is_err());
        assert!(serde_json::from_str::<AgingStack>(r#"[{"years":30}]"#).is_err());
    }
    #[test]
    fn hostile_values_and_names_are_rejected_without_changing_geometry() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 1.1] {
            assert!(AgingLayer { strength: value, ..Default::default() }.validate().is_err());
        }
        assert!(AgingLayer { scale_mm: 0.0, ..Default::default() }.validate().is_err());
        for name in ["", "bad\nname"] {
            assert!(BodyName::new(name).is_err());
        }
        assert!(BodyName::new(&"a".repeat(257)).is_err());
        let name = BodyName::new("Módulo Ω").unwrap();
        assert_eq!(serde_json::from_str::<BodyName>(&serde_json::to_string(&name).unwrap()).unwrap(), name);
    }
    #[test]
    fn periodic_rebasing_and_disabled_layers_have_exact_boundaries() {
        let mut stack = AgingStack::default();
        stack.push(AgingLayer { scale_mm: 2.0, ..Default::default() }).unwrap();
        let a = stack.gpu_layers([0.0; 3]).unwrap();
        let b = stack.gpu_layers([8192.0; 3]).unwrap();
        assert_eq!(a, b);
        stack.get_mut(0).unwrap().enabled = false;
        assert_eq!(stack.gpu_layers([0.0; 3]).unwrap(), [[0.0; 20]; 8]);
        assert!(!stack.active());
    }
}
