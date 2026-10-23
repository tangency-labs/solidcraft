//! Assembly extras: contact sets (occurrences that stop each other when joints are driven),
//! motion studies (joint values keyed along a timeline) and exploded views (occurrence moves
//! shown without changing the design).

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

/// The motion retained by an associative trace. Joint bounds keep their expressions, in
/// command units (degrees or millimetres); studies are referenced by their stable identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceMotion {
    Joint { joint: u64, index: usize, from: serde_json::Value, to: serde_json::Value },
    Study { study: u64 },
}

/// A generated root sketch curve and the inputs needed to regenerate it. The seed values
/// select the original assembly branch: playing or dragging the mechanism does not redefine
/// its trace. Older saved curves have no entry here and remain ordinary fixed sketch curves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionTrace {
    pub sketch: u64,
    pub wire: String,
    pub occurrence: Vec<u64>,
    pub point: crate::joints::JointOrigin,
    pub motion: TraceMotion,
    pub samples: Option<usize>,
    pub seeds: Vec<(u64, Vec<f64>)>,
}

/// Occurrences whose bodies may touch but not pass through each other while joints move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactSet {
    pub id: u64,
    pub name: String,
    pub occurrences: Vec<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// One joint value along a motion study: (step, value) points, values in internal units
/// (radians, mm), linear in between and held before the first and after the last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyKey {
    pub joint: u64,
    #[serde(default)]
    pub index: usize,
    pub points: Vec<(f64, f64)>,
}

impl StudyKey {
    pub fn value_at(&self, step: f64) -> Option<f64> {
        let first = self.points.first()?;
        let last = self.points.last()?;
        if step <= first.0 {
            return Some(first.1);
        }
        if step >= last.0 {
            return Some(last.1);
        }
        self.points.windows(2).find_map(|w| {
            let (a, b) = (w.first()?, w.get(1)?);
            (step >= a.0 && step <= b.0).then(|| if b.0 - a.0 < 1e-12 { b.1 } else { a.1 + (b.1 - a.1) * (step - a.0) / (b.0 - a.0) })
        })
    }
}

/// A motion study: a timeline of `steps` steps along which joints follow their keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionStudy {
    pub id: u64,
    pub name: String,
    pub steps: u32,
    pub keys: Vec<StudyKey>,
}

/// An exploded view: occurrences moved apart (shown only; the design keeps its placements).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplodedView {
    pub id: u64,
    pub name: String,
    /// (occurrence, world translation)
    pub moves: Vec<(u64, Vec3)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_interpolate_and_hold() {
        let k = StudyKey { joint: 1, index: 0, points: vec![(0.0, 0.0), (10.0, 1.0), (20.0, -1.0)] };
        assert_eq!(k.value_at(-5.0), Some(0.0));
        assert_eq!(k.value_at(5.0), Some(0.5));
        assert_eq!(k.value_at(15.0), Some(0.0));
        assert_eq!(k.value_at(99.0), Some(-1.0));
        assert_eq!(StudyKey { joint: 1, index: 0, points: vec![] }.value_at(1.0), None);
    }
}
