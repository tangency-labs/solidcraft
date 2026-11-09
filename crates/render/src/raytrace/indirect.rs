//! Original f64 opaque path proposals and finite rectangular emitter geometry.
//! Evaluation remains the shared surface BRDF; no radiance clamp or changed material model.
use super::{Ray, Result, TraceError};
use crate::job::RectangleLightOptions;
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_viewport::surface::{OpaqueSurface, unit_direction};
use std::f64::consts::{PI, TAU};

fn invalid(message: &'static str) -> TraceError {
    TraceError::InvalidInput(message)
}
fn vector(v: [f64; 3]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}
pub(super) fn multiply(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

/// Ratio form remains finite when the competing strategy has a very large density.
pub(super) fn power_weight(a: f64, b: f64) -> Result<f64> {
    if !a.is_finite() || !b.is_finite() || a < 0.0 || b < 0.0 {
        return Err(invalid("invalid CPU MIS density"));
    }
    if a == 0.0 {
        return Ok(0.0);
    }
    let ratio = b / a;
    Ok(1.0 / (1.0 + ratio * ratio))
}
pub(super) fn surface_pdf(surface: OpaqueSurface, n: Vec3, v: Vec3, l: Vec3) -> Result<f64> {
    solidcraft_viewport::clear_coat::surface_pdf(surface, n, v, l).map_err(|_| invalid("invalid CPU path mixture PDF"))
}
fn from_local(n: Vec3, local: Vec3) -> Result<Vec3> {
    let axis = if n.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
    let tangent = unit_direction(axis.cross(n)).map_err(|_| invalid("CPU path tangent"))?;
    unit_direction(tangent * local.x + n.cross(tangent) * local.y + n * local.z).map_err(|_| invalid("CPU path local direction"))
}
fn cosine_direction(n: Vec3, u: [f64; 2]) -> Result<Vec3> {
    let radius = u[0].sqrt();
    let phi = TAU * u[1];
    from_local(n, Vec3::new(radius * phi.cos(), radius * phi.sin(), (1.0 - u[0]).sqrt()))
}

#[derive(Clone, Copy)]
pub(super) struct PhotonGuide {
    pub center: Vec3,
    pub radius: f64,
}
#[derive(Clone, Copy)]
pub(super) struct PhotonCone {
    axis: Vec3,
    width: f64,
}
impl PhotonGuide {
    pub(super) fn cone(self, origin: Vec3, maximum_width: f64) -> Option<PhotonCone> {
        let offset = self.center - origin;
        let distance = offset.len();
        if distance <= self.radius || !distance.is_finite() {
            return None;
        }
        let sine_squared = (self.radius / distance).powi(2);
        // Width is 1-cos(half angle). This form retains a small solid angle.
        let width = sine_squared / (1.0 + (1.0 - sine_squared).sqrt());
        // Share the finite-precision envelope with WGSL. Wide cones do not
        // improve the peak density of the original cosine or spot proposal.
        (1e-5..maximum_width.min(0.5)).contains(&width).then_some(PhotonCone { axis: offset / distance, width })
    }
}
impl PhotonCone {
    pub(super) const MIX: f64 = 0.75;

    pub(super) fn sample(self, [u, v]: [f64; 2]) -> Result<Vec3> {
        let height = self.width * u;
        let sine = (height * (2.0 - height)).sqrt();
        let phi = TAU * v;
        from_local(self.axis, Vec3::new(sine * phi.cos(), sine * phi.sin(), 1.0 - height))
    }

    pub(super) fn mixture_pdf(self, direction: Vec3, baseline_pdf: f64, sampled_cone: bool) -> f64 {
        // Cross magnitude retains the narrow cone's angular separation when
        // 1-dot rounds away, as in the matching GPU proposal.
        let inside = sampled_cone || (self.axis.dot(direction) > 0.0 && self.axis.cross(direction).len2() <= self.width * (2.0 - self.width));
        let guide_pdf = if inside { 1.0 / (TAU * self.width) } else { 0.0 };
        (1.0 - Self::MIX) * baseline_pdf + Self::MIX * guide_pdf
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    pub direction: Vec3,
    pub pdf: f64,
    pub weight: Vec3,
}

/// The base mixes cosine and visible GGX normals equally. The coat selects its
/// own visible normals with probability weight/2. Null mass is never retried.
pub(super) fn sample(surface: OpaqueSurface, n: Vec3, geometric: Vec3, v: Vec3, u: [f64; 3]) -> Result<Option<Sample>> {
    if u.into_iter().any(|x| !x.is_finite() || !(0.0..1.0).contains(&x)) {
        return Err(invalid("CPU path variates must be in [0,1)"));
    }
    let n = unit_direction(n).map_err(|_| invalid("CPU path shading normal"))?;
    let geometric = unit_direction(geometric).map_err(|_| invalid("CPU path geometric normal"))?;
    let v = unit_direction(v).map_err(|_| invalid("CPU path view"))?;
    if n.dot(v) <= 0.0 || geometric.dot(v) <= 0.0 {
        return Ok(None);
    }
    let coat_probability = surface.clear_coat().map_or(0.0, |coat| coat.weight() * 0.5);
    let base_component = (u[0] - coat_probability) / (1.0 - coat_probability);
    let direction = if u[0] >= coat_probability && base_component < 0.5 {
        cosine_direction(n, [u[1], u[2]])?
    } else {
        let roughness = if u[0] < coat_probability {
            surface.clear_coat().ok_or_else(|| invalid("CPU coat selection disappeared"))?.roughness()
        } else {
            surface.roughness()
        };
        let half =
            solidcraft_viewport::surface::visible_normal(v, n, roughness * roughness, [u[1], u[2]]).map_err(|_| invalid("CPU visible GGX normal"))?;
        if v.dot(half) <= 0.0 {
            return Ok(None);
        }
        unit_direction(half * (2.0 * v.dot(half)) - v).map_err(|_| invalid("CPU reflected proposal"))?
    };
    let cosine = n.dot(direction);
    if cosine <= 0.0 || geometric.dot(direction) <= 0.0 {
        return Ok(None);
    }
    let pdf = surface_pdf(surface, n, v, direction)?;
    if pdf <= 0.0 {
        return Err(invalid("CPU sampled direction has zero mixture PDF"));
    }
    let weight = surface.brdf(n, v, direction).map_err(|_| invalid("CPU path BRDF"))? * (cosine / pdf);
    super::lighting::validate_radiance(weight)?;
    Ok(Some(Sample { direction, pdf, weight }))
}

#[derive(Clone, Copy)]
pub(super) struct Rectangle {
    center: Vec3,
    u: Vec3,
    v: Vec3,
    normal: Vec3,
    area: f64,
    pub radiance: Vec3,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct LightSample {
    pub position: Vec3,
    pub direction: Vec3,
    pub pdf: f64,
}
impl Rectangle {
    fn position(self, u: [f64; 2]) -> Vec3 {
        self.center + self.u * (u[0] - 0.5) + self.v * (u[1] - 0.5)
    }
    pub(super) fn emit_photon(self, position: [f64; 2], direction: [f64; 2], guided: Option<(PhotonGuide, f64)>) -> Result<(Vec3, Vec3, Vec3)> {
        let origin = self.position(position);
        if let Some((cone, select)) = guided.and_then(|(guide, select)| guide.cone(origin, 0.5).map(|cone| (cone, select))) {
            let sampled_cone = select < PhotonCone::MIX;
            let outgoing = if sampled_cone { cone.sample(direction)? } else { cosine_direction(self.normal, direction)? };
            let cosine = self.normal.dot(outgoing).max(0.0);
            if cosine == 0.0 {
                return Ok((origin, outgoing, Vec3::ZERO));
            }
            let pdf = cone.mixture_pdf(outgoing, cosine / PI, sampled_cone);
            return Ok((origin, outgoing, self.radiance * (self.area * cosine / pdf)));
        }
        // The position PDF is 1/area and the direction PDF is cos(theta)/pi.
        // Dividing emitted L*cos(theta) by their product gives constant power.
        Ok((origin, cosine_direction(self.normal, direction)?, self.radiance * (PI * self.area)))
    }
    /// Use the actual analytic consumer's plane arithmetic for event precision.
    pub(super) fn on_plane(&self, point: Vec3) -> bool {
        self.normal.dot(self.center - point) == 0.0
    }
    pub(super) fn new(input: &RectangleLightOptions) -> Result<Self> {
        // Reuse the existing bounds/units, including corners and orthogonal side vectors.
        let checked = super::area::RectangleLight::new(
            vector(input.center_mm),
            vector(input.edge_u_mm),
            vector(input.edge_v_mm),
            vector(input.color),
            input.intensity,
        )?;
        Ok(Self {
            center: vector(input.center_mm),
            u: vector(input.edge_u_mm),
            v: vector(input.edge_v_mm),
            normal: checked.normal(),
            area: checked.area_mm2(),
            radiance: vector(input.color) * input.intensity,
        })
    }
    pub(super) fn pdf(self, origin: Vec3, position: Vec3) -> Result<f64> {
        let delta = position - origin;
        let distance = delta.len();
        if !distance.is_finite() || distance <= 0.0 || distance > super::MAX_DISTANCE_MM {
            return Err(invalid("CPU emitter distance outside ray bounds"));
        }
        let cosine = self.normal.dot(-delta / distance);
        if cosine <= 0.0 {
            return Ok(0.0);
        }
        let density = (distance * distance) / (self.area * cosine);
        if !density.is_finite() {
            return Err(invalid("CPU emitter PDF overflow"));
        }
        Ok(density)
    }
    pub(super) fn sample(self, origin: Vec3, u: [f64; 2]) -> Result<Option<LightSample>> {
        if u.into_iter().any(|x| !x.is_finite() || !(0.0..1.0).contains(&x)) {
            return Err(invalid("CPU emitter variates"));
        }
        let position = self.position(u);
        let pdf = self.pdf(origin, position)?;
        if pdf == 0.0 {
            return Ok(None);
        }
        let delta = position - origin;
        let distance = delta.len();
        Ok(Some(LightSample { position, direction: delta / distance, pdf }))
    }
    /// One-sided exact plane/basis intersection. Geometry wins equal-distance ties.
    pub(super) fn intersect(self, ray: Ray) -> Result<Option<(f64, Vec3)>> {
        self.intersect_facing(ray, false, ray.interval()[0])
    }
    /// Reverse paths evaluate the emitting side from the camera's direction.
    pub(super) fn intersect_reverse(self, ray: Ray, before_origin: bool) -> Result<Option<(f64, Vec3)>> {
        let near = if before_origin { -super::MAX_DISTANCE_MM } else { ray.interval()[0] };
        self.intersect_facing(ray, true, near)
    }
    fn intersect_facing(self, ray: Ray, reverse: bool, near: f64) -> Result<Option<(f64, Vec3)>> {
        let denominator = ray.direction().dot(self.normal);
        if denominator == 0.0 || (denominator > 0.0) != reverse {
            return Ok(None);
        }
        let distance = (self.center - ray.origin()).dot(self.normal) / denominator;
        let far = ray.interval()[1];
        if !distance.is_finite() || distance < near || distance > far {
            return Ok(None);
        }
        let position = ray.origin() + ray.direction() * distance;
        let delta = position - self.center;
        // The admitted contract permits a small nonzero U.V. Independent projections
        // fail badly for extreme aspect ratios; solve the actual two-vector basis.
        let uu = self.u.len2();
        let vv = self.v.len2();
        let uv = self.u.dot(self.v);
        let determinant = uu * vv - uv * uv;
        if !determinant.is_finite() || determinant <= 0.0 {
            return Err(invalid("CPU emitter basis is singular"));
        }
        let du = delta.dot(self.u);
        let dv = delta.dot(self.v);
        let u = (du * vv - dv * uv) / determinant;
        let v = (dv * uu - du * uv) / determinant;
        if !(-0.5..=0.5).contains(&u) || !(-0.5..=0.5).contains(&v) {
            return Ok(None);
        }
        Ok(Some((distance, position)))
    }
}

/// Source samplers require an open unit interval. Counter samples and camera
/// jitter retain their original [0,1) contract; only the stratified source input
/// is repaired at floating-point endpoints, including last-cell rounding to one.
pub(super) fn open_stratum(raw: f64, cell: u32, side: u32) -> Result<f64> {
    if !raw.is_finite() || !(0.0..1.0).contains(&raw) || !matches!(side, 1 | 2 | 4 | 8) || cell >= side {
        return Err(invalid("CPU source stratum outside its bounded domain"));
    }
    let value = (f64::from(cell) + raw) / f64::from(side);
    Ok(value.max(0.0_f64.next_up()).min(1.0_f64.next_down()))
}

pub(super) fn uniform_environment(u: [f64; 2]) -> Vec3 {
    let z = 1.0 - 2.0 * u[0];
    let radius = (1.0 - z * z).max(0.0).sqrt();
    let phi = TAU * u[1];
    Vec3::new(radius * phi.cos(), radius * phi.sin(), z)
}
pub(super) const ENVIRONMENT_PDF: f64 = 1.0 / (4.0 * PI);

#[cfg(test)]
#[path = "indirect_tests.rs"]
mod tests;
