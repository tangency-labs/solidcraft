//! Primitive entry points through their topology construction dependencies.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`,
//! `source/SMLib/inc/SmPrimitiveCreation.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`create_rectangle`] | static `SmPrimitiveCreation::CreateRectangle` |
//! | [`create_circle`] | static `SmPrimitiveCreation::CreateCircle` |
//! | [`PrimitiveCreation::create_box`] | `SmPrimitiveCreation::CreateBox` |
//! | [`PrimitiveCreation::create_cylindrical_box`] | `CreateCylindricalBox` |
//! | [`PrimitiveCreation::create_cone`] | protected `CreateCone` |
//! | [`PrimitiveCreation::create_cylinder`] | `CreateCylinder` |
//! | `conic_solid` (private) | static `sm_CreateConicSolid` |
//! | [`PrimitiveCreation::create_sphere`] | protected `CreateSphere` |
//! | [`PrimitiveCreation::create_torus`] | protected `CreateTorus` |
//! | [`PrimitiveCreation::boolean_2d`] | `Boolean2D` |
//! | [`PrimitiveCreation::create_linear_sweep`] | protected curve `CreateLinearSweep` |
//! | [`PrimitiveCreation::create_sphere_no_pole`] | `CreateSphereNoPole` |
//! | [`PrimitiveCreation::create_cone_elliptic_ends`] | `CreateConeEllipticEnds` |
//! | [`analytic_rotational_sweep`] | `sm_CreateAnalyticRotationalSweep` |
//! | [`coplanar_with_axis`] | (not upstream, WR-776) |
//!
//! Deviations: missing topology edits return Unported at their first call.
//! Mutations after that call are deferred; no success is claimed for those paths.
//! Analytic cone and torus objects stay concrete until Surface dispatch supports
//! them. `SmBrep` is caller-owned; edits retain upstream edit-mode side effects.
//! Face construction uses caller notification records, attributes and identity keys.
//! No recursion.
//! Not upstream (WR-776): [`analytic_rotational_sweep`] makes its cone only for a line in
//! one plane with the axis ([`coplanar_with_axis`]); upstream makes the cone through the
//! end circles of a line skew to the axis too, which sweeps a hyperboloid of one sheet. A
//! skew line is refused here (a described failure, `SM_ERR` status) before any edit, so
//! the caller gives it its surface of revolution.
//! Not upstream (WR-18515): [`PrimitiveCreation::create_linear_sweep`] without end caps
//! stitches its walls in the first pass only ([`stitch_open`]); upstream's second pass
//! squeezed out a profile edge shorter than twenty times the first pass's tolerance.
//! Not upstream (WR-565, tangency-labs/solidcore#1477, opt-in
//! `Kernel::fixes.linear_sweep_arc_is_cylinder`; SolidCraft change, tangency-labs/smlib
//! 12918e9): [`PrimitiveCreation::create_linear_sweep`] makes a circular arc short of a
//! whole turn that is swept square to its plane a cylinder (`arc_sweep_cylinder`), where
//! upstream makes it an `SmSurfOfExtrusion`.
//! Not upstream (WR-576, tangency-labs/solidcore#1507; SolidCraft change,
//! tangency-labs/smlib fd59111): [`PrimitiveCreation::create_linear_sweep`] tests a profile curve for an arc
//! to the zero an `SmCylinder`'s own check holds its generating curve to (`arc_zero`),
//! not to the B-rep's tolerance.
//! Not upstream (SolidCraft #17462): a cylinder [`PrimitiveCreation::create_linear_sweep`]
//! makes from an arc within the B-rep's tolerance of square to the sweep has the sweep
//! direction as its axis and its origin in the profile's plane (`square_to_sweep`), not
//! the arc fit's normal, which a very short arc leaves off the sweep, when the squared
//! circle is certified to stay within the arc zero of the circle the arc test accepted:
//! the convex hull of the profile's control points close to that plane, and the circle's
//! in-plane change, `r (t² + |xa ya| / cx + ...)` plus its rounding, within what is
//! left. Otherwise the fit's frame stays, as upstream.
//! Not upstream (WR-596; the fork's tangency-labs/smlib 0df62c8, 35008b1, 449c81c):
//! [`PrimitiveCreation::create_box`] returns each face's and the stitch's status and skips
//! sides of zero extent, as the fork's `AddBoxFace` does; upstream drops them and returns
//! success with whatever faces were made.

use super::PrimitiveCreation;
use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::primitive::{AnalyticSweepRefusal, PrimitiveFailure};
use crate::error::{ignore_status, ignored, Failure, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::det;
use crate::math::Vec3;
use crate::math::{EFF_ZERO, EFF_ZERO_DEG};
use crate::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use crate::nurbs::b_spline_surface::p1::BSplineSurface;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::surface::cone::p1::Cone;
use crate::surface::plane::p1::Plane;
use crate::surface::sphere::Sphere;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};
use crate::topology::edge::NotifyEnv;
use crate::topology::{Brep, FaceId, FaceuseId, RegionId};

#[derive(Debug, Clone, Copy, PartialEq)]
struct SweepCurve(u8, crate::geom::extent1d::Extent1d);
impl crate::geom::solution_array::SolutionObject for SweepCurve {
    fn kind(&self) -> crate::geom::solution_array::ObjectKind {
        crate::geom::solution_array::ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<crate::geom::extent1d::Extent1d> {
        Some(self.1)
    }
}
fn intersection_options(
    a: crate::geom::extent1d::Extent1d,
    b: crate::geom::extent1d::Extent1d,
) -> crate::curve::curve::p8::CurveIntersectOptions<SweepCurve> {
    crate::curve::curve::p8::CurveIntersectOptions {
        handles: [Some(SweepCurve(0, a)), Some(SweepCurve(1, b))],
        analytic_handles: [Some(SweepCurve(2, a)), Some(SweepCurve(3, b))],
        ..Default::default()
    }
}

/// Operations on coincident planar regions (`Sm2DBooleanOperationType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boolean2d {
    /// Union.
    Union = 0,
    /// Intersection.
    Intersection = 1,
    /// A minus B.
    Difference = 2,
    /// Exclusive or.
    ExclusiveOr = 3,
    /// Merge, preserving internal edges.
    Merge = 4,
}

pub(super) fn new_brep(k: &mut Kernel) -> Result<Brep> {
    crate::topology::brep::p3::new_brep(
        k,
        crate::geom::tol::USE_DEFAULT,
        crate::geom::tol::USE_DEFAULT,
    )
}

/// Runs `body` with a face-construction environment over an arena-only notification
/// store (`ArenaNotifyState`): no observer holds identities for these B-reps' objects.
fn with_face_env<T>(
    k: &mut Kernel,
    b: &mut Brep,
    body: impl FnOnce(
        &mut Kernel,
        &mut Brep,
        &mut crate::topology::brep::p8::FaceBuildEnv<'_, '_>,
    ) -> Result<T>,
) -> Result<T> {
    // (not upstream) the kernel's reused record map, as the stitch's glues reuse it, so a
    // sweep's many faces do not each make one of every object of the B-rep
    let (tag, records) = crate::heal::stitch::p1::take_arena_records(k, b);
    let serial = k.context.issued_brep_serial(b.notify_identity());
    let mut state = crate::topology::brep::p2::ArenaNotifyState::reusing(records, b, serial);
    let mut env = state.env(None);
    let mut marks = crate::topology::topology_traverser::Marks::default();
    let this_face =
        |f: FaceId| crate::topology::loopuse::notify_ref(crate::error::EntityRef::Face(f));
    let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let r = body(k, b, &mut fenv);
    k.arena_notify_records = (tag, state.into_records());
    r
}

fn create_face(k: &mut Kernel, b: &mut Brep, surface: Surface, domain: Extent2d) -> Result<()> {
    create_face_id(k, b, surface, domain).map(|_| ())
}

fn stitch(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    stitch_with_keys(k, b, &crate::topology::loopuse::notify_ref)
}

fn stitch_with_keys(
    k: &mut Kernel,
    b: &mut Brep,
    key_of: &dyn Fn(crate::error::EntityRef) -> crate::geom::object::ObjectRef,
) -> Result<()> {
    stitch_faces_of(k, b, key_of, None)
}

/// Stitches the faces of a box's or a cylinder's solid. `outside` is a face use on the
/// solid's outside ([`side_away_from`]) when the B-rep was empty before the solid's faces
/// were made ([`is_empty`]), the solid is one the closing glue's ray leaves nothing in
/// ([`one_solid_as_built`]) and the faces say which side that is.
///
/// With it the glue that closes the solid's shell fires no ray to tell which half of the
/// region it splits is the infinite one
/// ([`stitch_and_orient_one_solid`](crate::topology::brep::p5::stitch_and_orient_one_solid)):
/// - the solid is alone, so the region split is the infinite one and the half outside the
///   solid is the infinite half; in a B-rep that holds anything else the solid may lie
///   inside another shell, and the closing glue's search also places the wire and vertex
///   shells already there;
/// - the solid is convex, so every point of it off a face's plane is on that face's inner
///   side, and the side of the face away from such a point is outside.
///
/// The stitch's end searches as ever. Without `outside` the solid is stitched as every
/// stitch is, and so is every cone.
fn stitch_solid(k: &mut Kernel, b: &mut Brep, outside: Option<FaceuseId>) -> Result<()> {
    stitch_faces_of(k, b, &crate::topology::loopuse::notify_ref, outside)
}

/// Whether `b` holds nothing but the one empty region a new B-rep has: no face, edge or
/// vertex, and no shell. A B-rep whose region list cannot be read is not that one: the
/// question raises no error of its own.
fn is_empty(b: &Brep) -> bool {
    let Ok(regions) = crate::topology::brep::p1::regions(b, None) else {
        return false;
    };
    let [region] = regions.as_slice() else {
        return false;
    };
    b.region(*region).is_ok_and(|r| r.shells.is_empty())
        && b.face_list().is_empty()
        && b.edge_list().is_empty()
        && b.vertex_list().is_empty()
}

/// Whether the closing glue's ray, besides naming the infinite half, leaves nothing in
/// the body that the stitch's end would not: the solid made on `frame` with `lengths` (a
/// box's three sides, or a cylinder's radius, twice, and its height) is met as built and
/// is of a size the B-rep's tolerance serves. The ray solves faces against the edited
/// B-rep and makes the UV trims of the faces it reaches
/// (`get_or_create_uv_trim_curve`), whose gaps may raise tolerances, and a ray that
/// fails leaves the region list as it found it. Outside these conditions old against new
/// over the review's probes changes such trims, tolerances and region lists; inside them
/// no body and no error changes:
/// - the frame's axes are unit and square to each other, so the stated lengths are the
///   distances between the solid's edges (a stored frame keeps the axes it is given);
/// - each length is beyond [`stitch_reach`]: the closing glue is the stitch's last edit
///   (a cylinder 1e-7 high glues and squeezes again after its shell closes);
/// - the solid is of a size the B-rep's tolerance serves ([`served_size`]): past it the
///   searches' ray solves fail, each on its own rays.
fn one_solid_as_built(b: &Brep, frame: &Axis2Placement, lengths: &[f64]) -> bool {
    let reach = stitch_reach(b);
    unit_frame(frame) && lengths.iter().all(|l| l.abs() > reach) && served_size(b, frame, lengths)
}

/// The model sizes upstream's table of zone tolerances lists
/// ([`size_estimate_to_zone_tol_3d`](crate::geom::tol::size_estimate_to_zone_tol_3d),
/// `SmTol::SizeEstimateToZoneTol3d`: its rows run from `0.00005` to `5e6`). Outside
/// them the table is a formula with no row behind it.
const LISTED_SIZES: core::ops::RangeInclusive<f64> = 0.00005..=5e6;

/// Whether the solid, which is the whole model (the B-rep was empty), is of a size
/// upstream's table lists ([`LISTED_SIZES`]) and the B-rep's tolerance is the one the
/// table gives a model of that size, or looser: `size_estimate_to_zone_tol_3d` of the
/// largest coordinate the solid can reach (the origin's largest plus its lengths) is no
/// more than the tolerance. At the default tolerance of 1e-5 that is a solid within 500
/// of the origin. It is the kernel's policy for which tolerance a model of a size is
/// built with, not a bound on what the searches compute.
fn served_size(b: &Brep, frame: &Axis2Placement, lengths: &[f64]) -> bool {
    let o = frame.origin();
    let far = o.x.abs().max(o.y.abs()).max(o.z.abs());
    let size = lengths.iter().fold(far, |sum, l| sum + l.abs());
    LISTED_SIZES.contains(&size)
        && crate::geom::tol::size_estimate_to_zone_tol_3d(size) <= b.tolerance()
}

/// Whether `frame`'s X and Y axes are unit vectors square to each other, to
/// [`EFF_ZERO`]: the frame a primitive's stated lengths are true lengths on.
fn unit_frame(frame: &Axis2Placement) -> bool {
    let (x, y) = (frame.x_axis(), frame.y_axis());
    (x.dot(x) - 1.0).abs() <= EFF_ZERO
        && (y.dot(y) - 1.0).abs() <= EFF_ZERO
        && x.dot(y).abs() <= EFF_ZERO
}

/// Twice the largest distance over which the primitives' stitch can glue two edges or two
/// vertices of a B-rep of this tolerance: its first pass runs at twice the B-rep's
/// tolerance and its second at twenty times that (`StitchAndOrient`), and each of the two
/// features may be moved by that much. The stitch caps its own tolerance for a loose
/// B-rep, but not the tolerances its edges and vertices carry, so the reach follows the
/// B-rep's tolerance uncapped: a box side of 0.04 in a B-rep of tolerance 0.1 is inside it.
fn stitch_reach(b: &Brep) -> f64 {
    2.0 * 20.0 * 2.0 * b.tolerance()
}

/// The face use of `face` on the side away from a point of the convex solid the face
/// bounds: the outside of the solid. The face's point is the one at the fractions `at` of
/// its UV box and the solid's the one at the fractions `inside` of the UV box of
/// `inside_face`; the surface's normal at the first says which of the face's two face
/// uses it is ([`OrientType::Same`] has the surface's normal).
///
/// `None` when the faces do not say: the normal has no part along the line between the
/// two points (a solid of no thickness), a surface is missing or cannot be evaluated
/// there, or the face lacks a face use. The solid is then stitched as every stitch is.
fn side_away_from(
    k: &Kernel,
    b: &Brep,
    (face, at): (FaceId, [f64; 2]),
    (inside_face, inside): (FaceId, [f64; 2]),
) -> Option<FaceuseId> {
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::nurbs::knots::Side;
    let record = b.face(face).ok()?;
    let surface = record.surface.as_ref()?;
    let uv = record.uv_domain.evaluate(at[0], at[1]);
    let point = surface.evaluate_point(k, uv).ok()?;
    let normal = surface
        .evaluate_normal(k, uv, Side::Right, Side::Right)
        .ok()?;
    let other = b.face(inside_face).ok()?;
    let within = other
        .surface
        .as_ref()?
        .evaluate_point(k, other.uv_domain.evaluate(inside[0], inside[1]))
        .ok()?;
    let first = record.faceuse?;
    let mate = b.faceuse(first).ok()?.mate?;
    let (same, opposite) = if b.faceuse(first).ok()?.orientation == OrientType::Same {
        (first, mate)
    } else {
        (mate, first)
    };
    let away = normal.dot(point - within);
    if away > 0.0 {
        Some(same)
    } else if away < 0.0 {
        Some(opposite)
    } else {
        None
    }
}

fn stitch_faces_of(
    k: &mut Kernel,
    b: &mut Brep,
    key_of: &dyn Fn(crate::error::EntityRef) -> crate::geom::object::ObjectRef,
    outside: Option<FaceuseId>,
) -> Result<()> {
    use crate::query::brep::BrepIdentity;
    use crate::topology::brep::p5::{
        stitch_and_orient_one_solid, stitch_and_orient_with_identity, StitchAndOrientOptions,
    };
    let keys = |object| crate::topology::brep::p2::caller_cache_key(key_of, object);
    let identity = Some(BrepIdentity {
        keys: &keys,
        ..BrepIdentity::default()
    });
    let options = StitchAndOrientOptions::default();
    match outside {
        Some(outside) => stitch_and_orient_one_solid(k, b, &options, identity, outside),
        None => stitch_and_orient_with_identity(k, b, &options, identity),
    }
}

/// Stitches the walls of an uncapped sweep, whose lamina edges are the profile's two
/// copies by construction, in the first pass only
/// ([`stitch_and_orient_open`](crate::topology::brep::p5::stitch_and_orient_open)): the
/// walls meet at the profile's shared curve ends, and the second pass would squeeze out
/// a profile edge shorter than its tolerance, leaving the wall's loop open. Not upstream
/// (WR-18515).
fn stitch_open(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    use crate::query::brep::BrepIdentity;
    use crate::topology::brep::p5::{stitch_and_orient_open, StitchAndOrientOptions};
    let keys = |object| {
        crate::topology::brep::p2::caller_cache_key(&crate::topology::loopuse::notify_ref, object)
    };
    let identity = Some(BrepIdentity {
        keys: &keys,
        ..BrepIdentity::default()
    });
    stitch_and_orient_open(k, b, &StitchAndOrientOptions::default(), identity)
}

/// `CreatePlanarFaceWith3DCurves`; `None` when the curves bound no face.
fn planar_face(
    k: &mut Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    curves: &[Curve],
    tolerance: f64,
) -> Result<Option<FaceId>> {
    with_face_env(k, b, |k, b, fenv| {
        crate::topology::brep::p8::create_planar_face_with_3d_curves(
            k, b, region, curves, tolerance, fenv,
        )
    })
}
pub(super) fn planar_faces(
    k: &mut Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    curves: &[Curve],
    tolerance: f64,
) -> Result<Vec<FaceId>> {
    with_face_env(k, b, |k, b, fenv| {
        crate::topology::brep::p8::create_planar_faces_with_3d_curves(
            k, b, region, curves, tolerance, fenv,
        )
    })
}
fn cone_face(k: &mut Kernel, b: &mut Brep, surface: &Cone, domain: Extent2d) -> Result<FaceId> {
    create_face_id(k, b, Surface::from(surface.clone()), domain)
}
fn torus_face(k: &mut Kernel, b: &mut Brep, surface: &Torus, domain: Extent2d) -> Result<FaceId> {
    create_face_id(k, b, Surface::from(surface.clone()), domain)
}
fn extrusion_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: &crate::surface::surf_of_extrusion::p1::SurfOfExtrusion,
    domain: Extent2d,
) -> Result<()> {
    create_face(k, b, Surface::from(surface.clone()), domain)
}
fn trimmed_face(
    k: &mut Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    curves: Vec<Curve>,
    surface: Surface,
    domain: Extent2d,
    notifications: &mut NotifyEnv<'_>,
) -> Result<FaceId> {
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    detailed_face(
        k,
        b,
        region,
        crate::topology::face::p2::FaceCurves {
            curve_loops: vec![4],
            curve_orients: vec![OrientType::Same; 4],
            curves_3d: Some(curves),
            ..Default::default()
        },
        surface,
        domain,
        notifications,
    )
}

fn create_face_id(
    k: &mut Kernel,
    b: &mut Brep,
    surface: Surface,
    domain: Extent2d,
) -> Result<FaceId> {
    with_face_env(k, b, |k, b, fenv| {
        crate::topology::brep::p8::create_face_from_surface(k, b, Some(surface), &domain, fenv)
    })
}
/// Face construction under the same caller identities as its trimmed neighbours.
fn create_face_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    surface: Surface,
    domain: Extent2d,
    notifications: &mut NotifyEnv<'_>,
) -> Result<FaceId> {
    let key_of = notifications.key_of;
    let mut marks = crate::topology::topology_traverser::Marks::default();
    let this_face = |face| key_of(crate::error::EntityRef::Face(face));
    let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
        env: notifications,
        marks: &mut marks,
        this_face: &this_face,
    };
    crate::topology::brep::p8::create_face_from_surface(k, b, Some(surface), &domain, &mut fenv)
}

/// `SmFace::GetEdges(rEdges)` with its defaults (`topology::face::p4::edges`).
fn face_edges(_k: &Kernel, b: &Brep, face: FaceId) -> Result<Vec<crate::topology::EdgeId>> {
    crate::topology::face::p4::edges(b, face, None, false, None)
}
pub(super) fn detailed_face(
    k: &mut Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    boundary: crate::topology::face::p2::FaceCurves,
    surface: Surface,
    domain: Extent2d,
    notifications: &mut NotifyEnv<'_>,
) -> Result<FaceId> {
    detailed_face_with_owners(
        k,
        b,
        region,
        boundary,
        surface,
        domain,
        notifications,
        &mut Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn detailed_face_with_owners(
    k: &mut Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    boundary: crate::topology::face::p2::FaceCurves,
    surface: Surface,
    domain: Extent2d,
    notifications: &mut NotifyEnv<'_>,
    owners: &mut Vec<Option<crate::topology::EdgeId>>,
) -> Result<FaceId> {
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::topology::brep::p7::{make_face_with_curve_owners, FaceCurvesInput};
    let loops = boundary
        .curve_loops
        .into_iter()
        .map(|count| {
            usize::try_from(count).map_err(|_| {
                SolidError::invalid_input(
                    "primitive::primitive_creation::p1::detailed_face",
                    "loop count exceeds usize",
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let input = FaceCurvesInput {
        loops,
        curves_3d: boundary
            .curves_3d
            .map(|curves| curves.into_iter().map(Some).collect()),
        uv_curves: boundary.uv_curves,
        orientations: boundary.curve_orients,
        loop_points: boundary.loop_points,
    };
    let key_of = notifications.key_of;
    make_face_with_curve_owners(
        k,
        b,
        region,
        input,
        surface,
        &domain,
        OrientType::Same,
        None,
        &|face| key_of(crate::error::EntityRef::Face(face)),
        owners,
        notifications,
    )?
    .face
    .ok_or_else(|| {
        SolidError::primitive_broken_link(
            "primitive::primitive_creation::p1::detailed_face",
            "the face maker succeeded without its new face, though it fails with \"MakeFace made no face\" when it made none",
        )
    })
}

/// `pSurface->DropCurve(ctx, pSurface->GetNaturalUVDomain(), *pCurve,
/// pCurve->GetNaturalInterval(), dTolerance, dMaxDist, dDeviation, rUVCurves)` with the
/// declaration's defaults: `output` is reset first (upstream `rUVCurves.ReSet()`) and
/// holds the UV curves only when the drop succeeds.
fn drop_uv_curves(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    tolerance: f64,
    output: &mut Vec<Curve>,
) -> Result<()> {
    output.clear();
    let dropped = surface.drop_curve::<NoObject>(
        k,
        &surface.natural_uv_domain_with_nurb(k)?,
        curve,
        &curve.natural_interval(),
        tolerance,
        true,
        None,
        true,
    )?;
    output.extend(dropped.uv_curves);
    Ok(())
}

enum AnalyticSurface {
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
}
fn analytic_face(k: &mut Kernel, b: &mut Brep, surface: AnalyticSurface) -> Result<FaceId> {
    match surface {
        AnalyticSurface::Cone(surface) => {
            cone_face(k, b, &surface, surface.rev.base().natural_uv_domain())
        }
        AnalyticSurface::Sphere(surface) => {
            let domain = surface.base().base().natural_uv_domain();
            sphere_face(k, b, &surface, domain)
        }
        AnalyticSurface::Torus(surface) => {
            let domain = surface.base().base().natural_uv_domain();
            torus_face(k, b, &surface, domain)
        }
    }
}

fn sphere_surface(k: &Kernel, frame: &Axis2Placement, radius: f64) -> Result<Sphere> {
    Sphere::create_canonical(k, frame, radius)
}

fn sphere_face(k: &mut Kernel, b: &mut Brep, surface: &Sphere, domain: Extent2d) -> Result<FaceId> {
    create_face_id(k, b, Surface::from(surface.clone()), domain)
}

/// Static rectangular-face constructor. The tolerance is only used by release
/// assertions upstream; the new B-rep inherits its context's tolerance.
pub fn create_rectangle(
    k: &mut Kernel,
    _tolerance: f64,
    width: f64,
    height: f64,
    frame: &Axis2Placement,
) -> Result<Brep> {
    let p00 = frame.origin();
    let p10 = p00 + frame.x_axis() * width;
    let p01 = p00 + frame.y_axis() * height;
    let p11 = p10 + p01 - p00;
    let base = BSplineSurface::create_bilinear_surface(p00, p10, p01, p11)?;
    let plane = Surface::from(base).copy_and_add_analytics(k)?;
    let mut brep = new_brep(k)?;
    let domain = plane.natural_uv_domain_with_nurb(k)?;
    ignore_status(create_face(k, &mut brep, plane, domain))?;
    Ok(brep)
}

// CreateCircle ignores the segment-constructor status (SE) and tests pCircle.
// An unavailable port must remain visible; a failed native allocation/geometry
// instead yields the static constructor's null result and its SM_ERR status.
fn circle_error(error: SolidError) -> SolidError {
    if error.is_unported_or_cancelled() {
        error
    } else {
        Failure::new(
            "primitive::primitive_creation::p1::create_circle",
            PrimitiveFailure::CircleNotMade,
        )
        .caused_by(error)
        .into_error()
    }
}
fn circle_curve(radius: f64, frame: &Axis2Placement) -> Result<BSplineCurve> {
    BSplineCurve::create_circle_segment(3, frame, radius, 0.0, 360.0, NurbCircleParam::Quadratic)
        .map_err(circle_error)
}

/// Static circular-face constructor. `tolerance` is passed to the planarity
/// check; it does not replace the context's B-rep tolerance.
pub fn create_circle(
    k: &mut Kernel,
    tolerance: f64,
    radius: f64,
    frame: &Axis2Placement,
) -> Result<Brep> {
    let mut brep = new_brep(k)?;
    let circle = circle_curve(radius, frame)?;
    brep.set_editing_enabled(true);
    let region = brep.infinite_region();
    if let Err(error) = planar_face(k, &mut brep, region, &[Curve::from(circle)], tolerance) {
        return Err(circle_error(error));
    }
    brep.set_editing_enabled(false);
    Ok(brep)
}

/// The six faces of [`PrimitiveCreation::create_box`] (`length`, `width`, `height`) and
/// their stitch; `propagate`: WR-596's statuses.
fn box_faces(
    k: &mut Kernel,
    brep: &mut Brep,
    [length, width, height]: [f64; 3],
    frame: &Axis2Placement,
) -> Result<()> {
    let alone = is_empty(brep) && one_solid_as_built(brep, frame, &[length, width, height]);
    let mut placement = Axis2Placement::new(frame.origin(), frame.x_axis(), frame.y_axis());
    let planes = [
        (frame.x_axis(), frame.y_axis(), length, width, height),
        (frame.y_axis(), frame.z_axis(), width, height, length),
        (frame.x_axis(), frame.z_axis(), length, height, -width),
    ];
    for (index, (x, y, u, v, offset)) in planes.into_iter().enumerate() {
        if index != 0 {
            placement.set_canonical(frame.origin(), x, y)?;
        }
        let domain = Extent2d::from_bounds(0.0, 0.0, u, v);
        for side in 0..2 {
            if side == 1 {
                placement.translate(placement.z_axis().unitize_or_z() * offset);
            }
            // `AddBoxFace`: no face on a side of zero extent; a plane or face that cannot be
            // made, or a face whose making fails after it was made, returns its status (the
            // face keeps its plane)
            if domain.x_length() == 0.0 || domain.y_length() == 0.0 {
                continue;
            }
            let mut plane = Plane::create_canonical(&placement)?;
            plane.adjust_step_uv_domain(&domain)?;
            let uv = plane.step_uv_domain();
            create_face(k, brep, Surface::from(plane), uv)?;
        }
    }
    // a box is convex: the outside of its first face is the side away from the middle of
    // its second, the face across the box from it
    let outside = match (alone, brep.face_list()) {
        (true, [first, across, ..]) => {
            side_away_from(k, brep, (*first, [0.5, 0.5]), (*across, [0.5, 0.5]))
        }
        _ => None,
    };
    stitch_solid(k, brep, outside)
}

impl PrimitiveCreation {
    /// Build six planar faces, then stitch and orient them (`CreateBox`).
    pub fn create_box(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        length: f64,
        width: f64,
        height: f64,
        frame: &Axis2Placement,
    ) -> Result<()> {
        brep.set_editing_enabled(true);
        // (WR-596: the fork's `AddBoxFace`, every status returned; editing is turned off
        // again on every way out)
        let result = box_faces(k, brep, [length, width, height], frame);
        brep.set_editing_enabled(false);
        result
    }

    /// Cylindrical box validation, followed by the first trimmed plane edit.
    /// The angle bounds are degrees, with exact inclusive bounds upstream.
    /// `notifications` supplies the caller's topology records, attributes and keys.
    pub fn create_cylindrical_box(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        dimensions: [f64; 5],
        frame: &Axis2Placement,
        notifications: &mut NotifyEnv<'_>,
    ) -> Result<()> {
        let [length, inner, outer, start, end] = dimensions;
        if !(length > EFF_ZERO
            && inner > EFF_ZERO
            && outer > inner + EFF_ZERO
            && (-360.0..=360.0).contains(&start)
            && end > start + EFF_ZERO
            && (0.0..=360.0).contains(&(end - start)))
        {
            return Err(SolidError::invalid_input(
                "primitive::primitive_creation::p1::create_cylindrical_box",
                "invalid length, radii or angular range",
            ));
        }
        // fork c8f6a31 (WR-662): every dropped status of CreateCylindricalBox is SER.
        brep.set_editing_enabled(true);
        let radians = core::f64::consts::PI / 180.0;
        let start_vec =
            frame.x_axis() * det::cos(start * radians) + frame.y_axis() * det::sin(start * radians);
        let mid = (start + end) / 2.0;
        let mid_vec =
            frame.x_axis() * det::cos(mid * radians) + frame.y_axis() * det::sin(mid * radians);
        let end_vec =
            frame.x_axis() * det::cos(end * radians) + frame.y_axis() * det::sin(end * radians);
        let delta = frame.z_axis() * length;
        let inner_points = [
            frame.origin() + start_vec * inner,
            frame.origin() + mid_vec * inner,
            frame.origin() + end_vec * inner,
        ];
        let outer_points = [
            frame.origin() + start_vec * outer,
            frame.origin() + mid_vec * outer,
            frame.origin() + end_vec * outer,
        ];
        let [a, b, c] = inner_points;
        let [d, e, f] = outer_points;
        let mut placement = *frame;
        for top in [false, true] {
            let (a, b, c, d, e, f) = if top {
                placement.translate(delta);
                (
                    a + delta,
                    b + delta,
                    c + delta,
                    d + delta,
                    e + delta,
                    f + delta,
                )
            } else {
                (a, b, c, d, e, f)
            };
            let mut plane = Plane::create_canonical(&placement)?;
            let domain = Extent2d::from_bounds(-outer, -outer, outer, outer);
            plane.adjust_step_uv_domain(&domain)?;
            let curves = vec![
                Curve::from(Line::from_points(a, d, 3)),
                Curve::from(Ellipse::circle_from_3_points(k, d, e, f, 3, false)?),
                Curve::from(Line::from_points(f, c, 3)),
                Curve::from(Ellipse::circle_from_3_points(k, c, b, a, 3, false)?),
            ];
            trimmed_face(
                k,
                brep,
                self.region(),
                curves,
                Surface::from(plane),
                domain,
                notifications,
            )?;
        }
        for radial in [start_vec, end_vec] {
            placement.set_canonical(frame.origin(), radial, frame.z_axis())?;
            let mut plane = Plane::create_canonical(&placement)?;
            plane.adjust_step_uv_domain(&Extent2d::from_bounds(inner, 0.0, outer, length))?;
            let domain = plane.natural_uv_domain();
            create_face_with_notifications(k, brep, Surface::from(plane), domain, notifications)?;
        }
        for radius in [inner, outer] {
            let mut cylinder = Cone::create_canonical_cylinder(k, frame, radius)?;
            cylinder.adjust_step_uv_domain(k, &Extent2d::from_bounds(start, 0.0, end, length))?;
            create_face_with_notifications(
                k,
                brep,
                Surface::from(cylinder.clone()),
                cylinder.rev.base().natural_uv_domain(),
                notifications,
            )?;
        }
        stitch_with_keys(k, brep, notifications.key_of)?;
        brep.set_editing_enabled(false);
        Ok(())
    }

    /// Full analytic cone; edit mode stays enabled, including on error.
    pub fn create_cone(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        height: f64,
        base: f64,
        top: f64,
        frame: &Axis2Placement,
    ) -> Result<()> {
        brep.set_editing_enabled(true);
        let cone = Cone::create_canonical(k, frame, base, top, height, false, false)?;
        self.conic_solid(k, brep, &cone, base, top, None)
    }

    /// Solid cylinder of `radius` and `height` on the frame (`CreateCylinder`): the
    /// canonical cylinder bounded to `[0, 360] x [0, height]` and its planar caps. A
    /// height below `SM_EFF_ZERO` or a failed bound is `SM_ERR`; edit mode ends
    /// disabled on success.
    ///
    /// # Errors
    ///
    /// `SM_ERR` as above; those of the cylinder construction and of the solid.
    pub fn create_cylinder(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        height: f64,
        radius: f64,
        frame: &Axis2Placement,
    ) -> Result<()> {
        const OP: &str = "primitive::primitive_creation::p1::create_cylinder";
        // empty entity maps - m_vMapToSame, m_vMapToHigher, m_vMapFromSame, and m_vMapFromLower
        self.init_entity_maps();

        if height < EFF_ZERO {
            return Err(SolidError::primitive(
                OP,
                PrimitiveFailure::HeightTooSmall {
                    height,
                    minimum: EFF_ZERO,
                },
            ));
        }

        brep.set_editing_enabled(true);

        // CreateCanonical makes an infinite cylinder; bound it to v in [0, dHeight]
        let mut cylinder = Cone::create_canonical_cylinder(k, frame, radius)?;
        let domain = Extent2d::from_bounds(0.0, 0.0, 360.0, height);
        match cylinder.adjust_step_uv_domain(k, &domain) {
            Ok(()) => {}
            Err(
                e @ (SolidError::Unported { .. }
                | SolidError::Cancelled { .. }
                | SolidError::OutOfFuel { .. }),
            ) => return Err(e),
            Err(e) => {
                return Err(Failure::new(OP, PrimitiveFailure::CylinderDomainNotSet)
                    .caused_by(e)
                    .into_error())
            }
        }

        self.conic_solid(k, brep, &cylinder, radius, radius, Some((height, frame)))?;

        brep.set_editing_enabled(false);
        Ok(())
    }

    /// `sm_CreateConicSolid`: the face of a bounded cone or cylinder surface and planar
    /// caps at its ends (each cap's edge curve made only for a cap that is built),
    /// stitched when a cap is made.
    fn conic_solid(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        cone: &Cone,
        base: f64,
        top: f64,
        cylinder: Option<(f64, &Axis2Placement)>,
    ) -> Result<()> {
        // (asked only for a cylinder, on its frame with its height: a cone's making is as
        // it was)
        let alone = cylinder.is_some_and(|(height, frame)| {
            is_empty(brep) && one_solid_as_built(brep, frame, &[base, top, height])
        });
        let domain = cone.rev.base().natural_uv_domain();
        let side = cone_face(k, brep, cone, domain)?;
        let tolerance = brep.tolerance();

        // make base face
        let base_face = if base > tolerance {
            // only make the edge curve for a cap that is built
            let base_edge = cone
                .create_iso_parametric_curve(k, SurfParamType::V, domain.min().y, None, false)?
                .curve;
            planar_face(k, brep, self.region(), &[base_edge], tolerance)?
        } else {
            None
        };
        let top_face = if top > tolerance {
            let top_edge = cone
                .create_iso_parametric_curve(k, SurfParamType::V, domain.max().y, None, false)?
                .curve;
            planar_face(k, brep, self.region(), &[top_edge], tolerance)?
        } else {
            None
        };
        if base_face.is_some() || top_face.is_some() {
            // a cylinder is convex: the outside of its side is away from the point of the
            // side half a turn on (a quarter and three quarters of the way round, clear of
            // the seam); a cone's solid is stitched as ever
            let outside = if alone {
                side_away_from(k, brep, (side, [0.25, 0.5]), (side, [0.75, 0.5]))
            } else {
                None
            };
            if cylinder.is_some() {
                stitch_solid(k, brep, outside)?;
            } else {
                stitch(k, brep)?;
            }
        }
        Ok(())
    }

    /// Full analytic sphere. Maps clear even if surface creation fails; the
    /// temporary edit mode restores on every exit.
    pub fn create_sphere(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        radius: f64,
        frame: &Axis2Placement,
    ) -> Result<()> {
        self.init_entity_maps();
        let editing = brep.editing_enabled();
        brep.set_editing_enabled(true);
        let result = (|| {
            let surface = sphere_surface(k, frame, radius)?;
            let domain = surface.base().base().natural_uv_domain();
            sphere_face(k, brep, &surface, domain).map(|_| ())
        })();
        brep.set_editing_enabled(editing);
        result
    }

    /// Full analytic torus, including the valid spindle representation.
    /// Maps clear and temporary edit mode restores even on constructor failure.
    pub fn create_torus(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        major: f64,
        minor: f64,
        frame: &Axis2Placement,
    ) -> Result<()> {
        self.init_entity_maps();
        let editing = brep.editing_enabled();
        brep.set_editing_enabled(true);
        let result = (|| {
            let torus =
                Torus::create_canonical(k, frame, major, minor, TorusOptions::default(), None)?;
            let domain = torus.base().base().natural_uv_domain();
            torus_face(k, brep, &torus, domain).map(|_| ())
        })();
        brep.set_editing_enabled(editing);
        result
    }

    /// Planar boolean's ordered collection and empty-input early return. The
    /// result slot remains untouched on the empty path, including Difference.
    /// `notifications` belongs to the primary B-rep and its newly created faces.
    pub fn boolean_2d(
        &mut self,
        k: &mut Kernel,
        a: &mut Brep,
        b: &mut Brep,
        operation: Boolean2d,
        output: &mut Option<Brep>,
        marks: (
            &mut crate::topology::topology_traverser::Marks,
            &mut crate::topology::topology_traverser::Marks,
        ),
        notifications: &mut NotifyEnv<'_>,
    ) -> Result<()> {
        Self::boolean_2d_keyed(k, a, b, operation, output, marks, notifications, None)
    }

    /// [`Self::boolean_2d`] with `b`'s objects notified under `other_key_of` (disjoint
    /// from `notifications.key_of`'s identities of `a`'s) when given, else under
    /// `notifications.key_of` as well.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn boolean_2d_keyed(
        k: &mut Kernel,
        a: &mut Brep,
        b: &mut Brep,
        operation: Boolean2d,
        output: &mut Option<Brep>,
        marks: (
            &mut crate::topology::topology_traverser::Marks,
            &mut crate::topology::topology_traverser::Marks,
        ),
        notifications: &mut NotifyEnv<'_>,
        other_key_of: Option<&dyn Fn(crate::error::EntityRef) -> crate::geom::object::ObjectRef>,
    ) -> Result<()> {
        // fork c8f6a31 (WR-662): Boolean2D's smgu_PlanePointDistance is SER.
        let previous = k.context.doing_boolean();
        let edit_a = a.editing_enabled();
        let edit_b = b.editing_enabled();
        let mut selected_result = false;
        k.context.set_doing_boolean(true);
        a.set_editing_enabled(true);
        b.set_editing_enabled(true);
        let result = (|| {
            let faces_a = crate::topology::brep::p1::faces(a, None)?;
            let faces_b = crate::topology::brep::p1::faces(b, None)?;
            let Some(&first) = faces_a.first() else {
                return Ok(());
            };
            if faces_b.is_empty() {
                return Ok(());
            }
            let op = BOOLEAN_2D;
            let tolerance = crate::math::max2(a.tolerance(), b.tolerance());
            let first_plane = a
                .face(first)?
                .surface
                .as_ref()
                .ok_or_else(|| SolidError::primitive(op, PrimitiveFailure::FaceWithoutSurface))?
                .is_planar(k, tolerance)?;
            if !first_plane.is_planar {
                return Err(SolidError::invalid_input(op, "non-planar first surface"));
            }
            for (brep, faces) in [
                (&*a, faces_a.get(1..).unwrap_or_default()),
                (&*b, faces_b.as_slice()),
            ] {
                for face in faces {
                    let other = brep
                        .face(*face)?
                        .surface
                        .as_ref()
                        .ok_or_else(|| {
                            SolidError::primitive(op, PrimitiveFailure::FaceWithoutSurface)
                        })?
                        .is_planar(k, tolerance)?;
                    if !other.is_planar {
                        return Err(SolidError::invalid_input(op, "non-planar surface"));
                    }
                    let d2 = crate::geom::geom_utility::p1::plane_point_distance(
                        first_plane.point,
                        first_plane.normal,
                        other.point,
                    )?;
                    let d1 = crate::geom::geom_utility::p1::plane_point_distance(
                        other.point,
                        other.normal,
                        first_plane.point,
                    )?;
                    if !(crate::math::max2(d2, d1) < tolerance * 2.0
                        && first_plane
                            .normal
                            .is_parallel_to(other.normal, EFF_ZERO_DEG))
                    {
                        return Err(SolidError::invalid_input(
                            op,
                            "surfaces are not coincident planes",
                        ));
                    }
                }
            }
            // extend the first surface's domain to cover all other face domains
            for (in_a, faces) in [
                (true, faces_a.get(1..).unwrap_or_default()),
                (false, faces_b.as_slice()),
            ] {
                for face in faces {
                    let brep: &Brep = if in_a { &*a } else { &*b };
                    let record = brep.face(*face)?;
                    let other_domain = record.uv_domain;
                    let other = record
                        .surface
                        .as_ref()
                        .ok_or_else(|| SolidError::primitive_broken_link(op, READ_SURFACE))?
                        .clone_for_evaluation()?;
                    let covered = cover_face_surface(
                        k,
                        a,
                        first,
                        &other,
                        tolerance * 2.0,
                        Some(&other_domain),
                    )?;
                    // Not upstream (WR-536): b's faces are rebuilt on copies of the first
                    // surface below: one it does not cover would carry edges far off its
                    // surface. Upstream ignores the failed cover.
                    if !covered {
                        return Err(SolidError::primitive(
                            op,
                            PrimitiveFailure::PlaneCannotCover,
                        ));
                    }
                }
            }
            // extend the rest of the surface domains to cover the first surface's domain
            let first_surface = a
                .face(first)?
                .surface
                .as_ref()
                .ok_or_else(|| SolidError::primitive_broken_link(op, FIRST_SURFACE))?
                .clone_for_evaluation()?;
            for (brep, faces) in [
                (&mut *a, faces_a.get(1..).unwrap_or_default()),
                (&mut *b, faces_b.as_slice()),
            ] {
                for face in faces {
                    let covered =
                        cover_face_surface(k, brep, *face, &first_surface, tolerance * 2.0, None)?;
                    // (WR-536) a plane that cannot be grown to cover the first face
                    if !covered {
                        return Err(SolidError::primitive(
                            op,
                            PrimitiveFailure::PlaneCannotCover,
                        ));
                    }
                }
            }
            super::boolean::finish(
                k,
                a,
                b,
                first,
                operation,
                output,
                marks,
                notifications,
                other_key_of,
            )?;
            selected_result = true;
            Ok(())
        })();
        k.context.set_doing_boolean(previous);
        a.set_editing_enabled(edit_a);
        b.set_editing_enabled(edit_b);
        if selected_result {
            if let Some(result) = output.as_mut() {
                result.set_editing_enabled(edit_a);
            }
        }
        result
    }
}

const BOOLEAN_2D: &str = "primitive::primitive_creation::p1::boolean_2d";
/// The first face's surface after the covers: `boolean_2d` read it before them, and
/// [`cover_face_surface`] puts the face's surface back on every exit.
/// A face's surface after `boolean_2d`'s planarity check: the check read the surface of
/// every face of both operands, and only the covers of the first face run in between.
const READ_SURFACE: &str = "a face of an operand lost its surface after the planarity check read the surface of every face of both operands, and only the first face's covers, which put it back, run in between";
const FIRST_SURFACE: &str = "the first face lost its surface in the covers, though boolean_2d read it before them and the cover puts it back on every exit";
/// A band patch's edge in `create_sphere_no_pole`: the face maker made it with its curve.
const PATCH_EDGE_CURVE: &str = "an edge of a sphere patch face made here has no curve, though the face maker gives each edge it makes its curve";
/// A band patch's edge's use on its face in `create_sphere_no_pole`: the edge is one of
/// that face's own.
const PATCH_EDGE_USE: &str = "an edge of a sphere patch face made here has no use on that face, though it was taken from that face's own edges";

/// `pFace->GetSurface()->CoverCoincidentSurface(pOther, tol, ..)` of `Boolean2D` on the
/// surface of face `face` of `brep`, followed by the `SmFace::RemoveUVTrimCurves`
/// (status ignored) a successful cover of a face-owned surface ends with. Returns
/// `bSuccess`, which upstream's `Boolean2D` ignores (WR-536 refuses on it).
///
/// Upstream's `STEPInversion` solves inside run on the face's own surface, whose
/// `GlobalPointSolve` classifies drops against the face (`GetFace()->PointClassify`).
/// The surface is moved out of its face for the call (keeping its owner, so the cache
/// it builds is its own) and lent with the B-rep through a face solve context; while it
/// is out, the face holds an evaluation copy of it for the B-rep reads of that cache
/// build. The real surface, possibly expanded, replaces the copy afterwards on every
/// exit, and the face tolerance a solve raised on the copy is synced back.
fn cover_face_surface(
    k: &Kernel,
    brep: &mut Brep,
    face: FaceId,
    other: &Surface,
    tol: f64,
    other_domain: Option<&Extent2d>,
) -> Result<bool> {
    use crate::query::brep::BrepFaceServices;
    use crate::topology::brep_cache::CacheObject;

    let Some(slot) = brep.face_mut(face)?.surface.as_mut() else {
        return Err(SolidError::primitive_broken_link(BOOLEAN_2D, READ_SURFACE));
    };
    let stand_in = slot.clone_for_evaluation()?;
    let mut surface = core::mem::replace(slot, stand_in);
    let r = surface.cover_coincident_surface_deferring_trims_with_context(
        k,
        other,
        tol,
        other_domain,
        (
            &mut BrepFaceServices::new(&mut *brep),
            Some(CacheObject::Surface(face)),
        ),
        true,
    );
    brep.face_mut(face)?.surface = Some(surface);
    brep.sync_face_surface_tolerance(face)?;
    let r = r?;
    if r.remove_uv_trim_curves_of.is_some() {
        // (passed on: the fork's `SER` in CoverCoincidentSurface, WR-663,
        // tangency-labs/smlib 21dcf69)
        crate::topology::face::p1::remove_uv_trim_curves(brep, face)?;
    }
    Ok(r.success)
}

/// Not upstream (WR-565, #1477, `fixes.linear_sweep_arc_is_cylinder`): the cylinder of
/// the circular arc of `frame` and `radius` from `start` to `end` degrees, swept square
/// to its plane over `distance` along the unit `sweep_dir`. The frame is turned about
/// its Z axis to the arc's start or, for a sweep against the arc's normal, to its end
/// and then flipped about its X axis, so that STEP u runs over the arc from 0 to its
/// sweep angle and v along the sweep from 0 to `distance`. `None` when the cylinder
/// cannot be made.
fn arc_sweep_cylinder(
    k: &Kernel,
    mut frame: Axis2Placement,
    radius: f64,
    (start, end): (f64, f64),
    sweep_dir: Vec3,
    distance: f64,
) -> Result<Option<Cone>> {
    let against = sweep_dir.dot(frame.z_axis()) < 0.0;
    let turn = if against { end } else { start };
    frame.rotate_about_axis_at_point(
        turn * core::f64::consts::PI / 180.0,
        frame.origin(),
        frame.z_axis(),
    );
    if against {
        frame.rotate_about_axis_at_point(
            180.0 * core::f64::consts::PI / 180.0,
            frame.origin(),
            frame.x_axis(),
        );
    }
    let Some(mut cylinder) = ignored(Cone::create_canonical_cylinder(k, &frame, radius))? else {
        return Ok(None);
    };
    // fork c8f6a31 (WR-662): the cylinder's AdjustSTEPUVDomain is SER.
    cylinder.adjust_step_uv_domain(k, &Extent2d::from_bounds(0.0, 0.0, end - start, distance))?;
    Ok(Some(cylinder))
}

/// Not upstream (#17462): turn an arc's frame, already within the B-rep's tolerance of
/// square to the sweep, exactly square to it, so the cylinder swept from it is the
/// translation surface the sweep defines, when that squared circle is certified to stay on
/// the profile. The frame of a short arc fitted from a planar curve is ill-conditioned: a
/// few hundredths of a degree of arc leave its normal up to `5e-8` off the plane's, and a
/// cylinder on that normal leans across the sweep, its far edge `distance` times that off
/// the swept curve and its rows no translates of each other.
///
/// Z becomes `sweep_dir` (or its negation `a`, keeping the frame's sense, so the arc's
/// angles still run the same way), X the old X's component across it, normalised, and the
/// origin moves along `a` into the plane `P` square to it through the curve's start. The
/// squared circle `C'` keeps the radius `r` and the angles of the fit's circle `C`. A point
/// `p` of the profile is then off `C'` by at most its offset along `a` from `P` plus the
/// in-plane distance between `C'(u)` and the projection of `C(u)` into `P`, on top of the
/// profile's own distance from `C` that the fit already accepted. Both terms must be
/// within `zero`, the arc zero the curve was fitted to, together:
///
/// - Along `a`: the convex hull of the curve's control points
///   ([`Curve::piece_hull_points`]) holds the curve, so the largest offset of a hull point
///   from `P` bounds it. Each point and the reference point `q` carry a coordinate error
///   bound `e`, `e_q`, worth `(e + e_q) * s` along `a` with `s = |ax| + |ay| + |az|`; the
///   subtraction and the dot product round by at most `γ₄ (|p|∞ + |q|∞) s`, `γ₄ ≤ 4 ε`.
///   A profile that really leans across the sweep (up to `2 r` times the tilt over a whole
///   circle, which the caller's `distance * tilt` check does not bound) fails here.
/// - In `P`: with `xa = X·a`, `ya = Y·a` (`xa² + ya² = t²`, `t = |Z × a|`), the projections
///   of `X` and `Y` into `P` are `X_p` (length `cx = √(1 − xa²)`, along X') and `Y_p`, whose
///   components along X' and Y' are `m = −xa·ya / cx` and `n`, `n² = 1 − ya² − m²`. Then
///   `|C'(u) − proj C(u)| = r |cos u (X' − X_p) + sin u (Y' − Y_p)|
///   ≤ r (|X' − X_p| + |Y' − Y_p|) ≤ r ((1 − cx) + |m| + (1 − n))
///   ≤ r (xa² + |m| + ya² + m²) = r (t² + |m| + m²)`, using `1 − √(1 − w) ≤ w`. A short arc
///   tilted about its chord stays within `zero` of `P` along `a` while missing `C'` by about
///   `r t² / 2` across it, so this term is checked on its own.
/// - Rounding of the in-plane term. Only `t² <= 1/4` is considered (a larger tilt is
///   never a fit's rounding; it keeps the fit's frame), so `|xa|, |ya| <= 1/2`,
///   `cx >= √3/2` and `|m| <= 0.29`. The terms of `B = t² + |m| + m²` are non-negative,
///   so the few products, the quotient and the sums that form it from the computed `xa`
///   and `ya` round by a relative `(1 + 16 ε)`. That does not cover the error of `xa` and
///   `ya` themselves: they are dot products of unit vectors that cancel when the tilt is
///   small, so their error is absolute, `|δ| <= γ₃ < 4 ε`, and unbounded relative to them.
///   Under `t² <= 1/4`, `|∂B/∂xa| + |∂B/∂ya| <= (1 + 0.77 + 0.45) + (1 + 0.58 + 0.34) < 5`,
///   so that error moves `r B` by at most `20 ε r`. `a` (unitized by the caller) and
///   `X'`, `Y'` (built by `unitize` and cross products in
///   [`Vec3::make_unit_ortho_vectors`]) are unit and orthogonal to within a few `ε`,
///   which moves `C'(u)` by at most `16 ε r`. With second-order terms, `64 ε r` covers
///   these absolute errors, and it is added to the bound.
/// - Skew of the stored frame. The derivation takes `X`, `Y` as orthonormal, and a stored
///   frame is not: [`Axis2Placement::set_canonical`] keeps `|X·Y|` up to `SM_EFF_ZERO` and
///   `|X·X − 1|` up to `SM_EFF_ZERO / 100` (a circle's `is_arc` returns that frame), and a
///   frame read from data may be anything. Let `X̂ = X / |X|` and `Ŷ` the unit part of `Y`
///   square to it. With `s = |X·Y|`, `|X − X̂| <= |X·X − 1|` and
///   `|Y − Ŷ| <= |Y·X̂| + ||Y − (Y·X̂) X̂| − 1| <= 2 s + |Y·Y − 1| + 4 s²` (for
///   `|X| >= 1/2`). So `k = |X·X − 1| + |Y·Y − 1| + 2 s + 4 s²` bounds
///   `|X − X̂| + |Y − Ŷ|`; its three dot products of near-unit vectors round by `γ₃ < 4 ε`
///   each, so `k + 16 ε` is used. `X'` is the same for `X` and `X̂` (only its direction is
///   used) and `Ŷ` gives the same `Z` direction, so `C'` is the squared circle of the
///   orthonormal frame `X̂, Ŷ`. The fit's circle `C` moves from that frame's by at most
///   `r k` (projection into `P` does not lengthen), and `xa`, `ya` from `X̂·a`, `Ŷ·a` by
///   at most `k` together, which moves `r B` by at most `2.3 r k` (the largest single
///   partial derivative above). `4 r k` is added; a frame with `k > 1e-6` keeps the
///   fit's frame.
///
/// A frame that fails either bound, or whose curve's kind has no certain hull, keeps the
/// fit's frame, as upstream. A frame already exactly square is left as it is.
fn square_to_sweep(
    frame: &mut Axis2Placement,
    curve: &Curve,
    radius: f64,
    sweep_dir: Vec3,
    zero: f64,
) -> Result<()> {
    let z = frame.z_axis();
    let axis = if sweep_dir.dot(z) < 0.0 {
        -sweep_dir
    } else {
        sweep_dir
    };
    if z == axis {
        return Ok(());
    }
    let Some(hull) = curve.piece_hull_points(&curve.natural_interval()) else {
        return Ok(());
    };
    let Some(&(on_plane, e_q)) = hull.first() else {
        return Ok(());
    };
    let spread = axis.x.abs() + axis.y.abs() + axis.z.abs();
    let max_abs = |p: Vec3| p.x.abs().max(p.y.abs()).max(p.z.abs());
    let gamma4 = 4.0 * f64::EPSILON;
    let mut axial = 0.0_f64;
    for &(p, e) in &hull {
        let off = (p - on_plane).dot(axis).abs()
            + (e + e_q) * spread
            + gamma4 * (max_abs(p) + max_abs(on_plane)) * spread;
        // Written so that a NaN (in `off` or `zero`) keeps the fit's frame.
        let within = off <= zero;
        if !within {
            return Ok(());
        }
        axial = axial.max(off);
    }
    let (x, y) = (frame.x_axis(), frame.y_axis());
    let s = x.dot(y).abs();
    let skew = (x.dot(x) - 1.0).abs()
        + (y.dot(y) - 1.0).abs()
        + 2.0 * s
        + 4.0 * s * s
        + 16.0 * f64::EPSILON;
    // A NaN keeps the fit's frame here too.
    let near_orthonormal = skew <= 1e-6;
    if !near_orthonormal {
        return Ok(());
    }
    let (xa, ya) = (x.dot(axis), y.dot(axis));
    let t2 = xa * xa + ya * ya;
    // A tilt past 30° is never a fit's rounding; a NaN keeps the fit's frame too.
    let small = t2 <= 0.25;
    if !small {
        return Ok(());
    }
    let cx = (1.0 - xa * xa).sqrt();
    let m = (xa * ya).abs() / cx;
    let r = radius.abs();
    let across = (r * (t2 + m + m * m) + 4.0 * r * skew) * (1.0 + 16.0 * f64::EPSILON)
        + 64.0 * f64::EPSILON * r;
    // As above, a NaN keeps the fit's frame.
    let within = axial + across <= zero;
    if !within {
        return Ok(());
    }
    let origin = frame.origin();
    let origin = origin + axis * (on_plane - origin).dot(axis);
    frame.set_step_canonical(origin, axis, frame.x_axis())
}

/// Not upstream (WR-576, #1507): the zero a
/// linear sweep tests a profile curve's arc to, the one an `SmCylinder`'s own check holds
/// its generating curve to: `SM_EFF_ZERO * 1000 * (1 + the largest coordinate of the
/// curve's ends)` (the fork's `ArcZero`). The ends' status is ignored, as `GetEnds`'
/// is there: ends that cannot be evaluated count as the origin.
fn arc_zero(k: &Kernel, curve: &Curve) -> Result<f64> {
    let (start, end) = match ignored(curve.ends(k))? {
        Some(ends) => (ends.start, ends.end),
        None => (Vec3::ZERO, Vec3::ZERO),
    };
    Ok(EFF_ZERO * 1000.0 * (1.0 + start.max_dimension().max(end.max_dimension())))
}

impl PrimitiveCreation {
    /// The curve overload of `CreateLinearSweep`. It consumes the curves, chooses
    /// a rectangular plane for an orthogonal line, a cylinder for a full circle
    /// swept along its axis, and an extrusion otherwise. Topology edits defer.
    /// With `k.fixes.linear_sweep_arc_is_cylinder` (not upstream, WR-565) an arc
    /// short of a full circle swept along its axis is a cylinder too.
    /// The arc test is to the cylinder check's zero (`arc_zero`, not upstream, WR-576),
    /// not the B-rep's tolerance.
    pub fn create_linear_sweep(
        &mut self,
        k: &mut Kernel,
        brep: &mut Brep,
        curves: Vec<Curve>,
        direction: Vec3,
        distance: f64,
        cap_ends: bool,
    ) -> Result<()> {
        use crate::geom::vector2d::Vec2;
        use crate::surface::surf_of_extrusion::p1::SurfOfExtrusion;
        // fork c8f6a31 (WR-662): CreateLinearSweep's dropped statuses are SER.
        let previous = k.context.doing_boolean();
        k.context.set_doing_boolean(true);
        let result = (|| {
            // Every surface and the end cap use this unit direction and a positive
            // length, so they meet. A negative distance sweeps against the direction.
            let mut sweep_dir = direction.unitize()?;
            let mut distance = distance;
            if distance < 0.0 {
                sweep_dir *= -1.0;
                distance = -distance;
            }
            let mut analytic_domain = Extent2d::from_bounds(0.0, 0.0, 1.0, distance);
            // A single full-circle sweep into an empty B-rep makes the same convex right cylinder
            // as `create_cylinder`, so `stitch_solid` can orient its shell without a closing ray.
            let was_empty = cap_ends && curves.len() == 1 && is_empty(brep);
            let mut single_cylinder: Option<FaceId> = None;
            for curve in &curves {
                let interval = match curve.kind() {
                    crate::curve::curve::CurveKind::Line(c) => c.step_interval(),
                    crate::curve::curve::CurveKind::Ellipse(c) => c.step_interval(),
                    crate::curve::curve::CurveKind::Hyperbola(c) => c.step_interval(),
                    crate::curve::curve::CurveKind::Parabola(c) => c.step_interval(),
                    _ => curve.natural_interval(),
                };
                analytic_domain.set_u_interval(&interval);
                let surface = if curve.is_linear(k, EFF_ZERO, false)?.linear {
                    let ends = curve.ends(k)?;
                    let tangent = (ends.end - ends.start).unitize_or_z();
                    if tangent.dot(sweep_dir).abs() < EFF_ZERO {
                        Some(Surface::from(Plane::new(
                            ends.start,
                            tangent,
                            sweep_dir,
                            Vec2::new((ends.end - ends.start).length(), distance),
                            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                        )))
                    } else {
                        None
                    }
                } else if let Some((mut frame, radius, start, end)) = curve.is_arc(
                    k,
                    8,
                    // Not upstream (WR-576, #1507): an arc only to the zero the
                    // cylinder's own check holds it to.
                    arc_zero(k, curve)?,
                )? {
                    let angle = end - start;
                    // A right cylinder only fits if the cap, translated along the
                    // sweep direction, is within tolerance of its far edge.
                    if angle < 360.0 + brep.tolerance()
                        && angle > 360.0 - brep.tolerance()
                        && distance * frame.z_axis().cross(sweep_dir).length() <= brep.tolerance()
                    {
                        square_to_sweep(&mut frame, curve, radius, sweep_dir, arc_zero(k, curve)?)?;
                        if sweep_dir.dot(frame.z_axis()) < 0.0 {
                            frame.rotate_about_axis_at_point(
                                180.0 * core::f64::consts::PI / 180.0,
                                frame.origin(),
                                frame.x_axis(),
                            );
                        }
                        if let Some(mut cylinder) =
                            ignored(Cone::create_canonical_cylinder(k, &frame, radius))?
                        {
                            cylinder.adjust_step_uv_domain(
                                k,
                                &Extent2d::from_bounds(0.0, 0.0, 360.0, distance),
                            )?;
                            let domain = cylinder.rev.base().natural_uv_domain();
                            let alone = was_empty
                                && one_solid_as_built(brep, &frame, &[radius, radius, distance]);
                            let side = cone_face(k, brep, &cylinder, domain)?;
                            if alone {
                                single_cylinder = Some(side);
                            }
                            continue;
                        }
                    }
                    // Not upstream (WR-565, #1477): with the fix, an arc short of a
                    // whole turn swept square to its plane is a cylinder too.
                    if k.fixes.linear_sweep_arc_is_cylinder
                        && angle > 0.0
                        && angle <= 360.0 - brep.tolerance()
                        && distance * frame.z_axis().cross(sweep_dir).length() <= brep.tolerance()
                    {
                        square_to_sweep(&mut frame, curve, radius, sweep_dir, arc_zero(k, curve)?)?;
                        if let Some(cylinder) =
                            arc_sweep_cylinder(k, frame, radius, (start, end), sweep_dir, distance)?
                        {
                            let domain = cylinder.rev.base().natural_uv_domain();
                            cone_face(k, brep, &cylinder, domain)?;
                            continue;
                        }
                    }
                    None
                } else {
                    None
                };
                let Some(surface) = surface else {
                    let copy = curve.copy(k)?;
                    let mut extrusion = SurfOfExtrusion::create_canonical(k, copy, sweep_dir)?;
                    extrusion.adjust_step_uv_domain(k, &analytic_domain)?;
                    let domain = extrusion.base().natural_uv_domain();
                    extrusion_face(k, brep, &extrusion, domain)?;
                    continue;
                };
                let domain = surface.natural_uv_domain_with_nurb(k)?;
                create_face(k, brep, surface, domain)?;
            }
            if cap_ends {
                planar_faces(k, brep, self.region(), &curves, brep.tolerance())?;
                let mut translation = Axis2Placement::IDENTITY;
                translation.translate(sweep_dir * distance);
                let mut end_curves = Vec::new();
                for curve in &curves {
                    let mut copy = curve.copy(k)?;
                    copy.transform(k, &translation, None)?;
                    end_curves.push(copy);
                }
                planar_faces(k, brep, self.region(), &end_curves, brep.tolerance())?;
                let outside = if brep.face_list().len() == 3 {
                    single_cylinder.and_then(|side| {
                        side_away_from(k, brep, (side, [0.25, 0.5]), (side, [0.75, 0.5]))
                    })
                } else {
                    None
                };
                stitch_solid(k, brep, outside)?;
            } else {
                // the profile's two copies stay open: the walls are a sheet
                stitch_open(k, brep)?;
            }
            brep.set_editing_enabled(false);
            Ok(())
        })();
        drop(curves);
        k.context.set_doing_boolean(previous);
        result
    }

    /// Assemble a sphere from two revolved centre patches and two trimmed caps,
    /// stitch at ten times the given tolerance, then translate to the centre.
    /// `notifications` supplies the caller's topology records, attributes and keys.
    pub fn create_sphere_no_pole(
        k: &mut Kernel,
        brep: &mut Brep,
        radius: f64,
        center: Vec3,
        tolerance: f64,
        notifications: &mut NotifyEnv<'_>,
    ) -> Result<()> {
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::nurbs::crv_basic::ConicType;
        use crate::nurbs::crv_common::{circ_arc, Arc};
        use crate::nurbs::srf_common::revolved_srf;
        use crate::topology::face::p2::FaceCurves;
        let op = "primitive::primitive_creation::p1::create_sphere_no_pole";
        // fork c8f6a31 (WR-662): DropCurve, MakeFaceWithCurves and Transform are SER.
        let start = -(det::atan(0.8_f64 / 0.6) * (180.0 / core::f64::consts::PI));
        let mut arc = circ_arc(
            &Arc {
                center: Vec3::ZERO,
                x: Vec3::X,
                y: Vec3::Y,
                r: radius,
                start,
                end: -start,
            },
            ConicType::Quadratic,
        )?;
        let mut patch = revolved_srf(&mut arc, Vec3::ZERO, Vec3::Y, 180.0, ConicType::Quadratic)?;
        let first_surface = BSplineSurface::from_gw_nurb(&patch);
        let first = create_face_with_notifications(
            k,
            brep,
            Surface::from(first_surface.clone()),
            first_surface.natural_uv_domain(),
            notifications,
        )?;
        let first_edges = face_edges(k, brep, first)?;
        patch.rotate_at_pt(Vec3::ZERO, Vec3::Y, 180.0)?;
        let second_surface = BSplineSurface::from_gw_nurb(&patch);
        let second = create_face_with_notifications(
            k,
            brep,
            Surface::from(second_surface.clone()),
            second_surface.natural_uv_domain(),
            notifications,
        )?;
        let second_edges = face_edges(k, brep, second)?;
        let mut dropped = Vec::new();
        for (rotation, index) in [(-90.0, 3), (180.0, 1)] {
            patch.rotate_at_pt(Vec3::ZERO, Vec3::X, rotation)?;
            let surface = Surface::from(BSplineSurface::from_gw_nurb(&patch));
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            let mut boundary = FaceCurves {
                curve_loops: vec![2],
                curves_3d: Some(Vec::new()),
                uv_curves: Some(Vec::new()),
                ..FaceCurves::default()
            };
            for (edges, face) in [(&first_edges, first), (&second_edges, second)] {
                let edge = *edges
                    .get(index)
                    .ok_or_else(|| SolidError::invalid_input(op, "sphere patch boundary order"))?;
                let original = brep
                    .edge(edge)?
                    .curve
                    .as_ref()
                    .ok_or_else(|| SolidError::primitive_broken_link(op, PATCH_EDGE_CURVE))?;
                let copy = original.copy(k)?;
                drop_uv_curves(k, &surface, &copy, tolerance, &mut dropped)?;
                let uv = dropped
                    .first()
                    .ok_or_else(|| {
                        SolidError::primitive(
                            op,
                            PrimitiveFailure::SphereEdgeNotDropped { tolerance },
                        )
                    })?
                    .copy(k)?;
                let edgeuse =
                    crate::topology::edge::upward_edgeuse_of_face(brep, edge, Some(face))?
                        .ok_or_else(|| SolidError::primitive_broken_link(op, PATCH_EDGE_USE))?;
                let orient = crate::topology::edgeuse::p1::orientation(brep, edgeuse)?;
                boundary.curve_orients.push(if orient == OrientType::Same {
                    OrientType::Opposite
                } else {
                    OrientType::Same
                });
                if let Some(curves) = boundary.curves_3d.as_mut() {
                    curves.push(copy);
                }
                if let Some(curves) = boundary.uv_curves.as_mut() {
                    curves.push(uv);
                }
            }
            detailed_face(
                k,
                brep,
                brep.infinite_region(),
                boundary,
                surface,
                domain,
                notifications,
            )?;
        }
        let keys =
            |object| crate::topology::brep::p2::caller_cache_key(notifications.key_of, object);
        let identity = crate::query::brep::BrepIdentity {
            keys: &keys,
            ..crate::query::brep::BrepIdentity::default()
        };
        let status = match crate::topology::brep::p4::stitch_faces_with_identity(
            k,
            brep,
            10.0 * tolerance,
            None,
            false,
            Some(identity),
        )
        .map(drop)
        {
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            other => other,
        };
        let mut translation = Axis2Placement::IDENTITY;
        translation.translate(center);
        crate::topology::brep::p5::transform_with_notifications(
            k,
            brep,
            &translation,
            None,
            true,
            Some(notifications),
        )?;
        status
    }

    /// Elliptic-ended cone. NLib errors map to `SM_ERR` before any B-rep edit (the error
    /// is the cause).
    pub fn create_cone_elliptic_ends(
        k: &mut Kernel,
        brep: &mut Brep,
        inputs: &crate::nurbs::srf_nurb::EllipticCone,
        cap_ends: bool,
    ) -> Result<()> {
        use crate::nurbs::b_spline_surface::p2::IsoCurveOptions;
        let nurb = crate::nurbs::srf_nurb::cone_elliptic_ends(inputs).map_err(|e| {
            Failure::new(
                "primitive::primitive_creation::p1::create_cone_elliptic_ends",
                PrimitiveFailure::ConeSurfaceNotMade,
            )
            .caused_by(e)
            .into_error()
        })?;
        let surface = BSplineSurface::from_gw_nurb(&nurb);
        create_face(
            k,
            brep,
            Surface::from(surface.clone()),
            surface.natural_uv_domain(),
        )?;
        if !cap_ends {
            return Ok(());
        }
        let mut front = surface
            .create_iso_parametric_curve(k, SurfParamType::U, 0.0, IsoCurveOptions::default())?
            .curve;
        let rear = surface
            .create_iso_parametric_curve(k, SurfParamType::U, 1.0, IsoCurveOptions::default())?
            .curve;
        let interval = front.natural_interval();
        // fork c8f6a31 (WR-662): SER(pFrontEllipse->ReverseParameterization(..)).
        front.reverse_parameterization(&interval)?;
        planar_face(
            k,
            brep,
            brep.infinite_region(),
            &[Curve::from(front)],
            0.001,
        )?;
        planar_face(k, brep, brep.infinite_region(), &[Curve::from(rear)], 0.001)?;
        stitch(k, brep)
    }
}

/// Whether the segment from `start` to `end` lies in one plane with the axis through
/// `axis_base` along `axis`, within `tol`: the lines are parallel (the sine of their
/// angle within [`EFF_ZERO`]), or their common perpendicular is no longer than `tol` (one
/// not finite is not). Only such a line, neither parallel nor square to the axis, sweeps a
/// cone; a skew one sweeps a hyperboloid of one sheet, its radius about the axis the root
/// of a quadratic in the axial coordinate, not linear in it (WR-776).
#[must_use]
pub(crate) fn coplanar_with_axis(
    axis_base: crate::math::Point3,
    axis: Vec3,
    start: crate::math::Point3,
    end: crate::math::Point3,
    tol: f64,
) -> bool {
    let line = end - start;
    let normal = line.cross(axis);
    let sine = normal.length();
    if sine <= EFF_ZERO * line.length() * axis.length() {
        return true;
    }
    ((start - axis_base).dot(normal) / sine).abs() <= tol
}

/// Create an analytic rotational sweep (`sm_CreateAnalyticRotationalSweep`).
/// Lines on the axis, interior axis crossings and unsuitable circular arcs return
/// `SM_ERR`; topology construction defers at its exact owning unit. Not upstream
/// (WR-776): a line skew to the axis and neither parallel nor square to it
/// ([`coplanar_with_axis`] at the B-rep's tolerance) fails, described, before any edit,
/// where upstream makes it the cone through its end circles.
pub fn analytic_rotational_sweep(
    k: &mut Kernel,
    brep: &mut Brep,
    curve: &Curve,
    axis_base: crate::math::Point3,
    axis: Vec3,
) -> Result<()> {
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::geom_utility::p1::{line_closest_point, line_point_distance};
    use crate::geom::solution_array::SolutionType;
    use crate::math::EFF_ZERO_SQRT;
    let op = ANALYTIC_SWEEP;
    // fork c8f6a31 (WR-662): AdjustSTEPUVDomain, GlobalCurveIntersect and
    // ComputeCrvsNormal are SER.
    let tol = brep.tolerance();
    let ends = curve.ends(k)?;
    let (start_distance, _) = line_point_distance(axis_base, axis, ends.start);
    let (end_distance, _) = line_point_distance(axis_base, axis, ends.end);
    let mut analytic = None;
    if curve.is_linear(k, EFF_ZERO, false)?.linear {
        let length = (ends.end - ends.start).length();
        if length < tol {
            return Err(SolidError::invalid_input(
                op,
                "line shorter than B-rep tolerance",
            ));
        }
        let line = (ends.end - ends.start) / length;
        let scale = crate::math::max4(start_distance, end_distance, length, EFF_ZERO_SQRT);
        let angle = tol / scale * (180.0 / core::f64::consts::PI);
        if line.is_parallel_to(axis, angle) {
            if start_distance <= tol {
                return Err(sweep_refused(AnalyticSweepRefusal::LineOnAxis));
            }
            let y = line.cross(ends.start - axis_base).unitize()?;
            let x = y.cross(line);
            let base = axis_base + axis * line_closest_point(axis_base, axis, ends.start);
            let frame = Axis2Placement::new(base, x, y);
            if let Some(mut cylinder) =
                ignored(Cone::create_canonical_cylinder(k, &frame, start_distance))?
            {
                cylinder
                    .adjust_step_uv_domain(k, &Extent2d::from_bounds(0.0, 0.0, 360.0, length))?;
                analytic = Some(AnalyticSurface::Cone(cylinder));
            }
        } else if line.is_perpendicular_to(axis, angle) {
            let base = axis_base + axis * line_closest_point(axis_base, axis, ends.start);
            let end_radius = line_point_distance(axis_base, axis, ends.end).0;
            let radial = if start_distance > end_radius {
                line * -1.0
            } else {
                line
            };
            let frame = Axis2Placement::new(base, radial, axis.cross(radial));
            let zero = crate::geom::tol::scaled_zero2(&ends.start, &ends.end);
            let mut circles = Vec::new();
            for radius in [start_distance, end_radius] {
                if radius > zero {
                    circles.push(Curve::from(Ellipse::create_canonical_circle(
                        k, &frame, radius, None, None,
                    )?));
                }
            }
            return match planar_face(k, brep, brep.infinite_region(), &circles, tol) {
                Ok(_) => Ok(()),
                Err(e) if e.is_unported_or_cancelled() => Err(e),
                Err(e) => Err(swept_face_not_made(e)),
            };
        } else {
            // Not upstream (WR-776): a line skew to the axis sweeps a hyperboloid, not the
            // cone through its end circles
            if !coplanar_with_axis(axis_base, axis, ends.start, ends.end, tol) {
                return Err(sweep_refused(AnalyticSweepRefusal::LineSkewToAxis));
            }
            let (bottom_radius, bottom_parameter) =
                line_point_distance(axis_base, axis, ends.start);
            let (top_radius, top_parameter) = line_point_distance(axis_base, axis, ends.end);
            let axis_line = Curve::from(Line::infinite(axis_base, axis, 3, false));
            let solutions = axis_line.global_curve_intersect(
                k,
                &axis_line.natural_interval(),
                curve,
                &curve.natural_interval(),
                tol,
                &intersection_options(axis_line.natural_interval(), curve.natural_interval()),
            )?;
            {
                if !solutions.is_empty() {
                    let first = solutions.get_at(0)?;
                    if first.solution_type == SolutionType::SingleValue {
                        let interval = curve.natural_interval();
                        let param_tol = EFF_ZERO_SQRT / 100.0 * (1.0 + interval.length());
                        let t = first.start.param(1)?;
                        if !((interval.min() - t).abs() < param_tol
                            || (interval.max() - t).abs() < param_tol)
                        {
                            return Err(sweep_refused(AnalyticSweepRefusal::CrossesAxis));
                        }
                    } else if first.solution_type == SolutionType::RangeOfValues {
                        return Err(sweep_refused(AnalyticSweepRefusal::RunsAlongAxis));
                    }
                }
            }
            let bottom = axis_line.evaluate_point(k, bottom_parameter)?;
            let top = axis_line.evaluate_point(k, top_parameter)?;
            let height = (bottom - top).length();
            let tmp = if axis.dot(line) < 0.0 {
                axis * -1.0
            } else {
                axis
            };
            let y = tmp.cross(ends.start - axis_base).unitize()?;
            let frame = Axis2Placement::new(bottom, y.cross(tmp), y);
            if let Some(cone) = ignored(Cone::create_canonical(
                k,
                &frame,
                bottom_radius,
                top_radius,
                height,
                false,
                false,
            ))? {
                analytic = Some(AnalyticSurface::Cone(cone));
            }
        }
    } else if let Some((frame, radius, start, end)) = curve.is_arc(k, 8, tol)? {
        let angle = end - start;
        if angle < 180.0 + tol && angle > 180.0 - tol {
            if start_distance > tol || end_distance > tol {
                let distance = if start_distance > tol {
                    start_distance
                } else {
                    end_distance
                };
                return Err(sweep_refused(AnalyticSweepRefusal::HalfCircleEndOffAxis {
                    distance,
                    tolerance: tol,
                }));
            }
            if let Some(sphere) = ignored(sphere_surface(k, &frame, radius))? {
                analytic = Some(AnalyticSurface::Sphere(sphere));
            }
        } else if angle < 360.0 + tol && angle > 360.0 - tol {
            let axis_line = Curve::from(Line::infinite(axis_base, axis, 3, false));
            let solutions = axis_line.global_curve_intersect(
                k,
                &axis_line.natural_interval(),
                curve,
                &curve.natural_interval(),
                tol,
                &intersection_options(axis_line.natural_interval(), curve.natural_interval()),
            )?;
            {
                if !solutions.is_empty() {
                    return Err(sweep_refused(AnalyticSweepRefusal::FullCircleMeetsAxis));
                }
            }
            let (major, param) = line_point_distance(axis_base, axis, frame.origin());
            let origin = axis_base + axis * param;
            let normal =
                crate::curve::curve::p3::compute_crvs_normal(k, core::slice::from_ref(curve))?;
            let frame = Axis2Placement::new(origin, normal, normal.cross(axis));
            if let Some(torus) = ignored(Torus::create_canonical(
                k,
                &frame,
                major,
                radius,
                TorusOptions::default(),
                None,
            ))? {
                analytic = Some(AnalyticSurface::Torus(torus));
            }
        } else {
            return Err(sweep_refused(AnalyticSweepRefusal::ArcNotHalfOrFull {
                angle,
            }));
        }
    } else {
        return Err(sweep_refused(AnalyticSweepRefusal::NotLineOrArc));
    }
    let Some(surface) = analytic else {
        return Err(SolidError::primitive(
            op,
            PrimitiveFailure::SweptSurfaceNotMade,
        ));
    };
    match analytic_face(k, brep, surface) {
        Ok(_) => Ok(()),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(e) => Err(swept_face_not_made(e)),
    }
}

const ANALYTIC_SWEEP: &str = "primitive::primitive_creation::p1::analytic_rotational_sweep";

/// The caller's curve, which [`analytic_rotational_sweep`] cannot revolve into one analytic
/// face ([`PrimitiveFailure::AnalyticSweepRefused`]).
#[track_caller]
fn sweep_refused(refusal: AnalyticSweepRefusal) -> SolidError {
    SolidError::primitive(
        ANALYTIC_SWEEP,
        PrimitiveFailure::AnalyticSweepRefused(refusal),
    )
}

/// The face [`analytic_rotational_sweep`] could not make on its surface
/// ([`PrimitiveFailure::SweptFaceNotMade`]), keeping the face's error as its cause.
#[track_caller]
fn swept_face_not_made(e: SolidError) -> SolidError {
    Failure::new(ANALYTIC_SWEEP, PrimitiveFailure::SweptFaceNotMade)
        .caused_by(e)
        .into_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::topology_traverser::Marks;
    use crate::topology::{Face, ShellId};

    /// #18415 review: a circle's stored frame may be skewed (`set_canonical` keeps
    /// `|X·Y|` up to `SM_EFF_ZERO`), which moves the circle by about `r |X·Y|`. That skew
    /// is counted in the in-plane bound: a frame skewed by 1e-12 at radius 1000 (a move
    /// of about 1e-9) stays as it is under a zero of 1e-9, where its orthonormal twin,
    /// at the same tilt, is squared.
    #[test]
    fn square_to_sweep_counts_a_skewed_frame() {
        let k = Kernel::default();
        let r = 1000.0;
        let flat = Axis2Placement::new(Vec3::ZERO, Vec3::X, Vec3::Y);
        let interval = crate::geom::extent1d::Extent1d::from_min_max(0.0, 10.0);
        let curve = Curve::from(
            Ellipse::create_canonical_circle(&k, &flat, r, Some(&interval), None).unwrap(),
        );
        let skewed = Axis2Placement::from_stored(Vec3::ZERO, Vec3::X, Vec3::new(1e-12, 1.0, 1e-15));
        let square = Axis2Placement::from_stored(Vec3::ZERO, Vec3::X, Vec3::new(0.0, 1.0, 1e-15));
        assert_eq!(skewed.x_axis().dot(skewed.y_axis()), 1e-12);
        assert!(skewed.z_axis() != Vec3::Z && square.z_axis() != Vec3::Z);
        let mut kept = skewed;
        square_to_sweep(&mut kept, &curve, r, Vec3::Z, 1e-9).unwrap();
        assert_eq!(kept, skewed, "a skewed frame is squared");
        let mut squared = square;
        square_to_sweep(&mut squared, &curve, r, Vec3::Z, 1e-9).unwrap();
        assert_eq!(
            squared.z_axis(),
            Vec3::Z,
            "the orthonormal twin is not squared"
        );
    }

    fn plane_brep(angle: f64) -> Brep {
        let angle = angle.to_radians();
        let frame = Axis2Placement::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(0.0, det::cos(angle), det::sin(angle)),
        );
        let mut b = Brep::new();
        let mut face = Face::new(ShellId::from_index(0));
        face.uv_domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let mut plane = Plane::create_canonical(&frame).unwrap();
        plane.adjust_step_uv_domain(&face.uv_domain).unwrap();
        face.surface = Some(Surface::from(plane));
        b.add_face_record(face).unwrap();
        b
    }

    /// A cylinder too short to build fails with the `SM_ERR` it replaced, before any
    /// B-rep edit, with a typed reason.
    #[test]
    fn a_flat_cylinder_is_a_primitive_failure() {
        let mut k = Kernel::default();
        let mut brep = new_brep(&mut k).unwrap();
        let faces = crate::topology::brep::p1::num_faces(&brep);
        let error = PrimitiveCreation::new(None, 17, true)
            .create_cylinder(&mut k, &mut brep, 0.0, 1.0, &Axis2Placement::default())
            .unwrap_err();
        assert_eq!(
            error.operation(),
            "primitive::primitive_creation::p1::create_cylinder"
        );
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(
            error.primitive_failure(),
            Some(&PrimitiveFailure::HeightTooSmall {
                height: 0.0,
                minimum: EFF_ZERO
            })
        );
        assert_eq!(error.kind(), crate::error::ErrorKind::InvalidInput);
        assert_eq!(crate::topology::brep::p1::num_faces(&brep), faces);
    }

    /// A curve the analytic rotational sweep cannot revolve into one analytic face is
    /// refused by name, of class `InvalidInput`, with the `SM_ERR` it replaced, before any
    /// B-rep edit.
    #[test]
    fn the_analytic_sweep_refuses_curves_by_name() {
        use AnalyticSweepRefusal as R;
        let refusal = |curve: Curve| {
            let mut k = Kernel::default();
            let mut brep = new_brep(&mut k).unwrap();
            let error = analytic_rotational_sweep(&mut k, &mut brep, &curve, Vec3::ZERO, Vec3::Z)
                .unwrap_err();
            assert_eq!(error.operation(), ANALYTIC_SWEEP);
            assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
            assert_eq!(
                error.kind(),
                crate::error::ErrorKind::InvalidInput,
                "{error}"
            );
            assert_eq!(crate::topology::brep::p1::num_faces(&brep), 0);
            match error.primitive_failure() {
                Some(&PrimitiveFailure::AnalyticSweepRefused(r)) => r,
                other => panic!("{other:?}: {error}"),
            }
        };
        let line =
            |a: Vec3, b: Vec3| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());
        assert_eq!(
            refusal(line(Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0))),
            R::LineOnAxis
        );
        assert_eq!(
            refusal(line(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0))),
            R::LineSkewToAxis
        );
        // arcs in the XZ plane about (5, 0, 0)
        let k = Kernel::default();
        let frame = Axis2Placement::new(Vec3::new(5.0, 0.0, 0.0), Vec3::X, Vec3::Z);
        let arc = |end: f64| {
            let interval = crate::geom::extent1d::Extent1d::from_min_max(0.0, end);
            Curve::from(
                Ellipse::create_canonical_circle(&k, &frame, 1.0, Some(&interval), None).unwrap(),
            )
        };
        match refusal(arc(90.0)) {
            R::ArcNotHalfOrFull { angle } => assert!((angle - 90.0).abs() < 1e-9, "{angle}"),
            other => panic!("{other:?}"),
        }
        match refusal(arc(180.0)) {
            R::HalfCircleEndOffAxis {
                distance,
                tolerance,
            } => assert!(
                (distance - 6.0).abs() < 1e-9 && distance > tolerance,
                "{distance}"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn failed_static_circle_geometry_reports_native_sm_err() {
        let frame = Axis2Placement::default();
        for radius in [-1.0, 0.0, EFF_ZERO * 0.5] {
            let error = circle_curve(radius, &frame).unwrap_err();
            assert_eq!(
                error.operation(),
                "primitive::primitive_creation::p1::create_circle"
            );
            assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
            assert_eq!(
                error.primitive_failure(),
                Some(&PrimitiveFailure::CircleNotMade)
            );
            assert!(error.cause().is_some(), "{error}");
        }
        let circle = circle_curve(0.5, &frame).unwrap();
        let point = circle
            .evaluate_point(circle.natural_interval().min())
            .unwrap();
        assert_eq!(point, Vec3::new(0.5, 0.0, 0.0));
    }

    /// A face of either operand without a surface is the caller's input: it is refused by
    /// name, of class `InvalidInput`, with the `SM_ERR_NULL_POINTER` it replaced.
    #[test]
    fn boolean_2d_refuses_a_face_without_a_surface() {
        for in_a in [true, false] {
            let mut k = Kernel::default();
            let mut records = crate::topology::topology::NotifyRecords::new();
            let mut attributes = crate::geom::attribute::AttributeStore::default();
            let mut notifications = NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &|_| unreachable!("the check stops before topology edits"),
            };
            let mut a = plane_brep(0.0);
            let mut b = plane_brep(0.0);
            let bare = if in_a { &mut a } else { &mut b };
            let face = bare.face_list()[0];
            bare.face_mut(face).unwrap().surface = None;
            let error = PrimitiveCreation::new(None, 17, true)
                .boolean_2d(
                    &mut k,
                    &mut a,
                    &mut b,
                    Boolean2d::Union,
                    &mut None,
                    (&mut Marks::default(), &mut Marks::default()),
                    &mut notifications,
                )
                .unwrap_err();
            assert_eq!(error.operation(), BOOLEAN_2D);
            assert_eq!(
                error.primitive_failure(),
                Some(&PrimitiveFailure::FaceWithoutSurface),
                "{error}"
            );
            assert_eq!(
                error.upstream_status(),
                Some(crate::error::SM_ERR_NULL_POINTER)
            );
            assert_eq!(error.kind(), crate::error::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn boolean_2d_covers_plane_domains_before_intersection() {
        let mut k = Kernel::default();
        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attributes = crate::geom::attribute::AttributeStore::default();
        let mut notifications = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &|_| unreachable!("validation stops before topology edits"),
        };
        let mut a = plane_brep(0.0);
        let mut b = plane_brep(0.0);
        let fa = a.face_list()[0];
        let fb = b.face_list()[0];
        let domain = Extent2d::from_bounds(-2.0, -3.0, 4.0, 5.0);
        b.face_mut(fb).unwrap().uv_domain = domain;
        b.face_mut(fb)
            .unwrap()
            .surface
            .as_mut()
            .unwrap()
            .adjust_step_uv_domain(&k, &domain)
            .unwrap();
        let mut marks_a = Marks::default();
        let mut marks_b = Marks::default();
        let error = PrimitiveCreation::new(None, 17, true)
            .boolean_2d(
                &mut k,
                &mut a,
                &mut b,
                Boolean2d::Union,
                &mut None,
                (&mut marks_a, &mut marks_b),
                &mut notifications,
            )
            .unwrap_err();
        // past IntersectInsertRelate: copying B's unrelated face needs its face use,
        // which the bare fixture face record lacks
        assert!(
            matches!(
                error,
                SolidError::InvalidInput {
                    operation: "topology::face::p2",
                    ..
                }
            ),
            "{error:?}"
        );
        let covered_a = a
            .face(fa)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_if_built();
        let covered_b = b
            .face(fb)
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .natural_uv_domain_if_built();
        assert!(covered_a.min().x <= -2.0 && covered_a.min().y <= -3.0);
        assert!(covered_a.max().x >= 4.0 && covered_a.max().y >= 5.0);
        assert_eq!(covered_a, covered_b);
    }

    #[test]
    fn boolean_2d_uses_vector_default_parallel_tolerance_and_restores_state() {
        for (angle, invalid) in [
            (0.0, false),
            (0.00005, false),
            (0.0002, true),
            (0.01, true),
            (180.0, false),
        ] {
            let mut k = Kernel::default();
            let mut records = crate::topology::topology::NotifyRecords::new();
            let mut attributes = crate::geom::attribute::AttributeStore::default();
            let mut notifications = NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &|_| unreachable!("validation stops before topology edits"),
            };
            let mut a = plane_brep(0.0);
            let mut b = plane_brep(angle);
            b.set_editing_enabled(true);
            let mut output = Some(Brep::new());
            let mut marks_a = Marks::default();
            let mut marks_b = Marks::default();
            let error = PrimitiveCreation::new(None, 17, true)
                .boolean_2d(
                    &mut k,
                    &mut a,
                    &mut b,
                    Boolean2d::Union,
                    &mut output,
                    (&mut marks_a, &mut marks_b),
                    &mut notifications,
                )
                .unwrap_err();
            // a coplanar pair goes on through IntersectInsertRelate and stops copying
            // B's bare face record (no face use); the others fail the coplanar check
            let expected = if invalid {
                "primitive::primitive_creation::p1::boolean_2d"
            } else {
                "topology::face::p2"
            };
            assert!(
                matches!(error, SolidError::InvalidInput { operation, .. } if operation == expected),
                "{angle}: {error}"
            );
            assert!(!a.editing_enabled());
            assert!(b.editing_enabled());
            assert!(!k.context.doing_boolean());
            assert!(output.is_some());
            assert!(a
                .face_list()
                .iter()
                .all(|f| a.face(*f).unwrap().surface.is_some()));
        }
    }
}
