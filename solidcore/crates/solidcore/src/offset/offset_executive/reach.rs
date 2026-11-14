//! How far past its face an offset face's surface must reach for the offsets of its
//! neighbours to meet its own: the margin the offset executive extends each surface by before
//! it intersects the offsets (not upstream: upstream extends every surface by five offset
//! distances).
//!
//! At a point `p` of an edge between a face `F` (unit normal `nF`, binormal `bF` into `F`,
//! offset `dF`) and its neighbour `G` (unit normal `nG`, offset `dG`), `c = nF·nG`, the
//! offsets' tangent planes `(x − p)·nF = dF` and `(x − p)·nG = dG` meet on the line through
//! `p + a nF + β nG`, `a + β c = dF`, `a c + β = dG`. Its foot on `F`'s tangent plane lies
//! past the edge (along the outward binormal `m = −bF`) by the edge's signed reach
//!
//! ```text
//! r = β (nG·m),   β = (dG − c dF) / (1 − c²)   ( = d / (1 + c) where dF = dG = d ),
//! ```
//!
//! `d·cot(α / 2)` for equal offsets, `α` the sector angle of the solid between the faces:
//! `d` across a box's edge, `0` across a tangent edge, negative where the offsets meet inside
//! `F`. At a vertex where `F`'s edge uses meet with reaches `r1`, `r2` and outward binormals
//! `m1`, `m2`, the offset corner's foot `x` on the tangent plane solves `x·m1 = r1`,
//! `x·m2 = r2`, at `|x|` from the vertex.
//!
//! Each foot is within `|r|` (`|x|` at a corner) of a point of the face, so within that of the
//! face's surface's box, however far the meeting runs along the face: an inward meeting that
//! leaves the face across another of its edges (a hole's offset growing past a side) is held
//! as well as an outward one. The reach of a face is the largest of these, sampled along each
//! edge (its sector angle varies) and at each corner. Faces whose offsets never meet, a knife
//! edge's (`c = −1` for equal offsets) or tangent faces offset apart (`c = 1`, `dF ≠ dG`),
//! add nothing: no extension makes them meet.
//!
//! An extension meets its distance at the middle of each side, and a reach is needed at every
//! station. [`shortest_across`] bounds the length across a B-spline's sides from below, for
//! the executive to ask again where the rows narrow along a side (#2908; the measurements and
//! the choice of asking again are `MarkusReichart`'s on #2926). Analytic faces are left as
//! they were.

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::curve::curve::CurveKind;
use crate::error::offset::OffsetFailure;
use crate::error::Failure;
use crate::error::{Result, SolidError};
use crate::geom::certified;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::{sqrt, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, EFF_ZERO};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::p2::IsoCurveOptions;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::nurbs_crv::create_bez_segments;
use crate::surface::extend::{ExtendedSides, SideExtension};
use crate::surface::implicit::{self, AnalyticSupport, Implicit};
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::{p1 as eu, p3 as eu3};
use crate::topology::{edge, loopuse, Brep, EdgeuseId, FaceId, FaceuseId};
use std::collections::BTreeMap;

mod net;
mod quadric;

const OP: &str = "offset::offset_executive::reach::extension_reach";
const LOCAL_OP: &str = "offset::offset_executive::reach::local_extension_reach";

/// The number of intervals each edge is sampled at for its sector angle.
const EDGE_SAMPLES: u32 = 8;

/// One end of an edge use, as the corner beside it reads it.
#[derive(Debug, Clone, Copy)]
struct End {
    /// The signed reach past the edge.
    reach: f64,
    /// The outward binormal (away from the face, perpendicular to the edge).
    outward: Vec3,
}

/// The signed reach past the edge of a face offset by `own` whose neighbour is offset by
/// `other`, from their unit normals and the face's outward binormal (module docs); zero where
/// the offsets do not meet: faces folded back on each other (equal offsets) or tangent to
/// each other (different offsets: their offsets run apart), each within `tangent_sine`, the
/// sine of the tangency tolerance.
fn signed_reach(
    n_own: Vec3,
    n_other: Vec3,
    outward: Vec3,
    own: f64,
    other: f64,
    tangent_sine: f64,
) -> f64 {
    let c = n_own.dot(n_other);
    let sine_squared = (tangent_sine * tangent_sine).max(EFF_ZERO);
    #[allow(clippy::float_cmp)] // the same offset, not a computed value
    let beta = if own == other {
        // 1 + c = 1 - cos(θ) ≈ θ² / 2 for normals θ off opposite
        let denominator = 1.0 + c;
        if denominator <= 0.5 * sine_squared {
            return 0.0;
        }
        own / denominator
    } else {
        // 1 - c² = sin²(θ) for normals θ apart
        let denominator = 1.0 - c * c;
        if denominator <= sine_squared {
            return 0.0;
        }
        (other - c * own) / denominator
    };
    beta * n_other.dot(outward)
}

/// The signed reach past `eu`'s edge at `parameter` (module docs), with `eu`'s outward
/// binormal there, and whether the edge has a neighbour (zero reach without).
fn edge_reach(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    parameter: f64,
    offset: f64,
    unmoved: &[FaceId],
    tangent_sine: f64,
) -> Result<(End, bool)> {
    let options = eu3::BinormalOptions::default();
    let own = eu3::evaluate_binormal(k, b, eu, parameter, false, &options)?;
    let outward = -own.binormal;
    // A seam's radial is on the face itself (its offset continues across), a lamina
    // edge's is its mate on the face's other side (its offset is closed by a side wall).
    let own_face = eu::face(b, eu)?;
    let neighbour = match eu::radial(b, eu)? {
        Some(r) if r != eu && eu::face(b, r)? != own_face => Some(r),
        _ => None,
    };
    let Some(radial) = neighbour else {
        let end = End {
            reach: 0.0,
            outward,
        };
        return Ok((end, false));
    };
    let other_offset = match eu::face(b, radial)? {
        Some(f) if unmoved.contains(&f) => 0.0,
        _ => offset,
    };
    let other = eu3::evaluate_binormal(k, b, radial, parameter, false, &options)?;
    let n_own = own.face_normal.unitize()?;
    let n_other = other.face_normal.unitize()?;
    let end = End {
        reach: signed_reach(n_own, n_other, outward, offset, other_offset, tangent_sine),
        outward,
    };
    Ok((end, true))
}

/// The distance from a vertex of the foot of its offset corner on the face's tangent plane
/// (module docs), from the reaches and outward binormals of the edge uses meeting there.
/// Where the edges run on tangent to each other (within `tangent_sine`, the sine of the
/// tangency tolerance) the corner is no corner: the offset edges run on into each other, and
/// the larger reach stands (the tangent lines' crossing, far off for nearly parallel lines,
/// is not where curved edges' offsets meet).
fn corner_reach(into: End, out: End, tangent_sine: f64) -> f64 {
    let g = into.outward.dot(out.outward);
    let det = 1.0 - g * g;
    if det <= (tangent_sine * tangent_sine).max(EFF_ZERO) {
        return into.reach.abs().max(out.reach.abs());
    }
    let a = (into.reach - g * out.reach) / det;
    let c = (out.reach - g * into.reach) / det;
    sqrt((a * a + c * c + 2.0 * a * c * g).max(0.0))
}

/// How far past its face the surface of `faceuse` must be extended for the offsets of its
/// neighbours to meet its own, the face offset by `offset` (signed, along the face use's
/// normal) and each neighbour by the same, except the `unmoved` faces, which stay where they
/// are (module docs): the largest distance from the face of the feet of the meetings, sampled
/// along each edge at its sector angle and at each corner (edges meeting within
/// `tangency_tol_radians` of tangent make no corner); `None` for a face with no
/// neighbour (every edge a lamina edge or a seam), whose offset meets no other.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an offset that is not finite or a vanishing face normal;
/// those of [`eu3::evaluate_binormal`]; [`SolidError::NotFound`] for a dangling id.
pub fn extension_reach(
    k: &Kernel,
    b: &mut Brep,
    faceuse: FaceuseId,
    offset: f64,
    unmoved: &[FaceId],
    tangency_tol_radians: f64,
) -> Result<Option<f64>> {
    if !offset.is_finite() {
        return Err(SolidError::invalid_input(OP, "the offset is not finite"));
    }
    let tangent_sine = tangency_tol_radians.abs().min(crate::math::PI / 2.0).sin();
    let mut reach = 0.0_f64;
    let mut met = false;
    for lu in eu::faceuse_loopuses(b, faceuse, false, None)? {
        let mut ends: Vec<(End, End)> = Vec::new();
        for edgeuse in loopuse::edgeuses(b, lu, None, None)? {
            let Some(e) = eu::edge(b, edgeuse)? else {
                continue;
            };
            let interval = edge::interval(b, e)?;
            let (lo, hi) = (interval.min(), interval.max());
            let mut first = None;
            let mut last = None;
            for i in 0..=EDGE_SAMPLES {
                let parameter = lo + (hi - lo) * f64::from(i) / f64::from(EDGE_SAMPLES);
                let (end, neighbour) =
                    edge_reach(k, b, edgeuse, parameter, offset, unmoved, tangent_sine)?;
                met |= neighbour;
                reach = reach.max(end.reach.abs());
                if i == 0 {
                    first = Some(end);
                }
                last = Some(end);
            }
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            // The edge use runs from its start to its end vertex along the loop.
            ends.push(match eu::orientation(b, edgeuse)? {
                OrientType::Opposite => (last, first),
                _ => (first, last),
            });
        }
        // Each edge use's end meets the next one's start (the loop closes).
        let next = ends.iter().cycle().skip(1);
        for (&(_, into), &(out, _)) in ends.iter().zip(next) {
            reach = reach.max(corner_reach(into, out, tangent_sine));
        }
    }
    Ok(met.then_some(reach))
}

/// A lower bound of the distance from each extended side of `base`'s B-spline to the same
/// side of `extended`, over the side's stations on `base`'s box: `None` where `base` is no
/// B-spline or the offset of one, a side's two rows differ in weights, or the bound is not positive. Each row is a
/// curve in the same parameter and basis, with the same weights, so the difference at a
/// station is a convex combination of the differences of their control points, and its length
/// is at least the least of those along their mean direction. Past the box's corners the
/// rows go on narrowing, to nothing at a ruled face's apex; the margin's one offset past the
/// reach is what stands for them.
///
/// # Errors
///
/// Those of the domains and the iso curves.
pub(super) fn shortest_across(
    k: &Kernel,
    base: &Surface,
    extended: &ExtendedSides,
) -> Result<Option<f64>> {
    // an offset surface is the offset of its base extended: the rows are its base's
    let (near_surface, far_surface) = match (base.kind(), extended.surface.kind()) {
        (SurfaceKind::OffsetSurface(a), SurfaceKind::OffsetSurface(b)) => {
            (a.base_surface(), b.base_surface())
        }
        _ => (base, &extended.surface),
    };
    let (SurfaceKind::BSpline(_), SurfaceKind::BSpline(net)) =
        (near_surface.kind(), far_surface.kind())
    else {
        return Ok(None);
    };
    let from = near_surface.natural_uv_domain_with_nurb(k)?;
    let to = far_surface.natural_uv_domain_with_nurb(k)?;
    let mut shortest = f64::INFINITY;
    for (&side, done) in BoxSide::ALL.iter().zip(&extended.sides) {
        if !matches!(done, SideExtension::Extended(_)) {
            continue;
        }
        let along = if side.is_u() {
            SurfParamType::U
        } else {
            SurfParamType::V
        };
        // the side's stations on the source's box, the row wherever it lies
        let box_of_row = if side.is_u() {
            Extent2d::from_points(
                Point2::new(to.min().x, from.min().y),
                Point2::new(to.max().x, from.max().y),
            )
        } else {
            Extent2d::from_points(
                Point2::new(from.min().x, to.min().y),
                Point2::new(from.max().x, to.max().y),
            )
        };
        let row = |at: f64| {
            let options = IsoCurveOptions {
                domain: Some(box_of_row),
                uv_iso_curve: false,
            };
            net.create_iso_parametric_curve(k, along, at, options)?
                .curve
                .canonical()
        };
        let (near, far) = (row(side.at(&from))?, row(side.at(&to))?);
        if near.control_points.len() != far.control_points.len() || near.weights != far.weights {
            return Ok(None);
        }
        let mut mean = Vec3::ZERO;
        for (a, b) in near.control_points.iter().zip(&far.control_points) {
            mean += *b - *a;
        }
        let Ok(direction) = mean.unitize() else {
            return Ok(None);
        };
        for (a, b) in near.control_points.iter().zip(&far.control_points) {
            shortest = shortest.min((*b - *a).dot(direction));
        }
    }
    Ok((shortest.is_finite() && shortest > 0.0).then_some(shortest))
}

/// A plane: a point on it and its unit normal, and the normal as its label gives it (the
/// exact plane, which the interval bounds read: [`plane_trace`], [`planes_line`]).
#[derive(Debug, Clone, Copy)]
struct Tangent {
    point: Vec3,
    normal: Vec3,
    raw: Vec3,
}

/// The point nearest `p` where the planes `a` and `b` meet: on their line of meeting, the
/// point `p + α a.n + β b.n` (`α + β c = h_a`, `α c + β = h_b`, `c = a.n·b.n`, `h` each
/// plane's height over `p`). Planes within `tangent_sine` of parallel meet only where they
/// are one plane (within `tolerance` apart; then at `p`'s foot on it), else not at all.
fn meeting_near(
    p: Vec3,
    a: Tangent,
    b: Tangent,
    tangent_sine: f64,
    tolerance: f64,
) -> Option<Vec3> {
    let c = a.normal.dot(b.normal);
    let h_a = (a.point - p).dot(a.normal);
    let h_b = (b.point - p).dot(b.normal);
    let det = 1.0 - c * c;
    if det <= (tangent_sine * tangent_sine).max(EFF_ZERO) {
        // one plane where the other's point lies on it, else apart
        return ((b.point - a.point).dot(a.normal).abs() <= tolerance).then(|| p + a.normal * h_a);
    }
    let alpha = (h_a - c * h_b) / det;
    let beta = (h_b - c * h_a) / det;
    Some(p + a.normal * alpha + b.normal * beta)
}

/// The point where the planes `a`, `b` and `c` meet, `None` where they meet in no one point
/// (two of them within `tangent_sine` of parallel: their triple product is below it).
fn corner_of(a: Tangent, b: Tangent, c: Tangent, tangent_sine: f64) -> Option<Vec3> {
    let bc = b.normal.cross(c.normal);
    let det = a.normal.dot(bc);
    if det.abs() <= tangent_sine.max(EFF_ZERO) {
        return None;
    }
    let (ha, hb, hc) = (
        a.point.dot(a.normal),
        b.point.dot(b.normal),
        c.point.dot(c.normal),
    );
    Some((bc * ha + c.normal.cross(a.normal) * hb + a.normal.cross(b.normal) * hc) * (1.0 / det))
}

/// A class a local operation's reach is bounded on: a plane, or a cone (a cylinder for
/// `tan_angle = 0`) or a sphere, as [`Implicit`] holds them, or a bare B-spline net
/// ([`net::Net`]).
#[derive(Debug, Clone)]
enum Exact {
    Plane(Tangent),
    Quadric(Implicit),
    Net(std::rc::Rc<net::Net>),
}

/// `surface`'s class: its analytic label (a plane, cylinder, cone or sphere) with its NURBS
/// form proved on that label's implicit equation ([`implicit::lies_on`], the owner's rule
/// for an exact class at bound 0); a B-spline surface or extrusion with no label and no
/// definition is a bare net where [`net::Net::of`] takes it, a plane stored as a NURBS among
/// them. `None` for any other kind, a net [`net::Net::of`] turns away, a torus (its
/// sections are quartics), or a form not proved on its label.
fn exact_class(k: &Kernel, surface: &Surface) -> Result<Option<Exact>> {
    let Some(support) = AnalyticSupport::of(surface) else {
        return Ok(net::Net::of(k, surface)?.map(|n| Exact::Net(std::rc::Rc::new(n))));
    };
    if matches!(support, AnalyticSupport::Torus { .. }) {
        return Ok(None);
    }
    let equation = support.implicit();
    let Some(form) = crate::blend::fillet_section::station::as_b_spline(k, surface)? else {
        return Ok(None);
    };
    if !implicit::lies_on(&form, &equation)? {
        return Ok(None);
    }
    Ok(match equation {
        Implicit::Plane { origin, normal } => Some(Exact::Plane(Tangent {
            point: origin,
            normal: normal.unitize()?,
            raw: normal,
        })),
        Implicit::Cone { .. } | Implicit::Sphere { .. } => Some(Exact::Quadric(equation)),
        Implicit::Torus { .. } => None,
    })
}

/// A certified upper bound on the distance from `x` to the surface of class `c` at a point
/// (a moved surface's move at a vertex); `None` for a net, whose move is bounded by the
/// meetings on it.
fn point_distance(x: Vec3, c: &Exact) -> Option<f64> {
    match c {
        Exact::Plane(t) => Some(plane_distance(x, *t)),
        Exact::Quadric(q) => Some(round_up(quadric_gap(q, Dv::constant(x)).hi)),
        Exact::Net(_) => None,
    }
}

/// How two planes beside an edge meet, decided once for the whole edge from the planes
/// alone (the thresholds of [`meeting_near`]); two parallel quadrics are one or apart in the
/// same sense ([`quadric_pair`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanePair {
    /// On a line.
    Line,
    /// They are one surface (two planes within the tangency tolerance of parallel and within
    /// the B-rep's tolerance apart; two quadrics as [`quadric_pair`] decides).
    One,
    /// Parallel and apart: they meet nowhere.
    Apart,
}

fn plane_pair(a: Tangent, b: Tangent, tangent_sine: f64, tolerance: f64) -> PlanePair {
    let c = a.normal.dot(b.normal);
    if 1.0 - c * c > (tangent_sine * tangent_sine).max(EFF_ZERO) {
        PlanePair::Line
    } else if (b.point - a.point).dot(a.normal).abs() <= tolerance {
        PlanePair::One
    } else {
        PlanePair::Apart
    }
}

/// How two quadrics beside an edge meet where they are parallel surfaces, decided once for
/// the whole edge from the quadrics alone, as [`plane_pair`] decides two planes: two spheres
/// about one centre, or two cones (cylinders) about one axis whose generators are parallel.
/// One centre or axis is within `tolerance` (`e`, how far the second's centre or origin is
/// off the first's centre or axis); parallel is within [`EFF_ZERO`] (the sine of the axes'
/// angle, and of the generators'), a label's rounding. Their gap `g` along the normals is the
/// difference of their radii (a cone's at one height, over `√(1 + t²)`), so the surfaces lie
/// between `|g| − e` and `|g| + e` apart: one surface ([`PlanePair::One`]) where
/// `|g| + e ≤ tolerance`, apart ([`PlanePair::Apart`]) where `|g| > tolerance + e`. On the
/// faces' nappes, two quadrics decided apart could meet only past `tolerance / EFF_ZERO`.
/// Unlike two planes, not within the tangency tolerance: coaxial quadrics a small angle
/// apart (a cylinder's half drafted 0.3 degrees beside the half it was split from) meet on
/// the edge, which [`quadric::meet`] proves. `None` for any pair not parallel, or with a gap
/// between those bounds: [`quadric::meet`] decides it.
fn quadric_pair(a: &Implicit, b: &Implicit, tolerance: f64) -> Option<PlanePair> {
    let sine_squared = EFF_ZERO * EFF_ZERO;
    let (gap, off) = match (*a, *b) {
        (
            Implicit::Sphere {
                centre: ca,
                radius: ra,
            },
            Implicit::Sphere {
                centre: cb,
                radius: rb,
            },
        ) => (rb - ra, (cb - ca).length()),
        (
            Implicit::Cone {
                origin: oa,
                axis: aa,
                radius: ra,
                tan_angle: ta,
            },
            Implicit::Cone {
                origin: ob,
                axis: ab,
                radius: rb,
                tan_angle: tb,
            },
        ) => {
            let c = aa.dot(ab);
            if aa.cross(ab).length_squared()
                > sine_squared * aa.length_squared() * ab.length_squared()
            {
                return None;
            }
            // the second cone's slope and height along the first's axis
            let tb = if c < 0.0 { -tb } else { tb };
            let v = ob - oa;
            let z = v.dot(aa);
            // the generators' normals in a meridian plane, (1, −t) / √(1 + t²), parallel
            let (wa, wb) = (1.0 + ta * ta, 1.0 + tb * tb);
            if (ta - tb) * (ta - tb) > sine_squared * wa * wb {
                return None;
            }
            // at height h on the first's axis the radii are ra + ta h and rb + tb (h − z)
            ((rb - tb * z - ra) / wa.sqrt(), (v - aa * z).length())
        }
        _ => return None,
    };
    // a centre or axis not proved within the tolerance (or not a number): not one axis
    if off.is_nan() || off > tolerance {
        return None;
    }
    if gap.abs() + off <= tolerance {
        Some(PlanePair::One)
    } else if gap.abs() > tolerance + off {
        Some(PlanePair::Apart)
    } else {
        None
    }
}

/// The rounding of a unit normal and a height over it, relative to the distance of the
/// point from the plane's point: the height's dot product (`gamma_4`), the difference
/// (`gamma_1`), and each normal component within [`certified::unit_error`] of exact
/// (`√3 < 2`).
fn height_rounding() -> f64 {
    certified::gamma(8) + 2.0 * certified::unit_error()
}

/// An upper bound on the distance from `x` to the plane `a`.
fn plane_distance(x: Vec3, a: Tangent) -> f64 {
    let d = x - a.point;
    d.dot(a.normal).abs() + height_rounding() * d.length() + f64::from_bits(1)
}

/// An upper bound on the distance from `x` to the line where the planes `a` and `b` meet
/// (`det = 1 − c²` above the tangency threshold, [`PlanePair::Line`]): the computed foot
/// ([`meeting_near`]) and the rounding of its solve, whose heights are each off by
/// [`height_rounding`] of their size and whose `α`, `β` divide by `det`, itself off by
/// `gamma_8`, so off by both over `det²`.
fn line_distance(x: Vec3, a: Tangent, b: Tangent, tangent_sine: f64, tolerance: f64) -> f64 {
    let c = a.normal.dot(b.normal);
    let det = 1.0 - c * c;
    let Some(foot) = meeting_near(x, a, b, tangent_sine, tolerance) else {
        return f64::INFINITY;
    };
    let size = (a.point - x).length() + (b.point - x).length();
    let solve = (4.0 * height_rounding() + certified::gamma(16)) * size / (det * det);
    let value = (foot - x).length();
    value + solve + certified::gamma(5) * value + f64::from_bits(1)
}

/// An upper bound on the distance from `p` to the corner `x` of the planes `a`, `b`, `c`
/// ([`corner_of`]): the computed distance and the rounding of the solve, its heights each
/// off by [`height_rounding`] of their size and the triple product `det` by `gamma_8`, so
/// off by both over `det²`.
fn corner_distance(p: Vec3, x: Vec3, planes: [Tangent; 3]) -> f64 {
    let [a, b, c] = planes;
    let det = a.normal.dot(b.normal.cross(c.normal)).abs();
    let size: f64 = planes
        .iter()
        .map(|t| t.point.dot(t.normal).abs() + t.point.length())
        .sum();
    let solve = (4.0 * height_rounding() + certified::gamma(16)) * size / (det * det);
    let value = (x - p).length();
    value + solve + certified::gamma(5) * (value + p.length() + x.length()) + f64::from_bits(1)
}

/// The most halvings a piece of an edge gets in [`edge_maximum`].
const MAX_HALVINGS: usize = 40;

/// The most pieces of one edge [`edge_maximum`] looks at.
const MAX_PIECES: usize = 4096;

/// A piece of an edge's curve: its homogeneous Bézier coefficients `[w x, w y, w z, w]`,
/// the absolute error each may carry, and how many times it was halved.
#[derive(Debug, Clone)]
struct Piece {
    cp: Vec<[f64; 4]>,
    err: f64,
    depth: usize,
}

/// Linear interpolation of homogeneous points.
fn lerp(a: [f64; 4], b: [f64; 4], s: f64) -> [f64; 4] {
    let mut out = a;
    for (o, (x, y)) in out.iter_mut().zip(a.iter().zip(b.iter())) {
        *o = (1.0 - s) * x + s * y;
    }
    out
}

/// The de Casteljau split of Bézier coefficients at `s`.
fn split_at(c: &[[f64; 4]], s: f64) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
    let mut work = c.to_vec();
    let mut left = Vec::new();
    let mut right = Vec::new();
    while let (Some(&first), Some(&last)) = (work.first(), work.last()) {
        left.push(first);
        right.push(last);
        work = work
            .windows(2)
            .filter_map(|w| match w {
                [x, y] => Some(lerp(*x, *y, s)),
                _ => None,
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The Euclidean control points of a piece, each with a bound on its distance from the
/// exact one (the homogeneous error `e` through the division by the weight `w`, and the
/// division's own rounding); `None` where a weight is not certified positive (the convex
/// hull property needs positive weights).
fn euclidean(piece: &Piece) -> Option<Vec<(Vec3, f64)>> {
    let e = piece.err;
    piece
        .cp
        .iter()
        .map(|&[x, y, z, w]| {
            if !certified::above(w - e, 0.0) {
                return None;
            }
            let p = Vec3::new(x / w, y / w, z / w);
            let size = p.x.abs().max(p.y.abs()).max(p.z.abs());
            let err = 2.0 * (e * (1.0 + size) / (w - e) + certified::gamma(1) * size);
            (p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).then_some((p, err))
        })
        .collect()
}

/// The homogeneous Bézier pieces of edge `e`'s curve over the edge's interval, with the
/// error each coefficient may carry; `None` for an edge with no curve, a curve with no
/// exact NURB (a line, conic or B-spline has one), or a 2D one.
fn edge_pieces(b: &Brep, e: crate::topology::EdgeId) -> Result<Option<Vec<Piece>>> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    let line;
    let nurb = match curve.kind() {
        CurveKind::BSpline(c) => c.gw_nurb(),
        CurveKind::Ellipse(c) => c.base().gw_nurb(),
        CurveKind::Parabola(c) => c.base().gw_nurb(),
        CurveKind::Hyperbola(c) => c.base().gw_nurb(),
        CurveKind::Line(c) => {
            line = c.base_with_nurb()?;
            line.gw_nurb()
        }
        _ => None,
    };
    let Some(nurb) = nurb else {
        return Ok(None);
    };
    let control = |c: CPoint| {
        if c.w == NOW {
            [c.x, c.y, c.z, 1.0]
        } else {
            [c.x, c.y, c.z, c.w]
        }
    };
    if nurb.cpts().iter().any(|c| c.z == NOZ) {
        return Ok(None);
    }
    // every coefficient made from the curve's is a convex combination of them, so no
    // larger: each combination rounds by `gamma_3` of this
    let size = nurb
        .cpts()
        .iter()
        .flat_map(|&c| control(c))
        .fold(0.0_f64, |m, x| m.max(x.abs()));
    // the Bézier extraction inserts up to `p + 1` knots into each span
    #[allow(clippy::cast_precision_loss)] // a degree
    let extraction = (nurb.degree() + 2) as f64 * certified::gamma(4) * size;
    let interval = edge::interval(b, e)?;
    let (lo, hi) = (interval.min(), interval.max());
    let mut pieces = Vec::new();
    for seg in create_bez_segments(nurb).map_err(SolidError::nurbs_routine(LOCAL_OP))? {
        let (Some(&k0), Some(&k1)) = (seg.knots().first(), seg.knots().last()) else {
            continue;
        };
        let (a, c) = (lo.max(k0), hi.min(k1));
        if !(a < c && k0 < k1) {
            continue;
        }
        let cp: Vec<[f64; 4]> = seg.cpts().iter().map(|&p| control(p)).collect();
        let s0 = (a - k0) / (k1 - k0);
        let s1 = (c - k0) / (k1 - k0);
        let (_, right) = split_at(&cp, s0);
        let cp = if s0 < 1.0 {
            split_at(&right, (s1 - s0) / (1.0 - s0)).0
        } else {
            right
        };
        pieces.push(Piece {
            cp,
            err: extraction + 2.0 * certified::gamma(3) * size,
            depth: 0,
        });
    }
    Ok(Some(pieces))
}

/// A certified upper bound on the largest value over edge `e` of a convex function `f`
/// that is 1-Lipschitz (a distance to a plane or a line), given as an upper bound of its
/// value at a point: on each Bézier piece (positive weights) the curve lies in its control
/// points' hull, where `f` is largest at a control point, so the largest of `f` there, each
/// raised by its point's error, bounds the piece. A piece is halved while that bound exceeds
/// `f` at its end points (points of the curve) by more than `tolerance`, at most
/// [`MAX_HALVINGS`] times and over at most [`MAX_PIECES`] pieces; a piece whose bound is
/// below the largest bound found is dropped. The bound is the hull's whichever way it
/// stops: never a sample. `None` where the edge has no exact NURB or a weight is not
/// certified positive.
fn edge_maximum(
    b: &Brep,
    e: crate::topology::EdgeId,
    f: &dyn Fn(Vec3) -> f64,
    tolerance: f64,
) -> Result<Option<f64>> {
    let Some(mut work) = edge_pieces(b, e)?.filter(|w| !w.is_empty()) else {
        return Ok(None);
    };
    let size = work
        .iter()
        .flat_map(|p| p.cp.iter())
        .flat_map(|c| c.iter())
        .fold(0.0_f64, |m, x| m.max(x.abs()));
    let mut found = 0.0_f64;
    let mut seen = 0_usize;
    while let Some(piece) = work.pop() {
        seen += 1;
        let Some(points) = euclidean(&piece) else {
            return Ok(None);
        };
        // (`f64::max` drops a NaN: every value is checked first)
        let values: Vec<f64> = points.iter().map(|&(p, err)| f(p) + err).collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Ok(None);
        }
        let upper = values.into_iter().fold(0.0_f64, f64::max);
        if upper <= found {
            continue;
        }
        let ends = [points.first(), points.last()]
            .into_iter()
            .flatten()
            .map(|&(p, _)| f(p))
            .fold(0.0_f64, f64::max);
        let settled = upper - ends <= tolerance;
        if settled || piece.depth >= MAX_HALVINGS || seen + work.len() >= MAX_PIECES {
            found = found.max(upper);
            continue;
        }
        let (left, right) = split_at(&piece.cp, 0.5);
        let err = piece.err + certified::gamma(3) * size;
        for cp in [left, right] {
            work.push(Piece {
                cp,
                err,
                depth: piece.depth + 1,
            });
        }
    }
    Ok(Some(found))
}

/// A value and its rate along the edge parameter, each enclosed over a set of parameters
/// in rounded interval arithmetic ([`Ivl`]): the forward-mode derivative of every
/// operation, so `d` encloses the rate of the exact value for every parameter `v` holds.
#[derive(Debug, Clone, Copy)]
struct Di {
    v: Ivl,
    d: Ivl,
}

impl Di {
    /// A number that does not move along the edge.
    fn constant(x: f64) -> Self {
        Self {
            v: Ivl::from(x),
            d: Ivl::from(0.0),
        }
    }

    /// Nothing known: the whole line for both.
    const fn whole() -> Self {
        Self {
            v: Ivl::whole(),
            d: Ivl::whole(),
        }
    }

    /// The root of a value certified positive (else nothing is known).
    fn sqrt(self) -> Self {
        if !self.v.is_positive() {
            return Self::whole();
        }
        let v = self.v.sqrt();
        Self {
            v,
            d: self.d / (Ivl::from(2.0) * v),
        }
    }

    /// Whether every value is of one sign, none zero.
    fn excludes_zero(self) -> bool {
        self.v.lo > 0.0 || self.v.hi < 0.0
    }
}

impl core::ops::Add for Di {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self {
            v: self.v + o.v,
            d: self.d + o.d,
        }
    }
}

impl core::ops::Sub for Di {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self {
            v: self.v - o.v,
            d: self.d - o.d,
        }
    }
}

impl core::ops::Mul for Di {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self {
            v: self.v * o.v,
            d: self.d * o.v + self.v * o.d,
        }
    }
}

impl core::ops::Div for Di {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let q = self.v / o.v;
        Self {
            v: q,
            d: (self.d - q * o.d) / o.v,
        }
    }
}

/// A point or vector of [`Di`] coordinates.
#[derive(Debug, Clone, Copy)]
struct Dv {
    x: Di,
    y: Di,
    z: Di,
}

impl Dv {
    fn constant(v: Vec3) -> Self {
        Self {
            x: Di::constant(v.x),
            y: Di::constant(v.y),
            z: Di::constant(v.z),
        }
    }

    /// The point `p` (enclosures of the value and of the rate).
    fn of(value: V3<Ivl>, rate: V3<Ivl>) -> Self {
        Self {
            x: Di {
                v: value.x,
                d: rate.x,
            },
            y: Di {
                v: value.y,
                d: rate.y,
            },
            z: Di {
                v: value.z,
                d: rate.z,
            },
        }
    }

    fn add(self, o: Self) -> Self {
        Self {
            x: self.x + o.x,
            y: self.y + o.y,
            z: self.z + o.z,
        }
    }

    fn sub(self, o: Self) -> Self {
        Self {
            x: self.x - o.x,
            y: self.y - o.y,
            z: self.z - o.z,
        }
    }

    fn scale(self, s: Di) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }

    fn dot(self, o: Self) -> Di {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    fn cross(self, o: Self) -> Self {
        Self {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }
}

/// The trace of a plane in a normal section, or of two planes' line: the foot `x0` of the
/// edge point on it, its squared distance `foot2` from the point, and its direction `u`,
/// unit only within rounding (a point `x0 + τ u` is `|τ| |u|` from the foot, not `|τ|`).
#[derive(Debug, Clone, Copy)]
struct Trace {
    x0: Dv,
    foot2: Di,
    u: Dv,
}

/// The trace of the plane `(o, ν)` in the normal section of the edge at `p`, the plane
/// through `p` across the edge's tangent `t` (the plane of the outward binormal and the
/// face normal): the line along `d = ν × t` whose point nearest `p` is
/// `x0 = p + h (s ν − c t) / |d|²` (`h = (o − p)·ν`, `c = ν·t`, `s = t·t`; unchanged by the
/// scale of `ν` or `t`), at `|x0 − p|² = h² s / |d|²`. `None` where `|d|²` is not certified
/// apart from zero (the section within rounding of parallel to the plane).
fn plane_trace(p: Dv, t: Dv, o: Vec3, nu: Vec3) -> Option<Trace> {
    let nu = Dv::constant(nu);
    let h = Dv::constant(o).sub(p).dot(nu);
    let c = nu.dot(t);
    let s = t.dot(t);
    let d = nu.cross(t);
    let dd = d.dot(d);
    if !dd.excludes_zero() {
        return None;
    }
    let x0 = p.add(nu.scale(s).sub(t.scale(c)).scale(h / dd));
    let length = dd.sqrt();
    Some(Trace {
        x0,
        foot2: h * h * s / dd,
        u: d.scale(Di::constant(1.0) / length),
    })
}

/// The line where the planes `a` and `b` meet, as it passes the vertex `p`: the point
/// `x0 = p + α na + β nb` nearest `p` (`α |na|² + β c = ha`, `α c + β |nb|² = hb`, `c = na·nb`,
/// `h` each plane's height over `p`, the determinant `|na × nb|²`) and the unit direction
/// of `na × nb`. `None` where the planes are within `tangent_sine` of parallel (they meet
/// in no line near the vertex) or the determinant is not certified apart from zero.
fn planes_line(p: Vec3, a: Tangent, b: Tangent, tangent_sine: f64) -> Option<Trace> {
    let pv = Dv::constant(p);
    let (na, nb) = (Dv::constant(a.raw), Dv::constant(b.raw));
    let ha = Dv::constant(a.point).sub(pv).dot(na);
    let hb = Dv::constant(b.point).sub(pv).dot(nb);
    let (aa, bb, c) = (na.dot(na), nb.dot(nb), na.dot(nb));
    let d = na.cross(nb);
    let det = d.dot(d);
    let parallel = det.v.lo <= (tangent_sine * tangent_sine).max(EFF_ZERO) * aa.v.hi * bb.v.hi;
    if parallel || !det.excludes_zero() {
        return None;
    }
    let alpha = (ha * bb - c * hb) / det;
    let beta = (hb * aa - c * ha) / det;
    let off = na.scale(alpha).add(nb.scale(beta));
    Some(Trace {
        x0: pv.add(off),
        foot2: off.dot(off),
        u: d.scale(Di::constant(1.0) / det.sqrt()),
    })
}

/// `F(x0 + τ u) = A τ² + B τ + C` for the quadric's implicit function `F` (a cone's
/// `|P v|² − (r + t z)²`, `v = x − origin`, `z = v·a`, `P` the projection off the axis; a
/// sphere's `|v|² − r²`), with the cone's radius `r0 + rd τ` along the line (a sphere has
/// no nappe: `cone` false) and the discriminant `B² − 4 A C`; `None` for any other kind.
#[derive(Debug, Clone, Copy)]
struct AlongLine {
    a: Di,
    b: Di,
    c: Di,
    r0: Di,
    rd: Di,
    cone: bool,
    disc: Di,
}

impl AlongLine {
    fn new(a: Di, b: Di, c: Di, r0: Di, rd: Di, cone: bool) -> Self {
        let disc = b * b - Di::constant(4.0) * a * c;
        Self {
            a,
            b,
            c,
            r0,
            rd,
            cone,
            disc,
        }
    }

    /// The same over a piece, its discriminant's value enclosed by its mean-value form
    /// about the piece's middle `mid`, `dt` the piece about its middle ([`mean_value`]):
    /// the plain enclosure of `B² − 4 A C` grows with every operation's dependency, so a
    /// piece where the discriminant keeps one sign is not decided on it.
    fn centred(self, mid: &Self, dt: Ivl) -> Self {
        Self {
            disc: Di {
                v: mean_value(self.disc, mid.disc, dt),
                d: self.disc.d,
            },
            ..self
        }
    }

    /// Whether the point at `tau` is certified on the cone's nappe (`Some(true)`), certified
    /// off it (`Some(false)`), or neither; a sphere's every point is on it.
    fn on_nappe(&self, tau: Di) -> Option<bool> {
        if !self.cone {
            return Some(true);
        }
        let radius = self.r0 + self.rd * tau;
        if radius.v.lo >= 0.0 {
            Some(true)
        } else if radius.v.hi < 0.0 {
            Some(false)
        } else {
            None
        }
    }
}

fn along_line(q: &Implicit, line: &Trace) -> Option<AlongLine> {
    let two = Di::constant(2.0);
    match *q {
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let axis = Dv::constant(axis);
            let v = line.x0.sub(Dv::constant(origin));
            let (z0, zd) = (v.dot(axis), line.u.dot(axis));
            let (p0, pd) = (v.sub(axis.scale(z0)), line.u.sub(axis.scale(zd)));
            let t = Di::constant(tan_angle);
            let (r0, rd) = (Di::constant(radius) + t * z0, t * zd);
            Some(AlongLine::new(
                pd.dot(pd) - rd * rd,
                two * (p0.dot(pd) - r0 * rd),
                p0.dot(p0) - r0 * r0,
                r0,
                rd,
                true,
            ))
        }
        Implicit::Sphere { centre, radius } => {
            let v = line.x0.sub(Dv::constant(centre));
            let r = Di::constant(radius);
            Some(AlongLine::new(
                line.u.dot(line.u),
                two * v.dot(line.u),
                v.dot(v) - r * r,
                Di::constant(0.0),
                Di::constant(0.0),
                false,
            ))
        }
        _ => None,
    }
}

/// The distance from points of `x` to the quadric `q`, enclosed: a cone's (a cylinder's)
/// from its generator in the meridian half-plane, `|ρ − (r + t z)| / √(1 + t²)`; a sphere's
/// `||v| − r|`. The whole line for any other kind.
fn quadric_gap(q: &Implicit, x: Dv) -> Ivl {
    let signed = match *q {
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let a = Dv::constant(axis);
            let v = x.sub(Dv::constant(origin));
            let h = v.dot(a);
            let pv = v.sub(a.scale(h));
            let rho = pv.dot(pv).v.sqrt();
            let t = Ivl::from(tan_angle);
            (rho - (Ivl::from(radius) + t * h.v)) / (Ivl::from(1.0) + t * t).sqrt()
        }
        Implicit::Sphere { centre, radius } => {
            let v = x.sub(Dv::constant(centre));
            v.dot(v).v.sqrt() - Ivl::from(radius)
        }
        _ => Ivl::whole(),
    };
    Ivl::new(signed.mig(), signed.mag())
}

/// How a line's meeting with a quadric is taken over a set of edge parameters, decided on
/// the enclosures of the whole set so that it holds for each of them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Plan {
    /// Real roots, from `w = −(B + sign(B) √disc) / 2` (`roots[0] = w / A`, `roots[1] = C / w`:
    /// no cancellation) when `positive` is `Some(sign of B)`, else `(−B ± √disc) / 2A`; the
    /// listed roots are certified on the cone's nappe (`r0 + rd τ ≥ 0`).
    Roots {
        positive: Option<bool>,
        roots: Vec<usize>,
    },
    /// No real root, the line within the B-rep's tolerance of the quadric at `τ = −B / 2A`
    /// (on the nappe): they touch there.
    Touch,
    /// No meeting: no real root and the line farther than the tolerance from the quadric,
    /// or every root on the other nappe.
    Apart,
    /// The roots not decided (the line within rounding of tangent to the quadric, or a root
    /// not certified on the nappe): every root lies within `|−B / 2A| + √disc⁺ / 2|A|` of the
    /// foot, which bounds the meeting, if any.
    Cover,
    /// Not decided on these enclosures (`A` not certified apart from zero).
    Unknown,
}

/// The discriminant `B² − 4 A C`.
const fn discriminant(q: &AlongLine) -> Di {
    q.disc
}

/// The two roots of `plan`'s formula (`None` where its divisor is not certified apart from
/// zero).
fn roots(q: &AlongLine, positive: Option<bool>) -> [Option<Di>; 2] {
    let sq = discriminant(q).sqrt();
    match positive {
        Some(up) => {
            let signed = if up { q.b + sq } else { q.b - sq };
            let w = signed * Di::constant(-0.5);
            [
                q.a.excludes_zero().then(|| w / q.a),
                w.excludes_zero().then(|| q.c / w),
            ]
        }
        None if q.a.excludes_zero() => {
            let two_a = Di::constant(2.0) * q.a;
            let minus_b = Di::constant(0.0) - q.b;
            [Some((minus_b + sq) / two_a), Some((minus_b - sq) / two_a)]
        }
        None => [None, None],
    }
}

/// The plan for a line's meeting with `quadric` over a set of parameters ([`Plan`]).
fn plan_of(line: &Trace, q: &AlongLine, quadric: &Implicit, tolerance: f64) -> Plan {
    let disc = discriminant(q);
    let fallback = if q.a.excludes_zero() {
        Plan::Cover
    } else {
        Plan::Unknown
    };
    if disc.v.lo > 0.0 {
        let positive = if q.b.v.lo > 0.0 {
            Some(true)
        } else if q.b.v.hi < 0.0 {
            Some(false)
        } else {
            None
        };
        let candidates = roots(q, positive);
        let mut valid = Vec::new();
        let mut off_nappe = 0;
        for (i, tau) in candidates.iter().enumerate() {
            match tau.map(|tau| q.on_nappe(tau)) {
                Some(Some(true)) => valid.push(i),
                Some(Some(false)) => off_nappe += 1,
                _ => {}
            }
        }
        return if !valid.is_empty() {
            Plan::Roots {
                positive,
                roots: valid,
            }
        } else if off_nappe == 2 {
            Plan::Apart
        } else {
            fallback
        };
    }
    if disc.v.hi < 0.0 && q.a.excludes_zero() {
        let tau = (Di::constant(0.0) - q.b) / (Di::constant(2.0) * q.a);
        let x = line.x0.add(line.u.scale(tau));
        let gap = quadric_gap(quadric, x);
        let nappe = q.on_nappe(tau);
        if gap.hi <= tolerance && nappe == Some(true) {
            return Plan::Touch;
        }
        if gap.lo > tolerance || nappe == Some(false) {
            return Plan::Apart;
        }
    }
    fallback
}

/// The squared distances from the edge point of the meetings `plan` takes (each a bound on
/// the reach: the meeting taken is the nearest), or `None` where the plan takes none or its
/// formula is not defined on these enclosures. A meeting at `τ` is `foot² + τ² |u|²` from
/// the point: `|u|²` enclosed as `u·u`, since the direction is unit only within rounding,
/// and an upper bound taken as if `|u| = 1` could sit an ulp below the distance.
fn planned(plan: &Plan, line: &Trace, q: &AlongLine) -> Option<Vec<Di>> {
    let unit = line.u.dot(line.u);
    match plan {
        Plan::Roots {
            positive,
            roots: which,
        } => {
            let taus = roots(q, *positive);
            which
                .iter()
                .map(|&i| {
                    let tau = taus.get(i).copied().flatten()?;
                    Some(line.foot2 + tau * tau * unit)
                })
                .collect()
        }
        Plan::Touch => {
            if !q.a.excludes_zero() {
                return None;
            }
            let tau = (Di::constant(0.0) - q.b) / (Di::constant(2.0) * q.a);
            Some(vec![line.foot2 + tau * tau * unit])
        }
        Plan::Cover => {
            if !q.a.excludes_zero() {
                return None;
            }
            let half = (Di::constant(0.0) - q.b) / (Di::constant(2.0) * q.a);
            let disc = discriminant(q).v.hi.max(0.0);
            let spread = Ivl::from(disc).sqrt() / (Ivl::from(2.0) * Ivl::from(q.a.v.mig()));
            let tau = round_up(half.v.mag() + spread.hi);
            if !tau.is_finite() {
                return None;
            }
            // (its rate is not known: the mean-value form falls back to the plain bound)
            let tau = Di {
                v: Ivl::from(tau),
                d: Ivl::whole(),
            };
            Some(vec![line.foot2 + tau * tau * unit])
        }
        Plan::Apart | Plan::Unknown => None,
    }
}

/// What a piece of an edge's parameters gives a bound.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PieceBound {
    /// An upper bound over the piece and a value the bounded quantity takes at its middle
    /// (a lower bound on its largest, which steers the halving only), and whether the two
    /// surfaces are certified to cross in every section of the piece ([`Plan::Roots`]).
    Value {
        upper: f64,
        lower: f64,
        crosses: bool,
    },
    /// No meeting anywhere on the piece.
    Apart,
    /// Not decided on the piece.
    Unknown,
}

/// The mean-value enclosure of `g` over a piece: `g(t) ∈ g(tm) + g'(ξ) (t − tm)`, `g'`
/// enclosed over the piece by `over.d` and `t − tm` by `dt`, met with the plain enclosure.
fn mean_value(over: Di, mid: Di, dt: Ivl) -> Ivl {
    let centred = mid.v + over.d * dt;
    let lo = over.v.lo.max(centred.lo);
    let hi = over.v.hi.min(centred.hi);
    if lo <= hi {
        Ivl::new(lo, hi)
    } else {
        over.v
    }
}

/// The certified upper bound of the reach over a piece where a plane `(o, ν)` and a
/// quadric meet in the edge's normal sections ([`local_extension_reach`]): the nearest root
/// in each section, its squared distance enclosed by its mean-value form over the piece.
fn plane_quadric_piece(
    over: (Dv, Dv),
    mid: (Dv, Dv),
    dt: Ivl,
    plane: Tangent,
    quadric: &Implicit,
    tolerance: f64,
) -> PieceBound {
    let trace = |(p, t): (Dv, Dv)| {
        let line = plane_trace(p, t, plane.point, plane.raw)?;
        let q = along_line(quadric, &line)?;
        Some((line, q))
    };
    let (Some((line, q)), Some((mline, mq))) = (trace(over), trace(mid)) else {
        return PieceBound::Unknown;
    };
    let q = q.centred(&mq, dt);
    let plan = plan_of(&line, &q, quadric, tolerance);
    if plan == Plan::Apart {
        return PieceBound::Apart;
    }
    let (Some(over), Some(mid)) = (planned(&plan, &line, &q), planned(&plan, &mline, &mq)) else {
        return PieceBound::Unknown;
    };
    // the mean-value form of each root the piece's plan takes, centred on the same formula
    // at the middle
    let upper = over
        .into_iter()
        .zip(mid)
        .map(|(g, m)| mean_value(g, m, dt).hi)
        .filter(|hi| hi.is_finite())
        .map(|hi| round_up(sqrt(hi.max(0.0))))
        .reduce(f64::min);
    match upper {
        Some(upper) => PieceBound::Value {
            upper,
            lower: attained(&mline, &mq, quadric, tolerance),
            crosses: matches!(plan, Plan::Roots { .. }),
        },
        None => PieceBound::Unknown,
    }
}

/// The certified upper bound of the reach over a piece where a plane `(o, ν)` and a bare
/// net meet in the edge's normal sections ([`local_extension_reach`]): the plane's trace
/// `x0 + τ u` there, its foot and direction by their mean-value forms over the piece, meets
/// the net (or its extension) at a `τ` proved for every section of the piece
/// ([`net::line_meets`]), `√(|x0 − p|² + τ²)` from the edge point; the same at the middle,
/// from its own box, is the value that steers the halving. `guess` carries Newton's last
/// root from piece to piece.
fn plane_net_piece(
    over: (Dv, Dv),
    mid: (Dv, Dv),
    dt: Ivl,
    plane: Tangent,
    net: &net::Net,
    guess: &std::cell::Cell<Option<[f64; 3]>>,
) -> PieceBound {
    let (Some(line), Some(mline)) = (
        plane_trace(over.0, over.1, plane.point, plane.raw),
        plane_trace(mid.0, mid.1, plane.point, plane.raw),
    ) else {
        return PieceBound::Unknown;
    };
    let centred = |a: Dv, m: Dv| {
        [
            mean_value(a.x, m.x, dt),
            mean_value(a.y, m.y, dt),
            mean_value(a.z, m.z, dt),
        ]
    };
    let numbers = |a: Dv| Vec3::new(a.x.v.mid(), a.y.v.mid(), a.z.v.mid());
    let at_middle = (numbers(mline.x0), numbers(mline.u));
    let values = |a: Dv| [a.x.v, a.y.v, a.z.v];
    // `|x0 + τ u − p|² = |x0 − p|² + τ² |u|²` (`u` across the foot), `|u|` enclosed about 1
    let reach = |foot2: Ivl, tau: Ivl, u: [Ivl; 3]| {
        let along = round_up(tau.mag() * net::length_bound(u));
        round_up(sqrt(round_up(foot2.hi.max(0.0) + round_up(along * along))))
    };
    let piece = (centred(line.x0, mline.x0), centred(line.u, mline.u));
    let Ok(Some(tau)) = net::line_meets(net, piece, at_middle, guess) else {
        return PieceBound::Unknown;
    };
    let upper = reach(mean_value(line.foot2, mline.foot2, dt), tau, piece.1);
    // a meeting at the middle, from its own box: its distance only steers the halving (it
    // need not be the nearest meeting there, nor rounded down)
    let lower = match net::line_meets(net, (values(mline.x0), values(mline.u)), at_middle, guess) {
        Ok(Some(tau)) => sqrt(mline.foot2.v.lo.max(0.0) + tau.mig() * tau.mig()),
        _ => 0.0,
    };
    if upper.is_finite() {
        PieceBound::Value {
            upper,
            lower,
            crosses: false,
        }
    } else {
        PieceBound::Unknown
    }
}

/// Two quadrics met in every normal section of an edge piece, proved by the shared
/// three-equation certificate. The midpoint's root only guides refinement; an unproved
/// section stays unknown and is subdivided, never classified as apart.
fn quadric_pair_piece(
    k: &Kernel,
    over: (Dv, Dv),
    mid: (Dv, Dv),
    dt: Ivl,
    a: &Implicit,
    b: &Implicit,
) -> Result<PieceBound> {
    let (over, mid) = (quadric::Section::edge(over), quadric::Section::edge(mid));
    let Some(meeting) = quadric::meet(k, a, b, over, mid, dt)? else {
        return Ok(PieceBound::Unknown);
    };
    let lower =
        quadric::meet(k, a, b, mid, mid, Ivl::from(0.0))?.map_or(0.0, |m| m.distance.lo.max(0.0));
    Ok(PieceBound::Value {
        upper: meeting.distance.hi,
        lower,
        crosses: meeting.transverse,
    })
}

/// The reach at one edge point (a piece's middle, whose `line` and `q` are point
/// enclosures): the distance of the meeting nearest the point, from the plan decided there
/// ([`plan_of`]), the low end of its enclosure: a value the reach takes, so at most its
/// largest. 0 where the nearest meeting is not decided there: a [`Plan::Cover`] (its bound covers
/// both roots, it is not a meeting) or a root certified neither on nor off the nappe.
///
/// The piece's own plan is not used for this: on a piece the roots may be undecided while
/// at its middle they are not, and the cover's value at the middle (`|−B/2A| + √disc/2|A|`,
/// the far root's distance where the roots are apart) would mark every piece as settled at
/// the first one's bound.
fn attained(line: &Trace, q: &AlongLine, quadric: &Implicit, tolerance: f64) -> f64 {
    let plan = plan_of(line, q, quadric, tolerance);
    // every root of the formula is a meeting the plan takes or certified off the nappe
    let off_nappe = |tau: Option<Di>| tau.and_then(|tau| q.on_nappe(tau)) == Some(false);
    let decided = match &plan {
        Plan::Roots {
            positive,
            roots: listed,
        } => roots(q, *positive)
            .into_iter()
            .enumerate()
            .all(|(i, tau)| listed.contains(&i) || off_nappe(tau)),
        Plan::Touch => true,
        Plan::Apart | Plan::Cover | Plan::Unknown => false,
    };
    if !decided {
        return 0.0;
    }
    planned(&plan, line, q)
        .and_then(|found| {
            found
                .iter()
                .map(|g| g.v.lo)
                .filter(|lo| lo.is_finite())
                .reduce(f64::min)
        })
        .map_or(0.0, |lo| sqrt(lo.max(0.0)))
}

/// The certified upper bound of the distance of a piece of the edge from a quadric (a
/// moved surface's move): the signed `ρ − (r + t z)` (a cone's, scaled by `1 / √(1 + t²)`)
/// or `|v| − r` (a sphere's), by its mean-value form.
fn quadric_distance_piece(over: Dv, mid: Dv, dt: Ivl, quadric: &Implicit) -> PieceBound {
    let signed = |p: Dv| -> Option<(Di, f64)> {
        match *quadric {
            Implicit::Cone {
                origin,
                axis,
                radius,
                tan_angle,
            } => {
                let a = Dv::constant(axis);
                let v = p.sub(Dv::constant(origin));
                let h = v.dot(a);
                let pv = v.sub(a.scale(h));
                let rho = pv.dot(pv).sqrt();
                let t = Di::constant(tan_angle);
                let g = rho - (Di::constant(radius) + t * h);
                Some((g, 1.0 + tan_angle * tan_angle))
            }
            Implicit::Sphere { centre, radius } => {
                let v = p.sub(Dv::constant(centre));
                Some((v.dot(v).sqrt() - Di::constant(radius), 1.0))
            }
            _ => None,
        }
    };
    let (Some((g, scale)), Some((m, _))) = (signed(over), signed(mid)) else {
        return PieceBound::Unknown;
    };
    let enclosed = mean_value(g, m, dt);
    // the scale's root, rounded down: dividing by it rounds the bound up
    let root = Ivl::from(scale).sqrt().lo;
    if !(enclosed.lo.is_finite() && enclosed.hi.is_finite() && root > 0.0) {
        return PieceBound::Unknown;
    }
    PieceBound::Value {
        upper: round_up(enclosed.mag() / root),
        lower: m.v.mig() / root,
        crosses: false,
    }
}

/// The most halvings of an edge's parameter interval in [`edge_interval_maximum`].
const MAX_INTERVAL_HALVINGS: usize = 40;

/// The most parameter intervals of one edge [`edge_interval_maximum`] looks at.
const MAX_INTERVALS: usize = 1 << 14;

/// The edge's curve as NURBS data for [`curve_derivs_over`]: a B-spline, line or conic;
/// `None` for any other kind or one whose data do not agree (or a weight not positive).
fn edge_nurbs(b: &Brep, e: crate::topology::EdgeId) -> Result<Option<NurbsCurve>> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    let line;
    let base = match curve.kind() {
        CurveKind::BSpline(c) => c,
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        CurveKind::Line(c) => {
            line = c.base_with_nurb()?;
            &*line
        }
        _ => return Ok(None),
    };
    match NurbsCurve::of(base) {
        Ok(n) => Ok(Some(n)),
        Err(e @ SolidError::Cancelled { .. }) => Err(e),
        Err(_) => Ok(None),
    }
}

/// The edge point and tangent over the parameters `s`, each with its rate (from the
/// curve's derivatives to order 2 enclosed over `s`, [`curve_derivs_over`]); `None` where
/// no piece of the curve covers `s`.
fn point_and_tangent(cn: &NurbsCurve, s: Ivl) -> Result<Option<(Dv, Dv)>> {
    let ders = match curve_derivs_over(cn, s, 2, None) {
        Ok(d) => d,
        Err(e @ SolidError::Cancelled { .. }) => return Err(e),
        Err(_) => return Ok(None),
    };
    let (Some(&d0), Some(&d1), Some(&d2)) = (ders.first(), ders.get(1), ders.get(2)) else {
        return Ok(None);
    };
    Ok(Some((Dv::of(d0, d1), Dv::of(d1, d2))))
}

/// What an edge's bound in [`local_extension_reach`] is of: a moved face's surface's distance
/// from the edge (the shift), or how far past the edge its two new surfaces meet
/// (the reach), by the faces whose surfaces they are, the pair in id order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EdgeQuantity {
    Shift(Option<FaceId>),
    Meeting(Option<FaceId>, Option<FaceId>),
}

/// A bound [`edge_interval_maximum`] found, with what it was found on, as bits
/// ([`bound_inputs`]).
#[derive(Debug, Clone)]
struct EdgeBound {
    on: Vec<u64>,
    bound: Option<(f64, bool, bool)>,
}

/// Everything an edge's bound is a function of besides the quantity, as bits: the edge's
/// interval, the tolerance, the curve's degree, knots and homogeneous control points (each
/// list after its length), and the bits of the surfaces the quantity is of ([`exact_bits`]).
/// Two asks with the same bits ask the same function of the same numbers.
fn bound_inputs(
    cn: &NurbsCurve,
    interval: crate::geom::extent1d::Extent1d,
    tolerance: f64,
    surfaces: &[u64],
) -> Vec<u64> {
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    let mut on = vec![
        interval.min().to_bits(),
        interval.max().to_bits(),
        tolerance.to_bits(),
        count(cn.p),
        count(cn.n),
        count(cn.u.len()),
    ];
    on.extend(cn.u.iter().map(|x| x.to_bits()));
    on.push(count(cn.pw.len()));
    on.extend(cn.pw.iter().map(|x| x.to_bits()));
    on.push(count(surfaces.len()));
    on.extend_from_slice(surfaces);
    on
}

/// The bits of a surface's exact class an edge's bound is of ([`EdgeBounds`]): a tag for
/// its kind, then its data; `None` for a class no edge's bound by pieces is of (a plane as
/// an [`Implicit`], a torus, a B-spline face), which is then bounded and not recorded.
fn exact_bits(c: &Exact) -> Option<Vec<u64>> {
    let v = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut bits = Vec::new();
    match *c {
        Exact::Plane(t) => {
            bits.push(0);
            bits.extend(v(t.point));
            bits.extend(v(t.normal));
            bits.extend(v(t.raw));
        }
        Exact::Quadric(Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        }) => {
            bits.push(1);
            bits.extend(v(origin));
            bits.extend(v(axis));
            bits.extend([radius.to_bits(), tan_angle.to_bits()]);
        }
        Exact::Quadric(Implicit::Sphere { centre, radius }) => {
            bits.push(2);
            bits.extend(v(centre));
            bits.push(radius.to_bits());
        }
        Exact::Quadric(Implicit::Plane { .. } | Implicit::Torus { .. }) | Exact::Net(_) => {
            return None;
        }
    }
    Some(bits)
}

/// The edge bounds one local operation has found ([`local_extension_reach`]), by edge and
/// quantity: an edge is met from both its faces, and the second takes the first's bound
/// rather than bounding the edge again, the same function of the same curve, so the same
/// bits. Kept by the operation (the offset executive, from its local surfaces being set
/// until its relations are cleared), never across operations.
#[derive(Debug, Clone, Default)]
pub struct EdgeBounds(BTreeMap<(crate::topology::EdgeId, EdgeQuantity), EdgeBound>);

impl EdgeBounds {
    /// Forgets every bound.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
impl EdgeBounds {
    /// How many bounds are held.
    pub(super) fn held(&self) -> usize {
        self.0.len()
    }

    /// Holds a bound for edge `e` (its shift by no face, on no inputs).
    pub(super) fn hold(&mut self, e: crate::topology::EdgeId) {
        let bound = EdgeBound {
            on: Vec::new(),
            bound: None,
        };
        self.0.insert((e, EdgeQuantity::Shift(None)), bound);
    }
}

/// A piece of an edge's parameters `[t0, t1]` with a certified upper bound on it, ordered by
/// that bound (the most is taken first in [`edge_interval_maximum`]); `halvable` false
/// where a halving left a half undecided, so the piece keeps its own bound.
#[derive(Debug, Clone, Copy)]
struct Bounded {
    upper: f64,
    t0: f64,
    t1: f64,
    depth: usize,
    halvable: bool,
}

impl PartialEq for Bounded {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o).is_eq()
    }
}

impl Eq for Bounded {}

impl PartialOrd for Bounded {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Bounded {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.upper.total_cmp(&o.upper)
    }
}

/// The edge's parameter interval cut at the curve's knots, each piece at depth 0: one per
/// polynomial span, as the mean-value form needs the point and its tangent continuous on
/// each piece.
fn spans(cn: &NurbsCurve, interval: crate::geom::extent1d::Extent1d) -> Vec<(f64, f64, usize)> {
    let (lo, hi) = (interval.min(), interval.max());
    let mut cuts: Vec<f64> = cn.u.iter().copied().filter(|&k| lo < k && k < hi).collect();
    cuts.dedup();
    let mut ends = vec![lo];
    ends.extend(cuts);
    ends.push(hi);
    ends.windows(2)
        .filter_map(|w| match w {
            [a, c] if a < c => Some((*a, *c, 0)),
            _ => None,
        })
        .collect()
}

/// A certified upper bound on the largest value over edge `e` of what `f` bounds on each
/// piece of its parameters (given the point and tangent over the piece, at its middle, and
/// the piece about its middle), whether some piece is apart, and whether the surfaces cross
/// on every piece ([`PieceBound::Value`]'s `crosses`; none apart).
///
/// Every piece is decided first: an undecided one is halved until each piece is bounded or
/// apart, at most [`MAX_INTERVAL_HALVINGS`] times; an edge with a piece still undecided
/// there, or past [`MAX_INTERVALS`] pieces looked at, is `None`, as is one with no NURBS
/// curve. Then the bounded piece with the largest bound is halved while that bound exceeds
/// the largest middle value found (a value the quantity takes) by more than `tolerance`,
/// within the same caps; the answer is the largest bound left, which bounds every piece.
/// A halving that leaves a half undecided keeps the piece's own bound. Deciding first, the
/// caps are never spent tightening some pieces' bounds while another piece is still
/// undecided: taken depth first, an edge of several spans (a full circle's four) could
/// spend them on the spans taken first and leave a later one undecided, refusing the whole
/// edge, though every piece was bounded or apart within the caps. Taking the largest bound
/// first spends them where the answer is.
///
/// Each edge is met from both its faces, and the second asks for the same bound: `bounds`
/// holds what the local operation has found ([`EdgeBounds`]), and a bound is taken from it
/// for the same quantity `of` where everything it is a function of has the same bits
/// ([`bound_inputs`]: the curve, interval, tolerance, and `surfaces`, the bits of the
/// surfaces `f` is of). With no `surfaces` the edge is bounded and nothing recorded.
fn edge_interval_maximum(
    b: &Brep,
    e: crate::topology::EdgeId,
    f: &dyn Fn((Dv, Dv), (Dv, Dv), Ivl) -> Result<PieceBound>,
    tolerance: f64,
    bounds: &mut EdgeBounds,
    of: EdgeQuantity,
    surfaces: Option<&[u64]>,
) -> Result<Option<(f64, bool, bool)>> {
    let Some(cn) = edge_nurbs(b, e)? else {
        return Ok(None);
    };
    let interval = edge::interval(b, e)?;
    let Some(surfaces) = surfaces else {
        return interval_maximum(&cn, interval, f, tolerance);
    };
    let on = bound_inputs(&cn, interval, tolerance, surfaces);
    if let Some(found) = bounds.0.get(&(e, of)) {
        if found.on == on {
            return Ok(found.bound);
        }
    }
    let bound = interval_maximum(&cn, interval, f, tolerance)?;
    bounds.0.insert((e, of), EdgeBound { on, bound });
    Ok(bound)
}

/// [`edge_interval_maximum`]'s bound over `interval` of the curve `cn`.
fn interval_maximum(
    cn: &NurbsCurve,
    interval: crate::geom::extent1d::Extent1d,
    f: &dyn Fn((Dv, Dv), (Dv, Dv), Ivl) -> Result<PieceBound>,
    tolerance: f64,
) -> Result<Option<(f64, bool, bool)>> {
    let mut undecided = spans(cn, interval);
    // the bound on a piece; a bound that is not finite bounds nothing
    let bound_of = |t0: f64, t1: f64| -> Result<PieceBound> {
        let tm = 0.5 * (t0 + t1);
        let over = point_and_tangent(cn, Ivl::new(t0, t1))?;
        let mid = point_and_tangent(cn, Ivl::from(tm))?;
        Ok(match (over, mid) {
            (Some(over), Some(mid)) => match f(over, mid, Ivl::checked(t0 - tm, t1 - tm))? {
                PieceBound::Value { upper, .. } if !upper.is_finite() => PieceBound::Unknown,
                bound => bound,
            },
            _ => PieceBound::Unknown,
        })
    };
    let halves = |t0: f64, t1: f64, depth: usize| {
        let tm = 0.5 * (t0 + t1);
        (depth < MAX_INTERVAL_HALVINGS && t0 < tm && tm < t1).then_some(tm)
    };
    let (mut best, mut apart, mut seen) = (0.0_f64, false, 0_usize);
    let mut crosses = true;
    let mut bounded: std::collections::BinaryHeap<Bounded> = std::collections::BinaryHeap::new();
    // every piece decided: bounded or apart
    while let Some((t0, t1, depth)) = undecided.pop() {
        seen += 1;
        match bound_of(t0, t1)? {
            PieceBound::Apart => apart = true,
            PieceBound::Value {
                upper,
                lower,
                crosses: c,
            } => {
                crosses &= c;
                best = best.max(lower);
                bounded.push(Bounded {
                    upper,
                    t0,
                    t1,
                    depth,
                    halvable: true,
                });
            }
            PieceBound::Unknown => {
                let tm =
                    halves(t0, t1, depth).filter(|_| seen + undecided.len() + 2 <= MAX_INTERVALS);
                let Some(tm) = tm else {
                    return Ok(None);
                };
                undecided.push((t0, tm, depth + 1));
                undecided.push((tm, t1, depth + 1));
            }
        }
    }
    // the largest bound tightened: it bounds every piece left
    while let Some(top) = bounded.pop() {
        let tm = halves(top.t0, top.t1, top.depth).filter(|_| seen + 2 <= MAX_INTERVALS);
        let Some(tm) = tm.filter(|_| top.halvable && top.upper > best + tolerance) else {
            return Ok(Some((top.upper, apart, crosses && !apart)));
        };
        seen += 2;
        let mut parts = Vec::with_capacity(2);
        let mut decided = true;
        for (t0, t1) in [(top.t0, tm), (tm, top.t1)] {
            match bound_of(t0, t1)? {
                PieceBound::Apart => apart = true,
                PieceBound::Value { upper, lower, .. } => {
                    best = best.max(lower);
                    parts.push(Bounded {
                        upper,
                        t0,
                        t1,
                        depth: top.depth + 1,
                        halvable: true,
                    });
                }
                PieceBound::Unknown => decided = false,
            }
        }
        if decided {
            bounded.extend(parts);
        } else {
            bounded.push(Bounded {
                halvable: false,
                ..top
            });
        }
    }
    // every piece apart
    Ok(Some((0.0, apart, false)))
}

/// How the line where two planes meet passes a quadric at a corner
/// ([`corner_with_quadric`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum CornerMeeting {
    /// A certified upper bound on the distance from the vertex of the meeting nearest it.
    Within(f64),
    /// No meeting near: the planes parallel, or their line apart from the quadric.
    Nowhere,
    /// Nothing is bounded: the meeting is not decided on the enclosures.
    Uncertified,
}

/// How the line where the planes `a` and `b` meet passes the quadric `q` (the corner of the
/// three surfaces, [`plan_of`]), from the vertex `p`: within a certified distance of it at
/// the meeting nearest it, nowhere near, or not decided ([`CornerMeeting`]).
fn corner_with_quadric(
    p: Vec3,
    a: Tangent,
    b: Tangent,
    q: &Implicit,
    tangent_sine: f64,
    tolerance: f64,
) -> CornerMeeting {
    let Some(line) = planes_line(p, a, b, tangent_sine) else {
        return CornerMeeting::Nowhere;
    };
    let Some(along) = along_line(q, &line) else {
        return CornerMeeting::Uncertified;
    };
    let plan = plan_of(&line, &along, q, tolerance);
    if plan == Plan::Apart {
        return CornerMeeting::Nowhere;
    }
    let upper = planned(&plan, &line, &along)
        .and_then(|found| found.iter().map(|g| g.v.hi).reduce(f64::min))
        .filter(|upper| upper.is_finite());
    match upper {
        Some(upper) => CornerMeeting::Within(round_up(sqrt(upper.max(0.0)))),
        None => CornerMeeting::Uncertified,
    }
}

/// The line a straight edge's curve runs on, from its first control point `a` to its last
/// `b`, with `dev`, a certified upper bound on how far the curve lies from that line.
#[derive(Debug, Clone, Copy)]
struct StraightEdge {
    a: Vec3,
    b: Vec3,
    dev: f64,
}

/// The edges at `vertex` between the faces `f` and `g` (one use on each) whose curves are
/// straight: every control point of the curve's Bézier pieces within the B-rep's
/// `tolerance` of the line through the first and the last ([`StraightEdge`]). The curve
/// lies in its pieces' hulls, so within the largest of those distances, each raised by its
/// point's error and the rounding of the cross product, of the line.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the Bézier extraction.
fn straight_edges_between(
    b: &Brep,
    vertex: crate::topology::VertexId,
    f: FaceId,
    g: FaceId,
    tolerance: f64,
) -> Result<Vec<StraightEdge>> {
    use crate::topology::{vertex as vx, vertexuse as vu};
    let mut seen: Vec<crate::topology::EdgeId> = Vec::new();
    let mut found = Vec::new();
    for use_ in vx::vertexuses(b, vertex)? {
        if !vu::is_edge_vertexuse(b, use_)? {
            continue;
        }
        let Some(edgeuse) = vu::edgeuse(b, use_)? else {
            continue;
        };
        let Some(e) = eu::edge(b, edgeuse)? else {
            continue;
        };
        if seen.contains(&e) {
            continue;
        }
        seen.push(e);
        let this = eu::face(b, edgeuse)?;
        let across = match eu::radial(b, edgeuse)? {
            Some(r) if r != edgeuse => eu::face(b, r)?,
            _ => None,
        };
        let between = matches!((this, across), (Some(x), Some(y))
            if (x == f && y == g) || (x == g && y == f));
        if !between {
            continue;
        }
        let Some(pieces) = edge_pieces(b, e)? else {
            continue;
        };
        // every piece's control points, or none where a weight is not certified positive
        let Some(points) = pieces.iter().map(euclidean).collect::<Option<Vec<_>>>() else {
            continue;
        };
        let points: Vec<(Vec3, f64)> = points.into_iter().flatten().collect();
        let (Some(&(a, _)), Some(&(z, _))) = (points.first(), points.last()) else {
            continue;
        };
        let d = z - a;
        let length = d.length();
        if !certified::above(length, 0.0) {
            continue;
        }
        let mut dev = 0.0_f64;
        for &(c, err) in &points {
            let v = c - a;
            let off = v.cross(d).length() / length;
            let rounding = certified::gamma(8) * (v.length() + off);
            dev = dev.max(off + rounding + err);
        }
        let dev = round_up(dev);
        if dev <= tolerance {
            found.push(StraightEdge { a, b: z, dev });
        }
    }
    Ok(found)
}

/// Where the line of a kept straight edge ([`StraightEdge`]) meets the plane `(o, ν)`,
/// from the vertex `p`: [`line_meets_plane`] on the line from the edge's first control
/// point along its unit direction (unit within rounding).
fn straight_edge_meets_plane(
    p: Vec3,
    line: StraightEdge,
    plane: Tangent,
    tangent_sine: f64,
) -> CornerMeeting {
    let a = Dv::constant(line.a);
    let d = Dv::constant(line.b).sub(a);
    let dd = d.dot(d);
    if !dd.excludes_zero() {
        return CornerMeeting::Uncertified;
    }
    let u = d.scale(Di::constant(1.0) / dd.sqrt());
    line_meets_plane(Dv::constant(p), a, u, line.dev, plane, tangent_sine)
}

/// Where the line `a + s u` meets the plane `(o, ν)`, from `p`: its point `M = x0 + τ u`
/// (`x0 = a + u (u·(p − a))`, the foot of `p` where `|u| = 1`, a point of the line in any
/// case; `τ = (o − x0)·ν / (u·ν)`) at `|M − p|`, enclosed as it is, so it holds whatever
/// `|u|` (the direction is unit only within rounding), raised by the curve's distance `dev`
/// from the line over the sine of their angle, `|u·ν| / (|u| |ν|)`; nowhere near where the
/// line is within `tangent_sine` of parallel to the plane (as two planes' line,
/// [`planes_line`]); not decided where the enclosures do not settle it.
fn line_meets_plane(
    pv: Dv,
    a: Dv,
    u: Dv,
    dev: f64,
    plane: Tangent,
    tangent_sine: f64,
) -> CornerMeeting {
    let x0 = a.add(u.scale(pv.sub(a).dot(u)));
    let nu = Dv::constant(plane.raw);
    let nn = nu.dot(nu).sqrt();
    let un = u.dot(nu);
    if !un.excludes_zero() || !nn.excludes_zero() {
        return CornerMeeting::Nowhere;
    }
    if un.v.mag() <= tangent_sine * nn.v.lo {
        return CornerMeeting::Nowhere;
    }
    let tau = Dv::constant(plane.point).sub(x0).dot(nu) / un;
    let to = x0.add(u.scale(tau)).sub(pv);
    let far = to.dot(to).v.hi.max(0.0);
    let slack = (Ivl::from(dev) * nn.v * u.dot(u).v.sqrt() / Ivl::from(un.v.mig())).hi;
    let reach = round_up(round_up(sqrt(far)) + slack);
    if reach.is_finite() {
        CornerMeeting::Within(reach)
    } else {
        CornerMeeting::Uncertified
    }
}

/// The corner of `three` surfaces at the vertex of `at`, faces `faces`, where two of them
/// are kept (not `is_moved`) and share a straight edge there ([`straight_edges_between`])
/// and the third's new surface is a plane: faces that keep their surfaces keep their edges,
/// so the three new surfaces meet on that edge's line, where it crosses the plane
/// ([`straight_edge_meets_plane`]), whether the two kept surfaces are one (a round drawn
/// as two faces, split along a ruling: their meeting with the plane is no one point) or
/// two that cross along the line. Each such edge is taken, the largest meeting kept;
/// nowhere near where every line runs past the plane. `None` means no kept pair
/// shares a straight edge: a different topology event. `Some(Uncertified)` means
/// such an edge exists but its third surface or meeting is not certified.
///
/// # Errors
///
/// Those of [`straight_edges_between`].
fn kept_straight_edge_corner(
    b: &Brep,
    at: &Corner,
    p: Vec3,
    is_moved: &dyn Fn(Option<FaceId>) -> bool,
    faces: [Option<FaceId>; 3],
    three: [&Exact; 3],
) -> Result<Option<CornerMeeting>> {
    let mut reach: Option<f64> = None;
    let mut any = false;
    let [fa, fc, fd] = faces;
    let [a, c, d] = three;
    // each pair of the three, with the third
    for ((fx, _), (fy, _), (_, z)) in [
        ((fa, a), (fc, c), (fd, d)),
        ((fa, a), (fd, d), (fc, c)),
        ((fc, c), (fd, d), (fa, a)),
    ] {
        let (Some(fx), Some(fy)) = (fx, fy) else {
            continue;
        };
        // a moved face's edge moves with it
        if is_moved(Some(fx)) || is_moved(Some(fy)) {
            continue;
        }
        let lines = straight_edges_between(b, at.vertex, fx, fy, at.tolerance)?;
        if lines.is_empty() {
            continue;
        }
        let Exact::Plane(plane) = *z else {
            return Ok(Some(CornerMeeting::Uncertified));
        };
        for line in lines {
            any = true;
            match straight_edge_meets_plane(p, line, plane, at.tangent_sine) {
                CornerMeeting::Within(r) => reach = Some(reach.map_or(r, |m| m.max(r))),
                CornerMeeting::Nowhere => {}
                CornerMeeting::Uncertified => return Ok(Some(CornerMeeting::Uncertified)),
            }
        }
    }
    Ok(match (any, reach) {
        (false, _) => None,
        (true, Some(r)) => Some(CornerMeeting::Within(r)),
        (true, None) => Some(CornerMeeting::Nowhere),
    })
}

/// How far past its face the surface of `faceuse` must be extended in a local operation
/// for the surfaces of its neighbours to meet its own ([`LocalReach::reach`]), how far the
/// surfaces moved there ([`LocalReach::shift`]), and the edges across which they never
/// meet ([`LocalReach::apart`]) (module docs of [`extension_reach`], with each face's
/// offset replaced by the surface it takes): the faces in `moved` take the surface given
/// with them, every other keeps its own (the copy given with it in `kept`, else the face's
/// surface). Where every surface stays, the reach and the move are zero. `None` for a face
/// with no neighbour (every edge a lamina edge or a seam).
///
/// # A certified bound
///
/// Each is a certified upper bound, found on exact geometry, on control points or in
/// rounded interval arithmetic, never at samples. Where a surface beside an edge moves, the
/// edge's two new surfaces must be of the classes [`exact_class`] names: two quadrics,
/// or a plane and another supported class. The new edge crosses the edge's normal section at each point `p` (the plane
/// through `p` across the edge's tangent: of the outward binormal and the face normal) where
/// the traces of the two new surfaces there meet, the meeting nearest `p`; the reach is the
/// largest distance from `p` of those meetings (a meeting farther than the nearest bounds
/// it too).
///
/// Two planes: how they meet is decided once for the edge from the planes ([`PlanePair`]):
///
/// - on a line `L`: the meeting of each edge point is its foot on `L`, and the reach is the
///   largest distance of an edge point from `L`;
/// - one plane: the largest distance of an edge point from it;
/// - apart (parallel, beyond the B-rep's tolerance): no extension of either makes them
///   meet; the edge adds nothing and is listed in [`LocalReach::apart`] (a step's wall
///   closes such a gap, or the edge's offset fails where it finds no meeting).
///
/// A distance to a plane or a line is convex, so over the edge it is largest at a control
/// point of the edge's Bézier pieces ([`edge_maximum`]).
///
/// A plane and a cylinder, cone or sphere: the plane's trace `x0 + τ u` meets the quadric's
/// where `A τ² + B τ + C = 0` ([`plane_trace`], [`along_line`]), exact at any angle between
/// the surfaces, so in both limits: across a tangent edge it gives the second-order
/// meeting (a round of radius `r` beside a plane moved `δ` into it is met `√(2 r δ)` from
/// the old edge, where tangent planes give nothing). The edge point, its tangent and their
/// rates are enclosed over pieces of the edge's parameters ([`curve_derivs_over`]), the
/// root's squared distance over each piece by its mean-value form ([`plane_quadric_piece`],
/// [`Plan`]), and the pieces halved to the B-rep's tolerance ([`edge_interval_maximum`]).
/// Pieces where the trace passes the quadric beyond the tolerance (a plane moved off a round,
/// which bends away from it) are apart: listed, adding nothing. Where the discriminant,
/// enclosed by its mean-value form, is positive on every piece and a root lies on the nappe
/// ([`Plan::Roots`]), the trace cuts the quadric in every section: the surfaces cross along
/// the edge, even where its faces
/// were tangent in the input, and the edge is listed in [`LocalReach::crossing`].
///
/// Two quadrics: two spheres about one centre, or two cones (cylinders) about one axis at one
/// angle, are parallel surfaces, decided once for the edge from the quadrics as two planes
/// are ([`quadric_pair`]): one surface, met as far as the edge is from it (a quadric's
/// distance, as for the move below); or apart, listed and adding nothing (a piece of a
/// split cylinder or sphere moved off the piece it was split from: a step's wall closes the
/// gap). Any other two: [`quadric_pair_piece`] proves a root of both implicit equations and
/// the normal-section plane, uniformly over each edge piece, with a displacement box
/// certified by Krawczyk's operator ([`quadric::meet`]). Both cones' positive-radius
/// sides are checked on the root box. Only a section Jacobian whose determinant excludes
/// zero marks a crossing. An unproved meeting, including an unresolved tangent or
/// separated pair, remains unknown and is subdivided or refused, never marked apart.
///
/// The move is the largest distance of an edge point from a moved surface beside it,
/// bounded the same way (a plane's on control points, a quadric's by the mean-value form of
/// [`quadric_distance_piece`]). At each vertex of the face where a face moves, the reach is
/// the largest distance from the vertex of the points where the face's surface and two
/// others of the faces at the vertex meet, for every two of them, not only its edge
/// neighbours there: where four or more faces meet and one across the vertex moves, the
/// vertex splits and the face's new edges run to the split points, which are among these
/// meetings. Every pair is taken, so the reach bounds the split points from above. Three
/// planes meet at [`corner_of`]; two planes and a quadric where the planes' line meets it
/// ([`corner_with_quadric`]); where they meet in no one point, the edges' meetings stand.
/// A plane and two quadrics meet by the same three-equation certificate, on the
/// plane at the vertex ([`quadric::meet`]); a plane and two parallel quadrics
/// ([`quadric_pair`]) meet in no one point, and the edges' meetings stand. Where two
/// quadrics are kept and share a straight edge at the vertex (a round drawn as two faces split along a ruling, or two surfaces
/// crossing along a line), and the third's new surface a plane: faces that keep their
/// surfaces keep their edges, so the three meet where that edge's line crosses the plane
/// ([`kept_straight_edge_corner`]). A surface at the vertex moved adds its distance from the
/// vertex to the move.
///
/// A plane and a bare B-spline net ([`net::Net`]: a B-spline surface or extrusion with no
/// label and no definition): the plane's trace `x0 + τ u` meets the net, or the
/// extension the executive builds past its sides, at a `(u, v, τ)` proved by Krawczyk's
/// operator for every section of a piece of the edge's parameters ([`plane_net_piece`],
/// [`net::line_meets`]); the meeting is `√(|x0 − p|² + τ²)` from `p`, `|τ|` its arc length
/// along the plane's trace, and the pieces are halved as above. A moved net's move is at
/// most the reach of an edge beside it, whose meetings lie on it. At a vertex, two planes'
/// line meets a net ([`corner_with_net`]), and a plane meets a quadric and a net, or two
/// nets ([`net::plane_quadric_and_net_meet`], [`net::plane_and_nets_meet`]), each at a
/// point proved the same way.
///
/// Anything else beside an edge or at a vertex where something moves (two curved
/// quadrics without a certified meeting, a quadric or net beside a net, any
/// three faces at the vertex but those above, even three that all stay, a net
/// [`net::Net::of`] turns away, a torus, a blend or sweep, a plane not proved on its label,
/// an edge curve with no NURBS form), a meeting with a net that is not proved, and a piece
/// left undecided past the caps, has no certified bound here:
/// [`OffsetFailure::ReachUncertified`].
///
/// An edge's bound by pieces is taken from `bounds` where the operation already found it from
/// the edge's other face ([`EdgeBounds`]), and recorded there otherwise.
///
/// # Errors
///
/// [`OffsetFailure::ReachUncertified`] as above; [`SolidError::InvalidInput`] for a
/// vanishing plane normal; those of the exact classes' proof ([`implicit::lies_on`]) and
/// of the Bézier extraction; a cancel; [`SolidError::NotFound`] for a dangling id.
#[allow(clippy::too_many_lines)] // the edges' and the corners' bounds
pub fn local_extension_reach(
    k: &Kernel,
    b: &Brep,
    faceuse: FaceuseId,
    moved: &[(FaceId, &Surface)],
    kept: &[(FaceId, &Surface)],
    tangency_tol_radians: f64,
    bounds: &mut EdgeBounds,
) -> Result<Option<LocalReach>> {
    let tangent_sine = tangency_tol_radians.abs().min(crate::math::PI / 2.0).sin();
    let tolerance = b.tolerance();
    let own_face = b.faceuse(faceuse)?.face;
    let is_moved = |f: Option<FaceId>| f.is_some_and(|f| moved.iter().any(|(m, _)| *m == f));
    let uncertified = || match own_face {
        Some(face) => Failure::new(LOCAL_OP, OffsetFailure::ReachUncertified { face }).into_error(),
        None => SolidError::null_pointer(LOCAL_OP),
    };
    // each face's exact class, proved once
    let mut classes: Vec<(FaceId, Option<Exact>)> = Vec::new();
    let mut class_of = |b: &Brep, face: Option<FaceId>| -> Result<Exact> {
        let face = face.ok_or_else(uncertified)?;
        if let Some((_, c)) = classes.iter().find(|(f, _)| *f == face) {
            return c.clone().ok_or_else(uncertified);
        }
        let given = moved
            .iter()
            .chain(kept)
            .find(|(f, _)| *f == face)
            .map(|(_, s)| *s);
        let c = match given {
            Some(s) => exact_class(k, s)?,
            None => match crate::topology::face::p2::surface(b, face)? {
                Some(s) => exact_class(k, s)?,
                None => None,
            },
        };
        classes.push((face, c.clone()));
        c.ok_or_else(uncertified)
    };
    // A face with no neighbour (every edge a seam or a lamina edge) meets no other surface:
    // nothing is bounded for it, whatever its class, so it is decided before any bound is
    // asked (a torus offset whole has no certified class, and needs none).
    let mut met = false;
    for lu in eu::faceuse_loopuses(b, faceuse, false, None)? {
        for edgeuse in loopuse::edgeuses(b, lu, None, None)? {
            met |= match eu::radial(b, edgeuse)? {
                Some(r) => r != edgeuse && eu::face(b, r)? != own_face,
                None => false,
            };
        }
    }
    if !met {
        return Ok(None);
    }
    let own_moved = is_moved(own_face);
    let mut found = LocalReach::default();
    for lu in eu::faceuse_loopuses(b, faceuse, false, None)? {
        for edgeuse in loopuse::edgeuses(b, lu, None, None)? {
            let Some(e) = eu::edge(b, edgeuse)? else {
                continue;
            };
            // the corner at the edge use's start (the loop closes: every vertex once)
            let start = match eu::vertexuse(b, edgeuse)? {
                Some(vu) => crate::topology::vertexuse::vertex(b, vu)?,
                None => None,
            };
            if let Some(vertex) = start {
                let at = Corner {
                    vertex,
                    own: own_face,
                    tangent_sine,
                    tolerance,
                };
                corner(
                    k,
                    b,
                    &at,
                    &is_moved,
                    &mut class_of,
                    &uncertified,
                    &mut found,
                )?;
            }
            let radial = match eu::radial(b, edgeuse)? {
                Some(r) if r != edgeuse && eu::face(b, r)? != own_face => Some(r),
                _ => None,
            };
            let other_face = match radial {
                Some(r) => eu::face(b, r)?,
                None => None,
            };
            let other_moved = radial.is_some() && is_moved(other_face);
            // where both surfaces stay they meet at the edge: nothing to reach
            if !(own_moved || other_moved) {
                continue;
            }
            let own = class_of(b, own_face)?;
            let hull = |f: &dyn Fn(Vec3) -> f64| -> Result<f64> {
                edge_maximum(b, e, f, tolerance)?.ok_or_else(uncertified)
            };
            let pieces = |f: &dyn Fn((Dv, Dv), (Dv, Dv), Ivl) -> Result<PieceBound>,
                          bounds: &mut EdgeBounds,
                          of: EdgeQuantity,
                          surfaces: Option<&[u64]>|
             -> Result<(f64, bool, bool)> {
                let found = edge_interval_maximum(b, e, f, tolerance, bounds, of, surfaces)?;
                k.check_cancelled(LOCAL_OP)?;
                found.ok_or_else(uncertified)
            };
            // a moved net's move is bounded by the meetings on it, below
            let shift_of =
                |c: &Exact, face: Option<FaceId>, bounds: &mut EdgeBounds| -> Result<Option<f64>> {
                    match c {
                        Exact::Plane(t) => hull(&|x| plane_distance(x, *t)).map(Some),
                        Exact::Quadric(q) => {
                            let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
                                Ok(quadric_distance_piece(over.0, mid.0, dt, q))
                            };
                            let surfaces = exact_bits(c);
                            let of = EdgeQuantity::Shift(face);
                            Ok(Some(pieces(&piece, bounds, of, surfaces.as_deref())?.0))
                        }
                        Exact::Net(_) => Ok(None),
                    }
                };
            let mut net_moved = false;
            if own_moved {
                match shift_of(&own, own_face, bounds)? {
                    Some(shift) => found.shift = found.shift.max(shift),
                    None => net_moved = true,
                }
            }
            if radial.is_none() {
                if net_moved {
                    return Err(uncertified());
                }
                continue;
            }
            let other = class_of(b, other_face)?;
            if other_moved {
                match shift_of(&other, other_face, bounds)? {
                    Some(shift) => found.shift = found.shift.max(shift),
                    None => net_moved = true,
                }
            }
            let (reach, apart, crosses) = match (&own, &other) {
                (&Exact::Plane(a), &Exact::Plane(c)) => {
                    match plane_pair(a, c, tangent_sine, tolerance) {
                        PlanePair::Line => (
                            hull(&|x| line_distance(x, a, c, tangent_sine, tolerance))?,
                            false,
                            false,
                        ),
                        PlanePair::One => (hull(&|x| plane_distance(x, a))?, false, false),
                        PlanePair::Apart => (0.0, true, false),
                    }
                }
                (&Exact::Plane(plane), Exact::Quadric(q))
                | (Exact::Quadric(q), &Exact::Plane(plane)) => {
                    let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
                        Ok(plane_quadric_piece(over, mid, dt, plane, q, tolerance))
                    };
                    let pair =
                        EdgeQuantity::Meeting(own_face.min(other_face), own_face.max(other_face));
                    // the plane's, then the quadric's: the same from either face
                    let surfaces = exact_bits(&Exact::Plane(plane))
                        .zip(exact_bits(&Exact::Quadric(*q)))
                        .map(|(mut a, c)| {
                            a.extend(c);
                            a
                        });
                    pieces(&piece, bounds, pair, surfaces.as_deref())?
                }
                (&Exact::Plane(plane), Exact::Net(n)) | (Exact::Net(n), &Exact::Plane(plane)) => {
                    let guess = std::cell::Cell::new(None);
                    let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
                        Ok(plane_net_piece(over, mid, dt, plane, n, &guess))
                    };
                    // a net's bound is not recorded: it carries no bits to key it on
                    let pair =
                        EdgeQuantity::Meeting(own_face.min(other_face), own_face.max(other_face));
                    pieces(&piece, bounds, pair, None)?
                }
                (Exact::Quadric(a), Exact::Quadric(c)) => {
                    let pair =
                        EdgeQuantity::Meeting(own_face.min(other_face), own_face.max(other_face));
                    let (first, second) = if own_face <= other_face {
                        (&own, &other)
                    } else {
                        (&other, &own)
                    };
                    let surfaces = exact_bits(first).zip(exact_bits(second)).map(|(mut a, c)| {
                        a.extend(c);
                        a
                    });
                    // decided on the faces in id order: the same from either face
                    let (q, r) = if own_face <= other_face {
                        (a, c)
                    } else {
                        (c, a)
                    };
                    match quadric_pair(q, r, tolerance) {
                        Some(PlanePair::Apart) => (0.0, true, false),
                        // one surface: met where it stands, as far as it is from the edge
                        Some(PlanePair::One) => {
                            let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
                                Ok(quadric_distance_piece(over.0, mid.0, dt, q))
                            };
                            pieces(&piece, bounds, pair, surfaces.as_deref())?
                        }
                        _ => {
                            let piece = |over, mid, dt| quadric_pair_piece(k, over, mid, dt, a, c);
                            pieces(&piece, bounds, pair, surfaces.as_deref())?
                        }
                    }
                }
                (Exact::Quadric(_) | Exact::Net(_), Exact::Quadric(_) | Exact::Net(_)) => {
                    return Err(uncertified())
                }
            };
            if apart && !found.apart.contains(&e) {
                found.apart.push(e);
            }
            if crosses && !found.crossing.contains(&e) {
                found.crossing.push(e);
            }
            found.reach = found.reach.max(reach);
            // each meeting lies on the moved net: no farther from the edge point than it
            if net_moved {
                found.shift = found.shift.max(reach);
            }
        }
    }
    Ok(Some(found))
}

/// A vertex of a face whose corner [`corner`] bounds, with the tolerances it decides by.
#[derive(Debug, Clone, Copy)]
struct Corner {
    vertex: crate::topology::VertexId,
    own: Option<FaceId>,
    tangent_sine: f64,
    tolerance: f64,
}

/// The reach and the move at a vertex of the face ([`local_extension_reach`]): where a face
/// there moves, every face there must be of a class (`class_of`), and the meetings of the
/// face's surface with every two others (three planes; two planes and a quadric or a net; a
/// plane, a quadric and a net; a plane and two nets; a plane and two quadrics, on a kept
/// straight edge where present, nowhere for parallel quadrics, otherwise by [`quadric::meet`];
/// anything else, or a meeting with a net not proved, is `refuse`d), and each moved
/// surface's distance from the vertex (a moved net's: the farthest meeting found on it
/// there), are taken into `found`.
fn corner(
    k: &Kernel,
    b: &Brep,
    at: &Corner,
    is_moved: &dyn Fn(Option<FaceId>) -> bool,
    class_of: &mut dyn FnMut(&Brep, Option<FaceId>) -> Result<Exact>,
    refuse: &dyn Fn() -> SolidError,
    found: &mut LocalReach,
) -> Result<()> {
    let around = faces_around(b, at.vertex, at.own)?;
    // a corner of faces that all stay stays: it adds nothing
    if !(is_moved(at.own) || around.iter().any(|&f| is_moved(Some(f)))) {
        return Ok(());
    }
    let p = crate::topology::vertex::point(b, at.vertex)?;
    let a = class_of(b, at.own)?;
    let mut others = Vec::new();
    // a moved net's move at the vertex is bounded by the meetings on it, below
    let mut moved_nets = Vec::new();
    if is_moved(at.own) && matches!(a, Exact::Net(_)) {
        moved_nets.push(None);
    }
    for &f in &around {
        let c = class_of(b, Some(f))?;
        if is_moved(Some(f)) {
            match point_distance(p, &c) {
                Some(shift) => found.shift = found.shift.max(shift),
                None => moved_nets.push(Some(f)),
            }
        }
        others.push((f, c));
    }
    // the largest meeting on each moved net, `None` until one is found
    let mut on_nets: Vec<Option<f64>> = vec![None; moved_nets.len()];
    for (i, (fc, c)) in others.iter().enumerate() {
        for (fd, d) in others.iter().skip(i + 1) {
            let three = [&a, c, d];
            let planes: Vec<Tangent> = three
                .iter()
                .filter_map(|x| match x {
                    Exact::Plane(t) => Some(*t),
                    _ => None,
                })
                .collect();
            let quadrics: Vec<Implicit> = three
                .iter()
                .filter_map(|x| match x {
                    Exact::Quadric(q) => Some(*q),
                    _ => None,
                })
                .collect();
            let nets: Vec<&net::Net> = three
                .iter()
                .filter_map(|x| match x {
                    Exact::Net(n) => Some(&**n),
                    _ => None,
                })
                .collect();
            let reach = match (planes.as_slice(), quadrics.as_slice(), nets.as_slice()) {
                (&[x, y, z], [], []) => corner_of(x, y, z, at.tangent_sine)
                    .map(|meeting| corner_distance(p, meeting, [x, y, z])),
                (&[x, y], [q], []) => {
                    let meeting = corner_with_quadric(p, x, y, q, at.tangent_sine, at.tolerance);
                    match meeting {
                        CornerMeeting::Within(reach) => Some(reach),
                        CornerMeeting::Nowhere => None,
                        CornerMeeting::Uncertified => return Err(refuse()),
                    }
                }
                (&[x, y], [], [n]) => {
                    match corner_with_net(p, x, y, n, at.tangent_sine, at.tolerance)? {
                        CornerMeeting::Within(reach) => Some(reach),
                        CornerMeeting::Nowhere => None,
                        CornerMeeting::Uncertified => return Err(refuse()),
                    }
                }
                (&[x], [q], [n]) => {
                    let meeting = net::plane_quadric_and_net_meet(n, q, x.point, x.raw, p)?;
                    Some(meeting.ok_or_else(refuse)?)
                }
                (&[x], [], [m, n]) => {
                    let meeting = net::plane_and_nets_meet(m, n, x.point, x.raw, p)?;
                    Some(meeting.ok_or_else(refuse)?)
                }
                // two or more quadrics: the kept straight edge's meeting, if any
                (_, [_, _, ..], []) => {
                    let faces = [at.own, Some(*fc), Some(*fd)];
                    match kept_straight_edge_corner(b, at, p, is_moved, faces, three)? {
                        Some(CornerMeeting::Within(reach)) => Some(reach),
                        Some(CornerMeeting::Nowhere) => None,
                        Some(CornerMeeting::Uncertified) => return Err(refuse()),
                        None => match (planes.as_slice(), quadrics.as_slice()) {
                            // parallel quadrics and a plane meet in no one point, as three
                            // planes two of which are parallel: the edges' meetings stand
                            // (parallel taken either way round: the same from every face)
                            (&[_], [a, c])
                                if quadric_pair(a, c, at.tolerance).is_some()
                                    && quadric_pair(c, a, at.tolerance).is_some() =>
                            {
                                None
                            }
                            (&[plane], [a, c]) => {
                                let section = quadric::Section::corner(p, plane.point, plane.raw);
                                let meeting =
                                    quadric::meet(k, a, c, section, section, Ivl::from(0.0))?;
                                Some(meeting.ok_or_else(refuse)?.distance.hi)
                            }
                            _ => return Err(refuse()),
                        },
                    }
                }
                _ => return Err(refuse()),
            };
            if let Some(reach) = reach {
                found.reach = found.reach.max(reach);
                for (on, which) in on_nets.iter_mut().zip(&moved_nets) {
                    // the own face is in every triple
                    if which.is_none_or(|w| w == *fc || w == *fd) {
                        *on = Some(on.map_or(reach, |r: f64| r.max(reach)));
                    }
                }
            }
        }
    }
    // a moved net at the vertex with no meeting found on it has no bounded move there
    for on in on_nets {
        found.shift = found.shift.max(on.ok_or_else(refuse)?);
    }
    Ok(())
}

/// Where the planes `a` and `b` and the net `n` meet at a corner, from the vertex `p`: within
/// a certified distance of it, nowhere near (the planes parallel), or not proved
/// ([`CornerMeeting`]). Where a boundary row of the net lies on one of the planes and passes
/// the vertex (each within the B-rep's `tolerance`, [`net::Net::rows_through`]), that row is
/// where the plane and the net
/// meet, to that tolerance (as two planes within it are one, [`PlanePair::One`]), and the
/// corner is where it crosses the other plane ([`net::Net::row_meets`]): the
/// planes' line may run along the net there (a plane tangent to the net along the row,
/// which the net's extension past the row continues). Else the planes' line meets the net
/// ([`net::line_meets`]).
///
/// # Errors
///
/// A cancel.
fn corner_with_net(
    p: Vec3,
    a: Tangent,
    b: Tangent,
    n: &net::Net,
    tangent_sine: f64,
    tolerance: f64,
) -> Result<CornerMeeting> {
    let mut through = n
        .rows_through(a, tolerance, p)?
        .into_iter()
        .map(|side| (side, b))
        .collect::<Vec<_>>();
    through.extend(
        n.rows_through(b, tolerance, p)?
            .into_iter()
            .map(|side| (side, a)),
    );
    if let Some(&(side, other)) = through.first() {
        return Ok(match n.row_meets(side, other.point, other.raw, p)? {
            Some(reach) if reach.is_finite() => CornerMeeting::Within(reach),
            _ => CornerMeeting::Uncertified,
        });
    }
    let Some(line) = planes_line(p, a, b, tangent_sine) else {
        return Ok(CornerMeeting::Nowhere);
    };
    let values = |d: Dv| [d.x.v, d.y.v, d.z.v];
    let numbers = |d: Dv| Vec3::new(d.x.v.mid(), d.y.v.mid(), d.z.v.mid());
    let guess = std::cell::Cell::new(None);
    let at = (numbers(line.x0), numbers(line.u));
    Ok(
        match net::line_meets(n, (values(line.x0), values(line.u)), at, &guess)? {
            Some(tau) => {
                // `|u|` enclosed about 1, as on an edge ([`plane_net_piece`])
                let along = round_up(tau.mag() * net::length_bound(values(line.u)));
                let reach = round_up(sqrt(round_up(
                    line.foot2.v.hi.max(0.0) + round_up(along * along),
                )));
                if reach.is_finite() {
                    CornerMeeting::Within(reach)
                } else {
                    CornerMeeting::Uncertified
                }
            }
            None => CornerMeeting::Uncertified,
        },
    )
}

/// What [`local_extension_reach`] finds for a face: certified upper bounds on how far its
/// surface must reach past it and how far the surfaces beside it moved, and the edges
/// across which its new surface and its neighbour's never meet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalReach {
    /// How far past the face its neighbours' new surfaces meet its own.
    pub reach: f64,
    /// How far the surfaces beside the face moved from its edges and vertices.
    pub shift: f64,
    /// The edges whose two new surfaces never meet across them, along the whole edge or a
    /// piece of it: parallel planes or parallel quadrics apart, or a plane whose trace passes
    /// a quadric beyond the tolerance (a plane moved off a round). No extension makes them
    /// meet there, and they add nothing to the reach.
    pub apart: Vec<crate::topology::EdgeId>,
    /// The edges where a plane and a quadric, the two new surfaces, are certified to cross
    /// in every normal section ([`Plan::Roots`] on every piece): they meet at an angle, even
    /// where the faces were tangent in the input.
    pub crossing: Vec<crate::topology::EdgeId>,
}

/// Every face at `vertex` other than `own` (each once): its edge neighbours at the vertex
/// and, where four or more faces meet, those across the vertex from it, which share no
/// edge with it.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
fn faces_around(
    b: &Brep,
    vertex: crate::topology::VertexId,
    own: Option<FaceId>,
) -> Result<Vec<FaceId>> {
    use crate::topology::{vertex as vx, vertexuse as vu};
    let mut around: Vec<FaceId> = Vec::new();
    for use_ in vx::vertexuses(b, vertex)? {
        if !vu::is_edge_vertexuse(b, use_)? {
            continue;
        }
        let Some(edgeuse) = vu::edgeuse(b, use_)? else {
            continue;
        };
        let Some(f) = eu::face(b, edgeuse)?.filter(|f| Some(*f) != own) else {
            continue;
        };
        if !around.contains(&f) {
            around.push(f);
        }
    }
    Ok(around)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Point3;
    use crate::surface::surface::SurfaceKind;
    use crate::topology::brep::p1 as bp1;
    use crate::topology::EdgeId;

    /// The basin's ruled side: rows 46 long at its rim and 20 at its floor.
    #[test]
    fn a_narrowing_side_is_short_of_its_distance_where_it_narrows() {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{
            BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
        };
        use crate::surface::extend::Continuity;
        let k = Kernel::default();
        let base = Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 1,
                v_degree: 1,
                control_points: &[
                    Point3::new(20.0, 4.0, 30.0),
                    Point3::new(20.0, 50.0, 30.0),
                    Point3::new(30.0, 20.0, 10.0),
                    Point3::new(30.0, 40.0, 10.0),
                ],
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[2, 2],
                v_multiplicities: &[2, 2],
                u_knots: &[0.0, 1.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                uv_domain: None,
            })
            .unwrap(),
        );
        let made = base
            .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, 11.24)
            .unwrap();
        // the rows are 33 at the side's middle, where the extension meets its distance, and
        // 20 at the floor, so the v sides reach 11.24 * 20 / 33 at the floor's end
        let shortest = shortest_across(&k, &base, &made).unwrap().unwrap();
        assert!((shortest - 11.24 * 20.0 / 33.0).abs() <= 1e-6, "{shortest}");
    }

    fn box_faceuses(k: &mut Kernel, inset: bool) -> (Brep, Vec<(FaceId, FaceuseId)>) {
        let brep = crate::primitive::api_primitives::create_box(
            k,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            2.0,
            3.0,
        )
        .unwrap();
        let faceuses = bp1::faces(&brep, None)
            .unwrap()
            .into_iter()
            .map(|f| {
                let fu = super::super::p2::select_offset_faceuse(&brep, f, false, inset, OP);
                (f, fu.unwrap())
            })
            .collect();
        (brep, faceuses)
    }

    /// Across a box's edges (sector angle a right angle) the offsets meet `d` past each
    /// edge, and at its corners `d` past both: `d·√2` from the vertex. Offset inward they meet
    /// as far inside: the same distance from the face.
    #[test]
    fn a_box_offset_either_way_reaches_its_corners_offset() {
        for inset in [false, true] {
            let mut k = Kernel::default();
            let (mut brep, faceuses) = box_faceuses(&mut k, inset);
            for (_, fu) in faceuses {
                let reach = extension_reach(&k, &mut brep, fu, 0.25, &[], 1e-3)
                    .unwrap()
                    .unwrap();
                assert!((reach - 0.25 * sqrt(2.0)).abs() < 1e-12, "{reach}");
            }
        }
    }

    /// A box shelled open at one face: that face stays where it is, and each wall's offset
    /// meets it at the wall's edge (a right angle, `β = 0`). A wall's corner on the open face
    /// lies `d` from its vertex, and its other corners `d·√2`.
    #[test]
    fn an_unmoved_face_is_met_where_it_stands() {
        let mut k = Kernel::default();
        let (mut brep, faceuses) = box_faceuses(&mut k, true);
        let (open, _) = faceuses[0];
        for &(_, fu) in faceuses.iter().filter(|(f, _)| *f != open) {
            let reach = extension_reach(&k, &mut brep, fu, 0.25, &[open], 1e-3)
                .unwrap()
                .unwrap();
            assert!((reach - 0.25 * sqrt(2.0)).abs() < 1e-12, "{reach}");
        }
    }

    /// The signed reach: `d·cot(α/2)` for equal offsets (a right angle `d`, a tangent edge
    /// `0`), `0` for a knife edge (no meeting); an unmoved neighbour at a right angle is met at
    /// the edge, at 60° between normals `dF·c·sin / (1 − c²)` inside.
    #[test]
    fn signed_reach_is_the_foot_of_the_offsets_meeting() {
        let up = Vec3::new(0.0, 0.0, 1.0);
        let out = Vec3::X;
        assert!((signed_reach(up, Vec3::X, out, 0.5, 0.5, 1e-3) - 0.5).abs() < 1e-15);
        assert!((signed_reach(up, -Vec3::X, out, 0.5, 0.5, 1e-3) + 0.5).abs() < 1e-15);
        assert_eq!(signed_reach(up, up, out, 0.5, 0.5, 1e-3), 0.0);
        assert_eq!(signed_reach(up, -up, out, 0.5, 0.5, 1e-3), 0.0);
        assert_eq!(signed_reach(up, up, out, 0.5, 0.0, 1e-3), 0.0);
        assert!(signed_reach(up, Vec3::X, out, 0.5, 0.0, 1e-3).abs() < 1e-15);
        let half = sqrt(3.0) / 2.0;
        let tilted = Vec3::new(half, 0.0, 0.5);
        let expected = -0.5 * 0.5 * half / (1.0 - 0.25);
        assert!((signed_reach(up, tilted, out, 0.5, 0.0, 1e-3) - expected).abs() < 1e-15);
        // within the tangency tolerance of tangent, offsets 0.5 apart never meet near
        let nearly = Vec3::new(1e-4, 0.0, (1.0 - 1e-8_f64).sqrt());
        assert_eq!(signed_reach(up, nearly, out, 0.5, 0.0, 1e-3), 0.0);
        assert!(signed_reach(up, nearly, out, 0.5, 0.0, 1e-5).abs() > 1e3);
    }

    /// The corner's foot: perpendicular edges each reached `1` and `2` past put the corner
    /// at `(1, 2)`; edges at 60° reached `d` past both put it on the bisector, `d / cos 30°`
    /// from the vertex, as reached `d` inside; parallel edges, or edges within the tangency
    /// tolerance of tangent, take the larger reach.
    #[test]
    fn corner_reach_is_the_foot_of_the_offset_corner() {
        let end = |reach: f64, outward: Vec3| End { reach, outward };
        let square = corner_reach(end(1.0, Vec3::X), end(2.0, Vec3::Y), 1e-3);
        assert!((square - sqrt(5.0)).abs() < 1e-12);
        let half = sqrt(3.0) / 2.0;
        let sixty = Vec3::new(0.5, half, 0.0);
        let acute = corner_reach(end(1.0, Vec3::X), end(1.0, sixty), 1e-3);
        assert!((acute - 1.0 / half).abs() < 1e-12, "{acute}");
        let inside = corner_reach(end(-1.0, Vec3::X), end(-1.0, sixty), 1e-3);
        assert!((inside - 1.0 / half).abs() < 1e-12, "{inside}");
        let parallel = corner_reach(end(1.0, Vec3::X), end(-3.0, Vec3::X), 1e-3);
        assert_eq!(parallel, 3.0);
        // edges 1e-4 off tangent, within a tolerance of 1e-3: no corner, not 1e4 off
        let nearly = Vec3::new((1.0 - 1e-8_f64).sqrt(), 1e-4, 0.0);
        let tangent = corner_reach(end(1.0, Vec3::X), end(2.0, nearly), 1e-3);
        assert_eq!(tangent, 2.0);
    }

    /// A moved input is refused: an offset that is not finite.
    #[test]
    fn a_non_finite_offset_is_refused() {
        let mut k = Kernel::default();
        let (mut brep, faceuses) = box_faceuses(&mut k, false);
        assert!(extension_reach(&k, &mut brep, faceuses[0].1, f64::NAN, &[], 1e-3).is_err());
    }

    /// The box's face whose plane has unit normal `n` (outward), with its upward face use.
    fn box_face(brep: &Brep, n: Vec3) -> (FaceId, FaceuseId) {
        for f in bp1::faces(brep, None).unwrap() {
            let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
            let SurfaceKind::Plane(plane) = surface.kind() else {
                continue;
            };
            let z = plane.position().z_axis();
            let fu = super::super::p2::select_offset_faceuse(brep, f, false, false, OP).unwrap();
            let up = brep.faceuse(fu).unwrap().orientation != OrientType::Opposite;
            let outward = if up { z } else { -z };
            if (outward - n).length() < 1e-12 {
                return (f, fu);
            }
        }
        panic!("no face of normal {n:?}");
    }

    /// The box [0, 1] x [0, 2] x [0, 3] with its top (z = 3) moved to `top`: each face's
    /// (reach, move).
    fn moved_top(top: &Surface) -> [(f64, f64); 4] {
        let k = Kernel::default();
        let mut kk = Kernel::default();
        let (brep, _) = box_faceuses(&mut kk, false);
        let (top_face, top_fu) = box_face(&brep, Vec3::Z);
        let moved = [(top_face, top)];
        let at = |n: Vec3| {
            let fu = if n == Vec3::Z {
                top_fu
            } else {
                box_face(&brep, n).1
            };
            let found =
                local_extension_reach(&k, &brep, fu, &moved, &[], 1e-3, &mut EdgeBounds::default())
                    .unwrap()
                    .unwrap();
            (found.reach, found.shift)
        };
        [at(Vec3::Z), at(-Vec3::Y), at(-Vec3::X), at(-Vec3::Z)]
    }

    /// The top raised by `h`: the sides meet it `h` above their top edges (and corners), the
    /// top meets the sides `h` above its old edges, both moved `h`; the bottom meets
    /// sides that stay: nothing to reach, nothing moved.
    #[test]
    fn a_raised_top_is_met_as_far_as_it_rose() {
        let h = 0.5;
        let top = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(0.0, 0.0, 3.0 + h),
            Vec3::Z,
        ));
        let [top, front, left, bottom] = moved_top(&top);
        for (reach, shift) in [top, front, left] {
            assert!((reach - h).abs() < 1e-12, "{reach}");
            assert!((shift - h).abs() < 1e-12, "{shift}");
        }
        assert_eq!(bottom, (0.0, 0.0));
    }

    /// The top tilted by `θ` about the line `y = 1, z = 3`: `z = 3 + (y - 1) tan θ`. The
    /// front (y = 0) meets it `tan θ` below its top edge, and moves `sin θ` there (the
    /// edge's distance from the tilted plane); the left side (x = 0) meets it at its
    /// corners `tan θ` off (along its top edge only `|y - 1| sin θ`); the top's own meetings
    /// are `tan θ` from its old edges at the front and back.
    #[test]
    fn a_tilted_top_is_met_by_its_tangent_at_the_corners() {
        let theta: f64 = 0.3;
        let (s, c) = (theta.sin(), theta.cos());
        let top = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(0.5, 1.0, 3.0),
            Vec3::new(0.0, -s, c),
        ));
        let [top, front, left, bottom] = moved_top(&top);
        let t = theta.tan();
        for (name, (reach, shift)) in [("top", top), ("front", front), ("left", left)] {
            assert!((reach - t).abs() < 1e-12, "{name} reach {reach}");
            assert!((shift - s).abs() < 1e-12, "{name} move {shift}");
        }
        assert_eq!(bottom, (0.0, 0.0));
    }

    /// The pyramid on the square [-1, 1]² with its apex at (0, 0, 1), its side y = -1..0
    /// (`z = 1 + y`) moved in by `δ` along its normal: `z = 1 + y - a`, `a = δ √2`. Four
    /// sides meet at the apex, and the side across it (`z = 1 - y`) shares no edge with the
    /// moved one: its edge neighbours (`z = 1 ∓ x`) and the base stay. The moved side cuts
    /// the apex off, and the side across meets it on a new edge from
    /// (±a/2, a/2, 1 - a/2), `a √3 / 2 = δ √(3/2)` from the apex; the move there is `δ`
    /// (the apex's distance from the moved plane). With only its edge neighbours read, the
    /// side across had neither.
    #[test]
    fn a_side_across_a_four_face_vertex_from_a_moved_one_reaches_its_split() {
        let k = Kernel::default();
        let mut kk = Kernel::default();
        let brep =
            crate::primitive::api_primitives::create_pyramid(&mut kk, Point3::ZERO, 2.0, 1.0)
                .unwrap();
        let s = sqrt(0.5);
        let delta = 0.1;
        let (moved_face, _) = box_face(&brep, Vec3::new(0.0, -s, s));
        let (_, across) = box_face(&brep, Vec3::new(0.0, s, s));
        let n = Vec3::new(0.0, -s, s);
        let plane = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(0.0, 0.0, 1.0) - n * delta,
            n,
        ));
        let moved = [(moved_face, &plane)];
        let found = local_extension_reach(
            &k,
            &brep,
            across,
            &moved,
            &[],
            1e-3,
            &mut EdgeBounds::default(),
        )
        .unwrap()
        .unwrap();
        let (reach, shift) = (found.reach, found.shift);
        assert!((reach - delta * sqrt(1.5)).abs() < 1e-12, "{reach}");
        assert!((shift - delta).abs() < 1e-12, "{shift}");
    }

    /// Nothing moved: every face meets its neighbours at its edges.
    #[test]
    fn a_body_nothing_moves_in_needs_no_extension() {
        let k = Kernel::default();
        let mut kk = Kernel::default();
        let (brep, faceuses) = box_faceuses(&mut kk, false);
        for (_, fu) in faceuses {
            let found =
                local_extension_reach(&k, &brep, fu, &[], &[], 1e-3, &mut EdgeBounds::default())
                    .unwrap();
            assert_eq!(found, Some(LocalReach::default()));
        }
    }

    /// A 20 mm cube with its vertical edge at x = y = 20 rounded r 5 (a cylinder about
    /// (15, 15)), and its faces: the round, the face on x = 20 and the bottom.
    fn rounded_cube(k: &mut Kernel) -> (Brep, FaceId, FaceId, FaceId) {
        use crate::blend::fillet_executive::api::fillet_edges;
        use crate::topology::vertex;
        let mut b = crate::primitive::api_primitives::create_box(k, Point3::ZERO, 20.0, 20.0, 20.0)
            .unwrap();
        let e = b
            .edges()
            .ids()
            .find(|&e| {
                let (s, t) = edge::start_end_vertices(&b, e).unwrap();
                [s, t].into_iter().flatten().all(|v| {
                    let p = vertex::point(&b, v).unwrap();
                    p.x == 20.0 && p.y == 20.0
                })
            })
            .unwrap();
        fillet_edges(k, &mut b, &[e], 5.0, 1, 1, 1.0).unwrap();
        let mut round = None;
        let mut side = None;
        let mut bottom = None;
        for f in bp1::faces(&b, None).unwrap() {
            let face = b.face(f).unwrap();
            let surface = face.surface.as_ref().unwrap();
            let p = surface
                .evaluate_point(k, face.uv_domain.evaluate(0.5, 0.5))
                .unwrap();
            match surface.kind() {
                SurfaceKind::Cone(_) => round = Some(f),
                SurfaceKind::Plane(_) if (p.x - 20.0).abs() < 1e-9 => side = Some(f),
                SurfaceKind::Plane(_) if p.z.abs() < 1e-9 => bottom = Some(f),
                _ => {}
            }
        }
        (b, round.unwrap(), side.unwrap(), bottom.unwrap())
    }

    fn faceuse_of(b: &Brep, f: FaceId) -> FaceuseId {
        super::super::p2::select_offset_faceuse(b, f, false, false, OP).unwrap()
    }

    /// The reach of `f` with `moved` and `kept`, refused: its reason.
    fn refusal(
        k: &Kernel,
        b: &Brep,
        f: FaceId,
        moved: &[(FaceId, &Surface)],
        kept: &[(FaceId, &Surface)],
    ) -> Option<OffsetFailure> {
        let fu = faceuse_of(b, f);
        let e = local_extension_reach(k, b, fu, moved, kept, 1e-3, &mut EdgeBounds::default())
            .unwrap_err();
        match e.failure().map(|f| &f.reason) {
            Some(crate::error::Reason::Offset(why)) => Some(*why),
            _ => None,
        }
    }

    /// Each face's reach with `moved`.
    fn reach_of(k: &Kernel, b: &Brep, f: FaceId, moved: &[(FaceId, &Surface)]) -> LocalReach {
        local_extension_reach(
            k,
            b,
            faceuse_of(b, f),
            moved,
            &[],
            1e-3,
            &mut EdgeBounds::default(),
        )
        .unwrap()
        .unwrap()
    }

    /// `x` is a certified upper bound of `want` within the B-rep's tolerance.
    fn bounds(b: &Brep, x: f64, want: f64) -> bool {
        x >= want - 1e-9 && x <= want + b.tolerance() + 1e-6
    }

    /// The edge of `b` between faces `f` and `g`.
    fn edge_between(b: &Brep, f: FaceId, g: FaceId) -> EdgeId {
        crate::topology::face::p4::edges(b, f, None, false, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                edge::edgeuses(b, e, None, None)
                    .unwrap()
                    .into_iter()
                    .any(|u| eu::face(b, u).unwrap() == Some(g))
            })
            .unwrap()
    }

    fn plane_at_x(x: f64) -> Surface {
        Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(x, 0.0, 0.0),
            Vec3::X,
        ))
    }

    /// The round's side face (x = 20, tangent to the round along y = 15) moved into the
    /// body by 0.1 meets the round `√(2 r δ − δ²) = √0.99` along it from the old tangent
    /// line (`½ s² / r ≈ δ`), `√(2 r δ) = 1` from the old edge's points, where the tangent
    /// planes, parallel, give nothing; the side's other edges, across right angles, reach
    /// 0.1. The round, which stays, is reached as far at that edge, and the bottom as far
    /// at its corner there (the moved side's line on the bottom meets the round's cylinder
    /// 1 from the vertex). Each moved 0.1. The side and the round cross along their tangent
    /// edge, on every piece of it; the bottom, whose edges with them stay, lists nothing.
    #[test]
    fn a_plane_moved_into_a_round_is_reached_where_it_meets_it() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let tangent = edge_between(&b, side, round);
        let into = plane_at_x(19.9);
        let moved = [(side, &into)];
        for f in [side, round, bottom] {
            let found = reach_of(&k, &b, f, &moved);
            assert!(bounds(&b, found.reach, 1.0), "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 0.1), "{f:?}: {found:?}");
            assert!(found.apart.is_empty(), "{f:?}: {found:?}");
            let crossing: &[EdgeId] = if f == bottom { &[] } else { &[tangent] };
            assert_eq!(found.crossing, crossing, "{f:?}: {found:?}");
        }
    }

    /// The round given the coaxial cylinder r 5.1 (moved out 0.1): the side faces, which
    /// stay, meet it `√(5.1² − 5²) ≈ 1.005` past their tangent edges, the bottom 0.1 past
    /// its arc and `√1.01` at its corners, where the sides' lines on it meet the cylinder.
    /// Each moved 0.1 (the arc's and the tangent lines' distance from the cylinder).
    #[test]
    fn a_round_moved_out_is_reached_on_its_cylinder() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let wider = Surface::from(
            crate::primitive::api_primitives::create_cylinder_surface(
                &k,
                Point3::new(15.0, 15.0, -1.0),
                5.1,
                22.0,
            )
            .unwrap(),
        );
        let moved = [(round, &wider)];
        let want = sqrt(5.1 * 5.1 - 25.0);
        for f in [side, round, bottom] {
            let found = reach_of(&k, &b, f, &moved);
            assert!(bounds(&b, found.reach, want), "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 0.1), "{f:?}: {found:?}");
        }
        // the round crosses both sides, x = 20 and y = 20, along its tangent edges, on every
        // piece of them
        let other_side = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let face = b.face(f).unwrap();
                let surface = face.surface.as_ref().unwrap();
                let p = surface
                    .evaluate_point(&k, face.uv_domain.evaluate(0.5, 0.5))
                    .unwrap();
                matches!(surface.kind(), SurfaceKind::Plane(_)) && (p.y - 20.0).abs() < 1e-9
            })
            .unwrap();
        let tangent = [
            edge_between(&b, side, round),
            edge_between(&b, other_side, round),
        ];
        assert_eq!(reach_of(&k, &b, side, &moved).crossing, tangent[..1]);
        let at_round = reach_of(&k, &b, round, &moved);
        assert!(
            tangent.iter().all(|e| at_round.crossing.contains(e)),
            "{at_round:?}"
        );
    }

    /// A cylinder r 1 from z 0 to 2 (its side one face meeting itself across its seam,
    /// plane caps), its side, caps and seam edge (the edge whose uses are all the side's),
    /// and the coaxial cylinder `r` from z −1 to 3 for the side.
    fn seamed_cylinder(k: &mut Kernel, r: f64) -> (Brep, FaceId, [FaceId; 2], EdgeId, Surface) {
        use crate::primitive::api_primitives as ap;
        let b = ap::create_cylinder(k, Point3::ZERO, 1.0, 2.0).unwrap();
        let wider = Surface::from(
            ap::create_cylinder_surface(k, Point3::new(0.0, 0.0, -1.0), r, 4.0).unwrap(),
        );
        let faces = bp1::faces(&b, None).unwrap();
        let curved = |f: &FaceId| {
            let surface = b.face(*f).unwrap().surface.as_ref().unwrap();
            matches!(surface.kind(), SurfaceKind::Cone(_))
        };
        let side = *faces.iter().find(|f| curved(f)).unwrap();
        let caps: Vec<FaceId> = faces.iter().copied().filter(|f| !curved(f)).collect();
        assert_eq!(caps.len(), 2, "{faces:?}");
        let seams: Vec<EdgeId> = b
            .edges()
            .ids()
            .filter(|&e| edge::faces(&b, e, None).unwrap() == [side])
            .collect();
        assert_eq!(seams.len(), 1, "the side's seam");
        (b, side, [caps[0], caps[1]], seams[0], wider)
    }

    /// The side of a cylinder r 1 given the coaxial cylinder r 2 (moved out 1, across its
    /// seam): each cap's trace in the normal section of its circle, the radial line, meets
    /// the cylinder r 2 1 past the circle, so the side and both caps reach 1, and each
    /// moved 1. The seam is no meeting of two surfaces: the side continues across it, so it
    /// adds nothing beyond the side's own move there (the seam line r 1 is 1 from the
    /// cylinder r 2) and is not apart.
    #[test]
    fn a_seamed_cylinder_moved_out_is_reached_and_its_seam_adds_only_the_move() {
        let mut k = Kernel::default();
        let (b, side, caps, seam, wider) = seamed_cylinder(&mut k, 2.0);
        let moved = [(side, &wider)];
        for f in [side, caps[0], caps[1]] {
            let found = reach_of(&k, &b, f, &moved);
            assert!(bounds(&b, found.reach, 1.0), "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 1.0), "{f:?}: {found:?}");
            assert!(found.apart.is_empty(), "{f:?}: {found:?}");
        }
        let Some(Exact::Quadric(q)) = exact_class(&k, &wider).unwrap() else {
            panic!("the cylinder r 2 is a quadric");
        };
        let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
            Ok(quadric_distance_piece(over.0, mid.0, dt, &q))
        };
        let (at_seam, apart, _) = edge_interval_maximum(
            &b,
            seam,
            &piece,
            b.tolerance(),
            &mut EdgeBounds::default(),
            EdgeQuantity::Shift(None),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(bounds(&b, at_seam, 1.0), "{at_seam}");
        assert!(!apart);
    }

    /// An edge's bound is taken from [`EdgeBounds`] only for the same quantity on the same
    /// inputs: the seam of a cylinder r 1 against the cylinder r 2, its pieces counted. The
    /// second ask gives the first's bits and bounds no piece; another surface, tolerance or
    /// quantity bounds the edge again, as does an ask after a clear or with no surfaces.
    #[test]
    fn an_edge_bound_is_reused_only_on_the_same_inputs() {
        let mut k = Kernel::default();
        let (b, side, _, seam, wider) = seamed_cylinder(&mut k, 2.0);
        let c = exact_class(&k, &wider).unwrap().unwrap();
        let Exact::Quadric(q) = c else {
            panic!("the cylinder r 2 is a quadric");
        };
        let calls = std::cell::Cell::new(0_usize);
        let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
            calls.set(calls.get() + 1);
            Ok(quadric_distance_piece(over.0, mid.0, dt, &q))
        };
        let ask = |bounds: &mut EdgeBounds, tol: f64, of: EdgeQuantity, on: Option<&[u64]>| {
            let before = calls.get();
            let found = edge_interval_maximum(&b, seam, &piece, tol, bounds, of, on)
                .unwrap()
                .unwrap();
            (found.0.to_bits(), found.1, calls.get() - before)
        };
        let surfaces = exact_bits(&c).unwrap();
        let mut other = surfaces.clone();
        *other.last_mut().unwrap() ^= 1;
        let (tol, shift) = (b.tolerance(), EdgeQuantity::Shift(Some(side)));
        let mut held = EdgeBounds::default();
        let first = ask(&mut held, tol, shift, Some(&surfaces));
        assert!(first.2 > 0, "{first:?}");
        assert_eq!(
            ask(&mut held, tol, shift, Some(&surfaces)),
            (first.0, first.1, 0)
        );
        assert!(ask(&mut held, tol, shift, Some(&other)).2 > 0);
        assert_eq!(ask(&mut held, tol, shift, Some(&other)).2, 0);
        assert!(ask(&mut held, 2.0 * tol, shift, Some(&other)).2 > 0);
        let none = EdgeQuantity::Shift(None);
        assert!(ask(&mut held, tol, none, Some(&surfaces)).2 > 0);
        assert_eq!(
            ask(&mut held, tol, none, Some(&surfaces)),
            (first.0, first.1, 0)
        );
        held.clear();
        assert_eq!(held.held(), 0);
        assert_eq!(ask(&mut held, tol, none, Some(&surfaces)), first);
        for _ in 0..2 {
            assert_eq!(ask(&mut EdgeBounds::default(), tol, shift, None), first);
        }
        let mut unrecorded = EdgeBounds::default();
        assert_eq!(ask(&mut unrecorded, tol, shift, None), first);
        assert_eq!(unrecorded.held(), 0);
    }

    /// What a bound is recorded on ([`bound_inputs`]) tells curves apart by their bits: a
    /// line from 0 to 1 against the same line with its first knot −0 (equal as numbers), a
    /// control point moved, or a weight changed; the same curve gives the same bits.
    #[test]
    fn a_changed_curve_is_another_bound() {
        let line = |u0: f64, x1: f64, w1: f64| NurbsCurve {
            p: 1,
            n: 2,
            u: vec![u0, 0.0, 1.0, 1.0],
            pw: vec![0.0, 0.0, 0.0, 1.0, x1 * w1, 0.0, 0.0, w1],
        };
        let interval = crate::geom::extent1d::Extent1d::from_raw(0.0, 1.0);
        let on = |c: &NurbsCurve| bound_inputs(c, interval, 1e-5, &[7]);
        let base = on(&line(0.0, 1.0, 1.0));
        assert_eq!(on(&line(0.0, 1.0, 1.0)), base);
        for other in [
            line(-0.0, 1.0, 1.0),
            line(0.0, 1.5, 1.0),
            line(0.0, 1.0, 2.0),
        ] {
            assert_ne!(on(&other), base, "{other:?}");
        }
        assert_ne!(
            bound_inputs(&line(0.0, 1.0, 1.0), interval, 1e-5, &[8]),
            base
        );
    }

    /// The side of a cylinder r 1 given the coaxial cylinder one rounding wider: every
    /// face is reached with no refusal, by a certified bound (the true meetings are a
    /// rounding past the circles: the bound stays within the B-rep's tolerance of that
    /// rounding when its pieces converge within the caps, and is a bound past them), and
    /// the move, a rounding along the seam and the circles, within the tolerance.
    #[test]
    fn a_seamed_cylinder_a_rounding_wider_is_reached() {
        let mut k = Kernel::default();
        let r = f64::from_bits(1.0_f64.to_bits() + 1);
        let (b, side, caps, _, wider) = seamed_cylinder(&mut k, r);
        let moved = [(side, &wider)];
        for f in [side, caps[0], caps[1]] {
            let found = local_extension_reach(
                &k,
                &b,
                faceuse_of(&b, f),
                &moved,
                &[],
                1e-3,
                &mut EdgeBounds::default(),
            )
            .unwrap()
            .unwrap();
            assert!(found.reach >= 0.0 && found.reach < 1e-3, "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 0.0), "{f:?}: {found:?}");
            assert!(found.apart.is_empty(), "{f:?}: {found:?}");
        }
    }

    /// The side moved out by 0.1, off the round: the plane x = 20.1 passes the cylinder,
    /// whose trace bends away from it, 0.1 off: no extension makes them meet, so their edge
    /// adds nothing and is listed apart (for the side and the round), and the bottom's
    /// corner there has no meeting. The side's other edges reach 0.1; the round reaches
    /// nothing.
    #[test]
    fn a_plane_moved_off_a_round_is_apart() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let off = plane_at_x(20.1);
        let moved = [(side, &off)];
        let at_side = reach_of(&k, &b, side, &moved);
        assert!(bounds(&b, at_side.reach, 0.1), "{at_side:?}");
        assert_eq!(at_side.apart.len(), 1, "{at_side:?}");
        let at_round = reach_of(&k, &b, round, &moved);
        assert!(bounds(&b, at_round.reach, 0.0), "{at_round:?}");
        assert_eq!(at_round.apart, at_side.apart);
        let at_bottom = reach_of(&k, &b, bottom, &moved);
        assert!(bounds(&b, at_bottom.reach, 0.1), "{at_bottom:?}");
        assert!(at_bottom.apart.is_empty(), "{at_bottom:?}");
        for found in [at_side, at_round, at_bottom] {
            assert!(bounds(&b, found.shift, 0.1), "{found:?}");
            assert!(found.crossing.is_empty(), "{found:?}");
        }
    }

    /// The round replaced by its chord plane x + y = 35 (through both tangent lines): the
    /// bottom's and the round's arc edge (centre (15, 15), r 5) lies off the planes'
    /// line z = 0, x + y = 35 by `(x + y − 35) / √2`, at most `5 − 5 / √2` at the arc's
    /// middle and 0 at its ends, and off the chord plane as far: both bounds hold it
    /// from above, within the B-rep's tolerance. The side meets the chord plane on its old
    /// tangent edge: nothing to reach, nothing moved.
    #[test]
    fn a_curved_edge_between_planes_is_bounded_on_its_control_points() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let chord = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(17.5, 17.5, 0.0),
            Vec3::new(1.0, 1.0, 0.0).unitize().unwrap(),
        ));
        let moved = [(round, &chord)];
        let want = 5.0 - 5.0 / sqrt(2.0);
        let slack = b.tolerance() + 1e-9;
        for f in [bottom, round] {
            let fu = faceuse_of(&b, f);
            let found =
                local_extension_reach(&k, &b, fu, &moved, &[], 1e-3, &mut EdgeBounds::default())
                    .unwrap()
                    .unwrap();
            for (name, x) in [("reach", found.reach), ("move", found.shift)] {
                assert!(
                    x >= want - 1e-9 && x <= want + slack,
                    "{f:?} {name}: {x} vs {want}"
                );
            }
            assert!(found.apart.is_empty());
        }
        let fu = faceuse_of(&b, side);
        let found =
            local_extension_reach(&k, &b, fu, &moved, &[], 1e-3, &mut EdgeBounds::default())
                .unwrap()
                .unwrap();
        assert!(found.reach <= slack && found.shift <= slack, "{found:?}");
    }

    /// The bottom moved down 0.1: the side x = 20 meets it 0.1 down; at their corner with
    /// the round (20, 15, 0) the side's line on the moved bottom is tangent to the round's
    /// cylinder, roots undecided within rounding, bounded on both: still 0.1 from the
    /// vertex, to the rounding. The round meets the moved bottom 0.1 below its arc.
    #[test]
    fn a_corner_tangent_to_a_round_is_bounded_on_both_roots() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let lower = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(0.0, 0.0, -0.1),
            Vec3::Z,
        ));
        let moved = [(bottom, &lower)];
        for f in [side, round] {
            let found = reach_of(&k, &b, f, &moved);
            assert!(bounds(&b, found.reach, 0.1), "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 0.1), "{f:?}: {found:?}");
        }
    }

    /// The corner of the planes z = 0 and x = 0.2 with the unit sphere about the origin,
    /// at the vertex (0, 0, 0): their line along y meets it at y = ±√0.96, both 1 from the
    /// vertex; the planes x = 1.1 and z = 0 pass the sphere: no corner; parallel planes
    /// meet in no line: no corner.
    #[test]
    fn a_line_of_two_planes_meets_a_quadric_at_its_nearest_root() {
        let plane = |o: Vec3, n: Vec3| Tangent {
            point: o,
            normal: n,
            raw: n,
        };
        let sphere = Implicit::Sphere {
            centre: Vec3::ZERO,
            radius: 1.0,
        };
        let floor = plane(Vec3::ZERO, Vec3::Z);
        let cut = |x: f64| plane(Vec3::new(x, 0.0, 0.0), Vec3::X);
        let at = |a: Tangent| corner_with_quadric(Vec3::ZERO, floor, a, &sphere, 1e-3, 1e-9);
        let CornerMeeting::Within(r) = at(cut(0.2)) else {
            panic!("no corner");
        };
        assert!((r - 1.0).abs() < 1e-12, "{r}");
        assert_eq!(at(cut(1.1)), CornerMeeting::Nowhere);
        assert_eq!(
            at(plane(Vec3::new(0.0, 0.0, 1.0), Vec3::Z)),
            CornerMeeting::Nowhere
        );
    }

    /// A kept straight edge along z through (10, 0), from the vertex (10, 0, 10), and a
    /// plane tilted 3 degrees about the line x = 5 on z = 10: the line crosses it at
    /// z = 10 + 5 tan 3 degrees, 5 tan 3 degrees from the vertex; an edge 0.4 off the vertex
    /// adds its foot's distance. A plane along the line, or within the tangency tolerance
    /// of it, meets it nowhere near.
    #[test]
    fn a_kept_straight_edge_meets_a_moved_plane_where_its_line_crosses_it() {
        let (s, c) = (3.0_f64.to_radians().sin(), 3.0_f64.to_radians().cos());
        let tilted = Tangent {
            point: Vec3::new(5.0, 0.0, 10.0),
            normal: Vec3::new(-s, 0.0, c),
            raw: Vec3::new(-s, 0.0, c),
        };
        let edge = |x: f64| StraightEdge {
            a: Vec3::new(x, 0.0, 0.0),
            b: Vec3::new(x, 0.0, 10.0),
            dev: 0.0,
        };
        let p = Vec3::new(10.0, 0.0, 10.0);
        let want = 5.0 * s / c;
        let CornerMeeting::Within(r) = straight_edge_meets_plane(p, edge(10.0), tilted, 1e-3)
        else {
            panic!("no meeting");
        };
        assert!(r >= want && r - want < 1e-12, "{r} {want}");
        let CornerMeeting::Within(r) = straight_edge_meets_plane(p, edge(10.4), tilted, 1e-3)
        else {
            panic!("no meeting");
        };
        let want = (0.16 + (5.4 * s / c).powi(2)).sqrt();
        assert!(r >= want && r - want < 1e-12, "{r} {want}");
        // the curve's distance from its line, over the sine of the line's angle to the plane
        let off = StraightEdge {
            dev: 1e-6,
            ..edge(10.0)
        };
        let CornerMeeting::Within(r) = straight_edge_meets_plane(p, off, tilted, 1e-3) else {
            panic!("no meeting");
        };
        assert!(r >= 5.0 * s / c + 1e-6 / c, "{r}");
        let along = Tangent {
            point: Vec3::new(20.0, 0.0, 0.0),
            normal: Vec3::X,
            raw: Vec3::X,
        };
        assert_eq!(
            straight_edge_meets_plane(p, edge(10.0), along, 1e-3),
            CornerMeeting::Nowhere
        );
        let nearly = Vec3::new(1.0, 0.0, 1e-4).unitize().unwrap();
        let grazing = Tangent {
            point: Vec3::new(20.0, 0.0, 0.0),
            normal: nearly,
            raw: nearly,
        };
        assert_eq!(
            straight_edge_meets_plane(p, edge(10.0), grazing, 1e-3),
            CornerMeeting::Nowhere
        );
    }

    /// A 60 x 40 x 20 block whose top is imprinted along x = 30, the piece x < 30 raised
    /// to z = 21: the two pieces' planes are parallel and 1 apart. Their edge adds nothing
    /// and is listed apart, for either piece; the piece that stays moved nothing itself,
    /// and the raised one is 1 from its old corners.
    #[test]
    fn parallel_planes_apart_are_listed_and_add_nothing() {
        use crate::boolean::api_brep::boolean_with_options;
        use crate::boolean::merge::MergeOptions;
        use crate::geom::a_object::BooleanOperation;
        let mut k = Kernel::default();
        let block =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 60.0, 40.0, 20.0)
                .unwrap();
        let cutter = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(30.0, -1.0, 10.0),
            60.0,
            42.0,
            20.0,
        )
        .unwrap();
        let mut b = boolean_with_options(
            &mut k,
            block,
            cutter,
            BooleanOperation::Imprint,
            MergeOptions::default(),
        )
        .unwrap()
        .result;
        b.refresh_cached_topology();
        let top_piece = |low: bool| {
            bp1::faces(&b, None)
                .unwrap()
                .into_iter()
                .find(|&f| {
                    let face = b.face(f).unwrap();
                    let s = face.surface.as_ref().unwrap();
                    let p = s
                        .evaluate_point(&k, face.uv_domain.evaluate(0.5, 0.5))
                        .unwrap();
                    matches!(s.kind(), SurfaceKind::Plane(_))
                        && (p.z - 20.0).abs() < 1e-9
                        && (p.x < 30.0) == low
                })
                .unwrap()
        };
        let (piece, twin) = (top_piece(true), top_piece(false));
        let raised = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(0.0, 0.0, 21.0),
            Vec3::Z,
        ));
        let moved = [(piece, &raised)];
        let at = |f: FaceId| {
            local_extension_reach(
                &k,
                &b,
                faceuse_of(&b, f),
                &moved,
                &[],
                1e-3,
                &mut EdgeBounds::default(),
            )
            .unwrap()
            .unwrap()
        };
        let stays = at(twin);
        assert_eq!(stays.apart.len(), 1, "{stays:?}");
        assert!(stays.reach < 1e-9, "{stays:?}");
        assert!((stays.shift - 1.0).abs() < 1e-9, "{stays:?}");
        let rises = at(piece);
        assert_eq!(rises.apart, stays.apart);
        assert!((rises.shift - 1.0).abs() < 1e-9, "{rises:?}");
    }

    /// Nothing moved on the rounded cube: every face meets its neighbours at its edges,
    /// tangent ones too.
    #[test]
    fn a_rounded_body_nothing_moves_in_needs_no_extension() {
        let mut k = Kernel::default();
        let (b, _, _, _) = rounded_cube(&mut k);
        for f in bp1::faces(&b, None).unwrap() {
            let fu = faceuse_of(&b, f);
            let found =
                local_extension_reach(&k, &b, fu, &[], &[], 1e-3, &mut EdgeBounds::default())
                    .unwrap();
            assert_eq!(found, Some(LocalReach::default()));
        }
    }

    /// The round's exact rational form with no analytic label: its bare net.
    fn bare_net(k: &Kernel, b: &Brep, round: FaceId) -> Surface {
        Surface::from(
            crate::blend::fillet_section::station::as_b_spline(
                k,
                b.face(round).unwrap().surface.as_ref().unwrap(),
            )
            .unwrap()
            .unwrap()
            .into_owned(),
        )
    }

    /// The round kept as its bare net beside the side moved in by 0.1: as on its cylinder
    /// (`a_plane_moved_into_a_round_is_reached_where_it_meets_it`), the side's trace meets
    /// the net `√(2 r δ) = 1` from the old tangent edge's points, the bottom as far at its
    /// corner there (the bottom's and the moved side's line meets the net), and each moved
    /// 0.1. A plane beside a net, bounded on the net.
    #[test]
    fn a_round_kept_as_its_net_is_reached_where_the_moved_side_meets_it() {
        let mut k = Kernel::default();
        let (b, round, side, bottom) = rounded_cube(&mut k);
        let net = bare_net(&k, &b, round);
        let into = plane_at_x(19.9);
        let moved = [(side, &into)];
        let kept = [(round, &net)];
        for f in [side, round, bottom] {
            let found = local_extension_reach(
                &k,
                &b,
                faceuse_of(&b, f),
                &moved,
                &kept,
                1e-3,
                &mut EdgeBounds::default(),
            )
            .unwrap()
            .unwrap();
            assert!(bounds(&b, found.reach, 1.0), "{f:?}: {found:?}");
            assert!(bounds(&b, found.shift, 0.1), "{f:?}: {found:?}");
            assert!(found.apart.is_empty(), "{f:?}: {found:?}");
        }
    }

    /// Along the tangent edge itself, each normal section's meeting of the side moved in by
    /// 0.1 with the round's bare net is `√0.99` along the side's trace from the foot `0.1`
    /// off the edge point: `√(0.1² + 0.99) = 1` from every point of the edge, which is the
    /// edge's bound ([`plane_net_piece`] over its pieces, without the corners).
    #[test]
    fn a_tangent_edge_is_bounded_on_the_net_by_the_foot_and_the_arc() {
        let mut k = Kernel::default();
        let (b, round, side, _) = rounded_cube(&mut k);
        let net = bare_net(&k, &b, round);
        let Some(Exact::Net(net)) = exact_class(&k, &net).unwrap() else {
            panic!("a bare net");
        };
        let tangent = crate::topology::face::p4::edges(&b, side, None, false, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                edge::edgeuses(&b, e, None, None)
                    .unwrap()
                    .into_iter()
                    .any(|u| eu::face(&b, u).unwrap() == Some(round))
            })
            .unwrap();
        let into = Tangent {
            point: Vec3::new(19.9, 0.0, 0.0),
            normal: Vec3::X,
            raw: Vec3::X,
        };
        let guess = std::cell::Cell::new(None);
        let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
            Ok(plane_net_piece(over, mid, dt, into, &net, &guess))
        };
        let (reach, apart, _) = edge_interval_maximum(
            &b,
            tangent,
            &piece,
            b.tolerance(),
            &mut EdgeBounds::default(),
            EdgeQuantity::Shift(None),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(bounds(&b, reach, 1.0) && !apart, "{reach}");
    }

    /// The round (r 5, axis vertical through (15, 15)) kept as its bare net, and the bottom,
    /// an end face of the round, given the tilted plane `z = −0.2 − 0.05 (x − 15)`. The
    /// round's rulings are vertical, so each point `(x, y, 0)` of the round's bottom edge
    /// meets the plane straight below it, at `|z*| = 0.2 + 0.05 (x − 15)`, most 0.45 at
    /// `(20, 15, 0)`. There the side x = 20, a kept plane holding the net's boundary row,
    /// meets the moved plane in a line that runs along the net's extension past that row
    /// (the side's own plane): their corner is where the row's ruling crosses the moved
    /// plane, 0.45 below. The round reaches that far, at its edge and at that corner; the
    /// bottom reaches as far as its edge with x = 0, where the plane meets x = 0 at z = 0.55.
    #[test]
    fn a_tilted_end_face_beside_a_bare_net_round_is_reached_in_closed_form() {
        let mut k = Kernel::default();
        let (b, round, _, bottom) = rounded_cube(&mut k);
        let net = bare_net(&k, &b, round);
        let tilted = Surface::from(crate::surface::plane::Plane::from_normal(
            Point3::new(15.0, 15.0, -0.2),
            Vec3::new(0.05, 0.0, 1.0),
        ));
        let moved = [(bottom, &tilted)];
        let kept = [(round, &net)];
        let reach = |f: FaceId| {
            local_extension_reach(
                &k,
                &b,
                faceuse_of(&b, f),
                &moved,
                &kept,
                1e-3,
                &mut EdgeBounds::default(),
            )
            .unwrap()
            .unwrap()
            .reach
        };
        let at_round = reach(round);
        assert!(bounds(&b, at_round, 0.45), "{at_round}");
        let at_bottom = reach(bottom);
        assert!(bounds(&b, at_bottom, 0.55), "{at_bottom}");
    }

    /// Along the round's bottom arc alone, each normal section's meeting of the tilted plane
    /// above with the net is on the ruling below the edge point, `0.2 + 0.05 (x − 15)` below
    /// it: the edge's bound is the largest, 0.45 at `x = 20`, within the B-rep's tolerance
    /// ([`plane_net_piece`] over its pieces).
    #[test]
    fn a_net_is_bounded_along_its_rulings_below_a_tilted_plane() {
        let mut k = Kernel::default();
        let (b, round, _, bottom) = rounded_cube(&mut k);
        let net = bare_net(&k, &b, round);
        let Some(Exact::Net(net)) = exact_class(&k, &net).unwrap() else {
            panic!("a bare net");
        };
        let arc = crate::topology::face::p4::edges(&b, bottom, None, false, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                edge::edgeuses(&b, e, None, None)
                    .unwrap()
                    .into_iter()
                    .any(|u| eu::face(&b, u).unwrap() == Some(round))
            })
            .unwrap();
        let n = Vec3::new(0.05, 0.0, 1.0);
        let tilted = Tangent {
            point: Vec3::new(15.0, 15.0, -0.2),
            normal: n.unitize().unwrap(),
            raw: n,
        };
        let guess = std::cell::Cell::new(None);
        let piece = |over: (Dv, Dv), mid: (Dv, Dv), dt: Ivl| {
            Ok(plane_net_piece(over, mid, dt, tilted, &net, &guess))
        };
        let (reach, apart, _) = edge_interval_maximum(
            &b,
            arc,
            &piece,
            b.tolerance(),
            &mut EdgeBounds::default(),
            EdgeQuantity::Shift(None),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(bounds(&b, reach, 0.45) && !apart, "{reach}");
    }

    /// The same net beside the side moved out by 0.1, off the round: the side's trace passes
    /// the net and meets it nowhere near, which no box proves (on the cylinder the plan
    /// decides it apart, `a_plane_moved_off_a_round_is_apart`): refused as uncertified.
    #[test]
    fn a_plane_moved_off_a_round_kept_as_its_net_is_uncertified() {
        let mut k = Kernel::default();
        let (b, round, side, _) = rounded_cube(&mut k);
        let net = bare_net(&k, &b, round);
        let off = plane_at_x(20.1);
        assert_eq!(
            refusal(&k, &b, side, &[(side, &off)], &[(round, &net)]),
            Some(OffsetFailure::ReachUncertified { face: side })
        );
    }

    /// The de Casteljau split at a half: the quadratic `(0, 1, 4)` splits into
    /// `(0, ½, 1½)` and `(1½, 2½, 4)`, and both halves' ends are points of it.
    #[test]
    fn a_split_keeps_the_curve() {
        let c = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [4.0, 0.0, 0.0, 1.0],
        ];
        let (left, right) = split_at(&c, 0.5);
        let xs = |v: &[[f64; 4]]| v.iter().map(|p| p[0]).collect::<Vec<_>>();
        assert_eq!(xs(&left), vec![0.0, 0.5, 1.5]);
        assert_eq!(xs(&right), vec![1.5, 2.5, 4.0]);
    }

    /// A meeting's distance takes the trace direction's length: the unit sphere and the line
    /// `x0 + τ u` through its centre with `u = (2, 0, 0)` (far from unit, to show the
    /// factor; a trace's own `u` is unit within rounding), its edge point `f` off the centre.
    /// The roots `τ = ±½` are the points `(±1, 0, 0)`, `√(f² + 1)` from the edge point; as
    /// if `|u| = 1` the bound would be `√(f² + ¼)`, below it.
    #[test]
    fn a_meeting_is_as_far_as_its_point_whatever_the_direction_length() {
        let sphere = Implicit::Sphere {
            centre: Vec3::new(0.0, 0.0, 0.0),
            radius: 1.0,
        };
        for f in [0.0, 0.5, 3.0] {
            let line = Trace {
                x0: Dv::constant(Vec3::new(0.0, 0.0, 0.0)),
                foot2: Di::constant(f * f),
                u: Dv::constant(Vec3::new(2.0, 0.0, 0.0)),
            };
            let q = along_line(&sphere, &line).unwrap();
            let plan = plan_of(&line, &q, &sphere, 1e-5);
            assert!(matches!(plan, Plan::Roots { .. }), "{plan:?}");
            let found = planned(&plan, &line, &q).unwrap();
            assert_eq!(found.len(), 2, "{found:?}");
            let exact = f * f + 1.0;
            for g in found {
                assert!(g.v.lo <= exact && exact <= g.v.hi, "{f}: {g:?} {exact}");
            }
        }
    }

    /// A line's meeting with a plane is as far as its point, whatever the direction's
    /// length: the line through the origin along `u = (2, 0, 0)` (far from unit, to show
    /// it; an edge's own `u` is unit within rounding) meets the plane `x = 3` at `(3, 0, 0)`,
    /// `√5` from `p = (1, 1, 0)`; `x0 = (4, 0, 0)` is a point of the line but not `p`'s foot.
    /// A curve `dev` off the line, which is square to the plane, adds `dev` (the sine is
    /// `|u·ν| / (|u| |ν|)` = 1), not `dev / 2`.
    #[test]
    fn a_line_meets_a_plane_as_far_as_its_point_whatever_the_direction_length() {
        let plane = Tangent {
            point: Vec3::new(3.0, 0.0, 0.0),
            normal: Vec3::new(1.0, 0.0, 0.0),
            raw: Vec3::new(1.0, 0.0, 0.0),
        };
        for dev in [0.0, 0.1] {
            let met = line_meets_plane(
                Dv::constant(Vec3::new(1.0, 1.0, 0.0)),
                Dv::constant(Vec3::new(0.0, 0.0, 0.0)),
                Dv::constant(Vec3::new(2.0, 0.0, 0.0)),
                dev,
                plane,
                1e-3,
            );
            let exact = 5.0_f64.sqrt() + dev;
            let CornerMeeting::Within(reach) = met else {
                panic!("{dev}: {met:?}");
            };
            assert!(
                reach >= exact && reach <= exact * (1.0 + 1e-12),
                "{dev}: {reach} {exact}"
            );
        }
    }

    /// A synthetic slab cut to a sphere of radius 3, then bored along z with radius 1.
    /// With `half`, the y = 0 side cuts the circular bore/sphere meetings at corners.
    /// Without it the bore's circular edges have only two incident surfaces, so its
    /// reach exercises the curved-edge bound independently of the corner bound.
    fn spherical_bored_slab(k: &mut Kernel, half: bool) -> (Brep, FaceId) {
        use crate::boolean::api_brep::boolean_with_options;
        use crate::boolean::merge::MergeOptions;
        use crate::curve::curve::Curve;
        use crate::geom::a_object::BooleanOperation;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::primitive::api_primitives as ap;
        // Revolve the two cap endpoints and the bore seam together. The later slab
        // cut cannot touch the full bore, whose radius is strictly less than 2.
        k.fixes.revolve_analytic_profile_faces = true;
        let angle = (1.0_f64 / 3.0).acos().to_degrees();
        let arc = Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &crate::geom::axis2_placement::Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Z),
                3.0,
                -angle,
                angle,
                crate::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
            )
            .unwrap(),
        );
        let span = arc.natural_interval();
        let bottom = arc.evaluate_point(k, span.min()).unwrap();
        let top = arc.evaluate_point(k, span.max()).unwrap();
        let side = Curve::from(BSplineCurve::create_line_segment(3, top, bottom).unwrap());
        let ring = crate::sweep::api_brep::create_rotational_sweep(
            k,
            &[Some(&side), Some(&arc)],
            Point3::ZERO,
            Vec3::Z,
            360.0,
            true,
        )
        .unwrap();
        let slab = ap::create_box(
            k,
            Point3::new(-2.0, if half { 0.0 } else { -2.0 }, -4.0),
            4.0,
            if half { 2.0 } else { 4.0 },
            8.0,
        )
        .unwrap();
        let b = boolean_with_options(
            k,
            ring,
            slab,
            BooleanOperation::Intersection,
            MergeOptions::default(),
        )
        .unwrap()
        .result;
        assert!(crate::topology::brep::p2::is_manifold_solid(&b).unwrap());
        let bore = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                matches!(exact_class(k, b.face(f).unwrap().surface.as_ref().unwrap()).unwrap(),
                Some(Exact::Quadric(Implicit::Cone { radius, tan_angle, .. }))
                    if (radius - 1.0).abs() < 1e-10 && tan_angle.abs() < 1e-10)
            })
            .unwrap();
        (b, bore)
    }

    /// On the sphere, the new bore r(z) = 1 + t z meets z² + r(z)² = 9.
    /// The two meridian displacements from the old circles z = ±√8 are closed form.
    fn spherical_bore_reach(slope: f64) -> f64 {
        [-1.0, 1.0]
            .into_iter()
            .map(|sign| {
                let z = (-slope + sign * sqrt(8.0 + 9.0 * slope * slope)) / (1.0 + slope * slope);
                let dr = slope * z;
                let dz = z - sign * sqrt(8.0);
                sqrt(dr * dr + dz * dz)
            })
            .fold(0.0, f64::max)
    }

    fn drafted_bore(k: &Kernel) -> Surface {
        Surface::from(
            crate::primitive::api_primitives::create_cone_surface(
                k,
                Point3::new(0.0, 0.0, -4.0),
                0.8,
                1.2,
                8.0,
            )
            .unwrap(),
        )
    }

    /// Let B = 1 + t² and a = 1, R = 3. The cap intersections solve
    /// Bz² + 2at z + a² - R² = 0, with roots (-at ± √(R²B-a²))/B.
    /// Integrating the annular area π[R²-z²-(a+tz)²] between those roots gives
    /// V(t) = (4π/3)(R²B-a²)^(3/2)/B². Hence ΔV = V(t)-V(0).
    /// The cone stays inside x,y = ±2 at both cap intersections for these angles,
    /// so the slab clips the same exterior volume before and after; it cancels.
    /// The half slab has exactly half the change by reflection in y = 0.
    fn spherical_bored_slab_draft_volume(half: bool, degrees: f64) {
        use crate::mass::integrator::properties::brep_volume_default;
        use crate::offset::draft::{draft_surface, Draft};
        use crate::topology::brep::{local_operation::local_operation_simple, p2};
        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let (mut b, bore) = spherical_bored_slab(&mut k, half);
        let before = brep_volume_default(&k, &mut b, 1e-7).unwrap();
        let angle = degrees.to_radians();
        let draft = Draft::new(Point3::ZERO, Vec3::Z, Vec3::Z, angle).unwrap();
        let cone = draft_surface(&k, &b, bore, &draft).unwrap().unwrap();
        local_operation_simple(&mut k, &mut b, &[bore], vec![cone]).unwrap();
        assert!(p2::is_closed_manifold(&b).unwrap());
        assert!(p2::is_manifold_solid(&b).unwrap());
        let after = brep_volume_default(&k, &mut b, 1e-7).unwrap();
        let t = angle.tan();
        let big_b = 1.0 + t * t;
        let delta = 4.0 * core::f64::consts::PI / 3.0
            * ((9.0 * big_b - 1.0).powf(1.5) / (big_b * big_b) - 8.0_f64.powf(1.5));
        let expected = before + delta * if half { 0.5 } else { 1.0 };
        println!("spherical bore half={half}, angle={degrees}: before={before:.12}, after={after:.12}, expected={expected:.12}");
        // Absolute integration error is far below the smallest (1 degree) change.
        assert!(
            (after - expected).abs() < 1e-5,
            "half={half}, angle={degrees}: before={before}, after={after}, expected={expected}"
        );
    }

    #[test]
    fn a_full_spherical_bored_slab_drafts_one_degree_to_its_closed_form_volume() {
        spherical_bored_slab_draft_volume(false, 1.0);
    }

    #[test]
    fn a_full_spherical_bored_slab_drafts_five_degrees_to_its_closed_form_volume() {
        spherical_bored_slab_draft_volume(false, 5.0);
    }

    #[test]
    fn a_half_spherical_bored_slab_drafts_one_degree_to_its_closed_form_volume() {
        spherical_bored_slab_draft_volume(true, 1.0);
    }

    #[test]
    fn a_half_spherical_bored_slab_drafts_five_degrees_to_its_closed_form_volume() {
        spherical_bored_slab_draft_volume(true, 5.0);
    }

    #[test]
    fn cancelling_a_quadric_solve_does_not_cache_an_uncertified_edge() {
        use crate::kernel::CancelToken;
        let mut k = Kernel::default();
        let (b, bore) = spherical_bored_slab(&mut k, false);
        let sphere = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                matches!(
                    exact_class(&k, b.face(f).unwrap().surface.as_ref().unwrap()).unwrap(),
                    Some(Exact::Quadric(Implicit::Sphere { .. }))
                )
            })
            .unwrap();
        let e = edge_between(&b, bore, sphere);
        let cone = exact_class(&k, &drafted_bore(&k)).unwrap().unwrap();
        let ball = exact_class(&k, b.face(sphere).unwrap().surface.as_ref().unwrap())
            .unwrap()
            .unwrap();
        let (Exact::Quadric(a), Exact::Quadric(c)) = (&cone, &ball) else {
            panic!("quadrics")
        };
        let mut surfaces = exact_bits(&cone).unwrap();
        surfaces.extend(exact_bits(&ball).unwrap());
        let of = EdgeQuantity::Meeting(Some(bore.min(sphere)), Some(bore.max(sphere)));
        let mut held = EdgeBounds::default();
        let calls = std::cell::Cell::new(0);
        let token = CancelToken::after_polls(2);
        k.set_cancel_token(token.clone());
        {
            let piece = |over, mid, dt| {
                calls.set(calls.get() + 1);
                quadric_pair_piece(&k, over, mid, dt, a, c)
            };
            let err =
                edge_interval_maximum(&b, e, &piece, b.tolerance(), &mut held, of, Some(&surfaces))
                    .unwrap_err();
            assert!(matches!(err, SolidError::Cancelled { .. }), "{err:?}");
        }
        assert_eq!(token.polls(), Some(2)); // Newton entered, then cancelled mid-solve.
        assert_eq!(calls.get(), 1);
        assert_eq!(held.held(), 0);
        k.clear_cancel_token();
        let piece = |over, mid, dt| {
            calls.set(calls.get() + 1);
            quadric_pair_piece(&k, over, mid, dt, a, c)
        };
        let first =
            edge_interval_maximum(&b, e, &piece, b.tolerance(), &mut held, of, Some(&surfaces))
                .unwrap();
        assert!(first.is_some());
        assert!(calls.get() > 1, "cleared cancellation must solve afresh");
        let solved = calls.get();
        assert_eq!(
            edge_interval_maximum(&b, e, &piece, b.tolerance(), &mut held, of, Some(&surfaces))
                .unwrap(),
            first
        );
        assert_eq!(calls.get(), solved, "the successful solve is cached");
    }

    /// At the tangent equator of the old sphere/cylinder, replacing r = 3 by
    /// r(z) = 2.99 - 0.05z gives z = (0.299 ± √0.3296)/2.005.
    /// The negative root (~-0.137) is nearer the old edge than the positive (~0.435)
    /// but is on the sphere's continuation, outside its retained upper hemisphere.
    #[test]
    fn a_curved_tangent_crossing_refuses_an_uncertified_branch() {
        use crate::primitive::api_primitives as ap;
        use crate::topology::brep::local_operation::local_operation_simple;
        let mut k = Kernel {
            fixes: crate::kernel::UpstreamFixes::all(),
            ..Kernel::default()
        };
        let sphere = ap::create_sphere(&mut k, Point3::ZERO, 3.0).unwrap();
        let cylinder = ap::create_cylinder(&mut k, Point3::new(0.0, 0.0, -3.0), 3.0, 3.0).unwrap();
        let mut b = crate::boolean::api_brep::boolean_union(&mut k, sphere, cylinder).unwrap();
        assert!(crate::topology::brep::p2::is_manifold_solid(&b).unwrap());
        let side = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                matches!(
                    exact_class(&k, b.face(f).unwrap().surface.as_ref().unwrap()).unwrap(),
                    Some(Exact::Quadric(Implicit::Cone { .. }))
                )
            })
            .unwrap();
        // Draft the original cylinder so the replacement keeps its seam frame and UV
        // period. At z = -0.2 its radius stays 3, hence r(z) = 2.99 - 0.05z.
        let draft = crate::offset::draft::Draft::new(
            Point3::new(0.0, 0.0, -0.2),
            Vec3::Z,
            Vec3::Z,
            0.05_f64.atan(),
        )
        .unwrap();
        let cone = crate::offset::draft::draft_surface(&k, &b, side, &draft)
            .unwrap()
            .unwrap();
        let err = local_operation_simple(&mut k, &mut b, &[side], vec![cone]).unwrap_err();
        assert_eq!(
            err.offset_failure(),
            Some(&OffsetFailure::TangentBranchUncertified),
            "{err:?}"
        );
    }

    #[test]
    fn a_drafted_bore_meets_spherical_ends_along_certified_curved_edges() {
        let mut k = Kernel::default();
        let (b, bore) = spherical_bored_slab(&mut k, false);
        let cone = drafted_bore(&k);
        let found = reach_of(&k, &b, bore, &[(bore, &cone)]);
        let expected = spherical_bore_reach(0.05);
        // The capped interval refinement may leave a conservative excess; the reach
        // must enclose the exact meeting, and remain below twice that displacement.
        assert!(found.reach >= expected - 1e-10, "{found:?}, {expected}");
        assert!(found.reach < 2.0 * expected, "{found:?}, {expected}");
        assert!(found.apart.is_empty());
        let spherical_edges: Vec<_> = crate::topology::face::p4::edges(&b, bore, None, false, None)
            .unwrap()
            .into_iter()
            .filter(|&e| {
                edge::edgeuses(&b, e, None, None)
                    .unwrap()
                    .into_iter()
                    .any(|u| {
                        eu::face(&b, u).unwrap().is_some_and(|f| {
                            matches!(
                                exact_class(&k, b.face(f).unwrap().surface.as_ref().unwrap())
                                    .unwrap(),
                                Some(Exact::Quadric(Implicit::Sphere { .. }))
                            )
                        })
                    })
            })
            .collect();
        assert!(spherical_edges.len() >= 2);
        assert!(
            spherical_edges.iter().all(|e| found.crossing.contains(e)),
            "{found:?}"
        );
    }

    #[test]
    fn a_drafted_bore_meets_a_plane_and_sphere_at_certified_curved_corners() {
        let mut k = Kernel::default();
        let (b, bore) = spherical_bored_slab(&mut k, true);
        let cone = drafted_bore(&k);
        let expected = spherical_bore_reach(0.05);
        let mut saw_corner = false;
        for f in bp1::faces(&b, None).unwrap() {
            if let Some(Exact::Plane(plane)) =
                exact_class(&k, b.face(f).unwrap().surface.as_ref().unwrap()).unwrap()
            {
                if plane.normal.y.abs() > 0.99 && plane.point.y.abs() < 1e-10 {
                    let found = reach_of(&k, &b, f, &[(bore, &cone)]);
                    assert!(bounds(&b, found.reach, expected), "{found:?}, {expected}");
                    saw_corner = true;
                }
            }
        }
        assert!(saw_corner);
    }

    #[test]
    fn a_bore_tangent_to_or_past_its_spherical_end_remains_uncertified() {
        let mut k = Kernel::default();
        for half in [false, true] {
            let (b, bore) = spherical_bored_slab(&mut k, half);
            for radius in [3.0, 3.1] {
                let tangent = Surface::from(
                    crate::primitive::api_primitives::create_cylinder_surface(
                        &k,
                        Point3::new(0.0, 0.0, -4.0),
                        radius,
                        8.0,
                    )
                    .unwrap(),
                );
                assert_eq!(
                    refusal(&k, &b, bore, &[(bore, &tangent)], &[]),
                    Some(OffsetFailure::ReachUncertified { face: bore })
                );
                if half {
                    let side = bp1::faces(&b, None).unwrap().into_iter().find(|&f| {
                        matches!(exact_class(&k, b.face(f).unwrap().surface.as_ref().unwrap()).unwrap(),
                            Some(Exact::Plane(plane)) if plane.normal.y.abs() > 0.99 && plane.point.y.abs() < 1e-10)
                    }).unwrap();
                    assert_eq!(
                        refusal(&k, &b, side, &[(bore, &tangent)], &[]),
                        Some(OffsetFailure::ReachUncertified { face: side })
                    );
                }
            }
        }
    }

    #[test]
    fn a_quadric_meeting_on_the_wrong_cone_nappe_is_not_certified() {
        let k = Kernel::default();
        let cone = Implicit::Cone {
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            radius: -1.0,
            tan_angle: 0.0,
        };
        let sphere = Implicit::Sphere {
            centre: Vec3::ZERO,
            radius: 3.0,
        };
        let section = quadric::Section::corner(Vec3::new(1.0, 0.0, sqrt(8.0)), Vec3::ZERO, Vec3::Y);
        assert!(
            quadric::meet(&k, &cone, &sphere, section, section, Ivl::from(0.0))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_rotated_quadric_corner_far_from_the_origin_keeps_its_distance_enclosure() {
        let k = Kernel::default();
        let axis = Vec3::new(1.0, 2.0, 3.0).unitize().unwrap();
        let radial = axis.cross(Vec3::X).unitize().unwrap();
        let normal = axis.cross(radial);
        let centre = Vec3::new(10000.0, -20000.0, 30000.0);
        let p = centre + radial + axis * sqrt(8.0);
        let cone = Implicit::Cone {
            origin: centre,
            axis,
            radius: 1.0,
            tan_angle: 0.05,
        };
        let sphere = Implicit::Sphere {
            centre,
            radius: 3.0,
        };
        let section = quadric::Section::corner(p, centre, normal);
        let meeting = quadric::meet(&k, &cone, &sphere, section, section, Ivl::from(0.0))
            .unwrap()
            .unwrap();
        let z = (-0.05 + sqrt(8.0 + 9.0 * 0.05 * 0.05)) / (1.0 + 0.05 * 0.05);
        let dr = 0.05 * z;
        let dz = z - sqrt(8.0);
        let expected = sqrt(dr * dr + dz * dz);
        // The translated input point and rotated frame carry rounding at 30,000 units.
        assert!(meeting.distance.lo <= expected + 1e-8);
        assert!(meeting.distance.hi >= expected - 1e-8);
        assert!(meeting.distance.hi - meeting.distance.lo < 1e-8);
        assert!(meeting.transverse);
    }

    /// Two quadrics that are parallel surfaces are decided from the quadrics, as two planes
    /// are: spheres about one centre, and cones or cylinders about one axis at one angle
    /// (either axis sense, the origin anywhere along it, a label's rounding off), are one
    /// surface within the tolerance of each other, apart beyond it, with the axis's or
    /// centre's offset counted on either side. A cone beside a cylinder on its
    /// axis (3 degrees, or 0.3, within the tangency tolerance), a tilted or shifted axis, or
    /// spheres about two centres are left to [`quadric::meet`].
    #[test]
    fn parallel_quadrics_are_one_or_apart() {
        let tol = 1e-3;
        let cone = |origin: Vec3, axis: Vec3, radius: f64, tan_angle: f64| Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        };
        let ball = |centre: Vec3, radius: f64| Implicit::Sphere { centre, radius };
        let on_z = |z: f64| Vec3::new(0.0, 0.0, z);
        let pair = |a: &Implicit, b: &Implicit| (quadric_pair(a, b, tol), quadric_pair(b, a, tol));
        let both = |p: Option<PlanePair>| (p, p);
        let r50 = cone(Vec3::ZERO, Vec3::Z, 50.0, 0.0);
        // a split cylinder's half moved out 4, and the moved half against itself
        assert_eq!(
            pair(&r50, &cone(Vec3::ZERO, Vec3::Z, 54.0, 0.0)),
            both(Some(PlanePair::Apart))
        );
        assert_eq!(
            pair(&r50, &cone(on_z(100.0), -Vec3::Z, 54.0, 0.0)),
            both(Some(PlanePair::Apart))
        );
        assert_eq!(
            pair(&r50, &cone(on_z(7.0), Vec3::Z, 50.0 + 1e-4, 0.0)),
            both(Some(PlanePair::One))
        );
        // an axis e off within the tolerance: one where |gap| + e is within it, apart where
        // |gap| exceeds it by more than e, else left to the proof
        let off = |e: f64, r: f64| cone(Vec3::new(e, 0.0, 0.0), Vec3::Z, r, 0.0);
        assert_eq!(pair(&r50, &off(0.0, 50.0015)), both(Some(PlanePair::Apart)));
        assert_eq!(pair(&r50, &off(8e-4, 50.0015)), both(None));
        assert_eq!(pair(&r50, &off(8e-4, 50.0005)), both(None));
        assert_eq!(pair(&r50, &off(1e-4, 50.0001)), both(Some(PlanePair::One)));
        assert_eq!(pair(&r50, &off(8e-4, 54.0)), both(Some(PlanePair::Apart)));
        // an axis a label's rounding off (1.44e-13 rad, a cylinder's label as read)
        let rounded = Vec3::new(1.44e-13, 0.0, 1.0);
        assert_eq!(
            pair(&r50, &cone(Vec3::ZERO, rounded, 54.0, 0.0)),
            both(Some(PlanePair::Apart))
        );
        // a cone offset 4 along its normals, and the cone itself from another origin and sense
        let t = 0.1;
        let slope = sqrt(1.0 + t * t);
        let c10 = cone(Vec3::ZERO, Vec3::Z, 10.0, t);
        assert_eq!(
            pair(&c10, &cone(Vec3::ZERO, Vec3::Z, 10.0 + 4.0 * slope, t)),
            both(Some(PlanePair::Apart))
        );
        assert_eq!(
            pair(&c10, &cone(on_z(5.0), Vec3::Z, 10.5, t)),
            both(Some(PlanePair::One))
        );
        assert_eq!(
            pair(&c10, &cone(on_z(5.0), -Vec3::Z, 10.5, -t)),
            both(Some(PlanePair::One))
        );
        assert_eq!(pair(&c10, &cone(on_z(5.0), -Vec3::Z, 10.5, t)), both(None));
        // a drafted half beside the half not drafted, a tilted and a shifted axis
        let r10 = cone(Vec3::ZERO, Vec3::Z, 10.0, 0.0);
        for degrees in [3.0_f64, 0.3] {
            let drafted = cone(on_z(10.0), Vec3::Z, 10.0, -degrees.to_radians().tan());
            assert_eq!(pair(&r10, &drafted), both(None), "{degrees}");
        }
        let tilted = Vec3::new(
            (0.3_f64).to_radians().sin(),
            0.0,
            (0.3_f64).to_radians().cos(),
        );
        assert_eq!(pair(&r50, &cone(Vec3::ZERO, tilted, 54.0, 0.0)), both(None));
        assert_eq!(
            pair(&r50, &cone(Vec3::new(0.01, 0.0, 0.0), Vec3::Z, 54.0, 0.0)),
            both(None)
        );
        // a sphere's cap moved out 4, the cap against itself, and two centres
        let b50 = ball(Vec3::ZERO, 50.0);
        assert_eq!(
            pair(&b50, &ball(Vec3::ZERO, 54.0)),
            both(Some(PlanePair::Apart))
        );
        assert_eq!(
            pair(&b50, &ball(Vec3::new(0.0, 1e-4, 0.0), 50.0)),
            both(Some(PlanePair::One))
        );
        assert_eq!(
            pair(&b50, &ball(on_z(8e-4), 54.0)),
            both(Some(PlanePair::Apart))
        );
        assert_eq!(pair(&b50, &ball(on_z(8e-4), 50.0015)), both(None));
        assert_eq!(pair(&b50, &ball(on_z(0.01), 54.0)), both(None));
        assert_eq!(pair(&b50, &r50), both(None));
    }
}
