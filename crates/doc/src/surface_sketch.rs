//! A sketch on a curved face: the face it is attached to, the chart its (u, v) live in, and
//! the sketch itself. This slice adds the saved shape only; the feature that evaluates it and
//! the attachment resolver follow.

use serde::{Deserialize, Serialize};

use crate::config::ConfigFaceTarget;

/// The chart a surface sketch is stored in: the face's (u, v) origin and two metric-orthonormal
/// axes, with the tolerance the chart was built at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceChart {
    /// The origin in the face's (u, v).
    pub origin: [f64; 2],
    /// The chart u axis in the face's (u, v).
    pub u_axis: [f64; 2],
    /// The chart v axis in the face's (u, v).
    pub v_axis: [f64; 2],
    /// The tolerance the chart was built at, in millimetres.
    pub tolerance: f64,
}

impl SurfaceChart {
    /// Whether every number is finite and the tolerance is positive.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.origin
            .iter()
            .chain(&self.u_axis)
            .chain(&self.v_axis)
            .all(|v| v.is_finite())
            && self.tolerance.is_finite()
            && self.tolerance > 0.0
    }
}

/// A sketch attached to a curved face: the face certificate and the chart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRef {
    /// The face the sketch follows.
    pub target: ConfigFaceTarget,
    /// The chart its (u, v) live in.
    pub chart: SurfaceChart,
}

impl SurfaceRef {
    /// Whether the target and the chart are valid.
    ///
    /// # Errors
    ///
    /// The target's own validation, or a chart that is not finite or has no positive tolerance.
    pub fn validate(&self) -> crate::Result<()> {
        self.target.validate()?;
        if !self.chart.is_valid() {
            return Err(crate::DocError::Invalid("invalid surface chart".into()));
        }
        Ok(())
    }
}

/// A sketch on a curved face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSketch {
    /// The face and chart.
    pub surface: SurfaceRef,
    /// The sketch's points, curves and constraints.
    pub sketch: solidcraft_sketch::Sketch,
}

impl SurfaceSketch {
    /// Whether the reference is valid.
    ///
    /// # Errors
    ///
    /// The surface reference's validation.
    pub fn validate(&self) -> crate::Result<()> {
        self.surface.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart() -> SurfaceChart {
        SurfaceChart {
            origin: [0.3, 1.2],
            u_axis: [0.6, 0.8],
            v_axis: [-0.8, 0.6],
            tolerance: 1e-6,
        }
    }

    #[test]
    fn a_surface_chart_round_trips_and_validates() {
        let chart = chart();
        let text = serde_json::to_string(&chart).expect("json");
        let back: SurfaceChart = serde_json::from_str(&text).expect("back");
        assert_eq!(back, chart);
        assert!(chart.is_valid());
    }

    #[test]
    fn a_non_finite_chart_is_refused() {
        let mut chart = chart();
        chart.u_axis[0] = f64::NAN;
        assert!(!chart.is_valid());
    }
}
