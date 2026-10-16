//! Topology sweeps: the geometry-creation base shared by the translational and
//! rotational sweeps.
//!
//! Upstream: `source/SMLib/inc/SmSweepGeometryCreation.h`,
//! `source/SMLib/src/SmSweepGeometryCreation.cpp`, `SmSweepCheckType` of
//! `source/SMLib/inc/SmTypes.h`.
//!
//! | Rust                                         | Upstream                                    |
//! |----------------------------------------------|---------------------------------------------|
//! | [`SweepGeometryCreation`]                    | `SmSweepGeometryCreation` (data members)    |
//! | [`SweepGeometryCreation::new`]               | `SmSweepGeometryCreation(dEps)`             |
//! | [`SweepGeometryCreation::set_make_analytics`] | `SetMakeAnalytics`                         |
//! | [`SweepCheckType`]                           | `SmSweepCheckType`                          |
//!
//! CO-PORTED by unit `sweep.rotational_sweep_geometry`: only the members the
//! rotational sweep geometry reads. The virtual defaults (`VertexSweepHigher`, ...,
//! all `SM_ERR`), `SmTopologySweep` and the rest of unit `sweep.topology_sweep` are
//! not ported yet; extend this file when porting them.
//!
//! Deviations: the base class is a field of each derived sweep geometry (no virtual
//! dispatch).

use crate::geom::tol::ZONE_TOL_3D;

/// `SmSweepCheckType`: whether sweeping a curve gives a valid surface; the
/// discriminants are the upstream values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SweepCheckType {
    /// `SM_SC_OKAY`: sweeping the curve gives a valid swept surface.
    Okay = 0,
    /// `SM_SC_ROT_ON_CURVE`: rotating a curve about one of its interior points gives
    /// a cone apex or bow-tie surface.
    RotOnCurve = 1,
    /// `SM_SC_SWEEP_ALONG_LENGTH`: the curve is tangent to the sweep direction.
    SweepAlongLength = 2,
    /// `SM_SC_SELF_INTERSECT`: the swept surface intersects itself.
    SelfIntersect = 3,
    /// `SM_SC_UNKNOWN`: the check failed, validity unknown.
    Unknown = 4,
}

impl SweepCheckType {
    /// The upstream enum value.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }
}

/// The data of `SmSweepGeometryCreation`, the base of the sweep geometry creators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepGeometryCreation {
    /// `m_bMakeAnalytics`: copy created surfaces as analytic surfaces where possible.
    pub make_analytics: bool,
    /// `m_dEps`: the tolerance of the base class.
    pub eps: f64,
}

impl Default for SweepGeometryCreation {
    /// `SmSweepGeometryCreation()`: `dEps = SM_ZONE_TOL_3D`.
    fn default() -> Self {
        Self::new(ZONE_TOL_3D)
    }
}

impl SweepGeometryCreation {
    /// `SmSweepGeometryCreation(dEps)`: analytics on.
    #[must_use]
    pub const fn new(eps: f64) -> Self {
        Self {
            make_analytics: true,
            eps,
        }
    }

    /// `SetMakeAnalytics`.
    pub fn set_make_analytics(&mut self, make_analytics: bool) {
        self.make_analytics = make_analytics;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let g = SweepGeometryCreation::default();
        assert!(g.make_analytics);
        assert_eq!(g.eps, ZONE_TOL_3D);
        let mut h = SweepGeometryCreation::new(1e-3);
        h.set_make_analytics(false);
        assert!(!h.make_analytics);
        assert_eq!(SweepCheckType::Unknown.code(), 4);
    }
}
