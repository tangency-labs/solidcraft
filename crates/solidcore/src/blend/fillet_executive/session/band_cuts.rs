//! Whether a convex blend would take all of the part between its rails somewhere along its
//! edge, cutting it into pieces, and the largest size that keeps it whole.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork (tangency-labs/smlib#79,
//! merged at 5e8c9c1) adds `SmFilletSolver::BandCutsThePart` with `sm_EdgesCrossingPlane`
//! and `sm_KeepsMaterialBehind` (`SmFilletSolver.cpp`, `SmFilletSolver.h`) in commits
//! 862971f, e568e45 (the section intersected with every face, not sampled) and aac4288 (a
//! run of cutting sections reaching an end of the edge is no cut). They run only with the
//! opt-in `Kernel::fixes.fillet_gn_section_generator`.
//!
//! | Rust                          | Upstream (fork)                          |
//! |-------------------------------|------------------------------------------|
//! | [`band_cuts_the_part`]        | `SmFilletSolver::BandCutsThePart`        |
//! | [`BandCut`]                   | its outputs `rbCuts`, `rdRadius`, `rdSetback` |
//! | `edges_crossing_plane` (private) | `sm_EdgesCrossingPlane`               |
//! | `keeps_material_behind` (private) | `sm_KeepsMaterialBehind`             |
//! | `keeps` (private)             | the `Keeps` lambda of `BandCutsThePart`  |
//!
//! Deviations: the fork's out-params are returned (`None` for no cut). The solutions of
//! `SmSurface::GlobalCurveIntersect` carry no object handles (the fork reads only their
//! parameters). Every loop is bounded by fixed counts (32 steps, 40 bisections, halvings
//! down to a millionth of the radius); there is no recursion.

// the fork's NaN-safe tests (`!(a > b)`) are kept as written
#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::blend::fillet_executive::session::ball_regular::{
    ball_law, ball_section_at, rolls_a_ball, set_ball_radius,
};
use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use crate::curve::curve::Curve;
use crate::curve::ellipse::p2::EllipseOptions;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::EntityRef;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec3, EFF_ZERO};
use crate::query::brep::{BrepFaceServices, BrepQuery};
use crate::surface::plane::p1::Plane;
use crate::surface::surface::Surface;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{face, Brep};

const OP: &str = "blend::fillet_executive::session::band_cuts";

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
    for e in crate::topology::brep::p1::edges(body, None)? {
        let Some(curve) = edge::evaluation_curve(body, e)? else {
            continue;
        };
        let interval = edge::interval(body, e)?;
        let solutions = plane.global_curve_intersect(
            k,
            [
                Operand::Curve(curve.natural_interval()),
                Operand::Plane(domain),
            ],
            &domain,
            &curve,
            &interval,
            tol,
        )?;
        for sol in solutions.as_slice() {
            let t = sol
                .start
                .parameters
                .first()
                .copied()
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            crossings.push(curve.evaluate_point(k, t)?);
            if sol.solution_type == SolutionType::RangeOfValues {
                let t = sol
                    .end
                    .parameters
                    .first()
                    .copied()
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                crossings.push(curve.evaluate_point(k, t)?);
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
    for f in crate::topology::brep::p1::faces(body, None)? {
        // the crossings inside the face's surface's domain, judged after the borrow ends
        let mut on_face: Vec<Point2> = Vec::new();
        {
            let Some(surface) = face::p2::surface(body, f)? else {
                continue;
            };
            let surface = surface.clone_for_evaluation()?;
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            // the face's surface solves through its face (its trimmed cache), as upstream's
            // `GlobalCurveIntersect` reaches the owner's cache
            let mut services = BrepFaceServices::new(&mut *body);
            let solutions = surface.global_curve_intersect_with_context::<CacheObject>(
                k,
                [None, Some(CacheObject::Surface(f))],
                None,
                &mut services,
                &domain,
                &section,
                &interval,
                ends,
            )?;
            for sol in solutions.as_slice() {
                if sol.solution_type == SolutionType::RangeOfValues {
                    return Ok(true); // the section runs along the face's surface
                }
                let params = &sol.start.parameters;
                let (Some(&t), Some(&u), Some(&v)) = (params.first(), params.get(1), params.get(2))
                else {
                    return Err(SolidError::null_pointer(OP));
                };
                let at = section.evaluate_point(k, t)?;
                if at.distance(rail1) <= ends || at.distance(rail2) <= ends {
                    continue; // the section's own ends, on the blended faces
                }
                on_face.push(Point2::new(u, v));
            }
        }
        for uv in on_face {
            let mut class = PointClassification::<EntityRef>::new(zone);
            crate::query::face::p2::point_classify(
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
    let class = BrepQuery::of_brep(body).point_3d_classify(k, body, middle, zone, true)?;
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

/// The `Keeps` lambda of `BandCutsThePart`: at the edge's `t` with the ball's radius
/// `trial` (0: the solver's own, restored to `own` after), whether material is kept behind
/// the section; `near` is the guess, then this section if found.
#[allow(clippy::too_many_arguments)]
fn keeps(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    edge_curve: &Curve,
    chord: bool,
    own: f64,
    t: f64,
    trial: f64,
    crossings: &[Point3],
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
    let evaluated = (|| -> Result<Option<(TsectPnt, [Point3; 3])>> {
        let Some(pnt) = ball_section_at(k, target, solver, edge_curve, t, near.as_ref())? else {
            return Ok(None);
        };
        let uvs = [pnt.uv_pos(SrfIndex::First), pnt.uv_pos(SrfIndex::Second)];
        let base = solver.base_mut();
        let mut at = [Point3::UNDEF; 3];
        {
            let slot = base
                .surface_mut(0)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            at[0] = slot.surface.surface.evaluate_point_extending(k, uvs[0])?;
        }
        for (rail, uv) in uvs.into_iter().enumerate() {
            let slot = base
                .surface(rail)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let p = slot.surface.surface.base_surface().evaluate_point(k, uv)?;
            if let Some(dst) = at.get_mut(rail + 1) {
                *dst = p;
            }
        }
        // the rails past a base surface's domain lie on its extension, as the offset evaluates it
        for (rail, uv) in uvs.into_iter().enumerate() {
            let slot = base
                .surface(rail)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            let offset = &slot.surface.surface;
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
    })();
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
    out.keeps = keeps_material_behind(k, target, centre, [rail1, rail2], chord, crossings)?;
    Ok(out)
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
    let base = solver.base();
    if base.surface(0)?.is_none() || base.surface(1)?.is_none() {
        return Ok(None);
    }
    let Some(edgeuse) = base.edgeuse(0)? else {
        return Ok(None);
    };
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let curve = edge::evaluation_curve(target, edge_id)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone();
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
            return Err(SolidError::null_pointer(OP));
        };
        if !(tangent.length() > EFF_ZERO) {
            all_crossings.push(Vec::new());
            nears.push(None);
            states.push(2);
            continue;
        }
        let normal = tangent.unitize()?;
        let crossings = edges_crossing_plane(k, target, point, normal)?;
        let judged = keeps(
            k, target, solver, &curve, chord, own, t, 0.0, &crossings, &mut near,
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
                    k, target, solver, &curve, chord, own, t, tr, crossings, &mut at,
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
                    k, target, solver, &curve, chord, own, t, mid, crossings, &mut at,
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
