//! Original display-linear creative looks. No imported film LUTs or measured
//! camera emulation. Ordered strength blending leaves the raw master untouched.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize, de, ser::SerializeSeq};
use std::fmt;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookKind {
    #[default]
    Monochrome,
    Sepia,
    Cyanotype,
    Warm,
    Cool,
    TealOrange,
    Faded,
    BleachBypass,
}
impl LookKind {
    pub const ALL: [Self; 8] =
        [Self::Monochrome, Self::Sepia, Self::Cyanotype, Self::Warm, Self::Cool, Self::TealOrange, Self::Faded, Self::BleachBypass];
    pub fn key(self) -> &'static str {
        match self {
            Self::Monochrome => "render.look_mono",
            Self::Sepia => "render.look_sepia",
            Self::Cyanotype => "render.look_cyanotype",
            Self::Warm => "render.look_warm",
            Self::Cool => "render.look_cool",
            Self::TealOrange => "render.look_teal_orange",
            Self::Faded => "render.look_faded",
            Self::BleachBypass => "render.look_bleach",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookLayer {
    pub kind: LookKind,
    pub strength: f64,
    pub enabled: bool,
}
impl Default for LookLayer {
    fn default() -> Self {
        Self { kind: LookKind::Monochrome, strength: 1.0, enabled: true }
    }
}
impl LookLayer {
    pub fn validate(self) -> Result<()> {
        if !self.strength.is_finite() || !(0.0..=1.0).contains(&self.strength) {
            Err(JobError::new(ErrorCode::InvalidRequest, "look strength must be finite in 0..1"))
        } else {
            Ok(())
        }
    }
    fn apply(self, rgb: [f64; 3]) -> [f64; 3] {
        if !self.enabled || self.strength == 0.0 {
            return rgb;
        }
        let y = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
        let look = match self.kind {
            LookKind::Monochrome => [y; 3],
            LookKind::Sepia => [y * 1.18, y * 0.93, y * 0.64],
            LookKind::Cyanotype => [0.012 + y * 0.73, 0.04 + y * 0.85, 0.11 + y * 0.88],
            LookKind::Warm => [rgb[0] * 1.09, rgb[1] * 1.01, rgb[2] * 0.86],
            LookKind::Cool => [rgb[0] * 0.87, rgb[1] * 1.02, rgb[2] * 1.1],
            LookKind::TealOrange => {
                let light = y.clamp(0.0, 1.0).sqrt();
                let shadows = [0.76, 1.04, 1.14];
                let highlights = [1.12, 1.02, 0.86];
                std::array::from_fn(|i| rgb[i] * (shadows[i] * (1.0 - light) + highlights[i] * light))
            }
            LookKind::Faded => rgb.map(|v| v * 0.82 + 0.045),
            LookKind::BleachBypass => std::array::from_fn(|i| ((rgb[i] * 0.35 + y * 0.65) - 0.18) * 1.32 + 0.18),
        };
        std::array::from_fn(|i| rgb[i] + (look[i] - rgb[i]) * self.strength)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LookStack {
    slots: [Option<LookLayer>; 8],
    length: u8,
}
impl LookStack {
    pub fn len(&self) -> usize {
        usize::from(self.length)
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn get(&self, index: usize) -> Option<&LookLayer> {
        self.slots.get(index).and_then(Option::as_ref)
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut LookLayer> {
        self.slots.get_mut(index).and_then(Option::as_mut)
    }
    pub fn push(&mut self, value: LookLayer) -> Result<()> {
        value.validate()?;
        let index = self.len();
        let slot = self.slots.get_mut(index).ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "at most eight creative look layers"))?;
        *slot = Some(value);
        self.length += 1;
        Ok(())
    }
    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.len() {
            return Err(JobError::new(ErrorCode::InvalidRequest, "look index is not present"));
        }
        for i in index..self.len() - 1 {
            let next = self.slots.get(i + 1).copied().flatten();
            if let Some(slot) = self.slots.get_mut(i) {
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
            return Err(JobError::new(ErrorCode::InvalidRequest, "look reorder index is not present"));
        }
        self.slots.swap(from, to);
        Ok(())
    }
    pub fn validate(self) -> Result<()> {
        for layer in self.slots.iter().take(self.len()).flatten() {
            layer.validate()?;
        }
        Ok(())
    }
    pub fn apply(self, mut rgb: [f64; 3]) -> [f64; 3] {
        for layer in self.slots.iter().take(self.len()).flatten() {
            rgb = layer.apply(rgb);
        }
        rgb
    }
}
impl Serialize for LookStack {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.len()))?;
        for layer in self.slots.iter().take(self.len()).flatten() {
            seq.serialize_element(layer)?;
        }
        seq.end()
    }
}
impl<'de> Deserialize<'de> for LookStack {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Layers;
        impl<'de> de::Visitor<'de> for Layers {
            type Value = LookStack;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("up to eight ordered creative look layers")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<LookStack, A::Error> {
                let mut result = LookStack::default();
                while let Some(v) = seq.next_element()? {
                    result.push(v).map_err(de::Error::custom)?;
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
    fn identity_order_and_zero_strength_are_exact() {
        let rgb = [0.7, 0.2, 0.1];
        assert_eq!(LookStack::default().apply(rgb), rgb);
        let mut s = LookStack::default();
        s.push(LookLayer { kind: LookKind::Warm, ..Default::default() }).unwrap();
        s.push(Default::default()).unwrap();
        let a = s.apply(rgb);
        s.move_layer(0, 1).unwrap();
        assert_ne!(a, s.apply(rgb));
        s.get_mut(0).unwrap().strength = 0.0;
        s.get_mut(1).unwrap().enabled = false;
        assert_eq!(s.apply(rgb), rgb);
        s.remove(0).unwrap();
        assert_eq!(s.len(), 1);
    }
    #[test]
    fn complete_stack_roundtrips_and_limits_are_not_truncated() {
        let mut s = LookStack::default();
        for kind in LookKind::ALL {
            s.push(LookLayer { kind, ..Default::default() }).unwrap();
        }
        assert_eq!(serde_json::from_str::<LookStack>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
        assert!(s.push(Default::default()).is_err());
        assert!(serde_json::from_str::<LookStack>(&format!("[{}]", ["{}"; 9].join(","))).is_err());
        for v in [f64::NAN, f64::INFINITY, -0.1, 1.01] {
            assert!(LookLayer { strength: v, ..Default::default() }.validate().is_err());
        }
        let monochrome = LookLayer::default().apply([1.0, 0.0, 0.0]);
        assert_eq!(monochrome, [0.2126; 3]);
    }
}
