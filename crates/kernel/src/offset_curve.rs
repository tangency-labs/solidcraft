//! Offset of a curve along a face: the curve whose every point is a given distance from a source
//! curve on the face, measured along the face. Each point is the end of the geodesic (the
//! straightest path on the surface) of that length leaving the source square to it, so the band
//! between the source and its offset has the same width along the face everywhere.
//!
//! The geometry is the kernel's: `solidcore::curve::geodesic_offset`, through
//! `solidcore_ops::curve::offset_on_surface_over`, on the face alone as a one-face sheet. It
//! integrates the geodesic equation in the surface's own (u, v) with its metric, fits a cubic
//! B-spline through the geodesics' ends and measures it against the geodesic shot at the middle of
//! every span, doubling the stations until that deviation is within the tolerance. On a plane the
//! geodesics are straight lines (the ordinary offset); on a developable face (a cylinder, cone or
//! extrusion) they unroll to straight lines, so the ends are where the offset in the unrolled flat
//! maps back to.
//!
//! This module picks the source (the face's edge nearest a point, with the side the face is on;
//! or curves lying on the face, with the side a point is on), calls the kernel, and cuts the
//! result to the face: the kernel keeps the geodesics on the face's surface box, and the parts of
//! the offset outside the face's own boundary (its trims, as the isoparametric curves read them)
//! are cut off at that boundary. An offset that leaves the face everywhere, a source not on the
//! face, and the kernel's refusals (a geodesic leaving the surface, a singular point, an offset
//! that folds over itself, a fit past the tolerance) are refused.

use solidcore_ops::curve::OffsetSide;
use solidcore_record::Vec3;
use solidcore_record::brep;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::iso::FaceTrims;
use crate::{KernelError, Result};

/// Points along a curve the source and the offset are read at.
const SAMPLES: usize = 128;
/// Bisection steps that find where the offset crosses the face's boundary.
const EDGE_STEPS: usize = 48;

/// What the offset is taken from.
#[derive(Clone, Copy, Debug)]
pub enum OffsetFrom<'a> {
    /// The face's edge nearest the point.
    Edge(Vec3),
    /// Curves lying on the face, each its exact record curve on its interval.
    Curves(&'a [(brep::Curve, [f64; 2])]),
}

/// An offset of a curve along a face ([`offset_on_face`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceOffset {
    /// The offset curve's pieces, each its record curve on its interval, in the source's order.
    pub curves: Vec<(brep::Curve, [f64; 2])>,
    /// The largest distance (mm) found between the curves and the ends of geodesics shot between
    /// the ones they were made through: measured, not proved.
    pub deviation: f64,
    /// The offset was cut where it left the face's boundary.
    pub clipped: bool,
}

/// One source curve on its range, and the side to offset to.
struct Source {
    curve: brep::Curve,
    range: [f64; 2],
    side: OffsetSide,
}

/// The offset of `from` on face `face` of `body` by `distance` (mm, > 0) measured along the face,
/// within `tolerance` mm. From a face's edge the offset goes into the face; from curves lying on
/// the face it goes to the side `toward` (a point on the face) lies on. See the module notes for
/// what is cut and what is refused.
pub fn offset_on_face(body: &Body, face: usize, from: OffsetFrom<'_>, distance: f64, toward: Vec3, tolerance: f64) -> Result<FaceOffset> {
    offset_on_face_to(body, face, from, distance, toward, tolerance, false)
}

/// [`offset_on_face`], to the other side of curves lying on the face than `toward` when `flip`
/// (the side point stays where it was picked; the offset goes the other way). From a face's edge
/// the offset always goes into the face, so `flip` is refused there.
pub fn offset_on_face_to(
    body: &Body,
    face: usize,
    from: OffsetFrom<'_>,
    distance: f64,
    toward: Vec3,
    tolerance: f64,
    flip: bool,
) -> Result<FaceOffset> {
    if flip && matches!(from, OffsetFrom::Edge(_)) {
        return Err(KernelError::Invalid(
            "offset curve: from an edge of the face the curve always goes into the face; Flip is for a curve lying on the face".into(),
        ));
    }
    if !(distance.is_finite() && distance > 0.0) {
        return Err(KernelError::Invalid("offset curve: the distance must be a positive number".into()));
    }
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(KernelError::Invalid("offset curve: the tolerance must be a positive number".into()));
    }
    let trims = FaceTrims::new(body, face)?;
    let sheet = crate::surfaces::face_sheet(body, face)?;
    let size = body.size().max(1e-9);
    let on = OnFace { trims: &trims, accept: size * 1e-6 + 1e-6, eps: trims.widths(size * 1e-6 + 1e-7) };
    let sources = match from {
        OffsetFrom::Edge(at) => {
            let mut s = edge_source(body, face, at, size)?;
            s.side = on.edge_side(&s, sheet.record(), (size * 1e-3).min(0.5 * distance))?;
            vec![s]
        }
        OffsetFrom::Curves(curves) => {
            if curves.is_empty() {
                return Err(KernelError::Invalid("offset curve: no curve to offset".into()));
            }
            let mut out = Vec::new();
            for (curve, range) in curves {
                on.check_on_face(curve, *range)?;
                let side = curve_side(curve, *range, sheet.record(), toward, distance, size)?;
                let side = match (flip, side) {
                    (false, side) => side,
                    (true, OffsetSide::Left) => OffsetSide::Right,
                    (true, OffsetSide::Right) => OffsetSide::Left,
                };
                out.push(Source { curve: curve.clone(), range: *range, side });
            }
            out
        }
    };
    let mut out = FaceOffset { curves: Vec::new(), deviation: 0.0, clipped: false };
    for s in sources {
        let offset = |range: [f64; 2]| solidcore_ops::curve::offset_on_surface_over(&s.curve, range, sheet.record(), distance, s.side, tolerance);
        let offsets = match offset(s.range) {
            Ok(o) => vec![o],
            // Some geodesics leave the face's surface: the runs of the source whose geodesics stay.
            Err(e) if leaves_surface(&e) => {
                out.clipped = true;
                staying_runs(&s, sheet.record(), distance)?.into_iter().map(|r| offset(r).map_err(refused)).collect::<Result<Vec<_>>>()?
            }
            Err(e) => return Err(refused(e)),
        };
        for o in offsets {
            out.deviation = out.deviation.max(o.deviation);
            let range = curve_range(&o.curve).ok_or_else(|| KernelError::Failed("offset curve: the kernel's offset has no range".into()))?;
            let (pieces, clipped) = on.cut(&o.curve, range)?;
            out.clipped |= clipped;
            out.curves.extend(pieces.into_iter().map(|r| (o.curve.clone(), r)));
        }
    }
    if out.curves.is_empty() {
        return Err(KernelError::Invalid(format!("offset curve: at {distance} mm the offset runs off the face everywhere; use a smaller distance")));
    }
    Ok(out)
}

/// The kernel's refusal of a geodesic that runs out of the surface's parameter box.
fn leaves_surface(e: &solidcore_ops::Error) -> bool {
    e.to_string().contains("leaves the surface")
}

/// Runs of the source probed along it.
const PROBES: usize = 32;

/// The parts of the source whose geodesics stay on the face's surface: the source probed at
/// [`PROBES`] + 1 points (the kernel's offset of a sliver of it there), each change between
/// staying and leaving found by bisection, each run pulled in by a millionth of the source's range
/// at an end found so. Empty when every probe leaves.
fn staying_runs(s: &Source, sheet: &brep::Brep, distance: f64) -> Result<Vec<[f64; 2]>> {
    let [a, b] = s.range;
    let sliver = (b - a) * 1e-6;
    let stays = |t: f64| -> Result<bool> {
        let r = if t + sliver <= b { [t, t + sliver] } else { [t - sliver, t] };
        match solidcore_ops::curve::offset_on_surface_over(&s.curve, r, sheet, distance, s.side, distance) {
            Ok(_) => Ok(true),
            Err(e) if leaves_surface(&e) => Ok(false),
            Err(e) => Err(refused(e)),
        }
    };
    let at = |k: usize| a + (b - a) * k as f64 / PROBES as f64;
    let mut valid = Vec::with_capacity(PROBES + 1);
    for k in 0..=PROBES {
        valid.push(stays(at(k))?);
    }
    let edge = |out: f64, inn: f64| -> Result<f64> {
        let (mut out, mut inn) = (out, inn);
        for _ in 0..EDGE_STEPS / 2 {
            let mid = 0.5 * (out + inn);
            if stays(mid)? { inn = mid } else { out = mid }
        }
        // Pulled in, so the run's own end stays too.
        Ok(inn + (out - inn).signum() * -sliver)
    };
    let mut runs = Vec::new();
    let mut k = 0;
    while k <= PROBES {
        if !valid.get(k).copied().unwrap_or(false) {
            k += 1;
            continue;
        }
        let start = k;
        while valid.get(k + 1).copied().unwrap_or(false) {
            k += 1;
        }
        let lo = if start == 0 { a } else { edge(at(start - 1), at(start))? };
        let hi = if k == PROBES { b } else { edge(at(k + 1), at(k))? };
        if hi > lo {
            runs.push([lo, hi]);
        }
        k += 1;
    }
    Ok(runs)
}

fn refused(e: solidcore_ops::Error) -> KernelError {
    match e {
        solidcore_ops::Error::Argument => KernelError::Invalid("offset curve: the kernel does not take this curve or distance".into()),
        other => KernelError::Invalid(format!("offset curve: {other}")),
    }
}

/// The face's edge nearest `at` (its nearest of 33 points along each), on its interval, with the
/// side the face lies on: the face is to the left of its loop's direction about the surface's own
/// normal, so to the left of the edge's curve where the loop runs along it, else to the right.
fn edge_source(body: &Body, face: usize, at: Vec3, size: f64) -> Result<Source> {
    let r = body.record();
    let f = r.faces.get(face).ok_or_else(|| KernelError::Invalid(format!("the body has no face {face}")))?;
    let mut best: Option<(f64, &brep::Edge, bool)> = None;
    for wire in &f.loops {
        let brep::Loop::Edges { edges } = wire else { continue };
        for use_ in edges {
            let edge = r.edges.get(use_.edge as usize).ok_or_else(|| KernelError::Invalid("offset curve: a loop names a missing edge".into()))?;
            let Ok(c) = CurveEval::new(edge.parameters()) else { continue };
            let [a, b] = edge.interval;
            for k in 0..=32 {
                let Some(p) = c.point(a + (b - a) * f64::from(k) / 32.0) else { continue };
                let d = p.dist(at);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, edge, use_.forward));
                }
            }
        }
    }
    let (d, edge, forward) = best.ok_or_else(|| KernelError::Invalid("offset curve: the face has no edges".into()))?;
    if d > size * 0.05 {
        return Err(KernelError::Invalid("offset curve: the edge point is not on an edge of the face".into()));
    }
    Ok(Source { curve: edge.parameters().clone(), range: edge.interval, side: if forward { OffsetSide::Left } else { OffsetSide::Right } })
}

/// The side of a curve on the face `toward` lies on: an offset by a small step to the left, and
/// whether its nearest point to `toward` is nearer than the curve's.
fn curve_side(curve: &brep::Curve, range: [f64; 2], sheet: &brep::Brep, toward: Vec3, distance: f64, size: f64) -> Result<OffsetSide> {
    let step = (size * 1e-3).min(0.5 * distance);
    let nearest = |c: &brep::Curve, [a, b]: [f64; 2]| -> Option<f64> {
        let e = CurveEval::new(c).ok()?;
        (0..=SAMPLES).filter_map(|k| e.point(a + (b - a) * k as f64 / SAMPLES as f64)).map(|p| p.dist(toward)).reduce(f64::min)
    };
    let here = nearest(curve, range).ok_or_else(|| KernelError::Failed("offset curve: the curve does not evaluate".into()))?;
    let on_curve = || KernelError::Invalid("offset curve: the side point lies on the curve; pick a point on the side to offset to".into());
    if here <= step {
        return Err(on_curve());
    }
    for (probe, toward_is) in [(OffsetSide::Left, [OffsetSide::Left, OffsetSide::Right]), (OffsetSide::Right, [OffsetSide::Right, OffsetSide::Left])]
    {
        let Ok(o) = solidcore_ops::curve::offset_on_surface_over(curve, range, sheet, step, probe, step * 1e-2) else { continue };
        let Some(there) = curve_range(&o.curve).and_then(|r| nearest(&o.curve, r)) else { continue };
        if (there - here).abs() <= step * 1e-3 {
            break;
        }
        let [nearer, farther] = toward_is;
        return Ok(if there < here { nearer } else { farther });
    }
    Err(on_curve())
}

/// A record curve's own parameter range: a line's [0, 1], a spline's knots'.
fn curve_range(c: &brep::Curve) -> Option<[f64; 2]> {
    match c {
        brep::Curve::Line { .. } => Some([0.0, 1.0]),
        brep::Curve::Spline { knots, .. } | brep::Curve::RationalSpline { knots, .. } => Some([*knots.first()?, *knots.last()?]),
        _ => None,
    }
}

/// The face's trims, read for points on it.
struct OnFace<'a> {
    trims: &'a FaceTrims<'a>,
    /// How far from the surface a point may lie and still be on it (mm).
    accept: f64,
    /// The (u, v) widths of the boundary's tolerance.
    eps: [f64; 2],
}

impl OnFace<'_> {
    /// The side of an edge the face is on: the side (the loop's guess first) whose offset by a
    /// small `step` along the middle half of the edge lies in the face at its middle.
    fn edge_side(&self, s: &Source, sheet: &brep::Brep, step: f64) -> Result<OffsetSide> {
        let [a, b] = s.range;
        let middle = [a + 0.25 * (b - a), b - 0.25 * (b - a)];
        let other = match s.side {
            OffsetSide::Left => OffsetSide::Right,
            OffsetSide::Right => OffsetSide::Left,
        };
        for side in [s.side, other] {
            let Ok(o) = solidcore_ops::curve::offset_on_surface_over(&s.curve, middle, sheet, step, side, step * 1e-2) else { continue };
            let Some([lo, hi]) = curve_range(&o.curve) else { continue };
            let Some(p) = CurveEval::new(&o.curve).ok().and_then(|e| e.point(0.5 * (lo + hi))) else { continue };
            if self.inside(p) {
                return Ok(side);
            }
        }
        Err(KernelError::Invalid("offset curve: the face lies on neither side of the edge".into()))
    }

    fn inside(&self, p: Vec3) -> bool {
        self.trims.uv_of(p, self.accept).is_some_and(|uv| self.trims.contains(uv, self.eps))
    }

    /// Refused unless the curve lies on the face at every sample.
    fn check_on_face(&self, curve: &brep::Curve, [a, b]: [f64; 2]) -> Result<()> {
        let c = CurveEval::new(curve).map_err(|e| KernelError::Invalid(format!("offset curve: {e}")))?;
        if !(a.is_finite() && b.is_finite() && b > a) {
            return Err(KernelError::Invalid("offset curve: the curve's interval is empty".into()));
        }
        for k in 0..=SAMPLES {
            let p = c
                .point(a + (b - a) * k as f64 / SAMPLES as f64)
                .ok_or_else(|| KernelError::Failed("offset curve: the curve does not evaluate".into()))?;
            if !self.inside(p) {
                return Err(KernelError::Invalid(format!("offset curve: the curve is not on the face (at {:.4}, {:.4}, {:.4})", p.x, p.y, p.z)));
            }
        }
        Ok(())
    }

    /// The parts of `curve` over `[a, b]` inside the face, each cut where it crosses the face's
    /// boundary (found by bisection between samples), and whether anything was cut off.
    fn cut(&self, curve: &brep::Curve, [a, b]: [f64; 2]) -> Result<(Vec<[f64; 2]>, bool)> {
        let c = CurveEval::new(curve).map_err(|e| KernelError::Failed(format!("offset curve: {e}")))?;
        let at = |k: usize| a + (b - a) * k as f64 / SAMPLES as f64;
        let inside = |t: f64| c.point(t).is_some_and(|p| self.inside(p));
        let valid: Vec<bool> = (0..=SAMPLES).map(|k| inside(at(k))).collect();
        let edge = |out: f64, inn: f64| {
            let (mut out, mut inn) = (out, inn);
            for _ in 0..EDGE_STEPS {
                let mid = 0.5 * (out + inn);
                if inside(mid) { inn = mid } else { out = mid }
            }
            inn
        };
        let mut pieces = Vec::new();
        let mut k = 0;
        while k <= SAMPLES {
            if !valid.get(k).copied().unwrap_or(false) {
                k += 1;
                continue;
            }
            let start = k;
            while valid.get(k + 1).copied().unwrap_or(false) {
                k += 1;
            }
            let lo = if start == 0 { a } else { edge(at(start - 1), at(start)) };
            let hi = if k == SAMPLES { b } else { edge(at(k + 1), at(k)) };
            if hi > lo {
                pieces.push([lo, hi]);
            }
            k += 1;
        }
        Ok((pieces, valid.iter().any(|v| !v)))
    }
}
