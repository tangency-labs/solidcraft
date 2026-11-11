//! How two faces meet across an edge, and how curves and surfaces bend: the continuity measure
//! at one parameter of an edge (G0 to G4) and the curvature vectors combs are drawn from.
//!
//! Not upstream. At a point of the edge's curve, each face's surface is reached by dropping
//! that point on it. The faces are then compared there:
//! - G0, the gap: the distance between the two feet;
//! - G1, the normal angle: the angle between the faces' unit normals, each in the sense of the
//!   face use given (its outward side, as the caller chooses it);
//! - G2 to G4, the jumps of the normal sections' curvature `κ`, its arc-length derivative and
//!   that derivative's, read across the edge: on each face along `n × t`, the direction in its
//!   tangent plane square to the edge's unit tangent `t`. They come from each surface's exact
//!   partial derivatives to the fourth order ([`crate::eval::normal_section`]); nothing is
//!   sampled.
//!
//! Where the faces' normals are opposed (a knife edge), face b's section is read in face a's
//! frame: its normal and its across direction both turn round, so its k-th derivative of
//! curvature reads `(-1)^(k + 1)` times as much there.
//!
//! [`curve_curvature`] (an edge's curve, through [`curvature_of`] any curve) and [`iso_curvature`]
//! give the teeth of a comb: a curve's curvature vector `κ N`, and a surface's normal curvature along one of its parameter lines
//! times its normal, `(n · S_tt / |S_t|²) n`.
//!
//! What is not proved: the drops find the nearest point of each surface from the given guess
//! (a gap is never smaller than the true one only where the drop found the nearest foot), and
//! the values are evaluated in floating point with no bound on their error.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::eval::normal_section::normal_section;
use crate::eval::{Partials, MAX_PARTIAL_ORDER};
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use crate::query::brep as qbrep;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::{Brep, EdgeId, FaceId, FaceuseId};

const OP: &str = "query::continuity";

/// The continuity measure across an edge at one parameter of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuityAt {
    /// The edge curve's point.
    pub point: Point3,
    /// Each face's foot of `point` (face a, then face b).
    pub feet: [Point3; 2],
    /// Each foot's surface parameters: the guesses of the next station along the edge.
    pub uv: [Point2; 2],
    /// G0: the distance between the feet.
    pub gap: f64,
    /// G1: the angle between the faces' unit normals in their uses' senses, radians.
    pub normal_angle: f64,
    /// G2 to G4: the jumps across the edge of the normal sections' curvature (1/mm), of its
    /// arc-length derivative (1/mm²) and of that derivative's (1/mm³).
    pub jumps: [f64; 3],
}

/// A tooth of a curvature comb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurvatureAt {
    /// Where the tooth stands.
    pub point: Point3,
    /// The curvature vector, 1/mm: towards the centre of curvature, its length the curvature.
    pub curvature: Vec3,
}

/// Whether every coordinate of `v` is finite.
fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// The sign of a face use against its surface's normal.
fn sense(b: &Brep, fu: FaceuseId, f: FaceId) -> Result<f64> {
    let u = b.faceuse(fu)?;
    if u.face != Some(f) {
        return Err(SolidError::invalid_input(OP, "a faceuse of another face"));
    }
    match u.orientation {
        OrientType::Same => Ok(1.0),
        OrientType::Opposite => Ok(-1.0),
        OrientType::Unknown => Err(SolidError::Unsupported {
            operation: OP,
            reason: "a faceuse whose orientation is unknown has no sense",
        }),
    }
}

/// The surface of `f`.
fn surface_of(b: &Brep, f: FaceId) -> Result<&Surface> {
    b.face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "face without a surface"))
}

/// The partial derivatives of `s` at `uv` to the fourth order, exact (no tangent replaced).
fn partials(k: &Kernel, s: &Surface, uv: Point2) -> Result<Partials> {
    let flags = EvaluateFlags {
        only_upper_half: true,
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let d = s.evaluate(k, uv, MAX_PARTIAL_ORDER, MAX_PARTIAL_ORDER, flags)?;
    let mut out: Partials = [[Vec3::ZERO; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            if i + j <= MAX_PARTIAL_ORDER {
                *x = d.get(i).and_then(|r| r.get(j)).copied().ok_or_else(|| {
                    SolidError::query_failed(
                        OP,
                        "the surface evaluation has fewer partials than asked",
                    )
                })?;
            }
        }
    }
    Ok(out)
}

/// One face at the station: its foot, unit normal in its use's sense and parameters.
struct Side {
    foot: Point3,
    normal: Vec3,
    uv: Point2,
}

/// `p` dropped on face `f`'s surface (from `guess`), with the normal there in the sense `sign`;
/// `None` where the drop fails or the normal is zero.
fn side(
    k: &Kernel,
    s: &Surface,
    sign: f64,
    p: Point3,
    guess: Option<Point2>,
) -> Result<Option<Side>> {
    let domain = s.natural_uv_domain_with_nurb(k)?;
    let drop = s.drop_point::<NoObject>(
        k,
        p,
        &domain,
        &DropPointOptions {
            guess_uv: guess,
            ..DropPointOptions::default()
        },
    )?;
    if !drop.success {
        return Ok(None);
    }
    let uv = drop.result_uv;
    let foot = s.evaluate_point(k, uv)?;
    let n = qbrep::evaluate_surface_normal(k, Some(s), uv)?;
    let len = n.length();
    if !(len > 0.0 && len.is_finite() && finite(foot)) {
        return Ok(None);
    }
    Ok(Some(Side {
        foot,
        normal: n * (sign / len),
        uv,
    }))
}

/// The continuity measure across edge `e` at its curve's parameter `t`, between the faces of
/// `faceuses` (face a's use, then face b's), each normal in its use's sense. `guess`: each
/// face's parameters to drop from (the last station's), or none. `None` where the station says
/// nothing: the curve has no tangent there, a drop fails, a normal is zero, the edge runs along
/// a normal, or a normal section is degenerate (a pole).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge without a curve, a face without a surface, a
/// faceuse of no face, or a `t` that is not finite; [`SolidError::Unsupported`] for a faceuse
/// whose orientation is unknown; those of the evaluations and the drops.
pub fn continuity_at(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    faceuses: [FaceuseId; 2],
    t: f64,
    guess: [Option<Point2>; 2],
) -> Result<Option<ContinuityAt>> {
    if !t.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "an edge parameter that is not finite",
        ));
    }
    let curve = b
        .edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "edge without a curve"))?;
    let d = curve.evaluate(k, t, 1, true, false)?;
    let (Some(&point), Some(&d1)) = (d.first(), d.get(1)) else {
        return Err(SolidError::query_failed(
            OP,
            "the curve evaluation has no first derivative",
        ));
    };
    let speed = d1.length();
    if !(speed > 0.0 && speed.is_finite() && finite(point)) {
        return Ok(None);
    }
    let tangent = d1 * (1.0 / speed);
    let mut sides = Vec::with_capacity(2);
    let mut jets = Vec::with_capacity(2);
    for (&fu, g) in faceuses.iter().zip(guess) {
        let f = b
            .faceuse(fu)?
            .face
            .ok_or_else(|| SolidError::invalid_input(OP, "a faceuse of no face"))?;
        let sign = sense(b, fu, f)?;
        let s = surface_of(b, f)?;
        let Some(one) = side(k, s, sign, point, g)? else {
            return Ok(None);
        };
        // across the edge, in the face's tangent plane
        let across = one.normal.cross(tangent);
        let len = across.length();
        if !(len > 0.0 && len.is_finite()) {
            return Ok(None);
        }
        let d = partials(k, s, one.uv)?;
        let Some(jet) = normal_section(&d, one.normal, across * (1.0 / len)) else {
            return Ok(None);
        };
        sides.push(one);
        jets.push(jet.values());
    }
    let (Some(a), Some(bb), Some(ja), Some(jb)) =
        (sides.first(), sides.get(1), jets.first(), jets.get(1))
    else {
        return Err(SolidError::query_failed(
            OP,
            "the edge has fewer than two sides",
        ));
    };
    let opposed = a.normal.dot(bb.normal) < 0.0;
    let mut jumps = [0.0_f64; 3];
    for (kk, ((j, x), y)) in jumps.iter_mut().zip(ja).zip(jb).enumerate() {
        let flip = if opposed && kk % 2 == 0 { -1.0 } else { 1.0 };
        *j = (x - flip * y).abs();
    }
    // clamp: constant bounds, the lower below the upper (cannot panic)
    let normal_angle = a.normal.dot(bb.normal).clamp(-1.0, 1.0).acos();
    Ok(Some(ContinuityAt {
        point,
        feet: [a.foot, bb.foot],
        uv: [a.uv, bb.uv],
        gap: a.foot.distance(bb.foot),
        normal_angle,
        jumps,
    }))
}

/// The curvature vector `κ N` of edge `e`'s curve at its parameter `t`: [`curvature_of`] its
/// curve. `None` where the curve has no tangent there or a value is not finite.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge without a curve or a `t` that is not finite; those
/// of the evaluation.
pub fn curve_curvature(k: &Kernel, b: &Brep, e: EdgeId, t: f64) -> Result<Option<CurvatureAt>> {
    let curve = b
        .edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "edge without a curve"))?;
    curvature_of(k, curve, t)
}

/// The curvature vector `κ N` of `curve` at its parameter `t`, from the curve's first two
/// derivatives: `(C' × C'') × C' / |C'|⁴` (zero where the curve is straight). `None` where the
/// curve has no tangent there or a value is not finite.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `t` that is not finite; those of the evaluation.
pub fn curvature_of(k: &Kernel, curve: &Curve, t: f64) -> Result<Option<CurvatureAt>> {
    if !t.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "a curve parameter that is not finite",
        ));
    }
    let d = curve.evaluate(k, t, 2, true, false)?;
    let (Some(&point), Some(&d1), Some(&d2)) = (d.first(), d.get(1), d.get(2)) else {
        return Err(SolidError::query_failed(
            OP,
            "the curve evaluation has no second derivative",
        ));
    };
    let s2 = d1.dot(d1);
    if !(s2 > 0.0) {
        return Ok(None);
    }
    let curvature = d1.cross(d2).cross(d1) * (1.0 / (s2 * s2));
    Ok((finite(point) && finite(curvature)).then_some(CurvatureAt { point, curvature }))
}

/// Face `f`'s surface's normal curvature along its `u` parameter line (`along_u`) or its `v`
/// line through `uv`, times its unit normal: `(n · S_tt / |S_t|²) n`, `n` the unit normal of
/// `S_u × S_v` (the surface's own sense). `None` at a pole (no tangent along the line, or no
/// normal) or where a value is not finite.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without a surface; those of the evaluation.
pub fn iso_curvature(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    uv: Point2,
    along_u: bool,
) -> Result<Option<CurvatureAt>> {
    let s = surface_of(b, f)?;
    let flags = EvaluateFlags {
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let d = s.evaluate(k, uv, 2, 2, flags)?;
    let at = |i: usize, j: usize| {
        d.get(i).and_then(|r| r.get(j)).copied().ok_or_else(|| {
            SolidError::query_failed(OP, "the surface evaluation has fewer partials than asked")
        })
    };
    let (point, su, sv) = (at(0, 0)?, at(1, 0)?, at(0, 1)?);
    let (along, bend) = if along_u {
        (su, at(2, 0)?)
    } else {
        (sv, at(0, 2)?)
    };
    let speed2 = along.dot(along);
    let n = su.cross(sv);
    let len = n.length();
    if !(speed2 > 0.0 && len > 0.0) {
        return Ok(None);
    }
    let n = n * (1.0 / len);
    let curvature = n * (n.dot(bend) / speed2);
    Ok((finite(point) && finite(curvature)).then_some(CurvatureAt { point, curvature }))
}

/// One station of [`continuity_along`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuityStation {
    /// How far along the edge's parameter: 0 at its start, 1 at its end.
    pub edge_fraction: f64,
    /// The measure there.
    pub measure: ContinuityAt,
    /// [`ContinuityAt::normal_angle`] in degrees, for display.
    pub normal_angle_deg: f64,
}

/// The fractions of `n` stations over a parameter interval, both ends included; a closed
/// curve's end is its start, taken once.
fn fractions(n: usize, closed: bool) -> impl Iterator<Item = f64> {
    let steps = if closed {
        n
    } else {
        n.saturating_sub(1).max(1)
    };
    (0..n).map(move |i| i as f64 / steps as f64)
}

/// [`continuity_at`] at `stations` even steps of edge `e`'s interval, its ends included, each
/// station's drops starting from the last one's feet. Stations that say nothing are left out.
///
/// # Errors
///
/// Those of [`continuity_at`].
pub fn continuity_along(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    faceuses: [FaceuseId; 2],
    stations: usize,
) -> Result<Vec<ContinuityStation>> {
    let interval = b.edge(e)?.interval;
    let mut out = Vec::new();
    let mut guess: [Option<Point2>; 2] = [None, None];
    for f in fractions(stations, false) {
        let Some(m) = continuity_at(k, b, e, faceuses, interval.evaluate(f), guess)? else {
            continue;
        };
        guess = [Some(m.uv[0]), Some(m.uv[1])];
        out.push(ContinuityStation {
            edge_fraction: f,
            measure: m,
            normal_angle_deg: m.normal_angle.to_degrees(),
        });
    }
    Ok(out)
}

/// The comb of edge `e`: [`curve_curvature`] at `teeth` even steps of its interval, the ends
/// included (a closed edge's end is its start, once). Stations with no tangent are left out.
///
/// # Errors
///
/// Those of [`curve_curvature`] and the topology lookups.
pub fn edge_comb(k: &Kernel, b: &Brep, e: EdgeId, teeth: usize) -> Result<Vec<CurvatureAt>> {
    let edge = b.edge(e)?;
    let curve = edge
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "edge without a curve"))?;
    let (v0, v1) = crate::topology::edge::start_end_vertices(b, e)?;
    let closed = v0.is_some() && v0 == v1;
    curve_comb(k, curve, edge.interval, teeth, closed)
}

/// The comb of `curve` over `interval` of its parameter: [`curvature_of`] at `teeth` even
/// steps, the ends included (a `closed` curve's end is its start, once). Stations with no
/// tangent are left out.
///
/// # Errors
///
/// Those of [`curvature_of`].
pub fn curve_comb(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    teeth: usize,
    closed: bool,
) -> Result<Vec<CurvatureAt>> {
    let mut out = Vec::new();
    for f in fractions(teeth, closed) {
        if let Some(c) = curvature_of(k, curve, interval.evaluate(f))? {
            out.push(c);
        }
    }
    Ok(out)
}

/// The comb along face `f`'s surface's `u` line (`along_u`) at `v = fixed`, or its `v` line
/// at `u = fixed`, over `span` of the running parameter: [`iso_curvature`] at `teeth` even
/// steps, the ends included. Stations at a pole are left out.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a parameter that is not finite; those of
/// [`iso_curvature`].
pub fn iso_comb(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    along_u: bool,
    fixed: f64,
    span: [f64; 2],
    teeth: usize,
) -> Result<Vec<CurvatureAt>> {
    let [s0, s1] = span;
    if !(fixed.is_finite() && s0.is_finite() && s1.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "a parameter that is not finite",
        ));
    }
    let mut out = Vec::new();
    for frac in fractions(teeth, false) {
        let s = s0 + (s1 - s0) * frac;
        let uv = if along_u {
            Point2::new(s, fixed)
        } else {
            Point2::new(fixed, s)
        };
        if let Some(c) = iso_curvature(k, b, f, uv, along_u)? {
            out.push(c);
        }
    }
    Ok(out)
}
