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
//! | [`brep_mass_properties_estimated_prepared`], [`brep_area_about_prepared`], [`face_area_prepared`], [`face_centroid_prepared`], [`missing_uv_trim`] | not upstream: the same computations on a B-rep read only, whose UV trim curves exist |
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
//! Every face's trim pieces, banded or not, are integrated the same way (#14460): the
//! outer rule runs along the trim piece and the inner one along each sample's iso-u line,
//! each level with its own bounds of 48 levels and 100,000 pieces (so the work is bounded
//! by their product), cancel polled in both, and no recursion. Neither level has a node
//! at a piece's end, so the WR-670 one-sided end evaluation (#2091) is not needed on this
//! path; every trim piece also ends at each crossing of its trim with a surface v knot
//! line, banded or not, as the outer integrand has a kink there (crossings found by sign
//! change between samples of each trim span, as `trim_curve_breaks` finds them).
//! An edge's tube area, volume and moments each have an error scale of their own size
//! (#2887): upstream's are `BIG_DOUBLE`, so only the edge's length was under error control.
//! Not upstream (#16735): `brep_properties` closes every volume-bounding face onto its
//! edges' curves and vertices (`gap_closure`: ruled strips from the trims' images to the
//! edge curves, and fans at the loop corners), so the faces bound one closed surface and
//! the volume and volume moments do not depend on the origin; upstream integrates each
//! face over its own trims, and the tolerant edges' gaps leak to first order in the
//! tolerance.
//! [`brep_mass_properties`] and [`brep_mass_properties_estimated`] (through
//! [`brep_properties_in_box`]) integrate about the middle of the body's exact box and
//! scale their errors by that box's size (#17361): the loose box upstream takes holds the
//! control points of every span of an edge's curve that the edge touches, so a short arc
//! of a circle of large radius puts it a quarter of that circle away, and the body's
//! first moments became large terms that cancel, under an error scale as large.
//! Not upstream: the `_prepared` forms read the B-rep only. They share the integration
//! with the forms that take it mutably (the trims are reached through a
//! [`QueryTopology`]: mutable, the missing ones are made as upstream; prepared, a missing
//! one is [`SolidError::PreparationRequired`] and nothing is made), so where every UV trim
//! curve exists the two give the same result, bit for bit. The surface continuation of
//! `Unclamped` is still toggled on the surface's own flags for the integration, through a
//! shared reference, and restored.
use super::{OdeIntegrator, OdeOptions};
use crate::curve::curve::Curve;
use crate::error::mass::MassFailure;
use crate::error::{Failure, SM_ERR, SM_ERR_NULL_POINTER};
use crate::geom::attribute::{ids, AttributeStore, AttributeValue, ObjectKey};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::tol::are_same;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, PI};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions, LazyBrep};
use crate::surface::surface::Surface;
use crate::topology::face::p2::{
    surface_knots_inside, trim_curve_breaks, OdeIsoCurveLength, PrecisePropertyLayout, PPF_ALL,
    PPF_AREA, PPF_AREA_FIRST_MOMENTS,
};
use crate::topology::loopuse::QueryTopology;
use crate::topology::{
    edge, vertex, Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, RegionId, VertexId,
};
use crate::{EntityRef, Result, SolidError};

const OP: &str = "mass::integrator::properties";

/// A defect ([`MassFailure::Defect`]) with the status `SM_ERR` of the `SE(SM_ERR)` it
/// replaces.
#[track_caller]
fn defect(why: &'static str) -> SolidError {
    Failure::new(OP, MassFailure::Defect { why }).into_error()
}

/// A defect ([`MassFailure::Defect`]) with the status `SM_ERR_NULL_POINTER` of the `NER` it
/// replaces.
#[track_caller]
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, MassFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// Why an evaluation returned fewer values than asked for.
const SHORT_EVALUATION: &str = "the curve's evaluation returned fewer values than asked for, \
                                though an evaluation that succeeds returns the point and each \
                                derivative asked for";
/// Why the closed boundary and the face's loops disagree.
const BOUNDARY_LOOPS: &str = "the face's closed boundary and its loops disagree, though the \
                              boundary is built from those loops, one edge use at a time";
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
        edge::curve(b, e)?.ok_or_else(|| SolidError::mass(OP, MassFailure::EdgeWithoutCurve))?;
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
        .ok_or_else(|| null_defect(SHORT_EVALUATION))?;
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
fn boundary(k: &Kernel, b: &mut QueryTopology<'_>, f: FaceId) -> Result<Vec<Curve>> {
    oriented_boundary(k, b, f).map(|t| t.curves)
}

/// [`boundary`] with the loops' curve counts and each curve's edge use (`None` for a UV
/// line filling a gap), for [`gap_closure`] (#16735).
fn oriented_boundary(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    f: FaceId,
) -> Result<crate::topology::face::p1::OrientedTrimBoundary> {
    let key_of = |e: EntityRef| crate::geom::object::ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    crate::topology::face::p1::oriented_trim_boundary_in(k, b, f, true, true, None, &key_of)
}

/// [`oriented_boundary`] with its loops closed ([`close_loops`]): the boundary
/// [`brep_properties`] integrates a face it closes over, and its closure starts from
/// (#16735).
fn closed_boundary(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    f: FaceId,
) -> Result<crate::topology::face::p1::OrientedTrimBoundary> {
    let mut trims = oriented_boundary(k, b, f)?;
    close_loops(k, &mut trims)?;
    Ok(trims)
}

/// Not upstream (#16735; adapted from #15946 by @bherbruck): the uv line from `a` to `z` as
/// a degree 1 B-spline on `[0, 1]`: a gap may be as short as a rounding, which a line's unit
/// direction cannot take (`Line::create_line_segment` refuses coincident ends).
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

/// Not upstream (#16735; adapted from #15946 by @bherbruck): closes a trim boundary's loops
/// where `create_oriented_trim_boundary` leaves them open, in place. It fills a gap between a
/// curve's end and the next one's start with a uv line only past `|d|^2 > EFF_ZERO (1 + |p|)`
/// (a squared length against a length: gaps up to about `1e-6` stay open). An open loop
/// loses its gap's shadow down to the integration's base, an error of first order that
/// depends on the origin. Each gap within the test gets the uv line too; the loop counts
/// and the edge-use records (`None`, a gap line) take the lines in.
fn close_loops(
    k: &Kernel,
    trims: &mut crate::topology::face::p1::OrientedTrimBoundary,
) -> Result<()> {
    let recorded = trims.edgeuses.len() == trims.curves.len();
    let mut curves = Vec::new();
    let mut edgeuses = Vec::new();
    let mut loops = Vec::new();
    let mut start = 0;
    for &n in &trims.loops {
        let lp = trims.curves.get(start..start + n).ok_or_else(|| {
            defect(
                "the oriented trim boundary's loop counts add up to more curves than it \
                     holds, though it counts each loop as it adds the loop's curves",
            )
        })?;
        let mut count = n;
        for (i, c) in lp.iter().enumerate() {
            curves.push(c.clone());
            if recorded {
                edgeuses.push(trims.edgeuses.get(start + i).copied().flatten());
            }
            // (`lp` holds `c`, so it has a first curve)
            let next = lp.get(i + 1).or_else(|| lp.first()).unwrap_or(c);
            let end = c.evaluate_point(k, c.natural_interval().max())?;
            let first = next.evaluate_point(k, next.natural_interval().min())?;
            let d = first - end;
            if end != first && d.length_squared() <= EFF_ZERO * (1.0 + first.max_dimension()) {
                curves.push(gap_segment(end, first)?);
                if recorded {
                    edgeuses.push(None);
                }
                count += 1;
            }
        }
        loops.push(count);
        start += n;
    }
    trims.curves = curves;
    trims.edgeuses = if recorded { edgeuses } else { Vec::new() };
    trims.loops = loops;
    Ok(())
}

/// Not upstream (#16735; adapted from #15946 by @bherbruck): where a trim of `trims` may
/// leave the parameter box `[u0, u1] x [v0, v1]`, where its surface's evaluation clamps it:
/// `None` where every curve's control points are proved inside (their homogeneous
/// coordinates over their weights in rounded interval arithmetic), else the uv box of all
/// the curves' control points, `(u0, u1, v0, v1)`. A NURB curve with positive weights lies
/// in the convex hull of its control points, and a box is convex, so the curve lies in the
/// box when they do; the loops' closing uv lines are among the curves ([`close_loops`]). A
/// weight not proved positive, or a curve with no B-spline form, is taken as leaving, over
/// the whole parameter plane.
fn trims_past(
    trims: &[Curve],
    u: (f64, f64),
    v: (f64, f64),
) -> Result<Option<(f64, f64, f64, f64)>> {
    use crate::blend::fillet_section::certificate::NurbsCurve;
    use crate::blend::fillet_section::interval::Ivl;
    let everywhere = (
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
    );
    let mut hull = (u.0, u.1, v.0, v.1);
    let mut leaves = false;
    for c in trims {
        let Some(bs) = c.b_spline_base() else {
            return Ok(Some(everywhere));
        };
        let cn = NurbsCurve::of(bs)?;
        for p in cn.pw.chunks(4) {
            let &[xw, yw, _, w] = p else {
                return Ok(Some(everywhere));
            };
            if w.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
                return Ok(Some(everywhere));
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

/// Not upstream (#16735; adapted from #15946 by @bherbruck): a face's surface continued
/// past the domain its evaluation clamps to, for the face's integration and its closure
/// strips, where its trims may leave that domain and the continuation is certified;
/// restored when dropped, so on every path out (as upstream's `SmTemporaryChangeValue` on
/// `GetOutOfBoundsEnabled()`).
///
/// Clamped, a trim past the domain has the clamped point as its image but the domain
/// edge's partials as its derivative: the face's Green integral and the strips along that
/// trim then integrate different curves, and the closed surface leaks a first-order,
/// origin-dependent flux (the `stubs4-degenerate-brepForFillet2-e9-r0.5` fillet's wall, its
/// trim 3.3e-6 past `u = 1`). Continued, the trim's image is the continued surface's, the
/// one both integrate.
///
/// The flags toggled are the B-spline part's the evaluation reads: a plane's base (whose
/// `evaluate` clamps to its analytic domain unless out-of-bounds evaluation is on), a
/// B-spline, an analytic kind's NURB, a ball patch's or answering blend's net. Other kinds
/// are left clamped. The domain tested is the one the evaluation clamps to: a plane's
/// analytic domain, else the NURB's natural (knot) domain.
struct Unclamped<'a> {
    /// The surface toggled, with the two flags' values before.
    base: Option<(
        &'a crate::nurbs::b_spline_surface::BSplineSurface,
        bool,
        bool,
    )>,
}

impl<'a> Unclamped<'a> {
    /// Turns `surface`'s continuation on where `trims` may leave its domain and its
    /// continuation over the trims' box is certified: a plane, a non-rational NURB, or a
    /// rational one whose weights are proved positive over that box
    /// ([`continuation_certified`]). Otherwise nothing is toggled: the face stays clamped,
    /// as upstream integrates it.
    ///
    /// Both flags are captured and restored on their own: an out-of-bounds flag already on
    /// does not continue the face integration's evaluation, which clamps unless
    /// `continues_past_domain` is on too.
    ///
    /// Known limit: a face left clamped (a rational surface whose weights are not proved
    /// positive over the trims' box, a polynomial-form B-spline, or a kind with no
    /// B-spline part) whose trims leave its domain keeps the clamped integration's first-order,
    /// origin-dependent error, as on upstream: its trims' clamped images and the domain
    /// edge's partials describe different curves, and the strips built on the same images
    /// do not remove it. No bound for it is computed or reported.
    fn where_trims_may_leave(k: &Kernel, surface: &'a Surface, trims: &[Curve]) -> Result<Self> {
        use crate::surface::surface::SurfaceKind;
        let none = Self { base: None };
        let (base, domain) = match surface.kind() {
            SurfaceKind::Plane(plane) => (plane.base(), Some(plane.step_uv_domain())),
            SurfaceKind::BSpline(s) => (s, None),
            SurfaceKind::BallPatch(s) => (s.net(), None),
            SurfaceKind::ProceduralBlend(s) if s.net_answers(k) => (s.net(), None),
            SurfaceKind::Cone(_)
            | SurfaceKind::SurfOfRevolution(_)
            | SurfaceKind::SurfOfExtrusion(_)
            | SurfaceKind::Sphere(_)
            | SurfaceKind::Torus(_) => (surface.analytic_base_with_nurb(k)?, None),
            _ => return Ok(none),
        };
        let domain = domain.unwrap_or_else(|| base.natural_uv_domain());
        let (u, v) = (
            (domain.u_min(), domain.u_max()),
            (domain.v_min(), domain.v_max()),
        );
        let Some(hull) = trims_past(trims, u, v)? else {
            return Ok(none);
        };
        // a plane is a bilinear, non-rational patch: its continuation is itself
        if !matches!(surface.kind(), SurfaceKind::Plane(_))
            && !continuation_certified(k, base, hull)?
        {
            return Ok(none);
        }
        let was = (
            base.is_out_of_bounds_enabled(),
            base.continues_past_domain(),
        );
        base.set_out_of_bounds_enabled_in_place(true);
        base.set_continues_past_domain_in_place(true);
        Ok(Self {
            base: Some((base, was.0, was.1)),
        })
    }
}

impl Drop for Unclamped<'_> {
    fn drop(&mut self) {
        if let Some((base, was_oob, was_cont)) = self.base {
            base.set_out_of_bounds_enabled_in_place(was_oob);
            base.set_continues_past_domain_in_place(was_cont);
        }
    }
}

/// The times a box is cut in halves along any path while [`continuation_certified`] proves
/// its weights, as [`crate::mass::enclosure`] cuts a trim's (2^16 parts).
const CERTIFY_CUTS: u32 = 16;

/// Not upstream (#16735; adapted from #15946 by @bherbruck): whether `base`'s continuation
/// over the uv box `hull` (`(u0, u1, v0, v1)`) is certified: non-rational (every weight 1),
/// or its weights proved positive over the box as the enclosure proves them
/// (`mass::enclosure::partials_over`), the box cut in halves where that is too coarse,
/// along the side longer against the domain's, at most [`CERTIFY_CUTS`] times along any
/// path. Cancellation is polled once per box.
///
/// # Errors
///
/// Those of the enclosure, and [`SolidError::Cancelled`] when `k` is cancelled.
fn continuation_certified(
    k: &Kernel,
    base: &crate::nurbs::b_spline_surface::BSplineSurface,
    hull: (f64, f64, f64, f64),
) -> Result<bool> {
    use crate::blend::fillet_section::interval::Ivl;
    use crate::blend::fillet_section::station::Nurbs;
    use crate::mass::enclosure::partials_over;
    let cn = Nurbs::of(base)?;
    if cn.pw.chunks(4).all(|p| p.get(3) == Some(&1.0)) {
        return Ok(true);
    }
    let (u0, u1, v0, v1) = hull;
    if !(u0.is_finite() && u1.is_finite() && v0.is_finite() && v1.is_finite()) {
        return Ok(false);
    }
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
        k.check_cancelled(OP)?;
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
/// `SmFace::ComputePreciseProperties`; the default property mask is `PPF_ALL`. Missing UV
/// trim curves of the face are made on `b`, after the arguments are checked: a non-finite
/// `accuracy` or an invalid mask is [`SolidError::InvalidInput`], as in
/// [`boundary_properties`].
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
    face_properties_in(
        k,
        &mut QueryTopology::Mutable(b),
        f,
        orientation,
        accuracy,
        origin,
        estimated_area,
        thickness,
        flags,
        false,
    )
}

/// [`face_properties`], or with `closed` as [`brep_properties`] integrates the faces it
/// closes (#16735): over the loops closed ([`closed_boundary`]), on the surface continued
/// past the domain its evaluation clamps to where its trims leave it and the continuation
/// is certified ([`Unclamped`]).
#[allow(clippy::too_many_arguments)] // face_properties' inputs and the scope
fn face_properties_in(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    f: FaceId,
    orientation: OrientType,
    accuracy: f64,
    origin: Point3,
    estimated_area: f64,
    thickness: f64,
    flags: u64,
    closed: bool,
) -> Result<PreciseProperties> {
    valid_flags(flags)?;
    valid_accuracy(accuracy)?;
    let trims = if closed {
        closed_boundary(k, b, f)?.curves
    } else {
        boundary(k, b, f)?
    };
    let fbox = face_box(k, b, f)?;
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceWithoutSurface))?;
    // set for the integration alone (after the face's box, which may fill the surface's
    // caches), and restored when the guard drops, on every path
    let _unclamped = if closed {
        Some(Unclamped::where_trims_may_leave(k, surface, &trims)?)
    } else {
        None
    };
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
            .ok_or_else(|| {
                defect(
                    "the trim's bounding box came back without the axis-aligned box, though \
                     that box is computed whenever it is asked for",
                )
            })?;
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
/// The surface lines of one face's iso-u integrals and how tightly each is integrated:
/// the inner level of the trim pieces' integral.
struct IsoLine<'a> {
    surface: &'a Surface,
    origin: Point3,
    thickness: f64,
    layout: &'a PrecisePropertyLayout,
    /// the inner integrals' accuracy (`SmODEAreaIFEO::m_dIsoCurveAccuracy`)
    accuracy: f64,
    /// the components' error scales
    scale: &'a [f64],
}

/// The trim pieces' integrand at `t` on `trim` into `dy` (`SmODEAreaIFEO::Evaluate`):
/// the iso-u integral at the trim point `(u, v)` from `v = base` to `v`, times the trim's
/// `-du/dt`. Zero, without the iso-u integral, where `du/dt` is exactly zero or the trim
/// point is on its base (`v = base`: a zero-width iso-u integral). The iso-u
/// integral is taken by [`kronrod_integrate`] piece by piece between the surface's v knots
/// between `base` and `v` (WR-494), at `iso.accuracy`; one running down (a trim point
/// below its base) is the negated integral up. `scratch` holds the iso integrand's
/// values.
///
/// Not upstream: upstream integrates both levels by Runge-Kutta (`SmODEAreaIFEO`'s inner
/// `SmRungeKuttaODEIntegrate`), and leaves `dy` as it was where `|du/dt| <= SM_EFF_ZERO`
/// (see [`OdeArea::evaluate`](crate::topology::face::p2::OdeArea::evaluate)).
///
/// # Errors
///
/// Those of the trim's evaluation (and a non-finite trim point or tangent), of
/// [`surface_knots_inside`] and of the inner integration.
fn trim_integrand(
    k: &Kernel,
    iso: &IsoLine<'_>,
    trim: &Curve,
    base: f64,
    t: f64,
    scratch: &mut [f64],
    dy: &mut [f64],
) -> Result<()> {
    let pv = trim.evaluate(k, t, 1, true, true)?;
    let (Some(p0), Some(p1)) = (pv.first().copied(), pv.get(1).copied()) else {
        return Err(defect(SHORT_EVALUATION));
    };
    // a non-finite trim point would bisect to the depth bound and fail as step control
    if !(p0.x.is_finite() && p0.y.is_finite() && p1.x.is_finite()) {
        return Err(SolidError::mass(OP, MassFailure::TrimNotFinite));
    }
    dy.fill(0.0);
    if p1.x == 0.0 {
        return Ok(());
    }
    let (lo, hi, sign) = if p0.y >= base {
        (base, p0.y, 1.0)
    } else {
        (p0.y, base, -1.0)
    };
    // (a trim point on its base: a zero-width iso-u integral, nothing to evaluate)
    if lo == hi {
        return Ok(());
    }
    let eval = OdeIsoCurveLength::new(k, iso.surface, p0.x, iso.origin, iso.thickness, iso.layout);
    let mut v0 = lo;
    // the 15-point rule: an iso-u line up a smooth surface that is no polynomial (a sphere's
    // rational form, a cylinder's quarter turn) takes the 7-point rule's pieces about as
    // often as upstream's Runge-Kutta steps; the 15-point rule, exact to degree 22, a fifth
    // of the evaluations on a sphere at the app's accuracy
    for v1 in surface_knots_inside(k, iso.surface, SurfParamType::V, lo, hi)?
        .into_iter()
        .chain(core::iter::once(hi))
    {
        kronrod15_integrate(k, v0, v1, iso.accuracy, iso.scale, dy, |v, g| {
            eval.evaluate(v, scratch)?;
            for (d, x) in g.iter_mut().zip(scratch.iter().skip(1)) {
                *d = *x;
            }
            Ok(())
        })?;
        v0 = v1;
    }
    for d in dy.iter_mut() {
        // (* -1.0: an exact negation)
        *d = -(*d * p1.x) * sign;
    }
    Ok(())
}
/// The 7-point Gauss-Kronrod rule on `[-1, 1]` and its embedded 3-point Gauss rule: the
/// nodes `0, ±x1, ±x2, ±x3` (`x2 = sqrt(3/5)`, the Gauss nodes with `0`), the Kronrod
/// weights of each, and the Gauss weights (zero where the node is not a Gauss node).
/// The Kronrod rule integrates polynomials of degree 10 exactly, the Gauss rule degree 5.
/// [`kronrod_integrate`] reads only the off-centre weights: the centre's are what makes
/// each rule's weights sum to 2.
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
/// The 15-point Gauss-Kronrod rule on `[-1, 1]` and its embedded 7-point Gauss rule, laid
/// out as [`KRONROD_NODES`] is: the nodes `0, ±x1, ..., ±x7` (the Gauss nodes `0, ±x2, ±x4,
/// ±x6`), their Kronrod weights, and the Gauss weights (zero off the Gauss nodes). The
/// Kronrod rule integrates polynomials of degree 22 exactly, the Gauss rule degree 13.
const KRONROD15_NODES: [f64; 8] = [
    0.0,
    0.207_784_955_007_898_467_600_689_403_773_245,
    0.405_845_151_377_397_166_906_606_412_076_961,
    0.586_087_235_467_691_130_294_144_845_693_013,
    0.741_531_185_599_394_439_863_864_773_280_788,
    0.864_864_423_359_769_072_789_712_788_640_926,
    0.949_107_912_342_758_524_526_189_684_047_851,
    0.991_455_371_120_812_639_206_854_697_526_329,
];
const KRONROD15_WEIGHTS: [f64; 8] = [
    0.209_482_141_084_727_828_012_999_174_891_714,
    0.204_432_940_075_298_892_414_161_999_234_649,
    0.190_350_578_064_785_409_913_256_402_421_014,
    0.169_004_726_639_267_902_826_583_426_598_550,
    0.140_653_259_715_525_918_745_189_590_510_238,
    0.104_790_010_322_250_183_839_876_322_541_518,
    0.063_092_092_629_978_553_290_700_663_189_204,
    0.022_935_322_010_529_224_963_732_008_058_970,
];
const GAUSS7_WEIGHTS: [f64; 8] = [
    0.417_959_183_673_469_387_755_102_040_816_327,
    0.0,
    0.381_830_050_505_118_944_950_369_775_488_975,
    0.0,
    0.279_705_391_489_276_667_901_467_771_423_780,
    0.0,
    0.129_484_966_168_869_693_270_611_432_679_082,
    0.0,
];
/// A Gauss-Kronrod rule as [`kronrod_integrate_with`] takes it: its nodes on `[0, 1]` (the
/// first `0`, each other one used at `±x`), their Kronrod weights and their Gauss weights.
type KronrodRule = (&'static [f64], &'static [f64], &'static [f64]);
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
/// of the piece's value and the component's error `scale` (as upstream's Runge-Kutta steps
/// are held to their values or that scale); otherwise it is halved.
/// The pieces are summed left to right. No node is an end of the range, so an integrand
/// smooth inside `(a, b)` is never read on the far side of a knot at either end.
/// Each rule is summed about the centre node's value `c`, as `2 c + sum w_i (f_i - c)`
/// over the other nodes (each rule's weights sum to 2, so this is the same rule): the
/// weights' rounding (the double Kronrod weights sum to `2 + 2^-54`) then scales only the
/// integrand's variation about `c`, never its level, and a constant integrand gives
/// exactly the piece's width times it (#15021: summed weight by weight, a 100 x 100 x 2
/// box's volume came out 1 ulp over 20000).
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
    integrand: F,
) -> Result<()>
where
    F: FnMut(f64, &mut [f64]) -> Result<()>,
{
    kronrod_integrate_with(
        (&KRONROD_NODES, &KRONROD_WEIGHTS, &GAUSS_WEIGHTS),
        k,
        (a, b),
        accuracy,
        scale,
        y,
        integrand,
    )
}

/// [`kronrod_integrate`] by the 15-point rule ([`KRONROD15_NODES`]): on a smooth integrand
/// that needs many 7-point pieces (an iso line up a sphere's rational form, held to a
/// billionth), its degree-22 pieces are fewer.
///
/// # Errors
///
/// Those of [`kronrod_integrate`].
fn kronrod15_integrate<F>(
    k: &Kernel,
    a: f64,
    b: f64,
    accuracy: f64,
    scale: &[f64],
    y: &mut [f64],
    integrand: F,
) -> Result<()>
where
    F: FnMut(f64, &mut [f64]) -> Result<()>,
{
    kronrod_integrate_with(
        (&KRONROD15_NODES, &KRONROD15_WEIGHTS, &GAUSS7_WEIGHTS),
        k,
        (a, b),
        accuracy,
        scale,
        y,
        integrand,
    )
}

/// [`kronrod_integrate`] by `rule`.
fn kronrod_integrate_with<F>(
    (nodes, kronrod_weights, gauss_weights): KronrodRule,
    k: &Kernel,
    (a, b): (f64, f64),
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
    let mut centre = vec![0.0; n];
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
        // (the centre node first, as the weights' order has it)
        centre.fill(0.0);
        integrand(mid, &mut centre)?;
        if !centre.iter().all(|v| v.is_finite()) {
            return Err(SolidError::mass(OP_INTEGRAND, MassFailure::Integrand));
        }
        for (i, &x) in nodes.iter().enumerate().skip(1) {
            let wk = kronrod_weights.get(i).copied().unwrap_or(0.0);
            let wg = gauss_weights.get(i).copied().unwrap_or(0.0);
            for side in [-1.0, 1.0] {
                dy.fill(0.0);
                integrand(mid + side * x * half, &mut dy)?;
                if !dy.iter().all(|v| v.is_finite()) {
                    return Err(SolidError::mass(OP_INTEGRAND, MassFailure::Integrand));
                }
                for (((kv, gv), d), c) in kronrod
                    .iter_mut()
                    .zip(gauss.iter_mut())
                    .zip(&dy)
                    .zip(&centre)
                {
                    let off = *d - *c;
                    *kv += wk * off;
                    *gv += wg * off;
                }
            }
        }
        let mut accepted = true;
        for (i, ((kv, gv), c)) in kronrod
            .iter_mut()
            .zip(gauss.iter_mut())
            .zip(&centre)
            .enumerate()
        {
            // Each rule's weights sum to 2, so the centre value's share is exactly `2 c`:
            // a constant integrand is integrated exactly, with no rounding of the weights.
            *kv = half * (2.0 * *c + *kv);
            *gv = half * (2.0 * *c + *gv);
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
/// A NaN accuracy would pass every piece's error test (`estimate > NaN` is false) and an
/// infinite one would hold no piece to any: neither asks for an accuracy.
fn valid_accuracy(accuracy: f64) -> Result<()> {
    if accuracy.is_finite() {
        Ok(())
    } else {
        Err(SolidError::invalid_input(OP, "accuracy not finite"))
    }
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
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an invalid property mask or a non-finite `accuracy`
/// (a NaN one would accept every quadrature piece unchecked); otherwise those of the
/// trims' and the surface's evaluation and of the integration.
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
    valid_accuracy(accuracy)?;
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
        floors.extend(lines.iter().copied());
    }
    let banded = floors.len() > 1;
    // Every trim piece ends at each crossing, found by sign change between samples, of its
    // trim with a surface v knot line, banded or not:
    // across one the iso-u integrand's v derivatives jump, so the outer integrand along
    // the trim is not smooth there and a piece's error estimate, the leading term of an
    // expansion that holds only where it is smooth, may not bound the piece's error.
    let v_lines = lines.as_slice();
    let band_of = |v: f64| floors.partition_point(|&f| f <= v).saturating_sub(1);
    let floor = |band: usize| floors.get(band).copied().unwrap_or(base_v);

    let area_scale = estimated_area / 10.0;
    if let Some(s) = scale.first_mut() {
        *s = area_scale;
    }
    // Scale volume and moment errors by a length per extra dimension (WR-472; the
    // SolidCraft fork's tangency-labs/smlib@073d4740): the body size, ten times
    // sqrt(area), or the origin's distance when larger. Each component is then held
    // to the area's share of its own size, where 1.0.1's tenth (sqrt(area), or a
    // tenth of the origin's distance) holds each extra power ten times tighter, below
    // roundoff at tight accuracies.
    let length = {
        let a = 10.0 * estimated_area.sqrt();
        let b = origin_distance;
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
    if trims.is_empty() {
        return Ok(PreciseProperties::default());
    }
    let iso = IsoLine {
        surface,
        origin,
        thickness,
        layout: &layout,
        accuracy: accuracy / 10.0,
        scale: &scale,
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
            // Gauss-Kronrod at both levels, as the bands are (#14460): a Runge-Kutta step
            // pair a level cost at least 144 samples a piece, and an iso-u line long in v
            // (a cylinder's quarter turns) hundreds a sample; the 7-point rule takes 49 a
            // piece when no piece is halved. Neither level's nodes reach a piece's ends,
            // so no sample is read at a knot of the trim, or at a piece's end on a surface
            // knot line.
            let base = floor(band);
            span_y.fill(0.0);
            kronrod_integrate(k, a, b, accuracy / 10.0, &scale, &mut span_y, |t, dy| {
                trim_integrand(k, &iso, trim, base, t, &mut scratch, dy)
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
                    let eval = OdeIsoCurveLength::new(k, surface, u, origin, thickness, &layout);
                    // (the band's height, piece by piece between any v knots inside it)
                    let mut v0 = lo;
                    for v1 in v_knots.iter().copied().chain(core::iter::once(hi)) {
                        kronrod_integrate(k, v0, v1, iso.accuracy, &scale, dy, |v, g| {
                            eval.evaluate(v, &mut scratch_iso)?;
                            for (d, x) in g.iter_mut().zip(scratch_iso.iter().skip(1)) {
                                *d = *x;
                            }
                            Ok(())
                        })?;
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
        .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceuseWithoutRegion))
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
// ---- #16735: closing tolerant edges ------------------------------------------------------
//
// A face is integrated over its own UV trims, whose images run within the edge tolerance
// of the edge curve, not on it. Two faces of an edge therefore leave a gap between them,
// and the integrand's fields (`(0, 0, z - o_z)` times polynomials) leak through it: the
// volume is wrong to first order in the tolerance, and differently for every origin.
// Each face is closed onto its edges' own curves instead. Along every edge use a ruled
// strip joins the trim's image P to the edge curve E at the nearest point, and at every
// loop corner a fan of triangles joins the two trims' ends, the two edge curves' ends and
// the vertex. The faces with their strips and corners then share the edge curves and the
// vertices exactly, so they bound one closed surface, and the divergence theorem makes
// every volume integral that of the region it bounds whatever the origin. That region
// differs from the solid only within each edge's tolerance tube and each vertex's
// tolerance ball, where the strips and corners lie, so the volume is right to
// sum(pi tol_e^2 L_e) + sum(4/3 pi tol_v^3), where it was right to O(tol L).

/// The volume group's integrands at `x` (relative to the origin) for the area vector
/// `n` (per unit parameter area): the volume and the volume moments `4..8`, in the
/// layout of [`PrecisePropertyLayout`]'s volume terms (see `OdeIsoCurveLength`).
fn closure_integrands(x: Vec3, n: Vec3, out: &mut [f64; 13]) {
    let vd = x.z * n.z;
    let vals = [
        vd,
        x.x * vd,
        x.y * vd,
        x.z * vd / 2.0,
        (x.x * x.x) * vd,
        (x.y * x.y) * vd,
        (x.z * x.z / 3.0) * vd,
        (x.y * x.z) * vd / 2.0,
        (x.z * x.x) * vd / 2.0,
        (x.x * x.y) * vd,
        (x.y * x.y + x.z * x.z / 3.0) * vd,
        (x.x * x.x + x.z * x.z / 3.0) * vd,
        (x.x * x.x + x.y * x.y) * vd,
    ];
    for (o, v) in out.iter_mut().zip(vals) {
        *o += v;
    }
}

/// The volume group over `w` in `[0, 1]` of the ruled patch `X = (1 - w) a + w c` at one
/// value of its other parameter, `da` and `dc` the rails' derivatives in it, the area
/// vector `X_t x X_w`, into `out` (with `weight`). The integrands are polynomials of
/// degree 4 in `w`, which the 3-point Gauss rule integrates exactly.
fn ruled_column(a: Vec3, da: Vec3, c: Vec3, dc: Vec3, weight: f64, out: &mut [f64; 13]) {
    let xw = c - a;
    for (i, &node) in KRONROD_NODES.iter().enumerate() {
        let wg = GAUSS_WEIGHTS.get(i).copied().unwrap_or(0.0);
        if wg == 0.0 {
            continue;
        }
        let sides: &[f64] = if i == 0 { &[0.0] } else { &[-1.0, 1.0] };
        for &side in sides {
            let w = 0.5 + 0.5 * side * node;
            let x = a * (1.0 - w) + c * w;
            let xt = da * (1.0 - w) + dc * w;
            let mut col = [0.0; 13];
            closure_integrands(x, xt.cross(xw), &mut col);
            for (o, v) in out.iter_mut().zip(col) {
                *o += 0.5 * wg * weight * v;
            }
        }
    }
}

/// The volume group of the polygon `points` (relative to the origin), its area vector by
/// the right-hand rule round them: a fan of triangles from their centroid, each the
/// ruled patch from the centroid to its side, whose integrands are polynomials of
/// degree 4 in both parameters (3-point Gauss rules, exact).
fn polygon_closure(points: &[Vec3], out: &mut [f64; 13]) {
    if points.is_empty() {
        return;
    }
    let (mut centre, mut count) = (Vec3::new(0.0, 0.0, 0.0), 0.0);
    for p in points {
        centre += *p;
        count += 1.0;
    }
    let centre = centre * (1.0 / count);
    let zero = Vec3::new(0.0, 0.0, 0.0);
    for (i, &p) in points.iter().enumerate() {
        let q = points
            .get(i + 1)
            .or_else(|| points.first())
            .copied()
            .unwrap_or(p);
        let side = q - p;
        let mut tri = [0.0; 13];
        for (j, &node) in KRONROD_NODES.iter().enumerate() {
            let wg = GAUSS_WEIGHTS.get(j).copied().unwrap_or(0.0);
            if wg == 0.0 {
                continue;
            }
            let sides: &[f64] = if j == 0 { &[0.0] } else { &[-1.0, 1.0] };
            for &s in sides {
                let t = 0.5 + 0.5 * s * node;
                ruled_column(centre, zero, p + side * t, side, 0.5 * wg, &mut tri);
            }
        }
        // the ruled patch's area vector is minus the triangle's right-hand one
        for (o, v) in out.iter_mut().zip(tri) {
            *o -= v;
        }
    }
}

/// The parameter of the point of `curve` on `[t0, t1]` nearest `p`, by Gauss-Newton steps
/// on `(E - p) . E'` from `guess` (each step `(p - E) . E' / |E'|^2`, along the tangent:
/// Newton's own denominator `|E'|^2 + (E - p) . E''` can vanish or turn negative on a
/// short, sharply curved knot span, where Gauss-Newton still moves toward `p`).
///
/// `Ok(None)` where the solve cannot move from an iterate: the derivative there is zero or
/// not finite (a parametric singularity, as at the end of an edge parametrised by `sqrt s`).
/// The caller then solves again from another seed, or refuses; the guess is never returned
/// as if it were the nearest point.
///
/// # Errors
///
/// The curve's evaluation errors, and [`SolidError::Mass`] when the evaluation returns no
/// derivative.
fn nearest_on_edge(
    k: &Kernel,
    curve: &Curve,
    t0: f64,
    t1: f64,
    p: Vec3,
    guess: f64,
) -> Result<Option<f64>> {
    // (max then min: no panic on an inverted or NaN range)
    let mut t = guess.max(t0).min(t1);
    for _ in 0..64 {
        let e = curve.evaluate(k, t, 1, true, false)?;
        let (Some(&ep), Some(&d1)) = (e.first(), e.get(1)) else {
            return Err(defect(SHORT_EVALUATION));
        };
        let df = d1.dot(d1);
        let f = (ep - p).dot(d1);
        // written so a NaN stops the solve
        if !(df > 0.0 && df.is_finite() && f.is_finite()) {
            return Ok(None);
        }
        let next = (t - f / df).max(t0).min(t1);
        let step = (next - t).abs();
        t = next;
        if step <= 1e-15 * (1.0 + t.abs()) {
            break;
        }
    }
    Ok(Some(t))
}

/// The grid of nearest points along each strip (see [`gap_closure`]).
const CLOSURE_GRID: u32 = 64;

/// One edge use of a loop, in the direction of its edge: its UV trim, its edge's curve
/// on the edge's interval (`None` for an edge without a curve: the vertex), and the
/// points at the loop's start and end of it.
struct ClosureSide {
    trim: Curve,
    edge: Option<(Curve, f64, f64)>,
    vertex: Vec3,
    same: bool,
    trim_ends: [Vec3; 2],
    edge_ends: [Vec3; 2],
}

/// The side of a UV line the face's boundary fills a gap with (#16735): no edge, so it
/// closes as a cone onto `vertex`, the corner's; the line runs the loop's way.
fn gap_side(trim: Curve, vertex: Vec3) -> ClosureSide {
    ClosureSide {
        trim,
        edge: None,
        vertex,
        same: true,
        trim_ends: [vertex; 2],
        edge_ends: [vertex; 2],
    }
}

/// Not upstream (#16735): the volume group of the strips and corners closing face `f`'s
/// trims onto its edges' curves and vertices (see above), its area vectors the face's
/// surface normal's way round, relative to `origin`; the strips integrated along the
/// trims by [`kronrod_integrate`] to `accuracy` with the error scales of the face's own
/// integration (`estimated_area`, see [`boundary_properties`]).
///
/// # Errors
///
/// Those of the evaluations; [`SolidError::Mass`] for a missing surface, UV trim or
/// vertex.
fn gap_closure(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    f: FaceId,
    origin: Point3,
    accuracy: f64,
    estimated_area: f64,
) -> Result<PreciseProperties> {
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use crate::nurbs::knots::Side;
    use crate::topology::edgeuse::{p1 as eu1, p3 as eu3};
    let fu = crate::topology::face::p2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceWithoutFaceuse))?;
    let mut pools = Vec::new();
    for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
        if crate::topology::loopuse::is_vertex_loopuse(b, lu)? {
            continue;
        }
        let mut sides = Vec::new();
        for eu in eu1::loopuse_edgeuses(b, lu)? {
            let same = eu1::orientation(b, eu)? == OrientType::Same;
            if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                match b {
                    QueryTopology::Mutable(b) => {
                        eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
                    }
                    QueryTopology::Prepared(_) | QueryTopology::Requested(..) => {
                        return Err(SolidError::PreparationRequired { operation: OP });
                    }
                }
            }
            let trim = eu1::uv_trim_curve(b, eu)?.0.cloned().ok_or_else(|| {
                null_defect(
                    "the edge use has no UV trim just after it was made where it was \
                         missing",
                )
            })?;
            let vu = b
                .edgeuse(eu)?
                .vertexuse
                .ok_or_else(|| SolidError::mass(OP, MassFailure::EdgeuseWithoutVertexuse))?;
            let v = b
                .vertexuse(vu)?
                .vertex
                .ok_or_else(|| SolidError::mass(OP, MassFailure::VertexuseWithoutVertex))?;
            let vertex = b.vertex(v)?.point;
            let e = eu1::edge(b, eu)?
                .ok_or_else(|| SolidError::mass(OP, MassFailure::EdgeuseWithoutEdge))?;
            let ivl = b.edge(e)?.interval;
            let edge = edge::curve(b, e)?.map(|c| (c.clone(), ivl.min(), ivl.max()));
            sides.push((
                eu,
                Some(ClosureSide {
                    trim,
                    edge,
                    vertex,
                    same,
                    trim_ends: [vertex; 2],
                    edge_ends: [vertex; 2],
                }),
            ));
        }
        pools.push(sides);
    }
    // The face is integrated over its oriented trim boundary ([`boundary`]), which fills a
    // UV gap between consecutive trims (a surface singularity, or a trim ending short of the
    // next) with a UV line. Such a line's image is part of the face's boundary but of no
    // edge use: it closes as a cone onto the corner's vertex, as a side without an edge
    // curve does, so every piece of the boundary the face integral runs along is closed.
    let integrated = closed_boundary(k, b, f)?;
    let mut at_curve = 0usize;
    let mut counts = integrated.loops.iter();
    let mut loops: Vec<Vec<ClosureSide>> = Vec::new();
    for mut pool in pools {
        let Some(&count) = counts.next() else {
            // a loop the boundary does not integrate keeps its edge uses' order
            loops.push(pool.into_iter().filter_map(|(_, side)| side).collect());
            continue;
        };
        let curves = at_curve..at_curve + count;
        at_curve += count;
        let mut closed = Vec::new();
        let mut gaps = Vec::new();
        for j in curves {
            match integrated.edgeuses.get(j) {
                Some(Some(eu)) => {
                    // the side of this boundary curve's own edge use, whatever its place
                    // in the loop; the gap lines before it end at its vertex
                    // (taken once: a second curve of the same edge use finds it gone)
                    let Some(side) = pool
                        .iter_mut()
                        .find(|(id, _)| id == eu)
                        .and_then(|(_, side)| side.take())
                    else {
                        return Err(defect(BOUNDARY_LOOPS));
                    };
                    for trim in core::mem::take(&mut gaps) {
                        closed.push(gap_side(trim, side.vertex));
                    }
                    closed.push(side);
                }
                Some(None) => {
                    let trim = integrated
                        .curves
                        .get(j)
                        .cloned()
                        .ok_or_else(|| defect(BOUNDARY_LOOPS))?;
                    gaps.push(trim);
                }
                None => {
                    return Err(defect(BOUNDARY_LOOPS));
                }
            }
        }
        // gap lines after the last trim close the loop at its first side's vertex
        if let Some(first) = closed.first().map(|s: &ClosureSide| s.vertex) {
            for trim in core::mem::take(&mut gaps) {
                closed.push(gap_side(trim, first));
            }
        }
        if pool.iter().any(|(_, side)| side.is_some()) {
            return Err(defect(BOUNDARY_LOOPS));
        }
        loops.push(closed);
    }
    let surface = b
        .face(f)?
        .surface
        .clone()
        .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceWithoutSurface))?;
    // continued past its domain where the face's integration continues it, so the strips
    // start on the curves the face's Green integral runs along
    let _unclamped = Unclamped::where_trims_may_leave(k, &surface, &integrated.curves)?;
    // The surface's v knot lines: a strip, like the face's own trim pieces
    // ([`boundary_properties`]), ends where its trim crosses a surface knot line in either
    // direction, as the trim image's derivative `su u' + sv v'` need not be smooth there.
    // Taken over the whole real line, end knots included: `trim_curve_breaks` keeps only
    // the lines strictly inside each trim's box, and a trim that leaves a clamped domain
    // still breaks at a line it crosses there.
    let v_lines = surface_knots_inside(
        k,
        &surface,
        SurfParamType::V,
        f64::NEG_INFINITY,
        f64::INFINITY,
    )?;
    // the surface as the face's integrand evaluates it (`OdeIsoCurveLength`): the point,
    // then the u and v derivatives
    let at = |uv: Vec3| -> Result<(Vec3, Vec3, Vec3)> {
        let flags = EvaluateFlags {
            u_side: Side::Left,
            v_side: Side::Left,
            only_upper_half: true,
            non_zero_tangents: true,
        };
        let mut pv = [[Vec3::ZERO; 2]; 2];
        surface.evaluate_simple_first_into(
            k,
            crate::geom::vector2d::Point2::new(uv.x, uv.y),
            flags,
            &mut pv,
        )?;
        let [[p, sv], [su, _]] = pv;
        Ok((p, su, sv))
    };
    let missing = || null_defect(SHORT_EVALUATION);
    // the face integration's error scales: a length per extra dimension
    let length = (10.0 * estimated_area.sqrt())
        .max(face_box(k, b, f)?.min_distance(&Extent3d::from_point(origin)));
    let area_scale = estimated_area / 10.0;
    let mut scale = [area_scale * length; 13];
    for (i, s) in scale.iter_mut().enumerate().skip(1) {
        *s = area_scale * length.powi(if i <= 3 { 2 } else { 3 });
    }
    let accuracy = if accuracy < EFF_ZERO {
        EFF_ZERO_SQ
    } else {
        accuracy
    };
    let mut sum = [0.0_f64; 13];
    for sides in &mut loops {
        // the strips, each along its edge's direction
        for side in sides.iter_mut() {
            let ivl = side.trim.natural_interval();
            let (c0, c1) = (ivl.min(), ivl.max());
            let p0 = at(side.trim.evaluate_point(k, c0)?)?.0;
            let p1 = at(side.trim.evaluate_point(k, c1)?)?.0;
            let Some((curve, e0, e1)) = &side.edge else {
                // no edge curve: the strip is the cone from the trim to the vertex
                let v = side.vertex;
                side.trim_ends = if side.same { [p0, p1] } else { [p1, p0] };
                let mut strip = [0.0; 13];
                let breaks = trim_curve_breaks(k, &side.trim, &surface, &v_lines)?;
                for piece in breaks.windows(2) {
                    let [lo, hi] = piece else { continue };
                    if hi.partial_cmp(lo) != Some(std::cmp::Ordering::Greater) {
                        continue;
                    }
                    kronrod_integrate(k, *lo, *hi, accuracy, &scale, &mut strip, |t, dy| {
                        let d = side.trim.evaluate(k, t, 1, true, false)?;
                        let (Some(&uv), Some(&duv)) = (d.first(), d.get(1)) else {
                            return Err(missing());
                        };
                        let (p, su, sv) = at(uv)?;
                        let mut col = [0.0; 13];
                        let zero = Vec3::new(0.0, 0.0, 0.0);
                        ruled_column(
                            p - origin,
                            su * duv.x + sv * duv.y,
                            v - origin,
                            zero,
                            1.0,
                            &mut col,
                        );
                        for (o, c) in dy.iter_mut().zip(col) {
                            *o = c;
                        }
                        Ok(())
                    })?;
                }
                let sigma = if side.same { 1.0 } else { -1.0 };
                for (o, v) in sum.iter_mut().zip(strip) {
                    *o -= sigma * v;
                }
                continue;
            };
            let (e0, e1) = (*e0, *e1);
            // The strip's correspondence t(tau) from the trim's parameter to the edge's:
            // the ends to the ends, and between them on a grid the nearest points,
            // marched from one to the next; linear between the grid's points, so the
            // strip is one continuous ruled surface whose derivative is exact, joined to
            // the edge curve end to end, whatever the nearest points' accuracy.
            let grid = CLOSURE_GRID;
            let mut table = vec![(c0, e0)];
            let mut last = e0;
            for i in 1..grid {
                let tau = c0 + (c1 - c0) * f64::from(i) / f64::from(grid);
                let p = at(side.trim.evaluate_point(k, tau)?)?.0;
                // marched from the last point; where that seed is singular (the edge's
                // end, for the first), from the grid point's share of the edge's interval;
                // singular from both, the strip is refused
                let share = e0 + (e1 - e0) * f64::from(i) / f64::from(grid);
                last = match nearest_on_edge(k, curve, e0, e1, p, last)? {
                    Some(t) => t,
                    None => nearest_on_edge(k, curve, e0, e1, p, share)?
                        .ok_or_else(|| SolidError::mass(OP, MassFailure::EdgeCurveSingular))?,
                };
                table.push((tau, last));
            }
            table.push((c1, e1));
            let ea = curve.evaluate_point(k, e0)?;
            let eb = curve.evaluate_point(k, e1)?;
            side.trim_ends = if side.same { [p0, p1] } else { [p1, p0] };
            side.edge_ends = if side.same { [ea, eb] } else { [eb, ea] };
            // pieces between the grid's points, the trim's knots and its crossings of the
            // surface's knot lines; a trim without knots (a B-spline missing its NURB) is
            // malformed: its error is the volume's
            let mut breaks: Vec<f64> = trim_curve_breaks(k, &side.trim, &surface, &v_lines)?
                .into_iter()
                .filter(|&t| t > c0 && t < c1)
                .collect();
            breaks.extend(table.iter().map(|&(tau, _)| tau));
            breaks.sort_by(f64::total_cmp);
            breaks.dedup();
            let mut strip = [0.0; 13];
            for piece in breaks.windows(2) {
                let [lo, hi] = piece else { continue };
                // the grid segment holding the piece
                let mid = 0.5 * (lo + hi);
                let j = table
                    .partition_point(|&(tau, _)| tau <= mid)
                    .saturating_sub(1);
                let (Some(&(ta, sa)), Some(&(tb, sb))) = (table.get(j), table.get(j + 1)) else {
                    continue;
                };
                let slope = if tb > ta { (sb - sa) / (tb - ta) } else { 0.0 };
                kronrod_integrate(k, *lo, *hi, accuracy, &scale, &mut strip, |tau, dy| {
                    let d = side.trim.evaluate(k, tau, 1, true, false)?;
                    let (Some(&uv), Some(&duv)) = (d.first(), d.get(1)) else {
                        return Err(missing());
                    };
                    let (p, su, sv) = at(uv)?;
                    let dp = su * duv.x + sv * duv.y;
                    let t = sa + (tau - ta) * slope;
                    let e = curve.evaluate(k, t, 1, true, false)?;
                    let (Some(&ep), Some(&de)) = (e.first(), e.get(1)) else {
                        return Err(missing());
                    };
                    let mut col = [0.0; 13];
                    ruled_column(p - origin, dp, ep - origin, de * slope, 1.0, &mut col);
                    for (o, c) in dy.iter_mut().zip(col) {
                        *o = c;
                    }
                    Ok(())
                })?;
            }
            // the strip's area vector X_tau x X_w runs against the face's along the trim
            let sigma = if side.same { 1.0 } else { -1.0 };
            for (o, v) in sum.iter_mut().zip(strip) {
                *o -= sigma * v;
            }
        }
        // the corners: from the end of one trim to the start of the next
        for (side, next) in sides.iter().zip(sides.iter().cycle().skip(1)) {
            let [_, q_end] = side.trim_ends;
            let [_, e_end] = side.edge_ends;
            let [q_start, _] = next.trim_ends;
            let [e_start, _] = next.edge_ends;
            let polygon = [q_end, e_end, next.vertex, e_start, q_start].map(|p| p - origin);
            polygon_closure(&polygon, &mut sum);
        }
    }
    let mut out = PreciseProperties::default();
    let [vol, m4x, m4y, m4z, m5x, m5y, m5z, m6x, m6y, m6z, m7x, m7y, m7z] = sum;
    out.volume = vol;
    let [_, _, _, _, m4, m5, m6, m7] = &mut out.moments;
    *m4 = Vec3::new(m4x, m4y, m4z);
    *m5 = Vec3::new(m5x, m5y, m5z);
    *m6 = Vec3::new(m6x, m6y, m6z);
    *m7 = Vec3::new(m7x, m7y, m7z);
    Ok(out)
}
/// `SmBrep::ComputePreciseProperties`, including density attributes and wire/point solids.
/// Missing UV trim curves of the integrated faces are made on `b`.
///
/// Not upstream (#16735): each volume-bounding face is closed onto its edges' curves and
/// vertices, and integrated on its surface continued past its domain where its trims leave
/// it and the continuation is certified. Known limit: a face whose trims leave its domain
/// but whose continuation is not certified is integrated clamped, as upstream does, and
/// keeps that integration's first-order, origin-dependent volume error; no bound for it is
/// reported (see `Unclamped::where_trims_may_leave`).
pub fn brep_properties(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
) -> Result<PreciseProperties> {
    let bbox = qbrep::calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?;
    brep_properties_in_box(k, b, accuracy, origin, behavior, attributes, &bbox)
}
/// [`brep_properties`] with the body's box given: its largest side sets every face's
/// error scale (not upstream: [`brep_properties`] takes the loose box, whose arc of a large
/// circle reaches a quarter of that circle away, #17361).
pub fn brep_properties_in_box(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
    bbox: &Extent3d,
) -> Result<PreciseProperties> {
    properties_in_box(
        k,
        &mut QueryTopology::Mutable(b),
        accuracy,
        origin,
        behavior,
        attributes,
        bbox,
        None,
    )
}

/// [`brep_properties_in_box`] and its summands: each face use's, closure's and wire
/// edge's contribution as it is added to the total, in order (the rounding of the total is at most `gamma_(n-1)` times the sum of the
/// summands' magnitudes, per component). The total is the same, bit for bit.
///
/// # Errors
///
/// Those of [`brep_properties_in_box`]; [`SolidError::Unsupported`] for a body with a
/// shell vertex, whose ball is not recorded as a summand.
#[doc(hidden)] // (for the goldens' rounding bounds; the kernel itself does not call it)
pub fn brep_properties_in_box_terms(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
    bbox: &Extent3d,
) -> Result<(PreciseProperties, Vec<PreciseProperties>)> {
    let mut terms = Vec::new();
    let total = properties_in_box(
        k,
        &mut QueryTopology::Mutable(b),
        accuracy,
        origin,
        behavior,
        attributes,
        bbox,
        Some(&mut terms),
    )?;
    Ok((total, terms))
}

/// Pushes on `terms`, when recording, the summand `add` adds to a zero total.
fn record(
    terms: &mut Option<&mut Vec<PreciseProperties>>,
    add: impl FnOnce(&mut PreciseProperties),
) {
    if let Some(terms) = terms.as_deref_mut() {
        let mut term = PreciseProperties::default();
        add(&mut term);
        terms.push(term);
    }
}

/// [`brep_properties_in_box`], recording its summands on `terms` when given.
#[allow(clippy::too_many_arguments)] // brep_properties_in_box's inputs and the record
fn properties_in_box(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    accuracy: f64,
    origin: Point3,
    behavior: MassPropertyBehavior,
    attributes: &PropertyAttributes<'_>,
    bbox: &Extent3d,
    mut terms: Option<&mut Vec<PreciseProperties>>,
) -> Result<PreciseProperties> {
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
            .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceWithoutFaceuse))?;
        let mate = b
            .faceuse(up)?
            .mate
            .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceuseWithoutMate))?;
        let mut separating = faceuse_region(b, up)? != faceuse_region(b, mate)?;
        // the face's closure onto its edges (#16735), made once for both its uses
        let mut closure: Option<PreciseProperties> = None;
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
                        .ok_or_else(|| SolidError::mass(OP, MassFailure::FaceuseWithoutMate))?;
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
                // on the surface continued where the trims leave its domain, as its closure
                // strips are built (#16735)
                let delta = face_properties_in(
                    k,
                    b,
                    f,
                    orientation,
                    accuracy,
                    origin,
                    estimated,
                    if separating { 0.0 } else { mass.y },
                    PPF_ALL,
                    true,
                )?;
                if separating {
                    // Not upstream (WR-535, the fork's tangency-labs/smlib@d2d9586): the
                    // faceuse's volume and mass enter only when it bounds a non void region,
                    // its area and area moments only when the area behaviour selects it: a
                    // face between two solid regions bounds both volumes but is no part of
                    // the area
                    add_selected(&mut total, &delta, mass.x, do_volume, do_area);
                    record(&mut terms, |t| {
                        add_selected(t, &delta, mass.x, do_volume, do_area);
                    });
                    // Not upstream (#16735): the strips and corners closing the face onto
                    // its edges, the use's way round as its volume
                    if do_volume {
                        if closure.is_none() {
                            closure = Some(gap_closure(k, b, f, origin, accuracy, estimated)?);
                        }
                        if let Some(c) = &closure {
                            let sign = if orientation == OrientType::Same {
                                -1.0
                            } else {
                                1.0
                            };
                            let mut c = c.clone();
                            c.volume *= sign;
                            for m in c.moments.iter_mut().skip(4) {
                                *m *= sign;
                            }
                            add_selected(&mut total, &c, mass.x, true, false);
                            record(&mut terms, |t| add_selected(t, &c, mass.x, true, false));
                        }
                    }
                } else {
                    add(&mut total, &delta, mass.x);
                    record(&mut terms, |t| add(t, &delta, mass.x));
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
        record(&mut terms, |t| add(t, &delta.properties, mass.x));
    }
    for v in vertices(b)? {
        if !vertex::is_shell_vertex(b, v)? {
            continue;
        }
        if terms.is_some() {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a shell vertex's ball is not recorded as a summand",
            });
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
    let c = edge::curve(b, e)?.ok_or_else(|| {
        Failure::new(OP, MassFailure::EdgeWithoutCurve)
            .with_status(SM_ERR)
            .into_error()
    })?;
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
    face_area_in(k, &mut QueryTopology::Mutable(b), f, accuracy)
}

/// [`face_area`] of a B-rep read only, whose UV trim curves exist: the same area, bit for
/// bit, and nothing is made on `b`.
///
/// # Errors
///
/// Those of [`face_area`]; [`SolidError::PreparationRequired`] where an edge use of the
/// face has no UV trim curve ([`missing_uv_trim`] names the first).
pub fn face_area_prepared(k: &Kernel, b: &Brep, f: FaceId, accuracy: f64) -> Result<f64> {
    require_uv_trims(b, Some(f))?;
    face_area_in(k, &mut QueryTopology::Prepared(b), f, accuracy)
}

/// [`face_area`] through `b` (one implementation of both forms).
fn face_area_in(k: &Kernel, b: &mut QueryTopology<'_>, f: FaceId, accuracy: f64) -> Result<f64> {
    valid_api_number(accuracy)?;
    let bbox = face_box(k, b, f)?;
    Ok(face_properties_in(
        k,
        b,
        f,
        OrientType::Same,
        accuracy,
        bbox.mid(),
        area_estimate(&bbox),
        0.0,
        PPF_AREA,
        false,
    )?
    .area
    .abs())
}
/// `SmApiFaceComputeCentroid`: rejects a non-finite accuracy; one area/first-moment
/// integration.
pub fn face_centroid(k: &Kernel, b: &mut Brep, f: FaceId, accuracy: f64) -> Result<(f64, Point3)> {
    face_centroid_in(k, &mut QueryTopology::Mutable(b), f, accuracy)
}

/// [`face_centroid`] of a B-rep read only, whose UV trim curves exist: the same area and
/// centroid, bit for bit, and nothing is made on `b`.
///
/// # Errors
///
/// As [`face_area_prepared`].
pub fn face_centroid_prepared(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    accuracy: f64,
) -> Result<(f64, Point3)> {
    require_uv_trims(b, Some(f))?;
    face_centroid_in(k, &mut QueryTopology::Prepared(b), f, accuracy)
}

/// [`face_centroid`] through `b` (one implementation of both forms).
fn face_centroid_in(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    f: FaceId,
    accuracy: f64,
) -> Result<(f64, Point3)> {
    valid_api_number(accuracy)?;
    let bbox = face_box(k, b, f)?;
    let origin = bbox.mid();
    let p = face_properties_in(
        k,
        b,
        f,
        OrientType::Same,
        accuracy,
        origin,
        area_estimate(&bbox),
        0.0,
        PPF_AREA | PPF_AREA_FIRST_MOMENTS,
        false,
    )?;
    if p.area.abs() < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "zero area"));
    }
    let [first_moments, ..] = p.moments;
    Ok((p.area.abs(), origin + first_moments / p.area))
}
/// `SmApiBrepComputeArea`, including open shells and sheets: [`brep_area_about`] each
/// face's own box middle.
pub fn brep_area(k: &Kernel, b: &mut Brep, accuracy: f64) -> Result<f64> {
    area_about(
        k,
        &mut QueryTopology::Mutable(b),
        accuracy,
        AreaAbout::FaceMiddle,
    )
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
    area_about(
        k,
        &mut QueryTopology::Mutable(b),
        estimate_accuracy(accuracy)?,
        about,
    )
}

/// [`brep_area_about`] of a B-rep read only, whose UV trim curves exist: the same area,
/// bit for bit, and nothing is made on `b`.
///
/// # Errors
///
/// Those of [`brep_area_about`]; [`SolidError::PreparationRequired`] where an edge use of
/// a face has no UV trim curve ([`missing_uv_trim`] names the first).
pub fn brep_area_about_prepared(
    k: &Kernel,
    b: &Brep,
    accuracy: f64,
    about: AreaAbout,
) -> Result<f64> {
    require_uv_trims(b, None)?;
    area_about(
        k,
        &mut QueryTopology::Prepared(b),
        estimate_accuracy(accuracy)?,
        about,
    )
}

/// The first edge use, of face `f` or (`None`) of every face in the B-rep's order, loop by
/// loop as [`crate::topology::trimming_tools::face_uv_curves_present`] walks them, that
/// has no UV trim curve, its own or its mate's: the trim a `_prepared` form needs and
/// does not make. `None` when every one exists.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the loop lists.
pub fn missing_uv_trim(b: &Brep, f: Option<FaceId>) -> Result<Option<EdgeuseId>> {
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::loopuse as lu1;
    let faces = match f {
        Some(f) => vec![f],
        None => faces(b)?,
    };
    for f in faces {
        for l in crate::topology::face::p2::loops(b, f, false, None)? {
            let (lu, _) = lu1::loop_loopuses(b, l)?;
            for eu in lu1::edgeuses(b, lu, None, None)? {
                if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                    return Ok(Some(eu));
                }
            }
        }
    }
    Ok(None)
}

/// [`SolidError::PreparationRequired`] unless every UV trim curve [`missing_uv_trim`]
/// looks for exists. A failure of the walk is the same signal: the form that takes the
/// B-rep mutably then reports it as it does.
fn require_uv_trims(b: &Brep, f: Option<FaceId>) -> Result<()> {
    match missing_uv_trim(b, f) {
        Ok(None) => Ok(()),
        Ok(Some(_)) | Err(_) => Err(SolidError::PreparationRequired { operation: OP }),
    }
}

/// [`SolidError::PreparationRequired`] when the tight box of the estimated entry could
/// classify a point on a face whose surface's trimmed cache would have its tree built
/// there ([`crate::query::face::p2::classification_builds_tree`]), which the read-only
/// form does not do: decided before any work, so the form that takes the B-rep mutably
/// then runs alone. A failure of the scan is the same signal.
fn require_classification_trees(b: &Brep) -> Result<()> {
    let builds = |f| crate::query::face::p2::classification_builds_tree(b, f).unwrap_or(true);
    match faces(b) {
        Ok(faces) if !faces.iter().copied().any(builds) => Ok(()),
        _ => Err(SolidError::PreparationRequired { operation: OP }),
    }
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
fn area_about(
    k: &Kernel,
    b: &mut QueryTopology<'_>,
    accuracy: f64,
    about: AreaAbout,
) -> Result<f64> {
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
/// exact bounding box's midpoint (#17361), the moments then shifted to `origin` (which
/// avoids cancellation for a distant origin).
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
        &mut QueryTopology::Mutable(b),
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
/// integration (Gauss-Kronrod over faces, Runge-Kutta along edges) whose error estimate
/// per piece or step is held under
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
/// once about the exact bounding box's middle (#17361): volume and moments over the
/// material regions (`SmBrep::ComputePreciseProperties`), the area of the faces between a
/// solid and a void, then the moments moved to the point asked, or to the centroid. **The
/// accuracy is requested and estimated, not proved** ([`EstimatedMassProperties`]).
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
    estimate(
        k,
        &mut QueryTopology::Mutable(b),
        accuracy,
        request,
        attributes,
    )
    .map_err(typed_failure)
}

/// [`brep_mass_properties_estimated`] of a B-rep read only, whose UV trim curves exist:
/// the same result, bit for bit, and nothing is made on `b`.
///
/// # Errors
///
/// Those of [`brep_mass_properties_estimated`]; [`SolidError::PreparationRequired`] where
/// an edge use of a face has no UV trim curve ([`missing_uv_trim`] names the first), or
/// the body's exact box would build a face's classification tree.
pub fn brep_mass_properties_estimated_prepared(
    k: &Kernel,
    b: &Brep,
    accuracy: f64,
    request: &MassPropertyRequest,
    attributes: &PropertyAttributes<'_>,
) -> Result<EstimatedMassProperties> {
    require_uv_trims(b, None)?;
    require_classification_trees(b)?;
    let accuracy = estimate_accuracy(accuracy)?;
    no_unsure_region(b)?;
    estimate(
        k,
        &mut QueryTopology::Prepared(b),
        accuracy,
        request,
        attributes,
    )
    .map_err(typed_failure)
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
    b: &mut QueryTopology<'_>,
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
    let bbox = match b {
        QueryTopology::Mutable(b) => qbrep::calculate_tight_bounding_box(k, &mut **b)?,
        QueryTopology::Prepared(b) | QueryTopology::Requested(b, _) => {
            qbrep::calculate_tight_bounding_box(k, LazyBrep::Prepared(b))?
        }
    };
    let mid = bbox.mid();
    if request.area_centroid_without_material && !has_material(b)? {
        return area_centroid(k, b, accuracy, mid);
    }
    let properties = properties_in_box(
        k,
        b,
        accuracy,
        mid,
        MassPropertyBehavior {
            density,
            ..MassPropertyBehavior::default()
        },
        attributes,
        &bbox,
        None,
    )?;
    valid_api_number(properties.mass)?;
    valid_api_number(properties.volume)?;
    if properties.mass < EFF_ZERO || properties.volume < EFF_ZERO {
        return Err(SolidError::invalid_input(OP, "nonpositive mass or volume"));
    }
    // [4] = static moment S about the midpoint; [7] = moments of inertia
    // (y^2+z^2, ...); [6] = products (yz, zx, xy) as raw positive integrals.
    let [_, _, _, _, static_moment, _, m6, m7] = properties.moments;
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
                m7 + Vec3::new(
                    shift(1, 1) + shift(2, 2),
                    shift(0, 0) + shift(2, 2),
                    shift(0, 0) + shift(1, 1),
                ),
                m6 + Vec3::new(shift(1, 2), shift(2, 0), shift(0, 1)),
            )
        }
        MomentsAbout::Centroid => {
            // the centroid's offset d = S / m from the midpoint; about the centroid the
            // integral of a b dm loses m d_a d_b
            let d = static_moment / mass;
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
    b: &mut QueryTopology<'_>,
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
    b: &mut QueryTopology<'_>,
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
        let p = face_properties_in(
            k,
            b,
            f,
            OrientType::Same,
            accuracy,
            origin,
            area_estimate(&fbox),
            0.0,
            flags,
            false,
        )?;
        // |area|, as `SmApiFaceComputeArea` takes it; the first moments flip with it
        area += p.area.abs();
        if first {
            let sign = if p.area < 0.0 { -1.0 } else { 1.0 };
            let [first_moments, ..] = p.moments;
            sum += first_moments * sign;
        }
    }
    Ok((area, sum))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 15-point Gauss-Kronrod rule integrates `x^n` on `[-1, 1]` exactly up to degree 22
    /// and its embedded 7-point Gauss rule up to degree 13; and the 7-point rule's to 10 and 5.
    #[test]
    fn kronrod_rules_are_exact_to_their_degrees() {
        let rule = |nodes: &[f64], weights: &[f64], n: i32| -> f64 {
            nodes
                .iter()
                .zip(weights)
                .map(|(&x, &w)| {
                    if x == 0.0 {
                        w * if n == 0 { 1.0 } else { 0.0 }
                    } else {
                        w * (x.powi(n) + (-x).powi(n))
                    }
                })
                .sum()
        };
        let exact = |n: i32| {
            if n % 2 == 1 {
                0.0
            } else {
                2.0 / f64::from(n + 1)
            }
        };
        for (nodes, kronrod, gauss, dk, dg) in [
            (
                &KRONROD_NODES[..],
                &KRONROD_WEIGHTS[..],
                &GAUSS_WEIGHTS[..],
                10,
                5,
            ),
            (
                &KRONROD15_NODES[..],
                &KRONROD15_WEIGHTS[..],
                &GAUSS7_WEIGHTS[..],
                22,
                13,
            ),
        ] {
            for n in 0..=dk {
                let got = rule(nodes, kronrod, n);
                assert!((got - exact(n)).abs() <= 1e-15, "Kronrod degree {n}: {got}");
            }
            for n in 0..=dg {
                let got = rule(nodes, gauss, n);
                assert!((got - exact(n)).abs() <= 1e-15, "Gauss degree {n}: {got}");
            }
            // and no further: the Gauss rule misses the next even power
            assert!((rule(nodes, gauss, dg + 1) - exact(dg + 1)).abs() > 1e-15);
        }
    }
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
    /// A body whose topology links the caller left out (the B-rep's links are public) is
    /// the caller's: a named invalid-input cause with `SM_ERR_NULL_POINTER`, not a defect.
    #[test]
    fn a_missing_link_of_the_callers_body_is_refused_by_name() {
        let mut k = Kernel::default();
        let boxed = |k: &mut Kernel| {
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap()
        };
        // a face use without its mate
        let mut b = boxed(&mut k);
        let f = *faces(&b).unwrap().first().unwrap();
        let fu = b.face(f).unwrap().faceuse.unwrap();
        b.faceuse_mut(fu).unwrap().mate = None;
        let err = brep_volume_default(&k, &mut b, 1e-6).unwrap_err();
        assert_eq!(
            err.mass_failure(),
            Some(&MassFailure::FaceuseWithoutMate),
            "{err}"
        );
        assert_eq!(err.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(err.kind(), crate::error::ErrorKind::InvalidInput);
        assert!(!err.is_defect(), "{err}");
        // (`face_area` on a face without its face use, or with an edge use without its
        // edge, is refused earlier as invalid input, by `topology::face::p1` and `p4`)
    }
    /// A trim whose point is not finite is refused as such at once, not bisected to the
    /// depth bound and reported as step control.
    #[test]
    fn a_non_finite_trim_point_is_refused_by_name() {
        let k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(
            &mut Kernel::default(),
            Point3::ZERO,
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let f = *faces(&b).unwrap().first().unwrap();
        let surface = crate::topology::face::p2::surface(&b, f).unwrap().unwrap();
        let layout = PrecisePropertyLayout::new(PPF_AREA);
        let scale = vec![1.0; layout.num_components];
        let iso = IsoLine {
            surface,
            origin: Point3::ZERO,
            thickness: 0.0,
            layout: &layout,
            accuracy: 1e-10,
            scale: &scale,
        };
        let trim = Curve::from(crate::curve::line::Line::from_points(
            Point3::new(0.5, f64::NAN, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            2,
        ));
        let mut scratch = vec![0.0; layout.num_components + 1];
        let mut dy = vec![0.0; layout.num_components];
        let err = trim_integrand(&k, &iso, &trim, 0.0, 0.5, &mut scratch, &mut dy).unwrap_err();
        assert!(err.to_string().contains("not finite"), "{err}");
        assert_eq!(err.mass_failure(), Some(&MassFailure::TrimNotFinite));
        assert_eq!(err.upstream_status(), Some(SM_ERR));
    }
    /// A constant integrand is integrated exactly: the piece's width times it, whatever its
    /// size (summed weight by weight, the rounded Kronrod weights' sum `2 + 2^-54` put
    /// 10^4 over `[0, 100]` 1 ulp over 10^6).
    #[test]
    fn a_constant_is_integrated_exactly() {
        let k = Kernel::default();
        for c in [1.0, 100.0, 10_000.0, 20_000.0, -50.0, 1.0 / 3.0] {
            for (a, b) in [(0.0, 100.0), (-1.0, 1.0), (0.0, 2.0), (-35.0, 65.0)] {
                let mut y = [0.0];
                kronrod_integrate(&k, a, b, 1e-10, &[1.0], &mut y, |_, dy| {
                    dy[0] = c;
                    Ok(())
                })
                .unwrap();
                assert_eq!(y[0], (b - a) * c, "{c} over [{a}, {b}]");
            }
        }
    }
    /// #15021: a 100 x 100 x 2 box away from the origin, about the middle of its box (as the
    /// app measures a body), has volume exactly 20000 and area exactly 20800: every face's
    /// integrand is constant.
    #[test]
    fn a_plate_box_is_measured_exactly() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(-30.0, -35.0, 10.0),
            100.0,
            100.0,
            2.0,
        )
        .unwrap();
        let bbox = crate::query::api_queries::brep_bounding_box(&k, &b, true).unwrap();
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let p = brep_properties_in_box(
            &k,
            &mut b,
            1e-8,
            bbox.mid(),
            MassPropertyBehavior::default(),
            &attrs,
            &bbox,
        )
        .unwrap();
        assert_eq!(p.volume, 20_000.0, "{p:?}");
        assert_eq!(p.area, 20_800.0, "{p:?}");
    }

    /// [`brep_properties_in_box_terms`] returns [`brep_properties_in_box`]'s total bit for
    /// bit with two summands per face use of a box, and refuses a body with a shell vertex,
    /// whose ball it does not record.
    #[test]
    fn properties_terms_match_the_total_and_refuse_a_shell_vertex() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(-3.0, 5.0, 1.0),
            7.0,
            2.0,
            3.0,
        )
        .unwrap();
        let bbox = crate::query::api_queries::brep_bounding_box(&k, &b, true).unwrap();
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let behavior = MassPropertyBehavior::default();
        let p =
            brep_properties_in_box(&k, &mut b, 1e-8, bbox.mid(), behavior, &attrs, &bbox).unwrap();
        let (total, terms) =
            brep_properties_in_box_terms(&k, &mut b, 1e-8, bbox.mid(), behavior, &attrs, &bbox)
                .unwrap();
        assert_eq!(total.volume.to_bits(), p.volume.to_bits());
        assert_eq!(total.area.to_bits(), p.area.to_bits());
        // (each of the 6 face uses, then its closure strips and corners)
        assert_eq!(terms.len(), 12, "{terms:?}");
        let mut sum = PreciseProperties::default();
        for t in &terms {
            add(&mut sum, t, 1.0);
        }
        assert_eq!(sum.volume.to_bits(), p.volume.to_bits());
        assert_eq!(sum.area.to_bits(), p.area.to_bits());

        crate::topology::brep::p2::make_shell_vertex_topology(&mut b, None).unwrap();
        let refused =
            brep_properties_in_box_terms(&k, &mut b, 1e-8, bbox.mid(), behavior, &attrs, &bbox);
        assert!(
            matches!(refused, Err(SolidError::Unsupported { .. })),
            "{refused:?}"
        );
        // (the total alone still measures it)
        brep_properties_in_box(&k, &mut b, 1e-8, bbox.mid(), behavior, &attrs, &bbox).unwrap();
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

    /// The estimated entry types each integration failure by its cause, and only step
    /// control as not converging.
    #[test]
    fn integration_failures_are_typed_by_cause() {
        use super::super::{OP_INTEGRAND, OP_STEP_CONTROL};
        const OP_SKIPPED_SPAN: &str = "mass::integrator::properties::skipped_span";
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

    /// What a measure could change on a B-rep through a shared reference or by making a
    /// trim: each edge use's UV trim and gap cache, each edge's and face's tolerance, each
    /// face surface's trim cache slot.
    fn trims_and_caches(b: &Brep) -> String {
        use crate::topology::edgeuse::p1 as eu1;
        let mut out = Vec::new();
        for (u, r) in b.edgeuses().iter() {
            let gap = eu1::is_max_edge_face_gap_init(b, u).unwrap();
            out.push(format!("{u:?} {} {gap}", r.uv_trim_curve.is_some()));
        }
        for e in edges(b).unwrap() {
            out.push(format!("{e:?} {:?}", edge::tolerance(b, e).unwrap()));
        }
        for f in faces(b).unwrap() {
            let s = crate::topology::face::p2::surface(b, f).unwrap().unwrap();
            let face = b.face(f).unwrap();
            out.push(format!(
                "{f:?} {:?} {} {}",
                face.tolerance,
                s.has_trim_cache(),
                s.trim_cache_in_use()
            ));
        }
        out.join("\n")
    }

    /// Primitives with plane, cylinder, cone, sphere and torus faces.
    fn curved_bodies(k: &mut Kernel) -> Vec<Brep> {
        use crate::primitive::api_primitives as p;
        vec![
            p::create_box(k, Point3::new(1.0, -2.0, 3.0), 1.0, 2.0, 3.0).unwrap(),
            p::create_cylinder(k, Point3::new(0.5, 0.0, -1.0), 2.0, 3.0).unwrap(),
            p::create_cone(k, Point3::ZERO, 2.0, 1.0, 3.0).unwrap(),
            p::create_sphere(k, Point3::new(1.0, 2.0, -3.0), 2.0).unwrap(),
            p::create_torus(k, Point3::new(3.0, 0.0, -1.0), 4.0, 1.0).unwrap(),
        ]
    }

    /// On a body whose UV trims exist, each `_prepared` form gives the `&mut` form's
    /// answer bit for bit (one implementation) and changes nothing on the body.
    #[test]
    fn prepared_measures_match_the_mutable_ones_bit_for_bit() {
        let mut k = Kernel::default();
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        let requests = [
            MassPropertyRequest::centroidal(),
            MassPropertyRequest::api(2.0, Point3::new(0.5, -1.0, 2.0)),
        ];
        let abouts = [AreaAbout::FaceMiddle, AreaAbout::Point(Point3::ZERO)];
        for mut b in curved_bodies(&mut k) {
            // the trims (and what else a measure makes) made once, as on any measured body
            estimated(&k, &mut b, &requests[0]).unwrap();
            crate::topology::brep::p2::create_missing_uv_trim_curves(&k, &mut b).unwrap();
            assert_eq!(missing_uv_trim(&b, None).unwrap(), None);
            let before = trims_and_caches(&b);
            for request in &requests {
                let read =
                    brep_mass_properties_estimated_prepared(&k, &b, 1e-8, request, &attrs).unwrap();
                let mut copy = b.clone_standing_for_self().unwrap();
                let made =
                    brep_mass_properties_estimated(&k, &mut copy, 1e-8, request, &attrs).unwrap();
                assert_eq!(format!("{read:?}"), format!("{made:?}"));
            }
            for about in abouts {
                let read = brep_area_about_prepared(&k, &b, 1e-8, about).unwrap();
                let mut copy = b.clone_standing_for_self().unwrap();
                let made = brep_area_about(&k, &mut copy, 1e-8, about).unwrap();
                assert_eq!(read.to_bits(), made.to_bits());
            }
            for f in faces(&b).unwrap() {
                let mut copy = b.clone_standing_for_self().unwrap();
                let read = face_area_prepared(&k, &b, f, 1e-8).unwrap();
                let made = face_area(&k, &mut copy, f, 1e-8).unwrap();
                assert_eq!(read.to_bits(), made.to_bits());
                let read = face_centroid_prepared(&k, &b, f, 1e-8).unwrap();
                let made = face_centroid(&k, &mut copy, f, 1e-8).unwrap();
                assert_eq!(format!("{read:?}"), format!("{made:?}"));
            }
            assert_eq!(trims_and_caches(&b), before, "the body is unchanged");
        }
    }

    /// On a body with an edge whose uses have no UV trim, each `_prepared` form makes
    /// nothing and asks for preparation; [`missing_uv_trim`] names a use of that edge.
    #[test]
    fn prepared_measures_ask_for_a_missing_trim() {
        let mut k = Kernel::default();
        let store = no_attributes();
        let attrs = PropertyAttributes {
            store: &store,
            key_of: &|_| ObjectKey(0),
        };
        for mut b in curved_bodies(&mut k) {
            crate::topology::brep::p2::create_missing_uv_trim_curves(&k, &mut b).unwrap();
            let e = *edges(&b).unwrap().first().unwrap();
            let uses = edge::edgeuses(&b, e, None, None).unwrap();
            for &u in &uses {
                b.edgeuse_mut(u).unwrap().uv_trim_curve = None;
            }
            let missing = missing_uv_trim(&b, None).unwrap();
            assert!(missing.is_some_and(|u| uses.contains(&u)), "{missing:?}");
            let before = trims_and_caches(&b);
            let request = MassPropertyRequest::centroidal();
            let asks = |r: Result<()>| matches!(r, Err(SolidError::PreparationRequired { .. }));
            assert!(asks(
                brep_mass_properties_estimated_prepared(&k, &b, 1e-8, &request, &attrs).map(|_| ())
            ));
            assert!(asks(
                brep_area_about_prepared(&k, &b, 1e-8, AreaAbout::FaceMiddle).map(|_| ())
            ));
            for f in faces(&b).unwrap() {
                if missing_uv_trim(&b, Some(f)).unwrap().is_none() {
                    continue;
                }
                assert!(asks(face_area_prepared(&k, &b, f, 1e-8).map(|_| ())));
                assert!(asks(face_centroid_prepared(&k, &b, f, 1e-8).map(|_| ())));
            }
            assert_eq!(trims_and_caches(&b), before, "nothing is made");
            // the `&mut` form still makes the trim and measures
            estimated(&k, &mut b, &request).unwrap();
            assert_eq!(missing_uv_trim(&b, None).unwrap(), None);
        }
    }
}
