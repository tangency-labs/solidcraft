//! Precise edge, face and B-rep properties and their `SM_API` query contracts.
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
//! | [`brep_mass_properties_estimated`], [`MassPropertyRequest`], [`EstimatedMassProperties`], [`sheet_face`], [`brep_area_about`], [`estimate_accuracy`] | not upstream: the same computation with sheets' area, a no-material area centroid and centroidal moments (the SolidCraft fork's `smlib_brep_mass_properties`) |
//!
//! Deviations: arena ids replace pointers, results replace output arguments; attribute
//! storage and object keys are supplied by the caller; the `_default` B-rep wrappers
//! use a local empty attribute store. Oriented trim construction,
//! B-rep enumeration, bounding boxes, and region classification remain calls into
//! their unported units. `boundary_properties` exposes the integration after trim
//! construction for callers already holding oriented trim curves. Nested integration
//! has a fixed two-level call structure, not input-driven recursion. Both levels split
//! at the surface's knots, as the SolidCraft fork's tangency-labs/smlib@08d9a14 does
//! (WR-494, #1435): upstream steps across them.
//! With `UpstreamFixes::brep_area_between_solid_and_void` (WR-535) a volume-separating
//! faceuse adds its volume only where it bounds a non void region and its area only
//! where the area behaviour selects it (tangency-labs/smlib@d2d9586).
//! The iso-u integrals start at the trims' lowest v, not at the face UV domain's
//! (WR-563, #1556).
//! Each span-wise iso-u piece is evaluated at its end from its own knot span, the
//! one-sided limit from inside the piece (WR-670, #2091): upstream takes the span above.
//! An edge's tube area, volume and moments each have an error scale of their own size
//! (#2887): upstream's are `BIG_DOUBLE`, so only the edge's length was under error control.
use super::{OdeIntegrator, OdeOptions};
use crate::curve::curve::Curve;
use crate::geom::attribute::{ids, AttributeStore, AttributeValue, ObjectKey};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::surface::surface::Surface;
use crate::topology::face::p2::{
    trim_curve_breaks, OdeArea, PrecisePropertyLayout, PPF_ALL, PPF_AREA, PPF_AREA_FIRST_MOMENTS,
};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, FaceuseId, RegionId, VertexId};
use crate::{EntityRef, Result, SolidError};

const OP: &str = "mass::integrator::properties";
/// The operation the `SM_ERR` of a span the integrator took no step over names (not
/// upstream: the code is upstream's, the name says which failure it is).
const OP_SKIPPED_SPAN: &str = "mass::integrator::properties::skipped_span";

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
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    accuracy: f64,
    origin: Point3,
    radius: f64,
) -> Result<EdgeProperties> {
    let curve = edge::curve(b, e)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    curve_properties(k, curve, &edge::interval(b, e)?, accuracy, origin, radius)
}
/// The precise edge integration on an already-bound curve and interval.
///
/// Not upstream (#2887): each of the 27 components has an error scale of its own size,
/// so each is held to the accuracy asked, or the integration is refused. Upstream sets
/// the length's to a tenth of the approximate length `L` and every other to
/// `BIG_DOUBLE`: their steps were the length's and their accuracy whatever that gave.
/// Here, as the face's components (`boundary_properties`), a tenth of each measure: the
/// length `L`, the tube's area `2 pi r L` and volume `pi r^2 L`, and a moment of power
/// `n` its measure times `D^n`, where `D` is the start's distance from the origin of
/// computation plus `L`: every point is within its arc length of the start, so `D` is
/// the farthest point's distance or more, up to `L`'s estimate. A moment through zero is held to that size, not to its
/// own value. Without a cross-section (`r = 0`) the tube's integrands are zero and
/// their scales the integrator's floor.
pub fn curve_properties(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    accuracy: f64,
    origin: Point3,
    radius: f64,
) -> Result<EdgeProperties> {
    // the moment groups' powers of length: area static, second, products, axial, then
    // the same of the volume
    const POWERS: [i32; 8] = [1, 2, 2, 2, 1, 2, 2, 2];
    let eval = edge::OdeEdgeLength::new(curve, origin, radius);
    let approx = curve.approximate_length(k, interval, 10, false)?.0;
    let start = curve
        .evaluate(k, interval.min(), 0, false, false)?
        .first()
        .copied()
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let reach = (start - origin).length() + approx;
    let length_scale = approx / 10.0;
    let area_scale = 2.0 * PI * radius.abs() * length_scale;
    let volume_scale = PI * radius * radius * length_scale;
    let mut y = [0.0; 27];
    let mut scale = [0.0; 27];
    let mut slots = scale.iter_mut();
    for (s, value) in slots.by_ref().zip([length_scale, area_scale, volume_scale]) {
        *s = value;
    }
    for (group, power) in POWERS.iter().enumerate() {
        let measure = if group < 4 { area_scale } else { volume_scale };
        let moment_scale = measure * reach.powi(*power);
        for s in slots.by_ref().take(3) {
            *s = moment_scale;
        }
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
            quadrature: true,
            rounding: None,
        },
        |t, _, dy| {
            eval.evaluate(k, t, &mut scratch)?;
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
/// the caller's B-rep: the UV trim curves it makes stay there, as upstream. The returned
/// curves' edge-use owners are not read here.
fn boundary(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Vec<Curve>> {
    let key_of = |e: EntityRef| crate::geom::object::ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    crate::topology::face::p1::create_oriented_trim_boundary(k, b, f, true, true, None, &key_of)
        .map(|t| t.curves)
}
/// `SmFace::ComputePreciseProperties`; the default property mask is `PPF_ALL`. Missing UV
/// trim curves of the face are made on `b`.
pub fn face_properties(
    k: &Kernel,
    b: &mut Brep,
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
    let fbox = face_box(k, b, f)?;
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
        &fbox,
        orientation,
        accuracy,
        origin,
        estimated_area,
        thickness,
        flags,
    )
}
/// The lowest v of the union of the trims' bounding boxes (as
/// `CalculateUVDomainFromUVTrimCurves` builds the trim domain), no lower than the face's
/// UV domain; the domain's lowest v without trims.
fn trims_base_v(k: &Kernel, trims: &[Curve], domain: &Extent2d) -> Result<f64> {
    use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let mut base = BIG_DOUBLE;
    for trim in trims {
        let bbox = trim
            .calculate_bounding_box(k, &trim.natural_interval(), req, true)?
            .normal
            .ok_or_else(|| SolidError::sm_err(OP))?;
        let v = bbox.min().y;
        if v < base {
            base = v;
        }
    }
    let min_v = domain.min().y;
    Ok(if trims.is_empty() || base < min_v {
        min_v
    } else {
        base
    })
}
fn valid_flags(flags: u64) -> Result<()> {
    if flags == 0 || flags & !PPF_ALL != 0 {
        Err(SolidError::invalid_input(OP, "invalid property mask"))
    } else {
        Ok(())
    }
}
/// Integrates the same precise face properties after oriented trim construction.
/// `trims` must have the orientation and order returned by `CreateOrientedTrimBoundary`;
/// `face_box` is the face's bounding box (`CalculateBoundingBox`), which sets the error
/// scale of the volume and moment components.
pub fn boundary_properties(
    k: &Kernel,
    surface: &Surface,
    trims: &[Curve],
    domain: &Extent2d,
    face_box: &Extent3d,
    orientation: OrientType,
    mut accuracy: f64,
    origin: Point3,
    estimated_area: f64,
    thickness: f64,
    flags: u64,
) -> Result<PreciseProperties> {
    // the moment groups' extra length dimensions
    const POWERS: [u32; 8] = [1, 2, 2, 2, 2, 3, 3, 3];
    valid_flags(flags)?;
    if accuracy < EFF_ZERO {
        accuracy = EFF_ZERO_SQ;
    }
    let layout = PrecisePropertyLayout::new(flags);
    let mut scale = vec![BIG_DOUBLE; layout.num_components];
    let origin_distance = face_box.min_distance(&Extent3d::from_point(origin));
    let sign = if orientation == OrientType::Same && thickness < EFF_ZERO {
        -1.0
    } else {
        1.0
    };
    // The base of every iso-u integral: the trims' lowest v. Any base gives the same loop
    // integral (a different base adds a function of u alone, whose integral round a
    // closed loop is zero), but one far below the trims, such as upstream's
    // `GetUVDomain().GetMin().y` on a face whose domain is unbounded in v, makes each
    // integral long and the area a small difference of large terms (WR-563, #1556).
    let base_v = trims_base_v(k, trims, domain)?;
    let mut result = PreciseProperties::default();
    for trim in trims {
        let area_scale = estimated_area / 10.0;
        if let Some(s) = scale.first_mut() {
            *s = area_scale;
        }
        // Scale volume and moment errors by a length per extra dimension. sqrt(area) is
        // a tenth of the body size; grow it once the origin is farther than the body.
        // With `fixes.precise_properties_body_length_scale` (WR-472; the SolidCraft
        // fork's tangency-labs/smlib@073d4740) the length is the body size, ten times
        // sqrt(area), or the origin's distance when larger: each component is then held
        // to the area's share of its own size, where 1.0.1's tenth holds each extra power
        // ten times tighter, below roundoff at tight accuracies.
        let length = if k.fixes.precise_properties_body_length_scale {
            let a = 10.0 * estimated_area.sqrt();
            let b = origin_distance;
            if a > b {
                a
            } else {
                b
            }
        } else {
            let a = estimated_area.sqrt();
            let b = origin_distance / 10.0;
            if a > b {
                a
            } else {
                b
            }
        };
        if layout.volume != 0 {
            if let Some(s) = scale.get_mut(layout.volume - 1) {
                *s = area_scale * length;
            }
        }
        for (&index, &power) in layout.moments.iter().zip(POWERS.iter()) {
            if index == 0 {
                continue;
            }
            let moment_scale = area_scale * length.powf(f64::from(power));
            for cc in 0..3 {
                if let Some(s) = scale.get_mut(index - 1 + cc) {
                    *s = moment_scale;
                }
            }
        }
        let eval = OdeArea {
            kernel: k,
            surface,
            uv_curve: trim,
            min_v: base_v,
            iso_curve_accuracy: accuracy / 10.0,
            y_scale: &scale,
            origin,
            face_thickness: thickness,
            layout: &layout,
        };

        // Integrate piece by piece between the trim curve's knots and its crossings of the
        // surface's knot lines u = k (WR-494; the SolidCraft fork's
        // tangency-labs/smlib@08d9a14): at either the integrand's derivatives jump, and
        // at a knot of multiplicity reaching the degree (a rational circle's double knot)
        // the integrand itself does. A step's error estimate is the leading term of an
        // expansion that holds only where the integrand is smooth across the step.
        let knots = trim_curve_breaks(k, trim, surface)?;

        // A coarse first Runge-Kutta step can trip the integrator's abort guard for one
        // traversal direction, so a reversed cap loop fails where its twin succeeds. Try
        // a span/2 first step, then fall back to finer steps.
        let mut y = vec![0.0; layout.num_components];
        let mut span_y = vec![0.0; layout.num_components];
        let mut scratch = vec![0.0; layout.num_components + 1];
        for pair in knots.windows(2) {
            let &[a, b] = pair else {
                continue;
            };
            // (`!(dB > dA)`: also an unordered pair)
            if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                continue; // a crossing at a knot
            }
            let mut status = Err(SolidError::sm_err(OP));
            for divisor in [2.0, 8.0, 64.0, 512.0, 4096.0] {
                k.check_cancelled(OP)?;
                let mut eval_calls = 0u64;
                status = OdeIntegrator::default()
                    .integrate(
                        &mut span_y,
                        a,
                        b,
                        OdeOptions {
                            accuracy: accuracy / 10.0,
                            initial_step: (b - a) / divisor,
                            minimum_step: 0.0,
                            scale: Some(&scale),
                            quadrature: true,
                            rounding: None,
                        },
                        |t, _, dy| {
                            eval_calls += 1;
                            if (eval_calls & 63) == 1 {
                                k.check_cancelled(OP)?;
                            }
                            // (the evaluator writes every component, zero where
                            // the trim's du/dt is)
                            eval.evaluate(k, t, &mut scratch)?;
                            for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                                *d = *v;
                            }
                            Ok(())
                        },
                    )
                    // Success with no steps means the integrator skipped the span.
                    .and_then(|counts| {
                        if counts.good + counts.reduced == 0 {
                            Err(SolidError::sm_err(OP_SKIPPED_SPAN))
                        } else {
                            Ok(())
                        }
                    });
                if status.is_ok() {
                    break;
                }
                if status
                    .as_ref()
                    .is_err_and(SolidError::is_unported_or_cancelled)
                {
                    break;
                }
            }
            // Propagate failure instead of dropping the boundary: a silent skip would
            // return success with zero/partial properties.
            status?;
            for (total, part) in y.iter_mut().zip(span_y.iter()) {
                *total += *part;
            }
        }
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

fn face_box(k: &Kernel, b: &Brep, f: FaceId) -> Result<Extent3d> {
    crate::query::face::p1::calculate_bounding_box(k, b, f, false)
}
fn faces(b: &Brep) -> Result<Vec<FaceId>> {
    crate::topology::brep::p1::faces(b, None)
}
fn edges(b: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(b, None)
}
fn vertices(b: &Brep) -> Result<Vec<VertexId>> {
    crate::topology::brep::p1::vertices(b, None)
}
/// The region on the side of face use `fu` (`SmFaceuse::GetRegion`); a use with no shell or
/// no region is upstream's null pointer.
fn faceuse_region(b: &Brep, fu: FaceuseId) -> Result<RegionId> {
    crate::topology::edgeuse::p1::faceuse_region(b, fu)?.ok_or_else(|| SolidError::null_pointer(OP))
}
/// `SmRegion::IsVoid` as a C truth value (`UNSURE`, 2, counts as void).
fn region_void(b: &Brep, r: RegionId) -> Result<bool> {
    Ok(!upstream_region_is_material(b, r)?)
}

/// Upstream's reading of whether region `r` is material, kept for parity on the API's
/// paths (`brep_properties`, `SmApiBrepComputeMassProperties`, `SmApiBrepComputeVolume`):
/// `IsVoid` is FALSE, so its `UNSURE` reads as void, as upstream's `TRUE` test of `IsVoid`
/// reads it in every mass property. Every other caller uses [`region_material`], which
/// refuses `UNSURE`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub(crate) fn upstream_region_is_material(b: &Brep, r: RegionId) -> Result<bool> {
    Ok(b.region(r)?.is_void() == 0)
}

/// Whether region `r` is material, refusing where the kernel is unsure: `IsVoid` FALSE is
/// material, TRUE is void, and `UNSURE` (or any other value) is
/// [`SolidError::UnsureRegion`] naming `r`, never read as either. Not upstream: the rule
/// of every measure and point class that must not decide on a region it cannot classify
/// ([`brep_mass_properties_estimated`]).
///
/// # Errors
///
/// [`SolidError::UnsureRegion`]; [`SolidError::NotFound`] for a dangling id.
pub fn region_material(b: &Brep, r: RegionId) -> Result<bool> {
    match b.region(r)?.is_void() {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(SolidError::UnsureRegion {
            operation: OP_REGION,
            region: r,
        }),
    }
}

const OP_REGION: &str = "mass::integrator::properties::region_material";

/// Refuses a body with a region the kernel is unsure of ([`region_material`]): the guard
/// of every measure that must not read `UNSURE` as void, in front of the API's
/// computations (the estimated mass properties; ops' volume and orientation test).
///
/// # Errors
///
/// [`SolidError::UnsureRegion`] naming the first such region; [`SolidError::NotFound`].
pub fn no_unsure_region(b: &Brep) -> Result<()> {
    for r in crate::topology::brep::p1::regions(b, None)? {
        region_material(b, r)?;
    }
    Ok(())
}

/// Whether face `f` is a partition: its two sides are two distinct material regions
/// ([`region_material`]).
///
/// # Errors
///
/// [`SolidError::UnsureRegion`] for a side the kernel is unsure of;
/// [`SolidError::NotFound`] for a dangling id.
pub fn partition_face(b: &Brep, f: FaceId) -> Result<bool> {
    Ok(match crate::topology::face::p2::face_side_regions(b, f)? {
        [Some(a), Some(c)] => a != c && region_material(b, a)? && region_material(b, c)?,
        _ => false,
    })
}
fn manifold_solid(b: &Brep) -> Result<bool> {
    crate::topology::brep::p2::is_manifold_solid(b)
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
/// [`add`] of a volume-separating faceuse with WR-535: its volume, mass and volume
/// moments (`4..`) only with `volume`, its area and area moments (`..4`) only with
/// `area`.
fn add_selected(
    total: &mut PreciseProperties,
    delta: &PreciseProperties,
    density: f64,
    volume: bool,
    area: bool,
) {
    if volume {
        total.volume += delta.volume;
        total.mass += delta.volume * density;
        for (t, d) in total.moments.iter_mut().zip(delta.moments.iter()).skip(4) {
            *t += *d * density;
        }
    }
    if area {
        total.area += delta.area;
        for (t, d) in total.moments.iter_mut().zip(delta.moments.iter()).take(4) {
            *t += *d;
        }
    }
}
/// `SmBrep::ComputePreciseProperties`, including density attributes and wire/point solids.
/// Missing UV trim curves of the integrated faces are made on `b`.
pub fn brep_properties(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
) -> Result<PreciseProperties> {
    let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
    let size = bbox.size();
    let estimated = size.z.max(size.x.max(size.y));
    let estimated = estimated * estimated / 100.0;
    let mut total = PreciseProperties::default();
    for f in faces(b)? {
        k.check_cancelled(OP)?;
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
        let mut separating = faceuse_region(b, up)? != faceuse_region(b, mate)?;
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
                    if !region_void(b, faceuse_region(b, fu)?)?
                        && region_void(b, faceuse_region(b, other)?)?
                    {
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
            if behavior.face_volume == 0 && !region_void(b, faceuse_region(b, fu)?)? {
                do_volume = true;
            }
            if !(do_area || do_volume) {
                continue;
            }
            if separating || mass.y > EFF_ZERO {
                let orientation = b.faceuse(fu)?.orientation;
                let delta = face_properties(
                    k,
                    b,
                    f,
                    orientation,
                    accuracy,
                    origin,
                    estimated,
                    if separating { 0.0 } else { mass.y },
                    PPF_ALL,
                )?;
                if separating && k.fixes.brep_area_between_solid_and_void {
                    // Not upstream (WR-535, the fork's tangency-labs/smlib@d2d9586): the
                    // faceuse's volume and mass enter only when it bounds a non void region,
                    // its area and area moments only when the area behaviour selects it: a
                    // face between two solid regions bounds both volumes but is no part of
                    // the area
                    add_selected(&mut total, &delta, mass.x, do_volume, do_area);
                } else {
                    add(&mut total, &delta, mass.x);
                }
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
        let delta = edge_properties(k, b, e, accuracy, origin, mass.y)?;
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
/// `SmApiEdgeComputeLength` (the curve's virtual Length). The edge's curve is owned by
/// the edge, so its zone tolerance (`SmTol::GetZoneTol3d(pCurve)`) is the edge's.
pub fn edge_length(k: &Kernel, b: &Brep, e: EdgeId, accuracy: f64) -> Result<f64> {
    let c = edge::curve(b, e)?.ok_or_else(|| SolidError::sm_err(OP))?;
    let owner_zone_tol_3d = if c.owner().is_some() {
        Some(edge::tolerance(b, e)?)
    } else {
        None
    };
    c.length_with_zone_tol(k, &edge::interval(b, e)?, accuracy, owner_zone_tol_3d)
}
fn area_estimate(bbox: &Extent3d) -> f64 {
    let s = bbox.size();
    let d = s.x.max(s.y).max(s.z);
    d * d / 100.0
}
/// `SmApiFaceComputeArea`: rejects a non-finite accuracy and integrates only area.
pub fn face_area(k: &Kernel, b: &mut Brep, f: FaceId, accuracy: f64) -> Result<f64> {
    valid_api_number(accuracy)?;
    let bbox = face_box(k, b, f)?;
    Ok(face_properties(
        k,
        b,
        f,
        OrientType::Same,
        accuracy,
        bbox.mid(),
        area_estimate(&bbox),
        0.0,
        PPF_AREA,
    )?
    .area
    .abs())
}
/// `SmApiFaceComputeCentroid`: rejects a non-finite accuracy; one area/first-moment
/// integration.
pub fn face_centroid(k: &Kernel, b: &mut Brep, f: FaceId, accuracy: f64) -> Result<(f64, Point3)> {
    valid_api_number(accuracy)?;
    let bbox = face_box(k, b, f)?;
    let origin = bbox.mid();
    let p = face_properties(
        k,
        b,
        f,
        OrientType::Same,
        accuracy,
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
/// `SmApiBrepComputeArea`, including open shells and sheets: [`brep_area_about`] each
/// face's own box middle.
pub fn brep_area(k: &Kernel, b: &mut Brep, accuracy: f64) -> Result<f64> {
    area_about(k, b, accuracy, AreaAbout::FaceMiddle)
}

/// The point each face's area integral is taken about: it changes no exact area, only
/// the integrator's error scale (and so its estimate's rounding).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AreaAbout {
    /// Each face's own bounding-box middle (`SmApiFaceComputeArea`).
    FaceMiddle,
    /// One point for every face.
    Point(Point3),
}

/// Every face's area counted once, open shells and sheets included, each face integrated
/// on its own about `about` (one implementation with [`brep_area`]), at `accuracy`
/// clamped by [`estimate_accuracy`]. **Requested accuracy, estimated, not proved**
/// ([`EstimatedMassProperties`]).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an accuracy that is not finite and positive, or a
/// point that is not finite; those of [`face_properties`].
pub fn brep_area_about(k: &Kernel, b: &mut Brep, accuracy: f64, about: AreaAbout) -> Result<f64> {
    area_about(k, b, estimate_accuracy(accuracy)?, about)
}

/// The accuracy an estimated property is asked at: finite and positive, clamped to
/// [[`ESTIMATE_ACCURACY_FINEST`], [`ESTIMATE_ACCURACY_COARSEST`]].
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an accuracy that is not finite, or not positive.
pub fn estimate_accuracy(accuracy: f64) -> Result<f64> {
    if !accuracy.is_finite() || accuracy.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(
            OP,
            "accuracy not finite and positive",
        ));
    }
    // `f64::clamp` is on the panic-site list (a reversed range panics); the bounds are
    // constants in order.
    #[allow(clippy::manual_clamp)]
    Ok(accuracy
        .max(ESTIMATE_ACCURACY_FINEST)
        .min(ESTIMATE_ACCURACY_COARSEST))
}

/// The finest accuracy an estimated property is asked at: finer, the integrator's step
/// control works below the rounding of its sums.
pub const ESTIMATE_ACCURACY_FINEST: f64 = 1e-8;
/// The coarsest accuracy an estimated property is asked at.
pub const ESTIMATE_ACCURACY_COARSEST: f64 = 1e-1;

/// [`brep_area_about`] at an accuracy as given (`SmApiBrepComputeArea`'s rules).
fn area_about(k: &Kernel, b: &mut Brep, accuracy: f64, about: AreaAbout) -> Result<f64> {
    valid_api_number(accuracy)?;
    if let AreaAbout::Point(p) = about {
        for v in [p.x, p.y, p.z] {
            valid_api_number(v)?;
        }
    }
    Ok(faces_area_about(k, b, accuracy, about, false, false)?.0)
}
/// `SmApiBrepComputeVolume`, computed about the bounding-box midpoint.
///
/// Use [`brep_volume_default`] when per-entity attributes are not needed.
pub fn brep_volume(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    attributes: &PropertyAttributes<'_>,
) -> Result<f64> {
    valid_api_number(accuracy)?;
    let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
    Ok(brep_properties(
        k,
        b,
        accuracy,
        bbox.mid(),
        MassPropertyBehavior::default(),
        attributes,
    )?
    .volume)
}

/// [`brep_volume`] without per-entity mass attributes. Attributes on `b` are
/// neither read nor modified; UV trim construction has the same effects as
/// [`brep_volume`].
///
/// # Errors
/// Those of [`brep_volume`].
pub fn brep_volume_default(k: &Kernel, b: &mut Brep, accuracy: f64) -> Result<f64> {
    let store = AttributeStore::default();
    brep_volume(
        k,
        b,
        accuracy,
        &PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        },
    )
}

/// [`brep_mass_properties`] without per-entity mass attributes. Uses `density`
/// uniformly, ignoring attributes attached to `b` and leaving them intact.
/// Missing UV trims may be created as in [`brep_mass_properties`].
///
/// # Errors
/// Those of [`brep_mass_properties`].
pub fn brep_mass_properties_default(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    density: f64,
    origin: Point3,
) -> Result<MassProperties> {
    let store = AttributeStore::default();
    brep_mass_properties(
        k,
        b,
        accuracy,
        density,
        origin,
        &PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        },
    )
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
/// `SmApiBrepComputeMassProperties`, with upstream validation: integrated once about the
/// bounding-box midpoint, the moments then shifted to `origin` (which avoids
/// cancellation for a distant origin).
///
/// The accuracy is the integrator's requested accuracy: its error ESTIMATE is held under
/// it, which is not a proved bound (see [`EstimatedMassProperties`]).
///
/// [`brep_mass_properties_estimated`]'s computation with [`MassPropertyRequest::api`],
/// with upstream's statuses (an integration that does not converge is `SM_ERR`).
pub fn brep_mass_properties(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    density: f64,
    origin: Point3,
    attributes: &PropertyAttributes<'_>,
) -> Result<MassProperties> {
    let r = estimate(
        k,
        b,
        accuracy,
        &MassPropertyRequest::api(density, origin),
        attributes,
    )?;
    Ok(MassProperties {
        properties: r.properties,
        centroid: r.centroid,
        inertia: r.inertia,
        products: r.products,
    })
}

/// Where [`brep_mass_properties_estimated`] takes the second moments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MomentsAbout {
    /// About this point, by the parallel-axis shift from the box's middle through the
    /// static moment (`SmApiBrepComputeMassProperties`).
    Point(Point3),
    /// About the centroid: the moments about the box's middle less the mass times the
    /// squared offsets of the centroid.
    Centroid,
}

/// What [`brep_mass_properties_estimated`] measures, and what it refuses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassPropertyRequest {
    /// The density of every material region (per-entity mass attributes are refused).
    pub density: f64,
    /// Where the second moments are taken.
    pub moments_about: MomentsAbout,
    /// Refuse a body that is not a manifold solid (`SmApiBrepComputeMassProperties`);
    /// otherwise the sum over the body's material regions, so several lumps, or lumps
    /// touching along edges or at points, measure too.
    pub manifold_solid_only: bool,
    /// Add the area of the [`sheet_face`]s (void on both sides, or one material region on
    /// both sides) to the solid boundary's: the area then counts every face once but the
    /// partitions between two material regions, which are no part of it. That needs
    /// `brep_area_between_solid_and_void` (WR-535), without which the boundary counts a
    /// partition once from each side: a body with a partition is then refused.
    pub sheet_faces_area: bool,
    /// For a body with no material region: its faces' area, volume and mass 0, the
    /// centroid of the area, no moments (otherwise refused as no mass).
    pub area_centroid_without_material: bool,
}

impl MassPropertyRequest {
    /// `SmApiBrepComputeMassProperties`' request: `density`, moments about `origin`,
    /// manifold solids only, no sheets, no material refused.
    #[must_use]
    pub const fn api(density: f64, origin: Point3) -> Self {
        Self {
            density,
            moments_about: MomentsAbout::Point(origin),
            manifold_solid_only: true,
            sheet_faces_area: false,
            area_centroid_without_material: false,
        }
    }

    /// Every body at unit density: moments about the centroid, the sum over material
    /// regions, sheet faces' area counted, and a body with no material measured by its
    /// area and the area's centroid.
    #[must_use]
    pub const fn centroidal() -> Self {
        Self {
            density: 1.0,
            moments_about: MomentsAbout::Centroid,
            manifold_solid_only: false,
            sheet_faces_area: true,
            area_centroid_without_material: true,
        }
    }
}

/// [`brep_mass_properties_estimated`]'s result.
///
/// **Requested accuracy, estimated, not proved.** Every number comes from an adaptive
/// Runge-Kutta integration whose per-step error estimate is held under
/// `requested_accuracy` times a per-property scale. That estimate is not a bound: the
/// error is not proved to be below the requested accuracy, and no caller may treat it
/// as one (decide a tolerance on these numbers only with that in mind). A certified
/// quadrature is a separate, later entry.
#[derive(Debug, Clone, PartialEq)]
pub struct EstimatedMassProperties {
    /// The accuracy asked of the integrator's error estimate (not a proved bound).
    pub requested_accuracy: f64,
    /// The B-rep's precise measures about the bounding box's middle (all zero but the
    /// area for a body with no material).
    pub properties: PreciseProperties,
    /// Surface area (with the sheet faces' when asked).
    pub area: f64,
    /// Volume of the material regions.
    pub volume: f64,
    /// Density-weighted volume.
    pub mass: f64,
    /// Centre of mass in world coordinates; of the area for a body with no material.
    pub centroid: Point3,
    /// Mass axial inertia (y^2+z^2, z^2+x^2, x^2+y^2) about [`MassPropertyRequest::moments_about`].
    pub inertia: Vec3,
    /// Raw positive products (yz, zx, xy) about the same point.
    pub products: Vec3,
}

impl EstimatedMassProperties {
    /// The inertia tensor about the point the moments are taken about (row-major): the axial
    /// inertias on the diagonal, the negated products off it.
    #[must_use]
    pub fn inertia_tensor(&self) -> [[f64; 3]; 3] {
        let (i, p) = (self.inertia, self.products);
        [[i.x, -p.z, -p.y], [-p.z, i.y, -p.x], [-p.y, -p.x, i.z]]
    }
}

/// Mass properties as `request` asks (one implementation:
/// [`brep_mass_properties`] is its `SmApiBrepComputeMassProperties` request). Integrated
/// once about the bounding box's middle: volume and moments over the material regions
/// (`SmBrep::ComputePreciseProperties`), the area of the faces between a solid and a
/// void, then the moments moved to the point asked, or to the centroid. **The accuracy is
/// requested and estimated, not proved** ([`EstimatedMassProperties`]).
///
/// `accuracy` must be finite and positive; it is clamped to
/// [[`ESTIMATE_ACCURACY_FINEST`], [`ESTIMATE_ACCURACY_COARSEST`]] ([`estimate_accuracy`]),
/// and the result's `requested_accuracy` is the clamped value.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an accuracy that is not finite and positive, an
/// invalid number (density, point), a density below epsilon, per-entity mass attributes, a body that is not a manifold
/// solid when `manifold_solid_only`, no mass or volume (or, for a body with no material
/// measured by its area, no area, or a non-finite area or centroid);
/// a non-finite result (the centroidal moments too); [`SolidError::NotConverged`] where the
/// integrator's step control fails (it cannot meet the requested accuracy's estimate:
/// #2545, WR-472's error scale off; the step count is not reported, `iterations` is 0);
/// [`SolidError::Degenerate`] for a non-finite integrand; [`SolidError::Unsupported`]
/// for a span the integrator took no step over, or for `sheet_faces_area` on a body with a
/// partition without `brep_area_between_solid_and_void` (WR-535: without it each side of a
/// partition adds its area, so the area would count it twice); [`SolidError::UnsureRegion`]
/// for a body with a region the kernel is unsure of (its volume and its faces' area depend
/// on whether it is material: refused, not read as void); those of the integration.
pub fn brep_mass_properties_estimated(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    request: &MassPropertyRequest,
    attributes: &PropertyAttributes<'_>,
) -> Result<EstimatedMassProperties> {
    let accuracy = estimate_accuracy(accuracy)?;
    no_unsure_region(b)?;
    if request.sheet_faces_area && !k.fixes.brep_area_between_solid_and_void {
        for f in faces(b)? {
            if partition_face(b, f)? {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a partition's area is counted from both sides without \
                             brep_area_between_solid_and_void",
                });
            }
        }
    }
    estimate(k, b, accuracy, request, attributes).map_err(typed_failure)
}

/// The integration's failures as their own errors (upstream's `SM_ERR` for each): its step
/// control failing is [`SolidError::NotConverged`]; a non-finite integrand (a surface
/// evaluated to no number) is [`SolidError::Degenerate`]; a span it took no step over is
/// [`SolidError::Unsupported`]. Every other error is as it came.
fn typed_failure(e: SolidError) -> SolidError {
    use super::{OP_INTEGRAND, OP_STEP_CONTROL};
    match e {
        SolidError::Upstream {
            operation: OP_STEP_CONTROL,
            code: crate::error::SM_ERR,
            ..
        } => SolidError::NotConverged {
            operation: OP,
            iterations: 0,
        },
        SolidError::Upstream {
            operation: OP_INTEGRAND,
            code: crate::error::SM_ERR,
            entity,
        } => SolidError::Degenerate {
            operation: OP_INTEGRAND,
            entity,
        },
        SolidError::Upstream {
            operation: OP_SKIPPED_SPAN,
            code: crate::error::SM_ERR,
            ..
        } => SolidError::Unsupported {
            operation: OP,
            reason: "the integrator took no step over a span of a face",
        },
        e => e,
    }
}

/// The one computation of [`brep_mass_properties`] and
/// [`brep_mass_properties_estimated`], with upstream's statuses.
fn estimate(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    request: &MassPropertyRequest,
    attributes: &PropertyAttributes<'_>,
) -> Result<EstimatedMassProperties> {
    let density = request.density;
    valid_api_number(accuracy)?;
    valid_api_number(density)?;
    if density < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "density below epsilon"));
    }
    if let MomentsAbout::Point(origin) = request.moments_about {
        for v in [origin.x, origin.y, origin.z] {
            valid_api_number(v)?;
        }
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
    if request.manifold_solid_only && !manifold_solid(b)? {
        return Err(SolidError::invalid_input(OP, "not a manifold solid"));
    }
    let mid = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?.mid();
    if request.area_centroid_without_material && !has_material(b)? {
        return area_centroid(k, b, accuracy, mid);
    }
    let properties = brep_properties(
        k,
        b,
        accuracy,
        mid,
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
    // [4] = static moment S about the midpoint; [7] = moments of inertia
    // (y^2+z^2, ...); [6] = products (yz, zx, xy) as raw positive integrals.
    let moment = |i: usize| {
        properties
            .moments
            .get(i)
            .copied()
            .ok_or_else(|| SolidError::sm_err(OP))
    };
    let static_moment = moment(4)?;
    let mass = properties.mass;
    let (inertia, products) = match request.moments_about {
        MomentsAbout::Point(origin) => {
            // Shift to origin: with d = mid - origin, the integral of (a+d_a)(b+d_b) dm
            // gains d_a S_b + d_b S_a + d_a d_b m.
            let d = mid - origin;
            let comp = |v: Vec3, i: usize| match i {
                0 => v.x,
                1 => v.y,
                _ => v.z,
            };
            let shift = |a: usize, b: usize| {
                comp(d, a) * comp(static_moment, b)
                    + comp(d, b) * comp(static_moment, a)
                    + comp(d, a) * comp(d, b) * mass
            };
            (
                moment(7)?
                    + Vec3::new(
                        shift(1, 1) + shift(2, 2),
                        shift(0, 0) + shift(2, 2),
                        shift(0, 0) + shift(1, 1),
                    ),
                moment(6)? + Vec3::new(shift(1, 2), shift(2, 0), shift(0, 1)),
            )
        }
        MomentsAbout::Centroid => {
            // the centroid's offset d = S / m from the midpoint; about the centroid the
            // integral of a b dm loses m d_a d_b
            let d = static_moment / mass;
            let (m6, m7) = (moment(6)?, moment(7)?);
            (
                Vec3::new(
                    m7.x - mass * (d.y * d.y + d.z * d.z),
                    m7.y - mass * (d.x * d.x + d.z * d.z),
                    m7.z - mass * (d.x * d.x + d.y * d.y),
                ),
                Vec3::new(
                    m6.x - mass * d.y * d.z,
                    m6.y - mass * d.z * d.x,
                    m6.z - mass * d.x * d.y,
                ),
            )
        }
    };
    let centroid = mid + static_moment / mass;
    let area = if request.sheet_faces_area {
        properties.area + faces_area_about(k, b, accuracy, AreaAbout::Point(mid), false, true)?.0
    } else {
        properties.area
    };
    let r = EstimatedMassProperties {
        requested_accuracy: accuracy,
        area,
        volume: properties.volume,
        mass,
        properties,
        centroid,
        inertia,
        products,
    };
    // the API's request keeps `SmApiBrepComputeMassProperties`' contract (no finiteness
    // test); the centroidal one refuses a number that is not finite
    if request.moments_about == MomentsAbout::Centroid {
        finite_result(&r)?;
    }
    Ok(r)
}

/// A body with no material: its faces' area and the area's centroid.
fn area_centroid(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    mid: Point3,
) -> Result<EstimatedMassProperties> {
    let (area, first) = faces_area_about(k, b, accuracy, AreaAbout::Point(mid), true, false)?;
    if area.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Err(SolidError::invalid_input(OP, "no area"));
    }
    let r = EstimatedMassProperties {
        requested_accuracy: accuracy,
        properties: PreciseProperties {
            area,
            ..PreciseProperties::default()
        },
        area,
        volume: 0.0,
        mass: 0.0,
        centroid: Point3::new(
            mid.x + first.x / area,
            mid.y + first.y / area,
            mid.z + first.z / area,
        ),
        inertia: Vec3::ZERO,
        products: Vec3::ZERO,
    };
    finite_result(&r)?;
    Ok(r)
}

fn finite_result(r: &EstimatedMassProperties) -> Result<()> {
    let v = |x: Vec3| x.x.is_finite() && x.y.is_finite() && x.z.is_finite();
    if r.area.is_finite()
        && r.volume.is_finite()
        && r.mass.is_finite()
        && v(r.centroid)
        && v(r.inertia)
        && v(r.products)
    {
        Ok(())
    } else {
        Err(SolidError::invalid_input(OP, "non-finite result"))
    }
}

/// Whether a region of `b` is material ([`region_material`]; reached only behind
/// [`no_unsure_region`], on the centroidal request).
fn has_material(b: &Brep) -> Result<bool> {
    for r in crate::topology::brep::p1::regions(b, None)? {
        if region_material(b, r)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether face `f` is outside the solid boundary's area: the boundary counts a face with
/// material on one side and void on the other, so these are the faces whose two sides are
/// both void (a sheet, standing free or hung on a solid) or are one material region (a
/// fin inside the material, the same region on both sides). A face between two distinct
/// material regions (a partition) is in neither: it bounds no part of the area
/// (`brep_area_between_solid_and_void`, WR-535). A side with no faceuse, no shell or no
/// region is void; a region the kernel is unsure of is refused ([`region_material`]).
///
/// # Errors
///
/// [`SolidError::UnsureRegion`]; [`SolidError::NotFound`] for a dangling id.
pub fn sheet_face(b: &Brep, f: FaceId) -> Result<bool> {
    // the side's region when it is material, `None` when void
    let material = |r: Option<RegionId>| -> Result<Option<RegionId>> {
        match r {
            Some(r) => Ok(region_material(b, r)?.then_some(r)),
            None => Ok(None),
        }
    };
    let [x, y] = crate::topology::face::p2::face_side_regions(b, f)?;
    Ok(match (material(x)?, material(y)?) {
        (None, None) => true,
        (Some(a), Some(c)) => a == c,
        _ => false,
    })
}

/// Every face's area counted once (or only the [`sheet_face`]s), each integrated on its
/// own about `about` with an area estimate from its loose box, and with `first` the
/// area's first moments about that point (meant with one point for every face). A trim
/// boundary integrated the other way round flips both.
fn faces_area_about(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    about: AreaAbout,
    first: bool,
    only_sheets: bool,
) -> Result<(f64, Vec3)> {
    let flags = if first {
        PPF_AREA | PPF_AREA_FIRST_MOMENTS
    } else {
        PPF_AREA
    };
    let mut area = 0.0;
    let mut sum = Vec3::ZERO;
    for f in faces(b)? {
        if only_sheets && !sheet_face(b, f)? {
            continue;
        }
        let fbox = face_box(k, b, f)?;
        let origin = match about {
            AreaAbout::FaceMiddle => fbox.mid(),
            AreaAbout::Point(p) => p,
        };
        let p = face_properties(
            k,
            b,
            f,
            OrientType::Same,
            accuracy,
            origin,
            area_estimate(&fbox),
            0.0,
            flags,
        )?;
        // |area|, as `SmApiFaceComputeArea` takes it; the first moments flip with it
        area += p.area.abs();
        if first {
            let sign = if p.area < 0.0 { -1.0 } else { 1.0 };
            sum += *p.moments.first().ok_or_else(|| SolidError::sm_err(OP))? * sign;
        }
    }
    Ok((area, sum))
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
    #[test]
    fn face_area_makes_missing_uv_trims_on_the_callers_brep() {
        // Upstream's `SmFace::ComputePreciseProperties` builds its oriented trim boundary on
        // the face itself, so the UV trims it makes stay on the B-rep.
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        let uses: Vec<_> = b.edgeuses().ids().collect();
        for u in uses {
            b.edgeuse_mut(u).unwrap().uv_trim_curve = None;
        }
        let f = *faces(&b).unwrap().first().unwrap();
        let area = face_area(&k, &mut b, f, 1e-6).unwrap();
        assert!((area - 1.0).abs() <= 1e-9, "{area}");
        assert!(
            b.edgeuses().iter().any(|(_, u)| u.uv_trim_curve.is_some()),
            "the trims are made on the box itself"
        );
    }
    fn no_attributes() -> AttributeStore {
        AttributeStore::default()
    }

    fn estimated(
        k: &Kernel,
        b: &mut Brep,
        request: &MassPropertyRequest,
    ) -> Result<EstimatedMassProperties> {
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        brep_mass_properties_estimated(k, b, 1e-8, request, &attrs)
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    /// A 1 x 2 x 3 box: volume 6, area 22, centroid its middle, centroidal inertia
    /// m (b^2 + c^2) / 12 and its turns, no products.
    #[test]
    fn centroidal_mass_properties_of_a_box() {
        let mut k = Kernel::default();
        let o = Point3::new(10.0, -20.0, 30.0);
        let mut b = crate::primitive::api_primitives::create_box(&mut k, o, 1.0, 2.0, 3.0).unwrap();
        let r = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        assert_eq!(r.requested_accuracy, 1e-8);
        assert!(
            near(r.volume, 6.0, 1e-9) && near(r.mass, 6.0, 1e-9),
            "{r:?}"
        );
        assert!(near(r.area, 22.0, 1e-9), "{r:?}");
        let c = o + Vec3::new(0.5, 1.0, 1.5);
        assert!((r.centroid - c).length() <= 1e-9, "{r:?}");
        let (ixx, iyy, izz) = (6.0 * 13.0 / 12.0, 6.0 * 10.0 / 12.0, 6.0 * 5.0 / 12.0);
        assert!(near(r.inertia.x, ixx, 1e-8), "{r:?}");
        assert!(near(r.inertia.y, iyy, 1e-8), "{r:?}");
        assert!(near(r.inertia.z, izz, 1e-8), "{r:?}");
        for p in [r.products.x, r.products.y, r.products.z] {
            assert!(p.abs() <= 1e-8, "{r:?}");
        }
        // the API request about the centroid agrees with the centroidal moments
        let api = estimated(&k, &mut b, &MassPropertyRequest::api(1.0, r.centroid)).unwrap();
        assert!((api.inertia - r.inertia).length() <= 1e-8, "{api:?}");
        assert!((api.products - r.products).length() <= 1e-8, "{api:?}");
    }

    /// A sphere of radius 2: volume 4/3 pi r^3, area 4 pi r^2, inertia 2/5 m r^2.
    #[test]
    fn centroidal_mass_properties_of_a_sphere() {
        let mut k = Kernel::default();
        k.fixes.precise_properties_body_length_scale = true;
        let c = Point3::new(1.0, 2.0, -3.0);
        let mut b = crate::primitive::api_primitives::create_sphere(&mut k, c, 2.0).unwrap();
        let r = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        let v = 4.0 / 3.0 * PI * 8.0;
        assert!(near(r.volume, v, 1e-6), "{r:?}");
        assert!(near(r.area, 16.0 * PI, 1e-6), "{r:?}");
        assert!((r.centroid - c).length() <= 1e-6, "{r:?}");
        let i = 0.4 * v * 4.0;
        for m in [r.inertia.x, r.inertia.y, r.inertia.z] {
            assert!(near(m, i, 1e-6), "{r:?}");
        }
    }

    /// #2545: a whole sphere at 1e-8 with WR-472's error scale off did not converge (its
    /// steps crossed the NURB sphere's double knots). Integrated between the surface's knots
    /// (WR-494, #1435) it converges, both entries, and holds the closed forms: area and
    /// volume 4 pi r^2 = 4/3 pi r^3 (r 3), inertia 2/5 m r^2. The typing of a failure that
    /// does not converge is `integration_failures_are_typed_by_cause`'s.
    #[test]
    fn a_sphere_without_the_length_scale_converges_between_knots() {
        let mut k = Kernel::default();
        k.fixes.precise_properties_body_length_scale = false;
        let mut b =
            crate::primitive::api_primitives::create_sphere(&mut k, Point3::ZERO, 3.0).unwrap();
        let exact = 36.0 * PI;
        let inertia = 0.4 * exact * 9.0;
        let typed = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        assert!(near(typed.area, exact, 1e-9), "{typed:?}");
        assert!(near(typed.volume, exact, 1e-9), "{typed:?}");
        for m in [typed.inertia.x, typed.inertia.y, typed.inertia.z] {
            assert!(near(m, inertia, 1e-9), "{typed:?}");
        }
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let api = brep_mass_properties(&k, &mut b, 1e-8, 1.0, Point3::ZERO, &attrs).unwrap();
        assert!(near(api.properties.area, exact, 1e-9), "{api:?}");
        assert!(near(api.properties.volume, exact, 1e-9), "{api:?}");
    }

    /// The estimated entry types each integration failure by its cause, and only step
    /// control as not converging.
    #[test]
    fn integration_failures_are_typed_by_cause() {
        use super::super::{OP_INTEGRAND, OP_STEP_CONTROL};
        assert!(matches!(
            typed_failure(SolidError::sm_err(OP_STEP_CONTROL)),
            SolidError::NotConverged { .. }
        ));
        assert!(matches!(
            typed_failure(SolidError::sm_err(OP_INTEGRAND)),
            SolidError::Degenerate { .. }
        ));
        assert!(matches!(
            typed_failure(SolidError::sm_err(OP_SKIPPED_SPAN)),
            SolidError::Unsupported { .. }
        ));
        // any other status stays as it came
        let other = SolidError::sm_err(OP);
        assert_eq!(typed_failure(other.clone()), other);
    }

    /// A box whose region is made void has no material: its area is its faces' (22) and
    /// its centroid the area's (the middle, by symmetry); the API request refuses it.
    #[test]
    fn a_body_with_no_material_measures_its_area_and_its_centroid() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        let regions = crate::topology::brep::p1::regions(&b, None).unwrap();
        for r in regions {
            b.region_mut(r).unwrap().set_is_void(1);
        }
        assert!(!has_material(&b).unwrap());
        let r = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        assert!(near(r.area, 22.0, 1e-9), "{r:?}");
        assert_eq!((r.volume, r.mass), (0.0, 0.0));
        assert!(
            (r.centroid - Point3::new(0.5, 1.0, 1.5)).length() <= 1e-9,
            "{r:?}"
        );
        assert_eq!((r.inertia, r.products), (Vec3::ZERO, Vec3::ZERO));
        // every face is a sheet face now
        for f in faces(&b).unwrap() {
            assert!(sheet_face(&b, f).unwrap());
        }
        let refuse = MassPropertyRequest {
            area_centroid_without_material: false,
            ..MassPropertyRequest::centroidal()
        };
        assert!(estimated(&k, &mut b, &refuse).is_err());
        assert!(estimated(&k, &mut b, &MassPropertyRequest::api(1.0, Point3::ZERO)).is_err());
    }

    /// A torus's area 4 pi^2 R r about each face's middle (the API's) and about one point:
    /// the same area to the estimate's rounding, the API's bit for bit.
    #[test]
    fn brep_area_about_a_point_or_each_face() {
        let mut k = Kernel::default();
        k.fixes.precise_properties_body_length_scale = true;
        let mut b = crate::primitive::api_primitives::create_torus(
            &mut k,
            Point3::new(3.0, 0.0, -1.0),
            4.0,
            1.0,
        )
        .unwrap();
        let exact = 4.0 * PI * PI * 4.0;
        let api = brep_area(&k, &mut b, 1e-8).unwrap();
        let mid = brep_area_about(&k, &mut b, 1e-8, AreaAbout::FaceMiddle).unwrap();
        assert_eq!(api.to_bits(), mid.to_bits());
        // `SmApiBrepComputeArea` as upstream sums it, `SmApiFaceComputeArea` face by face:
        // the one implementation gives its numbers bit for bit
        let mut upstream = 0.0;
        for f in faces(&b).unwrap() {
            upstream += face_area(&k, &mut b, f, 1e-8).unwrap();
        }
        assert_eq!(api.to_bits(), upstream.to_bits());
        let at = brep_area_about(&k, &mut b, 1e-8, AreaAbout::Point(Point3::ZERO)).unwrap();
        assert!(
            near(api, exact, 1e-7) && near(at, exact, 1e-7),
            "{api} {at} {exact}"
        );
        let bad = AreaAbout::Point(Point3::new(f64::NAN, 0.0, 0.0));
        assert!(brep_area_about(&k, &mut b, 1e-8, bad).is_err());
    }

    /// The estimated entries own their accuracy: not finite or not positive refused, the rest
    /// clamped to [1e-8, 1e-1] and reported as clamped.
    #[test]
    fn an_estimate_refuses_or_clamps_its_accuracy() {
        for bad in [0.0, -1e-6, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(estimate_accuracy(bad).is_err(), "{bad}");
        }
        assert_eq!(estimate_accuracy(1e-15).unwrap(), 1e-8);
        assert_eq!(estimate_accuracy(0.5).unwrap(), 1e-1);
        assert_eq!(estimate_accuracy(1e-5).unwrap(), 1e-5);
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let request = MassPropertyRequest::centroidal();
        for bad in [0.0, -1.0, f64::NAN] {
            assert!(brep_mass_properties_estimated(&k, &mut b, bad, &request, &attrs).is_err());
            assert!(brep_area_about(&k, &mut b, bad, AreaAbout::FaceMiddle).is_err());
        }
        let r = brep_mass_properties_estimated(&k, &mut b, 1e-12, &request, &attrs).unwrap();
        assert_eq!(r.requested_accuracy, 1e-8);
        let r = brep_mass_properties_estimated(&k, &mut b, 2.0, &request, &attrs).unwrap();
        assert_eq!(r.requested_accuracy, 1e-1);
    }

    /// A solid's faces are not sheet faces, so asking for the sheets' area adds none.
    #[test]
    fn a_solid_has_no_sheet_faces() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        for f in faces(&b).unwrap() {
            assert!(!sheet_face(&b, f).unwrap());
        }
        let with = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        let without = MassPropertyRequest {
            sheet_faces_area: false,
            ..MassPropertyRequest::centroidal()
        };
        let without = estimated(&k, &mut b, &without).unwrap();
        assert_eq!(with, without);
    }

    /// A sheet inside a box, merged into it, lies in the box's material region on both
    /// sides (a fin): it is neither a boundary face nor void on both sides, and the area
    /// that counts every face once counts it (6 x 1 x 2 + 2 x 1 x 3 + 2 x 2 x 3 = 22, and
    /// the fin's 0.5 x 0.5).
    #[test]
    fn a_fin_inside_the_material_is_counted_once() {
        use crate::primitive::api_primitives::{create_box, create_plane};
        let mut k = Kernel::default();
        k.fixes.brep_area_between_solid_and_void = true;
        k.fixes.precise_properties_body_length_scale = true;
        let a = create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
        let fin = create_plane(&mut k, Point3::new(0.25, 0.5, 1.5), 0.5, 0.5).unwrap();
        let mut b = crate::boolean::api_brep::boolean_merge(&mut k, a, fin).unwrap();
        let fins: Vec<FaceId> = faces(&b)
            .unwrap()
            .into_iter()
            .filter(|&f| {
                let (x, y) = crate::topology::face::p2::faceuses(&b, f).unwrap();
                let region = |u: Option<FaceuseId>| {
                    let sh = b.faceuse(u.unwrap()).unwrap().shell.unwrap();
                    b.shell(sh).unwrap().region.unwrap()
                };
                region(x) == region(y)
            })
            .collect();
        assert_eq!(fins.len(), 1, "one face with one region on both sides");
        assert!(sheet_face(&b, fins[0]).unwrap());
        let r = estimated(&k, &mut b, &MassPropertyRequest::centroidal()).unwrap();
        assert!(near(r.area, 22.25, 1e-9), "{r:?}");
        assert!(near(r.volume, 6.0, 1e-9), "{r:?}");
    }

    /// WR-535 (the fork's tangency-labs/smlib@d2d9586): the merge of two overlapping 10
    /// mm cubes has three solid regions; its area counts only the faces between solid
    /// and void (800, not 1200 with the two inner faces counted from both sides), and
    /// its volume is the union's.
    #[test]
    fn merged_cubes_area_counts_solid_void_faces_only() {
        use crate::primitive::api_primitives::create_box;
        let mut k = Kernel::default();
        k.fixes.brep_area_between_solid_and_void = true;
        let a = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let b = create_box(&mut k, Point3::new(5.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let mut merged = crate::boolean::api_brep::boolean_merge(&mut k, a, b).unwrap();
        let store = AttributeStore::default();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let p = brep_properties(
            &k,
            &mut merged,
            1e-6,
            Point3::ZERO,
            MassPropertyBehavior::default(),
            &attrs,
        )
        .unwrap();
        assert!((p.area - 800.0).abs() <= 1e-6, "area {}", p.area);
        assert!((p.volume - 1500.0).abs() <= 1e-6, "volume {}", p.volume);
    }

    /// A region the kernel is unsure of is refused by name, never read as void (which
    /// would give a box volume 0) or as material.
    #[test]
    fn an_unsure_region_is_refused_by_name() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        let infinite = b.infinite_region();
        let inner: Vec<RegionId> = crate::topology::brep::p1::regions(&b, None)
            .unwrap()
            .into_iter()
            .filter(|&r| Some(r) != infinite)
            .collect();
        assert_eq!(inner.len(), 1);
        b.region_mut(inner[0]).unwrap().set_is_void(2);
        assert_eq!(
            estimated(&k, &mut b, &MassPropertyRequest::centroidal()),
            Err(SolidError::UnsureRegion {
                operation: OP_REGION,
                region: inner[0],
            })
        );
        assert!(region_material(&b, inner[0]).is_err());
        // upstream's reading, kept for the API's request: UNSURE is void
        assert!(!upstream_region_is_material(&b, inner[0]).unwrap());
    }

    /// Without WR-535 each side of a partition adds its area, so an area counting every
    /// face once would count the partition twice: the request is refused; with WR-535 the
    /// partitions are no part of the area (800).
    #[test]
    fn a_partition_s_area_needs_wr535() {
        use crate::primitive::api_primitives::create_box;
        let mut k = Kernel::default();
        let a = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
        let b = create_box(&mut k, Point3::new(5.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let mut merged = crate::boolean::api_brep::boolean_merge(&mut k, a, b).unwrap();
        let partitions = faces(&merged)
            .unwrap()
            .into_iter()
            .filter(|&f| partition_face(&merged, f).unwrap())
            .count();
        assert_eq!(partitions, 2);
        assert!(matches!(
            estimated(&k, &mut merged, &MassPropertyRequest::centroidal()),
            Err(SolidError::Unsupported { .. })
        ));
        k.fixes.brep_area_between_solid_and_void = true;
        let r = estimated(&k, &mut merged, &MassPropertyRequest::centroidal()).unwrap();
        assert!(near(r.area, 800.0, 1e-9), "{r:?}");
        assert!(near(r.volume, 1500.0, 1e-9), "{r:?}");
    }
}
