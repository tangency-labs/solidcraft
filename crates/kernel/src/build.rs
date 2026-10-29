//! Building bodies: extrude and revolve planar regions, primitives, lofts and sweeps, each
//! through SMLib ([`crate::smlib_sweeps`]); a profile face as its schema 4 record
//! ([`planar_face_record`]).

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::draft_corners::DraftCorner;
use crate::numbered::{self, NumberedLoop, NumberedRegion};
use crate::sweep_path::RailFit;
use crate::{KernelError, Result};

const MAX_SEGS: usize = 20_000;

fn finite(x: f64, what: &str) -> Result<f64> {
    if x.is_finite() && x.abs() < 1e7 { Ok(x) } else { Err(KernelError::Invalid(format!("{what} = {x}"))) }
}

/// A region numbered on its own, as given.
fn numbered_region(r: &Region2) -> Result<NumberedRegion> {
    numbered::number_regions(std::slice::from_ref(r)).pop().ok_or_else(|| KernelError::Invalid("profile".into()))
}

/// A region numbered on its own and oriented as the builders build it.
fn oriented(r: &Region2) -> Result<NumberedRegion> {
    numbered_region(r).map(|n| n.oriented())
}

/// The outer loop a tapered extrusion takes: counterclockwise, collinear lines merged.
pub(crate) fn oriented_outer(r: &Region2) -> Result<Loop2> {
    Ok(oriented(r)?.map_loops(|l| l.merged_lines(continues_straight)).outer.lp)
}

pub(crate) fn region_ok(r: &Region2) -> Result<()> {
    if r.area().abs() < 1e-9 {
        return Err(KernelError::Invalid("profile has no area".into()));
    }
    if !std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs).all(seg_ok) {
        return Err(KernelError::Invalid("non-finite profile".into()));
    }
    Ok(())
}

/// A segment with finite, well-formed numbers.
fn seg_ok(s: &Seg2) -> bool {
    match *s {
        Seg2::Line { a, b } => a.is_finite() && b.is_finite(),
        Seg2::Arc { center, radius, start, sweep } => {
            center.is_finite() && radius.is_finite() && radius > 1e-9 && start.is_finite() && sweep.is_finite()
        }
        Seg2::Cubic { p0, p1, p2, p3 } => p0.is_finite() && p1.is_finite() && p2.is_finite() && p3.is_finite(),
        Seg2::Conic { a, apex, b, w } => a.is_finite() && apex.is_finite() && b.is_finite() && w.is_finite() && w > 1e-9,
        Seg2::Bezier { n, p } => (4..=solidcraft_geom::MAX_BEZIER_DEGREE as u8).contains(&n) && p.iter().all(|q| q.is_finite()),
    }
}

/// A finite, orthonormal sketch frame.
pub(crate) fn frame_ok(plane: &Plane) -> bool {
    plane.origin.is_finite()
        && plane.x.is_finite()
        && plane.y.is_finite()
        && (plane.x.len2() - 1.).abs() <= 1e-8
        && (plane.y.len2() - 1.).abs() <= 1e-8
        && plane.x.dot(plane.y).abs() <= 1e-8
}

/// Extrude each region from `start` to `end` along the plane normal (offsets in mm, `end > start`).
pub fn extrude(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "extrude");
    extrude_with(plane, regions, start, end, false)
}

/// [`extrude`] with each run of profile lines that continue one another in a straight line (a side
/// drawn in two pieces, a mirrored half meeting its mirror image) swept into one side face over
/// all its pieces, rather than coplanar faces with an edge between them (#1788). The side's
/// history lists every piece under it. For the Extrude feature; other builders (sheet metal's
/// flat pattern, tools) keep a face per segment.
pub fn extrude_whole_sides(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "extrude_whole_sides");
    extrude_with(plane, regions, start, end, true)
}

fn extrude_with(plane: &Plane, regions: &[Region2], start: f64, end: f64, whole_sides: bool) -> Result<Vec<Body>> {
    if !frame_ok(plane) {
        return Err(KernelError::Invalid("extrude frame is not finite and orthonormal".into()));
    }
    let (start, end) = (finite(start, "start")?, finite(end, "end")?);
    let (lo, hi) = if end >= start { (start, end) } else { (end, start) };
    if hi - lo < 1e-6 {
        return Err(KernelError::Invalid("extrude distance is zero".into()));
    }
    if regions.is_empty() {
        return Err(KernelError::Invalid("no profile selected".into()));
    }
    regions
        .iter()
        .map(|r| {
            region_ok(r)?;
            let pieces = oriented(r)?;
            let numbered = if whole_sides { pieces.map_loops(|l| l.merged_lines(continues_straight)) } else { pieces.clone() };
            crate::smlib_sweeps::extrude(plane, &crate::smlib_sweeps::region_pieces(&numbered, &pieces), Some(r), lo, hi - lo, 1.0, None)
        })
        .collect()
}

/// `t` carries on from `s` in the same straight line: the turn at their meeting point is within
/// rounding of none, measured against both lengths.
pub(crate) fn continues_straight(s: &Seg2, t: &Seg2) -> bool {
    let (u, v) = (s.end() - s.start(), t.end() - t.start());
    let (lu, lv) = (u.len(), v.len());
    lu > 0.0 && lv > 0.0 && s.end().dist(t.start()) <= 1e-9 * (lu + lv) && u.dot(v) > 0.0 && u.cross(v).abs() <= 1e-9 * lu * lv
}

/// Choose the per-profile axis sign whose positive rotation follows the sketch normal.
pub fn canonical_revolve_axis(plane: &Plane, region: &Region2, axis_origin: Vec2, axis_dir: Vec2) -> Result<Vec3> {
    if !axis_origin.is_finite() || !axis_dir.is_finite() || !plane.origin.is_finite() || !plane.x.is_finite() || !plane.y.is_finite() {
        return Err(KernelError::Invalid("non-finite revolve frame".into()));
    }
    if (plane.x.len2() - 1.).abs() > 1e-8 || (plane.y.len2() - 1.).abs() > 1e-8 || plane.x.dot(plane.y).abs() > 1e-8 {
        return Err(KernelError::Invalid("revolve frame is not orthonormal".into()));
    }
    region_ok(region)?;
    let direction = axis_dir.normalized().ok_or_else(|| KernelError::Invalid("revolve axis has no direction".into()))?;
    let axis = plane.dir_to_world(direction).normalized().ok_or_else(|| KernelError::Invalid("revolve frame has no axis".into()))?;
    let origin = plane.to_world(axis_origin);
    let center = plane.to_world(region.centroid());
    let radial = center - origin - axis * (center - origin).dot(axis);
    Ok(if axis.cross(radial).dot(plane.normal()) < 0. { -axis } else { axis })
}

/// Revolve each region about the sketch axis by `angle` radians.
/// This legacy API preserves its historical sweep sign behavior.
pub fn revolve(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "revolve");
    revolved(plane, regions, axis_origin, axis_dir, angle)
}

/// [`revolve`] for bodies that are final (new bodies, not a boolean's tools), with each planar
/// face whole (#726): SMLib's revolution makes one face of each profile segment and of each
/// cap, so this is [`revolve`].
pub fn revolve_whole(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "revolve_whole");
    revolved(plane, regions, axis_origin, axis_dir, angle)
}

/// Each region revolved through SMLib ([`crate::smlib_sweeps::revolve`]).
fn revolved(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
    let angle = finite(angle, "angle")?;
    if angle.abs() < 1e-6 {
        return Err(KernelError::Invalid("revolve angle is zero".into()));
    }
    let d = axis_dir.normalized().ok_or_else(|| KernelError::Invalid("revolve axis has no direction".into()))?;
    if regions.is_empty() {
        return Err(KernelError::Invalid("no profile selected".into()));
    }
    // The profile must lie on one side of the axis.
    let o = plane.to_world(axis_origin);
    let axis = plane.dir_to_world(d);
    let mut out = Vec::new();
    for r in regions {
        region_ok(r)?;
        let (mut neg, mut pos) = (false, false);
        for p in r.outer.polyline(1e-3) {
            let s = d.cross(p - axis_origin);
            neg |= s < -1e-7;
            pos |= s > 1e-7;
        }
        if neg && pos {
            return Err(KernelError::Invalid("the profile crosses the revolve axis".into()));
        }
        // Points on the axis (within solver tolerance) go exactly onto it, so the sweep does not
        // create hair-thin faces.
        let snap = |p: Vec2| {
            let along = (p - axis_origin).dot(d);
            let q = axis_origin + d * along;
            if p.dist(q) < 1e-7 { q } else { p }
        };
        let snap_loop = |l: &NumberedLoop| {
            l.map_segs(|s| match *s {
                Seg2::Line { a, b } => Seg2::Line { a: snap(a), b: snap(b) },
                arc => arc,
            })
        };
        // Adjacent profiles revolved together leave the axis edge split where they met: one
        // edge again, so the builders for a profile with an edge on the axis take it (swept
        // pieces of the axis are zero-area faces that booleans fail on).
        let on_axis = |p: Vec2| d.cross(p - axis_origin).abs() < 1e-9;
        let axis_run = |s: &Seg2, t: &Seg2| {
            let (a, b, c) = (s.start(), s.end(), t.end());
            on_axis(a) && on_axis(b) && on_axis(c) && (b - a).dot(c - b) > 0.0
        };
        // Partial turns go toward the side the sketch normal points to (as features extrude
        // toward it).
        let given = numbered_region(r)?.map_loops(|l| snap_loop(l).merged_lines(axis_run));
        let c = plane.to_world(r.centroid());
        let radial = (c - o) - axis * (c - o).dot(axis);
        let angle = if axis.cross(radial).dot(plane.normal()) < 0.0 { -angle } else { angle };
        out.push(crate::smlib_sweeps::revolve(plane, &crate::smlib_sweeps::pieces(given.oriented().loops()), o, axis, angle)?);
    }
    Ok(out)
}

/// Axis-aligned box between two corners.
pub fn box_solid(a: Vec3, b: Vec3) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "box_solid");
    let (lo, hi) = (a.min(b), a.max(b));
    let s = hi - lo;
    if !(s.x > 1e-6 && s.y > 1e-6 && s.z > 1e-6 && s.is_finite() && s.len() < 1e7) {
        return Err(KernelError::Invalid("box needs a positive size in x, y and z".into()));
    }
    let r = Region2 {
        outer: Loop2::polygon(&[Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y), Vec2::new(lo.x, hi.y)]),
        holes: vec![],
    };
    let mut v = extrude(&Plane::XY, &[r], lo.z, hi.z)?;
    let body = v.pop().ok_or_else(|| KernelError::Failed("box".into()))?;
    let sources = body.face_sources().ok_or_else(|| KernelError::Failed("box construction source".into()))?;
    let mut roles = std::collections::HashSet::new();
    let mut certified = Vec::new();
    for source in sources {
        let role = match source {
            Some(crate::FaceSource::ExtrudeCap { high, .. }) => Some((2u8, *high)),
            Some(crate::FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.x == b.x && a.x == lo.x => Some((0, false)),
            Some(crate::FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.x == b.x && a.x == hi.x => Some((0, true)),
            Some(crate::FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.y == b.y && a.y == lo.y => Some((1, false)),
            Some(crate::FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.y == b.y && a.y == hi.y => Some((1, true)),
            _ => None,
        }
        .ok_or_else(|| KernelError::Failed("box role correspondence".into()))?;
        if !roles.insert(role) {
            return Err(KernelError::Failed("duplicate box role".into()));
        }
        let (axis, high) = role;
        let sign = if high { 1. } else { -1. };
        let plane = match axis {
            0 => Plane { origin: Vec3::X * if high { hi.x } else { lo.x }, x: Vec3::Y, y: Vec3::Z * sign },
            1 => Plane { origin: Vec3::Y * if high { hi.y } else { lo.y }, x: Vec3::X, y: -Vec3::Z * sign },
            _ => Plane { origin: Vec3::Z * if high { hi.z } else { lo.z }, x: Vec3::X, y: Vec3::Y * sign },
        };
        certified.push(Some(crate::FaceSource::Box { producer: None, axis, high, plane }));
    }
    if roles.len() != 6 {
        return Err(KernelError::Failed("box role count".into()));
    }
    Ok(body.with_face_sources(Some(std::sync::Arc::new(certified))))
}

/// Where primitives start their circles (radians from the frame's x axis).
pub(crate) const SEAM_ANGLE: f64 = 0.6131;

/// Cylinder with base centre `base`, axis direction `axis`, radius and height.
pub fn cylinder(base: Vec3, axis: Vec3, radius: f64, height: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "cylinder");
    let plane = Plane::from_normal(base, axis).ok_or_else(|| KernelError::Invalid("cylinder axis".into()))?;
    let radius = finite(radius, "radius")?;
    if radius <= 1e-6 {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    // The seams at an odd angle: on the frame's axes they would lie exactly where faces placed
    // on a grid cut the cylinder (along a seam edge, which booleans don't survive).
    let r = Region2 { outer: Loop2::circle_from(Vec2::ZERO, radius, SEAM_ANGLE), holes: vec![] };
    let mut v = extrude(&plane, &[r], 0.0, finite(height, "height")?)?;
    v.pop().ok_or_else(|| KernelError::Failed("cylinder".into()))
}

/// Sphere: SMLib's own (smlib_make_sphere), each face swept from the arc (segment 0) of the half
/// disc a sphere revolves.
pub fn sphere(center: Vec3, radius: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sphere");
    let radius = finite(radius, "radius")?;
    if radius <= 1e-6 || !center.is_finite() || center.len() >= 1e7 {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    crate::smlib_sweeps::sphere(center, radius)
}

/// Torus about the Z axis through `center`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "torus");
    let (major, minor) = (finite(major, "major radius")?, finite(minor, "minor radius")?);
    if !(minor > 1e-6 && major > minor) {
        return Err(KernelError::Invalid("torus needs major > minor > 0".into()));
    }
    let plane = Plane { origin: center, ..Plane::XZ };
    let r = Region2 { outer: Loop2::circle(Vec2::new(major, 0.0), minor), holes: vec![] };
    let mut v = revolve(&plane, &[r], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?;
    v.pop().ok_or_else(|| KernelError::Failed("torus".into()))
}

/// Offset every segment of a loop to its right by `d` (outward for a region's outer loop and
/// holes alike), each segment as one of the same kind (`None`: a line the offset swallows, whose
/// neighbours now meet at one point). Loops of arcs on one circle, and loops of lines, arcs and
/// free curves (spline and conic pieces, such as the outlines of text) where the arcs meet their
/// neighbours tangentially are supported.
pub(crate) fn offset_loop(lp: &Loop2, d: f64) -> Result<Vec<Option<Seg2>>> {
    if d == 0.0 {
        return Ok(lp.segs.iter().copied().map(Some).collect());
    }
    let all_arcs = lp.segs.iter().all(|s| matches!(s, Seg2::Arc { .. }));
    if all_arcs {
        let moved = lp
            .segs
            .iter()
            .map(|s| match *s {
                Seg2::Arc { center, radius, start, sweep } => {
                    let r = radius + sweep.signum() * d;
                    if r > 1e-6 {
                        Ok(Seg2::Arc { center, radius: r, start, sweep })
                    } else {
                        Err(KernelError::Invalid("the taper closes the profile".into()))
                    }
                }
                _ => Err(KernelError::Failed("taper: profile loop".into())),
            })
            .collect::<Result<Vec<_>>>()?;
        return arc_corners(lp, moved).map(|arcs| arcs.into_iter().map(Some).collect());
    }
    offset_segments(lp, d)
}

/// The moved arcs of an all-arc loop made to meet again: where two arcs of different circles meet
/// at a corner (a circle and the lobes on its sides), their moved circles no longer cross at the
/// arcs' old end angles, so the corner goes to where the moved circles cross, nearest the old
/// corner, and both arcs are cut there. Arcs meeting tangentially (or on one circle) keep their
/// angles: moved along their shared normal, they still meet.
fn arc_corners(lp: &Loop2, moved: Vec<Seg2>) -> Result<Vec<Seg2>> {
    let bad = || KernelError::Failed("taper: profile loop".into());
    let closes = || KernelError::Invalid("the taper closes the profile".into());
    let n = moved.len();
    // Each arc's start and end angle once moved.
    let mut ang: Vec<(f64, f64)> = moved
        .iter()
        .map(|s| match *s {
            Seg2::Arc { start, sweep, .. } => Ok((start, start + sweep)),
            _ => Err(bad()),
        })
        .collect::<Result<_>>()?;
    for i in 0..n {
        let k = (i + n - 1) % n;
        let (Some(prev), Some(this), Some(Seg2::Arc { center: c1, radius: r1, .. }), Some(Seg2::Arc { center: c2, radius: r2, .. })) =
            (lp.segs.get(k), lp.segs.get(i), moved.get(k).copied(), moved.get(i).copied())
        else {
            return Err(bad());
        };
        let (tin, tout) = (prev.end_tangent(), this.start_tangent());
        if tin.cross(tout).abs() < 1e-6 && tin.dot(tout) > 0.0 {
            continue;
        }
        let x = circles_cross(c1, r1, c2, r2, this.start()).ok_or_else(|| {
            KernelError::Failed("not supported yet: a taper this steep for arcs meeting at a corner (their moved circles part)".into())
        })?;
        if let Some(a) = ang.get_mut(k) {
            a.1 = (x - c1).angle();
        }
        if let Some(a) = ang.get_mut(i) {
            a.0 = (x - c2).angle();
        }
    }
    moved
        .iter()
        .zip(&ang)
        .map(|(s, &(a0, a1))| match *s {
            Seg2::Arc { center, radius, sweep, .. } => {
                // The same turning sense, the whole-turn equivalent nearest the old sweep.
                let tau = std::f64::consts::TAU;
                let mut sw = a1 - a0;
                sw += ((sweep - sw) / tau).round() * tau;
                if sw * sweep <= 0.0 || sw.abs() < 1e-9 {
                    return Err(closes());
                }
                Ok(Seg2::Arc { center, radius, start: a0, sweep: sw })
            }
            _ => Err(bad()),
        })
        .collect()
}

/// Where the circles (`c1`, `r1`) and (`c2`, `r2`) cross, the crossing nearest `near`; `None`
/// when they don't (apart, one inside the other, or concentric).
fn circles_cross(c1: Vec2, r1: f64, c2: Vec2, r2: f64, near: Vec2) -> Option<Vec2> {
    let d = c1.dist(c2);
    if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        return None;
    }
    let u = (c2 - c1) * (1.0 / d);
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let base = c1 + u * a;
    let (p, q) = (base + u.perp() * h, base - u.perp() * h);
    let x = if p.dist(near) <= q.dist(near) { p } else { q };
    x.is_finite().then_some(x)
}

/// Where the line through `p` along `tin` and the one through `q` along `tout` meet once each
/// has moved `d` to its right (`q` moved, where they run on together).
fn mitre(p: Vec2, tin: Vec2, q: Vec2, tout: Vec2, d: f64) -> Vec2 {
    let right = |t: Vec2| Vec2::new(t.y, -t.x);
    let (p0, p1) = (p + right(tin) * d, q + right(tout) * d);
    let den = tin.cross(tout);
    if den.abs() < 1e-12 { p1 } else { p0 + tin * ((p1 - p0).cross(tout) / den) }
}

/// A free curve's control polygon moved `d` to its right, from `a` to `b` (where its moved
/// neighbours meet it): each leg moves `d` to its right and each inner control point goes where
/// its two moved legs meet. For a curve that turns little against `d` this stays close to the
/// true offset, and its ends run along the curve's end tangents, so its side meets its
/// neighbours' along straight rulings.
fn offset_polygon(ctrl: &[Vec2], a: Vec2, b: Vec2, d: f64) -> Option<Vec<Vec2>> {
    let legs: Vec<Option<Vec2>> = ctrl.iter().zip(ctrl.iter().skip(1)).map(|(p, q)| (*q - *p).normalized()).collect();
    // A leg of no length takes the direction of the nearest leg after it, else before it.
    let dirs: Vec<Vec2> = (0..legs.len())
        .map(|i| legs.iter().skip(i).flatten().next().or_else(|| legs.iter().take(i).rev().flatten().next()).copied())
        .collect::<Option<_>>()?;
    let m = ctrl.len();
    let mut out = Vec::with_capacity(m);
    for (i, p) in ctrl.iter().enumerate() {
        out.push(if i == 0 {
            a
        } else if i + 1 == m {
            b
        } else {
            mitre(*p, *dirs.get(i - 1)?, *p, *dirs.get(i)?, d)
        });
    }
    out.iter().all(|p| p.is_finite()).then_some(out)
}

/// Samples a moved free curve is fitted to, and checked against.
const OFFSET_SAMPLES: usize = 16;

/// Where the point at `t` of the free curve `s` goes once moved `d` to its right (along its
/// normal there). `None` where it has no tangent.
fn true_offset(s: &Seg2, t: f64, d: f64) -> Option<Vec2> {
    let tangent = s.derivative(t).normalized()?;
    Some(s.point_at(t) + Vec2::new(tangent.y, -tangent.x) * d)
}

/// The free curve `s` moved `d` to its right, as one curve of its kind from `a` to `b` (where
/// its moved neighbours meet it; its own moved ends where it runs on into them). It is fitted to
/// the true offset between the curve's own moved ends: a cubic keeps its end tangents and takes
/// the handle lengths that fit best (least squares over [`OFFSET_SAMPLES`] points); a conic or a
/// higher Bézier moves its control polygon ([`offset_polygon`]), its middle then put on the true
/// offset. Its ends then go to `a` and `b`, along its end tangents (where moved neighbours meet
/// it), their handles kept pointing the same way. Close for a curve that turns little against
/// `d`.
fn offset_curve(s: &Seg2, a: Vec2, b: Vec2, d: f64) -> Option<Seg2> {
    let (n0, n1) = (true_offset(s, 0.0, d)?, true_offset(s, 1.0, d)?);
    let (t0, t3) = (s.start_tangent(), s.end_tangent());
    let middle = |moved: &Seg2| Some(true_offset(s, 0.5, d)? - moved.point_at(0.5));
    // The control points fitted between the natural ends.
    let mut ctrl: Vec<Vec2> = match *s {
        Seg2::Cubic { p0, p1, p2, p3 } => {
            // The moved cubic n0·(B0+B1) + n1·(B2+B3) + α·t0·B1 − β·t3·B2: linear in α, β.
            let (mut m, mut r) = ([[0.0f64; 2]; 2], [0.0f64; 2]);
            for k in 1..OFFSET_SAMPLES {
                let t = k as f64 / OFFSET_SAMPLES as f64;
                let u = 1.0 - t;
                let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                let rest = true_offset(s, t, d)? - (n0 * (b0 + b1) + n1 * (b2 + b3));
                let (c1, c2) = (t0 * b1, t3 * -b2);
                m[0][0] += c1.dot(c1);
                m[0][1] += c1.dot(c2);
                m[1][1] += c2.dot(c2);
                r[0] += c1.dot(rest);
                r[1] += c2.dot(rest);
            }
            let det = m[0][0] * m[1][1] - m[0][1] * m[0][1];
            let (alpha, beta) = if det.abs() > 1e-12 * (m[0][0] * m[1][1]).max(1e-30) {
                ((r[0] * m[1][1] - r[1] * m[0][1]) / det, (m[0][0] * r[1] - m[0][1] * r[0]) / det)
            } else {
                (f64::NAN, f64::NAN)
            };
            if alpha.is_finite() && beta.is_finite() && alpha > 0.0 && beta > 0.0 {
                vec![n0, n0 + t0 * alpha, n1 - t3 * beta, n1]
            } else {
                // Handles that would turn back: the moved control polygon, its middle on the offset.
                let mut q = offset_polygon(&[p0, p1, p2, p3], n0, n1, d)?;
                let [q0, q1, q2, q3] = q[..] else { return None };
                let shift = middle(&Seg2::Cubic { p0: q0, p1: q1, p2: q2, p3: q3 })? * (8.0 / 6.0);
                for point in q.iter_mut().take(3).skip(1) {
                    *point += shift;
                }
                q
            }
        }
        Seg2::Conic { a: c0, apex, b: c1, w } => {
            let mut q = offset_polygon(&[c0, apex, c1], n0, n1, d)?;
            let [q0, qa, q1] = q[..] else { return None };
            // The middle is (a + 2w·apex + b) / (2 + 2w).
            let shift = middle(&Seg2::Conic { a: q0, apex: qa, b: q1, w })? * ((2.0 + 2.0 * w) / (2.0 * w));
            if let Some(apex) = q.get_mut(1) {
                *apex += shift;
            }
            q
        }
        Seg2::Bezier { n, p } => {
            let k = (n as usize).clamp(1, solidcraft_geom::MAX_BEZIER_DEGREE);
            let mut q = offset_polygon(p.get(..=k)?, n0, n1, d)?;
            let mut all = p;
            for (dst, src) in all.iter_mut().zip(&q) {
                *dst = *src;
            }
            // The middle weighs the inner points by binomials summing to 2^k − 2.
            let shift = middle(&Seg2::Bezier { n, p: all })? * (2f64.powi(k as i32) / (2f64.powi(k as i32) - 2.0));
            for point in q.iter_mut().take(k).skip(1) {
                *point += shift;
            }
            q
        }
        Seg2::Line { .. } | Seg2::Arc { .. } => return None,
    };
    // The ends to where the neighbours meet; a handle the move would turn round goes with it.
    let last = ctrl.len().checked_sub(1)?;
    if last < 2 {
        return None;
    }
    let (ea, eb) = (a - n0, b - n1);
    if let Some(h) = ctrl.get_mut(1)
        && (*h - a).dot(t0) <= 0.0
    {
        *h += ea;
    }
    if let Some(h) = ctrl.get_mut(last - 1)
        && (b - *h).dot(t3) <= 0.0
    {
        *h += eb;
    }
    if let Some(first) = ctrl.first_mut() {
        *first = a;
    }
    if let Some(end) = ctrl.last_mut() {
        *end = b;
    }
    Some(match (*s, &ctrl[..]) {
        (Seg2::Cubic { .. }, &[p0, p1, p2, p3]) => Seg2::Cubic { p0, p1, p2, p3 },
        (Seg2::Conic { w, .. }, &[a, apex, b]) => Seg2::Conic { a, apex, b, w },
        (Seg2::Bezier { n, p }, q) => {
            let mut all = p;
            for (dst, src) in all.iter_mut().zip(q) {
                *dst = *src;
            }
            Seg2::Bezier { n, p: all }
        }
        _ => return None,
    })
}

/// [`offset_loop`] for a loop of lines, arcs and free curves. Each arc keeps its centre and
/// angles and its radius moves by `d` (so its side stays an exact cone), each line moves `d` to
/// its right, and each free curve moves its control polygon ([`offset_polygon`]). Where two
/// moved segments part (a corner the offset goes round) they are carried on along their end
/// tangents to where they meet; where they run into each other (a corner the offset goes into)
/// both are cut where they cross. A line shorter than the offset can turn around between its
/// moved neighbours (the flat tip of a letter's stroke): it goes, and its neighbours meet. An arc
/// has to meet its neighbours tangentially: at a corner its side would meet the next one along a
/// conic, not a straight ruling.
fn offset_segments(lp: &Loop2, d: f64) -> Result<Vec<Option<Seg2>>> {
    let closes = || KernelError::Invalid("the taper closes the profile".into());
    let at_corner = || KernelError::Failed("not supported yet: tapered profiles where an arc meets another edge at a corner".into());
    let bad = || KernelError::Failed("taper: profile loop".into());
    let right = |t: Vec2| Vec2::new(t.y, -t.x);
    let n = lp.segs.len();
    let moved: Vec<Option<Seg2>> = lp
        .segs
        .iter()
        .map(|s| match *s {
            Seg2::Arc { center, radius, start, sweep } => {
                let r = radius + sweep.signum() * d;
                if r > 1e-6 { Ok(Some(Seg2::Arc { center, radius: r, start, sweep })) } else { Err(closes()) }
            }
            _ => Ok(None),
        })
        .collect::<Result<_>>()?;
    let mut gone = vec![false; n];
    // Each round takes away at least one line, or ends.
    let pieces = loop {
        let kept: Vec<usize> = (0..n).filter(|&i| !gone.get(i).copied().unwrap_or(true)).collect();
        if kept.len() < 2 {
            return Err(closes());
        }
        // Where each kept segment starts and ends once moved, and the corners where two moved
        // segments run into each other (the one before, the one after).
        let (mut starts, mut ends) = (vec![Vec2::ZERO; n], vec![Vec2::ZERO; n]);
        let mut into: Vec<(usize, usize)> = Vec::new();
        for (j, &i) in kept.iter().enumerate() {
            let Some(&k) = kept.get((j + kept.len() - 1) % kept.len()) else { return Err(bad()) };
            let (Some(prev), Some(s), Some(prev_moved), Some(this_moved)) = (lp.segs.get(k), lp.segs.get(i), moved.get(k), moved.get(i)) else {
                return Err(bad());
            };
            let (tin, tout) = (prev.end_tangent(), s.start_tangent());
            let adjacent = (k + 1) % n == i;
            let turn = tin.cross(tout);
            let tangent = adjacent && turn.abs() < 1e-6 && tin.dot(tout) > 0.0;
            let (end, start) = match (prev_moved, this_moved) {
                (Some(a), _) if tangent => (a.end(), a.end()),
                (None, Some(b)) if tangent => (b.start(), b.start()),
                // Two sides running back towards each other across a gone tip: the stroke is
                // thinner than the taper.
                (None, None) if !adjacent && turn.abs() < 1e-9 => return Err(closes()),
                // Running on smoothly (the pieces of one curve): both move to the same point.
                // (The tangents' crossing is ill-conditioned there.)
                (None, None) if tangent => {
                    let m = s.start() + right(tout) * d;
                    (m, m)
                }
                (None, None) if turn * d < 0.0 => {
                    into.push((k, i));
                    (prev.end() + right(tin) * d, s.start() + right(tout) * d)
                }
                (None, None) => {
                    let m = mitre(prev.end(), tin, s.start(), tout, d);
                    (m, m)
                }
                _ => return Err(at_corner()),
            };
            if let (Some(e), Some(st)) = (ends.get_mut(k), starts.get_mut(i)) {
                (*e, *st) = (end, start);
            }
        }
        let mut pieces: Vec<Option<Seg2>> = vec![None; n];
        for &i in &kept {
            let (Some(s), Some(m), Some(&a), Some(&b)) = (lp.segs.get(i), moved.get(i), starts.get(i), ends.get(i)) else { return Err(bad()) };
            let piece = match (m, *s) {
                (Some(arc), _) => {
                    if arc.start().dist(a) > 1e-6 || arc.end().dist(b) > 1e-6 {
                        return Err(at_corner());
                    }
                    *arc
                }
                (None, Seg2::Line { .. }) => Seg2::Line { a, b },
                (None, Seg2::Arc { .. }) => return Err(bad()),
                (None, curve) => offset_curve(&curve, a, b, d).ok_or_else(closes)?,
            };
            if let Some(slot) = pieces.get_mut(i) {
                *slot = Some(piece);
            }
        }
        // Cut the two pieces at each corner the offset goes into where they cross; pieces that
        // don't cross meet where their end tangents do.
        for &(k, i) in &into {
            let (Some(Some(p)), Some(Some(q))) = (pieces.get(k).copied(), pieces.get(i).copied()) else { return Err(bad()) };
            let (p, q) = match meet(&p, &q) {
                Some((tp, tq, x)) => (with_end(&p.split_at(tp).0, x), with_start(&q.split_at(tq).1, x)),
                None => {
                    let x = mitre(p.end(), p.end_tangent(), q.start(), q.start_tangent(), 0.0);
                    (with_end(&p, x), with_start(&q, x))
                }
            };
            if let Some(a) = pieces.get_mut(k) {
                *a = Some(p);
            }
            if let Some(b) = pieces.get_mut(i) {
                *b = Some(q);
            }
        }
        let mut more = false;
        for &i in &kept {
            // A line that turned around has been swallowed by its neighbours.
            if let (Some(Seg2::Line { a: s0, b: s1 }), Some(Some(Seg2::Line { a, b }))) = (lp.segs.get(i), pieces.get(i))
                && (*b - *a).dot(*s1 - *s0) <= 0.0
                && let Some(g) = gone.get_mut(i)
            {
                *g = true;
                more = true;
            }
        }
        if !more {
            break pieces;
        }
    };
    let out = Loop2 { segs: pieces.iter().flatten().copied().collect() };
    if out.signed_area().signum() != lp.signed_area().signum() || out.signed_area().abs() < 1e-9 {
        return Err(closes());
    }
    Ok(pieces)
}

/// Where the end of `p` crosses the start of `q`, nearest the corner between them: the
/// parameters on each and the point. On polylines of a few dozen points each, the point taken
/// on a line where one is, else half way between the two crossing pieces.
///
/// Its limit: curves are sampled at [`MEET_SAMPLES`] points, so two that cross twice within one
/// sample span (a tight curve against its neighbour) can be missed. The corner then gets the
/// mitre of the end tangents instead.
fn meet(p: &Seg2, q: &Seg2) -> Option<(f64, f64, Vec2)> {
    const SAMPLES: usize = MEET_SAMPLES;
    let samples = |s: &Seg2| -> Vec<(f64, Vec2)> {
        let n = if matches!(s, Seg2::Line { .. }) { 1 } else { SAMPLES };
        (0..=n).map(|k| k as f64 / n as f64).map(|t| (t, s.point_at(t))).collect()
    };
    let (sp, sq) = (samples(p), samples(q));
    let mut best: Option<(f64, f64)> = None;
    for (a, b) in sp.iter().zip(sp.iter().skip(1)) {
        for (c, e) in sq.iter().zip(sq.iter().skip(1)) {
            let (r, s) = (b.1 - a.1, e.1 - c.1);
            let den = r.cross(s);
            if den.abs() < 1e-15 {
                continue;
            }
            let (u, v) = ((c.1 - a.1).cross(s) / den, (c.1 - a.1).cross(r) / den);
            if !((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)) {
                continue;
            }
            let (tp, tq) = (a.0 + (b.0 - a.0) * u, c.0 + (e.0 - c.0) * v);
            // Nearest the corner: little of `p` left behind its crossing, little of `q` before.
            if best.is_none_or(|(bp, bq)| (1.0 - tp) + tq < (1.0 - bp) + bq) {
                best = Some((tp, tq));
            }
        }
    }
    let (tp, tq) = best?;
    // Not at an end: a piece cut to nothing has no length left to rule.
    if tp <= 1e-9 || tq >= 1.0 - 1e-9 {
        return None;
    }
    // On a line where one meets it, so a line stays on its line (its side stays flat).
    let x = match (p, q) {
        (Seg2::Line { .. }, _) => p.point_at(tp),
        (_, Seg2::Line { .. }) => q.point_at(tq),
        _ => (p.point_at(tp) + q.point_at(tq)) * 0.5,
    };
    Some((tp, tq, x))
}

/// `s` ending at `x` (its last control point moved there).
fn with_end(s: &Seg2, x: Vec2) -> Seg2 {
    match *s {
        Seg2::Line { a, .. } => Seg2::Line { a, b: x },
        Seg2::Cubic { p0, p1, p2, .. } => Seg2::Cubic { p0, p1, p2, p3: x },
        Seg2::Conic { a, apex, w, .. } => Seg2::Conic { a, apex, b: x, w },
        Seg2::Bezier { n, p } => {
            let mut q = p;
            if let Some(last) = q.get_mut((n as usize).clamp(1, solidcraft_geom::MAX_BEZIER_DEGREE)) {
                *last = x;
            }
            Seg2::Bezier { n, p: q }
        }
        Seg2::Arc { .. } => *s,
    }
}

/// `s` starting at `x` (its first control point moved there).
fn with_start(s: &Seg2, x: Vec2) -> Seg2 {
    match *s {
        Seg2::Line { b, .. } => Seg2::Line { a: x, b },
        Seg2::Cubic { p1, p2, p3, .. } => Seg2::Cubic { p0: x, p1, p2, p3 },
        Seg2::Conic { apex, b, w, .. } => Seg2::Conic { a: x, apex, b, w },
        Seg2::Bezier { n, p } => {
            let mut q = p;
            if let Some(first) = q.first_mut() {
                *first = x;
            }
            Seg2::Bezier { n, p: q }
        }
        Seg2::Arc { .. } => *s,
    }
}

/// Points a free curve is sampled at by [`meet`].
const MEET_SAMPLES: usize = 48;

/// Extrude one region by `length` along `dir_sign` × the plane normal with a taper angle
/// (radians; positive grows the profile), its sharp corners closed by `corner`
/// ([`crate::draft_corners`] says what each rule gives at each corner).
pub fn extrude_tapered(plane: &Plane, region: &Region2, length: f64, dir_sign: f64, taper: f64, corner: DraftCorner) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "extrude_tapered");
    extrude_tapered_between(plane, region, 0.0, length, 0.0, dir_sign, taper, corner)
}

/// [`extrude_tapered`] from `from` to `to` along `dir_sign` × the plane normal, the region
/// being the section at 0: its sides run on through it, so `from` below 0 starts the solid
/// that far before the profile (into a face the profile lies on), at the profile shrunk or
/// grown by the taper. With a `foot` (a height, less than `to - from`), the sides stand
/// straight for that height above `from` and only then slope, up to the same top: a face the
/// solid stands on can then be pushed into the body it joins, as a prism's can.
#[allow(clippy::too_many_arguments)]
pub fn extrude_tapered_between(
    plane: &Plane,
    region: &Region2,
    from: f64,
    to: f64,
    foot: f64,
    dir_sign: f64,
    taper: f64,
    corner: DraftCorner,
) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "extrude_tapered_between");
    let (from, to, foot, taper) = (finite(from, "start")?, finite(to, "distance")?, finite(foot, "foot")?, finite(taper, "taper")?);
    if to - from <= 1e-6 {
        return Err(KernelError::Invalid("extrude distance is zero".into()));
    }
    if !(0.0..(to - from) * 0.5).contains(&foot) {
        return Err(KernelError::Invalid(format!("taper foot = {foot}")));
    }
    if taper.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("taper must be between −90° and 90°".into()));
    }
    region_ok(region)?;
    smlib_tapered(plane, region, from, to, foot, dir_sign.signum(), taper, corner)
}

/// [`extrude_tapered_between`] through SMLib's draft extrusion, its section at `from` the
/// profile offset by the taper, after any straight foot. SMLib's draft extrusion takes one loop:
/// a region with holes is its outer loop drafted, less each hole drafted the other way (every
/// wall leaning by the taper on the material's side), refused only where the taper would make a
/// hole meet the outer wall or another hole within the extrusion.
#[allow(clippy::too_many_arguments)]
fn smlib_tapered(plane: &Plane, region: &Region2, from: f64, to: f64, foot: f64, sign: f64, taper: f64, corner: DraftCorner) -> Result<Body> {
    let joined = oriented(region)?.map_loops(|l| l.merged_lines(continues_straight));
    // A loop's section `at` along the extrusion: counter-clockwise, grown by `taper` (a hole's
    // the other way, as the wall of the material round it leans).
    let section = |l: &NumberedLoop, at: f64, taper: f64| -> Result<NumberedLoop> {
        let ccw = l.oriented(false);
        if at == 0.0 || taper == 0.0 {
            return Ok(ccw);
        }
        let segs: Option<Vec<Seg2>> = offset_loop(&ccw.lp, at * taper.tan())?.into_iter().collect();
        let segs = segs.ok_or_else(|| crate::smlib_sweeps::refuse("draft extrusion that starts before its profile where the taper closes a side"))?;
        Ok(NumberedLoop { lp: Loop2 { segs }, from: ccw.from })
    };
    let outer = section(&joined.outer, from, taper)?;
    let draft = (taper != 0.0).then_some(crate::smlib_sweeps::Draft { taper, foot, corner });
    if joined.holes.is_empty() {
        return crate::smlib_sweeps::extrude(plane, &crate::smlib_sweeps::pieces([&outer]), Some(region), from * sign, to - from, sign, draft);
    }
    let Some(draft) = draft else {
        return Err(KernelError::Invalid("a tapered extrusion needs a taper".into()));
    };
    let holes: Vec<NumberedLoop> = joined.holes.iter().map(|h| section(h, from, -taper)).collect::<Result<_>>()?;
    // The sections where the walls are farthest moved, the two ends: each hole stays inside the
    // outer loop and clear of the other holes there (the walls are planes, cones and their like,
    // which move monotonically in between).
    for at in [from, to] {
        let o = section(&joined.outer, at, taper)?;
        let hs: Vec<NumberedLoop> = joined.holes.iter().map(|h| section(h, at, -taper)).collect::<Result<_>>()?;
        let pts = |l: &NumberedLoop| l.lp.segs.iter().flat_map(|s| s.polyline(1e-3)).collect::<Vec<Vec2>>();
        // Apart: no point of either inside the other.
        let apart = |a: &NumberedLoop, b: &NumberedLoop| !pts(a).iter().any(|p| b.lp.contains(*p)) && !pts(b).iter().any(|p| a.lp.contains(*p));
        for (i, h) in hs.iter().enumerate() {
            let inside = pts(h).iter().all(|p| o.lp.contains(*p)) && !pts(&o).iter().any(|p| h.lp.contains(*p));
            if !inside || hs.iter().skip(i + 1).any(|k| !apart(h, k)) {
                return Err(KernelError::Invalid("the draft makes a hole meet the outer wall or another hole".into()));
            }
        }
    }
    let all: Vec<NumberedLoop> = std::iter::once(outer.clone()).chain(holes.iter().map(|h| h.oriented(true))).collect();
    crate::smlib_sweeps::extrude_drafted_with_holes(
        plane,
        &crate::smlib_sweeps::pieces([&outer]),
        &holes.iter().map(|h| crate::smlib_sweeps::pieces([h])).collect::<Vec<_>>(),
        &crate::smlib_sweeps::pieces(all.iter()),
        from * sign,
        to - from,
        sign,
        draft,
    )
}

/// A planar region on `plane` as the schema 4 record of one face (an open sheet): the plane's
/// own axes as its parameters (its natural normal the sketch's), the outer loop
/// counter-clockwise and the holes clockwise, each sketch segment one edge of its exact record
/// curve ([`crate::point_path::sketch_record_curve`]) with its exact pcurve on the plane. The record
/// equivalent of a profile face: SMLib takes it with `Brep::from_record`.
pub fn planar_face_record(plane: &Plane, r: &Region2) -> Result<solidcraft_geom::brep::Brep> {
    use solidcraft_geom::brep::{self as rec, pcurve};
    if !frame_ok(plane) {
        return Err(KernelError::Invalid("the face's plane is not finite and orthonormal".into()));
    }
    region_ok(r)?;
    let region = oriented(r)?;
    let tolerance = 1e-7;
    let d = |v: Vec3| [v.x, v.y, v.z];
    let surface = rec::Surface::Plane { origin: d(plane.origin), u_axis: d(plane.x), v_axis: d(plane.y) };
    let bad = |what: &str| KernelError::Invalid(format!("profile {what}"));
    // Edges and vertices numbered as the face's loops first meet them.
    let mut edges: Vec<rec::Edge> = Vec::new();
    let mut vertices: Vec<rec::Vertex> = Vec::new();
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for (li, l) in region.loops().enumerate() {
        let segs = &l.lp.segs;
        let first = u32::try_from(vertices.len()).map_err(|_| bad("size"))?;
        let mut ids = Vec::with_capacity(segs.len());
        for (k, s) in segs.iter().enumerate() {
            let (curve, interval) = crate::point_path::sketch_record_curve(plane, s).ok_or_else(|| bad("curve"))?;
            let start = u32::try_from(vertices.len()).map_err(|_| bad("size"))?;
            vertices.push(rec::Vertex { point: d(plane.to_world(s.start())), tolerance });
            let end = if k + 1 == segs.len() { first } else { start + 1 };
            let e = u32::try_from(edges.len()).map_err(|_| bad("size"))?;
            let (li, k) = (u32::try_from(li).map_err(|_| bad("size"))?, u32::try_from(k).map_err(|_| bad("size"))?);
            edges.push(rec::Edge {
                nurbs: None,
                curve,
                interval,
                vertices: [start, end],
                tolerance,
                uses: vec![rec::EdgeUseRef { face: 0, r#loop: li, position: k, upper: false }],
            });
            ids.push(e);
            for q in s.polyline(tolerance) {
                for (i, x) in [q.x, q.y].into_iter().enumerate() {
                    if let (Some(a), Some(b)) = (lo.get_mut(i), hi.get_mut(i)) {
                        *a = a.min(x);
                        *b = b.max(x);
                    }
                }
            }
        }
        loops.push(ids);
    }
    let uses: Vec<Vec<pcurve::EdgeUse<'_>>> = loops
        .iter()
        .map(|ids| {
            ids.iter()
                .filter_map(|&e| {
                    let edge = edges.get(e as usize)?;
                    Some(pcurve::EdgeUse { edge: e, curve: edge.parameters(), interval: edge.interval, forward: true, tolerance, given: None })
                })
                .collect()
        })
        .collect();
    let pcurves = pcurve::pcurves(&surface, &uses, tolerance).map_err(|e| KernelError::Failed(format!("profile pcurves: {e}")))?;
    let face_loops = loops
        .iter()
        .zip(pcurves)
        .map(|(ids, pcs)| rec::Loop::Edges {
            edges: ids.iter().zip(pcs).map(|(&edge, pcurve)| rec::OrientedEdge { edge, forward: true, pcurve }).collect(),
        })
        .collect();
    let pad = tolerance * 10.0;
    let brep = rec::Brep {
        version: rec::VERSION,
        tolerance,
        regions: vec![rec::Region { shells: vec![0], void: false }],
        shells: vec![rec::Shell::Faces { faces: vec![rec::OrientedFace { face: 0, forward: true }], wires: vec![] }],
        faces: vec![rec::Face { nurbs: None, surface, domain: [[lo[0] - pad, hi[0] + pad], [lo[1] - pad, hi[1] + pad]], loops: face_loops }],
        edges,
        vertices,
    };
    brep.validate().map_err(|e| KernelError::Failed(format!("profile face: {e}")))?;
    Ok(brep)
}

/// Ruled loft through two or more planar sections (outer loops only). The sections and the
/// same sections in reverse order give the same body.
pub fn loft(sections: &[(Plane, Loop2)]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "loft");
    loft_with_rails(sections, &[], RailFit::Scale)
}

/// [`loft`] following one or two guide rails (polylines from the first section to the last,
/// through every section's curve; see [`crate::loft_rails`]), `fit` saying how they reshape the
/// sections between. Without rails, the plain loft.
pub fn loft_with_rails(sections: &[(Plane, Loop2)], rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "loft_with_rails");
    let reversed = loft_reversed(sections)?;
    let numbered = sections.iter().map(|(p, _)| *p).zip(numbered::number_loops(sections.iter().map(|(_, l)| l))).collect::<Vec<_>>();
    let loops = loft_numbered_loops(&numbered, reversed)?;
    let first = if reversed { loops.len().saturating_sub(1) } else { 0 };
    let pieces: Vec<(Plane, Vec<crate::smlib_sweeps::Piece>)> = loops.iter().map(|(p, l)| (*p, crate::smlib_sweeps::pieces([l]))).collect();
    if !rails.is_empty() {
        return crate::smlib_sweeps::loft_rails(&pieces, first, rails, fit);
    }
    crate::smlib_sweeps::loft(&pieces, first)
}

/// A loft through regions with matching hole counts. Hole walls are built alongside the
/// outline and share the end caps, so the result is one shell with open passages.
/// Holes are paired by their positions relative to the outline, independently of sketch
/// curve order. A hole cannot appear or disappear partway through a loft.
pub fn loft_regions(sections: &[(Plane, Region2)]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "loft_regions");
    if !(2..=50).contains(&sections.len()) {
        return Err(KernelError::Invalid("a loft needs 2…50 sections".into()));
    }
    let holes = sections.first().map_or(0, |(_, r)| r.holes.len());
    if sections.iter().any(|(_, r)| r.holes.len() != holes) {
        return Err(KernelError::Invalid("every loft section must have the same number of holes".into()));
    }
    // Bound the quadratic matching work as well as the geometry built per section.
    if holes > 128 || sections.iter().any(|(_, r)| numbered::profile_loops(std::slice::from_ref(r)).map(|l| l.segs.len()).sum::<usize>() > MAX_SEGS) {
        return Err(KernelError::Invalid("too many loops or segments in a loft section".into()));
    }
    for (_, r) in sections {
        region_ok(r)?;
    }
    if holes == 0 {
        return loft(&sections.iter().map(|(p, r)| (*p, r.outer.clone())).collect::<Vec<_>>());
    }
    let regions: Vec<Region2> = sections.iter().map(|(_, r)| r.clone()).collect();
    let mut numbered = numbered::number_regions(&regions);
    let outlines: Vec<_> = sections.iter().map(|(p, _)| *p).zip(numbered.iter().map(|r| r.outer.clone())).collect();
    let reversed = loft_reversed(&sections.iter().map(|(p, r)| (*p, r.outer.clone())).collect::<Vec<_>>())?;
    // Match in canonical section order, so reversing the input cannot change the pairing.
    if reversed {
        numbered.reverse();
    }
    let planes: Vec<_> = if reversed { sections.iter().rev().map(|(p, _)| *p).collect() } else { sections.iter().map(|(p, _)| *p).collect() };
    let mut previous: Vec<Vec3> = Vec::new();
    for (p, r) in planes.iter().zip(&mut numbered) {
        let centre = r.outer.lp.centroid();
        let scale = r.outer.lp.signed_area().abs().sqrt().max(1e-9);
        let position = |l: &NumberedLoop| {
            let d = l.lp.centroid() - centre;
            (p.x * d.x + p.y * d.y) * (1.0 / scale)
        };
        if !previous.is_empty() {
            let positions: Vec<_> = r.holes.iter().map(position).collect();
            let mut used = vec![false; positions.len()];
            let mut order = Vec::new();
            for point in &previous {
                let (k, distance) = positions
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k, p.dist(*point)))
                    .min_by(|(_, a), (_, b)| a.total_cmp(b))
                    .ok_or_else(|| KernelError::Invalid("missing loft hole".into()))?;
                let occupied = used.get_mut(k).ok_or_else(|| KernelError::Invalid("missing loft hole".into()))?;
                if *occupied
                    || !distance.is_finite()
                    || positions.iter().enumerate().any(|(j, p)| j != k && (p.dist(*point) - distance).abs() <= 1e-9)
                {
                    return Err(KernelError::Invalid("loft hole correspondence is ambiguous; each hole needs a unique nearest match".into()));
                }
                *occupied = true;
                order.push(k);
            }
            let mut remaining: Vec<_> = std::mem::take(&mut r.holes).into_iter().map(Some).collect();
            for k in order {
                r.holes.push(remaining.get_mut(k).and_then(Option::take).ok_or_else(|| KernelError::Invalid("missing loft hole".into()))?);
            }
        }
        previous = r.holes.iter().map(position).collect();
    }
    // Matching resamples each loop family to its largest section. Different holes can
    // be the large one in different sketches: bound the combined result, not just each
    // input section, before allocating the wires.
    let mut counts = vec![0usize; holes + 1];
    for r in &numbered {
        for (count, lp) in counts.iter_mut().zip(r.loops()) {
            *count = (*count).max(lp.lp.segs.len());
        }
    }
    if counts.iter().sum::<usize>() > MAX_SEGS {
        return Err(KernelError::Invalid("matching loft sections would create too many segments".into()));
    }
    smlib_loft_regions(&outlines, &planes, &numbered, reversed)
}

/// [`loft_regions`] through SMLib's skin, which lofts a section's loops (its outline, then its
/// holes, in the order given) to the loops in the same place in the next: the outline and each
/// hole matched as above, every section's loops one profile, the holes turned against the
/// outline. `outlines` are as given, `planes` and the holes of `numbered` in canonical order.
fn smlib_loft_regions(outlines: &[(Plane, NumberedLoop)], planes: &[Plane], numbered: &[NumberedRegion], reversed: bool) -> Result<Body> {
    let mut sections: Vec<(Plane, Vec<crate::smlib_sweeps::Piece>)> =
        loft_numbered_loops(outlines, reversed)?.iter().map(|(p, l)| (*p, crate::smlib_sweeps::pieces([l]))).collect();
    let holes = numbered.first().map_or(0, |r| r.holes.len());
    for i in 0..holes {
        let secs = planes
            .iter()
            .zip(numbered)
            .map(|(p, r)| r.holes.get(i).cloned().map(|h| (*p, h)).ok_or_else(|| KernelError::Invalid("missing loft hole".into())))
            .collect::<Result<Vec<_>>>()?;
        let hole = loft_numbered_loops(&secs, false)?;
        if hole.len() != sections.len() {
            return Err(KernelError::Invalid("missing loft hole".into()));
        }
        for ((_, pieces), (_, h)) in sections.iter_mut().zip(&hole) {
            pieces.extend(crate::smlib_sweeps::pieces([&h.reversed()]));
        }
    }
    let first = if reversed { sections.len().saturating_sub(1) } else { 0 };
    crate::smlib_sweeps::loft(&sections, first)
}

pub(crate) fn loft_reversed(sections: &[(Plane, Loop2)]) -> Result<bool> {
    if sections.len() < 2 || sections.len() > 50 {
        return Err(KernelError::Invalid("a loft needs 2…50 sections".into()));
    }
    let n = sections.iter().map(|(_, l)| l.segs.len()).max().unwrap_or(0);
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid("empty section".into()));
    }
    // A canonical order, so that reversing the sections changes nothing: the one whose
    // first-to-last step has a positive largest component. The side faces' parameterisation
    // follows the order, and the boolean worked on one and failed on the other (a cut whose
    // loft touches the body along an edge: an internal panic, or "shell not oriented and
    // closed").
    let centre = |(p, l): &(Plane, Loop2)| p.to_world(l.centroid());
    // Two consecutive sections in one plane span no volume between them: a loft through them
    // used to build a flat, zero-volume body and report success.
    for (i, w) in sections.windows(2).enumerate() {
        let (a, b) = (&w[0], &w[1]);
        let n = a.0.normal();
        let size = (centre(a) - a.0.origin).len().max((centre(b) - b.0.origin).len()).max(1.0);
        let parallel = n.cross(b.0.normal()).len() < 1e-9;
        let in_plane = (centre(b) - centre(a)).dot(n).abs() < 1e-9 * size;
        if parallel && in_plane {
            return Err(KernelError::Invalid(format!("loft sections {} and {} lie in the same plane (no volume between them)", i + 1, i + 2)));
        }
    }
    let (Some(first), Some(last)) = (sections.first(), sections.last()) else { return Err(KernelError::Invalid("no sections".into())) };
    let step = centre(last) - centre(first);
    let big = [step.x, step.y, step.z].into_iter().max_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0);
    Ok(big < 0.0)
}

/// The loft's sections as the walls are built through them: in canonical order (`reversed`
/// turns them round), each oriented the same way, re-split to the same number of segments and
/// started at the segment that lines up with the section before it. Every section is matched to
/// its neighbour, as the walls pair them: it winds the way the section before it does about the
/// normal they share (its plane's normal, turned over where it faces back from the one before),
/// so sections whose planes turn on round the loft (a bar bent half a turn) keep one winding;
/// and it starts where its vertices line up best with the section before it. The first section is
/// matched to the one with the most segments, whose count every section takes.
pub(crate) fn loft_numbered_loops(sections: &[(Plane, NumberedLoop)], reversed: bool) -> Result<Vec<(Plane, NumberedLoop)>> {
    let n = sections.iter().map(|(_, l)| l.lp.segs.len()).max().unwrap_or(0);
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid("empty section".into()));
    }
    // The sections' segments are numbered in the order given.
    let mut ordered = sections.to_vec();
    if reversed {
        ordered.reverse();
    }
    let lead = ordered.iter().max_by_key(|(_, l)| l.lp.segs.len()).ok_or_else(|| KernelError::Invalid("no sections".into()))?;
    let corners = |p: &Plane, l: &NumberedLoop| -> (Vec3, Vec<Vec3>) {
        let c = p.to_world(l.lp.centroid());
        (c, l.lp.segs.iter().map(|s| p.to_world(s.start()) - c).collect())
    };
    let first_normal = ordered.first().map(|(p, _)| p.normal()).ok_or_else(|| KernelError::Invalid("no sections".into()))?;
    // What each section is matched to: the normal the windings share, and the corners' directions
    // from the centre (the lead's for the first section, then the section before).
    let (mut normal, mut dirs) = (first_normal, corners(&lead.0, &lead.1.oriented(lead.0.normal().dot(first_normal) < 0.0)).1);
    // The share of the lead's perimeter before each of its vertices: where a circle section
    // breaks when the matched directions do not go round in order.
    let lead_len: f64 = lead.1.lp.segs.iter().map(Seg2::length).sum();
    let fractions: Vec<f64> = lead
        .1
        .lp
        .segs
        .iter()
        .scan(0.0, |before, s| {
            let f = *before / lead_len;
            *before += s.length();
            Some(f)
        })
        .collect();
    let mut out = Vec::with_capacity(ordered.len());
    for (p, l) in &ordered {
        let back = p.normal().dot(normal) < 0.0;
        let local: Vec<Vec2> = dirs.iter().map(|d| Vec2::new(d.dot(p.x), d.dot(p.y))).collect();
        // Break angles: the matched corners seen in this plane. A re-split circle runs counter-
        // clockwise in its plane; wound the other way when the plane faces back.
        let angles: Vec<f64> = local.iter().map(|d| d.angle()).collect();
        let mut r = l.oriented(back).resample(n, &angles, &fractions, false).oriented(back);
        // Start where every vertex lines up best with the matched one of the same number (the
        // largest sum of dot products of their directions from the centroids): the single vertex
        // closest in direction twisted non-convex sections (#881).
        let c = r.lp.centroid();
        let m = r.lp.segs.len();
        let fit =
            |k: usize| -> f64 { local.iter().enumerate().filter_map(|(i, d)| r.lp.segs.get((i + k) % m).map(|s| (s.start() - c).dot(*d))).sum() };
        if let Some(k) = (0..m).min_by(|a, b| fit(*b).total_cmp(&fit(*a))) {
            r.rotate_left(k);
        }
        normal = if back { p.normal() * -1.0 } else { p.normal() };
        dirs = corners(p, &r).1;
        out.push((*p, r));
    }
    Ok(out)
}

/// A 3D path segment for sweeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSeg {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// Arc around `center`, rotating by `angle` about `axis` (right hand) from `a`.
    Arc {
        a: Vec3,
        center: Vec3,
        axis: Vec3,
        angle: f64,
    },
    /// A free-form curve of a sketch (a Bézier or conic piece): `seg` on `plane`.
    Curve {
        plane: Plane,
        seg: Seg2,
    },
    /// A rational Bézier span in space: a span of a drawn 3D sketch spline
    /// ([`crate::PathPiece::Spline`]) or of an edge's curve ([`crate::edge_path`]).
    Bezier(crate::point_path::SpaceBezier),
}

impl PathSeg {
    /// Where the segment starts.
    pub fn start(&self) -> Vec3 {
        match *self {
            PathSeg::Line { a, .. } | PathSeg::Arc { a, .. } => a,
            PathSeg::Curve { plane, seg } => plane.to_world(seg.start()),
            PathSeg::Bezier(b) => b.start(),
        }
    }

    /// Where the segment ends.
    pub fn end(&self) -> Vec3 {
        match *self {
            PathSeg::Line { b, .. } => b,
            PathSeg::Arc { a, center, axis, angle } => axis.normalized().map_or(a, |k| rotate_about(a, center, k, angle)),
            PathSeg::Curve { plane, seg } => plane.to_world(seg.end()),
            PathSeg::Bezier(b) => b.end(),
        }
    }

    /// Unit directions of the path where the segment starts and ends.
    pub fn tangents(&self) -> Option<(Vec3, Vec3)> {
        match *self {
            PathSeg::Line { a, b } => {
                let t = (b - a).normalized()?;
                Some((t, t))
            }
            PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized()?;
                let end = rotate_about(a, center, k, angle);
                Some((k.cross(a - center).normalized()?, k.cross(end - center).normalized()?))
            }
            PathSeg::Curve { plane, seg } => {
                Some((plane.dir_to_world(seg.start_tangent()).normalized()?, plane.dir_to_world(seg.end_tangent()).normalized()?))
            }
            PathSeg::Bezier(b) => Some((b.derivative(0.0)?.normalized()?, b.derivative(1.0)?.normalized()?)),
        }
    }

    /// The segment the other way round.
    pub fn reversed(&self) -> PathSeg {
        match *self {
            PathSeg::Line { a, b } => PathSeg::Line { a: b, b: a },
            PathSeg::Arc { center, axis, angle, .. } => PathSeg::Arc { a: self.end(), center, axis: axis * -1.0, angle },
            PathSeg::Curve { plane, seg } => PathSeg::Curve { plane, seg: seg.reversed() },
            PathSeg::Bezier(b) => PathSeg::Bezier(b.reversed()),
        }
    }

    /// The segment carried by the rigid motion `point` of points and `vector` of directions
    /// (a placement); `None` where a carried span no longer evaluates.
    pub fn mapped(&self, point: impl Fn(Vec3) -> Vec3, vector: impl Fn(Vec3) -> Vec3) -> Option<PathSeg> {
        Some(match *self {
            PathSeg::Line { a, b } => PathSeg::Line { a: point(a), b: point(b) },
            PathSeg::Arc { a, center, axis, angle } => PathSeg::Arc { a: point(a), center: point(center), axis: vector(axis), angle },
            PathSeg::Curve { plane, seg } => PathSeg::Curve { plane: Plane::new(point(plane.origin), vector(plane.x), vector(plane.y))?, seg },
            PathSeg::Bezier(b) => PathSeg::Bezier(b.mapped(point)?),
        })
    }

    /// A sketch segment on `plane` as the path segment that is the same curve: a line, an arc
    /// about the plane's normal, or the free curve on its plane.
    pub fn of_sketch(plane: &Plane, s: &Seg2) -> PathSeg {
        let w = |p: Vec2| plane.to_world(p);
        match *s {
            Seg2::Line { a, b } => PathSeg::Line { a: w(a), b: w(b) },
            Seg2::Arc { center, sweep, .. } => {
                PathSeg::Arc { a: w(s.start()), center: w(center), axis: plane.normal() * sweep.signum(), angle: sweep.abs() }
            }
            Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => PathSeg::Curve { plane: *plane, seg: *s },
        }
    }

    /// The path with each free-form curve as the lines of its polyline within 0.01 mm, as the
    /// sheet sweeps follow it.
    pub fn flattened(path: &[PathSeg]) -> Vec<PathSeg> {
        path.iter()
            .flat_map(|s| match *s {
                PathSeg::Curve { plane, seg } => {
                    seg.polyline(1e-2).windows(2).map(|q| PathSeg::Line { a: plane.to_world(q[0]), b: plane.to_world(q[1]) }).collect()
                }
                _ => vec![*s],
            })
            .collect()
    }

    /// Points along the segment (an arc within `tol` of its chords), without its start.
    pub(crate) fn points_after_start(&self, tol: f64) -> Vec<Vec3> {
        match *self {
            PathSeg::Line { b, .. } => vec![b],
            PathSeg::Curve { plane, seg } => seg.polyline(tol).into_iter().skip(1).map(|q| plane.to_world(q)).collect(),
            PathSeg::Bezier(b) => b.polyline(tol),
            PathSeg::Arc { a, center, axis, angle } => {
                let Some(k) = axis.normalized() else { return vec![a] };
                let r = (a - center).len();
                let step = if r > tol { 2.0 * (1.0 - tol / r).acos() } else { angle.abs() };
                let n = ((angle.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 512);
                (1..=n).map(|i| rotate_about(a, center, k, angle * i as f64 / n as f64)).collect()
            }
        }
    }
}

/// `p` turned by `angle` about the unit axis `k` through `center` (right hand).
pub(crate) fn rotate_about(p: Vec3, center: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let v = p - center;
    let (s, c) = angle.sin_cos();
    center + v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// [`sweep_with`] through SMLib: its curve sweep for the profile turning with the path or only
/// moving along it (orientation parallel), the walls meeting in mitres at the path's corners; a
/// twist, an end scale, guide rails or an orientation direction place the profile at stations
/// along a path without corners ([`crate::smlib_sweeps::sweep_controlled`]).
fn smlib_sweep(plane: &Plane, region: &Region2, path: &[PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
    ctl.check()?;
    region_ok(region)?;
    if path.is_empty() || path.len() > 1000 {
        return Err(KernelError::Invalid("the path needs 1…1000 segments".into()));
    }
    if path.iter().any(|s| s.tangents().is_none()) {
        return Err(KernelError::Invalid("a path segment has no direction".into()));
    }
    let translate = match ctl.orientation {
        _ if ctl.varies() => return crate::smlib_sweeps::sweep_controlled(plane, &oriented(region)?, path, ctl),
        crate::SweepOrientation::Perpendicular => false,
        crate::SweepOrientation::Parallel => true,
        crate::SweepOrientation::Direction(_) => return crate::smlib_sweeps::sweep_controlled(plane, &oriented(region)?, path, ctl),
    };
    crate::smlib_sweeps::sweep(plane, &crate::smlib_sweeps::pieces(oriented(region)?.loops()), path, translate)
}

/// The path as a polyline: its first point, then each segment's points (arcs within 0.01 mm of
/// their chords).
pub fn path_polyline(path: &[PathSeg]) -> Result<Vec<Vec3>> {
    let first = path.first().map(PathSeg::start).ok_or_else(|| KernelError::Invalid("the path needs 1…1000 segments".into()))?;
    let mut pts = vec![first];
    for seg in path {
        pts.extend(seg.points_after_start(1e-2));
    }
    Ok(pts)
}

/// [`sweep`] with sweep options. The default options are exactly [`sweep`]; a twist, an end
/// scale, guide rails or an orientation direction place the profile at stations along the path.
pub fn sweep_with(plane: &Plane, region: &Region2, path: &[PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep_with");
    smlib_sweep(plane, region, path, ctl)
}

/// Sweep a planar region along a chain of path segments, keeping the profile perpendicular to
/// the path; at sharp corners the walls meet in mitres.
pub fn sweep(plane: &Plane, region: &Region2, path: &[PathSeg]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep");
    smlib_sweep(plane, region, path, &crate::SweepControl::default())
}
