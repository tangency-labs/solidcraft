//! Building bodies: extrude and revolve planar regions, primitives, lofts and sweeps, each
//! through SMLib ([`crate::smlib_sweeps`]). The wire, face and curve helpers below (chain and
//! profile wires, ruled and smooth loft faces, cubic interpolation, sphere patch faces) are kept
//! only for the modules that still build on them (surfaces, blends, loft rails, the STEP
//! reader); they go when those are ported.

use mt::builder;
use solidcraft_geom::{Grid, Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3, v3};
use crate::numbered::{self, NumberedLoop, NumberedRegion};
use crate::record::{self, Side};
use crate::sweep_path::RailFit;
use crate::{KernelError, Result, guard};

const MAX_SEGS: usize = 20_000;

fn finite(x: f64, what: &str) -> Result<f64> {
    if x.is_finite() && x.abs() < 1e7 { Ok(x) } else { Err(KernelError::Invalid(format!("{what} = {x}"))) }
}

/// A closed wire in 3D from a sketch loop, with each edge's segment and that segment's
/// position in the loop (a big arc makes two edges).
fn wire_sources(plane: &Plane, lp: &Loop2) -> Result<(mt::Wire, Vec<(mt::Edge, Seg2, usize)>)> {
    let n = lp.segs.len();
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid(format!("loop with {n} segments")));
    }
    if !lp.is_closed(1e-6) {
        return Err(KernelError::Invalid("profile loop is not closed".into()));
    }
    chain_sources(plane, &lp.segs, true)
}

/// An open wire in 3D through a chain of sketch segments, each starting where the one before
/// ends (a closed chain makes a closed wire).
pub(crate) fn chain_wire(plane: &Plane, segs: &[Seg2]) -> Result<mt::Wire> {
    chain_wire_seeds(plane, segs).map(|(wire, _)| wire)
}

/// [`chain_wire`], with each edge's id and the position in the chain of the segment it was
/// built from (a big arc makes two edges), in wire order.
pub(crate) fn chain_wire_seeds(plane: &Plane, segs: &[Seg2]) -> Result<(mt::Wire, Vec<(mt::EdgeID, usize)>)> {
    let n = segs.len();
    if n == 0 || n > MAX_SEGS {
        return Err(KernelError::Invalid(format!("curve chain with {n} segments")));
    }
    if !segs.iter().all(seg_ok) {
        return Err(KernelError::Invalid("non-finite curve".into()));
    }
    for (s, t) in segs.iter().zip(segs.iter().skip(1)) {
        if s.end().dist(t.start()) > 1e-6 {
            return Err(KernelError::Invalid("the curves are not connected end to end".into()));
        }
    }
    let closed = segs.first().zip(segs.last()).is_some_and(|(f, l)| l.end().dist(f.start()) <= 1e-6);
    let (wire, sources) = chain_sources(plane, segs, closed)?;
    Ok((wire, sources.into_iter().map(|(e, _, i)| (e.id(), i)).collect()))
}

/// The edges of a chain of segments (closed: the last one ends on the first's start vertex),
/// with each edge's segment and that segment's position in the chain (a big arc makes two
/// edges).
fn chain_sources(plane: &Plane, segs: &[Seg2], closed: bool) -> Result<(mt::Wire, Vec<(mt::Edge, Seg2, usize)>)> {
    let n = segs.len();
    let mut verts: Vec<mt::Vertex> = segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
    if !closed && let Some(last) = segs.last() {
        verts.push(builder::vertex(p3(plane.to_world(last.end()))));
    }
    let mut edges = Vec::with_capacity(n);
    let mut sources = Vec::with_capacity(n);
    for (i, s) in segs.iter().enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get(if closed { (i + 1) % n } else { i + 1 })) else { continue };
        for e in seg_edges(plane, s, a, b)? {
            sources.push((e.clone(), *s, i));
            edges.push(e);
        }
    }
    Ok((edges.into(), sources))
}

/// The edges of one segment from `a` to `b` (a big arc makes two).
fn seg_edges(plane: &Plane, s: &Seg2, a: &mt::Vertex, b: &mt::Vertex) -> Result<Vec<mt::Edge>> {
    Ok(match *s {
        Seg2::Line { .. } => vec![builder::line(a, b)],
        Seg2::Arc { sweep, .. } => {
            if sweep.abs() < 1e-9 {
                return Err(KernelError::Invalid("zero-length arc".into()));
            }
            // Split big arcs so every piece is well conditioned.
            if sweep.abs() > std::f64::consts::PI * 1.5 {
                let vm = builder::vertex(p3(plane.to_world(s.mid())));
                vec![
                    builder::circle_arc(a, &vm, p3(plane.to_world(s.point_at(0.25)))),
                    builder::circle_arc(&vm, b, p3(plane.to_world(s.point_at(0.75)))),
                ]
            } else {
                vec![builder::circle_arc(a, b, p3(plane.to_world(s.mid())))]
            }
        }
        Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => {
            vec![crate::freeform::edge(plane, s, a, b).ok_or_else(|| KernelError::Invalid("profile curve".into()))?]
        }
    })
}

/// An edge of a profile wire: the segment it was built from and the profile segments under it.
pub(crate) struct ProfileEdge {
    pub edge: mt::Edge,
    pub segment: Seg2,
    pub numbers: Vec<usize>,
}

/// A wire built from a profile loop, with its edges in wire order.
pub(crate) struct ProfileWire {
    pub wire: mt::Wire,
    pub edges: Vec<ProfileEdge>,
}

/// The wires of a region oriented as the builders build it ([`NumberedRegion::oriented`]), each
/// with what its edges were built from.
fn region_wires(plane: &Plane, r: &NumberedRegion) -> Result<Vec<ProfileWire>> {
    r.loops()
        .map(|lp| {
            let (wire, s) = wire_sources(plane, &lp.lp)?;
            let edges = s.into_iter().map(|(edge, segment, i)| ProfileEdge { edge, segment, numbers: lp.numbers(i).to_vec() }).collect();
            Ok(ProfileWire { wire, edges })
        })
        .collect()
}

/// The wires alone.
fn wires_of(profile: &[ProfileWire]) -> Vec<mt::Wire> {
    profile.iter().map(|p| p.wire.clone()).collect()
}

/// A region numbered on its own, as given.
fn numbered_region(r: &Region2) -> Result<NumberedRegion> {
    numbered::number_regions(std::slice::from_ref(r)).pop().ok_or_else(|| KernelError::Invalid("profile".into()))
}

/// A region numbered on its own and oriented as the builders build it.
fn oriented(r: &Region2) -> Result<NumberedRegion> {
    numbered_region(r).map(|n| n.oriented())
}

/// A planar face from a region (outer loop with holes).
pub(crate) fn face(plane: &Plane, r: &Region2) -> Result<mt::Face> {
    attach_profile(&wires_of(&region_wires(plane, &oriented(r)?)?))
}

fn attach_profile(wires: &[mt::Wire]) -> Result<mt::Face> {
    guard("face", || builder::try_attach_plane(wires).map_err(|e| KernelError::Invalid(format!("profile: {e}"))))
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
    extrude_with(plane, regions, start, end, false)
}

/// [`extrude`] with each run of profile lines that continue one another in a straight line (a side
/// drawn in two pieces, a mirrored half meeting its mirror image) swept into one side face over
/// all its pieces, rather than coplanar faces with an edge between them (#1788). The side's
/// history lists every piece under it. For the Extrude feature; other builders (sheet metal's
/// flat pattern, tools) keep a face per segment.
pub fn extrude_whole_sides(plane: &Plane, regions: &[Region2], start: f64, end: f64) -> Result<Vec<Body>> {
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
    revolved(plane, regions, axis_origin, axis_dir, angle)
}

/// [`revolve`] for bodies that are final (new bodies, not a boolean's tools), with each planar
/// face whole (#726): SMLib's revolution makes one face of each profile segment and of each
/// cap, so this is [`revolve`].
pub fn revolve_whole(plane: &Plane, regions: &[Region2], axis_origin: Vec2, axis_dir: Vec2, angle: f64) -> Result<Vec<Body>> {
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
    let radius = finite(radius, "radius")?;
    if radius <= 1e-6 || !center.is_finite() || center.len() >= 1e7 {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    crate::smlib_sweeps::sphere(center, radius)
}

/// Torus about the Z axis through `center`.
pub fn torus(center: Vec3, major: f64, minor: f64) -> Result<Body> {
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
/// (radians; positive grows the profile).
pub fn extrude_tapered(plane: &Plane, region: &Region2, length: f64, dir_sign: f64, taper: f64) -> Result<Body> {
    extrude_tapered_between(plane, region, 0.0, length, 0.0, dir_sign, taper)
}

/// [`extrude_tapered`] from `from` to `to` along `dir_sign` × the plane normal, the region
/// being the section at 0: its sides run on through it, so `from` below 0 starts the solid
/// that far before the profile (into a face the profile lies on), at the profile shrunk or
/// grown by the taper. With a `foot` (a height, less than `to - from`), the sides stand
/// straight for that height above `from` and only then slope, up to the same top: a face the
/// solid stands on can then be pushed into the body it joins, as a prism's can.
pub fn extrude_tapered_between(plane: &Plane, region: &Region2, from: f64, to: f64, foot: f64, dir_sign: f64, taper: f64) -> Result<Body> {
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
    smlib_tapered(plane, region, from, to, foot, dir_sign.signum(), taper)
}

/// [`extrude_tapered_between`] through SMLib's draft extrusion: one loop (SMLib's takes no
/// holes), its section at `from` the profile offset by the taper, after any straight foot.
fn smlib_tapered(plane: &Plane, region: &Region2, from: f64, to: f64, foot: f64, sign: f64, taper: f64) -> Result<Body> {
    if !region.holes.is_empty() {
        return Err(crate::smlib_sweeps::refuse("draft extrusion of a profile with holes (SMLib's takes one loop)"));
    }
    let outer = oriented(region)?.map_loops(|l| l.merged_lines(continues_straight)).outer;
    let bottom = if from == 0.0 || taper == 0.0 {
        outer
    } else {
        let moved = offset_loop(&outer.lp, from * taper.tan())?;
        let segs: Option<Vec<Seg2>> = moved.into_iter().collect();
        let segs = segs.ok_or_else(|| crate::smlib_sweeps::refuse("draft extrusion that starts before its profile where the taper closes a side"))?;
        NumberedLoop { lp: Loop2 { segs }, from: outer.from }
    };
    let draft = (taper != 0.0).then_some(crate::smlib_sweeps::Draft { taper, foot });
    crate::smlib_sweeps::extrude(plane, &crate::smlib_sweeps::pieces([&bottom]), Some(region), from * sign, to - from, sign, draft)
}

/// Ruled loft through two or more planar sections (outer loops only). The sections and the
/// same sections in reverse order give the same body.
pub fn loft(sections: &[(Plane, Loop2)]) -> Result<Body> {
    loft_with_rails(sections, &[], RailFit::Scale)
}

/// [`loft`] following one or two guide rails (polylines from the first section to the last,
/// through every section's curve; see [`crate::loft_rails`]), `fit` saying how they reshape the
/// sections between. Without rails, the plain loft.
pub fn loft_with_rails(sections: &[(Plane, Loop2)], rails: &[Vec<Vec3>], fit: RailFit) -> Result<Body> {
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

/// The wires of a loft's closed sections, matched: oriented alike, resampled to the same
/// segment count and turned to start alike; with the profile segments under each wire edge, and
/// whether the sections were turned round into the canonical order.
pub(crate) fn loft_wires(sections: &[(Plane, Loop2)]) -> Result<(Vec<mt::Wire>, Vec<Vec<Vec<usize>>>, bool)> {
    let reversed = loft_reversed(sections)?;
    let ordered = sections.iter().map(|(p, _)| *p).zip(numbered::number_loops(sections.iter().map(|(_, l)| l))).collect::<Vec<_>>();
    let (wires, sources) = guard("loft sections", || loft_numbered_wires(&ordered, reversed))?;
    Ok((wires, sources, reversed))
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

fn loft_numbered_wires(sections: &[(Plane, NumberedLoop)], reversed: bool) -> Result<(Vec<mt::Wire>, Vec<Vec<Vec<usize>>>)> {
    let mut wires: Vec<mt::Wire> = Vec::new();
    // Per section, the profile segments under each of its wire's edges.
    let mut edge_sources: Vec<Vec<Vec<usize>>> = Vec::new();
    for (p, r) in loft_numbered_loops(sections, reversed)? {
        wires.push(uniform_wire(&p, &r.lp)?);
        edge_sources.push(r.from);
    }
    Ok((wires, edge_sources))
}

/// The loft's sections as the walls are built through them: in canonical order (`reversed`
/// turns them round), each oriented the same way, re-split to the same number of segments and
/// started at the segment that lines up with the lead section's first.
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
    // Orient every section counter-clockwise about the first section's normal (`Body::new`
    // turns the solid outward), then match segment counts and starting points to the section
    // with the most segments.
    let n0 = ordered.first().map(|(p, _)| p.normal()).unwrap_or(Vec3::Z);
    let parallel = ordered.iter().all(|(p, _)| p.normal().dot(n0).abs() > 1.0 - 1e-9);
    let oriented: Vec<(Plane, NumberedLoop)> = ordered
        .iter()
        .map(|(p, l)| {
            let ccw = p.normal().dot(n0) >= 0.0;
            (*p, l.oriented(!ccw))
        })
        .collect();
    let lead = oriented.iter().max_by_key(|(_, l)| l.lp.segs.len()).ok_or_else(|| KernelError::Invalid("no sections".into()))?;
    let lead_c = lead.0.to_world(lead.1.lp.centroid());
    let lead_dirs: Vec<Vec3> = lead.1.lp.segs.iter().map(|s| lead.0.to_world(s.start()) - lead_c).collect();
    let mut out = Vec::with_capacity(oriented.len());
    for (p, l) in &oriented {
        let c = l.lp.centroid();
        // Break angles: the lead section's vertex directions seen in this plane.
        let angles: Vec<f64> = lead_dirs.iter().map(|d| Vec2::new(d.dot(p.x), d.dot(p.y)).angle()).collect();
        let mut r = l.resample(n, &angles);
        // Circle resampling creates increasing local angles. An antiparallel sketch
        // plane must run clockwise locally to keep the same winding in world space.
        // Curving a run through a right angle does not reverse a section's winding.
        if parallel && p.normal().dot(n0) < 0.0 {
            r = r.oriented(true);
        }
        // Start where every vertex lines up best with the lead's vertex of the same number (the
        // largest sum of dot products of their directions from the centroids). The one vertex
        // closest in direction to the lead's first vertex twisted non-convex sections (#881): a
        // zig-zag's centroid sits off its outline, so its first vertex can point away from the
        // lead's.
        let dirs: Vec<Vec2> = lead_dirs.iter().map(|d| Vec2::new(d.dot(p.x), d.dot(p.y))).collect();
        let m = r.lp.segs.len();
        let fit =
            |k: usize| -> f64 { dirs.iter().enumerate().filter_map(|(i, d)| r.lp.segs.get((i + k) % m).map(|s| (s.start() - c).dot(*d))).sum() };
        if let Some(k) = (0..m).min_by(|a, b| fit(*b).total_cmp(&fit(*a))) {
            r.rotate_left(k);
        }
        out.push((*p, r));
    }
    Ok(out)
}

/// Ruled faces between each two neighbouring section wires; each comes from the section edges
/// it spans.
pub(crate) fn ruled_loft_faces(wires: &[mt::Wire], edge_sources: &[Vec<Vec<usize>>]) -> Result<Vec<Side>> {
    let mut sides: Vec<Side> = Vec::new();
    for (k, w) in wires.windows(2).enumerate() {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let shell = builder::try_wire_homotopy(a, b).map_err(|e| KernelError::Failed(format!("loft: {e}")))?;
        match (edge_sources.get(k), edge_sources.get(k + 1)) {
            (Some(na), Some(nb)) => sides.extend(record::ruled(&shell, a, b, na, nb)),
            _ => sides.extend(shell.face_iter().map(|f| (f.clone(), None))),
        }
    }
    Ok(sides)
}

/// Points across the sections (one per section, in order) as a smooth B-spline through them at the
/// parameters `u` (rising from 0 to 1, one per section): cubic through four or more, quadratic
/// through three.
pub(crate) fn across(pts: &[Vec3], u: &[f64]) -> Option<mt::BSplineCurve<mt::Point3>> {
    match pts {
        [a, b, c] => {
            // The quadratic Bezier through `b` at t = u[1].
            let t = *u.get(1)?;
            if !(t > 0.0 && t < 1.0) {
                return None;
            }
            let m = (*b - *a * ((1.0 - t) * (1.0 - t)) - *c * (t * t)) * (1.0 / (2.0 * t * (1.0 - t)));
            mt::BSplineCurve::try_new(mt::KnotVec::bezier_knot(2), vec![p3(*a), p3(m), p3(*c)]).ok()
        }
        _ => interpolate_cubic_at(pts, u),
    }
}

/// The parameters of the sections along a loft: in proportion to the distance between them (the
/// mean over the wires' vertices), so that unevenly spaced sections get a surface that follows
/// them. At a uniform parameter a section close to its neighbour makes the surface overshoot
/// between the others (by several times the section's size when the spacing differs tenfold).
/// Evenly spaced sections give the uniform parameters.
pub(crate) fn section_parameters(edges: &[Vec<mt::Edge>]) -> Vec<f64> {
    let m = edges.len();
    let uniform = || (0..m).map(|i| i as f64 / (m.max(2) - 1) as f64).collect::<Vec<f64>>();
    let corners: Vec<Vec<Vec3>> = edges.iter().map(|es| es.iter().map(|e| from_p3(e.front().point())).collect()).collect();
    let mut u = vec![0.0];
    for w in corners.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return uniform() };
        let n = a.len().min(b.len());
        if n == 0 {
            return uniform();
        }
        let step = a.iter().zip(b).map(|(p, q)| p.dist(*q)).sum::<f64>() / n as f64;
        u.push(u.last().copied().unwrap_or(0.0) + step);
    }
    let Some(total) = u.last().copied().filter(|t| t.is_finite() && *t > 0.0) else { return uniform() };
    let u: Vec<f64> = u.iter().map(|x| x / total).collect();
    // A section on top of its neighbour: no parameter for it, so the uniform ones.
    if u.windows(2).any(|w| w.get(1).zip(w.first()).is_none_or(|(b, a)| b - a < 1e-6)) {
        return uniform();
    }
    u
}

/// A loft whose side faces are B-spline surfaces through every section (smooth across the
/// middle sections, which leave no edges). Every section wire comes from `uniform_wire`, so the
/// k-th edges of all sections share one knot vector and their control points pair up. Face k
/// comes from the profile segments under the k-th edge of every section (`numbers`: by
/// section, by edge).
/// One smooth face through every section for each wire edge (cubic across four sections or
/// more, quadratic across three), neighbours sharing their seams. `closed` wires join their
/// last edge to their first; open ones have a seam at each end.
pub(crate) fn smooth_loft_faces(wires: &[mt::Wire], numbers: &[Vec<Vec<usize>>], closed: bool) -> Result<Vec<Side>> {
    let fail = |m: &str| KernelError::Failed(format!("smooth loft: {m}"));
    let edges: Vec<Vec<mt::Edge>> = wires.iter().map(|w| w.edge_iter().cloned().collect()).collect();
    let n = edges.first().map(Vec::len).ok_or_else(|| fail("no sections"))?;
    if edges.iter().any(|e| e.len() != n) {
        return Err(fail("sections with different edge counts"));
    }
    let curve_of = |e: &mt::Edge| -> Option<mt::BSplineCurve<mt::Point3>> {
        match e.curve() {
            mt::Curve::BSplineCurve(c) if e.orientation() => Some(c),
            _ => None,
        }
    };
    let (Some(first), Some(last)) = (edges.first(), edges.last()) else { return Err(fail("sections")) };
    let along = section_parameters(&edges);
    // Seams: through the k-th vertex of every section (an open wire's last one at the end of
    // its last edge).
    let vertex = |e: &mt::Edge, end: bool| if end { e.back().clone() } else { e.front().clone() };
    let seam_count = if closed { n } else { n + 1 };
    let mut seams: Vec<mt::Edge> = Vec::with_capacity(seam_count);
    for k in 0..seam_count {
        let (at, end) = if k < n { (k, false) } else { (n - 1, true) };
        let pts: Vec<Vec3> = edges.iter().filter_map(|es| es.get(at).map(|e| from_p3(vertex(e, end).point()))).collect();
        let c = across(&pts, &along).ok_or_else(|| fail("seam"))?;
        let (Some(a), Some(b)) = (first.get(at), last.get(at)) else { return Err(fail("seam")) };
        seams.push(mt::Edge::new(&vertex(a, end), &vertex(b, end), mt::Curve::BSplineCurve(c)));
    }
    let mut faces = Vec::with_capacity(n);
    for k in 0..n {
        let curves: Vec<mt::BSplineCurve<mt::Point3>> =
            edges.iter().map(|es| es.get(k).and_then(curve_of)).collect::<Option<_>>().ok_or_else(|| fail("section curves"))?;
        let c0 = curves.first().ok_or_else(|| fail("section curves"))?;
        let uk = c0.knot_vec().clone();
        let m = c0.control_points().len();
        if curves.iter().any(|c| c.control_points().len() != m || c.knot_vec() != &uk) {
            return Err(fail("sections parameterised differently"));
        }
        let mut ctrl: Vec<Vec<mt::Point3>> = Vec::with_capacity(m);
        let mut vk = None;
        for j in 0..m {
            let pts: Vec<Vec3> = curves.iter().filter_map(|c| c.control_points().get(j).map(|p| from_p3(*p))).collect();
            let c = across(&pts, &along).ok_or_else(|| fail("surface"))?;
            vk = Some(c.knot_vec().clone());
            ctrl.push(c.control_points().clone());
        }
        let vk = vk.ok_or_else(|| fail("surface"))?;
        let surface = mt::BSplineSurface::try_new((uk, vk), ctrl).map_err(|e| fail(&e.to_string()))?;
        let (Some(e0), Some(e1), Some(s0), Some(s1)) = (first.get(k), last.get(k), seams.get(k), seams.get((k + 1) % seam_count)) else {
            return Err(fail("face"));
        };
        // u along the section edge, v across the sections: counter-clockwise in (u, v).
        let w: mt::Wire = vec![e0.clone(), s1.clone(), e1.inverse(), s0.inverse()].into();
        let face = mt::Face::try_new(vec![w], mt::Surface::BSplineSurface(surface)).map_err(|e| fail(&e.to_string()))?;
        let under: Vec<&usize> = numbers.iter().filter_map(|s| s.get(k)).flatten().collect();
        faces.push((face, (under.len() >= numbers.len().max(1)).then(|| record::profile(under))));
    }
    Ok(faces)
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
            Seg2::Arc { center, sweep, .. } => PathSeg::Arc { a: w(s.start()), center: w(center), axis: plane.normal() * sweep.signum(), angle: sweep.abs() },
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
    let controlled = ctl.varies() || matches!(ctl.orientation, crate::SweepOrientation::Direction(_));
    if controlled {
        let ends: Vec<(Vec3, Vec3)> = path.iter().filter_map(PathSeg::tangents).collect();
        if ends.windows(2).any(|w| w[0].1.dot(w[1].0) < (1e-3f64).cos()) {
            return Err(crate::smlib_sweeps::refuse(
                "sweep with a twist, an end scale, guide rails or an orientation direction along a path with corners",
            ));
        }
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
    smlib_sweep(plane, region, path, ctl)
}

/// Sweep a planar region along a chain of path segments, keeping the profile perpendicular to
/// the path; at sharp corners the walls meet in mitres.
pub fn sweep(plane: &Plane, region: &Region2, path: &[PathSeg]) -> Result<Body> {
    smlib_sweep(plane, region, path, &crate::SweepControl::default())
}

/// Cox–de Boor basis values N_{j,3}(u) for all j.
fn cubic_basis(knots: &[f64], n_ctrl: usize, u: f64) -> Vec<f64> {
    let p = 3;
    let m = knots.len();
    let mut n = vec![0.0; m.saturating_sub(1)];
    let last = n_ctrl.saturating_sub(1);
    for (j, v) in n.iter_mut().enumerate() {
        let (Some(&a), Some(&b)) = (knots.get(j), knots.get(j + 1)) else { continue };
        // Half-open spans, with the last span closed at the end.
        if (a <= u && u < b) || (u >= b && (b - a) > 0.0 && j == last && (u - b).abs() < 1e-12) {
            *v = 1.0;
        }
    }
    for k in 1..=p {
        for j in 0..m.saturating_sub(1 + k) {
            let (Some(&tj), Some(&tjk), Some(&tj1), Some(&tjk1)) = (knots.get(j), knots.get(j + k), knots.get(j + 1), knots.get(j + k + 1)) else {
                continue;
            };
            let left = if tjk - tj > 0.0 { (u - tj) / (tjk - tj) * n.get(j).copied().unwrap_or(0.0) } else { 0.0 };
            let right = if tjk1 - tj1 > 0.0 { (tjk1 - u) / (tjk1 - tj1) * n.get(j + 1).copied().unwrap_or(0.0) } else { 0.0 };
            if let Some(v) = n.get_mut(j) {
                *v = left + right;
            }
        }
    }
    n.truncate(n_ctrl);
    n
}

/// Cubic B-spline through points at uniform parameters (clamped, knots by averaging).
pub(crate) fn interpolate_cubic(pts: &[Vec3]) -> Option<mt::BSplineCurve<mt::Point3>> {
    let m = pts.len();
    let u: Vec<f64> = (0..m).map(|i| i as f64 / (m.max(2) - 1) as f64).collect();
    interpolate_cubic_at(pts, &u)
}

/// A cubic B-spline surface through a grid of samples at uniform parameters: cubics across v
/// through each u index's samples, then cubics across u through their control points.
pub(crate) struct NetFit {
    pub surface: mt::BSplineSurface<mt::Point3>,
    /// The surface's edges at the first and the last u (curves across v).
    pub u_ends: [mt::BSplineCurve<mt::Point3>; 2],
    /// The surface's edges at the first and the last v (curves across u).
    pub v_ends: [mt::BSplineCurve<mt::Point3>; 2],
}

/// [`NetFit`] through `samples`; `None` with fewer than four samples across either direction.
pub(crate) fn interpolate_net(samples: &Grid<Vec3>) -> Option<NetFit> {
    let across_v: Vec<mt::BSplineCurve<mt::Point3>> = samples.transposed().lines().map(interpolate_cubic).collect::<Option<_>>()?;
    let across_u: Vec<mt::BSplineCurve<mt::Point3>> =
        control_net(&across_v)?.lines().map(|run| interpolate_cubic(&run.iter().map(|&p| from_p3(p)).collect::<Vec<_>>())).collect::<Option<_>>()?;
    let net = control_net(&across_u)?.transposed().into_u_rows();
    let knots = (across_u.first()?.knot_vec().clone(), across_v.first()?.knot_vec().clone());
    let surface = mt::BSplineSurface::try_new(knots, net).ok()?;
    let u_ends = [across_v.first()?.clone(), across_v.last()?.clone()];
    let v_ends = [across_u.first()?.clone(), across_u.last()?.clone()];
    Some(NetFit { surface, u_ends, v_ends })
}

/// The control points of `curves` as a grid: value `(i, j)` is control point `j` of curve `i`.
/// `None` unless every curve has as many control points.
fn control_net(curves: &[mt::BSplineCurve<mt::Point3>]) -> Option<Grid<mt::Point3>> {
    let rows: Vec<&[mt::Point3]> = curves.iter().map(|c| c.control_points().as_slice()).collect();
    Grid::from_u_rows(&rows)
}

/// [`interpolate_cubic`] by chord length: the parameters (0 to 1) in proportion to the distance
/// along the points. Points closer together toward an end (where a curve leaves another
/// tangentially) then don't slow the curve to a stop there, where a uniform parameter lets it
/// double back on itself.
pub(crate) fn interpolate_cubic_chordal(pts: &[Vec3]) -> Option<(mt::BSplineCurve<mt::Point3>, Vec<f64>)> {
    let mut u = vec![0.0];
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { return None };
        u.push(u.last().copied().unwrap_or(0.0) + a.dist(*b));
    }
    let total = u.last().copied().filter(|t| t.is_finite() && *t > 0.0)?;
    let u: Vec<f64> = u.iter().map(|x| x / total).collect();
    if u.windows(2).any(|w| w.first() >= w.get(1)) {
        return None;
    }
    interpolate_cubic_at(pts, &u).map(|c| (c, u))
}

/// The cubic B-spline through `pts` at parameters `u` (rising from 0 to 1), its knots averaged.
fn interpolate_cubic_at(pts: &[Vec3], u: &[f64]) -> Option<mt::BSplineCurve<mt::Point3>> {
    let m = pts.len();
    if m < 4 || u.len() != m {
        return None;
    }
    let mut knots = vec![0.0; 4];
    for j in 1..(m - 3) {
        knots.push((j..j + 3).filter_map(|i| u.get(i)).sum::<f64>() / 3.0);
    }
    knots.extend([1.0; 4]);
    // Dense solve of N · P = Q for each coordinate.
    let mut a: Vec<Vec<f64>> = u.iter().map(|ui| cubic_basis(&knots, m, *ui)).collect();
    let mut q: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    for col in 0..m {
        let piv = (col..m).max_by(|x, y| a[*x][col].abs().total_cmp(&a[*y][col].abs()))?;
        if a[piv][col].abs() < 1e-14 {
            return None;
        }
        a.swap(col, piv);
        q.swap(col, piv);
        for r in 0..m {
            if r == col {
                continue;
            }
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            let prow = a[col].clone();
            for (x, v) in a[r].iter_mut().zip(&prow).skip(col) {
                *x -= f * v;
            }
            let qc = q[col];
            for k in 0..3 {
                q[r][k] -= f * qc[k];
            }
        }
    }
    let ctrl: Vec<mt::Point3> = (0..m).map(|i| mt::Point3::new(q[i][0] / a[i][i], q[i][1] / a[i][i], q[i][2] / a[i][i])).collect();
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).ok()
}

/// A closed wire whose edges are cubic B-splines parameterised (nearly) by arc length, so that
/// ruled surfaces between sections pair points proportionally along each edge.
pub(crate) fn uniform_wire(plane: &Plane, lp: &Loop2) -> Result<mt::Wire> {
    uniform_chain_wire(plane, &lp.segs, true)
}

/// [`uniform_wire`] of a chain of segments, open or `closed`.
pub(crate) fn uniform_chain_wire(plane: &Plane, segs: &[Seg2], closed: bool) -> Result<mt::Wire> {
    const SAMPLES: usize = 24;
    let n = segs.len();
    let mut verts: Vec<mt::Vertex> = segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
    if !closed && let Some(last) = segs.last() {
        verts.push(builder::vertex(p3(plane.to_world(last.end()))));
    }
    let mut edges = Vec::with_capacity(n);
    for (i, s) in segs.iter().enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get(if closed { (i + 1) % n } else { i + 1 })) else { continue };
        let pts: Vec<Vec3> = (0..SAMPLES).map(|k| plane.to_world(s.point_at(k as f64 / (SAMPLES - 1) as f64))).collect();
        let curve = interpolate_cubic(&pts).ok_or_else(|| KernelError::Failed("loft curve".into()))?;
        edges.push(mt::Edge::new(a, b, mt::Curve::BSplineCurve(curve)));
    }
    Ok(edges.into())
}

/// Six outward patch faces of a sphere (the cube's faces projected onto it) on revolved surfaces
/// whose poles and seams lie outside each patch, for the STEP reader's spheres; callers guard them.
pub(crate) fn sphere_patch_faces(center: Vec3, radius: f64, rot: [Vec3; 3]) -> Result<Vec<mt::Face>> {
    let radius = finite(radius, "radius")?;
    let turn = |v: Vec3| rot[0] * v.x + rot[1] * v.y + rot[2] * v.z;
    if radius <= 1e-6 || !center.is_finite() {
        return Err(KernelError::Invalid("radius must be positive".into()));
    }
    let k = radius / 3f64.sqrt();
    let corner = |sx: f64, sy: f64, sz: f64| center + turn(Vec3::new(sx * k, sy * k, sz * k));
    // Cube faces: outward direction, rotation axis for the patch's surface (perpendicular to it),
    // and the four corner sign triples counter-clockwise seen from outside.
    let faces: [(Vec3, Vec3, [[f64; 3]; 4]); 6] = [
        (Vec3::X, Vec3::Z, [[1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]]),
        (-Vec3::X, Vec3::Z, [[-1.0, 1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0]]),
        (Vec3::Y, Vec3::Z, [[1.0, 1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]]),
        (-Vec3::Y, Vec3::Z, [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]]),
        (Vec3::Z, Vec3::X, [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
        (-Vec3::Z, Vec3::X, [[-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, -1.0, -1.0], [-1.0, -1.0, -1.0]]),
    ];
    let key = |s: [f64; 3]| ((s[0] > 0.0) as usize) | (((s[1] > 0.0) as usize) << 1) | (((s[2] > 0.0) as usize) << 2);
    let mut verts: std::collections::HashMap<usize, mt::Vertex> = std::collections::HashMap::new();
    let mut vert = |s: [f64; 3]| verts.entry(key(s)).or_insert_with(|| builder::vertex(p3(corner(s[0], s[1], s[2])))).clone();
    let mut edges: std::collections::HashMap<(usize, usize), mt::Edge> = std::collections::HashMap::new();
    let mut out: Vec<mt::Face> = Vec::new();
    for (dir, axis, cs) in faces {
        let (dir, axis) = (turn(dir), turn(axis));
        let mut wire: Vec<mt::Edge> = Vec::new();
        for i in 0..4 {
            let (a, b) = (cs[i], cs[(i + 1) % 4]);
            let (ka, kb) = (key(a), key(b));
            let e = match edges.get(&(ka.min(kb), ka.max(kb))) {
                Some(e) => e.clone(),
                None => {
                    let (lo, hi) = if ka < kb { (a, b) } else { (b, a) };
                    let (va, vb) = (vert(lo), vert(hi));
                    let mid = (corner(lo[0], lo[1], lo[2]) + corner(hi[0], hi[1], hi[2])) * 0.5 - center;
                    let transit = center + mid.normalized().unwrap_or(dir) * radius;
                    let e = builder::circle_arc(&va, &vb, p3(transit));
                    edges.insert((ka.min(kb), ka.max(kb)), e.clone());
                    e
                }
            };
            wire.push(if ka < kb { e } else { e.inverse() });
        }
        // The meridian runs pole to pole through the side away from the patch.
        let (np, sp, back) = (center + axis * radius, center - axis * radius, center - dir * radius);
        let meridian = builder::circle_arc(&builder::vertex(p3(np)), &builder::vertex(p3(sp)), p3(back));
        let mut surface =
            mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(meridian.oriented_curve(), p3(center), v3(axis))));
        // Outward normal at the patch centre.
        let probe = p3(center + dir * radius);
        use mt::{ParametricSurface3D, SearchParameter};
        if let Some((u, v)) = surface.search_parameter(probe, mt::SPHint2D::None, 100) {
            let n = surface.normal(u, v);
            if mt::InnerSpace::dot(n, v3(dir)) < 0.0 {
                mt::Invertible::invert(&mut surface);
            }
        }
        out.push(mt::Face::new(vec![wire.into()], surface));
    }
    Ok(out)
}
