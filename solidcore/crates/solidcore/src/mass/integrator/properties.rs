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
//! A volume-separating faceuse (WR-535) adds its volume only where it bounds a non void region and its area only
//! where the area behaviour selects it (tangency-labs/smlib@d2d9586).
//! The iso-u integrals start at the trims' lowest v, not at the face UV domain's
//! (WR-563, #1556).
//! A face whose trims span at least `MIN_BANDS` surface v knots is integrated in bands
//! between them (#14460): each trim piece's
//! iso-u integral starts at its own band's floor, and each band's full height under the
//! trims is integrated once along u between their crossings of its top line, so the cost
//! grows with the knot spans, where upstream's grows with their square.
//! Each band's full height is integrated by nested adaptive 7-point Gauss-Kronrod
//! quadrature (`kronrod_integrate`, bisection bounded at 48 levels and 100,000 pieces),
//! not by nested Runge-Kutta steps: a dense spline grid has a band cell per pair of knot
//! spans, and two Runge-Kutta steps a level cost 144 samples a cell at the least.
//! Each span-wise iso-u piece is evaluated at its end from its own knot span, the
//! one-sided limit from inside the piece (WR-670, #2091): upstream takes the span above.
//! An edge's tube area, volume and moments each have an error scale of their own size
//! (#2887): upstream's are `BIG_DOUBLE`, so only the edge's length was under error control.
use super::{OdeIntegrator, OdeOptions};
use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{span, Nurbs};
use crate::curve::curve::Curve;
use crate::error::mass::MassFailure;
use crate::geom::attribute::{ids, AttributeStore, AttributeValue, ObjectKey};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::tol::are_same;
use crate::kernel::Kernel;
use crate::mass::closure::{Closure, ClosureRefusal, ClosureRefused, ClosureStrip};
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2::{
    surface_knots_inside, trim_curve_breaks, OdeArea, OdeIsoCurveLength, PrecisePropertyLayout,
    PPF_ALL, PPF_AREA, PPF_AREA_FIRST_MOMENTS,
};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, FaceuseId, RegionId, VertexId};
use crate::{EntityRef, Result, SolidError};

const OP: &str = "mass::integrator::properties";
/// The operation the `SM_ERR` of a span the integrator took no step over names (not
/// upstream: the code is upstream's, the name says which failure it is).
const OP_SKIPPED_SPAN: &str = "mass::integrator::properties::skipped_span";
/// The fewest surface v knots inside a face's trims for which the face is integrated in
/// bands between them (#14460).
const MIN_BANDS: usize = 12;

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
    /// Whether a B-rep's volume is the flux of a closed surface, and the bound on what is
    /// still open (not upstream, #14170; [`VolumeClosure`]).
    pub closure: VolumeClosure,
}

/// Whether a B-rep's volume and volume moments are fluxes out of a closed surface (not
/// upstream, #14170).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum VolumeClosure {
    /// Not a B-rep's: an edge's or a face's own properties.
    #[default]
    NotABrep,
    /// The faces are closed by their strips; what is open is bounded ([`ClosureBound`]).
    Bounded(ClosureBound),
    /// A face cannot be part of a closed surface: the volume is the faces' flux alone, and
    /// its accuracy is not proved (the integration's estimate does not see an open
    /// surface's dependence on the origin).
    Refused(ClosureRefused),
    /// The faces are closed by their strips, but what is open is not bounded: this face, a
    /// procedural blend, is integrated as its definition (no [`NetScope`] around the call
    /// takes its net), while the strips and caps are built on its net, which is within the
    /// net's certified bound of the definition but not it (#14170). The strips are added;
    /// no bound is reported.
    ///
    /// [`NetScope`]: crate::surface::procedural_blend::NetScope
    NetNotDefinition(FaceId),
}

/// Proved bounds on the flux through the caps that close a solid's faces (not upstream,
/// #14170).
///
/// A body's volume and its volume moments are fluxes of fields `F = (0, 0, f)` out of its
/// faces (`df/dz` the integrand: `f = z` for the volume, `x z`, `y z`, `z^2 / 2` for the
/// first moments, and so on, `z` taken from the origin of computation). Where two faces'
/// boundaries along an edge are apart, that is the flux of an open surface, which depends
/// on the origin. [`brep_properties`] closes it as [`crate::mass::enclosure`] does
/// (`mass::closure`): along every edge the ruled strip between the two faces' boundaries,
/// whose flux it adds. What is still open (the strips' ends, the loops' closing uv
/// segments, the pcurves' parts past their common parameters) is a set of closed curves of
/// total length at most `L`, every point within `L` of a point recorded on them: capped by
/// cones of area at most `L^2 / 4`, whose flux is at most `max |f|` over the box of those
/// points widened by `L`, times `L^2 / 4`. Each field here is that bound, proved (rounded
/// interval arithmetic). It is not the integration's error, which the integrator estimates
/// and does not bound; the property's error is that estimate plus this bound. Never
/// clamped: a bound larger than the accuracy asked is reported as it is.
///
/// The faces are integrated as the closure builds them (#14170), as [`crate::mass::enclosure`]
/// encloses them: each loop closed in uv, and each surface continued past its domain. The
/// trim boundary (`create_oriented_trim_boundary`) leaves a loop open where two trims' ends
/// are within its gap test, and an open loop loses the shadow of its gap down to the
/// integration's base line, not a cap: the `issue_1476` shell's wall, open by 1.5e-6 in uv
/// at two corners, lost a strip of 2.3e-4 and its volume moved by 2.3e-4 per unit of the
/// origin's z. So [`brep_properties`] closes each face's loops with uv lines across those
/// gaps before it integrates the face. A surface clamps its evaluation to the domain it is
/// defined on (a plane its analytic domain, a NURB its knots' range), while the closure
/// continues it; a face whose trims may leave that domain is integrated with the surface's
/// out-of-bounds evaluation on, so on the surface the closure is built on (where the trims
/// stay inside, the clamp never acts and the integral is the same to the bit). The bound is
/// the caps' alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClosureBound {
    /// The bound on the volume's caps (`f = z`).
    pub volume: f64,
    /// The same weighted by the largest density of the faces (the mass).
    pub mass: f64,
    /// The bounds on the volume moments' caps, as [`PreciseProperties::moments`]' last four
    /// (first, second, products, axial), weighted by the largest density.
    pub moments: [Vec3; 4],
    /// The openings' total 3D length bound `L`.
    pub open_length: f64,
    /// What the strips added to the volume (integrated, not bounded): the faces' flux alone
    /// is the volume less it.
    pub strip_volume: f64,
    /// The strips' `int n_z dA` (integrated): the faces' open vector area along z is its
    /// negative, so the faces' flux alone moves by it per unit of `O_z`.
    pub strip_area_z: f64,
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
    let curve =
        edge::curve(b, e)?.ok_or_else(|| SolidError::mass_missing(OP, "the edge's curve"))?;
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
        .ok_or_else(|| SolidError::mass_missing(OP, "the curve's start point"))?;
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
            rate: None,
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
fn boundary(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
) -> Result<crate::topology::face::p1::OrientedTrimBoundary> {
    let key_of = |e: EntityRef| crate::geom::object::ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    crate::topology::face::p1::create_oriented_trim_boundary(k, b, f, true, true, None, &key_of)
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
    face_properties_over(
        k,
        b,
        f,
        &trims.curves,
        orientation,
        accuracy,
        origin,
        estimated_area,
        thickness,
        flags,
        None,
    )
}
/// [`face_properties`] over the face's oriented trim boundary `trims`, made already.
#[allow(clippy::too_many_arguments)] // face_properties' inputs and the trims
fn face_properties_over(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    trims: &[Curve],
    orientation: OrientType,
    accuracy: f64,
    origin: Point3,
    estimated_area: f64,
    thickness: f64,
    flags: u64,
    unclamp: Option<&mut Vec<ClosureRefused>>,
) -> Result<PreciseProperties> {
    valid_flags(flags)?;
    let fbox = face_box(k, b, f)?;
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::mass_missing(OP, "the face's surface"))?;
    // the surface continued past the domain its evaluation clamps to, where the trims may
    // leave it and the continuation is certified, as the closure builds the face (#14170);
    // set for the integration alone (after the face's box, which may fill the surface's
    // caches), and restored when the guard drops, on every path
    let _unclamped = match unclamp {
        Some(refused) => {
            let (guard, refusal) = Unclamped::where_trims_may_leave(k, surface, trims)?;
            if let Some(why) = refusal {
                refused.push(ClosureRefused { face: Some(f), why });
            }
            Some(guard)
        }
        None => None,
    };
    boundary_properties(
        k,
        surface,
        trims,
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
/// UV domain; the domain's lowest v without trims. Also the union's highest v (the lowest
/// without trims).
fn trims_v_range(k: &Kernel, trims: &[Curve], domain: &Extent2d) -> Result<(f64, f64)> {
    use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let mut base = BIG_DOUBLE;
    let mut top = -BIG_DOUBLE;
    for trim in trims {
        let bbox = trim
            .calculate_bounding_box(k, &trim.natural_interval(), req, true)?
            .normal
            .ok_or_else(|| SolidError::mass_failed(OP, "the trim has no bounding box"))?;
        let v = bbox.min().y;
        if v < base {
            base = v;
        }
        let v = bbox.max().y;
        if v > top {
            top = v;
        }
    }
    let min_v = domain.min().y;
    let base = if trims.is_empty() || base < min_v {
        min_v
    } else {
        base
    };
    Ok((base, if top > base { top } else { base }))
}
/// Whether the integrator takes no step from `a` to `b` (as [`trim_curve_breaks`] merges
/// such breaks): their ends `SM_ARE_SAME`, or the first step of half the span
/// `SM_ARE_SAME` as `SM_EFF_ZERO` or below it.
fn unsteppable(a: f64, b: f64) -> bool {
    are_same(a, b) || are_same(0.5 * (b - a), EFF_ZERO) || 0.5 * (b - a) < EFF_ZERO
}
/// The integral of `integrand` (writing `dy` at `x`) from `a` to `b` into `y`, the
/// precise properties' outer integration: Runge-Kutta at `accuracy` with the components'
/// error `scale`, a first step of half the span, then finer first steps where the
/// coarser fails.
fn integrate_piece<F>(
    k: &Kernel,
    a: f64,
    b: f64,
    accuracy: f64,
    scale: &[f64],
    y: &mut [f64],
    mut integrand: F,
) -> Result<()>
where
    F: FnMut(f64, &mut [f64]) -> Result<()>,
{
    // A coarse first Runge-Kutta step can trip the integrator's abort guard for one
    // traversal direction, so a reversed cap loop fails where its twin succeeds. Try
    // a span/2 first step, then fall back to finer steps.
    let mut status = Err(SolidError::mass_failed(OP, "the span was not integrated"));
    for divisor in [2.0, 8.0, 64.0, 512.0, 4096.0] {
        k.check_cancelled(OP)?;
        let mut eval_calls = 0u64;
        status = OdeIntegrator::default()
            .integrate(
                y,
                a,
                b,
                OdeOptions {
                    accuracy,
                    initial_step: (b - a) / divisor,
                    minimum_step: 0.0,
                    scale: Some(scale),
                    quadrature: true,
                    rounding: None,
                    rate: None,
                },
                |t, _, dy| {
                    eval_calls += 1;
                    if (eval_calls & 63) == 1 {
                        k.check_cancelled(OP)?;
                    }
                    integrand(t, dy)
                },
            )
            // Success with no steps means the integrator skipped the span.
            .and_then(|counts| {
                if counts.good + counts.reduced == 0 {
                    Err(SolidError::mass(OP_SKIPPED_SPAN, MassFailure::SkippedSpan))
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
    status
}
/// The 7-point Gauss-Kronrod rule on `[-1, 1]` and its embedded 3-point Gauss rule: the
/// nodes `0, ±x1, ±x2, ±x3` (`x2 = sqrt(3/5)`, the Gauss nodes with `0`), the Kronrod
/// weights of each, and the Gauss weights (zero where the node is not a Gauss node).
/// The Kronrod rule integrates polynomials of degree 10 exactly, the Gauss rule degree 5.
const KRONROD_NODES: [f64; 4] = [
    0.0,
    0.434_243_749_346_802_558_002_071_502_844_628,
    0.774_596_669_241_483_377_035_853_079_956_480,
    0.960_491_268_708_020_283_423_507_092_629_080,
];
const KRONROD_WEIGHTS: [f64; 4] = [
    0.450_916_538_658_474_142_345_110_087_045_571,
    0.401_397_414_775_962_222_905_051_818_618_432,
    0.268_488_089_868_333_440_728_569_280_666_710,
    0.104_656_226_026_467_265_193_823_857_192_073,
];
const GAUSS_WEIGHTS: [f64; 4] = [8.0 / 9.0, 0.0, 5.0 / 9.0, 0.0];
/// The deepest bisection of [`kronrod_integrate`]: a piece `2^-48` of the range.
const KRONROD_MAX_DEPTH: u32 = 48;
/// The most pieces [`kronrod_integrate`] evaluates over one range.
const KRONROD_MAX_PIECES: u32 = 100_000;

/// Not upstream (#14460): the integral of `integrand` (writing `dy` at `x`) from `a` to `b`
/// into `y`, by adaptive 7-point Gauss-Kronrod quadrature. Each piece's error is taken as
/// the difference of its Kronrod and embedded Gauss values, the error of the Gauss value:
/// the Kronrod value returned is exact on polynomials of degree 10 where the Gauss value
/// is on degree 5, so on a smooth integrand its own error is far below that estimate. A
/// piece is accepted when, per component, the estimate is within `accuracy` of the larger
/// of the piece's value and the component's error `scale` (as a Runge-Kutta step of
/// [`integrate_piece`] is held to its values or that scale); otherwise it is halved.
/// The pieces are summed left to right. No node is an end of the range, so an integrand
/// smooth inside `(a, b)` is never read on the far side of a knot at either end.
///
/// # Errors
///
/// Those of `integrand`; a non-finite value (`MassFailure::Integrand`), and the step
/// control's failure past `KRONROD_MAX_DEPTH` bisections or `KRONROD_MAX_PIECES` pieces
/// (`MassFailure::StepControl`).
fn kronrod_integrate<F>(
    k: &Kernel,
    a: f64,
    b: f64,
    accuracy: f64,
    scale: &[f64],
    y: &mut [f64],
    mut integrand: F,
) -> Result<()>
where
    F: FnMut(f64, &mut [f64]) -> Result<()>,
{
    use crate::mass::integrator::{OP_INTEGRAND, OP_STEP_CONTROL};
    let n = y.len();
    let mut kronrod = vec![0.0; n];
    let mut gauss = vec![0.0; n];
    let mut dy = vec![0.0; n];
    // the pieces still to integrate, the leftmost last (depth with each)
    let mut stack = vec![(a, b, 0_u32)];
    let mut pieces = 0_u32;
    while let Some((lo, hi, depth)) = stack.pop() {
        k.check_cancelled(OP)?;
        pieces += 1;
        if pieces > KRONROD_MAX_PIECES {
            return Err(SolidError::mass(OP_STEP_CONTROL, MassFailure::StepControl));
        }
        let mid = 0.5 * (lo + hi);
        let half = 0.5 * (hi - lo);
        kronrod.fill(0.0);
        gauss.fill(0.0);
        for (i, &x) in KRONROD_NODES.iter().enumerate() {
            let wk = KRONROD_WEIGHTS.get(i).copied().unwrap_or(0.0);
            let wg = GAUSS_WEIGHTS.get(i).copied().unwrap_or(0.0);
            let sides: &[f64] = if i == 0 { &[0.0] } else { &[-1.0, 1.0] };
            for &side in sides {
                dy.fill(0.0);
                integrand(mid + side * x * half, &mut dy)?;
                if !dy.iter().all(|v| v.is_finite()) {
                    return Err(SolidError::mass(OP_INTEGRAND, MassFailure::Integrand));
                }
                for ((kv, gv), d) in kronrod.iter_mut().zip(gauss.iter_mut()).zip(&dy) {
                    *kv += wk * d;
                    *gv += wg * d;
                }
            }
        }
        let mut accepted = true;
        for (i, (kv, gv)) in kronrod.iter_mut().zip(gauss.iter_mut()).enumerate() {
            *kv *= half;
            *gv *= half;
            let floor = scale.get(i).copied().unwrap_or(0.0);
            let s = kv.abs().max(floor).max(EFF_ZERO);
            let estimate = (*kv - *gv).abs();
            if estimate.is_nan() || estimate > accuracy * s {
                accepted = false;
            }
        }
        if accepted {
            for (total, part) in y.iter_mut().zip(&kronrod) {
                *total += *part;
            }
            continue;
        }
        if depth >= KRONROD_MAX_DEPTH || unsteppable(lo, mid) || unsteppable(mid, hi) {
            return Err(SolidError::mass(OP_STEP_CONTROL, MassFailure::StepControl));
        }
        stack.push((mid, hi, depth + 1));
        stack.push((lo, mid, depth + 1));
    }
    Ok(())
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
    let (base_v, top_v) = trims_v_range(k, trims, domain)?;
    // The face's bands: from `base_v` and each of the surface's v knots up to the next
    // (#14460). Upstream's iso-u integral at each trim point runs from the base up to
    // the point, across every knot span below it (span by span, WR-494), so a face long
    // in v, a thread flank over every turn, cost the trim points times the spans below
    // them: the square of its spans. Here a trim piece in band j integrates from the
    // band's floor `floors[j]` only, and the rest of its iso-u line, the full height of
    // each band i below j, is a function `G_i(u)` of u alone: its integral against the
    // piece's du depends on the piece's ends' u only. Summed over the loops, each band's
    // full-height part is `G_i` integrated once along u between where the trims cross
    // the band's top line: the area of that band under the trims, once per band.
    // The same Green's-theorem integral, regrouped by band; linear in the spans.
    // A face crossing fewer than `MIN_BANDS` knot lines keeps the one base: its iso-u
    // integrals are short already, and a band's own integral along u would cost more
    // than it saves.
    let mut floors = vec![base_v];
    let lines = surface_knots_inside(k, surface, SurfParamType::V, base_v, top_v)?;
    if lines.len() >= MIN_BANDS {
        floors.extend(lines);
    }
    let banded = floors.len() > 1;
    let v_lines = floors.get(1..).unwrap_or_default();
    let band_of = |v: f64| floors.partition_point(|&f| f <= v).saturating_sub(1);
    let floor = |band: usize| floors.get(band).copied().unwrap_or(base_v);

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
    // (no trims: nothing to integrate)
    let Some(first) = trims.first() else {
        return Ok(PreciseProperties::default());
    };
    let area = OdeArea {
        kernel: k,
        surface,
        uv_curve: first,
        min_v: base_v,
        iso_curve_accuracy: accuracy / 10.0,
        y_scale: &scale,
        origin,
        face_thickness: thickness,
        layout: &layout,
    };

    let mut y = vec![0.0; layout.num_components];
    let mut span_y = vec![0.0; layout.num_components];
    let mut scratch = vec![0.0; layout.num_components + 1];
    // every piece of every trim: its ends' u and its band
    let mut pieces: Vec<(f64, f64, usize)> = Vec::new();
    for trim in trims {
        // Integrate piece by piece between the trim curve's knots and its crossings of the
        // surface's knot lines u = k (WR-494; the SolidCraft fork's
        // tangency-labs/smlib@08d9a14): at either the integrand's derivatives jump, and
        // at a knot of multiplicity reaching the degree (a rational circle's double knot)
        // the integrand itself does. A step's error estimate is the leading term of an
        // expansion that holds only where the integrand is smooth across the step.
        // Also at its crossings of the bands' floors, where its iso-u base changes.
        let knots = trim_curve_breaks(k, trim, surface, v_lines)?;
        let mut u_at = Vec::new();
        if banded {
            for &t in &knots {
                u_at.push(trim.evaluate_point(k, t)?.x);
            }
        }
        let mut trim_y = vec![0.0; layout.num_components];
        for (i, pair) in knots.windows(2).enumerate() {
            let &[a, b] = pair else {
                continue;
            };
            let mut band = 0;
            if banded {
                band = band_of(trim.evaluate_point(k, 0.5 * (a + b))?.y);
                if let (Some(&ua), Some(&ub)) = (u_at.get(i), u_at.get(i + 1)) {
                    pieces.push((ua, ub, band));
                }
            }
            // (`!(dB > dA)`: also an unordered pair)
            if b.partial_cmp(&a) != Some(std::cmp::Ordering::Greater) {
                continue; // a crossing at a knot
            }
            let eval = OdeArea {
                uv_curve: trim,
                min_v: floor(band),
                ..area
            };
            span_y.fill(0.0);
            integrate_piece(k, a, b, accuracy / 10.0, &scale, &mut span_y, |t, dy| {
                // (the evaluator writes every component, zero where the trim's du/dt is)
                eval.evaluate(k, t, &mut scratch)?;
                for (d, v) in dy.iter_mut().zip(scratch.iter().skip(1)) {
                    *d = *v;
                }
                Ok(())
            })?;
            for (total, part) in trim_y.iter_mut().zip(span_y.iter()) {
                *total += *part;
            }
        }
        for (total, part) in y.iter_mut().zip(trim_y.iter()) {
            *total += *part;
        }
    }
    // Each band's full height under the pieces above it: the pieces' `-∫ G_i du`, as
    // `Σ c ∫ G_i du` over u, `c` the signed count of the pieces' ends at or left of u
    // (each piece's end +1, its start -1). Consecutive pieces' shared ends cancel, so `c`
    // changes only where the loops cross the band's top line, and a loop wholly above
    // it adds nothing.
    let mut scratch_iso = vec![0.0; layout.num_components + 1];
    let mut ends: Vec<(f64, i32)> = Vec::new();
    let mut steps: Vec<(f64, i32)> = Vec::new();
    for (band, pair) in floors.windows(2).enumerate() {
        let &[lo, hi] = pair else {
            continue;
        };
        ends.clear();
        for &(ua, ub, _) in pieces.iter().filter(|p| p.2 > band) {
            ends.push((ub, 1));
            ends.push((ua, -1));
        }
        ends.sort_by(|p, q| p.0.total_cmp(&q.0));
        // (where the count changes, and its value from there: an interval is split only
        // where its weight changes, not at every piece end inside it)
        steps.clear();
        let mut count = 0_i32;
        let mut i = 0;
        while let Some(&(u0, _)) = ends.get(i) {
            // (every end at u0)
            let before = count;
            while let Some(&(u, sign)) = ends.get(i) {
                if u != u0 {
                    break;
                }
                count += sign;
                i += 1;
            }
            if count != before {
                steps.push((u0, count));
            }
        }
        for pair in steps.windows(2) {
            let &[(u0, count), (u1, _)] = pair else {
                continue;
            };
            if count == 0 {
                continue;
            }
            // (`G_i` is smooth within the surface's u knot spans.) Gauss-Kronrod at both
            // levels: a dense grid (a G2 corner patch) has a band cell per u span, and a
            // Runge-Kutta step pair a level cost at least 144 samples a cell, the
            // 7-point rule 49 (#14460). Its nodes are inside each piece, so neither
            // level reads an integrand on the far side of a knot.
            let mut a = u0;
            let v_knots = surface_knots_inside(k, surface, SurfParamType::V, lo, hi)?;
            let u_knots = surface_knots_inside(k, surface, SurfParamType::U, u0, u1)?;
            for b in u_knots.into_iter().chain(core::iter::once(u1)) {
                if unsteppable(a, b) {
                    continue;
                }
                span_y.fill(0.0);
                kronrod_integrate(k, a, b, accuracy / 10.0, &scale, &mut span_y, |u, dy| {
                    let eval =
                        OdeIsoCurveLength::new(area.kernel, surface, u, origin, thickness, &layout);
                    // (the band's height, piece by piece between any v knots inside it)
                    let mut v0 = lo;
                    for v1 in v_knots.iter().copied().chain(core::iter::once(hi)) {
                        kronrod_integrate(
                            k,
                            v0,
                            v1,
                            area.iso_curve_accuracy,
                            &scale,
                            dy,
                            |v, g| {
                                eval.evaluate(v, &mut scratch_iso)?;
                                for (d, x) in g.iter_mut().zip(scratch_iso.iter().skip(1)) {
                                    *d = *x;
                                }
                                Ok(())
                            },
                        )?;
                        v0 = v1;
                    }
                    Ok(())
                })?;
                let c = f64::from(count);
                for (total, part) in y.iter_mut().zip(span_y.iter()) {
                    *total += c * *part;
                }
                a = b;
            }
        }
    }
    let value = |index: usize| y.get(index.wrapping_sub(1)).copied().unwrap_or(0.0);
    let mut result = PreciseProperties::default();
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
    crate::topology::edgeuse::p1::faceuse_region(b, fu)?
        .ok_or_else(|| SolidError::mass_missing(OP, "the faceuse's region"))
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
    // why the volume is not the flux of the faces alone (a face not separating, or thick;
    // the policy), which the closure strips close (#14170)
    let mut not_flux = (behavior.face_volume != 0).then_some(ClosureRefused {
        face: None,
        why: ClosureRefusal::VolumePolicy,
    });
    // each integrated face's oriented trim boundary, made once (#14170)
    let mut face_trims = std::collections::BTreeMap::new();
    // the first face integrated clamped because its continuation is not certified (#14170)
    // the faces integrated clamped, their continuation not certified, and why
    let mut uncertified: Vec<ClosureRefused> = Vec::new();
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
            .ok_or_else(|| SolidError::mass_missing(OP, "the face's faceuse"))?;
        let mate = b
            .faceuse(up)?
            .mate
            .ok_or_else(|| SolidError::mass_missing(OP, "the faceuse's mate"))?;
        let mut separating = faceuse_region(b, up)? != faceuse_region(b, mate)?;
        if not_flux.is_none() && (!separating || mass.y > EFF_ZERO) {
            not_flux = Some(ClosureRefused {
                face: Some(f),
                why: if separating {
                    ClosureRefusal::Thick
                } else {
                    ClosureRefusal::NotOneMaterialSide
                },
            });
        }
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
                        .ok_or_else(|| SolidError::mass_missing(OP, "the faceuse's mate"))?;
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
                // the face's trim boundary, made once and kept for the closure, which finds
                // the loops it leaves open from it (#14170)
                if let std::collections::btree_map::Entry::Vacant(slot) = face_trims.entry(f) {
                    // its loops closed: the uv lines across the gaps the trim boundary leaves
                    // open, integrated with the trims (#14170: an open loop loses its gap's
                    // shadow down to the integration's base)
                    let mut trims = boundary(k, b, f)?;
                    close_loops(k, &mut trims)?;
                    slot.insert(trims);
                }
                let trims = face_trims
                    .get(&f)
                    .ok_or_else(|| SolidError::mass_failed(OP, "the face's trims"))?;
                let delta = face_properties_over(
                    k,
                    b,
                    f,
                    &trims.curves,
                    orientation,
                    accuracy,
                    origin,
                    estimated,
                    if separating { 0.0 } else { mass.y },
                    PPF_ALL,
                    Some(&mut uncertified),
                )?;
                if separating {
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
    // a face integrated clamped (its continuation not certified) refuses the bound, by
    // name, and only its own strips: the body's other strips still close their gaps; a
    // volume that is no flux skips the closure
    let closed = match not_flux {
        Some(r) => VolumeClosure::Refused(r),
        None => close_faces(
            k,
            b,
            accuracy,
            origin,
            &bbox,
            estimated,
            attributes,
            behavior,
            &uncertified,
            &mut total,
        )?,
    };
    total.closure = match uncertified.first() {
        Some(&r) => VolumeClosure::Refused(r),
        None => closed,
    };
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
/// The number of volume components a strip adds: the volume, then the volume moments
/// (first 3, second 3, products 3, axial 3), as
/// [`crate::topology::face::p2::OdeIsoCurveLength`]'s layout orders them.
const CLOSURE_COMPONENTS: usize = 13;

/// The components a strip integrates: the volume components, then `int n_z dA` (the
/// strips' vector area along z, reported in [`ClosureBound::strip_area_z`]).
const STRIP_COMPONENTS: usize = CLOSURE_COMPONENTS + 1;

/// The strips' sense on the closed surface, derived. A face's flux is integrated round its
/// loops by Green's theorem: each loop walked with the face on its left about `S_u x S_v`
/// (`create_oriented_trim_boundary`), the result times the face's sign, `-1` where its
/// material side is its upward use (`mass::closure`'s `ClosureFace::sign`). That is the flux
/// with the normal `sign (S_u x S_v)` pointing out of the material, whose boundary (Stokes'
/// orientation: the face on the left about that normal) runs along the edge parameter `t`
/// where `dir sign > 0` (`dir` the walk along the pcurve): the strip's first side `X_1`.
/// The second face's boundary runs against `t` along `X_2`. A strip
/// `X(t, s) = X_1 + s (X_2 - X_1)` closes them when its own boundary runs opposite to
/// both: against `t` along `X_1` (`s = 0`), along `t` along `X_2` (`s = 1`). The boundary of the `(t, s)` square
/// with the normal `X_t x X_s` runs along `t` at `s = 0`, so the strip's outward normal is
/// the opposite, `-(X_t x X_s)`: the sense is -1. (The tilted box of #14170's test checks
/// it: the other sign doubles the origin dependence instead of cancelling it.)
const STRIP_SENSE: f64 = -1.0;

/// The fields `f` of the volume components at `p` (relative to the origin of
/// computation): their flux `int f n_z dA` out of the faces is each component, as
/// [`crate::topology::face::p2::OdeIsoCurveLength`] integrates it (`f` its integrand
/// over `n_z dA`).
fn volume_fields(p: Vec3) -> [f64; CLOSURE_COMPONENTS] {
    let (x, y, z) = (p.x, p.y, p.z);
    [
        z,
        x * z,
        y * z,
        z * z / 2.0,
        (x * x) * z,
        (y * y) * z,
        (z * z / 3.0) * z,
        (y * z) * z / 2.0,
        (z * x) * z / 2.0,
        (x * y) * z,
        (y * y + z * z / 3.0) * z,
        (x * x + z * z / 3.0) * z,
        (x * x + y * y) * z,
    ]
}

/// Bounds on `|f|` of [`volume_fields`] over the box `p`, rounded up.
fn volume_field_bounds(p: [Ivl; 3]) -> [f64; CLOSURE_COMPONENTS] {
    let [x, y, z] = p;
    let (two, three) = (Ivl::from(2.0), Ivl::from(3.0));
    [
        z,
        x * z,
        y * z,
        z * z / two,
        (x * x) * z,
        (y * y) * z,
        (z * z / three) * z,
        (y * z) * z / two,
        (z * x) * z / two,
        (x * y) * z,
        (y * y + z * z / three) * z,
        (x * x + z * z / three) * z,
        (x * x + y * y) * z,
    ]
    .map(Ivl::mag)
}

/// Not upstream (#14170): closes the faces' flux surface as [`crate::mass::enclosure`]
/// does (`mass::closure`): adds each strip's flux to `total`'s volume, mass and volume
/// moments, and returns the caps' [`ClosureBound`]; or the face that cannot be closed and
/// why ([`VolumeClosure::Refused`]: `total` is then the faces' flux alone, its accuracy not
/// proved); or, with `clamped` faces (integrated clamped, their continuation not
/// certified), the first of them refused, the strips of the others added; or, with the strips added, the procedural blend taken as its definition
/// ([`VolumeClosure::NetNotDefinition`]). An edge whose sides do not pair is an opening, not
/// a refusal.
#[allow(clippy::too_many_arguments)] // brep_properties' state, passed on
fn close_faces(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    origin: Point3,
    bbox: &Extent3d,
    estimated: f64,
    attributes: &PropertyAttributes<'_>,
    behavior: MassPropertyBehavior,
    clamped: &[ClosureRefused],
    total: &mut PreciseProperties,
) -> Result<VolumeClosure> {
    let refused = |why| VolumeClosure::Refused(ClosureRefused { face: None, why });
    let closure = match Closure::of(k, b)? {
        Ok(c) => c,
        Err(r) => return Ok(VolumeClosure::Refused(r)),
    };
    // a face integrated clamped: its strips, built on its surface continued, are not its
    // integration's edges, and are left out; no bound is given, so no openings are found
    let is_clamped = |i: usize| {
        closure
            .faces
            .get(i)
            .is_some_and(|cf| clamped.iter().any(|r| r.face == Some(cf.face)))
    };
    let openings = if clamped.is_empty() {
        match closure.openings() {
            Ok(o) => Some(o),
            Err(SolidError::InvalidInput { .. } | SolidError::Unsupported { .. }) => {
                return Ok(refused(ClosureRefusal::Unbounded))
            }
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    // each face's density, as the face loop reads it
    let mut density = Vec::with_capacity(closure.faces.len());
    for cf in &closure.faces {
        let mut m = attributes.mass_vector(
            EntityRef::Face(cf.face),
            Vec3::new(behavior.density, behavior.face_thickness, 0.0),
        )?;
        if m.x < EFF_ZERO {
            m.x = 1.0;
        }
        density.push(m.x);
    }
    // a procedural blend taken as its definition, which no bound covers yet (#15022)
    let mut net_not_definition = None;
    for cf in &closure.faces {
        if let SurfaceKind::ProceduralBlend(pb) = b
            .face(cf.face)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::mass_missing(OP, "the face's surface"))?
            .kind()
        {
            if !pb.net_answers(k) {
                net_not_definition = net_not_definition.or(Some(cf.face));
            }
        }
    }
    let scale = closure_scale(k, bbox, origin, estimated);
    // the strips' volume, mass and volume moments, added once all are integrated
    let mut volume = 0.0;
    let mut mass = 0.0;
    let mut moments = [Vec3::ZERO; 4];
    let mut area_z = 0.0;
    for strip in &closure.strips {
        k.check_cancelled(OP)?;
        if strip.sides.iter().any(|s| is_clamped(s.face)) {
            continue;
        }
        let y = match strip_flux(k, &closure, strip, origin, accuracy, &scale) {
            Ok(y) => y,
            Err(SolidError::Unsupported { .. }) => return Ok(refused(ClosureRefusal::Unbounded)),
            Err(e) => return Err(e),
        };
        // a strip's mass at the mean density of its two faces
        let d = 0.5
            * (strip.sides.iter())
                .map(|s| density.get(s.face).copied().unwrap_or(1.0))
                .sum::<f64>();
        let [v, rest @ .., nz] = y;
        volume += v;
        mass += v * d;
        area_z += nz;
        for (m, c) in moments.iter_mut().zip(rest.chunks(3)) {
            if let &[cx, cy, cz] = c {
                *m += Vec3::new(cx, cy, cz) * d;
            }
        }
    }
    total.volume += volume;
    total.mass += mass;
    for (t, m) in total.moments.iter_mut().skip(4).zip(moments) {
        *t += m;
    }
    if let Some(&r) = clamped.first() {
        return Ok(VolumeClosure::Refused(r));
    }
    // found above wherever no face is clamped
    let Some(openings) = openings else {
        return Ok(refused(ClosureRefusal::Unbounded));
    };
    // the caps: in the box of the openings' points widened by L
    let l = openings.length.hi;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &openings.points {
        for (i, c) in p.iter().enumerate() {
            if let (Some(a), Some(z)) = (lo.get_mut(i), hi.get_mut(i)) {
                *a = a.min(c.lo);
                *z = z.max(c.hi);
            }
        }
    }
    let o = [origin.x, origin.y, origin.z];
    let mut bound = ClosureBound {
        open_length: l,
        strip_volume: volume,
        strip_area_z: area_z,
        ..ClosureBound::default()
    };
    if !openings.points.is_empty() {
        let axis = |i: usize| -> Ivl {
            let (a, z, c) = (
                lo.get(i).copied().unwrap_or(f64::NAN),
                hi.get(i).copied().unwrap_or(f64::NAN),
                o.get(i).copied().unwrap_or(f64::NAN),
            );
            Ivl::new(a, z) + Ivl::new(-l, l) - Ivl::from(c)
        };
        let fb = volume_field_bounds([axis(0), axis(1), axis(2)]);
        let area = Ivl::from(l) * Ivl::from(l) / Ivl::from(4.0);
        let dmax = density.iter().copied().fold(1.0_f64, f64::max);
        let cap = |f: f64| (Ivl::from(f) * area).hi;
        let [v, rest @ ..] = fb;
        bound.volume = cap(v);
        bound.mass = (Ivl::from(bound.volume) * Ivl::from(dmax)).hi;
        for (m, c) in bound.moments.iter_mut().zip(rest.chunks(3)) {
            if let &[cx, cy, cz] = c {
                let w = |f: f64| (Ivl::from(cap(f)) * Ivl::from(dmax)).hi;
                *m = Vec3::new(w(cx), w(cy), w(cz));
            }
        }
    }
    Ok(match net_not_definition {
        Some(face) => VolumeClosure::NetNotDefinition(face),
        None => VolumeClosure::Bounded(bound),
    })
}

/// Where a trim of `trims` may leave the parameter box `[u0, u1] x [v0, v1]`, where its
/// surface's evaluation clamps it: `None` where every curve's control points are proved
/// inside (their homogeneous coordinates over their weights in rounded interval arithmetic),
/// else the uv box of all the curves' control points, `(u0, u1, v0, v1)`. A NURB curve with
/// positive weights lies in the convex hull of its control points, and a box is convex, so
/// the curve lies in the box when they do; the loops' closing uv lines are among the curves
/// (`close_loops`), and run between two curve ends. A trim along the box's edge has its
/// control points on it and is inside. A weight not proved positive, or a curve with no
/// B-spline form, is taken as leaving, over the whole parameter plane.
fn trims_past(
    trims: &[Curve],
    u: (f64, f64),
    v: (f64, f64),
) -> Result<Option<(f64, f64, f64, f64)>> {
    let mut hull = (u.0, u.1, v.0, v.1);
    let mut leaves = false;
    for c in trims {
        let Some(bs) = c.b_spline_base() else {
            return Ok(Some((
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
            )));
        };
        let cn = NurbsCurve::of(bs)?;
        for p in cn.pw.chunks(4) {
            let &[xw, yw, _, w] = p else {
                return Ok(Some((
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                )));
            };
            if w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return Ok(Some((
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                )));
            }
            let (x, y) = (Ivl::from(xw) / Ivl::from(w), Ivl::from(yw) / Ivl::from(w));
            if x.lo < u.0 || x.hi > u.1 || y.lo < v.0 || y.hi > v.1 {
                leaves = true;
            }
            hull = (
                hull.0.min(x.lo),
                hull.1.max(x.hi),
                hull.2.min(y.lo),
                hull.3.max(y.hi),
            );
        }
    }
    Ok(leaves.then_some(hull))
}

/// A face's surface with out-of-bounds evaluation on for the face's integration, where its
/// trims may leave the domain its evaluation clamps to and its continuation is certified
/// (#14170); restored when dropped, so on every path out of the integration (as upstream's
/// `SmTemporaryChangeValue` on `GetOutOfBoundsEnabled()`). With it the scope's own
/// continuation (`BSplineSurface::continues_past_domain`), which the B-spline evaluation of
/// the integration reads: upstream's `EvaluateSimple` clamps whatever the out-of-bounds flag,
/// and so does that evaluation outside this scope. The flags are the B-spline part's
/// that the evaluation reads: a plane's base, a B-spline, an analytic kind's NURB
/// (`Surface::analytic_base_with_nurb`), a blend's or a ball patch's net. A kind with no such
/// part (an offset, curve-bounded, STEP or in-volume surface) has no NURB form either, and
/// the closure refuses its face (`ClosureRefusal::NoNurbsForm`). The faces are integrated one
/// at a time, so a surface shared by two faces is toggled for one integration at a time.
///
/// The domain tested is the one the integration's evaluation clamps to: a plane's analytic
/// domain (`Plane::evaluate`), else the NURB's natural (knot) domain, which every other kind
/// is evaluated through (`Surface::evaluate_simple_first_into` reads `analytic_base_with_nurb`
/// for the analytic kinds and the net for a ball patch, at the face's uv, so the face's
/// pcurves are in the NURB's parameters).
///
/// What the flag reaches, and no cache: the guard is set inside `face_properties_over` after
/// the face's box (`face_box`, which can fill the trimmed cache) and covers
/// `boundary_properties` alone, whose evaluations are `Curve::calculate_bounding_box` and the
/// trims' own evaluation (`trims_v_range`, `trim_curve_breaks`, `OdeArea::evaluate`), the
/// surface's knots (`surface_knots_inside`, `Surface::knots_with_nurb`, which reads or makes
/// the NURB from the surface's own data), and its point and partials
/// (`OdeIsoCurveLength::evaluate` -> `Surface::evaluate_simple_first_into` ->
/// `BSplineSurface::evaluate_simple_first_into` or `Plane::evaluate_simple` or
/// `ProceduralBlend::evaluate_simple`, the net's while the net answers). None reads or fills
/// the trimmed cache (`TrimCacheSlot`) or `BoundaryMemo` (`CoincidenceCheck`'s).
struct Unclamped<'a> {
    /// The surface toggled, with the two flags' values before.
    base: Option<(
        &'a crate::nurbs::b_spline_surface::BSplineSurface,
        bool,
        bool,
    )>,
}

impl<'a> Unclamped<'a> {
    /// Turns `surface`'s out-of-bounds evaluation on where `trims` may leave its domain and
    /// its continuation over the trims' box is certified: a non-rational NURB, or a rational
    /// one whose weights are proved positive over that box, piece by piece (as the enclosure
    /// proves them, [`continuation_certified`]). Where the trims may leave but the
    /// continuation is not certified, nothing is toggled and the refusal is returned.
    fn where_trims_may_leave(
        k: &Kernel,
        surface: &'a Surface,
        trims: &[Curve],
    ) -> Result<(Self, Option<ClosureRefusal>)> {
        let none = Self { base: None };
        let (base, domain) = match surface.kind() {
            SurfaceKind::Plane(plane) => (plane.base(), Some(plane.step_uv_domain())),
            SurfaceKind::BSpline(s) => (s, None),
            SurfaceKind::BallPatch(s) => (s.net(), None),
            // a blend answers from its net only where a scope takes the net; integrated as its
            // definition it has no bound yet (#15022), and its station solves are left alone
            SurfaceKind::ProceduralBlend(s) if s.net_answers(k) => (s.net(), None),
            SurfaceKind::Cone(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_) => (surface.analytic_base_with_nurb(k)?, None),
            _ => return Ok((none, None)),
        };
        if base.is_out_of_bounds_enabled() {
            return Ok((none, None));
        }
        let domain = domain.unwrap_or_else(|| base.natural_uv_domain());
        let (u, v) = (
            (domain.u_min(), domain.u_max()),
            (domain.v_min(), domain.v_max()),
        );
        let Some(hull) = trims_past(trims, u, v)? else {
            return Ok((none, None));
        };
        // a plane is a bilinear, non-rational patch: its continuation is itself
        if !matches!(surface.kind(), SurfaceKind::Plane(_)) && !continuation_certified(base, hull)?
        {
            let unbounded = hull.0.is_infinite() || hull.1.is_infinite();
            return Ok((
                none,
                Some(if unbounded {
                    ClosureRefusal::TrimNotBounded
                } else {
                    ClosureRefusal::WeightsNotProved
                }),
            ));
        }
        let was = (
            base.is_out_of_bounds_enabled(),
            base.continues_past_domain(),
        );
        base.set_out_of_bounds_enabled_in_place(true);
        base.set_continues_past_domain_in_place(true);
        Ok((
            Self {
                base: Some((base, was.0, was.1)),
            },
            None,
        ))
    }
}

/// The times a box is cut in halves along any path while [`continuation_certified`] proves
/// its weights, as [`crate::mass::enclosure`] cuts a trim's (`g_integral`, 2^16 parts).
const CERTIFY_CUTS: u32 = 16;

/// Whether `base`'s continuation over the uv box `hull` (`(u0, u1, v0, v1)`) is certified:
/// non-rational (every weight 1), or its weights proved positive over the box as the
/// enclosure proves them (`mass::enclosure::partials_over`: within a span by its control
/// weights, past the knots by the continued piece's Taylor enclosure), the box cut in
/// halves where that is too coarse, along the side longer against the domain's, at most
/// [`CERTIFY_CUTS`] times along any path. Not certified where a box's middle is not proved
/// positive (the weight is not, or not provably, positive there) or the cuts run out.
fn continuation_certified(
    base: &crate::nurbs::b_spline_surface::BSplineSurface,
    hull: (f64, f64, f64, f64),
) -> Result<bool> {
    use crate::mass::enclosure::partials_over;
    let cn = Nurbs::of(base)?;
    if cn.pw.chunks(4).all(|p| p.get(3) == Some(&1.0)) {
        return Ok(true);
    }
    let (u0, u1, v0, v1) = hull;
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return Ok(false);
    }
    // the weights proved positive over the box
    let proved = |a: f64, b: f64, c: f64, d: f64| -> Result<bool> {
        match partials_over(&cn, Ivl::new(a, b), Ivl::new(c, d), 0) {
            Ok(p) => Ok(p.is_some()),
            Err(SolidError::Unsupported { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    };
    let domain = base.natural_uv_domain();
    let (du, dv) = (
        domain.u_max() - domain.u_min(),
        domain.v_max() - domain.v_min(),
    );
    let mut stack = vec![(u0, u1, v0, v1, 0u32)];
    while let Some((a, b, c, d, cuts)) = stack.pop() {
        if proved(a, b, c, d)? {
            continue;
        }
        let (um, vm) = (0.5 * (a + b), 0.5 * (c + d));
        if cuts >= CERTIFY_CUTS || !proved(um, um, vm, vm)? {
            return Ok(false);
        }
        let (cut_u, cut_v) = (a < um && um < b, c < vm && vm < d);
        if cut_u && (!cut_v || (b - a) * dv >= (d - c) * du) {
            stack.push((um, b, c, d, cuts + 1));
            stack.push((a, um, c, d, cuts + 1));
        } else if cut_v {
            stack.push((a, b, vm, d, cuts + 1));
            stack.push((a, b, c, vm, cuts + 1));
        } else {
            return Ok(false);
        }
    }
    Ok(true)
}

impl Drop for Unclamped<'_> {
    fn drop(&mut self) {
        if let Some((base, was_oob, was_cont)) = self.base {
            base.set_out_of_bounds_enabled_in_place(was_oob);
            base.set_continues_past_domain_in_place(was_cont);
        }
    }
}

/// The uv line from `a` to `z` as a degree 1 B-spline on `[0, 1]`: a gap may be as short as a
/// rounding, which a line's unit direction cannot take (`Line::create_line_segment` refuses
/// coincident ends).
fn gap_segment(a: Point3, z: Point3) -> Result<Curve> {
    use crate::nurbs::b_spline_curve::p1::{
        BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
    };
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: 1,
        control_points: &[a, z],
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .map(Curve::from)
}

/// Closes a trim boundary's loops where it leaves them open, in place: after a curve whose
/// end is apart from the next one's start, round each loop, the uv line between them, where
/// the two are within the gap test of `create_oriented_trim_boundary`
/// (`|d|^2 <= EFF_ZERO (1 + |p|)`, `p` the next start, a squared length against a length),
/// which fills every gap past it with such a line itself. The loop counts take the lines in.
fn close_loops(
    k: &Kernel,
    trims: &mut crate::topology::face::p1::OrientedTrimBoundary,
) -> Result<()> {
    let mut curves = Vec::with_capacity(trims.curves.len());
    let mut loops = Vec::with_capacity(trims.loops.len());
    let mut start = 0;
    for &n in &trims.loops {
        let lp = trims
            .curves
            .get(start..start + n)
            .ok_or_else(|| SolidError::mass_failed(OP, "the trim boundary's loop counts"))?;
        let mut count = n;
        for (i, c) in lp.iter().enumerate() {
            curves.push(c.clone());
            let next = lp
                .get((i + 1) % n)
                .ok_or_else(|| SolidError::mass_failed(OP, "the trim boundary's loop counts"))?;
            let end = c.evaluate_point(k, c.natural_interval().max())?;
            let first = next.evaluate_point(k, next.natural_interval().min())?;
            let d = first - end;
            if end != first && d.length_squared() <= EFF_ZERO * (1.0 + first.max_dimension()) {
                curves.push(gap_segment(end, first)?);
                count += 1;
            }
        }
        loops.push(count);
        start += n;
    }
    trims.curves = curves;
    trims.loops = loops;
    Ok(())
}

/// The strips' error scales: the faces' ([`boundary_properties`]) for the body's area
/// estimate, about the body's box.
fn closure_scale(
    k: &Kernel,
    bbox: &Extent3d,
    origin: Point3,
    estimated: f64,
) -> [f64; STRIP_COMPONENTS] {
    // the volume moment groups' extra length dimensions, as the faces'
    const POWERS: [i32; 4] = [2, 3, 3, 3];
    let area_scale = estimated / 10.0;
    let origin_distance = bbox.min_distance(&Extent3d::from_point(origin));
    let length = if k.fixes.precise_properties_body_length_scale {
        (10.0 * estimated.sqrt()).max(origin_distance)
    } else {
        estimated.sqrt().max(origin_distance / 10.0)
    };
    let mut scale = [area_scale * length; STRIP_COMPONENTS];
    for (group, power) in POWERS.iter().enumerate() {
        for c in 0..3 {
            if let Some(s) = scale.get_mut(1 + 3 * group + c) {
                *s = area_scale * length.powi(*power);
            }
        }
    }
    // the strips' area along z: an area
    if let Some(s) = scale.last_mut() {
        *s = area_scale;
    }
    scale
}

/// A strip side's point relative to `origin` and its derivative along the edge
/// parameter, at `t` on the curve's polynomial piece `span`.
fn strip_side_at(
    cn: &Nurbs,
    pc: &NurbsCurve,
    span: usize,
    t: f64,
    origin: Point3,
) -> Result<(Vec3, Vec3)> {
    let d = curve_derivs_over(pc, Ivl::from(t), 1, Some(span))?;
    let (p, dp) = (
        d.first()
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?,
        d.get(1)
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no derivative"))?,
    );
    let s = crate::mass::enclosure::partials_at(cn, p.x, p.y, 1)?;
    let mid = |v: V3<Ivl>| Vec3::new(v.x.mid(), v.y.mid(), v.z.mid());
    let (x, su, sv) = (mid(s.at(0, 0)), mid(s.at(1, 0)), mid(s.at(0, 1)));
    Ok((x - origin, su * dp.x.mid() + sv * dp.y.mid()))
}

/// The flux of the volume components' fields out of a strip `X_1 + s (X_2 - X_1)`:
/// exact in `s` (3-point Gauss-Legendre: the integrand is a polynomial of degree at most 4
/// in `s`), adaptive in `t` piece by piece between both curves' knots, as the faces.
fn strip_flux(
    k: &Kernel,
    closure: &Closure,
    strip: &ClosureStrip,
    origin: Point3,
    accuracy: f64,
    scale: &[f64; STRIP_COMPONENTS],
) -> Result<[f64; STRIP_COMPONENTS]> {
    let (fa, ca) = closure.side(strip, 0)?;
    let (fc, cc) = closure.side(strip, 1)?;
    let (t0, t1) = strip.t;
    let mut cuts = vec![t0, t1];
    for cn in [ca, cc] {
        for &x in &cn.u {
            if x > t0 && x < t1 {
                cuts.push(x);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    // Gauss-Legendre on [0, 1]
    let r = 0.5 * (0.6_f64).sqrt();
    let gauss = [
        (0.5 - r, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.5 + r, 5.0 / 18.0),
    ];
    let mut total = [0.0; STRIP_COMPONENTS];
    for pair in cuts.windows(2) {
        let &[a, z] = pair else {
            continue;
        };
        let tm = 0.5 * (a + z);
        let (sa, sc) = (span(&ca.u, ca.p, ca.n, tm), span(&cc.u, cc.p, cc.n, tm));
        let mut status = Err(SolidError::mass_failed(
            OP,
            "the strip piece was not integrated",
        ));
        let mut y = [0.0; STRIP_COMPONENTS];
        for divisor in [2.0, 8.0, 64.0, 512.0, 4096.0] {
            k.check_cancelled(OP)?;
            // (a failed integration leaves `y` as it was: zero)
            status = OdeIntegrator::default().integrate(
                &mut y,
                a,
                z,
                OdeOptions {
                    accuracy: accuracy / 10.0,
                    initial_step: (z - a) / divisor,
                    minimum_step: 0.0,
                    scale: Some(scale.as_slice()),
                    quadrature: true,
                    rounding: None,
                    rate: None,
                },
                |t, _, dy| {
                    let (x1, d1) = strip_side_at(&fa.cn, ca, sa, t, origin)?;
                    let (x2, d2) = strip_side_at(&fc.cn, cc, sc, t, origin)?;
                    let (dd, ddt) = (x2 - x1, d2 - d1);
                    dy.fill(0.0);
                    for (s, w) in gauss {
                        let p = x1 + dd * s;
                        let nz = (d1 + ddt * s).cross(dd).z;
                        let fields = volume_fields(p).into_iter().chain([1.0]);
                        for (v, f) in dy.iter_mut().zip(fields) {
                            *v += STRIP_SENSE * w * f * nz;
                        }
                    }
                    Ok(())
                },
            );
            if status.is_ok()
                || status
                    .as_ref()
                    .is_err_and(SolidError::is_unported_or_cancelled)
            {
                break;
            }
        }
        status?;
        for (t, v) in total.iter_mut().zip(y) {
            *t += v;
        }
    }
    Ok(total)
}

/// `SmApiEdgeComputeLength` (the curve's virtual Length). The edge's curve is owned by
/// the edge, so its zone tolerance (`SmTol::GetZoneTol3d(pCurve)`) is the edge's.
pub fn edge_length(k: &Kernel, b: &Brep, e: EdgeId, accuracy: f64) -> Result<f64> {
    let c =
        edge::curve(b, e)?.ok_or_else(|| SolidError::mass_failed(OP, "the edge has no curve"))?;
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
        origin
            + *p.moments
                .first()
                .ok_or_else(|| SolidError::mass_failed(OP, "no first moments"))?
                / p.area,
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
    /// partitions between two material regions, which are no part of it (WR-535;
    /// upstream's boundary counts a partition once from each side).
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
/// for a span the integrator took no step over; [`SolidError::UnsureRegion`]
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
    estimate(k, b, accuracy, request, attributes).map_err(typed_failure)
}

/// The integration's failures as their own errors (upstream's `SM_ERR` for each): its step
/// control failing is [`SolidError::NotConverged`]; a non-finite integrand (a surface
/// evaluated to no number) is [`SolidError::Degenerate`]; a span it took no step over is
/// [`SolidError::Unsupported`]. Every other error is as it came.
fn typed_failure(e: SolidError) -> SolidError {
    use super::OP_INTEGRAND;
    match e.mass_failure() {
        Some(MassFailure::StepControl) => SolidError::NotConverged {
            operation: OP,
            iterations: 0,
        },
        Some(MassFailure::Integrand) => SolidError::Degenerate {
            operation: OP_INTEGRAND,
            entity: e.entity(),
        },
        Some(MassFailure::SkippedSpan) => SolidError::Unsupported {
            operation: OP,
            reason: "the integrator took no step over a span of a face",
        },
        _ => e,
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
            .ok_or_else(|| SolidError::mass_failed(OP, "a moment group is missing"))
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
/// (WR-535). A side with no faceuse, no shell or no
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
            sum += *p
                .moments
                .first()
                .ok_or_else(|| SolidError::mass_failed(OP, "no first moments"))?
                * sign;
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
    /// #14170: the out-of-bounds flag set for a face's integration is restored on every
    /// path out of it, an error included: a cube's top cut short of its trims turns it on,
    /// and an integration that fails while it is on leaves it off.
    #[test]
    fn the_unclamped_flag_is_restored_when_the_integration_fails() {
        use crate::geom::extent2d::Extent2d;
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0)
                .unwrap();
        let top = faces(&b)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let face = b.face(f).unwrap();
                face.surface
                    .as_ref()
                    .unwrap()
                    .evaluate_point(&k, face.uv_domain.mid())
                    .is_ok_and(|p| (p.z - 10.0).abs() < 1e-9)
            })
            .unwrap();
        let mut trims = boundary(&k, &mut b, top).unwrap();
        close_loops(&k, &mut trims).unwrap();
        let surface = b.face_mut(top).unwrap().surface.as_mut().unwrap();
        let SurfaceKind::Plane(plane) = surface.kind_mut() else {
            panic!("the top is not a plane")
        };
        let d = plane.step_uv_domain();
        let short = Extent2d::from_bounds(d.u_min(), d.v_min(), d.u_max() - 1.0, d.v_max());
        plane.adjust_step_uv_domain(&short).unwrap();
        let surface = b.face(top).unwrap().surface.as_ref().unwrap();
        let flag = || match surface.kind() {
            SurfaceKind::Plane(p) => p.base().is_out_of_bounds_enabled(),
            _ => panic!("the top is not a plane"),
        };
        let failing = || -> Result<()> {
            let (_guard, refusal) = Unclamped::where_trims_may_leave(&k, surface, &trims.curves)?;
            assert!(refusal.is_none());
            assert!(flag(), "on for the integration");
            Err(SolidError::mass(OP, MassFailure::Integrand))
        };
        assert!(failing().is_err());
        assert!(!flag(), "restored after the error");
    }
    /// Outside the mass integration's scope the face integration's B-spline evaluation
    /// clamps to the natural domain whatever the out-of-bounds flag, as upstream's
    /// `EvaluateSimple` does (and `evaluate_simple`); within it, it continues.
    #[test]
    fn the_integration_s_evaluation_continues_only_within_its_scope() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        let corners = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.5),
            Point3::new(1.0, 0.0, -0.5),
            Point3::new(1.0, 1.0, 2.0),
        ];
        let mut s = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 1,
            control_points: &corners,
            form: BSplineSurfaceForm::default(),
            u_multiplicities: &[2, 2],
            v_multiplicities: &[2, 2],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            uv_domain: None,
        })
        .unwrap();
        *s.out_of_bounds_enabled_mut() = true;
        let uv = crate::geom::vector2d::Point2::new(1.25, 0.5);
        let clamped = s.evaluate_simple(uv, 1, 1, true, true, true).unwrap();
        let mut d = [[Vec3::ZERO; 2]; 2];
        s.evaluate_simple_first_into(uv, true, true, true, &mut d)
            .unwrap();
        for (row, full) in d.iter().zip(&clamped) {
            for (x, y) in row.iter().zip(full) {
                assert_eq!(x, y, "clamped as evaluate_simple, the flag on");
            }
        }
        s.set_continues_past_domain_in_place(true);
        s.evaluate_simple_first_into(uv, true, true, true, &mut d)
            .unwrap();
        // the bilinear map continued: x = u
        assert!((d[0][0].x - 1.25).abs() < 1e-12, "continued: {:?}", d[0][0]);
    }
    /// A cylinder's and a cone's NURBS forms (rational, their circles' interior knots
    /// doubled) certify their continuation over a box past their domain by rounding, and
    /// their faces, with the trims they are integrated with, are not refused: the certificate
    /// takes the spans the integration evaluates, not the empty spans of repeated knots
    /// (walking those refused every circle-based face, `WeightsNotProved`).
    #[test]
    fn circle_based_faces_certify_their_continuation_past_their_domain_by_rounding() {
        use crate::blend::fillet_section::station::knot;
        use crate::primitive::api_primitives::{create_cone, create_cylinder};
        let mut k = Kernel::default();
        let bodies = [
            create_cylinder(&mut k, Point3::new(3.0, -2.0, 1.0), 8.0, 20.0).unwrap(),
            create_cone(&mut k, Point3::new(3.0, -2.0, 1.0), 8.0, 3.0, 20.0).unwrap(),
        ];
        for mut b in bodies {
            let mut seen = 0;
            for f in faces(&b).unwrap() {
                let trims = boundary(&k, &mut b, f).unwrap();
                let surface = b.face(f).unwrap().surface.as_ref().unwrap();
                if !matches!(surface.kind(), SurfaceKind::Cone(_)) {
                    continue;
                }
                seen += 1;
                let base = surface.analytic_base_with_nurb(&k).unwrap();
                let cn = Nurbs::of(base).unwrap();
                assert!(
                    cn.pw.chunks(4).any(|p| p[3] != 1.0),
                    "the NURBS form is rational"
                );
                let doubled = |u: &[f64], p: usize, n: usize| {
                    (p + 1..n).any(|i| knot(u, i as isize) == knot(u, i as isize + 1))
                };
                assert!(
                    doubled(&cn.u, cn.p, cn.nu) || doubled(&cn.v, cn.q, cn.nv),
                    "a repeated interior knot"
                );
                let d = base.natural_uv_domain();
                let out = |x: f64| 4.0 * f64::EPSILON * (1.0 + x.abs());
                let hull = (
                    d.u_min() - out(d.u_min()),
                    d.u_max() + out(d.u_max()),
                    d.v_min() - out(d.v_min()),
                    d.v_max() + out(d.v_max()),
                );
                assert!(
                    continuation_certified(base, hull).unwrap(),
                    "the continuation past {hull:?} is certified"
                );
                let (_guard, refusal) =
                    Unclamped::where_trims_may_leave(&k, surface, &trims.curves).unwrap();
                assert_eq!(refusal, None, "face {f:?} is not refused");
            }
            assert!(seen > 0, "a circle-based face");
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
            typed_failure(SolidError::mass(OP_STEP_CONTROL, MassFailure::StepControl)),
            SolidError::NotConverged { .. }
        ));
        assert!(matches!(
            typed_failure(SolidError::mass(OP_INTEGRAND, MassFailure::Integrand)),
            SolidError::Degenerate { .. }
        ));
        assert!(matches!(
            typed_failure(SolidError::mass(OP_SKIPPED_SPAN, MassFailure::SkippedSpan)),
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

    /// Upstream's boundary adds a partition's area from each side, so an area counting every
    /// face once would count it twice; with WR-535 the partitions are no part of the area (800).
    #[test]
    fn a_partition_is_no_part_of_the_area() {
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
        let r = estimated(&k, &mut merged, &MassPropertyRequest::centroidal()).unwrap();
        assert!(near(r.area, 800.0, 1e-9), "{r:?}");
        assert!(near(r.volume, 1500.0, 1e-9), "{r:?}");
    }
}
