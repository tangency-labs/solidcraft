//! Bounded finite-disk Sun proposals. Shared material evaluation/PDFs stay unchanged.
use super::{Result, TraceError};
use crate::{job::Sun, surface::unit_direction};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use std::f64::consts::{PI, TAU};

pub(super) struct Sample {
    pub direction: Vec3,
    pub radiance: Vec3,
    pub pdf: f64,
}
fn invalid() -> TraceError {
    TraceError::InvalidInput("invalid CPU finite Sun sample")
}
pub(super) fn pdf(sun: Sun, direction: Vec3) -> Result<f64> {
    sun.validate().map_err(|_| invalid())?;
    let direction = unit_direction(direction).map_err(|_| invalid())?;
    Ok(if direction.dot(sun.direction()) >= sun.angular_radius_rad.cos() { sun.solid_angle().recip() } else { 0.0 })
}
pub(super) fn radiance(sun: Sun, direction: Vec3) -> Result<Vec3> {
    Ok(if pdf(sun, direction)? > 0.0 { Vec3::new(sun.radiance[0], sun.radiance[1], sun.radiance[2]) } else { Vec3::ZERO })
}
pub(super) fn sample(sun: Sun, u: [f64; 2]) -> Result<Sample> {
    sun.validate().map_err(|_| invalid())?;
    if u.into_iter().any(|v| !v.is_finite() || !(0.0..1.0).contains(&v)) {
        return Err(invalid());
    }
    let n = sun.direction();
    let axis = if n.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
    let tangent = unit_direction(axis.cross(n)).map_err(|_| invalid())?;
    let z = 1.0 - u[0] * sun.solid_angle() / (2.0 * PI);
    let radius = ((1.0 - z) * (1.0 + z)).max(0.0).sqrt();
    let phi = TAU * u[1];
    let direction = unit_direction(tangent * (radius * phi.cos()) + n.cross(tangent) * (radius * phi.sin()) + n * z).map_err(|_| invalid())?;
    Ok(Sample { direction, radiance: Vec3::new(sun.radiance[0], sun.radiance[1], sun.radiance[2]), pdf: sun.solid_angle().recip() })
}
