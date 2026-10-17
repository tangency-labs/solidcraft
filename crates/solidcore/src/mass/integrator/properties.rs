//! Precise edge, face and B-rep properties and their SM_API query contracts.
//!
//! Upstream: `source/SMLib/src/SmEdge.cpp`, `source/SMLib/src/SmFace.cpp`,
//! `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmTopoTypes.h`,
//! `source/SM_API/src/SmApiQueries.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`edge_properties`] | `SmEdge::ComputePreciseProperties` |
//! | [`face_properties`], [`boundary_properties`] | `SmFace::ComputePreciseProperties` |
//! | [`brep_properties`] | `SmBrep::ComputePreciseProperties` |
//! | [`MassPropertyBehavior`] | `SmMassPropertyBehavior` |
//! | [`edge_length`], [`face_area`], [`face_centroid`] | `SmApiEdgeComputeLength`, `SmApiFaceComputeArea`, `SmApiFaceComputeCentroid` |
//! | [`brep_area`], [`brep_volume`], [`brep_mass_properties`] | `SmApiBrepComputeArea`, `SmApiBrepComputeVolume`, `SmApiBrepComputeMassProperties` |
//!
//! Deviations: arena ids replace pointers, results replace output arguments; attribute
//! storage and object keys are supplied by the caller. Oriented trim construction,
//! B-rep enumeration, bounding boxes, and region classification remain calls into
//! their unported units. `boundary_properties` exposes the integration after trim
//! construction for callers already holding oriented trim curves. Nested integration
//! has a fixed two-level call structure, not input-driven recursion.
use super::{OdeIntegrator, OdeOptions};
use crate::curve::curve::Curve;
use crate::geom::attribute::{ids, AttributeStore, AttributeValue, ObjectKey};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::face::p2::{
    OdeArea, PrecisePropertyLayout, PPF_ALL, PPF_AREA, PPF_AREA_FIRST_MOMENTS,
};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, RegionId, VertexId};
use crate::{EntityRef, Result, SolidError};

const OP: &str = "mass::integrator::properties";

/// Surface/volume measures and the eight moment vectors of upstream.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreciseProperties {
    /// Surface area.
    pub area: f64,
    /// Enclosed/tube volume.
    pub volume: f64,
    /// Density-weighted volume (B-rep layer only).
    pub mass: f64,
    /// Area first, second, products, axial; then volume/mass equivalents.
    pub moments: [Vec3; 8],
}
/// Edge length and tube properties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EdgeProperties {
    /// Curve length on the edge interval.
    pub length: f64,
    /// Tube properties; its last four vectors are geometric volume integrals.
    pub properties: PreciseProperties,
}
/// Controls of `SmMassPropertyBehavior`.
#[derive(Debug, Clone, Copy)]
pub struct MassPropertyBehavior {
    /// Default density, overridden by per-entity mass attributes.
    pub density: f64,
    /// Thickness of sheet faces.
    pub face_thickness: f64,
    /// Radius of wire-edge tubes.
    pub edge_radius: f64,
    /// Radius of isolated-vertex spheres.
    pub vertex_radius: f64,
    /// Area policy: exterior of solid (0), once per face (1), both uses (2).
    pub face_area: u32,
    /// Volume policy: solid regions (0), thickness only (1).
    pub face_volume: u32,
}
impl Default for MassPropertyBehavior {
    fn default() -> Self {
        Self {
            density: 1.0,
            face_thickness: 0.0,
            edge_radius: 0.0,
            vertex_radius: 0.0,
            face_area: 0,
            face_volume: 0,
        }
    }
}
/// The session attribute store and the identities of topology records in it.
pub struct PropertyAttributes<'a> {
    /// Attribute storage.
    pub store: &'a AttributeStore,
    /// Converts an arena entity to its attribute identity.
    pub key_of: &'a dyn Fn(EntityRef) -> ObjectKey,
}
impl PropertyAttributes<'_> {
    fn mass_vector(&self, entity: EntityRef, default: Vec3) -> Result<Vec3> {
        let Some(h) = self
            .store
            .find_attribute((self.key_of)(entity), ids::MASS_PROPERTIES)
        else {
            return Ok(default);
        };
        match self
            .store
            .get(h)
            .map(crate::geom::attribute::Attribute::value)
        {
            Some(AttributeValue::Vector3d(v) | AttributeValue::Color(v)) => Ok(*v),
            _ => Err(SolidError::invalid_input(
                OP,
                "mass attribute is not a vector",
            )),
        }
    }
    fn has_mass(&self, entity: EntityRef) -> bool {
        self.store
            .find_attribute((self.key_of)(entity), ids::MASS_PROPERTIES)
            .is_some()
    }
}

/// `SmEdge::ComputePreciseProperties` on a live edge.
pub fn edge_properties(
    b: &Brep,
    e: EdgeId,
    accuracy: f64,
    origin: Point3,
    radius: f64,
) -> Result<EdgeProperties> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    curve_properties(curve, &edge::interval(b, e)?, accuracy, origin, radius)
}
/// The precise edge integration on an already-bound curve and interval.
pub fn curve_properties(
    curve: &Curve,
    interval: &Extent1d,
    accuracy: f64,
    origin: Point3,
    radius: f64,
) -> Result<EdgeProperties> {
    let eval = edge::OdeEdgeLength::new(curve, origin, radius);
    let approx = curve.approximate_length(interval, 10, false)?.0;
    let mut y = [0.0; 27];
    let mut scale = [BIG_DOUBLE; 27];
    if let Some(s) = scale.first_mut() {
        *s = approx / 10.0;
    }
    let mut scratch = [0.0; 28];
    OdeIntegrator::default().integrate(
        &mut y,
        interval.min(),
        interval.max(),
        OdeOptions {
            accuracy,
            initial_step: (interval.max() - interval.min()) / 2.0,
            minimum_step: 0.0,
            scale: Some(&scale),
        },
        |t, _, dy| {
            eval.evaluate(t, &mut scratch)?;
            for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                *d = *v;
            }
            Ok(())
        },
    )?;
    let mut iter = y.into_iter();
    let length = iter.next().unwrap_or(0.0);
    let mut properties = PreciseProperties {
        area: iter.next().unwrap_or(0.0),
        volume: iter.next().unwrap_or(0.0),
        ..PreciseProperties::default()
    };
    for m in &mut properties.moments {
        *m = Vec3::new(
            iter.next().unwrap_or(0.0),
            iter.next().unwrap_or(0.0),
            iter.next().unwrap_or(0.0),
        );
    }
    Ok(EdgeProperties { length, properties })
}

/// `SmFace::CreateOrientedTrimBoundary(ctx, TRUE, TRUE, ..)` (unit `topology.face_p1`) on
/// a copy of the B-rep (the caller holds it shared; the UV trim curves the call makes are
/// not kept; the copy is a whole-B-rep clone per face). The copied curves' edge-use
/// owners are not read here.
fn boundary(k: &Kernel, b: &Brep, f: FaceId) -> Result<Vec<Curve>> {
    let mut copy = b.clone();
    let key_of = |e: EntityRef| crate::geom::object::ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    crate::topology::face::p1::create_oriented_trim_boundary(
        k, &mut copy, f, true, true, None, &key_of,
    )
    .map(|t| t.curves)
}
/// `SmFace::ComputePreciseProperties`; the default property mask is `PPF_ALL`.
pub fn face_properties(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    orientation: OrientType,
    accuracy: f64,
    origin: Point3,
    estimated_area: f64,
    thickness: f64,
    flags: u64,
) -> Result<PreciseProperties> {
    valid_flags(flags)?;
    let trims = boundary(k, b, f)?;
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    boundary_properties(
        k,
        surface,
        &trims,
        &b.face(f)?.uv_domain,
        orientation,
        accuracy,
        origin,
        estimated_area,
        thickness,
        flags,
    )
}
fn valid_flags(flags: u64) -> Result<()> {
    if flags == 0 || flags & !PPF_ALL != 0 {
        Err(SolidError::invalid_input(OP, "invalid property mask"))
    } else {
        Ok(())
    }
}
/// Integrates the same precise face properties after oriented trim construction.
/// `trims` must have the orientation and order returned by `CreateOrientedTrimBoundary`.
pub fn boundary_properties(
    k: &Kernel,
    surface: &Surface,
    trims: &[Curve],
    domain: &Extent2d,
    orientation: OrientType,
    mut accuracy: f64,
    origin: Point3,
    estimated_area: f64,
    thickness: f64,
    flags: u64,
) -> Result<PreciseProperties> {
    valid_flags(flags)?;
    if accuracy < EFF_ZERO {
        accuracy = EFF_ZERO_SQ;
    }
    let layout = PrecisePropertyLayout::new(flags);
    let mut scale = vec![BIG_DOUBLE; layout.num_components];
    let sign = if orientation == OrientType::Same && thickness < EFF_ZERO {
        -1.0
    } else {
        1.0
    };
    let mut result = PreciseProperties::default();
    for trim in trims {
        if let Some(s) = scale.first_mut() {
            *s = estimated_area / 10.0;
        }
        let eval = OdeArea {
            kernel: k,
            surface,
            uv_curve: trim,
            min_v: domain.min().y,
            iso_curve_accuracy: accuracy / 10.0,
            y_scale: &scale,
            origin,
            face_thickness: thickness,
            layout: &layout,
        };
        let ivl = trim.natural_interval();
        let span = ivl.max() - ivl.min();
        let mut y = vec![0.0; layout.num_components];
        let mut scratch = vec![0.0; layout.num_components + 1];
        let mut status = Err(SolidError::sm_err(OP));
        for divisor in [2.0, 8.0, 64.0, 512.0, 4096.0] {
            y.fill(0.0);
            status = OdeIntegrator::default()
                .integrate(
                    &mut y,
                    ivl.min(),
                    ivl.max(),
                    OdeOptions {
                        accuracy: accuracy / 10.0,
                        initial_step: span / divisor,
                        minimum_step: 0.0,
                        scale: Some(&scale),
                    },
                    |t, _, dy| {
                        // A zero U tangent leaves the caller's derivative array
                        // untouched upstream. Preserve this particular stage's
                        // values, rather than another stage's shared scratch.
                        if let Some(unused) = scratch.first_mut() {
                            *unused = 0.0;
                        }
                        for (s, d) in scratch.iter_mut().skip(1).zip(dy.iter()) {
                            *s = *d;
                        }
                        eval.evaluate(t, &mut scratch)?;
                        for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                            *d = *v;
                        }
                        Ok(())
                    },
                )
                .map(|_| ());
            if status.is_ok() {
                break;
            }
            if status.as_ref().is_err_and(SolidError::is_unported) {
                break;
            }
        }
        status?;
        let value = |index: usize| y.get(index.wrapping_sub(1)).copied().unwrap_or(0.0);
        if layout.area != 0 {
            result.area += value(layout.area);
        }
        if layout.volume != 0 {
            result.volume += value(layout.volume) * sign;
        }
        for (kk, (m, &index)) in result
            .moments
            .iter_mut()
            .zip(layout.moments.iter())
            .enumerate()
        {
            if index == 0 {
                continue;
            }
            let moment_sign = if kk < 4 { 1.0 } else { sign };
            m.x += value(index) * moment_sign;
            m.y += value(index + 1) * moment_sign;
            m.z += value(index + 2) * moment_sign;
        }
    }
    Ok(result)
}

fn brep_box(_b: &Brep) -> Result<Extent3d> {
    // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
    Err(SolidError::unported(
        "SmBrep::CalculateBoundingBox",
        "query.brep",
    ))
}
fn face_box(_b: &Brep, _f: FaceId) -> Result<Extent3d> {
    // UNPORTED(query.face_p1): SmFace::CalculateBoundingBox
    Err(SolidError::unported(
        "SmFace::CalculateBoundingBox",
        "query.face_p1",
    ))
}
fn faces(_b: &Brep) -> Result<Vec<FaceId>> {
    // UNPORTED(topology.brep_p1): SmBrep::GetFaces
    Err(SolidError::unported("SmBrep::GetFaces", "topology.brep_p1"))
}
fn edges(_b: &Brep) -> Result<Vec<EdgeId>> {
    // UNPORTED(topology.brep_p1): SmBrep::GetEdges
    Err(SolidError::unported("SmBrep::GetEdges", "topology.brep_p1"))
}
fn vertices(_b: &Brep) -> Result<Vec<VertexId>> {
    // UNPORTED(topology.brep_p1): SmBrep::GetVertices
    Err(SolidError::unported(
        "SmBrep::GetVertices",
        "topology.brep_p1",
    ))
}
fn region_void(_b: &Brep, _r: RegionId) -> Result<bool> {
    // UNPORTED(topology.shell): SmRegion::IsVoid
    Err(SolidError::unported("SmRegion::IsVoid", "topology.shell"))
}
fn manifold_solid(_b: &Brep) -> Result<bool> {
    // UNPORTED(topology.brep_p2): SmBrep::IsManifoldSolid
    Err(SolidError::unported(
        "SmBrep::IsManifoldSolid",
        "topology.brep_p2",
    ))
}
fn add(total: &mut PreciseProperties, delta: &PreciseProperties, density: f64) {
    total.area += delta.area;
    total.volume += delta.volume;
    total.mass += delta.volume * density;
    for (i, (t, d)) in total
        .moments
        .iter_mut()
        .zip(delta.moments.iter())
        .enumerate()
    {
        *t += if i > 3 { *d * density } else { *d };
    }
}
/// `SmBrep::ComputePreciseProperties`, including density attributes and wire/point solids.
pub fn brep_properties(
    k: &Kernel,
    b: &Brep,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
) -> Result<PreciseProperties> {
    let bbox = brep_box(b)?;
    let size = bbox.size();
    let estimated = size.z.max(size.x.max(size.y));
    let estimated = estimated * estimated / 100.0;
    let mut total = PreciseProperties::default();
    for f in faces(b)? {
        let mut mass = attributes.mass_vector(
            EntityRef::Face(f),
            Vec3::new(behavior.density, behavior.face_thickness, 0.0),
        )?;
        if mass.x < EFF_ZERO {
            mass.x = 1.0;
        }
        let up = b
            .face(f)?
            .faceuse
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let mate = b
            .faceuse(up)?
            .mate
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let region = |fu| -> Result<RegionId> {
            let shell = b
                .faceuse(fu)?
                .shell
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            b.shell(shell)?
                .region
                .ok_or_else(|| SolidError::null_pointer(OP))
        };
        let mut separating = region(up)? != region(mate)?;
        for (j, fu) in [up, mate].into_iter().enumerate() {
            let mut do_area = false;
            let mut do_volume = false;
            if behavior.face_volume == 1 {
                separating = false;
            }
            if !separating && mass.y > EFF_ZERO && j == 0 {
                do_area = true;
            }
            match behavior.face_area {
                0 => {
                    let other = b
                        .faceuse(fu)?
                        .mate
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    if !region_void(b, region(fu)?)? && region_void(b, region(other)?)? {
                        do_area = true;
                    }
                }
                1 => {
                    if j == 0 {
                        do_area = true;
                    }
                }
                2 => do_area = true,
                _ => {}
            }
            if behavior.face_volume == 0 && !region_void(b, region(fu)?)? {
                do_volume = true;
            }
            if !(do_area || do_volume) {
                continue;
            }
            if separating || mass.y > EFF_ZERO {
                let delta = face_properties(
                    k,
                    b,
                    f,
                    b.faceuse(fu)?.orientation,
                    accuracy,
                    origin,
                    estimated,
                    if separating { 0.0 } else { mass.y },
                    PPF_ALL,
                )?;
                add(&mut total, &delta, mass.x);
                if !separating {
                    break;
                }
            }
        }
    }
    for e in edges(b)? {
        if !edge::is_wire(b, e)? {
            continue;
        }
        let mut mass = attributes.mass_vector(
            EntityRef::Edge(e),
            Vec3::new(behavior.density, behavior.edge_radius, 0.0),
        )?;
        let delta = edge_properties(b, e, accuracy, origin, mass.y)?;
        if mass.x < EFF_ZERO {
            mass.x = 1.0;
        }
        add(&mut total, &delta.properties, mass.x);
    }
    for v in vertices(b)? {
        if !vertex::is_shell_vertex(b, v)? {
            continue;
        }
        let mut mass = attributes.mass_vector(
            EntityRef::Vertex(v),
            Vec3::new(behavior.density, behavior.vertex_radius, 0.0),
        )?;
        if mass.x < EFF_ZERO {
            mass.x = 1.0;
        }
        let volume = 4.0 * PI * mass.y.powf(3.0) / 3.0;
        let area = 4.0 * PI * mass.y * mass.y;
        total.volume += volume;
        total.area += area;
        total.mass += volume * mass.x;
        // Upstream uses world coordinates here even when the computation origin differs.
        let p = b.vertex(v)?.point;
        let [m0, m1, m2, m3, m4, m5, m6, m7] = &mut total.moments;
        *m0 += area * p;
        *m1 += area * Vec3::new(p.x * p.x, p.y * p.y, p.z * p.z);
        m2.x += area * p.y * p.z;
        m2.y += area * p.z * p.x;
        m2.z += area * p.x * p.y;
        m3.x += area * (p.y * p.y + p.z * p.z);
        m3.y += area * (p.x * p.x + p.z * p.z);
        m3.z += area * (p.x * p.x + p.y * p.y);
        *m4 += volume * p * mass.x;
        *m5 += volume * mass.x * Vec3::new(p.x * p.x, p.y * p.y, p.z * p.z);
        m6.x += volume * mass.x * p.y * p.z;
        m6.y += volume * mass.x * p.z * p.x;
        m6.z += volume * mass.x * p.x * p.y;
        m7.x += volume * mass.x * (p.y * p.y + p.z * p.z);
        m7.y += volume * mass.x * (p.x * p.x + p.z * p.z);
        m7.z += volume * mass.x * (p.x * p.x + p.y * p.y);
    }
    Ok(total)
}
/// `SmApiEdgeComputeLength` (the curve's virtual Length).
pub fn edge_length(k: &Kernel, b: &Brep, e: EdgeId, accuracy: f64) -> Result<f64> {
    let c = edge::curve(b, e)?.ok_or_else(|| SolidError::sm_err(OP))?;
    c.length(k, &edge::interval(b, e)?, accuracy)
}
fn area_estimate(bbox: &Extent3d) -> f64 {
    let s = bbox.size();
    let d = s.x.max(s.y).max(s.z);
    d * d / 100.0
}
/// `SmApiFaceComputeArea`: clamps accuracy and integrates only area.
pub fn face_area(k: &Kernel, b: &Brep, f: FaceId, accuracy: f64) -> Result<f64> {
    let bbox = face_box(b, f)?;
    Ok(face_properties(
        k,
        b,
        f,
        OrientType::Same,
        accuracy.clamp(1e-4, 1e-1),
        bbox.mid(),
        area_estimate(&bbox),
        0.0,
        PPF_AREA,
    )?
    .area
    .abs())
}
/// `SmApiFaceComputeCentroid`: one area/first-moment integration.
pub fn face_centroid(k: &Kernel, b: &Brep, f: FaceId, accuracy: f64) -> Result<(f64, Point3)> {
    let bbox = face_box(b, f)?;
    let origin = bbox.mid();
    let p = face_properties(
        k,
        b,
        f,
        OrientType::Same,
        accuracy.clamp(1e-4, 1e-1),
        origin,
        area_estimate(&bbox),
        0.0,
        PPF_AREA | PPF_AREA_FIRST_MOMENTS,
    )?;
    if p.area.abs() < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "zero area"));
    }
    Ok((
        p.area.abs(),
        origin + *p.moments.first().ok_or_else(|| SolidError::sm_err(OP))? / p.area,
    ))
}
/// `SmApiBrepComputeArea`, including open shells and sheets.
pub fn brep_area(k: &Kernel, b: &Brep, accuracy: f64) -> Result<f64> {
    valid_api_number(accuracy)?;
    let mut area = 0.0;
    for f in faces(b)? {
        area += face_area(k, b, f, accuracy)?;
    }
    Ok(area)
}
/// `SmApiBrepComputeVolume`, computed about the bounding-box midpoint.
pub fn brep_volume(
    k: &Kernel,
    b: &Brep,
    accuracy: f64,
    attributes: &PropertyAttributes<'_>,
) -> Result<f64> {
    valid_api_number(accuracy)?;
    let bbox = brep_box(b)?;
    Ok(brep_properties(
        k,
        b,
        accuracy.clamp(1e-8, 1e-1),
        bbox.mid(),
        MassPropertyBehavior::default(),
        attributes,
    )?
    .volume)
}
/// Result of `SmApiBrepComputeMassProperties`.
#[derive(Debug, Clone, PartialEq)]
pub struct MassProperties {
    /// Precise measures and density-weighted moment vectors.
    pub properties: PreciseProperties,
    /// Center of mass in world coordinates.
    pub centroid: Point3,
    /// Mass axial inertia about the caller's origin.
    pub inertia: Vec3,
    /// Raw positive products yz, zx, xy about that origin.
    pub products: Vec3,
}
fn valid_api_number(value: f64) -> Result<()> {
    if !value.is_finite() || value.abs() > BIG_DOUBLE / 100.0 {
        Err(SolidError::invalid_input(OP, "invalid numeric argument"))
    } else {
        Ok(())
    }
}
/// `SmApiBrepComputeMassProperties`, with upstream validation and accuracy clamping.
pub fn brep_mass_properties(
    k: &Kernel,
    b: &Brep,
    accuracy: f64,
    density: f64,
    origin: Point3,
    attributes: &PropertyAttributes<'_>,
) -> Result<MassProperties> {
    valid_api_number(accuracy)?;
    valid_api_number(density)?;
    if density < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "density below epsilon"));
    }
    for v in [origin.x, origin.y, origin.z] {
        valid_api_number(v)?;
    }
    if faces(b)?
        .into_iter()
        .any(|f| attributes.has_mass(EntityRef::Face(f)))
        || edges(b)?
            .into_iter()
            .any(|e| attributes.has_mass(EntityRef::Edge(e)))
        || vertices(b)?
            .into_iter()
            .any(|v| attributes.has_mass(EntityRef::Vertex(v)))
    {
        return Err(SolidError::invalid_input(
            OP,
            "per-entity mass attributes override density",
        ));
    }
    if !manifold_solid(b)? {
        return Err(SolidError::invalid_input(OP, "not a manifold solid"));
    }
    let properties = brep_properties(
        k,
        b,
        accuracy.clamp(1e-4, 1e-1),
        origin,
        MassPropertyBehavior {
            density,
            ..MassPropertyBehavior::default()
        },
        attributes,
    )?;
    valid_api_number(properties.mass)?;
    valid_api_number(properties.volume)?;
    if properties.mass < EFF_ZERO || properties.volume < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "nonpositive mass or volume"));
    }
    let centroid = origin
        + *properties
            .moments
            .get(4)
            .ok_or_else(|| SolidError::sm_err(OP))?
            / properties.mass;
    let inertia = *properties
        .moments
        .get(7)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    let products = *properties
        .moments
        .get(6)
        .ok_or_else(|| SolidError::sm_err(OP))?;
    Ok(MassProperties {
        properties,
        centroid,
        inertia,
        products,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::{Attribute, Behavior};

    #[test]
    fn vector_attribute_subclasses_supply_mass_values() {
        let value = Vec3::new(2.0, 0.3, 0.0);
        for attribute in [
            Attribute::vector3d(ids::MASS_PROPERTIES, value, Behavior::Copy),
            Attribute::color(ids::MASS_PROPERTIES, value, Behavior::Copy),
        ] {
            let mut store = AttributeStore::default();
            let key = ObjectKey(7);
            let handle = store.insert(attribute).unwrap();
            store.add_attribute(key, handle).unwrap();
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &|_| key,
            };
            assert_eq!(
                attrs
                    .mass_vector(EntityRef::Edge(EdgeId::from_index(0)), Vec3::ZERO)
                    .unwrap(),
                value
            );
        }
    }
}
