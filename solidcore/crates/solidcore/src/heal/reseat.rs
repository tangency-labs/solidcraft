//! Re-seating an edge whose curve lies off the faces it joins (not upstream): the curve is
//! fitted again through points between its two surfaces, its ends kept, so the edge's
//! tolerance can shrink to the certified gap of the new curve.
//!
//! Upstream: none. SMLib's healer has a step for it, `SmHealData::Fix_BadGaps`
//! ([`crate::heal::heal_data`]), which is empty upstream ("not implemented").
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`reseat_edge`], [`reseat_edges`] | none (`SmHealData::Fix_BadGaps` does nothing) |
//!
//! **Why.** A curve read from a file is often a loose fit of the intersection of its two faces:
//! its ends lie on both surfaces, its interior up to a thousandth of a millimetre off them, and
//! the reader grows the edge's tolerance to cover that gap. Every later check then measures the
//! gap again on whatever the operation makes of the edge (`docs/validity-and-tolerances.md`
//! §5.2, §8). The gap belongs to the curve, not to the surfaces, when the surfaces themselves
//! meet there (or nearly): the curve can be recomputed between them.
//!
//! **The construction.** At `n + 1` parameters `t_i` spread over the edge's interval:
//! - the curve's point `C(t_i)` is dropped onto each face's surface ([`Surface::drop_point`],
//!   seeded with the previous sample's parameters);
//! - where the surfaces cross there, the two feet are refined onto `S1 ∩ S2` by the local
//!   surface intersection, held in the plane through `C(t_i)` normal to the curve so the point
//!   does not slide along the intersection ([`Surface::local_surface_intersect`]); the target is
//!   that point;
//! - where they do not cross (two tangent surfaces a little apart), projections alternate from
//!   one surface to the other until the feet settle on a common normal; the target is the
//!   midpoint of the two feet, which halves the larger of the two gaps;
//! - the new curve interpolates the targets at the same parameters `t_i`, its ends kept at the
//!   old curve's ends `C(t_0)` and `C(t_n)`, so the edge's vertices and its neighbours do not
//!   move ([`BSplineCurve::create_interpolating_curve_with_params`]).
//!
//! **Certificates.** Only the result is trusted, never the samples: the new curve's distance from
//! each face is bounded over the whole curve by [`curve_face_gap_bound`] (outward-rounded, every
//! knot span). The edge is re-seated only when that bound `g` makes a tolerance `g /` [`GAP_SHARE`]
//! (floored at the body's) at most [`GAIN`] times the edge's present one; the tolerance is then
//! set to it, and the faces' trims are made again from the new curve
//! ([`crate::heal::tolerance_defects`]'s `remake_trims`). Samples are doubled, up to
//! [`MAX_SAMPLES`], while the bound misses. How far the curve moved is bounded too, old curve
//! against new at every parameter ([`certified_hausdorff`]); a move that cannot be bounded is
//! not made. No tolerance grows: what cannot be certified is left as
//! it is, with why ([`Refusal`]).
//!
//! **Stated tolerances.** A tolerance read from a file states the file's accuracy, and is kept
//! as it is (`docs/validity-and-tolerances.md` §8), but in two cases: an edge whose curve is
//! fitted again takes the tolerance of its new curve, and a tolerance the check refuses as too
//! wide ([`LARGE_SMALL_SIZE_RATIO`] times the body's or more, Tests #59 and #82) shrinks to what
//! its certified gaps need. An edge already on its faces whose tolerance the check takes is left
//! as it is ([`Refusal::AlreadyOnFaces`]).
//!
//! **Vertices.** After the edges, each vertex of a re-seated edge that is that wide may shrink, to the
//! largest of the body's tolerance, its edges' tolerances and twice its certified gaps: to an
//! edge, the distance to the edge's curve end ([`bounds::point_at_curve`]); to a face, that
//! distance plus the curve's certified gap to the face, as
//! [`crate::heal::tolerance_defects`] bounds a vertex it moves. A vertex never grows.
//!
//! Deviations: no recursion; the alternation is bounded by [`MAX_ALTERNATIONS`], the samples by
//! [`MAX_SAMPLES`], the edges by [`crate::heal::tolerance_defects::MAX_EDGES`]; cancellation is
//! polled per edge and per sample.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::LARGE_SMALL_SIZE_RATIO;
use crate::geom::vector2d::Point2;
use crate::heal::stitch::bounds;
use crate::heal::tolerance_defects::{remake_trims, MAX_EDGES};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::crv_deviation::certified_hausdorff;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::query::curve_surface_gap::curve_face_gap_bound;
use crate::surface::surface::p11::LocalSurfaceIntersectOptions;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::brep::p1;
use crate::topology::edge::{self, NotifyEnv};
use crate::topology::{vertex, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "heal::reseat";

/// How far a curve may lie from a face it bounds, as a share of its edge's tolerance (as in
/// [`crate::heal::tolerance_defects`]: SMLib holds an edge's tolerance to at least twice its gap).
pub const GAP_SHARE: f64 = 0.5;

/// The most a re-seated edge's tolerance may be, as a share of its present one: a re-fit that
/// gains less is not worth the change of geometry.
pub const GAIN: f64 = 0.5;

/// The fewest samples of an edge.
pub const MIN_SAMPLES: usize = 16;

/// The most samples of an edge.
pub const MAX_SAMPLES: usize = 1024;

/// The most alternating projections at one sample.
pub const MAX_ALTERNATIONS: usize = 64;

/// Why an edge was not re-seated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refusal {
    /// The edge does not join two different faces (a seam, a lamina or a wire edge).
    NotTwoFaces,
    /// A face has no surface.
    NoSurface(FaceId),
    /// The edge has no curve, or none with a B-spline form.
    NoCurve,
    /// A point of the curve did not drop onto a face's surface, or the surfaces' feet did not
    /// settle.
    NoFoot(FaceId),
    /// A target lies farther from the curve than the edge's tolerance and [`REACH_GAIN`] times
    /// the curve's own gap there: the surfaces meet at a grazing angle, or are not the ones the
    /// curve follows.
    FarTarget(f64),
    /// The new curve's certified gap does not give a tolerance at most [`GAIN`] times the
    /// present one: the bound, or `None` when none could be found.
    NoGain(Option<f64>),
    /// How far the new curve lies from the old one could not be bounded (an old curve that is
    /// rational, say).
    NotEnclosed,
    /// The curve already lies on its faces, and its tolerance, which the check takes, is kept
    /// as the file states it.
    AlreadyOnFaces,
}

/// An edge re-seated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reseated {
    /// The edge.
    pub edge: EdgeId,
    /// Its tolerance before.
    pub before: f64,
    /// Its tolerance after.
    pub after: f64,
    /// The new curve's certified gap to its faces.
    pub gap: f64,
    /// How far the curve moved: a certified bound on the distance between the old and the new
    /// curve at every parameter (0 when the curve was kept).
    pub moved: f64,
    /// The samples the new curve interpolates; 0 when the curve was kept (already on its faces)
    /// and only the tolerance shrank.
    pub samples: usize,
}

/// What [`reseat_edges`] did: the edges re-seated, those refused with why, and the vertices
/// whose tolerances shrank after them (each with its tolerance before and after).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReseatReport {
    /// Re-seated edges, in edge order.
    pub reseated: Vec<Reseated>,
    /// Refused edges, in edge order.
    pub refused: Vec<(EdgeId, Refusal)>,
    /// Vertices of re-seated edges whose tolerances shrank: the vertex, before, after.
    pub vertices: Vec<(VertexId, f64, f64)>,
}

/// The B-spline that [`curve_face_gap_bound`] encloses for `c`: a line's own B-spline part, a
/// B-spline curve itself.
fn b_spline_of(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(s) => Some(s),
        CurveKind::Line(l) => Some(&l.base),
        _ => None,
    }
}

/// `s` without its weights when they are all one weight (a curve read with unit weights is
/// stored rational): the same curve, which the deviation bounds take. `None` for a curve whose
/// weights differ, or without a NURBS form.
fn polynomial(s: &BSplineCurve) -> Option<BSplineCurve> {
    let weights: Vec<f64> = s.nurb_ref().ok()?.cpts().iter().map(|p| p.w).collect();
    let first = *weights.first()?;
    if weights.iter().any(|&w| w.to_bits() != first.to_bits()) {
        return None;
    }
    let mut copy = s.clone();
    copy.make_non_rational().ok()?;
    Some(copy)
}

/// The tolerance from which the check refuses an edge's as too wide (Test #59) and holds a
/// vertex's to a clearance proof (Test #82): [`LARGE_SMALL_SIZE_RATIO`] times the body's.
fn wide(b: &Brep) -> f64 {
    LARGE_SMALL_SIZE_RATIO * b.tolerance()
}

fn failed(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

fn dist(a: Point3, b: Point3) -> f64 {
    (a - b).length()
}

/// A face's surface and its parameter box.
struct Side<'a> {
    face: FaceId,
    surface: &'a Surface,
    domain: Extent2d,
}

/// The point of `s` nearest `p`, from `guess`: its parameters and the point.
fn foot(
    k: &Kernel,
    s: &Side<'_>,
    p: Point3,
    guess: Option<Point2>,
) -> Result<Option<(Point2, Point3)>> {
    let opts = DropPointOptions {
        guess_uv: guess,
        ..DropPointOptions::default()
    };
    let drop = s.surface.drop_point::<NoObject>(k, p, &s.domain, &opts)?;
    if !drop.success {
        return Ok(None);
    }
    let q = s.surface.evaluate_point(k, drop.result_uv)?;
    Ok(Some((drop.result_uv, q)))
}

/// A sample's target point, with the feet's parameters on the two sides (to seed the next) and
/// how far the curve's point lay from the farther of its two feet (its gap there).
type Target = (Point3, [Option<Point2>; 2], f64);

/// How far a target may lie from the curve, as a multiple of the curve's own gap there (when
/// that is past the edge's tolerance): the point of `S1 ∩ S2` nearest a point off both
/// surfaces lies within a few times its gap of it unless the surfaces meet at a grazing angle,
/// where the exact edge is not defined that finely.
pub const REACH_GAIN: f64 = 4.0;

/// The target for curve point `p` with tangent `tangent`, from the feet `uv` on the two sides:
/// a point of `S1 ∩ S2` in the plane through `p` normal to the curve where the surfaces cross
/// within `reach` of `p` (or [`REACH_GAIN`] times `p`'s own gap, if larger), else the midpoint
/// of the feet of a common normal. Also the feet's parameters, to seed the
/// next sample. `Err(face)` when a side gave no foot.
fn target(
    k: &Kernel,
    sides: &[Side<'_>; 2],
    p: Point3,
    tangent: Vec3,
    uv: [Option<Point2>; 2],
    tol: f64,
    reach: f64,
) -> Result<std::result::Result<Target, FaceId>> {
    let [s1, s2] = sides;
    let Some((uv1, a)) = foot(k, s1, p, uv[0])? else {
        return Ok(Err(s1.face));
    };
    let Some((uv2, b)) = foot(k, s2, p, uv[1])? else {
        return Ok(Err(s2.face));
    };
    let own = dist(p, a).max(dist(p, b));
    let reach = reach.max(REACH_GAIN * own);
    let opts = LocalSurfaceIntersectOptions {
        plane_normal: tangent.unitize().ok(),
        previous_uv: None,
        do_boundary_testing: false,
    };
    let crossing = s1
        .surface
        .local_surface_intersect(k, &s1.domain, s2.surface, &s2.domain, tol, uv1, uv2, &opts)
        .ok()
        .flatten();
    if let Some(x) = crossing {
        let q1 = s1.surface.evaluate_point(k, x.uv)?;
        let q2 = s2.surface.evaluate_point(k, x.other_uv)?;
        let q = q1 + (q2 - q1) * 0.5;
        // Nearly tangent surfaces cross far along the normal plane, where the exact edge is
        // not: a crossing past the edge's tolerance is not taken.
        if dist(q1, q2) <= tol && dist(p, q) <= reach {
            return Ok(Ok((q, [Some(x.uv), Some(x.other_uv)], own)));
        }
    }
    // Two surfaces that do not cross here, or nearly tangent ones: alternate projections to a
    // common normal.
    let (mut uv1, mut a, mut uv2, mut b) = (uv1, a, uv2, b);
    for _ in 0..MAX_ALTERNATIONS {
        let Some((nuv2, nb)) = foot(k, s2, a, Some(uv2))? else {
            return Ok(Err(s2.face));
        };
        let Some((nuv1, na)) = foot(k, s1, nb, Some(uv1))? else {
            return Ok(Err(s1.face));
        };
        let settled = dist(na, a) <= tol * 1e-3 && dist(nb, b) <= tol * 1e-3;
        (uv1, a, uv2, b) = (nuv1, na, nuv2, nb);
        if settled {
            break;
        }
    }
    Ok(Ok((a + (b - a) * 0.5, [Some(uv1), Some(uv2)], own)))
}

/// The curve through the targets of `n + 1` samples of edge `e`'s curve over its interval, its
/// ends kept, with each sample's parameter.
fn refit(
    k: &Kernel,
    curve: &Curve,
    lo: f64,
    hi: f64,
    sides: &[Side<'_>; 2],
    n: usize,
    reach: f64,
    tol: f64,
) -> Result<std::result::Result<BSplineCurve, Refusal>> {
    let mut params = Vec::new();
    let mut points = Vec::new();
    let mut uv = [None, None];
    for i in 0..=n {
        k.check_cancelled(OP)?;
        let t = if i == n {
            hi
        } else {
            lo + (hi - lo) * (i as f64 / n as f64)
        };
        let derivs = curve.evaluate(k, t, 1, false, true)?;
        let (Some(&p), Some(&d)) = (derivs.first(), derivs.get(1)) else {
            return Err(failed("an evaluation without its derivative"));
        };
        let p = Point3::from(p);
        if i == 0 || i == n {
            params.push(t);
            points.push(p);
            continue;
        }
        let (q, seeds, own) = match target(k, sides, p, d, uv, tol, reach)? {
            Ok(found) => found,
            Err(face) => return Ok(Err(Refusal::NoFoot(face))),
        };
        let off = dist(p, q);
        if off > reach.max(REACH_GAIN * own) {
            return Ok(Err(Refusal::FarTarget(off)));
        }
        uv = seeds;
        params.push(t);
        points.push(q);
    }
    let fitted = BSplineCurve::create_interpolating_curve_with_params(
        ParamMethod::ChordLength,
        3,
        3,
        &points,
        &[],
        None,
        false,
        Some(&params),
    )?;
    Ok(Ok(fitted))
}

/// Re-seats edge `e` (module docs): its curve fitted again between its two faces' surfaces, its
/// ends kept, certified within [`GAP_SHARE`] of a tolerance at most [`GAIN`] times the edge's
/// present one, which the edge then takes; its trims made again. `Ok(Err(Refusal))`, with nothing
/// changed, when a proof is missing. Run inside an edit
/// ([`crate::topology::brep::p3::edit_brep`]).
///
/// # Errors
///
/// Those of the evaluations, the fit, the bound's reads, the curve's replacement and the trims'
/// remaking; a stop of the kernel's work.
pub fn reseat_edge(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    e: EdgeId,
) -> Result<std::result::Result<Reseated, Refusal>> {
    let before = edge::tolerance(b, e)?;
    let floor = b.tolerance();
    let faces = edge::faces(b, e, None)?;
    let (f1, f2) = match faces.as_slice() {
        [f1, f2] if f1 != f2 => (*f1, *f2),
        _ => return Ok(Err(Refusal::NotTwoFaces)),
    };
    let Some(curve) = edge::create_trimmed_nurbs_curve(k, b, e)? else {
        return Ok(Err(Refusal::NoCurve));
    };
    let ivl = edge::interval(b, e)?;
    let (lo, hi) = (ivl.min(), ivl.max());
    if !(lo.is_finite() && hi.is_finite() && hi > lo) {
        return Ok(Err(Refusal::NoCurve));
    }
    // A curve already on its faces keeps its geometry: only its tolerance shrinks to the gap.
    if let Some(own) = b_spline_of(&curve) {
        let mut gap: Option<f64> = Some(0.0);
        for f in [f1, f2] {
            let g = curve_face_gap_bound(k, b, f, own, [lo, hi], floor / 100.0)?;
            gap = match (gap, g) {
                (Some(a), Some(c)) => Some(a.max(c)),
                _ => None,
            };
        }
        if let Some(g) = gap {
            let after = (g / GAP_SHARE).max(floor);
            if after <= GAIN * before {
                // A tolerance stated by the file is its accuracy, and is kept, unless the check
                // refuses it as too wide (Test #59).
                if before < wide(b) {
                    return Ok(Err(Refusal::AlreadyOnFaces));
                }
                b.edge_mut(e)?.tolerance = after;
                return Ok(Ok(Reseated {
                    edge: e,
                    before,
                    after,
                    gap: g,
                    moved: 0.0,
                    samples: 0,
                }));
            }
        }
    }
    // The approximation tolerance of the targets: well under the body's tolerance, so the fit,
    // not the targets, decides the gap.
    let tol = floor * 0.1;
    let mut n = MIN_SAMPLES;
    loop {
        let fitted = {
            let side = |f: FaceId| -> Result<std::result::Result<Side<'_>, Refusal>> {
                let rec = b.face(f)?;
                Ok(match rec.surface.as_ref() {
                    Some(surface) => Ok(Side {
                        face: f,
                        surface,
                        domain: rec.uv_domain,
                    }),
                    None => Err(Refusal::NoSurface(f)),
                })
            };
            let sides = match (side(f1)?, side(f2)?) {
                (Ok(s1), Ok(s2)) => [s1, s2],
                (Err(why), _) | (_, Err(why)) => return Ok(Err(why)),
            };
            match refit(k, &curve, lo, hi, &sides, n, before, tol)? {
                Ok(fitted) => fitted,
                Err(why) => return Ok(Err(why)),
            }
        };
        let mut gaps = Vec::new();
        let mut gap: Option<f64> = Some(0.0);
        for f in [f1, f2] {
            let g = curve_face_gap_bound(k, b, f, &fitted, [lo, hi], floor / 100.0)?;
            gaps.push((f, g.unwrap_or(f64::INFINITY)));
            gap = match (gap, g) {
                (Some(a), Some(c)) => Some(a.max(c)),
                _ => None,
            };
        }
        let after = gap.map(|g| (g / GAP_SHARE).max(floor));
        if let (Some(g), Some(after)) = (gap, after) {
            if after <= GAIN * before {
                let moved = match (b_spline_of(&curve).and_then(polynomial), fitted.nurb_ref()) {
                    (Some(old), Ok(new)) => old
                        .nurb_ref()
                        .and_then(|old| certified_hausdorff(old, [lo, hi], new, [lo, hi]))
                        .ok(),
                    _ => None,
                };
                let Some(moved) = moved else {
                    return Ok(Err(Refusal::NotEnclosed));
                };
                edge::replace_3d_curve(b, e, Some(Curve::from(fitted)), env)?;
                b.edge_mut(e)?.tolerance = after;
                remake_trims(k, b, e, &gaps)?;
                return Ok(Ok(Reseated {
                    edge: e,
                    before,
                    after: edge::tolerance(b, e)?,
                    gap: g,
                    moved,
                    samples: n,
                }));
            }
        }
        if n >= MAX_SAMPLES {
            return Ok(Err(Refusal::NoGain(gap)));
        }
        n = n.saturating_mul(2).min(MAX_SAMPLES);
    }
}

/// [`reseat_edge`] on every edge of `b` whose tolerance is above `threshold` times the body's,
/// in edge order: what was re-seated and what was refused. Run inside an edit.
///
/// # Errors
///
/// As [`reseat_edge`]; `InvalidInput` past [`MAX_EDGES`] edges.
pub fn reseat_edges(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    threshold: f64,
) -> Result<ReseatReport> {
    let edges = p1::edges(b, None)?;
    if edges.len() > MAX_EDGES {
        return Err(failed("more edges than one heal looks at"));
    }
    let floor = b.tolerance();
    let mut report = ReseatReport::default();
    let mut touched = Vec::new();
    for e in edges {
        k.check_cancelled(OP)?;
        if edge::tolerance(b, e)? <= threshold * floor {
            continue;
        }
        match reseat_edge(k, b, env, e)? {
            Ok(r) => {
                let (s, t) = edge::start_end_vertices(b, e)?;
                touched.extend([s, t].into_iter().flatten());
                report.reseated.push(r);
            }
            Err(why) => report.refused.push((e, why)),
        }
    }
    touched.sort_unstable();
    touched.dedup();
    for v in touched {
        k.check_cancelled(OP)?;
        let before = vertex::tolerance(b, v)?;
        // as for an edge: a vertex tolerance the check takes is kept as the file states it
        if before < wide(b) {
            continue;
        }
        if let Some(after) = vertex_need(k, b, v)? {
            if after < before {
                b.vertex_mut(v)?.tolerance = after;
                report.vertices.push((v, before, after));
            }
        }
    }
    Ok(report)
}

/// The smallest tolerance vertex `v` can be certified to need, `None` when a bound is missing:
/// at least the body's and each of its edges' tolerance, and twice ([`GAP_SHARE`]) each of its
/// certified gaps. A gap to an edge is the distance from `v` to the edge's curve end there
/// ([`bounds::point_at_curve`]); a gap to a face is that distance plus the curve's certified gap
/// to the face ([`curve_face_gap_bound`]), the smallest over the vertex's edges on the face, as
/// [`crate::heal::tolerance_defects`] bounds a vertex it moves.
fn vertex_need(k: &Kernel, b: &Brep, v: VertexId) -> Result<Option<f64>> {
    let p = vertex::point(b, v)?;
    let floor = b.tolerance();
    let mut need = floor;
    let mut to_face: Vec<(FaceId, f64)> = Vec::new();
    for n in vertex::edges(b, v, None)? {
        need = need.max(edge::tolerance(b, n)?);
        let Some(c) = edge::create_trimmed_nurbs_curve(k, b, n)? else {
            return Ok(None);
        };
        let Some(spline) = b_spline_of(&c) else {
            return Ok(None);
        };
        let range = c.natural_interval();
        let (s, t) = edge::start_end_vertices(b, n)?;
        for (x, at) in [(s, range.min()), (t, range.max())] {
            if x != Some(v) {
                continue;
            }
            let Ok(near) = bounds::point_at_curve(p, &c, at) else {
                return Ok(None);
            };
            need = need.max(near / GAP_SHARE);
            for f in edge::faces(b, n, None)? {
                let Some(g) = curve_face_gap_bound(
                    k,
                    b,
                    f,
                    spline,
                    [range.min(), range.max()],
                    floor / 100.0,
                )?
                else {
                    return Ok(None);
                };
                let off = crate::geom::certified::next_up(g + near);
                match to_face.iter_mut().find(|(g, _)| *g == f) {
                    Some((_, d)) => *d = d.min(off),
                    None => to_face.push((f, off)),
                }
            }
        }
    }
    for (_, d) in to_face {
        need = need.max(d / GAP_SHARE);
    }
    Ok(need.is_finite().then_some(need))
}
