//! Explicit finite-depth CPU opaque and certified dielectric path transport. Absence preserves the legacy producer.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CpuPathOptions {
    /// Number of shaded opaque surfaces, Glass boundaries and medium collisions, including the first event.
    pub max_depth: u8,
    /// Repeat each production camera observation in 1, 2 or 4 first-Fresnel strata.
    /// The independent pilot always traces a single unstratified path.
    #[serde(skip_serializing_if = "single_stratum")]
    pub transmission_samples: u32,
    /// Split the first bare smooth closed interface into two physically weighted
    /// lanes. Requires one transmission stratum; pilots remain unsplit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub split_first_interface: bool,
    /// Continue low-throughput paths probabilistically after the third event,
    /// excluding temporary refractive radiance scaling from the survival test.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub russian_roulette: bool,
}
impl Default for CpuPathOptions {
    fn default() -> Self {
        Self { max_depth: 8, transmission_samples: 1, split_first_interface: false, russian_roulette: false }
    }
}
fn single_stratum(value: &u32) -> bool {
    *value == 1
}
impl CpuPathOptions {
    /// Bounded complete paths per production camera observation.
    pub const fn production_lanes(self) -> u32 {
        super::first_interface_lanes(self.transmission_samples, self.split_first_interface)
    }

    pub fn validate(self) -> Result<()> {
        if !(1..=super::MAX_PATH_DEPTH).contains(&self.max_depth) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU path depth must be 1..64 surfaces, Glass boundaries or medium collisions"));
        }
        if !matches!(self.transmission_samples, 1 | 2 | 4) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU first-Fresnel sampling needs 1, 2 or 4 strata"));
        }
        super::validate_first_interface_split(self.transmission_samples, self.split_first_interface)
    }
    pub fn validate_camera_budget(self, pixels: u64, production: u32, pilots: u32, budget: super::CpuBudget) -> Result<()> {
        let evaluations = self.camera_evaluations(pixels, production, pilots)?;
        if evaluations > budget.limits().max_camera_samples {
            return Err(JobError::new(ErrorCode::LimitExceeded, "CPU first-Fresnel path evaluations exceed the selected camera-sample budget"));
        }
        Ok(())
    }

    /// Preflight counts every bounded lane, including the shared prefix traced
    /// again by that lane. Photon maps still refresh once per camera observation.
    pub fn camera_evaluations(self, pixels: u64, production: u32, pilots: u32) -> Result<u64> {
        self.validate()?;
        u64::from(production)
            .checked_mul(u64::from(self.production_lanes()))
            .and_then(|paths| paths.checked_add(u64::from(pilots)))
            .and_then(|paths| paths.checked_mul(pixels))
            .ok_or_else(|| JobError::new(ErrorCode::LimitExceeded, "CPU primary path evaluation count overflow"))
    }
}

/// Positive-area overlapping same-facing coplanar supports cannot be represented
/// by a single closest-emitter label with separate per-emitter NEE. Refuse before
/// capture, rather than lose radiance or add an inconsistent MIS estimator.
pub(crate) fn validate_emitters(lights: &[super::RectangleLightOptions]) -> Result<()> {
    use solidcore_record::Vec3;
    use solidcraft_geom::prelude::*;
    let invalid = || {
        JobError::new(
            ErrorCode::InvalidRequest,
            "CPU opaque path transport refuses overlapping same-facing coplanar rectangle supports; preserve the emitters and separate them explicitly",
        )
    };
    if lights.len() > 4 {
        return Err(JobError::new(ErrorCode::InvalidRequest, "CPU opaque path transport supports at most four rectangle lights"));
    }
    let vector = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    for (i, a) in lights.iter().enumerate() {
        let au = vector(a.edge_u_mm);
        let av = vector(a.edge_v_mm);
        let ac = vector(a.center_mm);
        let an = solidcraft_viewport::surface::unit_direction(au.cross(av)).map_err(|_| invalid())?;
        let x = solidcraft_viewport::surface::unit_direction(au).map_err(|_| invalid())?;
        let y = an.cross(x);
        for b in lights.iter().skip(i + 1) {
            let bu = vector(b.edge_u_mm);
            let bv = vector(b.edge_v_mm);
            let bc = vector(b.center_mm);
            let bn = solidcraft_viewport::surface::unit_direction(bu.cross(bv)).map_err(|_| invalid())?;
            if an.dot(bn) <= 0.0 || an.cross(bn).len() > 64.0 * f64::EPSILON {
                continue;
            }
            let scale = [ac, bc, au, av, bu, bv].into_iter().map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs())).fold(1.0, f64::max);
            if ((bc - ac).dot(an)).abs() > scale * 64.0 * f64::EPSILON {
                continue;
            }
            // Plane-projected AABBs are deliberately conservative for rotated
            // supports. No extra heap state or unbounded geometry search occurs.
            let bounds = |center: Vec3, u: Vec3, v: Vec3| {
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                for su in [-0.5, 0.5] {
                    for sv in [-0.5, 0.5] {
                        let p = center + u * su + v * sv;
                        for (axis, value) in [p.dot(x), p.dot(y)].into_iter().enumerate() {
                            lo[axis] = lo[axis].min(value);
                            hi[axis] = hi[axis].max(value);
                        }
                    }
                }
                (lo, hi)
            };
            let (alo, ahi) = bounds(Vec3::ZERO, au, av);
            let (blo, bhi) = bounds(bc - ac, bu, bv);
            if (0..2).all(|axis| ahi[axis].min(bhi[axis]) - alo[axis].max(blo[axis]) > 0.0) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
