//! Whether a body is a uniform prism along a frame's normal, proved on its current geometry:
//! two caps square to the normal, every other face a wall running straight between them and
//! standing on both caps' edges, the two caps one section. Its section, its levels with their
//! bound, and an enclosure of its volume. And whether two bodies are proved the same prism.
//!
//! A query (owner rule 13564): it answers with a proved bound or not at all (`None`, never a
//! guess). What it proves:
//! - **Caps:** plane labels square to the normal (within the tolerance over the body's size),
//!   every cap vertex on its plane and its level within twice the tolerance (with the rounding of
//!   the plane's normal); each cap loop read exactly ([`super::read::planar_pieces`]), lines and
//!   arcs, its signed area enclosed.
//! - **Walls:** a plane label with its normal square to the direction; a cylinder label with its
//!   axis along it; an extrusion along it; or a net ruled along it (degree one across, each
//!   pair of rows a translation along the direction within its hull's bound). Each wall stands
//!   on cap edges at both levels (its support read in the frame is the cap's), its other edges
//!   lines along the direction.
//! - **The section:** the caps' loops matched one to one; a convex straight outline or a circle
//!   with separated round holes (the profiles the separation proof covers).
//! - **The volume:** the section's area interval, widened by the strip the corridor allows,
//!   times the height's interval.
//!
//! Not upstream: SolidCraft's `prism_measure.rs`, read off the kernel's body instead of a record
//! (docs/layers.md §1).
//!
//! Reference: Shewchuk, *Lecture Notes on Geometric Robustness* (2026), §2: anchor area
//! calculations near the object so their rounding bounds depend on its extent.
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use std::collections::BTreeSet;
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use super::profile::{axes, cross2, height, to_local, to_world, Loop2, Region2, Seg2};
use super::read::{edge_run, planar_pieces, Run};
use crate::eval::Frame;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec2};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::NOW;
use crate::query::api_queries::{brep_is_manifold_solid, face_get_boundary_loops, get_faces};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::{face::p2 as face_p2, Brep, EdgeId, FaceId};

/// An interval `[lo, hi]` holding a quantity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    /// Its low end.
    pub lo: f64,
    /// Its high end.
    pub hi: f64,
}

impl Interval {
    fn enclosing(value: f64, error: f64) -> Option<Self> {
        if !value.is_finite() || !error.is_finite() || error < 0.0 {
            return None;
        }
        // the extra allowance covers the subtraction and accumulation; never inward
        let padding = 16.0 * f64::EPSILON * (value.abs() + error + 1.0);
        Some(Self {
            lo: value - error - padding,
            hi: value + error + padding,
        })
    }

    fn add(self, other: Self) -> Option<Self> {
        Self::enclosing(
            (self.lo + self.hi + other.lo + other.hi) * 0.5,
            (self.hi - self.lo + other.hi - other.lo) * 0.5,
        )
    }
}

/// The signed area term of a line or arc read off a curve, enclosed with the curve's own
/// uncertainty: within `tolerance` of the segment all along, monotone along it (a line) or
/// round it (an arc), so it differs from the segment's term by a strip and the end wedges.
fn green_interval(segment: &Seg2, tolerance: f64) -> Option<Interval> {
    let (a, b) = (segment.start(), segment.end());
    let (length, extra) = match *segment {
        Seg2::Line { .. } => (a.distance(b), 0.0),
        Seg2::Arc {
            center,
            radius,
            sweep,
            ..
        } => (radius * sweep.abs(), center.length() + radius),
        _ => return None,
    };
    let (term, bound) = segment.area_term()?;
    let scale = length + a.length() + b.length() + extra;
    let error = 8.0 * tolerance * scale
        + 64.0 * tolerance * tolerance
        + bound
        + 1024.0 * f64::EPSILON * (1.0 + scale * scale);
    Interval::enclosing(term, error)
}

#[derive(Clone, Debug)]
struct CapLoop {
    segments: Vec<Seg2>,
    area: Interval,
}

/// A planar surface's point and unit natural normal, and a bound on the normal's rounding.
fn plane_of(s: &Surface) -> Option<(Point3, Vec3)> {
    match s.kind() {
        SurfaceKind::Plane(p) => {
            let (o, n, _) = p.is_planar();
            let len = n.length();
            ((len - 1.0).abs() <= 1e-12).then_some((o, n))
        }
        _ => None,
    }
}

/// A cylinder's axis point, unit axis and radius.
fn cylinder_of(s: &Surface) -> Option<(Point3, Vec3, f64)> {
    match s.kind() {
        SurfaceKind::Cone(c) if c.is_cylinder() => {
            let can = c.canonical();
            let a = can.origin.z_axis();
            ((a.length() - 1.0).abs() <= 1e-12 && can.radius > 0.0).then_some((
                can.origin.origin(),
                a,
                can.radius,
            ))
        }
        _ => None,
    }
}

/// A spline surface's control net, `(point, weight)` with `[i][j]` i along u, and its knots.
type Net = (Vec<Vec<(Point3, f64)>>, Vec<f64>, Vec<f64>);

fn net_of(s: &Surface) -> Option<Net> {
    let SurfaceKind::BSpline(b) = s.kind() else {
        return None;
    };
    let nurb = b.gw_nurb()?;
    let rows = nurb
        .rows()
        .map(|r| {
            r.iter()
                .map(|c| {
                    let w = if c.w == NOW { 1.0 } else { c.w };
                    let d = if c.w == NOW { 1.0 } else { c.w };
                    (Point3::new(c.x / d, c.y / d, c.z / d), w)
                })
                .collect()
        })
        .collect();
    Some((rows, nurb.u_knots().to_vec(), nurb.v_knots().to_vec()))
}

/// A whole bound on how far the wall strays across the direction `d` (a unit vector) over
/// `height`: a plane's normal against `d`, a cylinder's axis against it, an extrusion's
/// direction against it (its rulings are straight along that direction, whatever its
/// generator), a ruled net's rows against it (positive weights make the hull a bound on the
/// surface). With `reach`, a net's rows must also run monotonically along `d` for at least half
/// of `height`, as a prism's wall from cap to cap does ([`section`]); without it any rows do (the
/// face then lies within the bound of its first row's curve swept along `d`). `None` where none
/// is proved.
fn wall_error(
    surface: &Surface,
    d: Vec3,
    height: f64,
    work: &mut usize,
    reach: bool,
) -> Option<f64> {
    if let Some((_, n)) = plane_of(surface) {
        let error = 1024.0
            * f64::EPSILON
            * ((n.x * d.x).abs() + (n.y * d.y).abs() + (n.z * d.z).abs() + 1.0);
        return Some((n.dot(d).abs() + error) * height * 2.0);
    }
    if let Some((_, a, _)) = cylinder_of(surface) {
        return Some((a.cross(d).length() + 1024.0 * f64::EPSILON) * height * 4.0);
    }
    if let SurfaceKind::SurfOfExtrusion(x) = surface.kind() {
        // made unit scaled first: a direction of any finite length, none past f64::MAX squared
        let (_, a) = x.extrusion_vec.length_and_unit()?;
        return Some((a.cross(d).length() + 1024.0 * f64::EPSILON) * height * 4.0);
    }
    let (points, _, _) = net_of(surface)?;
    let (rows, cols) = (points.len(), points.first()?.len());
    let count = rows.checked_mul(cols)?;
    if rows < 2 || cols < 2 || count > 16_384 || !points.iter().all(|r| r.len() == cols) {
        return None;
    }
    *work = work.checked_sub(count.checked_mul(4)?)?;
    if points.iter().flatten().any(|(p, w)| {
        !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() || !w.is_finite() || *w <= 0.0
    }) {
        return None;
    }
    let max_w = points.iter().flatten().map(|p| p.1).fold(0.0, f64::max);
    let min_w = points
        .iter()
        .flatten()
        .map(|p| p.1 / max_w)
        .fold(1.0, f64::min);
    if min_w < 1e-6 {
        return None;
    }
    let magnitude = points
        .iter()
        .flatten()
        .map(|p| p.0.length())
        .fold(1.0, f64::max);
    let mut best = f64::INFINITY;
    for transpose in [false, true] {
        let at = |i: usize, j: usize| {
            if transpose {
                points.get(j)?.get(i).copied()
            } else {
                points.get(i)?.get(j).copied()
            }
        };
        let (groups, length) = if transpose {
            (cols, rows)
        } else {
            (rows, cols)
        };
        let mut variation: f64 = 0.0;
        let mut rank_error: f64 = 0.0;
        let mut monotone = true;
        let mut axial_sign = 0.0;
        for i in 0..groups {
            let (base, bw) = at(i, 0)?;
            let mut previous = base.dot(d);
            for j in 0..length {
                let (point, w) = at(i, j)?;
                let step = point.dot(d) - previous;
                previous = point.dot(d);
                if step.abs() > 2048.0 * f64::EPSILON * magnitude {
                    if axial_sign * step < 0.0 {
                        monotone = false;
                    }
                    axial_sign = step.signum();
                }
                variation = variation.max((point - base).cross(d).length());
                let lhs = w / max_w * (at(0, 0)?.1 / max_w);
                let rhs = bw / max_w * (at(0, j)?.1 / max_w);
                rank_error = rank_error.max((lhs - rhs).abs());
            }
            if (at(i, length - 1)?.0 - base).dot(d).abs() < height * 0.5 {
                monotone = false;
            }
        }
        if reach && (!monotone || axial_sign == 0.0) {
            continue;
        }
        // rank-one weights keep the normalized convex coefficients; their error is bounded by the
        // positive least weight
        let error = 2.0 * variation
            + 8.0 * magnitude * rank_error / min_w.powi(2)
            + 2048.0 * f64::EPSILON * magnitude / min_w.powi(2);
        best = best.min(error);
    }
    best.is_finite().then_some(best)
}

fn segment_same(a: &Seg2, b: &Seg2, tolerance: f64) -> bool {
    match (*a, *b) {
        (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
            (a.distance(c) <= tolerance && b.distance(d) <= tolerance)
                || (a.distance(d) <= tolerance && b.distance(c) <= tolerance)
        }
        (
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            },
            Seg2::Arc {
                center: c,
                radius: r,
                start: t,
                sweep: s,
            },
        ) => {
            let at = |angle: f64, radius: f64| Vec2::new(angle.cos(), angle.sin()) * radius;
            center.distance(c) <= tolerance
                && (radius - r).abs() <= tolerance
                && (sweep.abs() - s.abs()).abs() * radius <= tolerance
                && (at(start, radius).distance(at(t, r)) <= tolerance
                    || at(start, radius).distance(at(t + s, r)) <= tolerance)
        }
        _ => false,
    }
}

/// The circle a loop of arcs makes: one centre and radius, one way round, a whole turn.
fn circle(segments: &[Seg2], tolerance: f64) -> Option<(Point2, f64)> {
    let Seg2::Arc {
        center,
        radius,
        sweep: first,
        ..
    } = *segments.first()?
    else {
        return None;
    };
    let mut sweep = 0.0;
    for s in segments {
        let Seg2::Arc {
            center: c,
            radius: r,
            sweep: a,
            ..
        } = *s
        else {
            return None;
        };
        if c.distance(center) > tolerance || (r - radius).abs() > tolerance || a * first <= 0.0 {
            return None;
        }
        sweep += a;
    }
    ((sweep.abs() - TAU).abs() * radius <= tolerance).then_some((center, radius))
}

/// Whether two loops are the same within `tolerance`: their areas' signs, then one circle each,
/// or their segments matched one to one. `None` where a segment matches two.
fn loops_same(a: &CapLoop, b: &CapLoop, tolerance: f64, work: &mut usize) -> Option<bool> {
    *work = work.checked_sub(a.segments.len().checked_add(b.segments.len())?)?;
    if (a.area.lo > 0.0) != (b.area.lo > 0.0) {
        return Some(false);
    }
    if let (Some((ca, ra)), Some((cb, rb))) = (
        circle(&a.segments, tolerance),
        circle(&b.segments, tolerance),
    ) {
        return Some(ca.distance(cb) + (ra - rb).abs() <= tolerance);
    }
    if a.segments.len() != b.segments.len() {
        return Some(false);
    }
    *work = work.checked_sub(a.segments.len().checked_mul(b.segments.len())?)?;
    let mut used = vec![false; b.segments.len()];
    for s in &a.segments {
        let mut found = None;
        for (i, t) in b.segments.iter().enumerate() {
            if !used.get(i).copied()? && segment_same(s, t, tolerance) {
                if found.is_some() {
                    return None;
                }
                found = Some(i);
            }
        }
        let Some(i) = found else { return Some(false) };
        *used.get_mut(i)? = true;
    }
    Some(true)
}

/// A face's walk: each loop's edges, each with the way it runs about the face's outward normal.
type Walk = Vec<Vec<(EdgeId, bool)>>;

/// Whether face `f`'s outward side is its surface's natural normal's: its outer use (the one
/// bounding the infinite region, [`face_p2::outer_faceuse`]) is its upward use (on the normal's
/// side).
fn outward_is_natural(b: &Brep, f: FaceId) -> Option<bool> {
    let up = face_p2::upward_faceuse(b, f).ok()??;
    let outer = face_p2::outer_faceuse(b, f).ok()??;
    Some(outer == up)
}

/// Face `f`'s loops as they run about its outward normal.
fn walk(b: &Brep, f: FaceId, natural: bool) -> Option<Walk> {
    let loops = face_get_boundary_loops(b, f).ok()?;
    let mut uses = loops.edges.iter().zip(&loops.orientations);
    let mut out = Vec::new();
    for &count in &loops.edge_counts {
        let mut lp: Vec<(EdgeId, bool)> = uses
            .by_ref()
            .take(count)
            .map(|(e, o)| (*e, matches!(o, OrientType::Same) == natural))
            .collect();
        if !natural {
            lp.reverse();
        }
        out.push(lp);
    }
    Some(out)
}

fn face_edges(w: &Walk) -> impl Iterator<Item = EdgeId> + '_ {
    w.iter().flatten().map(|(e, _)| *e)
}

fn face_points<'a>(b: &'a Brep, w: &'a Walk) -> impl Iterator<Item = Point3> + 'a {
    face_edges(w)
        .filter_map(|e| edge_run(b, e, true))
        .flat_map(|r| r.ends)
}

/// A cap's loop from its edges in order, read segment by segment, with its area enclosed.
fn cap_loop(
    k: &Kernel,
    b: &Brep,
    edges: &[(EdgeId, bool)],
    frame: &Frame,
    tol: f64,
    work: &mut usize,
) -> Option<CapLoop> {
    let mut segments = Vec::new();
    let mut area = Interval::enclosing(0.0, 0.0)?;
    let mut ids = BTreeSet::new();
    let mut runs = Vec::new();
    for &(e, forward) in edges {
        if !ids.insert(e) {
            return None;
        }
        let run = edge_run(b, e, forward)?;
        let read = planar_pieces(k, &run, frame, tol, work, false)?;
        if segments.len().checked_add(read.len())? > 8192 {
            return None;
        }
        for s in &read {
            area = area.add(green_interval(s, tol)?)?;
        }
        segments.extend(read);
        runs.push(run);
    }
    // the edges run end to end round the loop, and so do the segments read off them
    let closed = runs
        .iter()
        .zip(runs.iter().cycle().skip(1))
        .all(|(a, b)| a.ends[1].distance(b.ends[0]) <= 2.0 * tol);
    let joined = segments
        .iter()
        .zip(segments.iter().cycle().skip(1))
        .all(|(a, b)| a.end().distance(b.start()) <= 2.0 * tol);
    (!segments.is_empty() && closed && joined).then_some(CapLoop { segments, area })
}

/// Does the wall `surface` carry the cap boundary `cap` (read in the frame at level `level`)
/// between the cap levels `levels`, its own heights `wall_levels`?
#[allow(clippy::too_many_arguments)]
fn wall_supports_cap(
    k: &Kernel,
    surface: &Surface,
    wall_levels: (f64, f64),
    cap: &[Seg2],
    level: f64,
    frame: &Frame,
    levels: (f64, f64),
    tol: f64,
    work: &mut usize,
) -> Option<bool> {
    let spans = |ha: f64, hb: f64| {
        !(ha.min(hb) > levels.0 + tol * 8.0 || ha.max(hb) < levels.1 - tol * 8.0)
    };
    if !spans(wall_levels.0, wall_levels.1) {
        return Some(false);
    }
    if let Some((origin, n)) = plane_of(surface) {
        for s in cap {
            let Seg2::Line { a, b } = *s else {
                return Some(false);
            };
            for q in [a, b] {
                let point = to_world(frame, q, level);
                let delta = point - origin;
                let error = 1024.0 * f64::EPSILON * (1.0 + point.length() + origin.length());
                if delta.dot(n).abs() + error > tol * 8.0 {
                    return Some(false);
                }
            }
        }
        return Some(true);
    }
    let on_circle = |center: Point2, radius: f64| {
        cap.iter()
            .all(|s| matches!(*s, Seg2::Arc { center: c, radius: r, .. } if c.distance(center) + (r - radius).abs() <= tol * 8.0))
    };
    if let Some((o, axis, radius)) = cylinder_of(surface) {
        let SurfaceKind::Cone(cylinder) = surface.kind() else {
            return None;
        };
        // Boundary vertices alone cannot prove incidence: the bounded cylinder support must
        // reach both caps too, even when its infinite extension is the same cylinder.
        let h0 = height(frame, o + axis * cylinder.bot_height());
        let h1 = height(frame, o + axis * cylinder.top_height());
        if !h0.is_finite() || !h1.is_finite() || !spans(h0, h1) {
            return Some(false);
        }
        let (_, _, _, n) = axes(frame);
        if axis.cross(n).length() * radius > tol {
            return None;
        }
        return Some(on_circle(to_local(frame, o), radius));
    }
    let (points, u_knots, v_knots) = net_of(surface)?;
    let (rows, cols) = (points.len(), points.first()?.len());
    *work = work.checked_sub(rows.checked_mul(cols)?.checked_mul(4)?)?;
    for transpose in [false, true] {
        let point = |i: usize, j: usize| {
            if transpose {
                points.get(j)?.get(i).copied()
            } else {
                points.get(i)?.get(j).copied()
            }
        };
        let (groups, length) = if transpose {
            (cols, rows)
        } else {
            (rows, cols)
        };
        // a degree-one ruling across: its positive rational interpolation is monotone and reaches
        // both caps
        let axial = if transpose { &u_knots } else { &v_knots };
        if !(2..=4096).contains(&groups)
            || length != 2
            || axial.len() != 4
            || axial.first() != axial.get(1)
            || axial.get(2) != axial.last()
            || !(axial.get(1)? < axial.get(2)?)
        {
            continue;
        }
        let mut projected = Vec::new();
        let mut valid = true;
        let mut increasing = None;
        for i in 0..groups {
            let (base, w) = point(i, 0)?;
            let (h0, h1) = (height(frame, base), height(frame, point(i, 1)?.0));
            if !spans(h0, h1) || increasing.is_some_and(|s| s != (h1 > h0)) {
                valid = false;
                break;
            }
            increasing = Some(h1 > h0);
            let p = to_local(frame, base);
            if to_local(frame, point(i, 1)?.0).distance(p) > tol * 8.0 {
                valid = false;
                break;
            }
            let q = to_world(frame, p, level);
            projected.push([q.x * w, q.y * w, q.z * w, w]);
        }
        if !valid {
            continue;
        }
        let knots = if transpose { &v_knots } else { &u_knots };
        if knots.len() > 8192 {
            return None;
        }
        let degree = knots.len().checked_sub(projected.len() + 1)?;
        let interval = [*knots.get(degree)?, *knots.get(projected.len())?];
        if !(interval[0] < interval[1]) {
            continue;
        }
        let curve = super::profile::rational_curve(knots, &projected).ok()?;
        let iv = curve.natural_interval();
        let ends = [
            curve.evaluate_point(k, iv.min()).ok()?,
            curve.evaluate_point(k, iv.max()).ok()?,
        ];
        let run = Run {
            curve: &curve,
            interval: iv,
            forward: true,
            ends,
        };
        let Some(fit) = planar_pieces(k, &run, frame, tol, work, false) else {
            continue;
        };
        if let Some((center, radius)) = circle(&fit, tol * 8.0) {
            if on_circle(center, radius) {
                return Some(true);
            }
        }
        let zero = Interval::enclosing(0.0, 0.0)?;
        let same = loops_same(
            &CapLoop {
                segments: cap.to_vec(),
                area: zero,
            },
            &CapLoop {
                segments: fit,
                area: zero,
            },
            tol * 8.0,
            work,
        )?;
        if same {
            return Some(true);
        }
    }
    Some(false)
}

struct Certificate {
    loops: Vec<CapLoop>,
    volume: Interval,
    lo: f64,
    hi: f64,
}

struct FaceInfo {
    id: FaceId,
    walk: Walk,
}

fn certificate(k: &Kernel, b: &Brep, size: f64, frame: &Frame, tol: f64) -> Option<Certificate> {
    if !brep_is_manifold_solid(b).ok()? {
        return None;
    }
    let ids = get_faces(b).ok()?;
    if ids.len() > 16_384 {
        return None;
    }
    let (_, _, _, d) = axes(frame);
    let mut caps: Vec<(usize, f64, bool)> = Vec::new();
    let mut faces = Vec::new();
    let mut walls = Vec::new();
    let mut edge_uses = 0usize;
    for (fi, &f) in ids.iter().enumerate() {
        let natural = outward_is_natural(b, f)?;
        let walk = walk(b, f, natural)?;
        edge_uses = edge_uses.checked_add(face_edges(&walk).count())?;
        if edge_uses > 65_536 {
            return None;
        }
        let surface = b.face(f).ok()?.surface.as_ref()?;
        let info = FaceInfo { id: f, walk };
        if let Some((_, n)) = plane_of(surface) {
            let n = if natural { n } else { -n };
            if n.dot(d).abs() > 1.0 - 1e-12 {
                if n.cross(d).length() * size > tol {
                    return None;
                }
                // the support's origin may be far from the trimmed face: use its boundary
                let level = height(frame, face_points(b, &info.walk).next()?);
                caps.push((fi, level, n.dot(d) > 0.0));
                faces.push(info);
                if caps.len() > 2 {
                    return None;
                }
                continue;
            }
        }
        walls.push(fi);
        faces.push(info);
    }
    let top = *caps.iter().find(|c| c.2)?;
    let bottom = *caps.iter().find(|c| !c.2)?;
    let height_ = top.1 - bottom.1;
    if caps.len() != 2 || !(height_ > tol) || !height_.is_finite() {
        return None;
    }
    for cap in [top, bottom] {
        let info = faces.get(cap.0)?;
        let (origin, n) = plane_of(b.face(info.id).ok()?.surface.as_ref()?)?;
        for point in face_points(b, &info.walk) {
            let delta = point - origin;
            let error = 1024.0
                * f64::EPSILON
                * ((delta.x * n.x).abs()
                    + (delta.y * n.y).abs()
                    + (delta.z * n.z).abs()
                    + delta.length());
            if delta.dot(n).abs() + error > tol * 2.0
                || (height(frame, point) - cap.1).abs() > tol * 2.0
            {
                return None;
            }
        }
    }
    let mut work = 2_000_000usize;
    let cap_loops = |cap: usize, work: &mut usize| -> Option<Vec<CapLoop>> {
        faces
            .get(cap)?
            .walk
            .iter()
            .map(|lp| cap_loop(k, b, lp, frame, tol, work))
            .collect()
    };
    let loops = cap_loops(top.0, &mut work)?;
    let lower = cap_loops(bottom.0, &mut work)?;
    if loops.len() != lower.len() || loops.is_empty() || loops.len() > 8192 {
        return None;
    }
    let mut pair_work = 16_000_000usize;
    for a in &loops {
        // the bottom cap has the opposite material orientation
        let reverse = CapLoop {
            segments: a.segments.clone(),
            area: Interval {
                lo: -a.area.hi,
                hi: -a.area.lo,
            },
        };
        let mut matches = 0usize;
        for l in &lower {
            if loops_same(&reverse, l, tol * 8.0, &mut pair_work)? {
                matches += 1;
            }
        }
        if matches != 1 {
            return None;
        }
    }
    let mut controls = 262_144usize;
    // the cap reading, the support reading and their <= 8 tol comparison, beside the walls'
    let mut corridor = tol * 16.0;
    let top_edges: BTreeSet<EdgeId> = face_edges(&faces.get(top.0)?.walk).collect();
    let bottom_edges: BTreeSet<EdgeId> = face_edges(&faces.get(bottom.0)?.walk).collect();
    for &wall in &walls {
        let info = faces.get(wall)?;
        let surface = b.face(info.id).ok()?.surface.as_ref()?;
        let variation = wall_error(surface, d, height_, &mut controls, true)?;
        if variation > tol * 8.0 {
            return None;
        }
        corridor = corridor.max(tol * 16.0 + variation);
        if corridor > tol * 24.0 {
            return None;
        }
        let heights = face_points(b, &info.walk).map(|p| height(frame, p));
        let wall_levels = heights.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| {
            (lo.min(h), hi.max(h))
        });
        // every wall ends on the caps' own edges: their topology, not the nearest geometry
        let (mut upper, mut lower_count) = (0usize, 0usize);
        for e in face_edges(&info.walk) {
            let run = edge_run(b, e, true)?;
            let (on_top, on_bottom) = (top_edges.contains(&e), bottom_edges.contains(&e));
            if on_top || on_bottom {
                let cap = planar_pieces(k, &run, frame, tol, &mut work, false)?;
                if !wall_supports_cap(
                    k,
                    surface,
                    wall_levels,
                    &cap,
                    height(frame, run.ends[0]),
                    frame,
                    (bottom.1, top.1),
                    tol,
                    &mut work,
                )? {
                    return None;
                }
                if on_top {
                    upper += 1;
                } else {
                    lower_count += 1;
                }
                continue;
            }
            if !matches!(run.curve.kind(), crate::curve::curve::CurveKind::Line(_)) {
                return None;
            }
            let (p, q) = (
                run.curve.evaluate_point(k, run.interval.min()).ok()?,
                run.curve.evaluate_point(k, run.interval.max()).ok()?,
            );
            if p.distance(run.ends[0]) > tol || q.distance(run.ends[1]) > tol {
                return None;
            }
            let delta = q - p;
            if delta.cross(d).length() > corridor || delta.dot(d).abs() < height_ - tol * 8.0 {
                return None;
            }
        }
        if upper == 0 || upper != lower_count {
            return None;
        }
    }
    let mut area = Interval::enclosing(0.0, 0.0)?;
    let mut length = 0.0;
    let mut count = 0usize;
    for lp in &loops {
        area = area.add(lp.area)?;
        for s in &lp.segments {
            length += match *s {
                Seg2::Arc { radius, sweep, .. } => radius * sweep.abs(),
                _ => s.start().distance(s.end()),
            };
            count = count.checked_add(1)?;
        }
    }
    if count > 8192 || area.lo <= 0.0 {
        return None;
    }
    separated_material(&loops, corridor * 16.0, &mut pair_work)?;
    // each boundary's corridor adds at most a two-sided strip and four end discs
    #[allow(clippy::cast_precision_loss)]
    let strip = 2.0 * length * corridor + 4.0 * PI * count as f64 * corridor * corridor;
    let h_error = tol * 8.0;
    if area.lo <= strip || height_ <= h_error {
        return None;
    }
    let lo = (area.lo - strip) * (height_ - h_error);
    let hi = (area.hi + strip) * (height_ + h_error);
    let volume = Interval::enclosing((lo + hi) * 0.5, (hi - lo) * 0.5)?;
    if volume.lo <= 0.0 || volume.hi - volume.lo > 0.001 {
        return None;
    }
    Some(Certificate {
        loops,
        volume,
        lo: bottom.1,
        hi: top.1,
    })
}

/// The material the separation proof covers: one outer loop, a convex straight outline or a
/// circle, with separated circular holes inside it.
fn separated_material(loops: &[CapLoop], clearance: f64, work: &mut usize) -> Option<()> {
    let mut outers = loops.iter().filter(|l| l.area.lo > 0.0);
    let outer = outers.next()?;
    if outers.next().is_some() {
        return None;
    }
    let outer_circle = circle(&outer.segments, clearance);
    if outer_circle.is_none() {
        *work = work.checked_sub(outer.segments.len().checked_mul(outer.segments.len())?)?;
        for s in &outer.segments {
            let Seg2::Line { a, b } = *s else { return None };
            let delta = b - a;
            if delta.length() <= clearance {
                return None;
            }
            for t in &outer.segments {
                if cross2(delta, t.start() - a) < -clearance * delta.length() {
                    return None;
                }
            }
        }
        let mut turn = 0.0;
        let mut angular_error = 0.0;
        let next = outer.segments.iter().cycle().skip(1);
        for (i, (s, nx)) in outer.segments.iter().zip(next).enumerate() {
            let Seg2::Line { a, b } = *s else { return None };
            let x = b - a;
            let y = nx.end() - nx.start();
            turn += cross2(x, y).atan2(x.dot(y));
            angular_error += clearance * 8.0 / x.length() + 256.0 * f64::EPSILON;
            for (j, t) in outer.segments.iter().enumerate() {
                if i != j && a.distance(t.start()) <= clearance {
                    return None;
                }
            }
        }
        if angular_error > 1e-3 || (turn - TAU).abs() > angular_error {
            return None;
        }
    }
    let mut holes: Vec<(Point2, f64)> = Vec::new();
    for lp in loops {
        if std::ptr::eq(lp, outer) {
            continue;
        }
        if lp.area.hi >= 0.0 {
            return None;
        }
        let (center, radius) = circle(&lp.segments, clearance)?;
        if let Some((c, r)) = outer_circle {
            if center.distance(c) + radius + clearance >= r {
                return None;
            }
        } else {
            *work = work.checked_sub(outer.segments.len())?;
            for s in &outer.segments {
                let Seg2::Line { a, b } = *s else { return None };
                if cross2(b - a, center - a) / (b - a).length() <= radius + clearance {
                    return None;
                }
            }
        }
        *work = work.checked_sub(holes.len())?;
        if holes
            .iter()
            .any(|(c, r)| center.distance(*c) <= radius + r + clearance)
        {
            return None;
        }
        holes.push((center, radius));
    }
    Some(())
}

/// A uniform prism along a frame's normal: its levels above the frame and its section in the
/// frame's coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PrismSection {
    /// The level of its bottom cap above the frame.
    pub lo: f64,
    /// The level of its top cap (`lo < hi`).
    pub hi: f64,
    /// Its section, exact: lines and arcs, the outer loop counter-clockwise, the holes clockwise.
    pub region: Region2,
    /// How far the levels and section may be off the body's caps, including frame conversion.
    pub tolerance: f64,
    /// An enclosure of its volume.
    pub volume: Interval,
}

/// A whole bound on how far face `f` of `b` strays across the direction `d`: the face lies
/// within the bound of a surface swept along `d`. It is the bound [`section`] proves for each of a
/// prism's walls (a plane by its normal against `d`, a cylinder by its axis, a ruled spline net by
/// its rows, however far they run along `d`), taken over the face's own size: the diagonal of its
/// tight box, curved edges included (a cylinder's cap has one seam vertex, whose vertex box omits
/// the circle). A net kept for a procedural definition adds its certified distance from it
/// (`DefinitionRef::bound`, 0 for a definition it matches exactly). A face whose bound is within
/// the zone tolerance runs straight along `d`.
///
/// `None` where no bound is proved: a face with no surface or whose tight box is not found; a
/// surface that is not a plane, a cylinder or a ruled spline net (an extruded surface among
/// them); a net past the size or work budget, or whose weights are not positive; a definition
/// whose distance is not finite; a `d` that is zero or not finite (any finite length else, scaled
/// before it is squared).
#[must_use]
pub fn face_straying(k: &Kernel, b: &Brep, f: FaceId, d: Vec3) -> Option<f64> {
    let (_, d) = d.length_and_unit()?;
    let surface = b.face(f).ok()?.surface.as_ref()?;
    let (size, definition) = straying_scale(k, b, f, surface)?;
    Some(wall_error(surface, d, size, &mut 262_144_usize, false)? + definition)
}

/// The size [`face_straying`] scales face `f`'s bound by (the diagonal of its tight box) and the
/// distance of its net from its procedural definition. `None` where [`face_straying`] proves no
/// bound for want of them.
fn straying_scale(k: &Kernel, b: &Brep, f: FaceId, surface: &Surface) -> Option<(f64, f64)> {
    let definition = surface
        .procedural_definition()
        .map_or(0.0, |def| def.bound());
    if !(definition >= 0.0 && definition.is_finite()) {
        return None;
    }
    let bounds = crate::query::api_queries::face_bounding_box(k, b, f, true).ok()?;
    let size = (bounds.max() - bounds.min()).length();
    if !size.is_finite() {
        return None;
    }
    Some((size, definition))
}

/// What a face gives towards the direction a set of faces runs straight along
/// ([`straight_candidates`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceRun {
    /// A plane label, by its unit normal (either sense) and its band: a unit `d` with
    /// `|normal · d| <= slope` is one the face runs straight along within the zone.
    Plane {
        /// The unit normal, either sense.
        normal: Vec3,
        /// The band's half width.
        slope: f64,
    },
    /// A cylinder label, by its unit axis (either sense) and its band: a unit `d` with
    /// `|axis × d| <= slope` is one the face runs straight along within the zone.
    Axis {
        /// The unit axis, either sense.
        axis: Vec3,
        /// The band's half width.
        slope: f64,
    },
    /// Another surface, or none, or one whose size is not found: it may still run straight
    /// along a direction ([`face_straying`]), but gives none and no band.
    Other,
}

/// [`FaceRun`] of face `f` of `b`, from its surface's label, with its band for the zone tolerance
/// `zone`: [`face_straying`]'s plane or cylinder term solved for the direction, with a margin so
/// that a unit `d` inside the band (measured against the unit normal or axis given here) is one
/// along which [`face_straying`] is within the zone.
///
/// - A plane strays by `(|n · d| + 1024ε (Σ|nᵢ dᵢ| + 1)) · 2 · size` with the stored `n`, unit
///   within 1e-12 (`plane_of`): `Σ|nᵢ dᵢ| <= |n| |d| < 2` takes 3072ε, the stored normal's
///   length a factor `1 + 1e-12` on `|n · d|`, and the dot product and `d`'s length a few ulps
///   more (8ε). So `slope = (room / (2 size) - 3080ε) / (1 + 1e-12)`.
/// - A cylinder strays by `(|a × d| + 1024ε) · 4 · size` with the stored axis, unit within 1e-12
///   (`cylinder_of`): `slope = (room / (4 size) - 1032ε) / (1 + 1e-12)`, the 8ε for the cross
///   product's and `d`'s rounding.
///
/// `room` is the zone less the definition's distance, rounded down. A band of no width (a face too
/// large for the zone) is kept: nothing is inside it.
#[must_use]
pub fn face_run(k: &Kernel, b: &Brep, f: FaceId, zone: f64) -> FaceRun {
    let Some(surface) = b.face(f).ok().and_then(|face| face.surface.as_ref()) else {
        return FaceRun::Other;
    };
    let Some((size, definition)) = straying_scale(k, b, f, surface) else {
        return FaceRun::Other;
    };
    // rounded down: the difference within an ulp of the larger
    let room = zone - definition - f64::EPSILON * zone.abs().max(definition.abs());
    if let Some((_, normal)) = plane_of(surface).and_then(|(_, n)| n.length_and_unit()) {
        let slope = (room / (2.0 * size) - 3080.0 * f64::EPSILON) / (1.0 + 1e-12);
        return FaceRun::Plane { normal, slope };
    }
    if let Some((_, axis)) = cylinder_of(surface).and_then(|(_, a, _)| a.length_and_unit()) {
        let slope = (room / (4.0 * size) - 1032.0 * f64::EPSILON) / (1.0 + 1e-12);
        return FaceRun::Axis { axis, slope };
    }
    FaceRun::Other
}

/// Where a candidate of [`straight_candidates`] comes from: indices into its `runs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StraightBy {
    /// The cross product of two plane normals.
    Planes(usize, usize),
    /// A cylinder's axis.
    Axis(usize),
    /// A direction inside every plane's and cylinder's band, found near the first candidate when
    /// that one is outside a band.
    Bands,
}

/// The longest cross product of two unit plane normals that is still taken for parallel. Two
/// normals stored exactly parallel have an exact cross product of 0 whatever their lengths, and the
/// computed one is off only by its rounding: each component `aᵢbⱼ - aⱼbᵢ` by at most
/// `γ₂ (|aᵢbⱼ| + |aⱼbᵢ|)`, at most `2ε |a| |b|` (about `2ε`) over the three, and the square root
/// adds an ulp; `8ε` covers that. The `2e-12` is a margin for normals parallel only before they
/// were rounded apart (each read from its own plane, or rotated), which nothing here bounds.
const PARALLEL: f64 = 2.0 * 1e-12 + 8.0 * f64::EPSILON;

/// The most constraints [`lean`] takes (two for each plane, one for each cylinder, four for the
/// chart): past it the search is not made and the candidates are not complete.
const LEAN_ROWS: usize = 100;

/// The directions faces with `runs` may run straight along ([`straight_candidates`]).
#[derive(Clone, Debug, PartialEq)]
pub struct StraightCandidates {
    /// The candidates, best first, each a unit vector in a canonical sense, with where it comes
    /// from.
    pub directions: Vec<(Vec3, StraightBy)>,
    /// Whether, when no direction in `directions` is inside every plane's and cylinder's band,
    /// it is proved that no direction is one `face_straying` accepts for every plane and
    /// cylinder. `false` when the search for one was not decided: past `LEAN_ROWS`
    /// constraints or `CERT_ROWS` half-planes, a band that may lie outside the search's reach, or
    /// no certificate that none exists.
    pub complete: bool,
}

/// The directions faces with `runs` may run straight along, best first, each a unit vector in a
/// canonical sense (its largest component positive, the first of equal ones), with where it comes
/// from:
/// - the cross product of the two plane normals furthest from parallel, when one pair is proved
///   not parallel (`PARALLEL`): two planes that both run along a direction fix it but for their
///   bands;
/// - when that one (or, with no such pair, the first axis) is outside a plane's or cylinder's
///   band, a direction inside every band near it, if there is one (`lean`);
/// - then each cylinder axis, in a canonical order, the same axes once.
///
/// Each normal and axis is taken in its canonical sense and the runs in a canonical order (by
/// their values; equal values by index), so neither the runs' order nor their senses change
/// which direction comes first. The indices it names are positions in `runs`: of runs with one
/// canonical normal or axis (a face and the one opposite it, say), the first in `runs` is named,
/// so reordering those changes which face is named, never the direction. Nothing here proves a
/// face straight along a candidate; a caller checks each face with [`face_straying`]. Empty when
/// no plane pair is proved apart and no face is a cylinder.
#[must_use]
pub fn straight_candidates(runs: &[FaceRun]) -> StraightCandidates {
    let mut planes: Vec<(Vec3, usize, f64)> = Vec::new();
    let mut axes: Vec<(Vec3, usize, f64)> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        match *run {
            FaceRun::Plane { normal, slope } => planes.push((canonical(normal), i, slope)),
            FaceRun::Axis { axis, slope } => axes.push((canonical(axis), i, slope)),
            FaceRun::Other => {}
        }
    }
    let by_value = |a: &(Vec3, usize, f64), b: &(Vec3, usize, f64)| {
        let (p, q) = (a.0, b.0);
        p.x.total_cmp(&q.x)
            .then(p.y.total_cmp(&q.y))
            .then(p.z.total_cmp(&q.z))
            .then(a.1.cmp(&b.1))
    };
    planes.sort_by(by_value);
    axes.sort_by(by_value);
    let mut out = Vec::new();
    // the best pair, and how far off its direction a direction inside both bands may lie (the
    // sine of that angle)
    let mut best: Option<(f64, Vec3, StraightBy, f64)> = None;
    // shortcut: the pair scan is quadratic in the planes and takes no cancel token (10 000 planes
    // are 5e7 pairs, about half a second); a cap that leaves the candidates incomplete, or a
    // cancel check, if a caller passes that many faces
    for (i, &(n, a, sn)) in planes.iter().enumerate() {
        for &(m, b, sm) in planes.iter().skip(i + 1) {
            let Some((s, d)) = n.cross(m).length_and_unit() else {
                continue;
            };
            if s > PARALLEL && best.is_none_or(|(t, ..)| s > t) {
                // a unit e off the exact pair's direction by α has the larger of |n · e| and
                // |m · e| at least sin α sin(ψ/2), ψ the acute angle between the planes; d is that
                // direction but for its rounding, which |n · d| and |m · d| bound (brett19 on
                // #22066). The bound takes its own rounding: s and |n · m| each within 4 ulps, the
                // two dot products within 4 more, so it leans on no slack in `accepted`.
                let acute = (s - 4.0 * f64::EPSILON).atan2(n.dot(m).abs() + 4.0 * f64::EPSILON);
                let leaning = accepted(sn, true).max(accepted(sm, true))
                    + n.dot(d).abs()
                    + m.dot(d).abs()
                    + 8.0 * f64::EPSILON;
                let reach = leaning / (0.5 * acute).sin() * (1.0 + 1e-12);
                best = Some((s, d, StraightBy::Planes(a, b), reach));
            }
        }
    }
    let mut reach = None;
    if let Some((_, d, by, r)) = best {
        out.push((canonical(d), by));
        reach = Some(r);
    }
    for &(a, i, t) in &axes {
        if !out.iter().any(|&(d, _)| d.cross(a).length() <= PARALLEL) {
            out.push((a, StraightBy::Axis(i)));
            // with no pair, the first axis leads: a direction face_straying accepts for its face
            // is within its accepted band of it
            if reach.is_none() {
                reach = Some(accepted(t, false) * (1.0 + 1e-12));
            }
        }
    }
    let mut complete = true;
    let mut bands = None;
    if let (Some(&(first, _)), Some(reach)) = (out.first(), reach) {
        if !inside(first, runs) {
            match lean(first, runs, reach) {
                Lean::Found(d) => bands = Some((canonical(d), StraightBy::Bands)),
                Lean::Nowhere => {}
                Lean::Undecided => complete = false,
            }
        }
    }
    // the bands' direction second, right after the one it leans from
    let directions = out
        .iter()
        .take(1)
        .copied()
        .chain(bands)
        .chain(out.iter().skip(1).copied())
        .collect();
    StraightCandidates {
        directions,
        complete,
    }
}

/// Whether the unit `d` is inside every plane's and cylinder's band of `runs`.
#[must_use]
pub fn inside(d: Vec3, runs: &[FaceRun]) -> bool {
    runs.iter().all(|run| match *run {
        FaceRun::Plane { normal, slope } => normal.dot(d).abs() <= slope,
        FaceRun::Axis { axis, slope } => axis.cross(d).length() <= slope,
        FaceRun::Other => true,
    })
}

/// What [`lean`] finds.
enum Lean {
    /// A unit direction inside every plane's and cylinder's band.
    Found(Vec3),
    /// Proved: no direction is inside every band.
    Nowhere,
    /// Neither found nor proved absent.
    Undecided,
}

/// The slope [`face_straying`] accepts for a band of `slope` ([`face_run`]), with its rounding:
/// the margin `face_run` takes off (`k` ulps and the stored normal's or axis's length) given
/// back, the rest of the length as a factor, and 8 ulps for the products and `d`'s rounding.
/// A unit `d` that [`face_straying`] puts within the zone is inside the band this wide.
fn accepted(slope: f64, plane: bool) -> f64 {
    let k = if plane { 3080.0 } else { 1032.0 };
    slope.mul_add(1.0 + 1e-12, k * f64::EPSILON) / (1.0 - 1e-12) + 8.0 * f64::EPSILON
}

/// A unit direction inside every plane's and cylinder's band of `runs`, near the unit `base`;
/// `reach` bounds the sine of the angle between `base` and any direction [`face_straying`]
/// accepts for every face ([`accepted`]).
///
/// The search is in the chart square to `base`: `d = base + p u + q w`, with `u` and `w` unit,
/// square to `base` and to each other, and `|p|, |q| <= tan α`, `α` the angle `reach` gives.
/// - A plane's band `|n · d| <= slope` is two lines in `(p, q)`, a strip.
/// - A cylinder's band `|a × d| <= slope` holds on the disc `|x - x_a| <= slope` around the point
///   `x_a = (a · u, a · w) / (a · base)` where the axis meets the chart: `a × d = a × (d - d_a)`
///   with `d_a = a / (a · base)`, so `|a × d| <= |x - x_a|` (brett19 on #22066).
///
/// Every one is enough once `d` is made unit (`|d| >= 1`). The direction is the centre of the
/// largest circle inside them all ([`Rows::centre`]), found when its radius is above 0 and it is
/// inside every band.
///
/// When there is none, that no direction exists is proved only by a certificate, from the
/// constraints widened to all [`face_straying`] accepts: `|n · d| <= slope |d|` with
/// `|d| <= g = sqrt(1 + 2 tan² α)`, the disc of radius `slope g / (a · base)` (as
/// `|a × v| >= (a · base) |v|` for `v` square to `base`), each with its rounding. A disc is taken
/// as the 16-gon around it, a larger set, so all are half-planes, and half-planes with no point
/// in common have two or three with none (Helly, in the plane), which [`no_point`] proves. A
/// cylinder's axis 60° or more off `base` with an accepted band under 0.25 (so under 15° wide)
/// proves it too: its directions lie more than 45° off `base`, past the chart (`α` under 45°).
/// Otherwise, or past [`LEAN_ROWS`] constraints, or with `α` past 45°, it is undecided.
fn lean(base: Vec3, runs: &[FaceRun], reach: f64) -> Lean {
    let Some((u, w)) = square_pair(base) else {
        return Lean::Undecided;
    };
    // the chart's half width: the tangent of the angle `reach` gives, at most 1 (45°)
    let within = reach.is_finite() && reach < FRAC_1_SQRT_2;
    let half = if within {
        (reach.max(0.0) / (1.0 - reach * reach).sqrt()) * (1.0 + 1e-9)
    } else {
        1.0
    };
    let g = 2.0_f64.mul_add(half * half, 1.0).sqrt() * (1.0 + 1e-12);
    // the rounding of a row's coefficients (dot products with base, u and w, a few ulps of 1
    // each) over the chart
    let row = 16.0 * f64::EPSILON * 2.0_f64.mul_add(half, 1.0);
    let (mut enough, mut needed) = (Rows::chart(half), Rows::chart(half));
    for run in runs {
        let (direction, slope, plane) = match *run {
            FaceRun::Plane { normal, slope } => (normal, slope, true),
            FaceRun::Axis { axis, slope } => (axis, slope, false),
            FaceRun::Other => continue,
        };
        if slope == f64::INFINITY {
            continue;
        }
        if !slope.is_finite() {
            return Lean::Undecided;
        }
        let need = accepted(slope, plane);
        if plane {
            let (c, a, b) = (direction.dot(base), direction.dot(u), direction.dot(w));
            enough.band(a, b, c, slope);
            needed.band(a, b, c, need.mul_add(g, row));
            continue;
        }
        let axis = if direction.dot(base) < 0.0 {
            -direction
        } else {
            direction
        };
        let cos = axis.dot(base);
        if cos < 0.5 {
            // the band's directions lie 60° or more off base, less its width (under 15°); the
            // chart's within 45°
            if within && need < 0.25 {
                return Lean::Nowhere;
            }
            return Lean::Undecided;
        }
        let at = (axis.dot(u) / cos, axis.dot(w) / cos);
        enough.discs.push((at, slope));
        let off = 16.0 * f64::EPSILON * (1.0 + at.0.abs() + at.1.abs());
        needed.discs.push((at, (need * g + off) / cos));
    }
    if enough.lines.len() + enough.discs.len() > LEAN_ROWS {
        return Lean::Undecided;
    }
    if !enough.never {
        if let Some(((p, q), r)) = enough.centre() {
            if r > 0.0 {
                if let Some((_, d)) = (base + u * p + w * q).length_and_unit() {
                    if inside(d, runs) {
                        return Lean::Found(d);
                    }
                }
            }
        }
    }
    if !within {
        return Lean::Undecided;
    }
    if needed.never {
        return Lean::Nowhere;
    }
    match needed.half_planes() {
        Some(planes) if no_point(&planes, half) => Lean::Nowhere,
        _ => Lean::Undecided,
    }
}

/// A point of the chart.
type P2 = (f64, f64);

/// [`lean`]'s constraints in the chart: lines `m · x + r <= h` (`m` unit) and discs
/// `|x - c| + r <= t`, `r` the radius of a circle around `x` inside them all.
#[derive(Default)]
struct Rows {
    lines: Vec<(P2, f64)>,
    discs: Vec<(P2, f64)>,
    /// A band no direction near the chart is inside (a plane square to `base`).
    never: bool,
}

/// How many sides [`Rows::half_planes`] takes around each disc.
const SIDES: u32 = 16;

/// The most half-planes [`no_point`] takes: past it no direction is proved absent.
const CERT_ROWS: usize = 200;

impl Rows {
    /// The chart `|p|, |q| <= half`.
    fn chart(half: f64) -> Self {
        let lines = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
            .into_iter()
            .map(|m| (m, half))
            .collect();
        Self {
            lines,
            discs: Vec::new(),
            never: false,
        }
    }

    /// The band `|c + a p + b q| <= slope`.
    fn band(&mut self, a: f64, b: f64, c: f64, slope: f64) {
        let n = a.hypot(b);
        if n == 0.0 {
            self.never |= c.abs() > slope;
            return;
        }
        self.lines.push(((a / n, b / n), (slope - c) / n));
        self.lines.push(((-a / n, -b / n), (slope + c) / n));
    }

    /// The lines, and each disc as the half-planes of the [`SIDES`]-gon around it (whose sides
    /// touch it, so it holds the disc); `None` past [`CERT_ROWS`].
    fn half_planes(&self) -> Option<Vec<(P2, f64)>> {
        let mut out = self.lines.clone();
        for &(c, t) in &self.discs {
            for k in 0..SIDES {
                let (sin, cos) = (TAU * f64::from(k) / f64::from(SIDES)).sin_cos();
                out.push(((cos, sin), cos.mul_add(c.0, sin * c.1) + t));
            }
        }
        (out.len() <= CERT_ROWS).then_some(out)
    }

    /// Whether the circle of radius `r` around `x` is inside every constraint, within their
    /// rounding.
    fn holds(&self, x: P2, r: f64) -> bool {
        let lines = self.lines.iter().all(|&(m, h)| {
            let slack =
                64.0 * f64::EPSILON * ((m.0 * x.0).abs() + (m.1 * x.1).abs() + r.abs() + h.abs());
            m.0 * x.0 + m.1 * x.1 + r <= h + slack
        });
        lines
            && self.discs.iter().all(|&(c, t)| {
                let d = (x.0 - c.0).hypot(x.1 - c.1);
                let slack = 64.0
                    * f64::EPSILON
                    * (d + r.abs() + t.abs() + c.0.abs() + c.1.abs() + x.0.abs() + x.1.abs());
                d + r <= t + slack
            })
    }

    /// The centre and radius of the largest circle inside every constraint that its points
    /// tried hold (a negative radius when they have no point in common), `None` when none
    /// holds. It is a convex problem in `(x, r)`, so its best is where one to three constraints
    /// are tight, and every such set is tried: three lines; two lines and a disc, or a strip
    /// (two opposite lines) and a disc; a line and two discs; three discs; a line and a disc;
    /// two discs; a disc alone. Only sets whose equations have no solution at all are skipped,
    /// each point found is checked against every constraint, and none of it proves there is
    /// no point ([`no_point`] does).
    fn centre(&self) -> Option<(P2, f64)> {
        let mut best: Option<(P2, f64)> = None;
        let mut consider = |x: P2, r: f64| {
            let fine = x.0.is_finite() && x.1.is_finite() && r.is_finite();
            if fine && best.is_none_or(|(_, b)| r > b) && self.holds(x, r) {
                best = Some((x, r));
            }
        };
        let (lines, discs) = (&self.lines, &self.discs);
        // three lines
        for (i, &(m1, h1)) in lines.iter().enumerate() {
            for (j, &(m2, h2)) in lines.iter().enumerate().skip(i + 1) {
                for &(m3, h3) in lines.iter().skip(j + 1) {
                    let (r1, r2, r3) = (
                        Vec3::new(m1.0, m1.1, 1.0),
                        Vec3::new(m2.0, m2.1, 1.0),
                        Vec3::new(m3.0, m3.1, 1.0),
                    );
                    let (c23, c31, c12) = (r2.cross(r3), r3.cross(r1), r1.cross(r2));
                    let det = r1.dot(c23);
                    if det == 0.0 {
                        continue;
                    }
                    let x = (c23 * h1 + c31 * h2 + c12 * h3) / det;
                    consider((x.x, x.y), x.z);
                }
            }
        }
        // two lines and a disc; two opposite lines (a plane's strip) and a disc
        for (i, &(m1, h1)) in lines.iter().enumerate() {
            for &(m2, h2) in lines.iter().skip(i + 1) {
                if let (Some(x0), Some(v)) = (solve2(m1, m2, h1, h2), solve2(m1, m2, -1.0, -1.0)) {
                    for &(c, t) in discs {
                        on_disc(x0, v, c, t, &mut consider);
                    }
                } else {
                    for &(c, t) in discs {
                        on_strip((m1, h1), (m2, h2), c, t, &mut consider);
                    }
                }
            }
        }
        // a line and two discs, three discs: subtracting two discs' equations is linear
        for (k, &(ck, tk)) in discs.iter().enumerate() {
            for (l, &(cl, tl)) in discs.iter().enumerate().skip(k + 1) {
                let (a, ar, ka) = disc_pair(ck, tk, cl, tl);
                for &(m, h) in lines {
                    if let (Some(x0), Some(v)) = (solve2(a, m, ka, h), solve2(a, m, -ar, -1.0)) {
                        on_disc(x0, v, ck, tk, &mut consider);
                    }
                }
                for &(cq, tq) in discs.iter().skip(l + 1) {
                    let (b, br, kb) = disc_pair(ck, tk, cq, tq);
                    if let (Some(x0), Some(v)) = (solve2(a, b, ka, kb), solve2(a, b, -ar, -br)) {
                        on_disc(x0, v, ck, tk, &mut consider);
                    }
                }
                // the two discs alone: the circle in their lens, on the line of their centres
                let (dx, dy) = (cl.0 - ck.0, cl.1 - ck.1);
                let gap = dx.hypot(dy);
                if gap > 0.0 {
                    let r = 0.5 * (tk + tl - gap);
                    let s = (tk - r) / gap;
                    consider((ck.0 + s * dx, ck.1 + s * dy), r);
                }
            }
        }
        // a line and a disc: the circle on the line's normal through the disc's centre
        for &(m, h) in lines {
            for &(c, t) in discs {
                let r = 0.5 * (h - m.0 * c.0 - m.1 * c.1 + t);
                consider((c.0 - (t - r) * m.0, c.1 - (t - r) * m.1), r);
            }
        }
        // a disc alone
        for &(c, t) in discs {
            consider(c, t);
        }
        best
    }
}

/// `x` with `a · x = ra` and `b · x = rb`, `None` when `a` and `b` are parallel (their
/// determinant 0 or not finite).
fn solve2(a: P2, b: P2, ra: f64, rb: f64) -> Option<P2> {
    let det = a.0 * b.1 - a.1 * b.0;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    Some(((ra * b.1 - a.1 * rb) / det, (a.0 * rb - ra * b.0) / det))
}

/// Two discs' equations `|x - c|² = (t - r)²` subtracted: `a · x + ar r = k`.
fn disc_pair(ck: P2, tk: f64, cl: P2, tl: f64) -> (P2, f64, f64) {
    let a = (2.0 * (cl.0 - ck.0), 2.0 * (cl.1 - ck.1));
    let k = tk * tk - tl * tl - (ck.0 * ck.0 + ck.1 * ck.1) + (cl.0 * cl.0 + cl.1 * cl.1);
    (a, 2.0 * (tk - tl), k)
}

/// The points `x = x0 + r v` where the disc around `c` of radius `t` is tight,
/// `|x - c| = t - r` with `r <= t`, handed to `consider`.
fn on_disc(x0: P2, v: P2, c: P2, t: f64, consider: &mut impl FnMut(P2, f64)) {
    let e = (x0.0 - c.0, x0.1 - c.1);
    let qa = v.0 * v.0 + v.1 * v.1 - 1.0;
    let qb = 2.0 * (e.0 * v.0 + e.1 * v.1 + t);
    let qc = e.0 * e.0 + e.1 * e.1 - t * t;
    let mut roots = [f64::NAN; 2];
    if qa == 0.0 {
        if qb != 0.0 {
            roots[0] = -qc / qb;
        }
    } else {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let root = disc.sqrt();
            roots = [(-qb + root) / (2.0 * qa), (-qb - root) / (2.0 * qa)];
        }
    }
    for r in roots {
        if r <= t {
            consider((x0.0 + r * v.0, x0.1 + r * v.1), r);
        }
    }
}

/// The points where two opposite lines (a plane's strip, `m2 = -m1`) and the disc around `c` of
/// radius `t` are tight, handed to `consider`: both lines tight fix `r` to half the strip's
/// width and put `x` on its midline, and along it the disc is tight where `|x - c| = t - r`.
/// The point of the midline nearest `c` is tried too (brett19 on #22066: a strip narrower than
/// the disc).
fn on_strip(
    (m1, h1): (P2, f64),
    (m2, h2): (P2, f64),
    c: P2,
    t: f64,
    consider: &mut impl FnMut(P2, f64),
) {
    if m1.0 * m2.0 + m1.1 * m2.1 >= 0.0 {
        return;
    }
    let r = 0.5 * (h1 + h2);
    let mid = 0.5 * (h1 - h2);
    let along = (-m1.1, m1.0);
    let x0 = (m1.0 * mid, m1.1 * mid);
    let e = (x0.0 - c.0, x0.1 - c.1);
    let et = e.0 * along.0 + e.1 * along.1;
    consider((x0.0 - et * along.0, x0.1 - et * along.1), r);
    let room = (t - r) * (t - r) - (e.0 * e.0 + e.1 * e.1) + et * et;
    if room >= 0.0 && r <= t {
        for s in [-et + room.sqrt(), -et - room.sqrt()] {
            consider((x0.0 + s * along.0, x0.1 + s * along.1), r);
        }
    }
}

/// Whether the half-planes `m · x <= h`, with `|p|, |q| <= half` among them, are proved to have
/// no point in common. Half-planes in the plane with none in common have two or three with none
/// (Helly), and for those some `λ >= 0` gives `Σ λ h < Σ λ m · x` for every `x` (Farkas): for two,
/// `λ = (1, 1)`; for three, `λᵢ` the cross product of the other two normals (so `Σ λ m` is 0 but
/// for rounding), all of one sign. Any `λ >= 0`, as computed, is a proof when
/// `Σ λ h + |Σ λ m|₁ half < 0`, the `half` term bounding `(Σ λ m) · x` over the chart, which
/// holds every point; the sums are taken with a margin of 16 ulps of their terms.
fn no_point(planes: &[(P2, f64)], half: f64) -> bool {
    let cross = |p: P2, q: P2| p.0 * q.1 - p.1 * q.0;
    for (i, &a) in planes.iter().enumerate() {
        for (j, &b) in planes.iter().enumerate().skip(i + 1) {
            if farkas(&[(1.0, a), (1.0, b)], half) {
                return true;
            }
            for &c in planes.iter().skip(j + 1) {
                let mut lam = [cross(b.0, c.0), cross(c.0, a.0), cross(a.0, b.0)];
                if lam.iter().all(|&l| l <= 0.0) {
                    lam = lam.map(|l| -l);
                }
                if lam.iter().any(|&l| l < 0.0) || lam.iter().all(|&l| l == 0.0) {
                    continue;
                }
                if farkas(&[(lam[0], a), (lam[1], b), (lam[2], c)], half) {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether the half-planes `m · x <= h` of `set`, each with its `λ >= 0`, prove that no `x` with
/// `|p|, |q| <= half` meets them all: `Σ λ h + |Σ λ m|₁ half < 0`, with a margin of 16 ulps of
/// the terms ([`no_point`]).
fn farkas(set: &[(f64, (P2, f64))], half: f64) -> bool {
    let (mut sum, mut ex, mut ey, mut size) = (0.0, 0.0, 0.0, 0.0);
    for &(lam, (m, h)) in set {
        sum += lam * h;
        ex += lam * m.0;
        ey += lam * m.1;
        size += lam.abs() * half.mul_add(m.0.abs() + m.1.abs(), h.abs());
    }
    (ex.abs() + ey.abs()).mul_add(half, sum) < -16.0 * f64::EPSILON * size
}

/// Two unit vectors square to the unit `v` and to each other.
fn square_pair(v: Vec3) -> Option<(Vec3, Vec3)> {
    let pick = if v.x.abs() < 0.5 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let (_, u) = (pick - v * pick.dot(v)).length_and_unit()?;
    let (_, w) = v.cross(u).length_and_unit()?;
    Some((u, w))
}

/// `v` in its canonical sense: its largest component positive (the first of equal ones), with
/// no negative zero (so a flipped `v` sorts as `v` does).
fn canonical(v: Vec3) -> Vec3 {
    let mut lead = v.x;
    for c in [v.y, v.z] {
        if c.abs() > lead.abs() {
            lead = c;
        }
    }
    let v = if lead < 0.0 { -v } else { v };
    Vec3::new(v.x + 0.0, v.y + 0.0, v.z + 0.0)
}

/// The size a body's tolerances scale with: the diagonal of its vertices' box (at least 1).
pub(crate) fn body_size(b: &Brep) -> Option<f64> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for e in crate::query::api_queries::get_edges(b).ok()? {
        let run = edge_run(b, e, true)?;
        for p in run.ends {
            any = true;
            for ((l, h), c) in lo.iter_mut().zip(hi.iter_mut()).zip([p.x, p.y, p.z]) {
                *l = l.min(c);
                *h = h.max(c);
            }
        }
    }
    let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
    let d = Vec3::new(x1 - x0, y1 - y0, z1 - z0).length();
    (any && d.is_finite()).then_some(d.max(1.0))
}

/// The uniform prism `b` is along `frame`'s normal, when its current geometry proves it one (see
/// the module's documentation for what is proved). `None` when it is not proved, including a
/// budget run out: never a guess.
#[must_use]
pub fn section(k: &Kernel, b: &Brep, frame: &Frame) -> Option<PrismSection> {
    if !super::profile::frame_is_orthonormal(frame) {
        return None;
    }
    // Apply the certificate's topology budgets before asking for curve and face extrema.
    if get_faces(b).ok()?.len() > 16_384
        || crate::query::api_queries::get_edges(b).ok()?.len() > 65_536
    {
        return None;
    }
    // Include curved boundaries in the object scale: a closed circular edge can have just
    // one seam vertex, whose vertex box omits the circle's diameter entirely.
    let bounds = crate::query::api_queries::brep_bounding_box(k, b, true).ok()?;
    let size = (bounds.max() - bounds.min()).length();
    if !size.is_finite() {
        return None;
    }
    let size = size.max(1.0);
    let tol = size * 1e-10;
    // The query's origin is arbitrary. Prove area, volume and matching supports near the
    // body, then express the answer in the requested coordinates. Keep the axes unchanged.
    let origin = face_points_of_first(b)?;
    let local = Frame {
        origin: [origin.x, origin.y, origin.z],
        x_axis: frame.x_axis,
        y_axis: frame.y_axis,
    };
    let c = certificate(k, b, size, &local, tol)?;
    let shift = to_local(frame, origin);
    let level = height(frame, origin);
    let (o, _, _, _) = axes(frame);
    // Subtraction of the origins and projection (three products, two sums, and the normal's
    // cross product): 64 eps times the input lengths bounds their accumulated rounding.
    let projection_error = 64.0 * f64::EPSILON * (o.length() + origin.length()).max(1.0);
    let translate = |p: Point2| {
        let q = p + shift;
        let error = projection_error + 8.0 * f64::EPSILON * (p.length() + shift.length()).max(1.0);
        // Refuse an unrepresentable section instead of widening the certificate's corridor.
        (error <= tol && q.x.is_finite() && q.y.is_finite()).then_some(q)
    };
    let mut outer = None;
    let mut holes = Vec::new();
    for lp in c.loops {
        let positive = lp.area.lo > 0.0;
        let mut segs = lp.segments;
        for segment in &mut segs {
            match segment {
                Seg2::Line { a, b } => {
                    *a = translate(*a)?;
                    *b = translate(*b)?;
                }
                Seg2::Arc { center, .. } => *center = translate(*center)?,
                _ => return None,
            }
        }
        let lp = Loop2 { segs };
        if positive {
            if outer.replace(lp).is_some() {
                return None;
            }
        } else {
            holes.push(lp);
        }
    }
    let (lo, hi) = (c.lo + level, c.hi + level);
    // Cap vertices are within twice tol of the centered levels. Include projection and the
    // final additions; the section's conversion error above is also bounded by this allowance.
    let tolerance = 2.0 * tol
        + projection_error
        + 8.0 * f64::EPSILON * (c.lo.abs() + c.hi.abs() + level.abs()).max(1.0);
    if !(lo.is_finite() && hi.is_finite() && lo < hi && tolerance.is_finite()) {
        return None;
    }
    Some(PrismSection {
        lo,
        hi,
        region: Region2 {
            outer: outer?,
            holes,
        },
        tolerance,
        volume: c.volume,
    })
}

/// Whether `a` and `b` are proved the same uniform prism: along a direction one of `a`'s planar
/// faces is square to, both certified, their levels and loops the same within 1e-7 of their
/// size, their volumes' enclosures within 1e-5 of each other relatively. `Some(false)` when proved
/// different along that direction; `None` when nothing is proved (never a copy granted).
#[must_use]
pub fn equivalent(k: &Kernel, a: &Brep, b: &Brep) -> Option<bool> {
    let (size_a, size_b) = (body_size(a)?, body_size(b)?);
    let size = size_a.max(size_b);
    let tol = size * 1e-10;
    let identity_tol = size * 1e-7;
    let origin = face_points_of_first(a)?;
    let mut candidates: Vec<Vec3> = Vec::new();
    for f in get_faces(a).ok()? {
        let Some((_, n)) = b_plane(a, f) else {
            continue;
        };
        let n = if outward_is_natural(a, f)? { n } else { -n };
        if !candidates.iter().any(|d| d.dot(n).abs() > 1.0 - 1e-12) {
            candidates.push(n);
            if candidates.len() == 8 {
                break;
            }
        }
    }
    for d in candidates {
        let frame = frame_from_normal(origin, d)?;
        let Some(ca) = certificate(k, a, size_a, &frame, tol) else {
            continue;
        };
        let Some(cb) = certificate(k, b, size_b, &frame, tol) else {
            continue;
        };
        if (ca.lo - cb.lo).abs() > identity_tol
            || (ca.hi - cb.hi).abs() > identity_tol
            || ca.loops.len() != cb.loops.len()
        {
            return Some(false);
        }
        let mut work = 16_000_000usize;
        let mut used = vec![false; cb.loops.len()];
        for lp in &ca.loops {
            let mut matched = None;
            for (i, other) in cb.loops.iter().enumerate() {
                if !used.get(i).copied()? && loops_same(lp, other, identity_tol, &mut work)? {
                    if matched.is_some() {
                        return None;
                    }
                    matched = Some(i);
                }
            }
            let Some(i) = matched else { return Some(false) };
            *used.get_mut(i)? = true;
        }
        let delta = (ca.volume.hi - cb.volume.lo)
            .abs()
            .max((cb.volume.hi - ca.volume.lo).abs());
        return Some(delta <= 1e-5 * (1.0 + ca.volume.lo.abs()));
    }
    None
}

fn b_plane(b: &Brep, f: FaceId) -> Option<(Point3, Vec3)> {
    plane_of(b.face(f).ok()?.surface.as_ref()?)
}

fn face_points_of_first(b: &Brep) -> Option<Point3> {
    let e = *crate::query::api_queries::get_edges(b).ok()?.first()?;
    Some(edge_run(b, e, true)?.ends[0])
}

/// An orthonormal frame at `origin` whose normal is the unit `n`: x the unit vector square to `n`
/// along the world axis it is least along, y = n × x.
fn frame_from_normal(origin: Point3, n: Vec3) -> Option<Frame> {
    if !((n.length() - 1.0).abs() <= 1e-12) {
        return None;
    }
    let axis = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if n.y.abs() <= n.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let x = (axis - n * axis.dot(n)).unitize().ok()?;
    let y = n.cross(x);
    Some(Frame {
        origin: [origin.x, origin.y, origin.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// A circle retraced is no circle, and a square run twice is no separated material.
    #[test]
    fn winding_retracing_and_uncertain_material_do_not_get_certificates() {
        let arc = Seg2::Arc {
            center: Vec2::ZERO,
            radius: 6.0,
            start: 0.0,
            sweep: TAU,
        };
        assert!(circle(&[arc, arc.reversed(), arc], 1e-6).is_none());
        assert!(circle(&[arc], 1e-6).is_some());
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let corners = [p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0), p(0.0, 20.0)];
        let square: Vec<Seg2> = corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .map(|(a, b)| Seg2::Line { a: *a, b: *b })
            .collect();
        let once = CapLoop {
            segments: square.clone(),
            area: Interval::enclosing(400.0, 1e-8).unwrap(),
        };
        assert!(separated_material(std::slice::from_ref(&once), 1e-6, &mut 16_000_000).is_some());
        let doubled = CapLoop {
            segments: square.iter().chain(&square).copied().collect(),
            area: Interval::enclosing(800.0, 1e-8).unwrap(),
        };
        assert!(separated_material(&[doubled], 1e-6, &mut 16_000_000).is_none());
    }

    /// An extrusion's wall (here a free-form curve swept along z) is bounded by its direction
    /// against the prism's, whatever its generator: straight along z, and a degree off is not.
    #[test]
    fn an_extrusion_wall_is_bounded_by_its_direction() {
        let mut k = Kernel::default();
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let region = Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Cubic {
                        p0: p(0.0, 0.0),
                        p1: p(4.0, -2.0),
                        p2: p(8.0, 2.0),
                        p3: p(12.0, 0.0),
                    },
                    Seg2::Line {
                        a: p(12.0, 0.0),
                        b: p(12.0, 6.0),
                    },
                    Seg2::Line {
                        a: p(12.0, 6.0),
                        b: p(0.0, 6.0),
                    },
                    Seg2::Line {
                        a: p(0.0, 6.0),
                        b: p(0.0, 0.0),
                    },
                ],
            },
            holes: Vec::new(),
        };
        let frame = Frame {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        };
        let b = crate::prism::extrude::extrude_region(&mut k, &frame, &region, 0.0, 120.0).unwrap();
        let extrusions: Vec<&Surface> = b
            .face_list()
            .iter()
            .filter_map(|&f| b.face(f).ok()?.surface.as_ref())
            .filter(|s| matches!(s.kind(), SurfaceKind::SurfOfExtrusion(_)))
            .collect();
        assert_eq!(extrusions.len(), 1);
        let mut work = usize::MAX;
        let along = wall_error(
            extrusions[0],
            Vec3::new(0.0, 0.0, 1.0),
            120.0,
            &mut work,
            true,
        )
        .unwrap();
        assert!(along < 1e-9, "{along}");
        let off = Vec3::new(0.0, 1.0_f64.to_radians().sin(), 1.0_f64.to_radians().cos());
        let skew = wall_error(extrusions[0], off, 120.0, &mut work, true).unwrap();
        // 4 x 120 x sin(1 degree), and the 1024 eps
        assert!(
            (skew - 480.0 * 1.0_f64.to_radians().sin()).abs() < 1e-9,
            "{skew}"
        );
    }
}

#[cfg(test)]
mod straight_tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn plane(x: f64, y: f64, z: f64) -> FaceRun {
        FaceRun::Plane {
            normal: v(x, y, z).unitize().unwrap(),
            slope: 1.0,
        }
    }

    fn axis(x: f64, y: f64, z: f64) -> FaceRun {
        FaceRun::Axis {
            axis: v(x, y, z),
            slope: 1.0,
        }
    }

    /// The directions of `runs`' candidates, checking the search was decided.
    fn found(runs: &[FaceRun]) -> Vec<(Vec3, StraightBy)> {
        let got = straight_candidates(runs);
        assert!(got.complete, "{got:?}");
        got.directions
    }

    /// Two planes that are not parallel give their cross product first, whatever the order and
    /// sense of the runs; a cylinder's axis comes after (once); parallel planes alone give nothing.
    #[test]
    fn a_plane_pair_comes_before_an_axis_in_any_order_and_sense() {
        let runs = [
            plane(1.0, 0.0, 0.0),
            FaceRun::Other,
            plane(0.0, 1.0, 0.0),
            axis(0.0, 0.0, -1.0),
        ];
        let got = found(&runs);
        // the axis is the pair's direction, so it is not listed again
        assert_eq!(
            got,
            vec![(v(0.0, 0.0, 1.0), StraightBy::Planes(2, 0))],
            "{got:?}"
        );
        let flipped = [
            axis(0.0, 0.0, 1.0),
            plane(0.0, -1.0, 0.0),
            plane(-1.0, 0.0, 0.0),
        ];
        let got = found(&flipped);
        assert_eq!(got.first().map(|c| c.0), Some(v(0.0, 0.0, 1.0)), "{got:?}");
        // parallel planes and a cylinder: the cylinder's axis
        let round = [
            plane(0.0, 1.0, 0.0),
            axis(0.0, 0.0, -1.0),
            plane(0.0, -1.0, 0.0),
        ];
        assert_eq!(found(&round), vec![(v(0.0, 0.0, 1.0), StraightBy::Axis(1))]);
        // parallel planes only: nothing
        let flat = [plane(0.0, 1.0, 0.0), plane(0.0, 1.0, 1e-13)];
        assert!(found(&flat).is_empty());
    }

    /// brett19's runs (+x, +y, -x) on #22066: flipping the first and last senses changes neither
    /// the direction nor the faces it names (no negative zero sorts ahead of a zero).
    #[test]
    fn a_flipped_sense_names_the_same_faces() {
        let runs = [
            plane(1.0, 0.0, 0.0),
            plane(0.0, 1.0, 0.0),
            plane(-1.0, 0.0, 0.0),
        ];
        let flipped = [
            plane(-1.0, 0.0, 0.0),
            plane(0.0, 1.0, 0.0),
            plane(1.0, 0.0, 0.0),
        ];
        let got = found(&runs);
        assert_eq!(found(&flipped), got);
        assert_eq!(got, vec![(v(0.0, 0.0, 1.0), StraightBy::Planes(1, 0))]);
    }

    /// brett19's three faces on #22066, each tilted off square to y by 0.9 of its band: y is in
    /// every band, the cross product of the two furthest from parallel is not (the third strays
    /// along it by twice its band), so the next candidate is a direction inside every band.
    #[test]
    fn a_direction_inside_every_band_follows_a_pair_outside_one() {
        let slope = 1e-5 / (2.0 * 10.0 * 2.0_f64.sqrt());
        let alpha = 0.9 * slope;
        let band = |n: Vec3| FaceRun::Plane {
            normal: n.unitize().unwrap(),
            slope,
        };
        let runs = [
            band(v(0.0, alpha, 1.0)),
            band(v(1.0, alpha, 0.0)),
            band(v(1.0, -alpha, 1.0)),
        ];
        assert!(inside(v(0.0, 1.0, 0.0), &runs));
        let got = found(&runs);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(!inside(got[0].0, &runs), "{got:?}");
        assert_eq!(got[1].1, StraightBy::Bands);
        assert!(inside(got[1].0, &runs), "{got:?}");
        assert!((got[1].0.length() - 1.0).abs() < 4.0 * f64::EPSILON);
        // the same planes with bands too narrow to meet: proved that no direction is inside them
        let narrow: Vec<FaceRun> = runs
            .iter()
            .map(|r| match *r {
                FaceRun::Plane { normal, .. } => FaceRun::Plane {
                    normal,
                    slope: 0.3 * alpha,
                },
                other => other,
            })
            .collect();
        assert_eq!(found(&narrow).len(), 1);
        // a cylinder whose axis leans off the planes' direction by half its band
        let lean = 0.5 * slope;
        let cylinder = [
            band(v(0.0, alpha, 1.0)),
            band(v(1.0, alpha, 0.0)),
            FaceRun::Axis {
                axis: v(lean, 1.0, 0.0).unitize().unwrap(),
                slope,
            },
        ];
        let got = found(&cylinder);
        let bands = got.iter().find(|c| c.1 == StraightBy::Bands);
        assert!(bands.is_some_and(|c| inside(c.0, &cylinder)), "{got:?}");
    }

    /// brett19's lens on #22066: planes square to x and y and a cylinder, bands 1e-6, the
    /// cylinder's axis leaning off z towards the planes' corner so that its disc reaches the
    /// corner at f of its band (f = 0.93, 0.96, 0.99), the whole turned about z through 64
    /// angles. Every one has a direction inside every band, and the search finds one.
    #[test]
    fn a_cylinder_band_meeting_two_plane_bands_in_a_lens_is_found_at_every_turn() {
        let (s, t) = (1e-6, 1e-6);
        for f in [0.93, 0.96, 0.99] {
            let reach = s + f * t * FRAC_1_SQRT_2;
            for k in 0..64 {
                let turn = TAU * f64::from(k) / 64.0;
                let (sin, cos) = turn.sin_cos();
                let at = |x: f64, y: f64, z: f64| v(cos * x - sin * y, sin * x + cos * y, z);
                let runs = [
                    FaceRun::Plane {
                        normal: at(1.0, 0.0, 0.0),
                        slope: s,
                    },
                    FaceRun::Plane {
                        normal: at(0.0, 1.0, 0.0),
                        slope: s,
                    },
                    FaceRun::Axis {
                        axis: at(reach, reach, 1.0).unitize().unwrap(),
                        slope: t,
                    },
                ];
                let got = found(&runs);
                let bands = got.iter().find(|c| c.1 == StraightBy::Bands);
                assert!(
                    bands.is_some_and(|c| inside(c.0, &runs)),
                    "f {f}, turn {k}: {got:?}"
                );
            }
        }
    }

    /// `n` planes, each tilted off square to y by 0.9 of its band, their normals spread round y.
    fn tilted(n: u32) -> Vec<FaceRun> {
        let slope = 1e-5 / (2.0 * 10.0 * 2.0_f64.sqrt());
        (0..n)
            .map(|k| {
                let (sin, cos) = (TAU * f64::from(k) / f64::from(n)).sin_cos();
                FaceRun::Plane {
                    normal: v(cos, 0.9 * slope, sin).unitize().unwrap(),
                    slope,
                }
            })
            .collect()
    }

    /// brett19's 33 and 40 tilted planes on #22066 are searched and found; past the search's
    /// constraints the candidates say the search was not decided, not that no direction is.
    #[test]
    fn many_tilted_planes_are_found_and_past_the_budget_undecided() {
        for n in [3, 16, 33, 40] {
            let runs = tilted(n);
            let got = found(&runs);
            let bands = got.iter().find(|c| c.1 == StraightBy::Bands);
            assert!(bands.is_some_and(|c| inside(c.0, &runs)), "{n}: {got:?}");
        }
        let runs = tilted(60);
        let got = straight_candidates(&runs);
        assert!(!got.complete, "{got:?}");
        assert!(
            got.directions.iter().all(|c| c.1 != StraightBy::Bands),
            "{got:?}"
        );
    }

    /// Whether `runs`' candidates were decided and one of them is inside every band.
    fn one_inside(runs: &[FaceRun]) -> bool {
        let got = straight_candidates(runs);
        got.complete && got.directions.iter().any(|c| inside(c.0, runs))
    }

    /// brett19's runs on #22066 (review of 84964e53), each with a direction inside every band
    /// that the search missed: a 106 mm plane and two cylinders (refused as not straight), two
    /// planes a hair from parallel and a third (refused), and a plane and a cylinder (undecided).
    /// The search now finds one in each: a plane's strip with a cylinder's disc, and three lines
    /// nearly dependent, are tried.
    #[test]
    fn brett19_s_runs_with_a_direction_find_one() {
        let unit = |x: f64, y: f64, z: f64| v(x, y, z).unitize().unwrap();
        let plane = |normal: Vec3, slope: f64| FaceRun::Plane { normal, slope };
        let round = |axis: Vec3, slope: f64| FaceRun::Axis { axis, slope };
        let two_cylinders = [
            plane(
                unit(
                    0.962_086_425_830_913_4,
                    0.272_744_769_392_735_06,
                    -8.956_549_741_183_395e-7,
                ),
                4.714e-8,
            ),
            round(v(0.0, 0.0, 1.0), 1e-6),
            round(
                unit(4.168_275_780_491_783e-7, -9.927_491_530_186_688e-7, 1.0),
                1.2708e-6,
            ),
        ];
        assert!(inside(unit(9.817e-7, -1.790e-7, 1.0), &two_cylinders));
        assert!(
            one_inside(&two_cylinders),
            "{:?}",
            straight_candidates(&two_cylinders)
        );
        for (lean, slope) in [
            (1e-10, 1e-12),
            (4e-10, 2e-11),
            (1e-9, 1e-11),
            (1e-10, 1e-13),
        ] {
            let (sin, cos) = f64::sin_cos(lean);
            let near = [
                plane(unit(1.0, 0.1, 0.0), slope),
                plane(unit(cos, 0.1, sin), slope),
                plane(unit(0.0, 0.05, 1.0), 0.1),
            ];
            assert!(inside(unit(-0.1, 1.0, 0.0), &near));
            assert!(
                one_inside(&near),
                "{lean} {slope}: {:?}",
                straight_candidates(&near)
            );
        }
        let two = [
            plane(unit(0.0, 1.0, 7e-7), 2e-7),
            round(v(0.0, 0.0, 1.0), 1e-6),
        ];
        assert!(inside(unit(0.0, -7e-7, 1.0), &two));
        assert!(one_inside(&two), "{:?}", straight_candidates(&two));
    }

    /// A plane's strip that misses a cylinder's disc, by far or by half its width: proved that
    /// no direction is inside both (decided, and no direction found).
    #[test]
    fn a_strip_missing_a_cylinder_s_disc_is_proved_to_have_none() {
        for off in [7e-6, 1.3e-6] {
            let runs = [
                FaceRun::Plane {
                    normal: v(0.0, 1.0, off).unitize().unwrap(),
                    slope: 2e-7,
                },
                FaceRun::Axis {
                    axis: v(0.0, 0.0, 1.0),
                    slope: 1e-6,
                },
            ];
            let got = found(&runs);
            assert!(got.iter().all(|c| !inside(c.0, &runs)), "{off}: {got:?}");
        }
    }

    /// brett19 on #22066 (reviews of 39d1970a and ea07889d): bands the search cannot settle are
    /// never proved to have none. A plane's strip and a cylinder's disc missing each other by
    /// 3e-13 at slopes of 2e-7 and 1e-6 meet only inside the widening to what `face_straying`
    /// accepts (`accepted`'s margins): undecided, and no direction is inside both. At slopes of 0.01
    /// and 0.05, 5e-5 apart, they meet on the unit sphere (the witness below) but not in the chart's
    /// unnormalised rows, where `enough` takes `|d| >= 1`: undecided, or a direction found inside
    /// both, never decided with none inside. Without the widening, without `g` and the rounding
    /// terms, or without the certificate, one of them is decided with none inside.
    #[test]
    fn bands_that_meet_only_inside_the_widening_are_undecided() {
        let runs = |slope: f64, t: f64, gap: f64| {
            [
                FaceRun::Plane {
                    normal: v(0.0, 1.0, slope + t + gap).unitize().unwrap(),
                    slope,
                },
                FaceRun::Axis {
                    axis: v(0.0, 0.0, 1.0),
                    slope: t,
                },
            ]
        };
        let tight = runs(2e-7, 1e-6, 3e-13);
        let got = straight_candidates(&tight);
        assert!(!got.complete, "{got:?}");
        assert!(
            got.directions.iter().all(|c| !inside(c.0, &tight)),
            "{got:?}"
        );
        let wide = runs(0.01, 0.05, 5e-5);
        assert!(inside(v(0.0, -0.05, (1.0_f64 - 0.0025).sqrt()), &wide));
        let got = straight_candidates(&wide);
        assert!(
            !got.complete || got.directions.iter().any(|c| inside(c.0, &wide)),
            "{got:?}"
        );
    }

    /// brett19 on #22066 (review of 39d1970a): a cylinder whose axis is 70° off the plane pair's
    /// direction, all three bands narrow, is proved to have no direction inside them all, by the
    /// axis's angle alone.
    #[test]
    fn an_axis_far_off_the_pair_is_proved_to_have_none() {
        let (sin, cos) = 70.0_f64.to_radians().sin_cos();
        let runs = [
            FaceRun::Plane {
                normal: v(1.0, 0.0, 0.0),
                slope: 1e-6,
            },
            FaceRun::Plane {
                normal: v(0.0, 1.0, 0.0),
                slope: 1e-6,
            },
            FaceRun::Axis {
                axis: v(sin, 0.0, cos),
                slope: 1e-6,
            },
        ];
        let got = found(&runs);
        assert!(got.iter().all(|c| !inside(c.0, &runs)), "{got:?}");
    }
}
