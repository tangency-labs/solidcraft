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
//!   axis along it; or a net ruled along it (degree one across, each pair of rows a translation
//!   along the direction within its hull's bound). Each wall stands on cap edges at both levels
//!   (its support read in the frame is the cap's), its other edges lines along the direction.
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

/// A whole bound on how far the wall strays across the direction `d` over `height`: a plane's
/// normal against `d`, a cylinder's axis against it, a ruled net's rows against it (positive
/// weights make the hull a bound on the surface). With `reach`, a net's rows must also run
/// monotonically along `d` for at least half of `height`, as a prism's wall from cap to cap does
/// ([`section`]); without it any rows do (the face then lies within the bound of its first row's
/// curve swept along `d`). `None` where none is proved.
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
/// `zone`. The band is [`face_straying`]'s plane or cylinder term solved for the direction, made
/// a little narrower so that it is enough: a plane strays by
/// `(|n · d| + 1024ε (Σ|nᵢ dᵢ| + 1)) · 2 · size`, with `Σ|nᵢ dᵢ| <= |n| |d| < 2`, and a cylinder by
/// `(|a × d| + 1024ε) · 4 · size`, each plus the definition's distance. A band of no width (a face
/// too large for the zone) is kept: nothing is inside it.
#[must_use]
pub fn face_run(k: &Kernel, b: &Brep, f: FaceId, zone: f64) -> FaceRun {
    let Some(surface) = b.face(f).ok().and_then(|face| face.surface.as_ref()) else {
        return FaceRun::Other;
    };
    let Some((size, definition)) = straying_scale(k, b, f, surface) else {
        return FaceRun::Other;
    };
    let room = zone - definition;
    if let Some((_, normal)) = plane_of(surface).and_then(|(_, n)| n.length_and_unit()) {
        let slope = room / (2.0 * size) - 3072.0 * f64::EPSILON;
        return FaceRun::Plane { normal, slope };
    }
    if let Some((_, axis)) = cylinder_of(surface).and_then(|(_, a, _)| a.length_and_unit()) {
        let slope = room / (4.0 * size) - 1024.0 * f64::EPSILON;
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

/// The most rows [`lean`] solves over: 64 band rows and its 4 chart rows.
const LEAN_ROWS: usize = 68;

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
/// which direction comes first, nor the indices it names. Nothing here proves a face straight
/// along a candidate; a caller checks each face with [`face_straying`]. Empty when no plane pair
/// is proved apart and no face is a cylinder.
#[must_use]
pub fn straight_candidates(runs: &[FaceRun]) -> Vec<(Vec3, StraightBy)> {
    let mut planes: Vec<(Vec3, usize)> = Vec::new();
    let mut axes: Vec<(Vec3, usize)> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        match *run {
            FaceRun::Plane { normal, .. } => planes.push((canonical(normal), i)),
            FaceRun::Axis { axis, .. } => axes.push((canonical(axis), i)),
            FaceRun::Other => {}
        }
    }
    let by_value = |a: &(Vec3, usize), b: &(Vec3, usize)| {
        let (p, q) = (a.0, b.0);
        p.x.total_cmp(&q.x)
            .then(p.y.total_cmp(&q.y))
            .then(p.z.total_cmp(&q.z))
            .then(a.1.cmp(&b.1))
    };
    planes.sort_by(by_value);
    axes.sort_by(by_value);
    let mut out = Vec::new();
    let mut best: Option<(f64, Vec3, StraightBy)> = None;
    for (i, &(n, a)) in planes.iter().enumerate() {
        for &(m, b) in planes.iter().skip(i + 1) {
            let Some((s, d)) = n.cross(m).length_and_unit() else {
                continue;
            };
            if s > PARALLEL && best.is_none_or(|(t, ..)| s > t) {
                best = Some((s, d, StraightBy::Planes(a, b)));
            }
        }
    }
    if let Some((_, d, by)) = best {
        out.push((canonical(d), by));
    }
    for &(a, i) in &axes {
        if !out.iter().any(|&(d, _)| d.cross(a).length() <= PARALLEL) {
            out.push((a, StraightBy::Axis(i)));
        }
    }
    if let Some(&(first, _)) = out.first() {
        if !inside(first, runs) {
            if let Some(d) = lean(first, runs) {
                out.insert(1, (canonical(d), StraightBy::Bands));
            }
        }
    }
    out
}

/// Whether the unit `d` is inside every plane's and cylinder's band of `runs`.
fn inside(d: Vec3, runs: &[FaceRun]) -> bool {
    runs.iter().all(|run| match *run {
        FaceRun::Plane { normal, slope } => normal.dot(d).abs() <= slope,
        FaceRun::Axis { axis, slope } => axis.cross(d).length() <= slope,
        FaceRun::Other => true,
    })
}

/// A unit direction inside every plane's and cylinder's band of `runs`, near the unit `base`, or
/// `None` when there is none in reach.
///
/// It is a small linear programme in the plane square to `base`: `d = base + p u + q w`, with `u`
/// and `w` unit, square to `base` and to each other, and `|p|, |q| <= 1`. A plane's band
/// `|n · d| <= slope` is two rows linear in `p` and `q`. A cylinder's band `|a × d| <= slope` is a
/// disc in the plane square to its axis; it takes the regular octagon inside it, four rows
/// `|e · d| <= slope cos(π/8)` for `e` square to the axis at every eighth of a turn. Every row is
/// enough for its band once `d` is made unit, as `|d| >= 1`. Of the points inside every row it
/// takes the one furthest inside (the centre of the largest circle that fits), found at the
/// corners of the rows taken three at a time; `None` when that circle has no radius, or past
/// [`LEAN_ROWS`] rows. So for planes the answer is exact up to the rounding of the rows, for
/// cylinders up to the octagon (a band `cos(π/8)` as wide); it reaches directions within 45° of
/// `base` across each of `u` and `w`.
fn lean(base: Vec3, runs: &[FaceRun]) -> Option<Vec3> {
    let (u, w) = square_pair(base)?;
    let mut rows: Vec<[f64; 3]> = vec![
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
    ];
    let mut band = |n: Vec3, slope: f64| {
        let (c, a, b) = (n.dot(base), n.dot(u), n.dot(w));
        rows.push([a, b, slope - c]);
        rows.push([-a, -b, slope + c]);
    };
    // cos and sin of every eighth of a turn up to a half, and cos(π/8)
    let eighths = [
        (1.0, 0.0),
        (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        (0.0, 1.0),
        (-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    ];
    let octagon = 0.923_879_532_511_286_7;
    for run in runs {
        let (direction, slope, round) = match *run {
            FaceRun::Plane { normal, slope } => (normal, slope, false),
            FaceRun::Axis { axis, slope } => (axis, slope, true),
            FaceRun::Other => continue,
        };
        if slope == f64::INFINITY {
            continue;
        }
        if !slope.is_finite() {
            return None;
        }
        if round {
            let (e1, e2) = square_pair(direction)?;
            for (c, s) in eighths {
                band(e1 * c + e2 * s, slope * octagon);
            }
        } else {
            band(direction, slope);
        }
    }
    if rows.len() > LEAN_ROWS {
        return None;
    }
    let row = |r: [f64; 3]| {
        let [a, b, h] = r;
        (Vec3::new(a, b, a.hypot(b)), h)
    };
    let holds = |x: Vec3| {
        rows.iter().all(|&r| {
            let (m, h) = row(r);
            let slack = 64.0
                * f64::EPSILON
                * ((m.x * x.x).abs() + (m.y * x.y).abs() + (m.z * x.z).abs() + h.abs());
            m.dot(x) <= h + slack
        })
    };
    let mut best: Option<Vec3> = None;
    for (i, &r1) in rows.iter().enumerate() {
        for (j, &r2) in rows.iter().enumerate().skip(i + 1) {
            for &r3 in rows.iter().skip(j + 1) {
                let ((m1, h1), (m2, h2), (m3, h3)) = (row(r1), row(r2), row(r3));
                let (c23, c31, c12) = (m2.cross(m3), m3.cross(m1), m1.cross(m2));
                let det = m1.dot(c23);
                if det.abs() <= 1e-9 * m1.length() * m2.length() * m3.length() {
                    continue;
                }
                let x = (c23 * h1 + c31 * h2 + c12 * h3) / det;
                if best.is_some_and(|b| b.z >= x.z) || !holds(x) {
                    continue;
                }
                best = Some(x);
            }
        }
    }
    let x = best?;
    if x.z <= 0.0 {
        return None;
    }
    let (_, d) = (base + u * x.x + w * x.y).length_and_unit()?;
    Some(d)
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
        let got = straight_candidates(&runs);
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
        let got = straight_candidates(&flipped);
        assert_eq!(got.first().map(|c| c.0), Some(v(0.0, 0.0, 1.0)), "{got:?}");
        // parallel planes and a cylinder: the cylinder's axis
        let round = [
            plane(0.0, 1.0, 0.0),
            axis(0.0, 0.0, -1.0),
            plane(0.0, -1.0, 0.0),
        ];
        assert_eq!(
            straight_candidates(&round),
            vec![(v(0.0, 0.0, 1.0), StraightBy::Axis(1))]
        );
        // parallel planes only: nothing
        let flat = [plane(0.0, 1.0, 0.0), plane(0.0, 1.0, 1e-13)];
        assert!(straight_candidates(&flat).is_empty());
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
        let got = straight_candidates(&runs);
        assert_eq!(straight_candidates(&flipped), got);
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
        let got = straight_candidates(&runs);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(!inside(got[0].0, &runs), "{got:?}");
        assert_eq!(got[1].1, StraightBy::Bands);
        assert!(inside(got[1].0, &runs), "{got:?}");
        assert!((got[1].0.length() - 1.0).abs() < 4.0 * f64::EPSILON);
        // the same planes with bands too narrow to meet: no direction inside them all
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
        assert_eq!(straight_candidates(&narrow).len(), 1);
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
        let got = straight_candidates(&cylinder);
        let bands = got.iter().find(|c| c.1 == StraightBy::Bands);
        assert!(bands.is_some_and(|c| inside(c.0, &cylinder)), "{got:?}");
    }
}
