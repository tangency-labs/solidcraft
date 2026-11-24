//! Whether a convex blend would take all of the part between its rails somewhere along its
//! edge, cutting it into pieces, and the largest size that keeps it whole.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork (tangency-labs/smlib#79,
//! merged at 5e8c9c1) adds `SmFilletSolver::BandCutsThePart` with `sm_EdgesCrossingPlane`
//! and `sm_KeepsMaterialBehind` (`SmFilletSolver.cpp`, `SmFilletSolver.h`) in commits
//! 862971f, e568e45 (the section intersected with every face, not sampled) and aac4288 (a
//! run of cutting sections reaching an end of the edge is no cut).
//!
//! | Rust                          | Upstream (fork)                          |
//! |-------------------------------|------------------------------------------|
//! | [`band_cuts_the_part`]        | `SmFilletSolver::BandCutsThePart`        |
//! | [`BandCut`]                   | its outputs `rbCuts`, `rdRadius`, `rdSetback` |
//! | `edges_crossing_plane` (private) | `sm_EdgesCrossingPlane`               |
//! | `keeps_material_behind` (private) | `sm_KeepsMaterialBehind`             |
//! | `keeps` (private)             | the `Keeps` lambda of `BandCutsThePart`  |
//!
//! Not the fork's: [`band_breaks_through_the_part`] (WR-707, #2299), the same sections
//! judged for breaking through the part into a hollow behind a wall, less the hollows a
//! concave round of the same blend fills ([`ConcaveFill`]).
//!
//! Deviations: the fork's out-params are returned (`None` for no cut). The solutions of
//! `SmSurface::GlobalCurveIntersect` carry no object handles (the fork reads only their
//! parameters). Every loop is bounded by fixed counts (32 steps, 40 bisections, halvings
//! down to a millionth of the radius) or by the part's edges and faces (the stretches
//! between a band's events); there is no recursion.

// the fork's NaN-safe tests (`!(a > b)`) are kept as written
#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::blend::fillet_executive::session::ball_regular::{
    ball_law, ball_section_at, rolls_a_ball, set_ball_radius,
};
use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::fillet_solver::p1::{FilletSurfaceGeneratorType, SeamDomain};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::p2::EllipseOptions;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::blend::BlendFailure;
use crate::error::EntityRef;
use crate::error::{Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::query::brep::BrepQuery;
use crate::surface::plane::p1::Plane;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{face, vertex, Brep, FaceId, VertexId};
use std::collections::BTreeSet;

const OP: &str = "blend::fillet_executive::session::band_cuts";

/// A reviewed defect for something the band cut reads that the kernel built and should be
/// there, `why` saying why (status `SM_ERR_NULL_POINTER`, the fork's `NER`).
fn null_defect(why: &'static str) -> SolidError {
    Failure::new(OP, BlendFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// The solver's first edge use, `edgeuse`, has no edge. The caller passes both the body and
/// the solver (whose edge uses are public), so this is the caller's input:
/// [`BlendFailure::EdgeUseWithoutEdge`] at the edge use, status `SM_ERR_NULL_POINTER`.
fn edge_use_without_edge(edgeuse: crate::topology::EdgeuseId) -> SolidError {
    SolidError::blend_at(
        OP,
        BlendFailure::EdgeUseWithoutEdge,
        EntityRef::Edgeuse(edgeuse),
    )
}

/// Why a ball section's solver has both offset surfaces: the section was found on them
/// ([`ball_section_at`] reads both), and nothing between takes one away.
const BALL_OFFSETS: &str = "a ball section's solver has no offset surface on a side: the section \
     was just found on both offset surfaces, and nothing since removes one";

/// The curve of `edge`, the filleted edge of the caller's body.
fn filleted_edge_curve(target: &Brep, edge: crate::topology::EdgeId) -> Result<Curve> {
    edge::evaluation_curve(target, edge)?.ok_or_else(|| {
        SolidError::blend_at(OP, BlendFailure::EdgeWithoutCurve, EntityRef::Edge(edge))
    })
}

/// `lSteps`: the sections along the edge.
const STEPS: usize = 32;

/// `lBisections`: the bisection steps on the ball's radius.
const BISECTIONS: usize = 40;

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// Where a convex blend would cut the part ([`band_cuts_the_part`]): the largest ball
/// radius that keeps it whole, least over the cutting sections (`rdRadius`), and that
/// radius's setback from the edge to the first rail (`rdSetback`, a chamfer's size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandCut {
    /// `rdRadius`.
    pub radius: f64,
    /// `rdSetback`.
    pub setback: f64,
}

#[derive(Debug)]
struct CachedEdge {
    curve: Curve,
    natural_interval: Extent1d,
    interval: Extent1d,
    bbox: Option<Extent3d>,
}

#[derive(Debug)]
struct CachedFace {
    id: FaceId,
    surface: Surface,
    domain: Extent2d,
    bbox: Option<Extent3d>,
}

/// Precomputed target-body topology, bounding boxes, and [`BrepQuery`] (with its kept
/// `BrepCache`, `Extent3d`, and `KeptFaceCaches`) shared across all station and solver
/// checks in [`band_cuts_the_part_cached`] and [`band_breaks_through_the_part_cached`].
#[derive(Debug)]
pub(crate) struct BandCutCache {
    edges: Vec<CachedEdge>,
    faces: Vec<CachedFace>,
    /// Whether some edge has no curve or some face no surface: those are not in `edges`
    /// or `faces`, so nothing about the band is known in closed form ([`Band::events`]).
    unseen: bool,
    query: BrepQuery<'static>,
}

impl BandCutCache {
    fn new(k: &Kernel, body: &mut Brep) -> Result<Self> {
        let mut edges = Vec::new();
        let mut unseen = false;
        for e in crate::topology::brep::p1::edges(body, None)? {
            let Some(curve) = edge::evaluation_curve(body, e)? else {
                unseen = true;
                continue;
            };
            let interval = edge::interval(body, e)?;
            let natural_interval = curve.natural_interval();
            let bbox = curve
                .calculate_bounding_box(
                    k,
                    &interval,
                    BoundingBoxRequest {
                        normal: true,
                        pseudo: false,
                        polar: false,
                    },
                    false,
                )
                .ok()
                .and_then(|b| b.normal);
            edges.push(CachedEdge {
                curve,
                natural_interval,
                interval,
                bbox,
            });
        }
        let mut faces = Vec::new();
        for f in crate::topology::brep::p1::faces(body, None)? {
            let Some(surface) = face::p2::surface(body, f)? else {
                unseen = true;
                continue;
            };
            let surface = surface.clone_for_evaluation()?;
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            let bbox = crate::query::face::p1::calculate_bounding_box(k, &mut *body, f, false).ok();
            faces.push(CachedFace {
                id: f,
                surface,
                domain,
                bbox,
            });
        }
        let query = BrepQuery::of_brep(body);
        Ok(Self {
            edges,
            faces,
            unseen,
            query,
        })
    }
}

/// The operands of the plane's intersections: an edge's curve and the plane, named as
/// upstream's solutions name them (the caches they build own their trees).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Operand {
    /// The edge's curve, with its natural interval.
    Curve(Extent1d),
    /// The plane, with its natural domain.
    Plane(Extent2d),
}

impl SolutionObject for Operand {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Curve(_) => ObjectKind::Curve,
            Self::Plane(_) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Self::Curve(ivl) => Some(*ivl),
            Self::Plane(_) => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Self::Plane(domain) => Some(*domain),
            Self::Curve(_) => None,
        }
    }
}

/// `sm_EdgesCrossingPlane`: the points where `body`'s edges cross the plane through
/// `origin` square to the unit `normal`, each edge's curve intersected with the plane; an
/// edge lying in the plane gives its ends there.
fn edges_crossing_plane(
    k: &Kernel,
    body: &Brep,
    cache: &BandCutCache,
    origin: Point3,
    normal: Vec3,
) -> Result<Vec<Point3>> {
    let mut crossings = Vec::new();
    let x = if normal.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0).cross(normal)
    } else {
        Vec3::new(0.0, 1.0, 0.0).cross(normal)
    };
    let x = x.unitize()?;
    let y = normal.cross(x);
    let plane = Surface::from(Plane::create_canonical(&Axis2Placement::new(origin, x, y))?);
    let domain = plane.natural_uv_domain_with_nurb(k)?;
    let tol = body.tolerance();
    let plane_tol = smos_max(tol, 1.0e-4);
    for edge in &cache.edges {
        if let Some(bbox) = &edge.bbox {
            if bbox.distance_to_plane(origin, normal).abs() > plane_tol {
                continue;
            }
        }
        let solutions = plane.global_curve_intersect(
            k,
            [
                Operand::Curve(edge.natural_interval),
                Operand::Plane(domain),
            ],
            &domain,
            &edge.curve,
            &edge.interval,
            tol,
        )?;
        for sol in solutions.as_slice() {
            let [t, ..] = sol.start.parameters;
            crossings.push(edge.curve.evaluate_point(k, t)?);
            if sol.solution_type == SolutionType::RangeOfValues {
                let [t, ..] = sol.end.parameters;
                crossings.push(edge.curve.evaluate_point(k, t)?);
            }
        }
    }
    Ok(crossings)
}

/// `sm_KeepsMaterialBehind`: whether the part keeps material behind the section of a
/// convex blend at a station, the region between the section and the ball's centre
/// `centre` (the ball's sector between the rails for a round, the triangle of the centre
/// and the rails for a chamfer's `chord`).
///
/// Kept where a body edge crosses the station's plane inside that region (`crossings`),
/// or the section (as an exact curve) crosses a face's surface inside the face. With
/// neither, one point of the section classified against the body says whether it lies
/// inside the part; outside, the blend takes all of the part between its rails there.
fn keeps_material_behind(
    k: &Kernel,
    body: &mut Brep,
    cache: &BandCutCache,
    centre: Point3,
    rails: [Point3; 2],
    chord: bool,
    crossings: &[Point3],
) -> Result<bool> {
    let [rail1, rail2] = rails;
    let a = rail1 - centre;
    let b = rail2 - centre;
    let r2 = smos_max(a.length_squared(), b.length_squared());
    let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
    let det = aa * bb - ab * ab;
    if !(det > EFF_ZERO * aa * bb) {
        return Ok(true); // rails opposite or together: no region to judge
    }
    let tol = 1.0e-9 * r2.sqrt();
    for &crossing in crossings {
        let x = crossing - centre;
        let (xa, xb) = (x.dot(a), x.dot(b));
        let alpha = (xa * bb - xb * ab) / det;
        let beta = (xb * aa - xa * ab) / det;
        if alpha < -1.0e-12 || beta < -1.0e-12 {
            continue;
        }
        let inside = if chord {
            alpha + beta <= 1.0 + 1.0e-12
        } else {
            x.length() <= r2.sqrt() + tol
        };
        if inside {
            return Ok(true);
        }
    }

    // the section as an exact curve: the chord, or the circle's arc from the first rail to the
    // second the short way
    let (section, middle) = if chord {
        let line = Line::create_line_segment(3, rail1, rail2, None)?;
        (Curve::from(line), (rail1 + rail2) * 0.5)
    } else {
        let rho = aa.sqrt();
        let e1 = a / rho;
        let e2 = (b - a * (ab / aa)).unitize()?;
        let phi = b.dot(e2).atan2(b.dot(e1));
        let circle = Ellipse::new_circle(
            k,
            centre,
            e1,
            e2,
            Extent1d::from_min_max(0.0, rad_to_deg(phi)),
            rho,
            &EllipseOptions {
                dim: 3,
                ..EllipseOptions::default()
            },
        )?;
        let middle = centre + e1 * (rho * (0.5 * phi).cos()) + e2 * (rho * (0.5 * phi).sin());
        (Curve::from(circle), middle)
    };
    let interval = section.natural_interval();
    let ends = body.tolerance();
    let zone = brep_zone_tol_3d(k, body, SmallTopoType::default());
    let sec_box = section
        .calculate_bounding_box(
            k,
            &interval,
            BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            },
            false,
        )
        .ok()
        .and_then(|b| b.normal);
    for face in &cache.faces {
        if let (Some(f_box), Some(s_box)) = (&face.bbox, &sec_box) {
            if f_box.are_disjoint(s_box, ends) {
                continue;
            }
        }
        let f = face.id;
        // the crossings inside the face's surface's domain, judged after the borrow ends
        let mut on_face: Vec<Point2> = Vec::new();
        {
            // the face's surface solves through its face (its trimmed cache), as upstream's
            // `GlobalCurveIntersect` reaches the owner's cache
            let solutions = cache
                .query
                .face_caches()
                .with_services(k, &mut *body, |services| {
                    face.surface
                        .global_curve_intersect_with_context::<CacheObject>(
                            k,
                            [None, Some(CacheObject::Surface(f))],
                            None,
                            services,
                            &face.domain,
                            &section,
                            &interval,
                            ends,
                        )
                })?;
            for sol in solutions.as_slice() {
                if sol.solution_type == SolutionType::RangeOfValues {
                    return Ok(true); // the section runs along the face's surface
                }
                let [t, u, v, ..] = sol.start.parameters;
                let at = section.evaluate_point(k, t)?;
                if at.distance(rail1) <= ends || at.distance(rail2) <= ends {
                    continue; // the section's own ends, on the blended faces
                }
                on_face.push(Point2::new(u, v));
            }
        }
        for uv in on_face {
            let mut class = PointClassification::<EntityRef>::new(zone);
            cache.query.face_caches().point_classify(
                k,
                body,
                f,
                uv,
                &mut class,
                crate::query::face::p2::PointClassifyOptions {
                    src_zone_tol_3d: zone,
                    do_boundary_intersections: true,
                    use_uv_classification: true,
                    ..crate::query::face::p2::PointClassifyOptions::default()
                },
            )?;
            if class.point_class() == PointClassType::Face {
                return Ok(true); // the section crosses the part's boundary inside a face
            }
        }
    }
    // no crossing: the section between its rails is wholly in the part or wholly outside it
    let class = cache.query.point_3d_classify(k, body, middle, zone, true)?;
    if class.point_class() != PointClassType::Region {
        return Ok(true);
    }
    if let Some(EntityRef::Region(region)) = class.object() {
        if body.region(region)?.is_void() == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One section judged by `keeps`.
#[derive(Debug, Clone, Copy, Default)]
struct Section {
    /// `rbJudged`: the section was found.
    found: bool,
    /// `rbKeeps`: material is kept behind it.
    keeps: bool,
    /// `rdSetbackHere`: the distance from the edge to the first rail.
    setback: f64,
}

/// The ball's section at the edge's `t` (`near` the guess): the solve, and the ball's
/// centre and its two rails (on the base surfaces, or on their extensions past their
/// domains, as the offsets evaluate them). `None` where the solve finds no section.
fn section_points(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    edge_curve: &Curve,
    t: f64,
    near: Option<&TsectPnt>,
) -> Result<Option<(TsectPnt, [Point3; 3])>> {
    let Some(pnt) = ball_section_at(k, target, solver, edge_curve, t, near, SeamDomain::Wrapped)?
    else {
        return Ok(None);
    };
    let uvs = [pnt.uv_pos(SrfIndex::First), pnt.uv_pos(SrfIndex::Second)];
    let base = solver.base_mut();
    let mut at = [Point3::UNDEF; 3];
    {
        let slot = base
            .surface_mut(0)?
            .ok_or_else(|| null_defect(BALL_OFFSETS))?;
        at[0] = slot.surface.surface.evaluate_point_extending(k, uvs[0])?;
    }
    for (rail, uv) in uvs.into_iter().enumerate() {
        let slot = base
            .surface(rail)?
            .ok_or_else(|| null_defect(BALL_OFFSETS))?;
        let offset = &slot.surface.surface;
        let p = offset.base_surface().evaluate_point(k, uv)?;
        if let Some(dst) = at.get_mut(rail + 1) {
            *dst = p;
        }
        // a rail past its base surface's domain lies on its extension, as the offset
        // evaluates it
        if let Some(extended) = offset.extended_base_surface() {
            if !offset
                .base_surface()
                .natural_uv_domain_with_nurb(k)?
                .contains_point(uv, 0.0)
                && extended
                    .natural_uv_domain_with_nurb(k)?
                    .contains_point(uv, 0.0)
            {
                let p = extended.evaluate_point(k, uv)?;
                if let Some(dst) = at.get_mut(rail + 1) {
                    *dst = p;
                }
            }
        }
    }
    Ok(Some((pnt, at)))
}

/// The `Keeps` lambda of `BandCutsThePart`: at the edge's `t` with the ball's radius
/// `trial` (0: the solver's own, restored to `own` after), whether material is kept behind
/// the section; `near` is the guess, then this section if found.
#[allow(clippy::too_many_arguments)]
fn keeps(
    k: &Kernel,
    target: &mut Brep,
    cache: &BandCutCache,
    solver: &mut SolverObject,
    edge_curve: &Curve,
    chord: bool,
    own: f64,
    t: f64,
    trial: f64,
    judge: Judge<'_>,
    near: &mut Option<TsectPnt>,
) -> Result<Section> {
    let mut out = Section {
        found: false,
        keeps: true,
        setback: 0.0,
    };
    if trial > 0.0 && !set_ball_radius(solver, trial)? {
        return Ok(out);
    }
    let evaluated = section_points(k, target, solver, edge_curve, t, near.as_ref());
    if trial > 0.0 {
        // the solver's own radius back, whatever the solve did
        let _ = set_ball_radius(solver, own);
    }
    let Some((pnt, [centre, rail1, rail2])) = evaluated? else {
        return Ok(out);
    };
    *near = Some(pnt);
    out.found = true;
    let p = edge_curve.evaluate_point(k, t)?;
    out.setback = p.distance(rail1);
    out.keeps = match judge {
        Judge::KeepsMaterial(crossings) => {
            keeps_material_behind(k, target, cache, centre, [rail1, rail2], chord, crossings)?
        }
        Judge::StaysInside(faces) => {
            !section_breaks_through(k, target, cache, centre, [rail1, rail2], chord, faces)?
        }
    };
    Ok(out)
}

/// What `keeps` judges at a section: material kept behind it (`sm_KeepsMaterialBehind`,
/// with the body's edges crossing the station's plane), or (WR-707, not the fork's) the
/// section not breaking through the part (`section_breaks_through`).
#[derive(Clone, Copy)]
enum Judge<'a> {
    /// `sm_KeepsMaterialBehind` with these plane crossings.
    KeepsMaterial(&'a [Point3]),
    /// `section_breaks_through` false, with these faces.
    StaysInside(&'a ThroughFaces),
}

/// The faces `section_breaks_through` tells apart (WR-707, not the fork's).
struct ThroughFaces {
    /// The faces at the blended edge's ends, which the corners trim: not judged.
    skip: BTreeSet<FaceId>,
    /// The blended faces and every face sharing an edge with one: the faces the blend
    /// reaches over the blended faces' boundaries, which it is trimmed to.
    reached: BTreeSet<FaceId>,
    /// The material the concave rounds of the same blend add ([`ConcaveFill`]).
    fills: Vec<ConcaveFill>,
    /// Which of `fills` count at the station being judged: those whose edge runs along the
    /// blended edge and whose span holds every stretch of the blended edge the station
    /// lies in where the section meets the part alike ([`Band::events`]).
    usable: Vec<bool>,
}

/// A section's shape between its rails (`section_breaks_through`).
#[derive(Clone, Copy)]
enum SectionShape {
    /// The ball's arc: `centre + rho (cos θ e1 + sin θ e2)`, θ from 0 at the first rail
    /// up to the second (less than a half turn).
    Arc {
        /// The ball's centre.
        centre: Point3,
        /// Unit, towards the first rail.
        e1: Vec3,
        /// Unit, square to `e1` in the section's plane, towards the second rail.
        e2: Vec3,
        /// The ball's radius.
        rho: f64,
    },
    /// The chamfer's chord, straight between the rails.
    Chord,
}

/// Not the fork's (WR-707, #2299; the rig's `1668-fillet-both-sides-of-thin-wall-r7`): the
/// material a concave constant round of the same blend adds, along a straight edge between
/// two planes. Across the edge it is the round's section region: bounded by the two faces
/// from the edge out to the rails and by the ball's arc between them, so the points in the
/// hollow's corner (`(x - edge)·n_i >= 0`, `n_i` the face's normal out of the material,
/// from the rail to the ball's centre), on the edge's side of the lines from the centre
/// through the rails (`(x - centre)·u_i <= 0`, `u_i` from the edge to the rail along the
/// face), and outside the ball (at least its radius from the line of centres). Along the
/// edge it is that region swept between the edge's ends (a prism: both planes, and the
/// line of centres, run along the edge).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConcaveFill {
    /// A point of the round's edge, where its section was solved.
    edge: Point3,
    /// The ball's centre in that section.
    centre: Point3,
    /// The ball's rails in that section, on the edge's two faces.
    rails: [Point3; 2],
    /// The edge's unit direction.
    dir: Vec3,
    /// The ball's radius.
    radius: f64,
    /// The edge's ends along `dir`, measured from `edge`.
    span: [f64; 2],
    /// The edge's two faces, which bound the hollow's corner the round fills.
    faces: [FaceId; 2],
}

/// The least of `a + b cos θ + c sin θ` over `θ` in `[t0, t1]` (`t1 - t0` under a turn):
/// at an end, or at the one angle where `b cos θ + c sin θ` is least, `-sqrt(b² + c²)`.
/// NaN where any of these is (so a comparison `least >= m` fails).
fn least_on_arc(a: f64, b: f64, c: f64, t0: f64, t1: f64) -> f64 {
    // the lesser, NaN where either is (`f64::min` drops a NaN operand)
    let lesser = |x: f64, y: f64| {
        if x.is_nan() || y.is_nan() {
            f64::NAN
        } else {
            x.min(y)
        }
    };
    let f = |t: f64| a + b * t.cos() + c * t.sin();
    let mut least = lesser(f(t0), f(t1));
    let at = (-c).atan2(-b);
    let turn = 2.0 * std::f64::consts::PI;
    for t in [at - turn, at, at + turn] {
        if t > t0 && t < t1 {
            least = lesser(least, a - (b * b + c * c).sqrt());
        }
    }
    least
}

/// Not the fork's (WR-707): whether the hollow's corner a concave round fills is closed at
/// the round's end vertex `v`: the one face at `v` besides the round's two `blended` faces is
/// a plane square to the edge (`dir`, within `tol` over the section's size) that holds the
/// whole corner triangle, from `v` out along the two faces by the section's `legs`. Then the
/// round is trimmed square there, so it adds its whole section right up to `v`, and the face
/// bounds the part's material over the whole triangle, so the hollow's corner does not run on
/// past `v`. The triangle lies in the face where no edge of the face runs into its inside
/// (`tol` in from its sides) and its centroid classifies on the face; an edge that is not a
/// line, or a face that is not a plane, is no closure.
///
/// # Errors
/// Those of the topology queries, the evaluations and the classification.
fn capped(
    k: &Kernel,
    target: &mut Brep,
    v: VertexId,
    blended: [FaceId; 2],
    legs: [Vec3; 2],
    dir: Vec3,
    tol: f64,
) -> Result<bool> {
    let corner = vertex::point(target, v)?;
    let others: BTreeSet<FaceId> = vertex::faces(target, v)?
        .into_iter()
        .filter(|f| !blended.contains(f))
        .collect();
    let mut others = others.into_iter();
    let (Some(end), None) = (others.next(), others.next()) else {
        return Ok(false);
    };
    let [leg0, leg1] = legs;
    let triangle = [corner, corner + leg0, corner + leg1];
    let centroid = corner + (leg0 + leg1) / 3.0;
    let Some((normal, uv)) = face::p2::surface(target, end)?.and_then(|s| match s.kind() {
        SurfaceKind::Plane(plane) => Some((
            plane.evaluate_normal(),
            plane.project_point_to_uv_domain(centroid),
        )),
        _ => None,
    }) else {
        return Ok(false);
    };
    let Some((_, normal)) = normal.length_and_unit() else {
        return Ok(false);
    };
    let cos = normal.dot(dir);
    let off = (1.0 - cos * cos).max(0.0).sqrt();
    if !(off * (leg0.length() + leg1.length()) <= tol) {
        return Ok(false);
    }
    for e in face::p4::edges(target, end, None, false, None)? {
        let Some(curve) = edge::evaluation_curve(target, e)? else {
            return Ok(false);
        };
        if !matches!(curve.kind(), CurveKind::Line(_)) {
            return Ok(false);
        }
        let ivl = edge::interval(target, e)?;
        let (p, q) = (
            curve.evaluate_point(k, ivl.min())?,
            curve.evaluate_point(k, ivl.max())?,
        );
        if segment_enters_triangle(p, q, triangle, tol) {
            return Ok(false);
        }
    }
    let zone = brep_zone_tol_3d(k, target, SmallTopoType::default());
    let mut class = PointClassification::<EntityRef>::new(zone);
    crate::query::face::p2::point_classify(
        k,
        target,
        end,
        uv,
        &mut class,
        crate::query::face::p2::PointClassifyOptions {
            src_zone_tol_3d: zone,
            do_boundary_intersections: true,
            use_uv_classification: true,
            ..crate::query::face::p2::PointClassifyOptions::default()
        },
    )?;
    Ok(class.point_class() == PointClassType::Face)
}

/// Whether the segment from `p` to `q` runs into the inside of `triangle`, `tol` in from each
/// of its sides (the segment and the triangle in one plane): what is left of the segment
/// after clipping it to the three half-planes has length. A degenerate triangle, or a value
/// that is not finite, counts as entering (the side that refuses a closure).
fn segment_enters_triangle(p: Point3, q: Point3, triangle: [Point3; 3], tol: f64) -> bool {
    let [a, b, c] = triangle;
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for (from, to, opposite) in [(a, b, c), (b, c, a), (c, a, b)] {
        let Some((_, along)) = (to - from).length_and_unit() else {
            return true;
        };
        let reach = opposite - from;
        let Some((_, inward)) = (reach - along * reach.dot(along)).length_and_unit() else {
            return true;
        };
        // the clearance inside this side, less `tol`, at each end: `f0 + t (f1 - f0) > 0`
        let (f0, f1) = (inward.dot(p - from) - tol, inward.dot(q - from) - tol);
        if !(f0.is_finite() && f1.is_finite()) {
            return true;
        }
        if f0 <= 0.0 && f1 <= 0.0 {
            return false;
        }
        if f0 < 0.0 {
            lo = lo.max(f0 / (f0 - f1));
        } else if f1 < 0.0 {
            hi = hi.min(f0 / (f0 - f1));
        }
    }
    hi > lo
}

impl ConcaveFill {
    /// The concave round `solver`'s fill, where it is one: a constant ball of circular
    /// section along a straight edge between two planes, its section solved at the edge's
    /// middle. `None` for any other round (its material is not counted: the section
    /// through it is judged against the part alone, which refuses).
    ///
    /// # Errors
    /// Those of the topology queries and of the section's solve.
    pub(crate) fn of_round(
        k: &Kernel,
        target: &mut Brep,
        solver: &mut SolverObject,
    ) -> Result<Option<Self>> {
        let base = solver.base();
        if base.surface(0)?.is_none() || base.surface(1)?.is_none() {
            return Ok(None);
        }
        let circular = base.fsg.as_ref().is_some_and(|fsg| {
            fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Circular
        });
        let Some(edgeuse) = base.edgeuse(0)? else {
            return Ok(None);
        };
        let (rolls, on_edge) = rolls_a_ball(solver);
        if !circular || !rolls || on_edge {
            return Ok(None);
        }
        let Some(edge_id) = eu::edge(target, edgeuse)? else {
            return Ok(None);
        };
        let Some(curve) = edge::evaluation_curve(target, edge_id)? else {
            return Ok(None);
        };
        if !matches!(curve.kind(), CurveKind::Line(_)) {
            return Ok(None);
        }
        let faces = edge::faces(target, edge_id, None)?;
        let [f0, f1] = faces.as_slice() else {
            return Ok(None);
        };
        let faces = [*f0, *f1];
        if f0 == f1 {
            return Ok(None);
        }
        for f in faces {
            let planar = face::p2::surface(target, f)?
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
            if !planar {
                return Ok(None);
            }
        }
        let ivl = edge::interval(target, edge_id)?;
        let t = ivl.evaluate(0.5);
        let Some((_, [centre, rail0, rail1])) = section_points(k, target, solver, &curve, t, None)?
        else {
            return Ok(None);
        };
        let at = curve.evaluate_point(k, t)?;
        let (p0, p1) = (
            curve.evaluate_point(k, ivl.min())?,
            curve.evaluate_point(k, ivl.max())?,
        );
        let Some((_, dir)) = (p1 - p0).length_and_unit() else {
            return Ok(None);
        };
        let tol = target.tolerance();
        let radius = centre.distance(rail0);
        if !(radius > tol) || !((centre.distance(rail1) - radius).abs() <= tol) {
            return Ok(None);
        }
        let (s0, s1) = ((p0 - at).dot(dir), (p1 - at).dot(dir));
        // the hollow's corner closed at both ends of the edge (`capped`): the round runs its
        // whole section from end to end, and the hollow does not run on past either end
        let legs = [rail0 - at, rail1 - at];
        let (start, end) = edge::start_end_vertices(target, edge_id)?;
        let (Some(start), Some(end)) = (start, end) else {
            return Ok(None);
        };
        for v in [start, end] {
            if !capped(k, target, v, faces, legs, dir, tol)? {
                return Ok(None);
            }
        }
        Ok(Some(Self {
            edge: at,
            centre,
            rails: [rail0, rail1],
            dir,
            radius,
            span: [s0.min(s1), s0.max(s1)],
            faces,
        }))
    }

    /// Whether this fill counts at the station `s` of `band` (with its `events`): its edge
    /// runs along the band's (within `tol` over the band's length), and its span holds
    /// every stretch between events that the station lies in (`tol` beyond its ends), so
    /// the section meets the part alike all along a stretch inside the round.
    fn covers(&self, band: &Band, events: &[f64], s: f64, tol: f64) -> bool {
        // the sine of the angle between the edges, from the cross product of the two unit
        // directions: exact to a few units of rounding however near parallel they are (not
        // `sqrt(1 - cos²)`, which turns the dot product's last-bit rounding into ~2e-8 for
        // two equal directions), counted at 8 u (each unit direction is off unit length by
        // at most a few u, and each component of the cross product rounds twice)
        let off = self.dir.cross(band.dir).length() + 8.0 * f64::EPSILON;
        if !(off * (band.span[1] - band.span[0]) <= tol) {
            return false;
        }
        let sigma = |e: f64| (band.at + band.dir * e - self.edge).dot(self.dir);
        let [lo, hi] = self.span;
        let mut any = false;
        for pair in events.windows(2) {
            let [e0, e1] = pair else {
                continue;
            };
            if !(s >= e0 - tol && s <= e1 + tol) {
                continue;
            }
            any = true;
            let (a, b) = (sigma(*e0), sigma(*e1));
            if !(a.is_finite() && b.is_finite() && a.min(b) >= lo - tol && a.max(b) <= hi + tol) {
                return false;
            }
        }
        any
    }

    /// Whether the piece between meets on `faces` is bounded by this fill's two faces, one
    /// at each end: the hollow's corner the round fills.
    fn bounds(&self, faces: [FaceId; 2]) -> bool {
        let [a, b] = faces;
        let [f0, f1] = self.faces;
        (a == f0 && b == f1) || (a == f1 && b == f0)
    }

    /// Whether the piece of a section from `ends[0]` to `ends[1]` (along `shape`) lies in
    /// this fill, `tol` inside it where it leaves the part: at least `tol` inside the
    /// edge's ends and `tol` outside the ball, within `tol` of the hollow's corner and of
    /// the lines through the rails. Each condition is a bound over the whole piece, in
    /// closed form: a linear function along an arc is `a + b cos θ + c sin θ`, least at an
    /// end or at one angle ([`least_on_arc`]); along a chord, at an end. The distance from
    /// the line of centres is bounded below by the distance from the centre less the most
    /// the piece runs along the edge.
    fn holds(&self, shape: &SectionShape, ends: [Point3; 2], tol: f64) -> bool {
        let [q0, q1] = ends;
        // `n_i` (out of the material, from the rail to the centre) and `u_i` (from the
        // edge to the rail): the hollow's corner `(x - edge)·n_i >= -tol` and the edge's
        // side `(x - centre)·u_i <= tol`, as `w·(x - p) >= m`
        let [rail0, rail1] = self.rails;
        let unit = |v: Vec3| v.length_and_unit().map(|(_, u)| u);
        let (Some(n0), Some(n1), Some(u0), Some(u1)) = (
            unit(self.centre - rail0),
            unit(self.centre - rail1),
            unit(rail0 - self.edge),
            unit(rail1 - self.edge),
        ) else {
            return false;
        };
        let [span0, span1] = self.span;
        let linear = [
            (n0, self.edge, -tol),
            (n1, self.edge, -tol),
            (u0 * -1.0, self.centre, -tol),
            (u1 * -1.0, self.centre, -tol),
            // inside the edge's ends: `(x - edge)·dir` in `(span0 + tol, span1 - tol)`
            (self.dir, self.edge, span0 + tol),
            (self.dir * -1.0, self.edge, tol - span1),
        ];
        let clear = self.radius + tol;
        match *shape {
            SectionShape::Arc {
                centre,
                e1,
                e2,
                rho,
            } => {
                let angle = |q: Point3| {
                    let v = q - centre;
                    v.dot(e2).atan2(v.dot(e1))
                };
                let (a0, a1) = (angle(q0), angle(q1));
                let (t0, t1) = (a0.min(a1), a0.max(a1));
                for (w, p, m) in linear {
                    let least =
                        least_on_arc(w.dot(centre - p), rho * w.dot(e1), rho * w.dot(e2), t0, t1);
                    if !(least >= m) {
                        return false;
                    }
                }
                // |x - c|² = |cs - c|² + rho² + 2 rho ((cs - c)·e1 cos θ + (cs - c)·e2 sin θ),
                // and |(x - c)·dir| <= |(cs - c)·dir| + rho (|e1·dir| + |e2·dir|)
                let d = centre - self.centre;
                let near = least_on_arc(
                    d.dot(d) + rho * rho,
                    2.0 * rho * d.dot(e1),
                    2.0 * rho * d.dot(e2),
                    t0,
                    t1,
                );
                let along =
                    d.dot(self.dir).abs() + rho * (e1.dot(self.dir).abs() + e2.dot(self.dir).abs());
                near - along * along >= clear * clear
            }
            SectionShape::Chord => {
                for (w, p, m) in linear {
                    if !(w.dot(q0 - p) >= m && w.dot(q1 - p) >= m) {
                        return false;
                    }
                }
                // the distance from the line of centres, square to it: least on the chord
                // where its projection is nearest
                let across = |v: Vec3| v - self.dir * v.dot(self.dir);
                let w0 = across(q0 - self.centre);
                let v = across(q1 - q0);
                let vv = v.dot(v);
                let s = if vv > 0.0 {
                    (-w0.dot(v) / vv).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let x = w0 + v * s;
                x.dot(x) >= clear * clear
            }
        }
    }
}

/// Not the fork's (WR-707): a convex blend's band, where it is a constant section swept along
/// a straight edge between two planes. The section (the ball's arc, or the chamfer's chord)
/// is the same at every station, moved along the edge, so the band is a piece of a circular
/// cylinder (or a strip of a plane) and the faces and edges of the part meet it in closed
/// form ([`Band::events`]).
struct Band {
    /// The section's shape at the edge's middle.
    shape: SectionShape,
    /// The section's rails there.
    rails: [Point3; 2],
    /// The edge's middle, from where the band runs along `dir`.
    at: Point3,
    /// The edge's unit direction.
    dir: Vec3,
    /// The edge's ends along `dir`, measured from `at`.
    span: [f64; 2],
    /// The arc's turn from its first rail to its second (0 for a chord).
    phi: f64,
}

impl Band {
    /// The band of the section `centre`, `rails` at the edge's point `at` (`chord` for a
    /// chamfer), along `dir` over `span`. `None` where the rails are opposite or together
    /// (no section, as `section_breaks_through`). The section is taken square to `dir`
    /// (`e1`, `e2` and the rails' chord across the edge), as a section of a straight edge
    /// between two planes is to the solver's tolerance; a tilt `eps` moves `misses`' and
    /// `line_crossings`' bounds by about `eps` times the span, below `tol`.
    fn new(
        centre: Point3,
        rails: [Point3; 2],
        chord: bool,
        at: Point3,
        dir: Vec3,
        span: [f64; 2],
    ) -> Option<Self> {
        let [rail1, rail2] = rails;
        let a = rail1 - centre;
        let b = rail2 - centre;
        let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
        let det = aa * bb - ab * ab;
        if !(det > EFF_ZERO * aa * bb) {
            return None;
        }
        if chord {
            return Some(Self {
                shape: SectionShape::Chord,
                rails,
                at,
                dir,
                span,
                phi: 0.0,
            });
        }
        let rho = aa.sqrt();
        let e1 = a / rho;
        let e2 = (b - a * (ab / aa)).unitize().ok()?;
        let phi = b.dot(e2).atan2(b.dot(e1));
        Some(Self {
            shape: SectionShape::Arc {
                centre,
                e1,
                e2,
                rho,
            },
            rails,
            at,
            dir,
            span,
            phi,
        })
    }

    /// Where along the edge the station through `x` lies.
    fn along(&self, x: Point3) -> f64 {
        (x - self.at).dot(self.dir)
    }

    /// `v` square to the edge.
    fn across(&self, v: Vec3) -> Vec3 {
        v - self.dir * v.dot(self.dir)
    }

    /// Whether the box `b` is certainly clear of the band by more than `tol`: all of it
    /// beyond an end of the span; or, for an arc, all of it farther from the line of centres
    /// than the radius, all of it nearer, or all of it on the line of centres' side of the
    /// chord between the rails (the arc, less than a half turn, lies beyond that chord); or,
    /// for a chord, all of it on one side of the strip's plane or beyond one of its rails.
    /// Every bound is over the box's corners (the least distance from the line of centres
    /// by the box's centre less its half-diagonal across the edge), so a box that meets the
    /// band is never clear. A NaN is never clear.
    fn misses(&self, b: &Extent3d, tol: f64) -> bool {
        let corners: Vec<Point3> = (0..8_u8)
            .map(|i| {
                Point3::new(
                    if i & 1 == 0 { b.min.x } else { b.max.x },
                    if i & 2 == 0 { b.min.y } else { b.max.y },
                    if i & 4 == 0 { b.min.z } else { b.max.z },
                )
            })
            .collect();
        if !corners
            .iter()
            .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite())
        {
            return false;
        }
        // every corner's value of `f` above `limit`, or every one below `-limit`
        let all_above =
            |f: &dyn Fn(Point3) -> f64, limit: f64| corners.iter().all(|&c| f(c) > limit);
        let [lo, hi] = self.span;
        if all_above(&|c| lo - self.along(c), tol) || all_above(&|c| self.along(c) - hi, tol) {
            return true;
        }
        match self.shape {
            SectionShape::Arc {
                centre,
                e1,
                e2,
                rho,
            } => {
                let radial = |c: Point3| self.across(c - centre).length();
                if all_above(&|c| rho - radial(c), tol) {
                    return true;
                }
                let middle = (b.min + b.max) * 0.5;
                let mut reach = 0.0_f64;
                for c in &corners {
                    let h = self.across(*c - middle);
                    reach = reach.max(h.length());
                }
                if radial(middle) - reach - rho > tol {
                    return true;
                }
                let m = e1 * (0.5 * self.phi).cos() + e2 * (0.5 * self.phi).sin();
                let floor = rho * (0.5 * self.phi).cos();
                all_above(&|c| floor - self.across(c - centre).dot(m), tol)
            }
            SectionShape::Chord => {
                let [r1, r2] = self.rails;
                let Some((width, w)) = (r2 - r1).length_and_unit() else {
                    return false;
                };
                let Some((_, n)) = w.cross(self.dir).length_and_unit() else {
                    return false;
                };
                all_above(&|c| n.dot(c - r1), tol)
                    || all_above(&|c| -n.dot(c - r1), tol)
                    || all_above(&|c| -(c - r1).dot(w), tol)
                    || all_above(&|c| (c - r1).dot(w) - width, tol)
            }
        }
    }

    /// Whether the section's point `x` (on the band's cylinder) lies on the arc between its
    /// rails, `tol` past them (`-tol`: `tol` short of them).
    fn on_arc(&self, x: Point3, tol: f64) -> bool {
        let SectionShape::Arc {
            centre,
            e1,
            e2,
            rho,
        } = self.shape
        else {
            return false;
        };
        let v = self.across(x - centre);
        let theta = v.dot(e2).atan2(v.dot(e1));
        let slack = tol / rho;
        theta >= -slack && theta <= self.phi + slack
    }

    /// Not the fork's (WR-707): the stations along the edge, measured from `at`, where the
    /// way the section meets the part can change. Between two of them the section meets the
    /// same faces, inside them or at the same edges, in the same order, so what
    /// `section_breaks_through` decides at one station of the stretch holds for all of it:
    /// a section's meet with a face moves onto or off the face only where the band crosses
    /// one of the face's edges, and a meet is made or lost on a plane only where the band's
    /// curve along the plane turns back along the edge (a fold, at the extremes of a
    /// `a + b cos θ + c sin θ`) or reaches a rail. Both ends of the edge are stations.
    ///
    /// `None` where this is not known in closed form: an edge that is not a line, or a face
    /// that is not a plane, not certainly clear of the band ([`Band::misses`]); a line or a
    /// plane lying on it; an edge with no curve or a face with no surface; or a NaN.
    /// Events within `tol` of each other are one: a stretch of `2 tol` or less gets no
    /// station of its own (a change of the section's meets within the body's tolerance).
    ///
    /// # Errors
    /// Those of the evaluations.
    fn events(&self, k: &Kernel, cache: &BandCutCache, tol: f64) -> Result<Option<Vec<f64>>> {
        if cache.unseen {
            return Ok(None);
        }
        let mut out = vec![self.span[0], self.span[1]];
        for e in &cache.edges {
            k.check_cancelled(OP)?;
            if e.bbox.as_ref().is_some_and(|b| self.misses(b, tol)) {
                continue;
            }
            if !matches!(e.curve.kind(), CurveKind::Line(_)) {
                return Ok(None);
            }
            let p = e.curve.evaluate_point(k, e.interval.min())?;
            let q = e.curve.evaluate_point(k, e.interval.max())?;
            if !self.line_crossings(p, q, tol, &mut out) {
                return Ok(None);
            }
        }
        for f in &cache.faces {
            k.check_cancelled(OP)?;
            if f.bbox.as_ref().is_some_and(|b| self.misses(b, tol)) {
                continue;
            }
            let SurfaceKind::Plane(plane) = f.surface.kind() else {
                return Ok(None);
            };
            let (origin, normal, _) = plane.is_planar();
            let Some((_, normal)) = normal.length_and_unit() else {
                return Ok(None);
            };
            if !self.plane_folds(origin, normal, tol, &mut out) {
                return Ok(None);
            }
        }
        let [lo, hi] = self.span;
        let mut out: Vec<f64> = out
            .into_iter()
            .filter(|s| *s >= lo - tol && *s <= hi + tol)
            .map(|s| s.max(lo).min(hi))
            .collect();
        if out.iter().any(|s| !s.is_finite()) {
            return Ok(None);
        }
        out.sort_by(f64::total_cmp);
        out.dedup_by(|b, a| *b - *a <= tol);
        Ok(Some(out))
    }

    /// The stations where the segment `p`-`q` crosses the band (onto `out`); `false` where
    /// it lies on the band.
    fn line_crossings(&self, p: Point3, q: Point3, tol: f64, out: &mut Vec<f64>) -> bool {
        if ![p.x, p.y, p.z, q.x, q.y, q.z].iter().all(|v| v.is_finite()) {
            return false;
        }
        let [lo, hi] = self.span;
        let in_span = |s: f64| s >= lo - tol && s <= hi + tol;
        let overlaps = |a: f64, b: f64| a.min(b) <= hi + tol && a.max(b) >= lo - tol;
        let length = p.distance(q);
        match self.shape {
            SectionShape::Arc { centre, rho, .. } => {
                let a = self.across(p - centre);
                let d = self.across(q - p);
                let dd = d.dot(d);
                if !(dd.sqrt() > tol) {
                    // along the edge: on the band, or a crossing of no extent along it
                    if (a.length() - rho).abs() <= 2.0 * tol && self.on_arc(p, tol) {
                        let (sp, sq) = (self.along(p), self.along(q));
                        if (sq - sp).abs() > 2.0 * tol && overlaps(sp, sq) {
                            return false;
                        }
                        out.push(sp);
                    }
                    return true;
                }
                // |a + t d| = rho, and the closest approach where it grazes the cylinder
                let (aa, ad) = (a.dot(a), a.dot(d));
                let closest = -ad / dd;
                let mut ts = vec![closest];
                let disc = ad * ad - dd * (aa - rho * rho);
                if !disc.is_finite() || !closest.is_finite() {
                    return false;
                }
                if disc >= 0.0 {
                    let root = disc.sqrt();
                    ts.push((-ad - root) / dd);
                    ts.push((-ad + root) / dd);
                }
                let slack = if length > 0.0 { tol / length } else { 0.0 };
                for t in ts {
                    if !(t >= -slack && t <= 1.0 + slack) {
                        continue;
                    }
                    let x = p + (q - p) * t.clamp(0.0, 1.0);
                    let r = self.across(x - centre).length();
                    if (r - rho).abs() <= 2.0 * tol && self.on_arc(x, tol) && in_span(self.along(x))
                    {
                        out.push(self.along(x));
                    }
                }
                true
            }
            SectionShape::Chord => {
                let [r1, r2] = self.rails;
                let Some((width, w)) = (r2 - r1).length_and_unit() else {
                    return false;
                };
                let Some((_, m)) = w.cross(self.dir).length_and_unit() else {
                    return false;
                };
                let (dp, dq) = (m.dot(p - r1), m.dot(q - r1));
                let on_strip = |x: Point3| {
                    let u = (x - r1).dot(w);
                    u >= -tol && u <= width + tol
                };
                if dp.abs() <= tol && dq.abs() <= tol {
                    // in the strip's plane
                    let (up, uq) = ((p - r1).dot(w), (q - r1).dot(w));
                    let meets = up.min(uq) <= width + tol && up.max(uq) >= -tol;
                    return !(meets && overlaps(self.along(p), self.along(q)));
                }
                if dp * dq > 0.0 && dp.abs() > tol && dq.abs() > tol {
                    return true;
                }
                let t = if dp == dq { 0.0 } else { dp / (dp - dq) };
                let x = p + (q - p) * t.clamp(0.0, 1.0);
                if on_strip(x) && in_span(self.along(x)) {
                    out.push(self.along(x));
                }
                true
            }
        }
    }

    /// The stations where the meets of the section with the plane through `origin` with unit
    /// `normal` can be made or lost (onto `out`): where its curve along the band turns back
    /// along the edge, or reaches a rail. `false` where the plane lies on the band.
    fn plane_folds(&self, origin: Point3, normal: Vec3, tol: f64, out: &mut Vec<f64>) -> bool {
        let nd = normal.dot(self.dir);
        // the station of the section's point `x` moved along the edge onto the plane
        let onto = |x: Point3| self.along(x) + normal.dot(origin - x) / nd;
        match self.shape {
            SectionShape::Arc {
                centre,
                e1,
                e2,
                rho,
            } => {
                let (b, c) = (normal.dot(e1), normal.dot(e2));
                if nd.abs() <= EFF_ZERO {
                    // along the edge: it meets each section alike, unless it touches the
                    // cylinder along a line of the arc between its rails (touching it at a
                    // rail, as each blended face does, meets each section alike there)
                    let gap = normal.dot(centre - origin);
                    if (gap.abs() - rho).abs() <= tol {
                        let touch = centre - normal * (gap.signum() * rho);
                        if self.on_arc(touch, -tol) {
                            return false;
                        }
                    }
                    return true;
                }
                let at_angle = |theta: f64| centre + (e1 * theta.cos() + e2 * theta.sin()) * rho;
                let pi = std::f64::consts::PI;
                let turn = c.atan2(b);
                for theta in [0.0, self.phi, turn - pi, turn, turn + pi] {
                    if theta >= 0.0 && theta <= self.phi {
                        out.push(onto(at_angle(theta)));
                    }
                }
                true
            }
            SectionShape::Chord => {
                let [r1, r2] = self.rails;
                if nd.abs() <= EFF_ZERO {
                    let Some((_, w)) = (r2 - r1).length_and_unit() else {
                        return false;
                    };
                    // the strip's own plane
                    return !(normal.dot(w).abs() <= EFF_ZERO
                        && normal.dot(r1 - origin).abs() <= tol);
                }
                out.push(onto(r1));
                out.push(onto(r2));
                true
            }
        }
    }
}

/// Not the fork's (WR-707): the band of the convex `solver` along its edge `edge_id` (curve
/// `curve` over `ivl`), with its events ([`Band::events`]), where it is known in closed form:
/// a straight edge between two planes, its section solved at the edge's middle. `None`
/// otherwise. That solve runs before the events are known, so it also runs when the band
/// turns out uncertified: it makes the same solver calls every station's `keeps` makes
/// (with no `near` to carry), so the stations read nothing it leaves behind.
///
/// # Errors
/// Those of the topology queries, the section's solve and the evaluations.
#[allow(clippy::too_many_arguments)]
fn band_with_events(
    k: &Kernel,
    target: &mut Brep,
    cache: &BandCutCache,
    solver: &mut SolverObject,
    curve: &Curve,
    ivl: &Extent1d,
    chord: bool,
    edge_id: crate::topology::EdgeId,
    tol: f64,
) -> Result<Option<(Band, Vec<f64>)>> {
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Ok(None);
    }
    for f in edge::faces(target, edge_id, None)? {
        let planar = face::p2::surface(target, f)?
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_)));
        if !planar {
            return Ok(None);
        }
    }
    let t = ivl.evaluate(0.5);
    let Some((_, [centre, rail1, rail2])) = section_points(k, target, solver, curve, t, None)?
    else {
        return Ok(None);
    };
    let at = curve.evaluate_point(k, t)?;
    let (p0, p1) = (
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    );
    let Some((_, dir)) = (p1 - p0).length_and_unit() else {
        return Ok(None);
    };
    let span = [(p0 - at).dot(dir), (p1 - at).dot(dir)];
    let Some(band) = Band::new(centre, [rail1, rail2], chord, at, dir, span) else {
        return Ok(None);
    };
    Ok(band.events(k, cache, tol)?.map(|events| (band, events)))
}

/// Not the fork's (WR-707, #2299): whether the section of a convex blend at a station,
/// between its rails (the ball's arc, or the chamfer's chord, as an exact curve), breaks
/// through the part's boundary: it leaves the part through a face and comes back into it
/// through another (or the same) face, so the part between those crossings is a hollow
/// behind a wall that the blend opens (a round on a wall thinner than it reaches into the
/// hollow behind, #2299).
///
/// The section is split at every point where it meets the body's boundary (crossings
/// within the body's tolerance of its own ends are its rails), and each piece between two
/// such points is classified by its middle. The section breaks through where a piece
/// lies outside the part's material, one of its ends crosses a face inside the face (not
/// at an edge), neither end is on a face at the blended edge's ends (which the corners
/// trim), and one end is on a face that shares no edge with the blended faces: an inner
/// face of the part, bounding material behind a wall, that the blend does not reach over
/// the blended faces' boundaries; and the material beside the piece runs to a rail on one
/// side (WR-707 addendum, 2026-11-03, #2328). That material is the wall, part of what the
/// blend rounds. A piece between islands of material, with void on either side of each
/// along the section (a thin rib across a slot), is not behind a wall: the blend runs past
/// the islands and is trimmed to their faces.
///
/// So a piece outside the part that reaches a rail is not a break-through: there the
/// blend runs off its face (over a cliff, into a hole or an opening) and is trimmed to the
/// faces it runs onto. Nor is a piece through a hole or an opening in a blended face: its
/// walls bound that face. The construction builds both to their closed-form volumes
/// (SolidCraft rig cases the fork builds). It builds a blind pocket or a cavity under a
/// blended face wrong, and those are refused (WR-707 addendum, 2026-11-02).
fn section_breaks_through(
    k: &Kernel,
    body: &mut Brep,
    cache: &BandCutCache,
    centre: Point3,
    rails: [Point3; 2],
    chord: bool,
    faces: &ThroughFaces,
) -> Result<bool> {
    let [rail1, rail2] = rails;
    let a = rail1 - centre;
    let b = rail2 - centre;
    let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
    let det = aa * bb - ab * ab;
    if !(det > EFF_ZERO * aa * bb) {
        return Ok(false); // rails opposite or together: no section to judge
    }
    let mut shape = SectionShape::Chord;
    let section = if chord {
        Curve::from(Line::create_line_segment(3, rail1, rail2, None)?)
    } else {
        let rho = aa.sqrt();
        let e1 = a / rho;
        let e2 = (b - a * (ab / aa)).unitize()?;
        let phi = b.dot(e2).atan2(b.dot(e1));
        shape = SectionShape::Arc {
            centre,
            e1,
            e2,
            rho,
        };
        Curve::from(Ellipse::new_circle(
            k,
            centre,
            e1,
            e2,
            Extent1d::from_min_max(0.0, rad_to_deg(phi)),
            rho,
            &EllipseOptions {
                dim: 3,
                ..EllipseOptions::default()
            },
        )?)
    };
    let interval = section.natural_interval();
    let ends = body.tolerance();
    let zone = brep_zone_tol_3d(k, body, SmallTopoType::default());
    let sec_box = section
        .calculate_bounding_box(
            k,
            &interval,
            BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            },
            false,
        )
        .ok()
        .and_then(|b| b.normal);
    // where the section meets the boundary of face `f`: its parameter, whether it crosses
    // `f` inside it (not on its boundary), whether `f` is judged and whether the blend
    // reaches `f`
    let mut meets: Vec<Meet> = Vec::new();
    for face in &cache.faces {
        if let (Some(f_box), Some(s_box)) = (&face.bbox, &sec_box) {
            if f_box.are_disjoint(s_box, ends) {
                continue;
            }
        }
        let f = face.id;
        let mut on_face: Vec<(f64, Point2)> = Vec::new();
        {
            let solutions = cache
                .query
                .face_caches()
                .with_services(k, &mut *body, |services| {
                    face.surface
                        .global_curve_intersect_with_context::<CacheObject>(
                            k,
                            [None, Some(CacheObject::Surface(f))],
                            None,
                            services,
                            &face.domain,
                            &section,
                            &interval,
                            ends,
                        )
                })?;
            for sol in solutions.as_slice() {
                let [t, u, v, ..] = sol.start.parameters;
                let at = section.evaluate_point(k, t)?;
                if at.distance(rail1) <= ends || at.distance(rail2) <= ends {
                    continue; // the section's own ends, on the blended faces
                }
                on_face.push((t, Point2::new(u, v)));
            }
        }
        for (t, uv) in on_face {
            let mut class = PointClassification::<EntityRef>::new(zone);
            cache.query.face_caches().point_classify(
                k,
                body,
                f,
                uv,
                &mut class,
                crate::query::face::p2::PointClassifyOptions {
                    src_zone_tol_3d: zone,
                    do_boundary_intersections: true,
                    use_uv_classification: true,
                    ..crate::query::face::p2::PointClassifyOptions::default()
                },
            )?;
            let inside = match class.point_class() {
                PointClassType::Unknown => continue, // outside the face
                PointClassType::Face => true,
                _ => false, // on the face's boundary
            };
            meets.push(Meet {
                t,
                face: f,
                inside,
                judged: !faces.skip.contains(&f),
                reached: faces.reached.contains(&f),
            });
        }
    }
    meets.sort_by(|x, y| x.t.total_cmp(&y.t));
    // the bounds of the section's pieces: its rails and the meets between them; piece `i`
    // runs from `at[i]` to `at[i + 1]`, so meet `j` ends piece `j` and starts piece `j + 1`
    let mut at = Vec::with_capacity(meets.len() + 2);
    at.push(interval.min());
    at.extend(meets.iter().map(|m| m.t));
    at.push(interval.max());
    let mut pieces = Pieces {
        section: &section,
        at: &at,
        classes: vec![None; at.len() - 1],
        zone,
        query: &cache.query,
    };
    for (j, pair) in meets.windows(2).enumerate() {
        let [m0, m1] = pair else {
            continue;
        };
        if !(m0.judged && m1.judged) || !(m0.inside || m1.inside) || (m0.reached && m1.reached) {
            continue;
        }
        let piece = j + 1;
        if pieces.class(k, body, piece)? != Piece::Void {
            continue;
        }
        // the wall: the material beside the piece runs to a rail
        if pieces.material_to_rail(k, body, (0..piece).rev())?
            || pieces.material_to_rail(k, body, piece + 1..at.len() - 1)?
        {
            // the hollow there filled by a concave round of the same blend: the piece runs
            // between that round's two faces and stays in the material it adds (piece
            // `j + 1` runs from meet `j` to meet `j + 1`)
            let piece_ends = [
                section.evaluate_point(k, m0.t)?,
                section.evaluate_point(k, m1.t)?,
            ];
            if faces.fills.iter().zip(&faces.usable).any(|(f, &usable)| {
                usable && f.bounds([m0.face, m1.face]) && f.holds(&shape, piece_ends, ends)
            }) {
                continue;
            }
            return Ok(true); // a hollow behind a wall: the section breaks through
        }
    }
    Ok(false)
}

/// The pieces of a section between the points where it meets the part's boundary
/// (`section_breaks_through`), each classified once, by its middle.
struct Pieces<'a> {
    /// The section.
    section: &'a Curve,
    /// The pieces' bounds, in the section's parameter.
    at: &'a [f64],
    /// Each piece's class, once classified.
    classes: Vec<Option<Piece>>,
    /// The body's zone tolerance.
    zone: f64,
    /// Shared B-rep query for point classifications.
    query: &'a BrepQuery<'static>,
}

impl Pieces<'_> {
    /// The class of piece `i`.
    fn class(&mut self, k: &Kernel, body: &mut Brep, i: usize) -> Result<Piece> {
        if let Some(Some(known)) = self.classes.get(i) {
            return Ok(*known);
        }
        let (Some(&t0), Some(&t1)) = (self.at.get(i), self.at.get(i + 1)) else {
            return Err(null_defect(
                "a band piece has no end parameters: each piece runs between two \
                 consecutive cut parameters, which hold one more than the pieces",
            ));
        };
        let class = if !(t1 - t0 > 0.0) {
            Piece::Point
        } else if point_in_material(
            k,
            body,
            self.query,
            self.section.evaluate_point(k, 0.5 * (t0 + t1))?,
            self.zone,
        )? {
            Piece::Material
        } else {
            Piece::Void
        };
        if let Some(slot) = self.classes.get_mut(i) {
            *slot = Some(class);
        }
        Ok(class)
    }

    /// Whether the pieces `run`, taken from a void piece out to a rail, all lie in the
    /// material (a piece of no length does not interrupt them): the material beside the
    /// void piece is the part the blend rounds, reaching the blended face at its rail, not
    /// an island with void on both sides that the blend runs past and is trimmed to.
    fn material_to_rail(
        &mut self,
        k: &Kernel,
        body: &mut Brep,
        run: impl Iterator<Item = usize>,
    ) -> Result<bool> {
        for i in run {
            if self.class(k, body, i)? == Piece::Void {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// What a piece of a section between two meets is (`section_breaks_through`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// In the part's material.
    Material,
    /// Outside it.
    Void,
    /// Of no length: one point, met on two faces.
    Point,
}

/// Where a section meets the boundary of a face (`section_breaks_through`).
#[derive(Clone, Copy)]
struct Meet {
    /// The section's parameter there.
    t: f64,
    /// The face.
    face: FaceId,
    /// Inside the face, not on its boundary.
    inside: bool,
    /// The face is judged: not one at the blended edge's ends.
    judged: bool,
    /// The face is one the blend reaches ([`ThroughFaces::reached`]).
    reached: bool,
}

/// Whether `point` lies in the part's material, inside a region that is not void (the
/// final classification of `sm_KeepsMaterialBehind`, with a point on the boundary, within
/// the zone tolerance of a face, edge or vertex, not counted in: a piece of a section
/// between two crossings of faces that lies within the tolerance of the boundary is too
/// shallow to keep material).
fn point_in_material(
    k: &Kernel,
    body: &mut Brep,
    query: &BrepQuery<'_>,
    point: Point3,
    zone: f64,
) -> Result<bool> {
    let class = query.point_3d_classify(k, body, point, zone, true)?;
    if class.point_class() != PointClassType::Region {
        return Ok(false);
    }
    if let Some(EntityRef::Region(region)) = class.object() {
        return Ok(body.region(region)?.is_void() == 0);
    }
    Ok(false)
}

/// `SmFilletSolver::BandCutsThePart`: whether this convex blend (one that removes
/// material) would take all of the part between its rails somewhere along its edge,
/// cutting it into pieces (a round or chamfer through the bridge over a slot); and where it
/// would, the largest size that keeps the part whole. `None` where it does not cut.
///
/// At the middles of 32 equal steps of the edge (as [`super::ball_regular::ball_is_regular`]),
/// with the body's edges crossing each section's plane found once. A section where no
/// material lies behind the blend's section is a cut where sections either side of its run
/// along the edge keep material. The limit is then found at each cutting section by
/// bisection on the ball's radius, least over the sections. A constant ball only.
///
/// `target` is the body blended; it holds the solver's edge use.
///
/// # Errors
///
/// Those of the section solves, the intersections, the classifications and the
/// evaluations; `SM_ERR_NULL_POINTER` for an edge without a curve.
pub fn band_cuts_the_part(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
) -> Result<Option<BandCut>> {
    band_cuts_the_part_cached(k, target, solver, &mut None)
}

pub(crate) fn band_cuts_the_part_cached(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    cache_slot: &mut Option<BandCutCache>,
) -> Result<Option<BandCut>> {
    let base = solver.base();
    if base.surface(0)?.is_none() || base.surface(1)?.is_none() {
        return Ok(None);
    }
    let Some(edgeuse) = base.edgeuse(0)? else {
        return Ok(None);
    };
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| edge_use_without_edge(edgeuse))?;
    let curve = filleted_edge_curve(target, edge_id)?;
    let ivl = edge::interval(target, edge_id)?;
    let (rolls, on_edge) = rolls_a_ball(solver);
    if !rolls || on_edge {
        return Ok(None); // a constant ball only
    }
    let probe = TsectPnt::new();
    let own = ball_law(k, solver, &probe)?.r[0];
    let chord = solver.base().fsg.as_ref().is_some_and(|fsg| {
        fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear
    });
    let cache = match cache_slot {
        Some(c) => c,
        None => cache_slot.insert(BandCutCache::new(k, target)?),
    };

    // each section: its parameter, plane crossings and section found near, and whether it keeps
    // material behind it (0), takes all of the part between the rails (1), or is not judged (2)
    let mut ts: Vec<f64> = Vec::new();
    let mut all_crossings: Vec<Vec<Point3>> = Vec::new();
    let mut nears: Vec<Option<TsectPnt>> = Vec::new();
    let mut states: Vec<u8> = Vec::new();
    let mut near: Option<TsectPnt> = None;
    for j in 0..STEPS {
        let t = ivl.evaluate((j as f64 + 0.5) / STEPS as f64);
        ts.push(t);
        let pv = curve.evaluate(k, t, 1, true, false)?;
        let (Some(&point), Some(&tangent)) = (pv.first(), pv.get(1)) else {
            return Err(null_defect(
                "the filleted edge's curve evaluated with its first derivative gives no point \
                 or tangent: the evaluation returns the point and each derivative asked",
            ));
        };
        if !(tangent.length() > EFF_ZERO) {
            all_crossings.push(Vec::new());
            nears.push(None);
            states.push(2);
            continue;
        }
        let normal = tangent.unitize()?;
        let crossings = edges_crossing_plane(k, target, cache, point, normal)?;
        let judged = keeps(
            k,
            target,
            cache,
            solver,
            &curve,
            chord,
            own,
            t,
            0.0,
            Judge::KeepsMaterial(&crossings),
            &mut near,
        )?;
        all_crossings.push(crossings);
        nears.push(near.clone());
        states.push(if judged.found {
            u8::from(!judged.keeps)
        } else {
            2
        });
    }

    // a run of sections that take all of the part cuts it in two only where sections either side
    // of the run along the edge keep material: a run that reaches an end of the edge (a tip, where
    // the part runs out thinner than the band) blends that end away and leaves the part whole
    let state = |i: usize| states.get(i).copied().unwrap_or(2);
    let mut cut: Option<BandCut> = None;
    let mut j = 0;
    while j < STEPS {
        if state(j) != 1 {
            j += 1;
            continue;
        }
        let first = j;
        let mut last = j;
        while last + 1 < STEPS && state(last + 1) != 0 {
            last += 1;
        }
        j = last + 1;
        let kept_before = (0..first).any(|i| state(i) == 0);
        let kept_after = (last + 1..STEPS).any(|i| state(i) == 0);
        if !kept_before || !kept_after {
            continue;
        }
        for m in first..=last {
            if state(m) != 1 {
                continue;
            }
            let t = ts.get(m).copied().unwrap_or(0.0);
            let crossings = all_crossings.get(m).map_or(&[][..], Vec::as_slice);
            // the largest radius that keeps material behind its section here
            let mut at = nears.get(m).cloned().flatten();
            let radius = own;
            let (mut lo, mut hi, mut lo_setback) = (0.0_f64, radius, 0.0);
            let mut tr = 0.5 * radius;
            while tr > 1.0e-6 * radius {
                let judged = keeps(
                    k,
                    target,
                    cache,
                    solver,
                    &curve,
                    chord,
                    own,
                    t,
                    tr,
                    Judge::KeepsMaterial(crossings),
                    &mut at,
                )?;
                if judged.found && judged.keeps {
                    lo = tr;
                    lo_setback = judged.setback;
                    break;
                }
                hi = tr;
                tr *= 0.5;
            }
            let mut i = 0;
            while i < BISECTIONS && lo > 0.0 {
                let mid = 0.5 * (lo + hi);
                let judged = keeps(
                    k,
                    target,
                    cache,
                    solver,
                    &curve,
                    chord,
                    own,
                    t,
                    mid,
                    Judge::KeepsMaterial(crossings),
                    &mut at,
                )?;
                if judged.found && judged.keeps {
                    lo = mid;
                    lo_setback = judged.setback;
                } else {
                    hi = mid;
                }
                i += 1;
            }
            if cut.is_none_or(|c| lo < c.radius) {
                cut = Some(BandCut {
                    radius: lo,
                    setback: lo_setback,
                });
            }
        }
    }
    Ok(cut)
}

/// Not the fork's (WR-707, #2299): whether this convex blend's section breaks
/// through the part somewhere along its edge (it leaves the part and comes back into it
/// through an inner face, one sharing no edge with the blended faces: a hollow behind a
/// wall, `section_breaks_through`), and where it would, the largest size that keeps every
/// section from breaking through. At the same 32 sections as [`band_cuts_the_part`] and,
/// where the band is known in closed form (a straight edge between two planes, with only
/// lines and planes near it: [`Band::events`]), at one more inside each stretch of the edge
/// between the stations where the way the section meets the part can change, so that every
/// stretch is judged and the judgement is exact there; faces at the blended edge's ends (the
/// corners trim the blend there) are not judged. The limit at each crossing section is found
/// as the cut's is (halving from the ball's radius, then 40 bisections), least over the
/// sections. A constant ball only. `None` where no section breaks through.
///
/// # Errors
///
/// As [`band_cuts_the_part`].
pub fn band_breaks_through_the_part(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
) -> Result<Option<BandCut>> {
    band_breaks_through_the_part_cached(k, target, solver, Vec::new(), &mut None)
}

pub(crate) fn band_breaks_through_the_part_cached(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    fills: Vec<ConcaveFill>,
    cache_slot: &mut Option<BandCutCache>,
) -> Result<Option<BandCut>> {
    let base = solver.base();
    if base.surface(0)?.is_none() || base.surface(1)?.is_none() {
        return Ok(None);
    }
    let Some(edgeuse) = base.edgeuse(0)? else {
        return Ok(None);
    };
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| edge_use_without_edge(edgeuse))?;
    let curve = filleted_edge_curve(target, edge_id)?;
    let ivl = edge::interval(target, edge_id)?;
    let (rolls, on_edge) = rolls_a_ball(solver);
    if !rolls || on_edge {
        return Ok(None); // a constant ball only
    }
    let probe = TsectPnt::new();
    let own = ball_law(k, solver, &probe)?.r[0];
    let chord = solver.base().fsg.as_ref().is_some_and(|fsg| {
        fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear
    });
    // the faces at the edge's ends
    let mut skip = BTreeSet::new();
    let v0 = edge::vertex(target, edge_id)?;
    for v in [v0, edge::other_vertex(target, edge_id, v0)?]
        .into_iter()
        .flatten()
    {
        skip.extend(vertex::faces(target, v)?);
    }
    // the blended faces and the faces sharing an edge with them
    let mut reached = BTreeSet::new();
    for f in edge::faces(target, edge_id, None)? {
        reached.insert(f);
        for e in face::p4::edges(target, f, None, false, None)? {
            reached.extend(edge::faces(target, e, None)?);
        }
    }
    let mut faces = ThroughFaces {
        skip,
        reached,
        usable: vec![false; fills.len()],
        fills,
    };
    let cache = match cache_slot {
        Some(c) => c,
        None => cache_slot.insert(BandCutCache::new(k, target)?),
    };
    // the 32 steps; and where the band is known in closed form, one station inside each
    // stretch between its events, where the section meets the part alike all along, so
    // that every stretch is judged (a fill counts only there)
    let mut stations: Vec<f64> = (0..STEPS)
        .map(|j| ivl.evaluate((j as f64 + 0.5) / STEPS as f64))
        .collect();
    let tol = target.tolerance();
    let band = band_with_events(k, target, cache, solver, &curve, &ivl, chord, edge_id, tol)?;
    match &band {
        Some((band, events)) => {
            let [sa, sb] = band.span;
            for pair in events.windows(2) {
                let [e0, e1] = pair else {
                    continue;
                };
                if e1 - e0 > 2.0 * tol && sb > sa {
                    let s = 0.5 * (e0 + e1);
                    stations.push(ivl.min() + (s - sa) / (sb - sa) * (ivl.max() - ivl.min()));
                }
            }
        }
        // no fill counts where a stretch may go unjudged
        None => faces.fills.clear(),
    }
    let mut out: Option<BandCut> = None;
    let mut near: Option<TsectPnt> = None;
    for t in stations {
        k.check_cancelled(OP)?;
        faces.usable.clear();
        if let Some((band, events)) = &band {
            let s = band.along(curve.evaluate_point(k, t)?);
            for fill in &faces.fills {
                faces.usable.push(fill.covers(band, events, s, tol));
            }
        }
        faces.usable.resize(faces.fills.len(), false);
        let judged = keeps(
            k,
            target,
            cache,
            solver,
            &curve,
            chord,
            own,
            t,
            0.0,
            Judge::StaysInside(&faces),
            &mut near,
        )?;
        if !judged.found || judged.keeps {
            continue;
        }
        // the largest radius whose section here stays inside the part
        let mut at = near.clone();
        let (mut lo, mut hi, mut lo_setback) = (0.0_f64, own, 0.0);
        let mut tr = 0.5 * own;
        while tr > 1.0e-6 * own {
            let s = keeps(
                k,
                target,
                cache,
                solver,
                &curve,
                chord,
                own,
                t,
                tr,
                Judge::StaysInside(&faces),
                &mut at,
            )?;
            if s.found && s.keeps {
                lo = tr;
                lo_setback = s.setback;
                break;
            }
            hi = tr;
            tr *= 0.5;
        }
        let mut i = 0;
        while i < BISECTIONS && lo > 0.0 {
            let mid = 0.5 * (lo + hi);
            let s = keeps(
                k,
                target,
                cache,
                solver,
                &curve,
                chord,
                own,
                t,
                mid,
                Judge::StaysInside(&faces),
                &mut at,
            )?;
            if s.found && s.keeps {
                lo = mid;
                lo_setback = s.setback;
            } else {
                hi = mid;
            }
            i += 1;
        }
        if out.is_none_or(|c| lo < c.radius) {
            out = Some(BandCut {
                radius: lo,
                setback: lo_setback,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{least_on_arc, segment_enters_triangle, Band, ConcaveFill, SectionShape};
    use crate::math::{Point3, Vec3};
    use crate::topology::FaceId;

    /// A fill along an edge parallel to the band's covers it however the two run: along
    /// (1, 1, 0) / sqrt2 stored as the same rounded unit vector, whose dot product with
    /// itself rounds to 1 - 2u (`sqrt(1 - cos²)` read 2.1e-8 there, 2.1e-6 over the
    /// 100 mm band, past the 1e-6 tolerance). A fill turned 1e-6 rad off the band's edge
    /// (1e-4 over its length) does not.
    #[test]
    fn a_parallel_fill_covers_whatever_its_direction_and_a_skew_one_does_not() {
        let dir = Vec3::new(1.0, 1.0, 0.0).unitize().unwrap();
        assert!(dir.dot(dir) < 1.0, "the rounding this test is about");
        let o = Point3::new(0.0, 0.0, 0.0);
        let band = Band {
            shape: SectionShape::Chord,
            rails: [o, o],
            at: o,
            dir,
            span: [-50.0, 50.0],
            phi: 0.0,
        };
        let fill = |dir: Vec3| ConcaveFill {
            edge: o,
            centre: o,
            rails: [o, o],
            dir,
            radius: 1.0,
            span: [-60.0, 60.0],
            faces: [FaceId::from_index(0), FaceId::from_index(1)],
        };
        let tol = 1e-6;
        assert!(fill(dir).covers(&band, &[-50.0, 50.0], 0.0, tol));
        let (c, s) = (1e-6_f64.cos(), 1e-6_f64.sin());
        let skew = Vec3::new(c * dir.x - s * dir.y, s * dir.x + c * dir.y, 0.0);
        assert!(!fill(skew).covers(&band, &[-50.0, 50.0], 0.0, tol));
    }

    /// `least_on_arc` is the least of `a + b cos θ + c sin θ`: inside the arc at the angle
    /// of `-(b, c)`, `a - sqrt(b² + c²)`; at an end otherwise; and NaN where an input is.
    /// `segment_enters_triangle`: a segment along a side, or outside, does not enter; one
    /// across the inside, or wholly inside, does; a degenerate triangle counts as entered.
    #[test]
    fn a_segment_enters_a_triangle_only_through_its_inside() {
        let tri = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(4.0, 0.0, 0.0),
            Point3::new(0.0, 4.0, 0.0),
        ];
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let tol = 1e-6;
        // along a side, and past it
        assert!(!segment_enters_triangle(
            p(-1.0, 0.0),
            p(9.0, 0.0),
            tri,
            tol
        ));
        // outside, beyond the long side
        assert!(!segment_enters_triangle(p(5.0, 0.0), p(0.0, 5.0), tri, tol));
        // across the inside, and wholly inside
        assert!(segment_enters_triangle(p(-1.0, 1.0), p(5.0, 1.0), tri, tol));
        assert!(segment_enters_triangle(p(0.5, 0.5), p(1.0, 1.0), tri, tol));
        // a degenerate triangle
        assert!(segment_enters_triangle(
            p(9.0, 9.0),
            p(10.0, 9.0),
            [tri[0]; 3],
            tol
        ));
    }

    #[test]
    fn least_on_arc_is_the_closed_form_minimum_and_keeps_nan() {
        let pi = std::f64::consts::PI;
        // 1 + cos θ is least, 0, at θ = π
        assert_eq!(least_on_arc(1.0, 1.0, 0.0, 0.5 * pi, 1.5 * pi), 0.0);
        // not reached on [0, π/2]: least at the end π/2, 1
        assert!((least_on_arc(1.0, 1.0, 0.0, 0.0, 0.5 * pi) - 1.0).abs() < 1e-15);
        assert!(least_on_arc(f64::NAN, 1.0, 0.0, 0.0, 1.0).is_nan());
        assert!(least_on_arc(1.0, 1.0, 0.0, f64::NAN, 1.0).is_nan());
        assert!(least_on_arc(1.0, 1.0, 0.0, 0.0, f64::NAN).is_nan());
        assert!(!(least_on_arc(1.0, f64::NAN, 0.0, 0.0, 1.0) >= 0.0));
    }

    /// #19824 review: the caller passes both the body and the solver, so a solver whose
    /// first edge use has no edge in the body is the caller's input
    /// (`BlendFailure::EdgeUseWithoutEdge` at the edge use, `SM_ERR_NULL_POINTER`), not a
    /// defect, for both entries.
    #[test]
    fn a_solver_edge_use_without_an_edge_is_the_callers_input() {
        use crate::blend::fillet_executive::session::SolverObject;
        use crate::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
        use crate::blend::fillet_standard_solver::p1::constant_radius_from_edgeuse;
        use crate::error::{EntityRef, ErrorKind, SM_ERR_NULL_POINTER};
        use crate::primitive::api_primitives::create_box;
        use crate::topology::edgeuse::p1 as eu;
        use crate::Kernel;

        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let e = b.edges().ids().next().unwrap();
        let edgeuse = eu::primary_edgeuse(&b, e).unwrap().unwrap();
        let mut pseudo = FilletBrep::new();
        let solver = constant_radius_from_edgeuse(
            &k,
            &mut b,
            &mut pseudo,
            FilletSolverId(0),
            1e-6,
            0.5,
            0.03,
            1.0,
            edgeuse,
            None,
        )
        .unwrap();
        let mut solver = SolverObject::ConstantRadius(Box::new(solver));
        b.edgeuse_mut(edgeuse).unwrap().edge = None;
        for breaks in [false, true] {
            let error = if breaks {
                super::band_breaks_through_the_part(&k, &mut b, &mut solver)
            } else {
                super::band_cuts_the_part(&k, &mut b, &mut solver)
            }
            .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error:?}");
            assert!(!error.is_defect(), "{error:?}");
            assert_eq!(
                error.entity(),
                Some(EntityRef::Edgeuse(edgeuse)),
                "{error:?}"
            );
            assert_eq!(
                error.failed_routine_status(),
                Some(SM_ERR_NULL_POINTER),
                "{error:?}"
            );
        }
    }
}
