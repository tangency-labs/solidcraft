//! Explicit finite-depth CPU opaque and certified dielectric path transport. Absence preserves the legacy producer.
use super::{ErrorCode, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CpuPathOptions {
    /// Number of shaded opaque surfaces, Glass boundaries and medium collisions, including the first event.
    pub max_depth: u8,
}
impl Default for CpuPathOptions {
    fn default() -> Self {
        Self { max_depth: 8 }
    }
}
impl CpuPathOptions {
    pub fn validate(self) -> Result<()> {
        if !(1..=super::MAX_PATH_DEPTH).contains(&self.max_depth) {
            return Err(JobError::new(ErrorCode::InvalidRequest, "CPU path depth must be 1..64 surfaces, Glass boundaries or medium collisions"));
        }
        Ok(())
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
        let an = crate::surface::unit_direction(au.cross(av)).map_err(|_| invalid())?;
        let x = crate::surface::unit_direction(au).map_err(|_| invalid())?;
        let y = an.cross(x);
        for b in lights.iter().skip(i + 1) {
            let bu = vector(b.edge_u_mm);
            let bv = vector(b.edge_v_mm);
            let bc = vector(b.center_mm);
            let bn = crate::surface::unit_direction(bu.cross(bv)).map_err(|_| invalid())?;
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
