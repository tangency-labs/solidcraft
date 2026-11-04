//! Which constant rounds are exact (opt-in, not upstream): a blend's faces as the natural
//! quadrics they lie on, the one classification of the curve the rolling ball's centre runs
//! on (the spine) from their kinds and placement, and the dispatch of a classified round to
//! its exact builder.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletExact.cpp`
//! and `source/SMLib/inc/SmFilletExact.h` (tangency-labs/smlib@b5f2867, in its final form at
//! tangency-labs/smlib@7ac037e), behind
//! [`UpstreamFixes::fillet_exact_round_classes`](crate::kernel::UpstreamFixes::fillet_exact_round_classes)
//! (WR-669).
//!
//! | Rust                               | Fork                                       |
//! |------------------------------------|--------------------------------------------|
//! | [`QuadricKind`]                    | `SmBlendQuadric::Kind`                     |
//! | [`BlendQuadric`]                   | `SmBlendQuadric`                           |
//! | [`BlendQuadric::from_surface`]     | `SmBlendQuadric::FromSurface`              |
//! | `quadric_of_surface` (private)     | `sm_QuadricOfSurface`                      |
//! | [`BlendQuadric::offset`]           | `SmBlendQuadric::Offset`                   |
//! | [`BlendQuadric::normal`]           | `SmBlendQuadric::Normal`                   |
//! | [`BlendQuadric::distance`]         | `SmBlendQuadric::Distance`                 |
//! | [`BlendQuadric::implicit`]         | `SmBlendQuadric::Implicit`                 |
//! | [`BlendQuadric::is_revolved`]      | `SmBlendQuadric::IsRevolved`               |
//! | [`BlendSpine`]                     | `SmBlendSpine`                             |
//! | [`ExactBlend`]                     | `SmExactBlend`                             |
//! | [`classify_blend_spine`]           | `sm_ClassifyBlendSpine`                    |
//! | `line_spine` (private), [`BlendQuadric::foot`] | none (#2405): the line spine's line and the faces' feet of it |
//! | [`classify_exact_blend`]           | `SmFilletSolver::ClassifyExactBlend`       |
//! | [`calc_exact_fillet_geom`]         | `SmFilletSolver::CalcExactFilletGeom`      |
//! | [`ball_zone_open_at_an_end`], [`CollapsedOffset`] | not the fork's (#2401): a collapsed offset's zone open at an end |
//! | `bending_toward_ball`, `focal_offset`, `edge_match`, `EdgeMatch` (private) | none (solidcore #2652) |
//! | `across`, `lines_meet`, `coaxial`, `inscribed_radius`, `pair_plane`, `points_with_contact`, `directions_with_dots`, `touching_point` (private) | `sm_Across`, `sm_LinesMeet`, `sm_Coaxial`, `sm_InscribedRadius`, `sm_PairPlane`, `sm_PointsWithContact`, `sm_DirectionsWithDots`, `sm_TouchingPoint` |
//! | [`classify_ball_on_collapsed_offset`], `curve_crosses_quadric` (private) | none (solidcore #2403) |
//! | `ball_on_face_along_arc` (private) | none (solidcore #2652) |
//!
//! Deviations:
//! - **A round of the ball's radius as a face (solidcore #2403, not in the fork).** With
//!   `fillet_blend_corner`, a torus is read too ([`QuadricKind::Torus`]; it has no class of
//!   its own), so that a torus moved in by its tube radius is its tube-centre circle
//!   ([`QuadricKind::Circle`]), as a cylinder so moved is its axis. An offset collapsed
//!   onto such a curve that the other offset crosses is a point spine
//!   ([`classify_ball_on_collapsed_offset`]): the round along an earlier round's end curve,
//!   where that round has the ball's radius, is a piece of the ball.
//! - **An offset collapsed by its geometry (solidcore #2652, not in the fork).** With
//!   `fillet_blend_corner` and a constant ball, a round on a face of no natural quadric (a
//!   B-spline, as a round of the ball's radius built by a trace) and a surface quadric is
//!   the ball's piece ([`BlendSpine::Point`]) where the geometry says the face is as wide as
//!   the ball: the ball's centre is the same at every point of the round and the ball lies
//!   on the face along its contact arc, within the solver's tolerance
//!   (`ball_on_face_along_arc`, decided where it is built); otherwise it is traced.
//! - **Out-params.** `FromSurface`, `ClassifyExactBlend` and `CalcExactFilletGeom` return
//!   their results (`rQuadric`, `rBlend`, `rbBuilt`); `Implicit` returns `(A, b, c)`.
//! - **Rational classes.** The conic, conic-pair and nodal-quartic classes are built
//!   exactly by [`rational::calc_rational_spine_fillet_geom`] (tangency-labs/smlib@0061c41,
//!   1b5f1e7, dda3b5d); a nodal-quartic round whose trace ends at the node stays traced,
//!   as the fork's.
//! - **A chamfer round a circle** (a linear section, class circle) is traced unless
//!   `fillet_chamfer_cone_on_arc` (WR-421) is on as well: the fork revolves it
//!   unconditionally; here that revolution stays behind its own flag.
//! - **Recognisers' statuses.** `SmSphere::IsNurbSurfaceSphere` returning `TRUE` without a
//!   sphere (its `SER`s) is "no sphere", as the fork's `&& pNurbSphere != NULL` reads it:
//!   [`Sphere::is_nurb_surface_sphere`]'s non-`Unported` errors.
//! - **The line spine's line (not the fork, #2405).** [`classify_blend_spine`] also gives a
//!   line spine its line, `O1 ∩ O2` in closed form (`line_spine`): two planes meet along
//!   `n1 x n2`, a plane parallel to a cylinder's axis and two cylinders with parallel axes
//!   along the axis through their sections' crossing nearest the classified centre. The
//!   line class's cylinder is built on it, its rails on the faces' feet of it
//!   ([`BlendQuadric::foot`]), where the fork takes the solved stations' centres and feet
//!   (2e-12 off the spine on the fork's oblique Ball).
//! - No recursion: `FromSurface` of an offset surface reads its base once, and a base that
//!   is itself an offset surface is read once more and no further (the solver's offset
//!   surfaces are at most two levels deep, as `CalcFilletGeom`'s cast of the base assumes);
//!   a deeper one is a surface of no kind.

use crate::blend::fillet_geom::p1::{FilletBrep, FilletGeomId};
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverOps, FilletSurfaceGeneratorType};
use crate::error::{Result, SolidError};
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::knots::Side;
use crate::surface::cone::Cone;
use crate::surface::procedural_blend::exact::{ExactClass, ExactRound};
use crate::surface::sphere::Sphere;
use crate::surface::surface::p2::PLANE_TYPE;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::torus::Torus;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{Brep, EdgeId};

pub mod rational;

const OP: &str = "blend::fillet_exact";

/// `SmBlendQuadric::Kind`: the natural quadric a surface lies on, or an offset of a
/// cylinder or sphere by its radius collapsed onto its axis (a line) or centre (a point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuadricKind {
    /// `SM_BQ_NONE`: no natural quadric.
    #[default]
    None,
    /// `SM_BQ_PLANE`.
    Plane,
    /// `SM_BQ_SPHERE`.
    Sphere,
    /// `SM_BQ_CYLINDER` (circular).
    Cylinder,
    /// `SM_BQ_CONE` (circular).
    Cone,
    /// `SM_BQ_LINE`: a cylinder moved in by its radius.
    Line,
    /// `SM_BQ_POINT`: a sphere moved in by its radius.
    Point,
    /// Not in the fork (solidcore #2403, with `fillet_blend_corner`): a torus, its tube of
    /// radius [`BlendQuadric::tube`] about the circle of radius [`BlendQuadric::radius`] about
    /// the axis. No class of its own: it is read so that its offset by its tube radius is
    /// known as [`QuadricKind::Circle`].
    Torus,
    /// Not in the fork (solidcore #2403): a torus moved in by its tube radius, the circle its
    /// tube is centred on.
    Circle,
}

/// `SmBlendQuadric`: a surface as the natural quadric it lies on, with the surface's own
/// normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendQuadric {
    /// `m_eKind`.
    pub kind: QuadricKind,
    /// `m_sPoint`: plane, a point on it; sphere, point: the centre; cylinder, line: a point
    /// of the axis; cone: the apex; torus, circle: the centre.
    pub point: Point3,
    /// `m_sDir`: plane: the surface's unit normal; cylinder, line: the unit axis; cone: the
    /// unit axis from the apex into the nappe the surface is on; torus, circle: the unit axis.
    pub dir: Vec3,
    /// `m_dRadius`: sphere, cylinder: the radius; torus, circle: the radius of the circle the
    /// tube is centred on.
    pub radius: f64,
    /// `m_dAngle`: cone: the half-angle in radians, in `(0, pi/2)`.
    pub angle: f64,
    /// `m_dSide`: sphere, cylinder, cone: `+1` when the surface's normal points away from the
    /// centre or axis, `-1` when toward it; torus: from the circle its tube is centred on.
    pub side: f64,
    /// Not in the fork (solidcore #2403): torus, the tube's radius.
    pub tube: f64,
}

impl Default for BlendQuadric {
    fn default() -> Self {
        Self {
            kind: QuadricKind::None,
            point: Point3::ZERO,
            dir: Vec3::ZERO,
            radius: 0.0,
            angle: 0.0,
            side: 1.0,
            tube: 0.0,
        }
    }
}

/// `sm_Across`: the component of `v` square to the unit `dir`.
fn across(v: Vec3, dir: Vec3) -> Vec3 {
    v - dir * v.dot(dir)
}

/// `SM_CAST_PTR(SmBSplineSurface, pSurface)`: the B-spline part of the kinds derived from
/// `SmBSplineSurface`, `None` for the others.
fn bspline_part<'a>(k: &Kernel, surface: &'a Surface) -> Result<Option<&'a BSplineSurface>> {
    Ok(match surface.kind_with_nurb(k)? {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base()),
        SurfaceKind::SurfOfExtrusion(r) => Some(r.base()),
        SurfaceKind::Cone(c) => Some(c.rev().base()),
        SurfaceKind::Sphere(s) => Some(s.base().base()),
        SurfaceKind::Torus(s) => Some(s.base().base()),
        // (the fork's SmProceduralBlend is an SmBSplineSurface: its net)
        SurfaceKind::ProceduralBlend(pb) => Some(pb.net()),
        // (the fork's SmBallPatch is an SmBSplineSurface too: its net)
        SurfaceKind::BallPatch(pb) => Some(pb.net()),
        _ => None,
    })
}

/// `pSurface->EvaluatePoint(crUV)` and `EvaluateNormal(crUV, TRUE, TRUE)`.
fn point_and_normal(k: &Kernel, surface: &Surface, uv: Point2) -> Result<(Point3, Vec3)> {
    let p = surface.evaluate_point(k, uv)?;
    let n = surface.evaluate_normal(k, uv, Side::Left, Side::Left)?;
    Ok((p, n))
}

/// Not the fork (solidcore #2652): a face's bending toward the ball at `uv`, read from its
/// geometry: the point, the unit normal toward the ball (the side `off`, the ball centre's
/// signed offset along the face's normal, is on), the principal curvatures toward the ball
/// (larger first: the roots of `(L - kE)(N - kG) - (M - kF)^2`) and the unit principal
/// direction of the larger. `None` at a pole (`Su x Sv = 0`) or where that direction is not
/// defined (an umbilic).
///
/// # Errors
///
/// Those of the evaluation.
fn bending_toward_ball(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    off: f64,
) -> Result<Option<Bending>> {
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
    };
    let d = surface.evaluate(k, uv, 2, 2, flags)?;
    let at = |i: usize, j: usize| d.get(i).and_then(|row| row.get(j)).copied();
    let (Some(s), Some(sv), Some(svv), Some(su), Some(suv), Some(suu)) =
        (at(0, 0), at(0, 1), at(0, 2), at(1, 0), at(1, 1), at(2, 0))
    else {
        return Ok(None);
    };
    let m = su.cross(sv);
    let dl = m.length();
    // (a NaN length is a pole too)
    if dl.is_nan() || dl <= EFF_ZERO * su.length() * sv.length() || dl <= 0.0 {
        return Ok(None);
    }
    let n = m * ((if off < 0.0 { -1.0 } else { 1.0 }) / dl);
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, mm, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
    let a = l * nn - mm * mm;
    let b = e * nn + g * l - 2.0 * f * mm;
    let c = e * g - f * f;
    let disc = b * b - 4.0 * a * c;
    let root = (if disc > 0.0 { disc } else { 0.0 }).sqrt();
    let (k_max, k_min) = ((b + root) / (2.0 * c), (b - root) / (2.0 * c));
    // the principal direction of k_max, from the larger row of II - k I
    let r0 = (l - k_max * e, mm - k_max * f);
    let r1 = (mm - k_max * f, nn - k_max * g);
    let (p, q) = if r0.0.abs() + r0.1.abs() >= r1.0.abs() + r1.1.abs() {
        r0
    } else {
        r1
    };
    let Ok(bending) = (su * (-q) + sv * p).unitize() else {
        return Ok(None);
    };
    Ok(Some(Bending {
        point: s,
        normal: n,
        k_max,
        k_min,
        direction: bending,
    }))
}

/// [`bending_toward_ball`]'s answer.
#[derive(Debug, Clone, Copy)]
struct Bending {
    point: Point3,
    /// The unit normal toward the ball.
    normal: Vec3,
    /// The larger principal curvature toward the ball.
    k_max: f64,
    /// The smaller.
    k_min: f64,
    /// The unit principal direction of `k_max`.
    direction: Vec3,
}

/// Not the fork (solidcore #2652): the offset by the ball (`off`) of a face of no natural
/// quadric at `uv` where its larger radius of curvature toward the ball is the ball's
/// (within `tol`): its focal curve's tangent line through the ball's centre there
/// ([`QuadricKind::Line`], along the face's other principal direction), or the centre
/// ([`QuadricKind::Point`]) where both are. `None` where the offset does not collapse
/// there.
///
/// # Errors
///
/// Those of the evaluation.
fn focal_offset(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    off: f64,
    tol: f64,
) -> Result<Option<BlendQuadric>> {
    let Some(Bending {
        point: s,
        normal: n,
        k_max,
        k_min,
        direction: bending,
    }) = bending_toward_ball(k, surface, uv, off)?
    else {
        return Ok(None);
    };
    let radius = off.abs();
    if k_max.is_nan() || k_max <= 0.0 || (1.0 / k_max - radius).abs() > tol {
        return Ok(None);
    }
    let centre = s + n * radius;
    if k_min > 0.0 && (1.0 / k_min - radius).abs() <= tol {
        return Ok(Some(BlendQuadric {
            kind: QuadricKind::Point,
            point: centre,
            ..BlendQuadric::default()
        }));
    }
    let Ok(dir) = n.cross(bending).unitize() else {
        return Ok(None);
    };
    Ok(Some(BlendQuadric {
        kind: QuadricKind::Line,
        point: centre,
        dir,
        ..BlendQuadric::default()
    }))
}

/// Not the fork (solidcore #2652): how a face of no natural quadric matches a ball of
/// radius `|off|` along an edge (`curve` over `ivl`, on the face): [`EdgeMatch::Full`] where
/// its larger radius of curvature toward the ball is the ball's (within `tol`) at each of
/// 17 stations, [`EdgeMatch::Partial`] with its least radius there where that holds at some
/// stations only or the radius crosses the ball's between two, [`EdgeMatch::None`]
/// otherwise (or where a station does not drop onto the face).
///
/// # Errors
///
/// Those of the evaluations and the drops.
fn edge_match(
    k: &Kernel,
    surface: &Surface,
    curve: &crate::curve::curve::Curve,
    ivl: crate::geom::extent1d::Extent1d,
    off: f64,
    tol: f64,
) -> Result<EdgeMatch> {
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;
    const STATIONS: u32 = 16;
    let radius = off.abs();
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let (mut all, mut some, mut crosses) = (true, false, false);
    let mut side = 0.0_f64;
    let mut least = BIG_DOUBLE;
    for i in 0..=STATIONS {
        let v = curve.evaluate_point(k, ivl.evaluate(f64::from(i) / f64::from(STATIONS)))?;
        let dropped =
            surface.drop_point::<NoObject>(k, v, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(EdgeMatch::None);
        }
        let rho = match bending_toward_ball(k, surface, dropped.result_uv, off)? {
            Some(b) if b.k_max > 0.0 => 1.0 / b.k_max,
            _ => BIG_DOUBLE,
        };
        least = least.min(rho);
        if (rho - radius).abs() <= tol {
            some = true;
            continue;
        }
        all = false;
        let here = if rho < radius { -1.0 } else { 1.0 };
        #[allow(clippy::float_cmp)] // the +-1 signs
        if side != 0.0 && here != side {
            crosses = true;
        }
        side = here;
    }
    Ok(if all {
        EdgeMatch::Full
    } else if some || crosses {
        EdgeMatch::Partial(least)
    } else {
        EdgeMatch::None
    })
}

/// [`edge_match`]'s answer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EdgeMatch {
    /// The face is as wide as the ball along the whole edge.
    Full,
    /// Along part of it, the face's least radius of curvature toward the ball given.
    Partial(f64),
    /// Nowhere.
    None,
}

/// The cone's data (`GetZAxis`, `GetRadius(0)`, `GetRadius(1)`, `IsCylinder`,
/// `GetOrigin`).
struct ConeData {
    z: Vec3,
    r0: f64,
    r1: f64,
    cylinder: bool,
    origin: Point3,
}

impl ConeData {
    fn of(c: &Cone) -> Self {
        let frame = c.canonical().origin;
        Self {
            z: frame.z_axis(),
            r0: c.radius(0.0),
            r1: c.radius(1.0),
            cylinder: c.is_cylinder(),
            origin: frame.origin(),
        }
    }
}

/// A torus's data (its placement's origin and Z axis, its major and minor radii).
struct TorusData {
    origin: Point3,
    axis: Vec3,
    major: f64,
    minor: f64,
}

impl TorusData {
    fn of(t: &Torus) -> Self {
        let frame = t.base.position;
        Self {
            origin: frame.origin(),
            axis: frame.z_axis(),
            major: t.major_radius,
            minor: t.minor_radius,
        }
    }
}

/// `sm_QuadricOfSurface`: the natural quadric of an analytic surface (not an offset
/// surface), oriented by its normal at `uv`, or [`QuadricKind::None`]; with
/// `fillet_blend_corner`, a torus as well (solidcore #2403).
fn quadric_of_surface(k: &Kernel, surface: &Surface, uv: Point2, tol: f64) -> Result<BlendQuadric> {
    let mut q = BlendQuadric::default();

    let (p, n) = point_and_normal(k, surface, uv)?;

    let Some(bss) = bspline_part(k, surface)? else {
        return Ok(q);
    };
    // (the recognisers' corner points come from the virtual EvaluatePoint: a procedural
    // blend's definition)
    let this = matches!(surface.kind(), SurfaceKind::ProceduralBlend(_)).then_some(surface);

    // the analytic kinds, or a B-spline that is one (a plane, a cone or cylinder, a sphere)
    let mut cone = match surface.kind() {
        SurfaceKind::Cone(c) => Some(ConeData::of(c)),
        _ => None,
    };
    let mut sphere = match surface.kind() {
        SurfaceKind::Sphere(s) => Some((s.base().position().origin(), s.radius())),
        _ => None,
    };
    // Not in the fork (solidcore #2403): with `fillet_blend_corner` a torus is read too, as
    // the last kind (it has no class of its own; see `QuadricKind::Torus`)
    let tori = k.fixes.fillet_blend_corner;
    let mut torus = match surface.kind() {
        SurfaceKind::Torus(t) if tori => Some(TorusData::of(t)),
        _ => None,
    };
    let mut plane = surface.type_code() == PLANE_TYPE;
    if !plane && cone.is_none() && sphere.is_none() && torus.is_none() {
        if surface.is_planar(k, tol)?.is_planar {
            plane = true;
        } else if let Some(c) = Cone::is_nurb_surface_cone_on(k, bss, 1.0, this)? {
            cone = Some(ConeData::of(&c));
        } else {
            match Sphere::is_nurb_surface_sphere_on(k, bss, 1.0, this) {
                Ok(Some(s)) => sphere = Some((s.base().position().origin(), s.radius())),
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                // no sphere, or TRUE without a sphere (its SER statuses)
                Ok(None) | Err(_) => {}
            }
            if sphere.is_none() && tori {
                match Torus::is_nurb_surface_torus(k, bss, 1.0) {
                    Ok(Some(t)) => torus = Some(TorusData::of(&t)),
                    Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                        return Err(e)
                    }
                    // no torus, or TRUE without a torus (its SER statuses)
                    Ok(None) | Err(_) => {}
                }
            }
        }
    }

    if plane {
        q.kind = QuadricKind::Plane;
        q.point = p;
        q.dir = n;
    } else if let Some(c) = cone {
        // Rad(v) = R0 + tan(semi-angle) v about the axis through the origin along z
        let z = c.z.unitize()?;
        let r0 = c.r0;
        let tan_b = c.r1 - r0;
        if c.cylinder {
            q.kind = QuadricKind::Cylinder;
            q.point = c.origin;
            q.dir = z;
            q.radius = r0.abs();
        } else {
            q.kind = QuadricKind::Cone;
            q.point = c.origin + z * (-r0 / tan_b);
            q.dir = if tan_b > 0.0 { z } else { -z };
            q.angle = tan_b.abs().atan();
        }
        // the nappe the surface point is on
        if q.kind == QuadricKind::Cone && (p - q.point).dot(q.dir) < 0.0 {
            q.dir = -q.dir;
        }
        let out = q.normal(p);
        q.side = if out.dot(n) >= 0.0 { 1.0 } else { -1.0 };
    } else if let Some((origin, radius)) = sphere {
        q.kind = QuadricKind::Sphere;
        q.point = origin;
        q.radius = radius;
        q.side = if (p - q.point).dot(n) >= 0.0 {
            1.0
        } else {
            -1.0
        };
    } else if let Some(t) = torus {
        q.kind = QuadricKind::Torus;
        q.point = t.origin;
        q.dir = t.axis.unitize()?;
        q.radius = t.major;
        q.tube = t.minor;
        q.side = if q.normal(p).dot(n) >= 0.0 { 1.0 } else { -1.0 };
        // a torus read off the surface that is not it is no torus: the torus is read only to
        // tell a round of the ball's radius, never an error of the kinds the fork reads
        if q.distance(p) > tol || q.normal(p).dot(n) < 1.0 - EFF_ZERO_SQRT {
            return Ok(BlendQuadric::default());
        }
        return Ok(q);
    } else {
        return Ok(q);
    }

    // what was read off the surface is the surface
    if q.distance(p) > tol || q.normal(p).dot(n) < 1.0 - EFF_ZERO_SQRT {
        return Err(SolidError::sm_err(OP));
    }
    Ok(q)
}

impl BlendQuadric {
    /// `SmBlendQuadric::FromSurface`: the natural quadric `surface` lies on
    /// ([`QuadricKind::None`] if none), oriented by its normal at `uv`; an offset surface is
    /// its base's quadric moved by its offset there.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a surface of a quadric's kind that lies farther than `tol` from the
    /// quadric read off it at `uv`; those of the evaluations and recognisers.
    pub fn from_surface(k: &Kernel, surface: &Surface, uv: Point2, tol: f64) -> Result<Self> {
        Self::from_surface_at_depth(k, surface, uv, tol, 0)
    }

    fn from_surface_at_depth(
        k: &Kernel,
        surface: &Surface,
        uv: Point2,
        tol: f64,
        depth: usize,
    ) -> Result<Self> {
        let SurfaceKind::OffsetSurface(offset) = surface.kind() else {
            return quadric_of_surface(k, surface, uv, tol);
        };
        // bound (see the header): two levels of offset
        if depth >= 2 {
            return Ok(Self::default());
        }
        let base = offset.base_surface();
        let base_q = Self::from_surface_at_depth(k, base, uv, tol, depth + 1)?;
        if base_q.kind == QuadricKind::None {
            return Ok(Self::default());
        }

        // the offset there, along the base's normal
        let (p, n) = point_and_normal(k, base, uv)?;
        let q = surface.evaluate_point(k, uv)?;
        Ok(base_q.offset((q - p).dot(n), tol))
    }

    /// `SmBlendQuadric::Offset`: this quadric moved `dist` along its normal (a cylinder or
    /// sphere moved in by its radius, within `tol`, is its axis or its centre).
    #[must_use]
    pub fn offset(&self, dist: f64, tol: f64) -> Self {
        let mut o = *self;
        match self.kind {
            QuadricKind::Plane => {
                o.point = self.point + self.dir * dist;
            }
            QuadricKind::Sphere | QuadricKind::Cylinder => {
                o.radius = self.radius + self.side * dist;
                if o.radius.abs() <= tol {
                    o.kind = if self.kind == QuadricKind::Sphere {
                        QuadricKind::Point
                    } else {
                        QuadricKind::Line
                    };
                    o.radius = 0.0;
                } else if o.radius < 0.0 {
                    // moved in past its centre or axis: the same surface the other way round
                    o.radius = -o.radius;
                    o.side = -self.side;
                }
            }
            QuadricKind::Cone => {
                // the normal away from the axis is cos(a) radial - sin(a) d: the apex moves
                // back along d
                o.point = self.point - self.dir * (self.side * dist / self.angle.sin());
            }
            // (solidcore #2403) a torus moved in by its tube radius is its tube's circle
            QuadricKind::Torus => {
                o.tube = self.tube + self.side * dist;
                if o.tube.abs() <= tol {
                    o.kind = QuadricKind::Circle;
                    o.tube = 0.0;
                } else if o.tube < 0.0 {
                    o.tube = -o.tube;
                    o.side = -self.side;
                }
            }
            QuadricKind::None | QuadricKind::Line | QuadricKind::Point | QuadricKind::Circle => {}
        }
        o
    }

    /// Not the fork (#2405): the foot of `m` on a plane or cylinder, the point of it
    /// nearest `m` (on a cylinder, along `m`'s radial line from the axis); `None` for other
    /// kinds and for `m` on a cylinder's axis.
    #[must_use]
    pub fn foot(&self, m: Point3) -> Option<Point3> {
        match self.kind {
            QuadricKind::Plane => Some(m - self.dir * self.dir.dot(m - self.point)),
            QuadricKind::Cylinder => {
                let axis = self.point + self.dir * (m - self.point).dot(self.dir);
                let radial = (m - axis).unitize().ok()?;
                Some(axis + radial * self.radius)
            }
            _ => None,
        }
    }

    /// Torus, circle (solidcore #2403): the distance of `point` from the circle the tube is
    /// centred on.
    fn circle_distance(&self, point: Point3) -> f64 {
        let w = point - self.point;
        let along = w.dot(self.dir);
        let off = across(w, self.dir).length() - self.radius;
        (off * off + along * along).sqrt()
    }

    /// Torus, circle (solidcore #2403): the point of the circle the tube is centred on nearest
    /// `point` (the centre for a point on the axis).
    fn circle_point(&self, point: Point3) -> Point3 {
        let r = across(point - self.point, self.dir);
        let len = r.length();
        if len > 0.0 {
            self.point + r * (self.radius / len)
        } else {
            self.point
        }
    }

    /// `SmBlendQuadric::Normal`: the unit normal at `point` (on the quadric), as the
    /// surface's.
    #[must_use]
    pub fn normal(&self, point: Point3) -> Vec3 {
        let n = match self.kind {
            QuadricKind::Plane => return self.dir,
            QuadricKind::Sphere => point - self.point,
            QuadricKind::Cylinder => across(point - self.point, self.dir),
            QuadricKind::Cone => {
                let r = across(point - self.point, self.dir);
                let len = r.length();
                if len > 0.0 {
                    r * (self.angle.cos() / len) - self.dir * self.angle.sin()
                } else {
                    Vec3::ZERO
                }
            }
            QuadricKind::Torus => point - self.circle_point(point),
            QuadricKind::None | QuadricKind::Line | QuadricKind::Point | QuadricKind::Circle => {
                return Vec3::ZERO
            }
        };
        let len = n.length();
        if len > 0.0 {
            n * (self.side / len)
        } else {
            n
        }
    }

    /// `SmBlendQuadric::Distance`: the distance of `point` from the quadric (from the cone's
    /// nappe, measured in the plane through the axis); `SM_BIG_DOUBLE` for no quadric.
    #[must_use]
    pub fn distance(&self, point: Point3) -> f64 {
        let w = point - self.point;
        match self.kind {
            QuadricKind::Plane => w.dot(self.dir).abs(),
            QuadricKind::Sphere | QuadricKind::Point => (w.length() - self.radius).abs(),
            QuadricKind::Cylinder | QuadricKind::Line => {
                (across(w, self.dir).length() - self.radius).abs()
            }
            QuadricKind::Cone => (across(w, self.dir).length() * self.angle.cos()
                - w.dot(self.dir) * self.angle.sin())
            .abs(),
            QuadricKind::Torus => (self.circle_distance(point) - self.tube).abs(),
            QuadricKind::Circle => self.circle_distance(point),
            QuadricKind::None => BIG_DOUBLE,
        }
    }

    /// `SmBlendQuadric::Implicit`: the implicit form `x.A.x + 2 b.x + c = 0` as `(A, b, c)`
    /// (sphere, cylinder, cone: the double cone; plane: `A = 0`, `b` the half normal; no
    /// quadric, a torus or its circle: all zero).
    #[must_use]
    pub fn implicit(&self) -> ([[f64; 3]; 3], [f64; 3], f64) {
        let ap = [self.point.x, self.point.y, self.point.z];
        let ad = [self.dir.x, self.dir.y, self.dir.z];
        let mut a = [[0.0; 3]; 3];
        let mut b = [0.0; 3];
        // |x - p|^2 kI - (d.(x - p))^2 kD - k0
        let (mut ki, mut kd, mut k0) = (0.0, 0.0, 0.0);
        match self.kind {
            QuadricKind::Plane => {
                for (bi, di) in b.iter_mut().zip(ad) {
                    *bi = 0.5 * di;
                }
                let c = -(ap[0] * ad[0] + ap[1] * ad[1] + ap[2] * ad[2]);
                return (a, b, c);
            }
            QuadricKind::Sphere | QuadricKind::Point => {
                ki = 1.0;
                k0 = self.radius * self.radius;
            }
            QuadricKind::Cylinder | QuadricKind::Line => {
                ki = 1.0;
                kd = 1.0;
                k0 = self.radius * self.radius;
            }
            QuadricKind::Cone => {
                ki = self.angle.cos() * self.angle.cos();
                kd = 1.0;
            }
            QuadricKind::None | QuadricKind::Torus | QuadricKind::Circle => {}
        }
        for (i, row) in a.iter_mut().enumerate() {
            for (j, aij) in row.iter_mut().enumerate() {
                let di = ad.get(i).copied().unwrap_or(0.0);
                let dj = ad.get(j).copied().unwrap_or(0.0);
                *aij = (if i == j { ki } else { 0.0 }) - kd * di * dj;
            }
        }
        // b = -A p
        for (bi, row) in b.iter_mut().zip(a.iter()) {
            *bi = -(row[0] * ap[0] + row[1] * ap[1] + row[2] * ap[2]);
        }
        // c = p.A.p - k0
        let mut pap = 0.0;
        for (pi, row) in ap.iter().zip(a.iter()) {
            for (aij, pj) in row.iter().zip(ap.iter()) {
                pap += pi * aij * pj;
            }
        }
        (a, b, pap - k0)
    }

    /// `SmBlendQuadric::IsRevolved`: a sphere, cylinder, cone or line.
    #[must_use]
    pub fn is_revolved(&self) -> bool {
        matches!(
            self.kind,
            QuadricKind::Sphere | QuadricKind::Cylinder | QuadricKind::Cone | QuadricKind::Line
        )
    }
}

/// `SmBlendSpine`: the curve a constant round's rolling ball's centre runs on, by the
/// faces' kinds and placement. All but [`BlendSpine::Traced`] are rational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendSpine {
    /// `SM_BS_TRACED`: none of the others; the round is traced and fitted.
    #[default]
    Traced,
    /// `SM_BS_LINE`: a line (two planes; a plane, or a cylinder, parallel to a cylinder's
    /// axis): the round is a cylinder.
    Line,
    /// `SM_BS_CIRCLE`: a circle about a common axis (coaxial quadrics; a plane square to
    /// the axis; a sphere with its centre on it): the ball's section turned about the axis.
    Circle,
    /// `SM_BS_POINT`: one point (two cylinders as wide as the ball, their axes meeting): a
    /// piece of the ball. With `fillet_blend_corner` also an offset collapsed onto a line
    /// or a circle (a cylinder or torus as wide as the ball) that the other offset crosses
    /// ([`classify_ball_on_collapsed_offset`], solidcore #2403): the ball's triangle.
    Point,
    /// `SM_BS_CONIC`: a plane section of the other quadric (a plane and a cylinder or cone
    /// at any other angle).
    Conic,
    /// `SM_BS_CONIC_PAIR`: one of two plane conics (two cylinders or cones whose offsets
    /// touch a common ball the same way round).
    ConicPair,
    /// `SM_BS_NODAL_QUARTIC`: a quartic with a node where the offsets touch.
    NodalQuartic,
}

/// `SmExactBlend`: a blend's exact class: its faces' quadrics, their offsets (the ball's
/// centre lies on both), and the spine's data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExactBlend {
    /// `m_eSpine`.
    pub spine: BlendSpine,
    /// `m_aFaces`.
    pub faces: [BlendQuadric; 2],
    /// `m_aOffsets`.
    pub offsets: [BlendQuadric; 2],
    /// `m_adContact[i]`: the ball touches face `i` at its centre plus the radius times this
    /// times the face's normal there (`+1` or `-1`).
    pub contact: [f64; 2],
    /// `m_dRadius`.
    pub radius: f64,
    /// `m_sPoint`: circle, a point of the common axis; point, the ball's centre; conic
    /// (pair), a point of the conic's plane; nodal quartic, the node; line (not the fork,
    /// #2405), the point of the line nearest the classified centre.
    pub point: Point3,
    /// `m_sDir`: circle, the axis's unit direction; conic (pair), the plane's unit normal;
    /// line (not the fork, #2405), its unit direction.
    pub dir: Vec3,
    /// Not the fork (#2485): what [`classify_exact_blend`] read the class from, so that
    /// [`classify_supports`] reads it again from the round's supports alone; `None` for a
    /// blend not classified from a solver.
    pub inputs: Option<ClassInputs>,
}

impl Default for ExactBlend {
    fn default() -> Self {
        Self {
            spine: BlendSpine::Traced,
            faces: [BlendQuadric::default(); 2],
            offsets: [BlendQuadric::default(); 2],
            contact: [1.0, 1.0],
            radius: 0.0,
            point: Point3::ZERO,
            dir: Vec3::ZERO,
            inputs: None,
        }
    }
}

/// Not the fork (#2485): the numbers a classification reads off a round's two faces (its
/// supports) beside the faces themselves: where each face's quadric is read, how far the
/// ball's centre is off each along its normal, the centre, the tolerance placements are
/// decided within and the classification's own switches. With the supports they decide
/// the class ([`classify_supports`]), as [`classify_exact_blend`] decided it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassInputs {
    /// The `(u, v)` each face's quadric is read at.
    pub uv: [Point2; 2],
    /// The signed offset of the ball's centre from each face, along the face's normal.
    pub offsets: [f64; 2],
    /// The ball's centre the spine is classified at.
    pub centre: Point3,
    /// The tolerance placements are decided within (the body's).
    pub tol: f64,
    /// Whether a face's offset collapsed onto its spine and crossed by the other is the
    /// ball's piece ([`classify_ball_on_collapsed_offset`], `fillet_blend_corner` with a
    /// constant ball).
    pub ball_on_collapsed: bool,
    /// Whether the solver's radii can change along the edge (only the line, circle and
    /// point classes are kept then).
    pub radii_can_change: bool,
}

/// Not the fork (#2405): the line two offsets of a line spine meet in, `O1 ∩ O2`, as its
/// point nearest `centre` and its unit direction. Two planes meet along `n1 x n2`; a plane
/// parallel to a cylinder's axis meets the cylinder along the axis, `w = sqrt(R^2 - h^2)`
/// either side of the axis's foot on the plane (`h` the axis's height over it); two
/// cylinders with parallel axes along the axes through their sections' crossing,
/// `x = (d^2 + Ra^2 - Rb^2) / 2d` along their centres' line and `sqrt(Ra^2 - x^2)` across
/// it. A crossing within rounding of touching is the touching point. `None` for other
/// kinds or offsets that do not meet in a line (parallel planes, coaxial cylinders).
fn line_spine(a: &BlendQuadric, b: &BlendQuadric, centre: Point3) -> Option<(Point3, Vec3)> {
    use QuadricKind as Q;
    // of two crossings, the one nearer the centre
    let nearer = |p: Point3, q: Point3| {
        if centre.distance(p) <= centre.distance(q) {
            p
        } else {
            q
        }
    };
    match (a.kind, b.kind) {
        (Q::Plane, Q::Plane) => {
            let c = a.dir.dot(b.dir);
            let s = 1.0 - c * c;
            let dir = a.dir.cross(b.dir).unitize().ok()?;
            if s.is_nan() || s <= 0.0 {
                return None;
            }
            // centre + x n1 + y n2 on both planes
            let r1 = a.dir.dot(a.point - centre);
            let r2 = b.dir.dot(b.point - centre);
            let p = centre + a.dir * ((r1 - c * r2) / s) + b.dir * ((r2 - c * r1) / s);
            Some((p, dir))
        }
        (Q::Plane, Q::Cylinder) | (Q::Cylinder, Q::Plane) => {
            let (pl, x) = if a.kind == Q::Plane { (a, b) } else { (b, a) };
            let u = x.dir;
            let n = across(pl.dir, u).unitize().ok()?;
            let axis = x.point + u * (centre - x.point).dot(u);
            let h = n.dot(axis - pl.point);
            let foot = axis - n * h;
            let w = (x.radius * x.radius - h * h).max(0.0).sqrt();
            let t = u.cross(n);
            Some((nearer(foot + t * w, foot - t * w), u))
        }
        (Q::Cylinder, Q::Cylinder) => {
            let u = a.dir;
            let ca = a.point + u * (centre - a.point).dot(u);
            let ab = across(b.point - ca, u);
            let d = ab.length();
            if d.is_nan() || d <= 0.0 {
                return None;
            }
            let e = ab / d;
            let x = (d * d + a.radius * a.radius - b.radius * b.radius) / (2.0 * d);
            let w = (a.radius * a.radius - x * x).max(0.0).sqrt();
            let base = ca + e * x;
            let t = u.cross(e);
            Some((nearer(base + t * w, base - t * w), u))
        }
        _ => None,
    }
}

/// `sm_LinesMeet`: the point where two lines meet within `tol` (their nearest points'
/// midpoint), `None` when they are parallel or miss each other.
fn lines_meet(p0: Point3, d0: Vec3, p1: Point3, d1: Vec3, tol: f64) -> Option<Point3> {
    let w = p0 - p1;
    let b = d0.dot(d1);
    let den = d0.dot(d0) * d1.dot(d1) - b * b;
    if den <= EFF_ZERO_SQRT * d0.dot(d0) * d1.dot(d1) {
        return None;
    }
    let d = d0.dot(w);
    let e = d1.dot(w);
    let q0 = p0 + d0 * ((b * e - d1.dot(d1) * d) / den);
    let q1 = p1 + d1 * ((d0.dot(d0) * e - b * d) / den);
    if q0.distance(q1) > tol {
        return None;
    }
    Some(q0 + (q1 - q0) * 0.5)
}

/// `sm_Coaxial`: whether two quadrics of revolution (or a plane, or a sphere's centre)
/// share their axis within `tol` at a distance `reach` from it; the axis.
fn coaxial(a: &BlendQuadric, b: &BlendQuadric, tol: f64, reach: f64) -> Option<(Point3, Vec3)> {
    use QuadricKind as Q;
    let point_a = matches!(a.kind, Q::Sphere | Q::Point);
    let point_b = matches!(b.kind, Q::Sphere | Q::Point);
    let axis_a = matches!(a.kind, Q::Cylinder | Q::Cone | Q::Line);
    let axis_b = matches!(b.kind, Q::Cylinder | Q::Cone | Q::Line);
    let ang_tol = tol / reach.max(tol);

    if point_a && point_b {
        let d = b.point - a.point;
        if d.length() <= tol {
            return None; // concentric: no circle
        }
        let d = d.unitize().ok()?;
        return Some((a.point, d));
    }
    if (point_a && axis_b) || (axis_a && point_b) {
        let (p, x) = if point_a { (a, b) } else { (b, a) };
        if across(p.point - x.point, x.dir).length() > tol {
            return None;
        }
        return Some((x.point, x.dir));
    }
    if axis_a && axis_b {
        if a.dir.cross(b.dir).length() > ang_tol {
            return None;
        }
        if across(b.point - a.point, a.dir).length() > tol {
            return None;
        }
        return Some((a.point, a.dir));
    }
    // a plane square to an axis, or through a sphere's centre's normal line
    let (pl, x) = if a.kind == Q::Plane { (a, b) } else { (b, a) };
    if pl.kind != Q::Plane || x.kind == Q::Plane {
        return None;
    }
    if x.kind == Q::Sphere {
        return Some((x.point, pl.dir));
    }
    if axis_a || axis_b {
        if pl.dir.cross(x.dir).length() > ang_tol {
            return None;
        }
        return Some((x.point, x.dir));
    }
    None
}

/// `sm_InscribedRadius`: the radius of the ball about `c`, a point of the axis, that the
/// cylinder or cone touches along a circle (negative: `c` beyond the apex).
fn inscribed_radius(q: &BlendQuadric, c: Point3) -> f64 {
    if q.kind == QuadricKind::Cylinder {
        return q.radius;
    }
    (c - q.point).dot(q.dir) * q.angle.sin()
}

/// `sm_PairPlane`: two cylinders or cones (offsets) touching a common ball about `c` of
/// radius `rho` the same way round: the plane of the conic of their intersection through
/// `centre`, as a point and unit normal (`None` when neither is within `tol` of it).
fn pair_plane(
    q: &[BlendQuadric; 2],
    c: Point3,
    rho: f64,
    centre: Point3,
    tol: f64,
) -> Option<(Point3, Vec3)> {
    let mut u = [Vec3::ZERO; 2];
    let mut kk = [0.0; 2];
    let mut s = [0.0; 2];
    for (i, qi) in q.iter().enumerate() {
        // P_i(x) = (x - c).u_i - k_i, u_i from c toward the apex (along -d), k_i = rho sin(a)
        let cone = qi.kind == QuadricKind::Cone;
        if let (Some(ui), Some(ki), Some(si)) = (u.get_mut(i), kk.get_mut(i), s.get_mut(i)) {
            *ui = -qi.dir;
            *ki = if cone { rho * qi.angle.sin() } else { 0.0 };
            *si = if cone { 1.0 / qi.angle.cos() } else { 1.0 };
        }
    }
    let mut best = BIG_DOUBLE;
    let mut out = None;
    for sign in [1.0, -1.0] {
        let n = u[1] * s[1] - u[0] * (sign * s[0]);
        let k = kk[1] * s[1] - kk[0] * sign * s[0];
        let len = n.length();
        if len <= EFF_ZERO_SQRT {
            continue;
        }
        let off = (n.dot(centre - c) - k).abs() / len;
        if off < best {
            best = off;
            let normal = n * (1.0 / len);
            out = Some((c + normal * (k / len), normal));
        }
    }
    if best <= tol {
        out
    } else {
        None
    }
}

/// `sm_PointsWithContact`: the points of `q` where the ball touching it has contact
/// direction `e`: a line of a cylinder or cone (`(point, dir, true)`), a point of a sphere
/// (`(point, _, false)`); `None` where there are none.
fn points_with_contact(q: &BlendQuadric, contact: f64, e: Vec3) -> Option<(Point3, Vec3, bool)> {
    // the surface's normal there: e / contact
    let n = e * contact;
    match q.kind {
        QuadricKind::Sphere => Some((q.point + n * (q.side * q.radius), Vec3::ZERO, false)),
        QuadricKind::Cylinder => {
            if n.dot(q.dir).abs() > EFF_ZERO_SQRT {
                return None;
            }
            Some((q.point + n * (q.side * q.radius), q.dir, true))
        }
        QuadricKind::Cone => {
            // the normal is side (cos a radial - sin a d): along the generator in the plane
            // of d and n
            if (n.dot(q.dir) + q.side * q.angle.sin()).abs() > EFF_ZERO_SQRT {
                return None;
            }
            let g = across(q.dir, n).unitize().ok()?;
            Some((q.point, g, true))
        }
        _ => None,
    }
}

/// `sm_DirectionsWithDots`: the unit directions `e` with `e.d0 = s0` and `e.d1 = s1`
/// (none, or two).
fn directions_with_dots(d0: Vec3, s0: f64, d1: Vec3, s1: f64) -> Vec<Vec3> {
    let c = d0.dot(d1);
    let den = 1.0 - c * c;
    if den <= EFF_ZERO_SQRT {
        return Vec::new();
    }
    let al = (s0 - c * s1) / den;
    let be = (s1 - c * s0) / den;
    let inside = d0 * al + d1 * be;
    let x = d0.cross(d1);
    let g2 = (1.0 - inside.dot(inside)) / x.dot(x);
    if g2 < 0.0 {
        return Vec::new();
    }
    let g = g2.sqrt();
    vec![inside + x * g, inside - x * g]
}

/// `sm_TouchingPoint`: a point where two offsets (cylinders, cones, or one and a sphere)
/// touch with the ball's contact directions agreeing, within `tol`: the node of their
/// quartic intersection.
fn touching_point(q: &[BlendQuadric; 2], contact: [f64; 2], tol: f64) -> Option<Point3> {
    use QuadricKind as Q;
    let curved = |k: Q| matches!(k, Q::Cylinder | Q::Cone);
    let dot_of = |qi: &BlendQuadric, ci: f64| {
        if qi.kind == Q::Cylinder {
            0.0
        } else {
            -ci * qi.side * qi.angle.sin()
        }
    };
    // the candidate contact directions e: from the kinds' normal constraints
    let sphere_index = if q[0].kind == Q::Sphere {
        Some(0)
    } else if q[1].kind == Q::Sphere {
        Some(1)
    } else {
        None
    };
    let es = if let Some(is) = sphere_index {
        let (ps, x, cx) = if is == 0 {
            (&q[0], &q[1], contact[1])
        } else {
            (&q[1], &q[0], contact[0])
        };
        if !curved(x.kind) {
            return None;
        }
        // e in the plane through the axis and the centre
        // the centre on the axis: coaxial
        let w = across(ps.point - x.point, x.dir).unitize().ok()?;
        let dot = dot_of(x, cx);
        let across_len = (1.0 - dot * dot).max(0.0).sqrt();
        vec![x.dir * dot + w * across_len, x.dir * dot - w * across_len]
    } else {
        if !curved(q[0].kind) || !curved(q[1].kind) {
            return None;
        }
        let dots = [dot_of(&q[0], contact[0]), dot_of(&q[1], contact[1])];
        directions_with_dots(q[0].dir, dots[0], q[1].dir, dots[1])
    };

    for e in es {
        let Ok(e) = e.unitize() else {
            continue;
        };
        let (Some((p0, d0, line0)), Some((p1, d1, line1))) = (
            points_with_contact(&q[0], contact[0], e),
            points_with_contact(&q[1], contact[1], e),
        ) else {
            continue;
        };
        let sq = if line0 && line1 {
            // both lines lie square to e: they meet when they lie in one plane
            if (p1 - p0).dot(e).abs() > tol {
                continue;
            }
            let Some(m) = lines_meet(p0, d0, p1, d1, tol) else {
                continue;
            };
            m
        } else {
            let (pt, other, other_dir) = if line0 { (p1, p0, d0) } else { (p0, p1, d1) };
            if across(pt - other, other_dir).length() > tol {
                continue;
            }
            pt
        };
        // on both, the ball touching each along e there
        let mut touch = true;
        for (qi, ci) in q.iter().zip(contact) {
            if qi.distance(sq) > tol
                || (qi.kind == Q::Cone && (sq - qi.point).dot(qi.dir) <= tol)
                || (qi.normal(sq) * ci).dot(e) < 1.0 - EFF_ZERO_SQRT
            {
                touch = false;
            }
        }
        if touch {
            return Some(sq);
        }
    }
    None
}

/// `blend.spine = SM_BS_LINE`, with (not the fork, #2405) the line its offsets meet in
/// ([`line_spine`]); offsets that meet in no line are no line spine.
fn set_line_spine(blend: &mut ExactBlend, centre: Point3) {
    let [a, b] = blend.offsets;
    if let Some((point, dir)) = line_spine(&a, &b, centre) {
        blend.spine = BlendSpine::Line;
        blend.point = point;
        blend.dir = dir;
    }
}

/// `sm_ClassifyBlendSpine`: the one classification of the spine of a constant round of
/// radius `blend.radius` between faces on `blend.faces`, given their offsets
/// `blend.offsets` and `centre`, a point of the spine, from the faces' kinds and
/// placement (decided within `tol`). Sets `blend.spine` (and `point`, `dir`).
pub fn classify_blend_spine(centre: Point3, tol: f64, blend: &mut ExactBlend) {
    use QuadricKind as Q;
    blend.spine = BlendSpine::Traced;
    let [a, b] = blend.offsets;
    if a.kind == Q::None || b.kind == Q::None {
        return;
    }
    // (solidcore #2403) a torus and its circle take no class of the fork's
    if matches!(a.kind, Q::Torus | Q::Circle) || matches!(b.kind, Q::Torus | Q::Circle) {
        return;
    }

    // the reach: how far the spine lies from the quadrics' points (at least the ball's
    // radius), over which an angle of tol / reach moves a point by tol
    let mut reach = tol.max(blend.radius);
    for q in [&a, &b] {
        if q.kind != Q::Plane {
            reach = reach.max(centre.distance(q.point));
        }
    }

    // a common axis: circles (a plane square to it, a sphere's centre on it, the same axis)
    if let Some((axis_point, axis_dir)) = coaxial(&a, &b, tol, reach) {
        // two coaxial cylinders meet nowhere or all along: no spine
        if matches!(a.kind, Q::Cylinder | Q::Line) && matches!(b.kind, Q::Cylinder | Q::Line) {
            return;
        }
        blend.spine = BlendSpine::Circle;
        blend.point = axis_point;
        blend.dir = axis_dir;
        return;
    }

    let ang_tol = tol / reach.max(tol);
    let (ea, eb) = (a.kind, b.kind);

    // two planes: a line
    if ea == Q::Plane && eb == Q::Plane {
        if a.dir.cross(b.dir).length() > ang_tol {
            set_line_spine(blend, centre);
        }
        return;
    }

    // a plane and a cylinder or cone: a line along a cylinder parallel to the plane, else a
    // conic
    if ea == Q::Plane || eb == Q::Plane {
        let (pl, x) = if ea == Q::Plane { (&a, &b) } else { (&b, &a) };
        if x.kind == Q::Cylinder {
            if pl.dir.dot(x.dir).abs() <= ang_tol {
                set_line_spine(blend, centre);
            } else {
                blend.spine = BlendSpine::Conic;
            }
        } else if x.kind == Q::Cone {
            // a plane through the apex meets the cone in lines: not a blend's conic
            if pl.distance(x.point) > tol {
                blend.spine = BlendSpine::Conic;
            }
        }
        if blend.spine == BlendSpine::Conic {
            blend.point = pl.point;
            blend.dir = pl.dir;
        }
        return;
    }

    // two lines (cylinders as wide as the ball) meeting: a point
    if ea == Q::Line && eb == Q::Line {
        if let Some(meet) = lines_meet(a.point, a.dir, b.point, b.dir, tol) {
            blend.spine = BlendSpine::Point;
            blend.point = meet;
        }
        return;
    }
    if ea == Q::Line || eb == Q::Line || ea == Q::Point || eb == Q::Point {
        return;
    }

    // two cylinders with parallel axes: lines
    if ea == Q::Cylinder && eb == Q::Cylinder && a.dir.cross(b.dir).length() <= ang_tol {
        set_line_spine(blend, centre);
        return;
    }

    // two cylinders or cones touching a common ball the same way round: two plane conics
    // (the inscribed radius moves by -side * contact * r as the ball's radius r does: alike
    // for both)
    #[allow(clippy::float_cmp)] // the fork compares the +-1 products exactly
    let same_way = a.side * blend.contact[0] == b.side * blend.contact[1];
    if ea != Q::Sphere && eb != Q::Sphere && same_way {
        if let Some(c) = lines_meet(a.point, a.dir, b.point, b.dir, tol) {
            let rho_a = inscribed_radius(&a, c);
            let rho_b = inscribed_radius(&b, c);
            if rho_a > tol && (rho_a - rho_b).abs() <= tol {
                if let Some((p, d)) = pair_plane(&[a, b], c, 0.5 * (rho_a + rho_b), centre, tol) {
                    blend.point = p;
                    blend.dir = d;
                    blend.spine = BlendSpine::ConicPair;
                    return;
                }
            }
        }
    }

    // touching at one point the same way round: a quartic with a node there
    if let Some(node) = touching_point(&[a, b], blend.contact, tol) {
        blend.spine = BlendSpine::NodalQuartic;
        blend.point = node;
    }
}

/// The curve `c` (a [`QuadricKind::Line`] or [`QuadricKind::Circle`]) crossing the quadric
/// surface `q` (a plane, sphere, cylinder or cone) nearest `near`: Newton's method on the
/// curve's parameter on `q`'s implicit form (about `near`, at most 50 steps), from the
/// curve's point nearest `near`. `None` where it does not converge, or where the curve runs
/// along `q` there (its unit tangent's part along `q`'s unit normal within `ang_tol`): the
/// crossing is then not one point.
fn curve_crosses_quadric(
    c: &BlendQuadric,
    q: &BlendQuadric,
    near: Point3,
    ang_tol: f64,
) -> Option<Point3> {
    // the curve about its point nearest `near`: x(s) and x'(s), relative to `near`
    let (foot, e1, e2) = match c.kind {
        QuadricKind::Line => (
            c.point + c.dir * (near - c.point).dot(c.dir),
            c.dir,
            Vec3::ZERO,
        ),
        QuadricKind::Circle => {
            let r = across(near - c.point, c.dir);
            let len = r.length();
            if len <= 0.0 {
                return None;
            }
            let e1 = r * (1.0 / len);
            (c.point, e1, c.dir.cross(e1))
        }
        _ => return None,
    };
    let at = |s: f64| -> (Vec3, Vec3) {
        if c.kind == QuadricKind::Line {
            (foot - near + e1 * s, e1)
        } else {
            let (sn, cs) = s.sin_cos();
            (
                foot - near + (e1 * cs + e2 * sn) * c.radius,
                (e1 * (-sn) + e2 * cs) * c.radius,
            )
        }
    };
    let mut local = *q;
    local.point = q.point - near;
    let (a, b, k0) = local.implicit();
    let value = |x: Vec3| -> (f64, Vec3) {
        let v = [x.x, x.y, x.z];
        let mut f = k0;
        let mut g = [0.0; 3];
        for ((gi, row), (bi, xi)) in g.iter_mut().zip(a.iter()).zip(b.iter().zip(v.iter())) {
            let ax = row[0] * v[0] + row[1] * v[1] + row[2] * v[2];
            f += xi * ax + 2.0 * bi * xi;
            *gi = 2.0 * (ax + bi);
        }
        (f, Vec3::new(g[0], g[1], g[2]))
    };
    let mut s = 0.0f64;
    let mut converged = false;
    for _ in 0..50 {
        let (x, dx) = at(s);
        let (f, g) = value(x);
        let slope = g.dot(dx);
        if slope == 0.0 {
            return None;
        }
        let step = f / slope;
        s -= step;
        if step.abs() <= EFF_ZERO * (1.0 + s.abs()) {
            converged = true;
            break;
        }
    }
    if !converged {
        return None;
    }
    let (x, dx) = at(s);
    let (_, g) = value(x);
    let (t, n) = (dx.unitize().ok()?, g.unitize().ok()?);
    if t.dot(n).abs() <= ang_tol {
        return None;
    }
    Some(near + x)
}

/// Not in the fork (solidcore #2403; with `fillet_blend_corner`): a constant round one of
/// whose faces is itself a round of the ball's radius on the ball's side (a cylinder or a
/// torus as wide as the ball), whose offset `blend.offsets` is therefore collapsed onto that
/// round's spine, a line or a circle, which the other offset (a plane, sphere, cylinder or
/// cone) crosses at `centre` (within `tol`): every ball touching both faces is the one
/// about that crossing, so the spine is that point ([`BlendSpine::Point`], `blend.point`)
/// and the round is a piece of that ball: its triangle between the end sections and its
/// contact circle with the collapsed face. This is the round along an earlier round's end
/// curve, run on by a tangent chain of the same radius: the corner ball of the faces the
/// two rounds roll on. A curve that runs along the other offset (a line parallel to a
/// plane, a circle in it) is not one point and stays traced.
pub fn classify_ball_on_collapsed_offset(centre: Point3, tol: f64, blend: &mut ExactBlend) {
    use QuadricKind as Q;
    let [a, b] = blend.offsets;
    let curve = |q: &BlendQuadric| matches!(q.kind, Q::Line | Q::Circle);
    let surface = |q: &BlendQuadric| matches!(q.kind, Q::Plane | Q::Sphere | Q::Cylinder | Q::Cone);
    let (c, q) = if curve(&a) && surface(&b) {
        (a, b)
    } else if curve(&b) && surface(&a) {
        (b, a)
    } else {
        return;
    };
    // the reach, as `classify_blend_spine` takes it: an angle of tol / reach moves a point
    // by tol
    let mut reach = tol.max(blend.radius);
    for x in [&c, &q] {
        if x.kind != Q::Plane {
            reach = reach.max(centre.distance(x.point));
        }
    }
    let Some(p) = curve_crosses_quadric(&c, &q, centre, tol / reach.max(tol)) else {
        return;
    };
    if p.distance(centre) > tol || q.distance(p) > tol || c.distance(p) > tol {
        return;
    }
    blend.spine = BlendSpine::Point;
    blend.point = p;
}

/// `SmFilletSolver::ClassifyExactBlend(rTsectPnt, rBlend)`: the exact class of the
/// solver's constant round from its faces' kinds and placement at `tsect`. The faces are
/// the offset surfaces' bases; each face's offset there is the ball's radius along its
/// normal, either way. A section other than the circle (or the chamfer's line), a face of
/// no natural quadric, or no surface generator is [`BlendSpine::Traced`]; so is a rational
/// spine other than a line, circle or point for a solver whose offsets may vary along the
/// edge. Placements are decided within the target B-rep's tolerance.
///
/// # Errors
///
/// Those of `CanMakeAnalytic`, `SetupOffsetValues`, the evaluations and
/// [`BlendQuadric::from_surface`]; [`SolidError::InvalidInput`] for a missing edge use,
/// edge or offset surface (upstream: `NER`).
pub fn classify_exact_blend(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
    tsect: &TsectPnt,
) -> Result<ExactBlend> {
    let blend = ExactBlend::default();
    if !ops.can_make_analytic()? {
        return Ok(blend);
    }
    let Some(fsg) = ops.solver().fsg.as_ref() else {
        return Ok(blend);
    };
    let section = fsg.fillet_surface_generator_type();
    if section != FilletSurfaceGeneratorType::Circular
        && section != FilletSurfaceGeneratorType::Linear
    {
        return Ok(blend);
    }

    // placements are decided within the body's tolerance (the filleted edge's B-rep)
    let eu0 = ops
        .solver()
        .edgeuse(0)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    eu::edge(b, eu0)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let tol = b.tolerance();

    let mut radius = 0.0;
    ops.setup_offset_values(tsect, Some(&mut radius))?;
    // Not the fork (solidcore #2652): with a constant ball, a face of no natural quadric is
    // read on (`classify_faces`: the ball's piece where its centre stays put)
    let any_faces = k.fixes.fillet_blend_corner && !ops.offset_radii_can_change()?;
    let mut faces = [BlendQuadric::default(); 2];
    let mut uvs = [Point2::new(0.0, 0.0); 2];
    let mut offs = [0.0; 2];
    let mut centre = Point3::ZERO;
    for i in 0..2 {
        let uv = tsect.uv_pos(if i == 0 {
            SrfIndex::First
        } else {
            SrfIndex::Second
        });
        let (face, p, n) = {
            let off = ops
                .solver()
                .surface(i)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let base = off.surface.base_surface();
            let face = BlendQuadric::from_surface(k, base, uv, tol)?;
            if face.kind == QuadricKind::None && !any_faces {
                return Ok(blend);
            }
            let (p, n) = point_and_normal(k, base, uv)?;
            (face, p, n)
        };
        let q = crate::blend::fillet_solver::p2::eval_offset(k, ops.solver_mut(), i, uv)?;
        if let (Some(f), Some(u), Some(o)) = (faces.get_mut(i), uvs.get_mut(i), offs.get_mut(i)) {
            *f = face;
            *u = uv;
            *o = (q - p).dot(n);
        }
        if i == 0 {
            centre = q;
        }
    }
    let radii_can_change = ops.offset_radii_can_change()?;
    let inputs = ClassInputs {
        uv: uvs,
        offsets: offs,
        centre,
        tol,
        ball_on_collapsed: k.fixes.fillet_blend_corner && !radii_can_change,
        radii_can_change,
    };
    Ok(classify_faces(faces, &inputs))
}

/// The class of a round on faces whose quadrics are `faces`, read with `inputs`
/// ([`classify_exact_blend`]'s, past the faces): each face moved by its offset, the spine
/// classified at the centre ([`classify_blend_spine`]; then, as solidcore #2403 does, a
/// collapsed offset the other crosses), and with radii that can change only the line,
/// circle and point classes kept.
///
/// Not the fork (solidcore #2652): with `inputs.ball_on_collapsed`, a round one of whose
/// faces has no natural quadric ([`QuadricKind::None`], a B-spline) and the other a surface
/// quadric is the ball's piece about the centre ([`BlendSpine::Point`]) where that face is
/// as wide as the ball: whether it is is decided from the geometry where the round is built
/// ([`calc_exact_fillet_geom`]: the ball's centre is the same at every point and the ball
/// lies on that face along its contact arc), else it is traced. Its offset is read as the
/// line through the centre ([`QuadricKind::Line`]), its direction that contact arc's axis,
/// set where it is built.
fn classify_faces(faces: [BlendQuadric; 2], inputs: &ClassInputs) -> ExactBlend {
    let tol = inputs.tol;
    let mut blend = ExactBlend {
        inputs: Some(*inputs),
        ..ExactBlend::default()
    };
    let generic = faces.iter().filter(|f| f.kind == QuadricKind::None).count();
    if generic > 0 && !(generic == 1 && inputs.ball_on_collapsed) {
        return ExactBlend::default();
    }
    for (i, (face, off)) in faces.iter().zip(inputs.offsets).enumerate() {
        let offset = if face.kind == QuadricKind::None {
            BlendQuadric {
                kind: QuadricKind::Line,
                point: inputs.centre,
                ..BlendQuadric::default()
            }
        } else {
            face.offset(off, tol)
        };
        if let (Some(f), Some(o), Some(c)) = (
            blend.faces.get_mut(i),
            blend.offsets.get_mut(i),
            blend.contact.get_mut(i),
        ) {
            *f = *face;
            *o = offset;
            *c = if off > 0.0 { -1.0 } else { 1.0 };
        }
        blend.radius = off.abs();
    }
    if generic > 0 {
        let surface = |q: &BlendQuadric| {
            matches!(
                q.kind,
                QuadricKind::Plane
                    | QuadricKind::Sphere
                    | QuadricKind::Cylinder
                    | QuadricKind::Cone
            )
        };
        if blend
            .faces
            .iter()
            .zip(&blend.offsets)
            .any(|(f, o)| f.kind != QuadricKind::None && surface(o))
        {
            blend.spine = BlendSpine::Point;
            blend.point = inputs.centre;
        }
        return blend;
    }

    classify_blend_spine(inputs.centre, tol, &mut blend);
    // Not in the fork (solidcore #2403): a face that is a round of the ball's radius, its
    // offset collapsed onto its spine, which the other offset crosses: the ball's piece
    if blend.spine == BlendSpine::Traced && inputs.ball_on_collapsed {
        classify_ball_on_collapsed_offset(inputs.centre, tol, &mut blend);
    }

    if inputs.radii_can_change
        && !matches!(
            blend.spine,
            BlendSpine::Line | BlendSpine::Circle | BlendSpine::Point
        )
    {
        blend.spine = BlendSpine::Traced;
    }
    blend
}

/// Not the fork (#2485): the class of a round read again from its two supports alone (the
/// faces [`classify_exact_blend`] read, as the round keeps them) with the numbers it read
/// beside them, `inputs`. On the same supports it is that classification, bit for bit; on
/// supports rebuilt from a record, whose analytic parameters are derived from their NURBS
/// forms to rounding, it is the same class to rounding.
///
/// # Errors
///
/// Those of [`BlendQuadric::from_surface`].
pub fn classify_supports(
    k: &Kernel,
    supports: [&Surface; 2],
    inputs: &ClassInputs,
) -> Result<ExactBlend> {
    let mut faces = [BlendQuadric::default(); 2];
    for ((face, support), uv) in faces.iter_mut().zip(supports).zip(inputs.uv) {
        *face = BlendQuadric::from_surface(k, support, uv, inputs.tol)?;
        // (solidcore #2652) a face of no natural quadric is read on with a constant ball
        if face.kind == QuadricKind::None && !inputs.ball_on_collapsed {
            return Ok(ExactBlend::default());
        }
    }
    Ok(classify_faces(faces, inputs))
}

/// Not the fork (#2485): a quadric of revolution about the axis through `a` along the
/// unit `d`, as its meridian in the half-plane `(rho, z)`, `rho` the distance from the
/// axis and `z` the height along it from `a`: a line `p + t e` or a circle about `c` of
/// radius `r`. `None` for a quadric that is not coaxial with it.
#[derive(Debug, Clone, Copy)]
enum Meridian {
    Line { p: Point2, e: Point2 },
    Circle { c: Point2, r: f64 },
}

fn meridian(q: &BlendQuadric, a: Point3, d: Vec3) -> Option<Meridian> {
    let z = |x: Point3| (x - a).dot(d);
    let rho = |x: Point3| across(x - a, d).length();
    Some(match q.kind {
        // a plane square to the axis: z = h
        QuadricKind::Plane => Meridian::Line {
            p: Point2::new(0.0, z(q.point)),
            e: Point2::new(1.0, 0.0),
        },
        // a coaxial cylinder: rho = R
        QuadricKind::Cylinder => Meridian::Line {
            p: Point2::new(q.radius, 0.0),
            e: Point2::new(0.0, 1.0),
        },
        // a cylinder as wide as the ball, collapsed onto the axis: rho = 0
        QuadricKind::Line => Meridian::Line {
            p: Point2::new(0.0, 0.0),
            e: Point2::new(0.0, 1.0),
        },
        // a sphere about a point of the axis
        QuadricKind::Sphere => Meridian::Circle {
            c: Point2::new(rho(q.point), z(q.point)),
            r: q.radius,
        },
        // a coaxial cone: the line from its apex at its half-angle into its nappe
        QuadricKind::Cone => Meridian::Line {
            p: Point2::new(rho(q.point), z(q.point)),
            e: Point2::new(q.angle.sin(), q.angle.cos() * q.dir.dot(d).signum()),
        },
        _ => return None,
    })
}

/// The crossings of two meridians (at most two).
fn meridians_cross(a: Meridian, b: Meridian) -> Vec<Point2> {
    let cross2 = |u: Point2, v: Point2| u.x * v.y - u.y * v.x;
    let sub = |u: Point2, v: Point2| Point2::new(u.x - v.x, u.y - v.y);
    let line_circle = |p: Point2, e: Point2, c: Point2, r: f64| {
        // |p + t e - c|^2 = r^2
        let w = sub(p, c);
        let ee = e.x * e.x + e.y * e.y;
        let be = w.x * e.x + w.y * e.y;
        let disc = be * be - ee * (w.x * w.x + w.y * w.y - r * r);
        if !(disc >= 0.0 && ee > 0.0) {
            return Vec::new();
        }
        let root = disc.sqrt();
        [(-be - root) / ee, (-be + root) / ee]
            .iter()
            .map(|t| Point2::new(p.x + t * e.x, p.y + t * e.y))
            .collect()
    };
    match (a, b) {
        (Meridian::Line { p: p0, e: e0 }, Meridian::Line { p: p1, e: e1 }) => {
            let den = cross2(e0, e1);
            if den == 0.0 {
                return Vec::new();
            }
            let t = cross2(sub(p1, p0), e1) / den;
            vec![Point2::new(p0.x + t * e0.x, p0.y + t * e0.y)]
        }
        (Meridian::Line { p, e }, Meridian::Circle { c, r })
        | (Meridian::Circle { c, r }, Meridian::Line { p, e }) => line_circle(p, e, c, r),
        (Meridian::Circle { c: c0, r: r0 }, Meridian::Circle { c: c1, r: r1 }) => {
            // the radical line, then a line-circle crossing
            let d = sub(c1, c0);
            let dd = d.x * d.x + d.y * d.y;
            if dd == 0.0 {
                return Vec::new();
            }
            let along = (dd + r0 * r0 - r1 * r1) / (2.0 * dd);
            let p = Point2::new(c0.x + along * d.x, c0.y + along * d.y);
            line_circle(p, Point2::new(-d.y, d.x), c0, r0)
        }
    }
}

/// Not the fork (#2486): the plane a chamfer between two planes lies on, from its offsets
/// alone: the plane through the feet, on the two faces, of the line the offsets meet in
/// (its unit normal `d x (F_2 - F_1)`). `None` for a blend that is not a line spine
/// between planes, or feet that coincide.
#[must_use]
pub fn chord_implicit(blend: &ExactBlend) -> Option<crate::surface::implicit::Implicit> {
    let planes = blend.faces.iter().all(|f| f.kind == QuadricKind::Plane);
    if blend.spine != BlendSpine::Line || !planes {
        return None;
    }
    let (p, d) = (blend.point, blend.dir);
    let [a, b] = &blend.faces;
    let foot = |f: &BlendQuadric| p - f.dir * (p - f.point).dot(f.dir);
    let (f1, f2) = (foot(a), foot(b));
    let normal = d.cross(f2 - f1).unitize().ok()?;
    Some(crate::surface::implicit::Implicit::Plane { origin: f1, normal })
}

/// Not the fork (#2485): the surface a classified round lies on, from its offsets alone:
/// for the line, circle and point classes the analytic surface by its implicit equation
/// (the cylinder of the ball's radius about the line the offsets meet in, the ball about
/// the point, and the torus about the common axis whose centre circle is where the
/// offsets' meridians cross, the crossing nearest the classified centre); for the conic,
/// conic-pair and nodal-quartic classes the canal surface of the ball's radius about
/// `leader`, the round's rational centre line (its parameter the net's through `map`),
/// which must lie on both offsets. `None` for
/// a traced round, a blend classified without its inputs, offsets whose meridians do not
/// cross and a rational class without its leader.
pub fn class_surface(
    blend: &ExactBlend,
    leader: Option<&crate::nurbs::b_spline_curve::BSplineCurve>,
    map: &crate::surface::implicit::LeaderMap,
) -> Option<crate::surface::implicit::ClassSurface> {
    use crate::surface::implicit::{ClassSurface, Implicit};
    let r = blend.radius;
    let implicit = match blend.spine {
        BlendSpine::Line => Some(Implicit::Cone {
            origin: blend.point,
            axis: blend.dir,
            radius: r,
            tan_angle: 0.0,
        }),
        BlendSpine::Point => Some(Implicit::Sphere {
            centre: blend.point,
            radius: r,
        }),
        BlendSpine::Circle => {
            let centre = blend.inputs?.centre;
            let (a, d) = (blend.point, blend.dir);
            let ma = meridian(&blend.offsets[0], a, d)?;
            let mb = meridian(&blend.offsets[1], a, d)?;
            let near = Point2::new(across(centre - a, d).length(), (centre - a).dot(d));
            let dist = |x: &Point2| (x.x - near.x).hypot(x.y - near.y);
            let spine = meridians_cross(ma, mb)
                .into_iter()
                .filter(|x| x.x >= 0.0)
                .min_by(|x, y| dist(x).total_cmp(&dist(y)))?;
            Some(Implicit::Torus {
                centre: a + d * spine.y,
                axis: d,
                major: spine.x,
                minor: r,
            })
        }
        BlendSpine::Conic | BlendSpine::ConicPair | BlendSpine::NodalQuartic => {
            return Some(ClassSurface::Canal {
                spine: Box::new(leader?.clone()),
                map: *map,
                offsets: [blend.offsets[0].implicit(), blend.offsets[1].implicit()],
                radius: r,
            });
        }
        BlendSpine::Traced => None,
    };
    implicit.map(ClassSurface::Implicit)
}

/// [`ball_zone_open_at_an_end`]'s refusal: the radius named (the face's) and whether the
/// face is as wide as the ball along part of the edge only (solidcore #2652), not along the
/// whole edge with the zone open at an end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollapsedOffset {
    /// The face's radius of curvature toward the ball: its radius, or (partial) its least
    /// along the edge.
    pub radius: f64,
    /// Whether the face is as wide as the ball along part of the edge only.
    pub partial: bool,
}

/// Not the fork's (#2401; with `fixes.fillet_gn_section_generator`): for a constant ball of
/// `radius` (`None`: the solver's own) whose offset of one of its faces collapses (the face
/// a cylinder or sphere as wide as the ball, the ball inside it: its offset is the axis or
/// the centre, [`QuadricKind::Line`] or [`QuadricKind::Point`]), the radius of that face
/// where the zone the ball sweeps is open at an end of the edge; `None` where no offset
/// collapses, the edge is closed, or each end closes it.
///
/// The ball's centre then lies on the collapsed offset and the other face's offset at once:
/// it does not move along the edge, and every section of the round is a section of that one
/// ball. The zone it sweeps ends at an end of the edge only where the plane through that end
/// square to the edge holds the centre: the next round's section there (the tray's seam, a
/// cornered block's rim) or its End's plane. Otherwise the zone has no section at that end
/// (the ball touches the other face at one point off it, as at the tangent ends of the
/// middle edge of an arc's tangent chain) and no round ends there. Judged where the
/// collapsed axis crosses that plane (or lies in it): the centre is there if that point lies
/// on the other face's offset, within the solver's tolerance.
///
/// An end at a vertex of one of `joined` (the session's other blended edges) is where the
/// chain runs on into the next round: with `fixes.fillet_blend_corner` the Join places that
/// round's section through the fixed centre and the piece is built as the ball's triangle
/// between the two sections (`classify_ball_on_collapsed_offset`, #2403), so that end
/// closes the zone whatever the plane square to the edge. The caller passes `joined` only
/// under that flag; without it such a piece is traced and is judged by the plane alone.
///
/// Not the fork (solidcore #2652; the rule of hub #2243): a face of no natural quadric (a
/// B-spline) is judged by its geometry along the edge (`edge_match`). As wide as the ball
/// along the whole edge (its larger radius of curvature toward the ball the ball's, within
/// the body's tolerance, at every station), its offset collapses as a cylinder's does, onto
/// its focal curve (`focal_offset`), read again at each end's own foot to judge that end.
/// As wide as the ball along part of the edge only, it is refused
/// ([`CollapsedOffset::partial`]), naming the face's least radius of curvature toward the
/// ball along the edge: open, as below it the ball no longer matches the face anywhere.
///
/// # Errors
///
/// Those of the drops, the evaluations and the topology lookups; `SM_ERR_NULL_POINTER` for
/// a solver without its edge use, offset surfaces or edge curve.
pub fn ball_zone_open_at_an_end(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
    radius: Option<f64>,
    joined: &[EdgeId],
) -> Result<Option<CollapsedOffset>> {
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;
    use crate::topology::edge;
    let eu0 = ops
        .solver()
        .edgeuse(0)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let e = eu::edge(b, eu0)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    if edge::is_closed(b, e)? {
        return Ok(None);
    }
    let curve = edge::evaluation_curve(b, e)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone();
    let ivl = edge::interval(b, e)?;
    let tol = b.tolerance();
    let approx = ops.solver().approx_tol_3d;
    let mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;

    // each face's quadric there and its offset by the ball; (solidcore #2652) a face of no
    // natural quadric as wide as the ball along the whole edge, its offset collapsed by its
    // geometry (`edge_match`, `focal_offset`)
    let mut faces = [BlendQuadric::default(); 2];
    let mut offsets = [BlendQuadric::default(); 2];
    let mut focal = [None; 2];
    for i in 0..2 {
        let (face, p, n, uv) = {
            let off = ops
                .solver()
                .surface(i)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let base = off.surface.base_surface();
            let domain = base.natural_uv_domain_with_nurb(k)?;
            let dropped =
                base.drop_point::<NoObject>(k, mid, &domain, &DropPointOptions::default())?;
            if !dropped.success {
                return Ok(None);
            }
            let uv = dropped.result_uv;
            let face = BlendQuadric::from_surface(k, base, uv, tol)?;
            let (p, n) = point_and_normal(k, base, uv)?;
            (face, p, n, uv)
        };
        let q = crate::blend::fillet_solver::p2::eval_offset(k, ops.solver_mut(), i, uv)?;
        let mut off = (q - p).dot(n);
        if let Some(r) = radius {
            off = if off < 0.0 { -r } else { r };
        }
        let offset = if face.kind == QuadricKind::None {
            let surface = ops
                .solver()
                .surface(i)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let base = surface.surface.base_surface();
            match edge_match(k, base, &curve, ivl, off, tol)? {
                // as wide as the ball along part of the edge only: refused, naming the face's
                // least radius there (hub #2243)
                EdgeMatch::Partial(least) => {
                    return Ok(Some(CollapsedOffset {
                        radius: least,
                        partial: true,
                    }))
                }
                EdgeMatch::None => face.offset(off, tol),
                EdgeMatch::Full => {
                    let (Some(collapsed), Some(Bending { k_max, .. })) = (
                        focal_offset(k, base, uv, off, tol)?,
                        bending_toward_ball(k, base, uv, off)?,
                    ) else {
                        return Ok(None);
                    };
                    // the face's radius there, the one named
                    if let Some(slot) = focal.get_mut(i) {
                        *slot = Some((off, 1.0 / k_max));
                    }
                    collapsed
                }
            }
        } else {
            face.offset(off, tol)
        };
        if let (Some(f), Some(o)) = (faces.get_mut(i), offsets.get_mut(i)) {
            *f = face;
            *o = offset;
        }
    }
    let collapsed = |q: &BlendQuadric| matches!(q.kind, QuadricKind::Line | QuadricKind::Point);
    let Some(c) = offsets.iter().position(collapsed) else {
        return Ok(None);
    };
    let (Some(&axis), Some(&other), Some(face), Some(&focal)) = (
        offsets.get(c),
        offsets.get(1 - c),
        faces.get(c),
        focal.get(c),
    ) else {
        return Ok(None);
    };
    // the other face's offset must be read to judge the ends
    if other.kind == QuadricKind::None {
        return Ok(None);
    }
    let face_radius = focal.map_or(face.radius, |(_, rho)| rho);

    // each end of the edge: does the plane through it square to the edge hold the centre?
    for t in [ivl.min(), ivl.max()] {
        let pv = curve.evaluate(k, t, 1, true, false)?;
        let (Some(&v), Some(&d)) = (pv.first(), pv.get(1)) else {
            return Ok(None);
        };
        let Ok(along) = d.unitize() else {
            return Ok(None);
        };
        if end_runs_on(b, e, v, tol, joined)? {
            continue;
        }
        let closes = if let Some((off, _)) = focal {
            // (solidcore #2652) the focal curve read again at the end's own foot: there it
            // crosses the plane square to the edge (the face's normal there is square to the
            // edge), or (along it) lies in it
            let surface = ops
                .solver()
                .surface(c)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let base = surface.surface.base_surface();
            let domain = base.natural_uv_domain_with_nurb(k)?;
            let dropped =
                base.drop_point::<NoObject>(k, v, &domain, &DropPointOptions::default())?;
            if !dropped.success {
                return Ok(None);
            }
            let Some(at_end) = focal_offset(k, base, dropped.result_uv, off, tol)? else {
                return Ok(None);
            };
            at_end.kind == QuadricKind::Point
                || at_end.dir.dot(along).abs() <= EFF_ZERO_SQRT
                || other.distance(at_end.point) <= approx
        } else if axis.kind == QuadricKind::Point {
            (axis.point - v).dot(along).abs() <= approx
        } else {
            let across_plane = axis.dir.dot(along);
            if across_plane.abs() <= EFF_ZERO_SQRT {
                // the axis lies in the plane (or misses it)
                (axis.point - v).dot(along).abs() <= approx
            } else {
                let x = axis.point + axis.dir * ((v - axis.point).dot(along) / across_plane);
                other.distance(x) <= approx
            }
        };
        if !closes {
            return Ok(Some(CollapsedOffset {
                radius: face_radius,
                partial: false,
            }));
        }
    }
    Ok(None)
}

/// A rail of a constant ball's round or chamfer whose ends lie within the part's
/// resolution of each other ([`rail_below_resolution`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailBelowResolution {
    /// The distance between the rail's ends.
    pub chord: f64,
    /// The resolution: two of the blend's vertex tolerances (its approximation tolerance),
    /// within which the rail's two end vertices would be one.
    pub resolution: f64,
    /// The largest ball radius on this side of the collapse whose rail is longer than the
    /// resolution (an open bound).
    pub fit: f64,
    /// The ball radius at which the rail closes to a point on the axis (the cone's apex
    /// on it: no rail to resolve).
    pub point_size: f64,
    /// The rail lies on the far side of the axis from the edge (its section runs through
    /// the axis): refused whatever its length, `fit` is then `point_size`.
    pub past_axis: bool,
}

/// A coaxial quadric's trace in the meridian half-plane of `axis` through the edge, as a
/// line `(point, unit direction)` in `(r, h)`: `r` the signed distance from the axis
/// along `radial`, `h` along the axis from `axis_point`. A plane square to the axis, a
/// cylinder or its axis, and a cone are lines there; a sphere, a torus or a plane at an
/// angle to the axis have none (`None`). `radius` is the cylinder's signed radius (an
/// offset taken through the axis stays on its far side, as `r < 0`).
fn meridian_line(
    q: &BlendQuadric,
    radius: f64,
    axis_point: Point3,
    axis: Vec3,
    ang_tol: f64,
) -> Option<([f64; 2], [f64; 2])> {
    match q.kind {
        QuadricKind::Plane => {
            if q.dir.cross(axis).length() > ang_tol {
                return None;
            }
            Some(([0.0, (q.point - axis_point).dot(axis)], [1.0, 0.0]))
        }
        QuadricKind::Cylinder | QuadricKind::Line => Some(([radius, 0.0], [0.0, 1.0])),
        QuadricKind::Cone => {
            let s = if q.dir.dot(axis) < 0.0 { -1.0 } else { 1.0 };
            Some((
                [0.0, (q.point - axis_point).dot(axis)],
                [q.angle.sin(), s * q.angle.cos()],
            ))
        }
        _ => None,
    }
}

/// Where two meridian lines meet (`None` when parallel).
fn meet(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])) -> Option<[f64; 2]> {
    let ([px, py], [dx, dy]) = a;
    let ([qx, qy], [ex, ey]) = b;
    let cross = dx * ey - dy * ex;
    if cross.abs() <= EFF_ZERO {
        return None;
    }
    let s = ((qx - px) * ey - (qy - py) * ex) / cross;
    Some([px + s * dx, py + s * dy])
}

/// The foot of `m` on a meridian line (its direction a unit vector).
fn foot_on(line: ([f64; 2], [f64; 2]), m: [f64; 2]) -> [f64; 2] {
    let ([px, py], [dx, dy]) = line;
    let s = (m[0] - px) * dx + (m[1] - py) * dy;
    [px + s * dx, py + s * dy]
}

/// Not the fork's (#2546, WR-739): a
/// constant ball's round or chamfer along an arc whose faces are coaxial quadrics that
/// trace lines in the meridian half-plane (a plane square to the axis, a cylinder, a
/// cone) has its spine on a circle about the axis, and each rail is the circle of the
/// ball's contact with a face: an arc similar to the edge, of radius `r_i` where the
/// edge's is `R_e`, so its ends are `C_e |r_i| / R_e` apart for the edge's chord `C_e`.
/// The rail whose ends come within the resolution (two of the solver's approximation
/// tolerances, `2 tol`: its two end vertices would be one) without closing to a point
/// cannot be represented: it is reported with the size that builds. The spine point is
/// where the faces' offsets by the ball meet in that half-plane, and the rail point its
/// foot on the face, in closed form; each moves linearly with the ball's radius `t`, so
/// `r_i(t) = R_e + t (r_i - R_e) / d` for the solver's radius `d`, and the rail closes
/// to a point at `t0 = -R_e / g` (`g` that slope) and leaves the resolution at
/// `r_i(t) = 2 tol R_e / C_e`. A rail at a point to rounding (`|r_i| <= EFF_ZERO (1 +
/// R_e)`: the ball's centre on the axis, a chamfer as wide as the round it runs round)
/// is not refused: it builds as the cone's apex. A rail on the far side of the axis from
/// the edge (`r_i < 0`: the ball's section runs through the axis, a chamfer wider than the
/// round it runs round) is no blend of the edge whatever its length: it is reported
/// `past_axis`, with `t0` as its fit, the largest size that keeps the rail on the edge's
/// side.
/// `None` for a closed edge, faces of other kinds or placements, and rails on the edge's
/// side longer than the resolution.
///
/// # Errors
///
/// Those of the drops and the evaluations; `SM_ERR_NULL_POINTER` for a solver without
/// its edge use, offset surfaces or edge curve.
pub fn rail_below_resolution(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
) -> Result<Option<RailBelowResolution>> {
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;
    use crate::topology::edge;
    let eu0 = ops
        .solver()
        .edgeuse(0)?
        .ok_or_else(|| SolidError::null_pointer(OP))?;
    let e = eu::edge(b, eu0)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    if edge::is_closed(b, e)? {
        return Ok(None);
    }
    let curve = edge::evaluation_curve(b, e)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone();
    let ivl = edge::interval(b, e)?;
    let tol = b.tolerance();
    let resolution = 2.0 * ops.solver().approx_tol_3d;
    let start = curve.evaluate_point(k, ivl.min())?;
    let mid = curve.evaluate_point(k, ivl.evaluate(0.5))?;
    let end = curve.evaluate_point(k, ivl.max())?;

    // each face's quadric there, and its signed offset by the ball
    let mut faces = [BlendQuadric::default(); 2];
    let mut offs = [0.0; 2];
    for i in 0..2 {
        let (face, p, n, uv) = {
            let off = ops
                .solver()
                .surface(i)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let base = off.surface.base_surface();
            let domain = base.natural_uv_domain_with_nurb(k)?;
            let dropped =
                base.drop_point::<NoObject>(k, mid, &domain, &DropPointOptions::default())?;
            if !dropped.success {
                return Ok(None);
            }
            let uv = dropped.result_uv;
            let face = BlendQuadric::from_surface(k, base, uv, tol)?;
            if face.kind == QuadricKind::None {
                return Ok(None);
            }
            let (p, n) = point_and_normal(k, base, uv)?;
            (face, p, n, uv)
        };
        let q = crate::blend::fillet_solver::p2::eval_offset(k, ops.solver_mut(), i, uv)?;
        if let (Some(f), Some(o)) = (faces.get_mut(i), offs.get_mut(i)) {
            *f = face;
            *o = (q - p).dot(n);
        }
    }
    let [fa, fb] = faces;
    let [oa, ob] = offs;
    let d = oa.abs();
    if d <= tol {
        return Ok(None);
    }

    // the common axis, and the meridian half-plane through the edge's middle
    let reach = tol
        .max(d)
        .max(mid.distance(fa.point))
        .max(mid.distance(fb.point));
    let Some((axis_point, axis)) = coaxial(&fa, &fb, tol, reach) else {
        return Ok(None);
    };
    let ang_tol = tol / reach.max(tol);
    let edge_radius = across(mid - axis_point, axis).length();
    let edge_chord = across(end - start, axis).length();
    if edge_radius <= tol || edge_chord <= tol {
        return Ok(None);
    }
    let lines = |q: &BlendQuadric, off: f64| {
        let moved = if q.kind == QuadricKind::Cylinder {
            let mut m = *q;
            m.radius = q.radius + q.side * off;
            m
        } else {
            q.offset(off, 0.0)
        };
        meridian_line(&moved, moved.radius, axis_point, axis, ang_tol)
    };
    let (Some(face_a), Some(face_b), Some(off_a), Some(off_b)) = (
        lines(&fa, 0.0),
        lines(&fb, 0.0),
        lines(&fa, oa),
        lines(&fb, ob),
    ) else {
        return Ok(None);
    };
    let Some(centre) = meet(off_a, off_b) else {
        return Ok(None);
    };

    let point_zero = EFF_ZERO * (1.0 + edge_radius);
    let mut worst: Option<RailBelowResolution> = None;
    for face in [face_a, face_b] {
        let r = foot_on(face, centre)[0];
        let chord = edge_chord * r.abs() / edge_radius;
        let past_axis = r < 0.0;
        if r.abs() <= point_zero || (!past_axis && chord > resolution) {
            continue;
        }
        let slope = (r - edge_radius) / d;
        if slope.abs() <= EFF_ZERO {
            continue;
        }
        let point_size = -edge_radius / slope;
        // on the edge's side: where the rail's ends come to the resolution, short of t0
        let fit = if past_axis {
            point_size
        } else {
            (resolution * edge_radius / edge_chord - edge_radius) / slope
        };
        let rail = RailBelowResolution {
            chord,
            resolution,
            fit: fit.max(0.0),
            point_size,
            past_axis,
        };
        if worst.is_none_or(|w| rail.fit < w.fit) {
            worst = Some(rail);
        }
    }
    Ok(worst)
}

/// Whether the end of `e` at `at` (within `tol`) is a vertex one of `joined` also ends at:
/// the chain runs on there into another blended edge's round.
fn end_runs_on(b: &Brep, e: EdgeId, at: Point3, tol: f64, joined: &[EdgeId]) -> Result<bool> {
    use crate::topology::{edge, vertex};
    if joined.is_empty() {
        return Ok(false);
    }
    let (start, end) = edge::start_end_vertices(b, e)?;
    for v in [start, end].into_iter().flatten() {
        if vertex::point(b, v)?.distance(at) > tol {
            continue;
        }
        for other in vertex::edges(b, v, None)? {
            if other != e && joined.contains(&other) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Not the fork (solidcore #2652): whether the ball about `blend.point` lies on face `ring`
/// (one of no natural quadric) along an arc, so that its offset by the ball collapses there
/// and the round is the ball's piece ([`classify_faces`]); returns that arc's unit axis.
/// Decided from the geometry, within the solver's tolerance: the ball's centre is
/// `blend.point` at every point of `tsects` (it does not move along the edge), and the
/// ball's great circle through the first and last points' feet on the face lies on the face
/// between them, the face's normal through the centre at each of 16 steps (the ball touches
/// it there, it does not cross it). `None` where any of that fails, or the feet are one
/// point or opposite (no one great circle).
///
/// # Errors
///
/// Those of `SetupOffsetValues`, the evaluations and the drops;
/// `SM_ERR_NULL_POINTER` for a solver without its offset surface.
fn ball_on_face_along_arc(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    blend: &ExactBlend,
    ring: usize,
    tsects: &[TsectPnt],
) -> Result<Option<Vec3>> {
    use crate::blend::fillet_solver::p2::eval_offset;
    use crate::surface::surface::p4::NoObject;
    use crate::surface::surface::p5::DropPointOptions;
    const STEPS: u32 = 16;
    let (Some(first), Some(last)) = (tsects.first(), tsects.last()) else {
        return Ok(None);
    };
    let tol = ops.solver().approx_tol_3d;
    let (centre, radius) = (blend.point, blend.radius);
    for t in tsects {
        ops.setup_offset_values(t, None)?;
        let c = eval_offset(k, ops.solver_mut(), 0, t.uv_pos(SrfIndex::First))?;
        if c.distance(centre) > tol {
            return Ok(None);
        }
    }
    let index = if ring == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    };
    let surface = ops
        .solver()
        .surface(ring)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .surface
        .base_surface();
    let mut feet = [Vec3::ZERO; 2];
    for (foot, t) in feet.iter_mut().zip([first, last]) {
        *foot = surface.evaluate_point(k, t.uv_pos(index))? - centre;
        if (foot.length() - radius).abs() > tol {
            return Ok(None);
        }
    }
    let [e0, e1] = feet;
    let Ok(axis) = e0.cross(e1).unitize() else {
        return Ok(None);
    };
    let (Ok(x), Ok(y)) = (e0.unitize(), axis.cross(e0).unitize()) else {
        return Ok(None);
    };
    let turn = e1.dot(y).atan2(e1.dot(x));
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    for i in 1..STEPS {
        let a = turn * f64::from(i) / f64::from(STEPS);
        let p = centre + (x * a.cos() + y * a.sin()) * radius;
        let dropped =
            surface.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(None);
        }
        let (q, n) = point_and_normal(k, surface, dropped.result_uv)?;
        if q.distance(p) > tol || n.dot(p - centre).abs() < radius * (1.0 - EFF_ZERO_SQRT) {
            return Ok(None);
        }
    }
    Ok(Some(axis))
}

/// `SmFilletSolver::CalcExactFilletGeom(crBlend, rTsectPnts, pFG, bRecalc, rbBuilt)`: the
/// exact surface of a constant round by its class, stored in `fillet_geom` (`None`: the
/// solver's first fillet geometry) of `pseudo` with its rails; `recalc` is the surface
/// rebuilt through refined points (`SmFilletGeom::ReCalcFilletGeom`). Returns whether it
/// was built (`false`: the round is traced).
///
/// A line spine is the cylinder about it (a chamfer on one, or one whose trace ends at one
/// centre, is traced); a circle the section turned about the common axis; a point the
/// ball's piece between two contact circles (not rebuilt on a recalc), or with one face's
/// offset collapsed and the other's not, the ball's triangle (solidcore #2403, rebuilt).
///
/// A geometry built here is marked [`FilletGeom::exact_class`](crate::blend::fillet_geom::p1::FilletGeom::exact_class)
/// (not upstream, #2489).
///
/// # Errors
///
/// Those of the builders (a failure in them is an error, not a trace), of
/// `SetupOffsetValues` and the evaluations.
#[allow(clippy::too_many_arguments)]
pub fn calc_exact_fillet_geom(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
    pseudo: &mut FilletBrep,
    blend: &ExactBlend,
    tsects: &[TsectPnt],
    fillet_geom: Option<FilletGeomId>,
    recalc: bool,
) -> Result<bool> {
    let built = exact_fillet_geom_of_class(ops, k, b, pseudo, blend, tsects, fillet_geom, recalc)?;
    if built {
        let g = match fillet_geom {
            Some(g) => g,
            None => ops.solver().first_fillet_geom()?,
        };
        pseudo.geom_mut(g)?.exact_class = true;
        if k.fixes.fillet_round_as_definition {
            keep_definition(ops, k, pseudo, blend, fillet_geom)?;
        }
    }
    Ok(built)
}

/// Not the fork (#2485, with `fillet_round_as_definition`): the round just built by its
/// exact class keeps its definition on its surface ([`ExactRound`]): its supports (the
/// faces the class was read off), the ball's radius, the ball's circle as its section and
/// the centre line as its leader. Its net is its exact surface, re-certified at once
/// ([`ExactRound::recertify`]: the class read again from the supports, the net on the
/// class's surface). A round of a linear section (a chamfer) or of a nodal quartic (its
/// spine on its offsets only to 2e-9 of the radius) keeps none here.
///
/// # Errors
///
/// Those of the re-certification (a class's surface not exact is refused, not kept with a
/// bound it does not have); `SM_ERR_NULL_POINTER` for a solver without its offset surfaces;
/// those of copying the supports.
fn keep_definition(
    ops: &dyn FilletSolverOps,
    k: &Kernel,
    pseudo: &mut FilletBrep,
    blend: &ExactBlend,
    fillet_geom: Option<FilletGeomId>,
) -> Result<()> {
    use crate::curve::curve::CurveKind;
    let (Some(class), Some(inputs)) = (ExactClass::of_spine(blend.spine), blend.inputs) else {
        return Ok(());
    };
    // a nodal quartic's spine is on its offsets only to 2e-9 of the ball's radius (its
    // construction's; the fork's own bar is 1e-9): no bound of 0, so it keeps no definition
    if class == ExactClass::NodalQuartic {
        return Ok(());
    }
    let s = ops.solver();
    let circular = s
        .fsg
        .as_ref()
        .is_some_and(|g| g.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Circular);
    if !circular {
        return Ok(());
    }
    let support = |i: usize| -> Result<&Surface> {
        Ok(s.surface(i)?
            .ok_or_else(|| SolidError::null_pointer(OP))?
            .surface
            .base_surface())
    };
    let g = match fillet_geom {
        Some(g) => g,
        None => s.first_fillet_geom()?,
    };
    let leader = match pseudo
        .geom(g)?
        .center_line_curve
        .as_ref()
        .map(crate::curve::curve::Curve::kind)
    {
        Some(CurveKind::BSpline(c)) => Some(c),
        Some(CurveKind::Line(c)) => Some(c.base()),
        Some(CurveKind::Ellipse(c)) => Some(c.base()),
        _ => None,
    };
    let round = ExactRound::create(
        k,
        class,
        [support(0)?, support(1)?],
        blend.radius,
        leader,
        inputs,
    )?;
    let Some(surface) = pseudo.geom_mut(g)?.fillet_surface_mut(None)? else {
        return Err(SolidError::null_pointer(OP));
    };
    round.recertify(k, surface)?;
    surface.set_exact_round(Some(round));
    Ok(())
}

/// [`calc_exact_fillet_geom`]'s build by the class (`CalcExactFilletGeom`), without the
/// definition kept.
#[allow(clippy::too_many_arguments)]
fn exact_fillet_geom_of_class(
    ops: &mut dyn FilletSolverOps,
    k: &Kernel,
    b: &Brep,
    pseudo: &mut FilletBrep,
    blend: &ExactBlend,
    tsects: &[TsectPnt],
    fillet_geom: Option<FilletGeomId>,
    recalc: bool,
) -> Result<bool> {
    use crate::blend::fillet_solver::p2::eval_offset;
    let (Some(first), Some(last)) = (tsects.first(), tsects.last()) else {
        return Ok(false);
    };
    // `m_pFSG->GetFilletSurfaceGeneratorType()` (read only by the line and circle arms; a
    // classified round has a generator)
    let section = |ops: &dyn FilletSolverOps| {
        ops.solver()
            .fsg
            .as_ref()
            .map(crate::blend::fillet_solver::p1::FilletSurfaceGenerator::fillet_surface_generator_type)
            .ok_or_else(|| SolidError::null_pointer(OP))
    };
    match blend.spine {
        BlendSpine::Line => {
            // a chamfer on a straight spine is traced (its line from rail to rail is the
            // face's plane)
            if section(ops)? != FilletSurfaceGeneratorType::Circular {
                return Ok(false);
            }
            // the trace's ends at one centre (the corners at its ends take the whole edge):
            // there is no cylinder along it, and the trace's handling of such a round stands
            let mut c = [Point3::ZERO; 2];
            for (ci, t) in c.iter_mut().zip([first, last]) {
                ops.setup_offset_values(t, None)?;
                *ci = eval_offset(k, ops.solver_mut(), 0, t.uv_pos(SrfIndex::First))?;
            }
            if c[0].distance(c[1]) <= EFF_ZERO_SQRT * 1.0_f64.max(blend.radius) {
                return Ok(false);
            }
            FilletSolver::calc_line_spine_fillet_geom(
                ops,
                k,
                b,
                pseudo,
                blend,
                tsects,
                fillet_geom,
            )?;
            Ok(true)
        }
        BlendSpine::Circle => {
            // not the fork: a linear section's revolution stays behind WR-421's flag
            if section(ops)? == FilletSurfaceGeneratorType::Linear
                && !k.fixes.fillet_chamfer_cone_on_arc
            {
                return Ok(false);
            }
            FilletSolver::calc_circle_spine_fillet_geom(
                ops,
                k,
                b,
                pseudo,
                blend,
                tsects,
                fillet_geom,
            )?;
            Ok(true)
        }
        BlendSpine::Point => {
            // (solidcore #2403) one face's offset collapsed onto a line or circle, the other a
            // surface: the ball touches the one along a circle and the other at a point, and
            // its piece is the ball's triangle, rebuilt on a recalc from its new ends
            let collapsed =
                |q: &BlendQuadric| matches!(q.kind, QuadricKind::Line | QuadricKind::Circle);
            if collapsed(&blend.offsets[0]) != collapsed(&blend.offsets[1]) {
                // (solidcore #2652) a face of no natural quadric: the ball's piece only where
                // its geometry says so, its contact arc's axis then the collapsed offset's
                let mut blend = *blend;
                if let Some(ring) = blend.faces.iter().position(|f| f.kind == QuadricKind::None) {
                    let Some(axis) = ball_on_face_along_arc(ops, k, &blend, ring, tsects)? else {
                        return Ok(false);
                    };
                    if let Some(o) = blend.offsets.get_mut(ring) {
                        o.dir = axis;
                    }
                }
                FilletSolver::calc_ball_triangle_fillet_geom(
                    ops,
                    k,
                    pseudo,
                    &blend,
                    tsects,
                    fillet_geom,
                )?;
                return Ok(true);
            }
            if recalc {
                return Ok(false);
            }
            FilletSolver::calc_sphere_fillet_geom(ops, k, b, pseudo, tsects, fillet_geom)?;
            Ok(true)
        }
        // tangency-labs/smlib@0061c41 (conic) and 1b5f1e7 (conic pair): the exact
        // rational surface of the ball's arcs
        BlendSpine::Conic | BlendSpine::ConicPair => {
            rational::calc_rational_spine_fillet_geom(
                ops,
                k,
                b,
                pseudo,
                blend,
                tsects,
                fillet_geom,
            )?;
            Ok(true)
        }
        // tangency-labs/smlib@dda3b5d: faces touching at one point, exact on their nodal
        // quartic
        BlendSpine::NodalQuartic => {
            // A round that runs into the node ends there tangentially: its contacts merge,
            // the end event of the trace (the tangent end), which takes the traced round
            // only. Such a round is traced until that event takes the exact one.
            let tol = b.tolerance();
            for t in [first, last] {
                ops.setup_offset_values(t, None)?;
                let centre = eval_offset(k, ops.solver_mut(), 0, t.uv_pos(SrfIndex::First))?;
                if centre.distance(blend.point) <= tol {
                    return Ok(false);
                }
            }
            rational::calc_rational_spine_fillet_geom(
                ops,
                k,
                b,
                pseudo,
                blend,
                tsects,
                fillet_geom,
            )?;
            Ok(true)
        }
        BlendSpine::Traced => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(point: Point3, normal: Vec3) -> BlendQuadric {
        BlendQuadric {
            kind: QuadricKind::Plane,
            point,
            dir: normal,
            ..BlendQuadric::default()
        }
    }

    fn cylinder(point: Point3, axis: Vec3, radius: f64, side: f64) -> BlendQuadric {
        BlendQuadric {
            kind: QuadricKind::Cylinder,
            point,
            dir: axis,
            radius,
            side,
            ..BlendQuadric::default()
        }
    }

    fn classify(a: BlendQuadric, b: BlendQuadric, r: f64, centre: Point3) -> ExactBlend {
        let mut e = ExactBlend {
            offsets: [a, b],
            radius: r,
            ..ExactBlend::default()
        };
        classify_blend_spine(centre, 1e-7, &mut e);
        e
    }

    #[test]
    fn two_planes_meet_in_a_line() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let e = classify(
            plane(Point3::ZERO, x),
            plane(Point3::ZERO, z),
            1.0,
            Point3::ZERO,
        );
        assert_eq!(e.spine, BlendSpine::Line);
        // parallel planes: no spine
        let e = classify(plane(Point3::ZERO, z), plane(z, z), 1.0, Point3::ZERO);
        assert_eq!(e.spine, BlendSpine::Traced);
    }

    #[test]
    fn plane_and_cylinder_by_angle() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let c = cylinder(Point3::ZERO, z, 3.0, 1.0);
        // square to the axis: circle about it
        let e = classify(
            plane(Point3::new(0.0, 0.0, 1.0), z),
            c,
            1.0,
            Point3::new(3.0, 0.0, 1.0),
        );
        assert_eq!(e.spine, BlendSpine::Circle);
        assert_eq!(e.dir, z);
        // parallel to the axis: a line
        let e = classify(
            plane(Point3::new(4.0, 0.0, 0.0), x),
            c,
            1.0,
            Point3::new(4.0, 0.0, 0.0),
        );
        assert_eq!(e.spine, BlendSpine::Line);
        // oblique: a conic
        let n = Vec3::new(1.0, 0.0, 1.0).unitize().unwrap();
        let e = classify(plane(Point3::ZERO, n), c, 1.0, Point3::new(3.0, 0.0, -3.0));
        assert_eq!(e.spine, BlendSpine::Conic);
    }

    #[test]
    fn two_lines_meeting_are_a_point_and_coaxial_cylinders_no_spine() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let a = BlendQuadric {
            kind: QuadricKind::Line,
            dir: z,
            ..BlendQuadric::default()
        };
        let b = BlendQuadric {
            kind: QuadricKind::Line,
            dir: x,
            ..BlendQuadric::default()
        };
        let e = classify(a, b, 2.0, Point3::ZERO);
        assert_eq!(e.spine, BlendSpine::Point);
        assert_eq!(e.point, Point3::ZERO);
        let e = classify(
            cylinder(Point3::ZERO, z, 1.0, 1.0),
            cylinder(Point3::ZERO, z, 2.0, 1.0),
            1.0,
            Point3::new(1.0, 0.0, 0.0),
        );
        assert_eq!(e.spine, BlendSpine::Traced);
    }

    #[test]
    fn offsets_collapse_cylinders_and_spheres() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let c = cylinder(Point3::ZERO, z, 2.0, -1.0).offset(2.0, 1e-9);
        assert_eq!(c.kind, QuadricKind::Line);
        let c = cylinder(Point3::ZERO, z, 2.0, -1.0).offset(3.0, 1e-9);
        assert_eq!(
            (c.kind, c.radius, c.side),
            (QuadricKind::Cylinder, 1.0, 1.0)
        );
        let cone = BlendQuadric {
            kind: QuadricKind::Cone,
            dir: z,
            angle: std::f64::consts::FRAC_PI_4,
            ..BlendQuadric::default()
        };
        let p = Point3::new(1.0, 0.0, 1.0);
        assert!(cone.distance(p) < 1e-15);
        let n = cone.normal(p);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!((n - Vec3::new(s, 0.0, -s)).length() < 1e-15);
        // the offset passes through p + n
        assert!(cone.offset(1.0, 1e-9).distance(p + n) < 1e-14);
        let (a, bb, c) = cone.implicit();
        let x = [p.x, p.y, p.z];
        let mut v = c;
        for i in 0..3 {
            v += 2.0 * bb[i] * x[i];
            for j in 0..3 {
                v += x[i] * a[i][j] * x[j];
            }
        }
        assert!(v.abs() < 1e-14);
    }

    fn collapsed(a: BlendQuadric, b: BlendQuadric, r: f64, centre: Point3) -> ExactBlend {
        let mut e = classify(a, b, r, centre);
        assert_eq!(e.spine, BlendSpine::Traced, "no class of the fork's");
        classify_ball_on_collapsed_offset(centre, 1e-7, &mut e);
        e
    }

    fn torus(major: f64, tube: f64, side: f64) -> BlendQuadric {
        BlendQuadric {
            kind: QuadricKind::Torus,
            dir: Vec3::new(0.0, 0.0, 1.0),
            radius: major,
            tube,
            side,
            ..BlendQuadric::default()
        }
    }

    #[test]
    fn a_torus_moved_in_by_its_tube_is_its_circle() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let t = torus(5.0, 2.0, -1.0);
        let p = Point3::new(5.0, 0.0, 2.0);
        // its normal towards its circle (side -1), along which it moves in
        assert!(t.distance(p) < 1e-15);
        assert!((t.normal(p) + z).length() < 1e-15);
        let c = t.offset(2.0, 1e-9);
        assert_eq!((c.kind, c.radius, c.tube), (QuadricKind::Circle, 5.0, 0.0));
        assert!(c.distance(Point3::new(0.0, -5.0, 0.0)) < 1e-15);
        assert!((c.distance(Point3::ZERO) - 5.0).abs() < 1e-15);
        // moved on past its circle: the tube the other way round
        let o = t.offset(3.0, 1e-9);
        assert_eq!((o.kind, o.tube, o.side), (QuadricKind::Torus, 1.0, 1.0));
        // no implicit form, no class of the fork's
        assert_eq!(t.implicit(), ([[0.0; 3]; 3], [0.0; 3], 0.0));
        let e = classify(t, plane(Point3::ZERO, z), 1.0, p);
        assert_eq!(e.spine, BlendSpine::Traced);
    }

    #[test]
    fn a_collapsed_offset_crossed_by_the_other_is_the_ball() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        let line = cylinder(Point3::ZERO, x, 2.0, -1.0).offset(2.0, 1e-9);
        assert_eq!(line.kind, QuadricKind::Line);
        // a plane across the line: the one ball about the crossing, either way round
        let at = Point3::new(3.0, 0.0, 0.0);
        let n = Vec3::new(1.0, 1.0, 0.0).unitize().unwrap();
        let e = collapsed(line, plane(at, n), 2.0, at + y * 1e-9);
        assert_eq!(e.spine, BlendSpine::Point);
        assert!(e.point.distance(at) < 1e-15, "{:?}", e.point);
        let e = collapsed(plane(at, n), line, 2.0, at);
        assert_eq!(e.spine, BlendSpine::Point);
        // a plane the line runs in, or one it runs alongside: traced
        let e = collapsed(line, plane(Point3::ZERO, z), 2.0, at);
        assert_eq!(e.spine, BlendSpine::Traced);
        let e = collapsed(line, plane(z, z), 2.0, at);
        assert_eq!(e.spine, BlendSpine::Traced);
        // a crossing away from the centre is not this ball
        let e = collapsed(line, plane(at, n), 2.0, Point3::new(4.0, 0.0, 0.0));
        assert_eq!(e.spine, BlendSpine::Traced);

        // a torus's circle crossed by a plane through its axis
        let circle = torus(5.0, 2.0, -1.0).offset(2.0, 1e-9);
        let at = Point3::new(5.0, 0.0, 0.0);
        let e = collapsed(circle, plane(Point3::ZERO, y), 2.0, at + y * 1e-9);
        assert_eq!(e.spine, BlendSpine::Point);
        assert!(e.point.distance(at) < 1e-14, "{:?}", e.point);
        // the circle's own plane, and a plane touching it: traced
        let e = collapsed(circle, plane(Point3::ZERO, z), 2.0, at);
        assert_eq!(e.spine, BlendSpine::Traced);
        let e = collapsed(circle, plane(at, x), 2.0, at);
        assert_eq!(e.spine, BlendSpine::Traced);
        // two collapsed offsets are not this class
        let e = collapsed(circle, line, 2.0, at);
        assert_eq!(e.spine, BlendSpine::Traced);
    }
}
