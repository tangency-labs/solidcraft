//! Original periodic preview fields in an explicit world-space millimetre frame.
//! Only roughness and shading normals change; geometry and visibility do not.
use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;
use std::f64::consts::TAU;

use crate::job::{ErrorCode, JobError, Result};
use crate::surface::{MIN_ROUGHNESS, OpaqueSurface, unit_direction};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MappingFrame {
    pub origin_mm: [f64; 3],
    /// Right-handed unit axes, expressed in world coordinates.
    pub axes: [[f64; 3]; 3],
}

impl Default for MappingFrame {
    fn default() -> Self {
        Self { origin_mm: [0.0; 3], axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceEffect {
    #[serde(deserialize_with = "bounded_body_name")]
    pub body: String,
    #[serde(default)]
    pub frame: MappingFrame,
    pub repeat_mm: [f64; 3],
    #[serde(default)]
    pub roughness_amplitude: f64,
    #[serde(default)]
    pub bump_amplitude_mm: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct PreparedEffect {
    origin: Vec3,
    axes: [Vec3; 3],
    repeat: [f64; 3],
    roughness: f64,
    bump: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadingSurface {
    pub surface: OpaqueSurface,
    pub shading_normal: Vec3,
}

fn invalid(message: &'static str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn vector([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}
fn bounded_position(p: Vec3) -> bool {
    p.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 1.0e12
}

impl SurfaceEffect {
    pub fn prepare(&self) -> Result<PreparedEffect> {
        if self.body.is_empty() || self.body.len() > 256 || self.body.capacity() > 256 || self.body.chars().any(char::is_control) {
            return Err(invalid("surface effect requires an exact bounded captured-body name"));
        }
        let origin = vector(self.frame.origin_mm);
        let axes = self.frame.axes.map(vector);
        if !bounded_position(origin)
            || axes.iter().any(|axis| !axis.is_finite() || (axis.dot(*axis) - 1.0).abs() > 1.0e-9)
            || axes.iter().enumerate().any(|(i, a)| axes.iter().skip(i + 1).any(|b| a.dot(*b).abs() > 1.0e-9))
            || (axes
                .first()
                .copied()
                .ok_or_else(|| invalid("missing mapping axis"))?
                .cross(*axes.get(1).ok_or_else(|| invalid("missing mapping axis"))?)
                .dot(*axes.get(2).ok_or_else(|| invalid("missing mapping axis"))?)
                - 1.0)
                .abs()
                > 1.0e-9
        {
            return Err(invalid("surface mapping frame must be finite, right-handed and orthonormal"));
        }
        if !self.repeat_mm.iter().all(|n| n.is_finite() && (0.001..=1.0e6).contains(n))
            || !self.roughness_amplitude.is_finite()
            || !(0.0..=1.0).contains(&self.roughness_amplitude)
            || !self.bump_amplitude_mm.is_finite()
            || !(0.0..=1.0e3).contains(&self.bump_amplitude_mm)
        {
            return Err(invalid("invalid surface repeat or amplitude"));
        }
        // Bound the L1 norm of the analytic height gradient before shading.
        if self.repeat_mm.iter().map(|r| self.bump_amplitude_mm * TAU / (3.0 * r)).sum::<f64>() > 4.0 {
            return Err(invalid("surface bump gradient exceeds preview bound 4"));
        }
        Ok(PreparedEffect { origin, axes, repeat: self.repeat_mm, roughness: self.roughness_amplitude, bump: self.bump_amplitude_mm })
    }
}

fn bounded_body_name<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    struct Name;
    impl serde::de::Visitor<'_> for Name {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("captured body name of 1..256 bytes without control characters")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<String, E> {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err(E::custom("bounded captured body name required"));
            }
            let mut name = String::new();
            name.try_reserve_exact(value.len()).map_err(E::custom)?;
            name.push_str(value);
            Ok(name)
        }
    }
    d.deserialize_str(Name)
}

pub(crate) fn deserialize_effects<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Option<Vec<SurfaceEffect>>, D::Error> {
    struct Effects;
    impl<'de> serde::de::Visitor<'de> for Effects {
        type Value = Vec<SurfaceEffect>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("at most 32 surface effects")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut effects = Vec::new();
            while let Some(effect) = sequence.next_element()? {
                if effects.len() == 32 {
                    return Err(serde::de::Error::custom("at most 32 surface effects"));
                }
                effects.push(effect);
            }
            Ok(effects)
        }
    }
    deserializer.deserialize_seq(Effects).map(Some)
}

pub fn validate_effects(effects: &[SurfaceEffect]) -> Result<()> {
    if effects.len() > 32 {
        return Err(invalid("at most 32 surface effects"));
    }
    for (i, effect) in effects.iter().enumerate() {
        effect.prepare()?;
        if effects.iter().take(i).any(|old| old.body == effect.body) {
            return Err(invalid("duplicate surface-effect body target"));
        }
    }
    Ok(())
}

impl PreparedEffect {
    pub fn is_identity(self) -> bool {
        self.roughness == 0.0 && self.bump == 0.0
    }

    /// Height in mm, world gradient in mm/mm, and a dimensionless roughness field.
    pub fn field(self, position: Vec3) -> Result<(f64, Vec3, f64)> {
        if !bounded_position(position) {
            return Err(invalid("surface sample position exceeds millimetre bounds"));
        }
        let offset = position - self.origin;
        let (mut height, mut gradient, mut roughness) = (0.0, Vec3::ZERO, 0.0);
        for (axis, repeat) in self.axes.iter().zip(self.repeat) {
            let cycles = offset.dot(*axis) / repeat;
            if !cycles.is_finite() || cycles.abs() > 1.0e9 {
                return Err(invalid("surface sample exceeds periodic precision bound"));
            }
            let phase = TAU * cycles.rem_euclid(1.0);
            let (sine, cosine) = phase.sin_cos();
            height += self.bump * sine / 3.0;
            gradient += *axis * (self.bump * TAU * cosine / (3.0 * repeat));
            roughness += cosine / 3.0;
        }
        Ok((height, gradient, roughness))
    }

    pub fn apply(self, surface: OpaqueSurface, shading_normal: Vec3, geometric_normal: Vec3, position: Vec3) -> Result<ShadingSurface> {
        if self.is_identity() {
            return Ok(ShadingSurface { surface, shading_normal });
        }
        let (_, gradient, field) = self.field(position)?;
        let normal = unit_direction(shading_normal).map_err(|_| invalid("invalid shading normal"))?;
        let geometric = unit_direction(geometric_normal).map_err(|_| invalid("invalid geometric normal"))?;
        let mut changed = shading_normal;
        if self.bump != 0.0 {
            let tangent = gradient - normal * gradient.dot(normal);
            let candidate = unit_direction(normal - tangent).map_err(|_| invalid("invalid bump normal"))?;
            // Never turn the normal through the geometric surface. Visibility and
            // offsets continue to use the original geometric normal independently.
            changed = if candidate.dot(geometric) > 0.0 { candidate } else { shading_normal };
        }
        let surface = if self.roughness == 0.0 {
            surface
        } else {
            let changed = OpaqueSurface::new(
                surface.base_color(),
                surface.metallic(),
                (surface.roughness() + self.roughness * field).clamp(MIN_ROUGHNESS, 1.0),
                surface.dielectric_ior(),
            )
            .map_err(|_| invalid("invalid procedural surface"))?;
            // GPU2 shares this surface type. Changing the substrate roughness
            // must retain an already captured coat for direct-light evaluation.
            surface.clear_coat().map_or(changed, |coat| changed.with_clear_coat(coat))
        };
        Ok(ShadingSurface { surface, shading_normal: changed })
    }
}

#[cfg(test)]
#[path = "surface_patterns_tests.rs"]
mod tests;
