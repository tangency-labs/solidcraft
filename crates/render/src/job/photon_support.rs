//! Numerically bounded concentration of positive contribution events in one query.
//! Repeated records, queries and Fresnel lanes are not independent photons. This
//! descriptive support never supplies a camera or photon-map confidence interval.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhotonContributionSupport {
    /// Positive accepted contribution events, after all receiver/kernel checks.
    /// Zero-valued eligible events do not increase this concentration denominator.
    pub accepted: u32,
    pub maximum_luminance: f64,
    pub normalized_sum: f64,
    pub normalized_squared_sum: f64,
}
impl PhotonContributionSupport {
    /// Update without squaring a potentially large physical radiance value.
    /// Invalid input leaves the previous summary untouched and returns false.
    pub fn observe(&mut self, luminance: f64) -> bool {
        if !luminance.is_finite() || luminance < 0.0 {
            return false;
        }
        if luminance == 0.0 {
            return true;
        }
        let Some(accepted) = self.accepted.checked_add(1) else { return false };
        let mut next = *self;
        if luminance > next.maximum_luminance {
            let scale = next.maximum_luminance / luminance;
            next.normalized_sum *= scale;
            next.normalized_squared_sum *= scale * scale;
            next.maximum_luminance = luminance;
        }
        let value = luminance / next.maximum_luminance;
        next.normalized_sum += value;
        next.normalized_squared_sum += value * value;
        next.accepted = accepted;
        if !next.normalized_sum.is_finite() || !next.normalized_squared_sum.is_finite() {
            return false;
        }
        *self = next;
        true
    }

    /// Inverse normalized weight concentration, in positive contribution events.
    /// None means no positive support was observed, not demonstrated convergence.
    pub fn effective_event_support(self) -> Option<f64> {
        if self.accepted == 0
            || !self.maximum_luminance.is_finite()
            || self.maximum_luminance <= 0.0
            || self.normalized_sum <= 0.0
            || !self.normalized_squared_sum.is_finite()
            || self.normalized_squared_sum <= 0.0
        {
            return None;
        }
        let value = self.normalized_sum / self.normalized_squared_sum.sqrt();
        let result = value * value;
        result.is_finite().then(|| result.clamp(1.0, f64::from(self.accepted)))
    }
}
