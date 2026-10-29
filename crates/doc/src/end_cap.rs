//! End caps (#4003): a plate closing the open end of a frame member, its outline the member
//! profile's outer outline set in from the edge, built from existing extrude and boolean
//! operations.
use std::collections::BTreeMap;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Feature, FeatureKind, ModelBody, ModelState, PlaneRef, Result};
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
use solidcraft_kernel::{self as kernel, BoolOp};

/// Most end faces one end cap feature closes.
pub const MAX_FACES: usize = 256;
/// The default inset as a share of the thickness.
pub const DEFAULT_RATIO: f64 = 0.5;

/// The member a capped face belongs to: its feature and line, which outlive body renames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub feature: crate::FeatureId,
    pub line: String,
}

/// One capped end: the member's planar end face and the member it belongs to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct End {
    pub plane: PlaneRef,
    pub member: Member,
}

/// Which way the cap goes from the end face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Past the end: the frame gets longer by the thickness.
    #[default]
    Outward,
    /// Into the member, which is cut back by the thickness: the frame keeps its length.
    Inward,
}

fn invalid(s: impl Into<String>) -> DocError {
    DocError::Invalid(s.into())
}

/// The member that `plane`'s face belongs to; an error when it is no frame member's face.
pub fn member(doc: &Document, st: &ModelState, plane: &PlaneRef) -> Result<Member> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = plane else { return Err(invalid("an end cap needs a planar face picked on a frame member")) };
    let source = doc.feature(anchor.target.origin_feature).ok_or_else(|| invalid("the capped member's feature no longer exists"))?;
    let FeatureKind::FrameMember { names, .. } = &source.kind else { return Err(invalid("the face is not on a frame member")) };
    let ids = crate::frame::member_line_ids(st, &source.kind)?;
    let line = if source.body_names.is_empty() {
        ids.iter().find(|line| names.get(*line).is_some_and(|name| *name == anchor.target.body))
    } else {
        source.body_names.iter().position(|name| *name == anchor.target.body).and_then(|i| ids.get(i))
    }
    .ok_or_else(|| invalid("the face is no longer on the member it was picked on"))?;
    Ok(Member { feature: source.id.into(), line: line.clone() })
}

fn length(v: f64, what: &str, min: f64) -> Result<f64> {
    if !v.is_finite() || v > 1e6 || v < min {
        return Err(invalid(format!("the end cap's {what} must be a length from {min} to 1e6 mm")));
    }
    Ok(v)
}

/// Most edges in an outline an end cap follows.
const MAX_EDGES: usize = 4096;

/// Where a segment of the outline goes when it moves in: a line through a point along a unit
/// direction, or a circle.
#[derive(Clone, Copy)]
enum Carrier {
    Line(Vec2, Vec2),
    Circle(Vec2, f64),
}

/// The points where two carriers cross.
fn crossings(c1: Carrier, c2: Carrier) -> Vec<Vec2> {
    match (c1, c2) {
        (Carrier::Line(p1, t1), Carrier::Line(p2, t2)) => {
            let den = t1.cross(t2);
            if den.abs() < 1e-9 { Vec::new() } else { vec![p1 + t1 * ((p2 - p1).cross(t2) / den)] }
        }
        (Carrier::Line(p, t), Carrier::Circle(c, r)) | (Carrier::Circle(c, r), Carrier::Line(p, t)) => {
            let foot = p + t * (c - p).dot(t);
            let h2 = r * r - (c - foot).dot(c - foot);
            if h2 < -1e-9 * r.max(1.0) {
                return Vec::new();
            }
            let h = h2.max(0.0).sqrt();
            vec![foot - t * h, foot + t * h]
        }
        (Carrier::Circle(c1, r1), Carrier::Circle(c2, r2)) => {
            let dd = c1.dist(c2);
            if dd < 1e-12 {
                return Vec::new();
            }
            let u = (c2 - c1) / dd;
            let along = (r1 * r1 - r2 * r2 + dd * dd) / (2.0 * dd);
            let h2 = r1 * r1 - along * along;
            if h2 < -1e-9 * r1.max(1.0) {
                return Vec::new();
            }
            let h = h2.max(0.0).sqrt();
            let foot = c1 + u * along;
            vec![foot - u.perp() * h, foot + u.perp() * h]
        }
    }
}

/// How far `p` is from segment `s` (a line or an arc).
fn distance(s: &Seg2, p: Vec2) -> f64 {
    match *s {
        Seg2::Line { a, b } => {
            let ab = b - a;
            let l2 = ab.dot(ab);
            let u = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
            p.dist(a + ab * u)
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let tau = std::f64::consts::TAU;
            let rel = (p - center).angle() - start;
            let u = if sweep >= 0.0 { rel.rem_euclid(tau) } else { (-rel).rem_euclid(tau) };
            if u <= sweep.abs() { (p.dist(center) - radius).abs() } else { p.dist(s.start()).min(p.dist(s.end())) }
        }
        _ => p.dist(s.start()).min(p.dist(s.end())),
    }
}

/// Points along `s`: a few on a line, more on an arc.
fn samples(s: &Seg2) -> Vec<Vec2> {
    let k = match *s {
        Seg2::Arc { sweep, .. } => ((sweep.abs() / (std::f64::consts::PI / 32.0)).ceil() as usize).clamp(2, 128),
        _ => 8,
    };
    (0..=k).map(|i| s.point_at(i as f64 / k as f64)).collect()
}

/// Whether two straight pieces cross (touching at their ends does not count).
fn pieces_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (o1, o2) = ((b - a).cross(c - a), (b - a).cross(d - a));
    let (o3, o4) = ((d - c).cross(a - c), (d - c).cross(b - c));
    let eps = 1e-12 * (1.0 + a.dist(b) + c.dist(d)).powi(2);
    o1 * o2 < -eps && o3 * o4 < -eps
}

/// Whether `lp`'s edges cross one another.
fn crosses_itself(lp: &Loop2) -> bool {
    let pts: Vec<Vec2> = lp
        .segs
        .iter()
        .flat_map(|s| {
            let mut v = samples(s);
            v.pop();
            v
        })
        .collect();
    let n = pts.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j + 1 == n {
                continue;
            }
            let (Some(&a), Some(&b), Some(&c), Some(&d)) = (pts.get(i), pts.get((i + 1) % n), pts.get(j), pts.get((j + 1) % n)) else {
                continue;
            };
            if pieces_cross(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

/// `outline`, a counter-clockwise loop of lines and arcs, moved in by `d` all round: lines move
/// along their inward normal, arcs that bulge out lose `d` of radius and arcs that curve in (the
/// root of an angle or a beam) gain it. An arc that shrinks away leaves its neighbours meeting
/// at a corner. Any shape works: round and rectangular tubes as well as angles, channels and
/// beams. An inset more than half a wall somewhere (an angle's leg, a beam's web) is refused.
pub(crate) fn inset(outline: &Loop2, d: f64) -> Result<Loop2> {
    let no_plate = || {
        invalid(format!(
            "an inset of {} mm leaves no plate inside the profile's outline (it is more than half a wall somewhere)",
            crate::frame::short(d)
        ))
    };
    if !d.is_finite() || d < 0.0 {
        return Err(invalid("the end cap's inset must be a length of 0 or more"));
    }
    let segs = &outline.segs;
    let n = segs.len();
    if n == 0 || outline.signed_area() <= 0.0 {
        return Err(invalid("the profile's outline has no area"));
    }
    if n > MAX_EDGES {
        return Err(invalid(format!("end caps follow outlines of at most {MAX_EDGES} edges")));
    }
    // A full circle (arcs on one circle): its radius shrinks.
    if let Some(Seg2::Arc { center, radius, .. }) = segs.first().copied()
        && segs
            .iter()
            .all(|s| matches!(*s, Seg2::Arc { center: c, radius: r, sweep, .. } if c.dist(center) < 1e-9 && (r - radius).abs() < 1e-9 && sweep > 0.0))
    {
        return if radius - d > 1e-6 { Ok(Loop2::circle(center, radius - d)) } else { Err(no_plate()) };
    }
    let mut carriers = Vec::with_capacity(n);
    for s in segs {
        carriers.push(match *s {
            Seg2::Line { a, b } => {
                let t = (b - a).normalized().ok_or_else(|| invalid("the profile's outline has an edge of no length"))?;
                Some(Carrier::Line(a + t.perp() * d, t))
            }
            // Counter-clockwise arcs bulge out and shrink; clockwise ones curve in and grow.
            Seg2::Arc { center, radius, sweep, .. } if sweep.abs() > 1e-12 && radius > 0.0 => {
                let r = radius - d * sweep.signum();
                (r > 1e-6).then_some(Carrier::Circle(center, r))
            }
            Seg2::Arc { .. } => return Err(invalid("the profile's outline has an arc of no length")),
            _ => return Err(invalid("end caps follow outlines of lines and arcs (a spline or conic outline is not supported)")),
        });
    }
    let kept: Vec<usize> = (0..n).filter(|i| carriers.get(*i).is_some_and(Option::is_some)).collect();
    let m = kept.len();
    if m < 2 {
        return Err(no_plate());
    }
    // Where kept segment `k` starts: after a smooth join, the old point moved in; otherwise (a
    // corner, or arcs shrunk away between) where it meets the previous kept segment, nearest the
    // corner it replaces.
    let mut starts = Vec::with_capacity(m);
    for (j, &k) in kept.iter().enumerate() {
        let p = kept.get((j + m - 1) % m).copied().ok_or_else(no_plate)?;
        let (Some(prev), Some(this)) = (segs.get(p), segs.get(k)) else { return Err(no_plate()) };
        let (tin, tout) = (prev.end_tangent(), this.start_tangent());
        if (p + 1) % n == k && tin.cross(tout).abs() < 1e-7 && tin.dot(tout) > 0.0 {
            starts.push(this.start() + tout.perp() * d);
            continue;
        }
        let (Some(Some(c1)), Some(Some(c2))) = (carriers.get(p).copied(), carriers.get(k).copied()) else { return Err(no_plate()) };
        let near = prev.end().lerp(this.start(), 0.5);
        let best = crossings(c1, c2).into_iter().min_by(|a, b| a.dist(near).total_cmp(&b.dist(near))).ok_or_else(no_plate)?;
        starts.push(best);
    }
    let mut out = Vec::with_capacity(m);
    for (j, &k) in kept.iter().enumerate() {
        let (Some(&a), Some(&b)) = (starts.get(j), starts.get((j + 1) % m)) else { return Err(no_plate()) };
        match (segs.get(k).copied(), carriers.get(k).copied()) {
            (Some(Seg2::Line { a: oa, b: ob }), _) => {
                if (b - a).dot(ob - oa) <= 1e-6 * oa.dist(ob) || a.dist(b) < 1e-6 {
                    return Err(no_plate());
                }
                out.push(Seg2::Line { a, b });
            }
            (Some(Seg2::Arc { sweep, .. }), Some(Some(Carrier::Circle(center, r)))) => {
                let tau = std::f64::consts::TAU;
                let (s, e) = ((a - center).angle(), (b - center).angle());
                let sw = if sweep > 0.0 { (e - s).rem_euclid(tau) } else { -(s - e).rem_euclid(tau) };
                // An arc turned inside out shows as one sweeping most of the way round.
                if sw.abs() < 1e-9 || sw.abs() > sweep.abs() + std::f64::consts::FRAC_PI_2 {
                    return Err(no_plate());
                }
                out.push(Seg2::Arc { center, radius: r, start: s, sweep: sw });
            }
            _ => return Err(no_plate()),
        }
    }
    let lp = Loop2 { segs: out };
    // Every point of the plate's edge is at least `d` from the profile's: closer means the inset
    // reached across a wall thinner than twice the inset.
    let tol = 1e-6 * (1.0 + d);
    let far_enough = lp.segs.iter().flat_map(samples).all(|q| segs.iter().map(|s| distance(s, q)).fold(f64::INFINITY, f64::min) >= d - tol);
    if !(lp.signed_area() > 1e-6) || !far_enough || crosses_itself(&lp) {
        return Err(no_plate());
    }
    Ok(lp)
}

/// `outline` (as [`inset`] makes it) with every outside corner cut by a chamfer `c` long along
/// each edge, measured from where the edges meet. A rounded corner is cut away whole, so `c`
/// must be at least as long as its rounding. Inside corners (the root of an angle or a beam)
/// stay as they are.
pub(crate) fn chamfered(outline: &Loop2, c: f64) -> Result<Loop2> {
    if !c.is_finite() || c < 0.0 {
        return Err(invalid("the end cap's chamfer must be a length of 0 or more"));
    }
    if c == 0.0 {
        return Ok(outline.clone());
    }
    let segs = &outline.segs;
    let n = segs.len();
    let round = || invalid("a round end cap has no corners to chamfer (set the chamfer to 0)");
    // The lines in order from the first; an outline with none is round.
    let first = segs.iter().position(|s| matches!(s, Seg2::Line { .. })).ok_or_else(round)?;
    let lines: Vec<usize> = (0..n).map(|i| (first + i) % n).filter(|i| matches!(segs.get(*i), Some(Seg2::Line { .. }))).collect();
    let k = lines.len();
    if k < 2 {
        return Err(round());
    }
    let line = |i: usize| -> Result<(Vec2, Vec2, Vec2)> {
        match lines.get(i % k).and_then(|j| segs.get(*j)).copied() {
            Some(Seg2::Line { a, b }) => Ok((a, b, (b - a).normalized().ok_or_else(|| invalid("the end cap's outline has an edge of no length"))?)),
            _ => Err(invalid("the end cap's outline lost an edge")),
        }
    };
    // The segments between line `i` and the next line.
    let between = |i: usize| -> Vec<Seg2> {
        let (Some(&a), Some(&b)) = (lines.get(i % k), lines.get((i + 1) % k)) else { return Vec::new() };
        let count = (b + n - a - 1) % n;
        (1..=count).filter_map(|o| segs.get((a + o) % n).copied()).collect()
    };
    // Each line's corner at its end, where chamfered: an outside corner, sharp or rounded by arcs
    // that bulge out.
    let mut corners = Vec::with_capacity(k);
    for i in 0..k {
        let (a1, b1, t1) = line(i)?;
        let (a2, _, t2) = line(i + 1)?;
        let den = t1.cross(t2);
        let outside = den > 1e-9 && between(i).iter().all(|s| matches!(*s, Seg2::Arc { sweep, .. } if sweep > 0.0));
        if !outside {
            corners.push(None);
            continue;
        }
        let v = a1 + t1 * ((a2 - a1).cross(t2) / den);
        let setback = v.dist(b1).max(v.dist(a2));
        if c + 1e-9 < setback {
            return Err(invalid(format!(
                "the chamfer must be at least the corner rounding it cuts away ({} mm), or 0",
                crate::frame::short(setback)
            )));
        }
        corners.push(Some((v - t1 * c, v + t2 * c)));
    }
    if corners.iter().all(Option::is_none) {
        return Err(invalid("the end cap has no outside corners to chamfer (set the chamfer to 0)"));
    }
    let too_large = || invalid("the chamfer is too large for the end cap's edges");
    let mut out = Vec::with_capacity(2 * n);
    for i in 0..k {
        let (a, b, t) = line(i)?;
        let start = match corners.get((i + k - 1) % k) {
            Some(Some((_, s))) => *s,
            _ => a,
        };
        let end = match corners.get(i) {
            Some(Some((e, _))) => *e,
            _ => b,
        };
        if (end - start).dot(t) <= 1e-6 {
            return Err(too_large());
        }
        out.push(Seg2::Line { a: start, b: end });
        match corners.get(i) {
            Some(Some((e, s))) => out.push(Seg2::Line { a: *e, b: *s }),
            _ => out.extend(between(i)),
        }
    }
    let lp = Loop2 { segs: out };
    if !(lp.signed_area() > 1e-6) || crosses_itself(&lp) {
        return Err(too_large());
    }
    Ok(lp)
}

/// A free body name for the cap on `member`: `<member> End Cap`, then `… (2)`, `… (3)`…
fn cap_name(st: &mut ModelState, f: &Feature, k: usize, member: &str) -> String {
    if f.body_names.get(k).is_some_and(|n| !n.is_empty()) {
        return crate::eval::new_name(st, f, k);
    }
    let base = format!("{member} End Cap");
    (1..=st.bodies.len() + 2).map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") }).find(|n| st.body(n).is_none()).unwrap_or(base)
}

/// A capped end, worked out before any body changes.
struct Planned {
    body: String,
    plane: Plane,
    region: Region2,
    /// The cap's extent along the plane's normal, and the slab cut from an inward cap's member.
    cap: (f64, f64),
    cut: Option<(Region2, f64, f64)>,
}

pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameEndCap { ends, thickness, offset, ratio, chamfer, direction } = &f.kind else {
        return Err(invalid("not an end cap"));
    };
    if ends.is_empty() {
        return Err(invalid("pick at least one open end face of a frame member"));
    }
    if ends.len() > MAX_FACES {
        return Err(invalid(format!("at most {MAX_FACES} end faces in one end cap feature")));
    }
    let t = length(crate::eval::val(vals, thickness, Kind::Length)?, "thickness", 0.01)?;
    let d = match (offset, ratio) {
        (Some(e), None) => length(crate::eval::val(vals, e, Kind::Length)?, "inset", 0.0)?,
        (None, Some(e)) => {
            let r = crate::eval::val(vals, e, Kind::Unitless)?;
            if !r.is_finite() || !(0.0..=1000.0).contains(&r) {
                return Err(invalid("the end cap's inset ratio must be from 0 to 1000 times the thickness"));
            }
            r * t
        }
        _ => return Err(invalid("give the end cap's inset as either a length or a ratio of the thickness")),
    };
    let c = length(crate::eval::val(vals, chamfer, Kind::Length)?, "chamfer", 0.0)?;
    let mut planned = Vec::with_capacity(ends.len());
    for (k, end) in ends.iter().enumerate() {
        if ends.iter().take(k).any(|e| e.plane == end.plane) {
            return Err(invalid("an end face is picked twice"));
        }
        let PlaneRef::Face { anchor: Some(anchor), .. } = &end.plane else {
            return Err(invalid("an end cap needs a planar face picked on a frame member"));
        };
        if member(doc, st, &end.plane)? != end.member {
            return Err(invalid("the capped face is no longer on the member it was picked on"));
        }
        let face = doc.resolve_face_anchor(anchor, f.component, st).map_err(|e| invalid(format!("the capped end face: {e}")))?;
        let body = st.body(&anchor.target.body).ok_or_else(|| invalid("the capped member's body no longer exists"))?;
        let source = doc.feature(end.member.feature.0).ok_or_else(|| invalid("the capped member's feature no longer exists"))?;
        let (section, len, region) = crate::eval::frame_member::member_section(doc, vals, source, st, &end.member.line)?;
        let to_author = anchor.source_to_author(doc, f.component, body)?;
        let section = Plane::new(
            crate::apply_point(&to_author, section.origin),
            crate::apply_vector(&to_author, section.x),
            crate::apply_vector(&to_author, section.y),
        )
        .ok_or_else(|| invalid("the member's placement is degenerate"))?;
        let along = section.normal();
        let facing = face.normal().dot(along);
        if facing.abs() < 1.0 - 1e-7 {
            return Err(invalid("the picked face is not an end of the member (it is not square to the member's line)"));
        }
        let plane = Plane::new(section.origin + along * (face.origin - section.origin).dot(along), section.x, section.y)
            .ok_or_else(|| invalid("the end face's plane is degenerate"))?;
        let outline = chamfered(&inset(&region.outer, d)?, c)?;
        let cap_region = Region2 { outer: outline, holes: Vec::new() };
        // Along the plane's normal: the member lies behind an end facing that way.
        let s = facing.signum();
        let (cap, cut) = match direction {
            Direction::Outward => (if s > 0.0 { (0.0, t) } else { (-t, 0.0) }, None),
            Direction::Inward => {
                if t >= len - 1e-3 {
                    return Err(invalid(format!("an inward end cap {} mm thick needs a longer member", crate::frame::short(t))));
                }
                // A slab wider than the profile and past the end, so no face of it lies on the
                // member's.
                let bb = region.outer.polyline(1e-3).bounds();
                let m = 1.0 + 0.01 * (bb.max - bb.min).len();
                let (lo, hi) = (bb.min - Vec2::new(m, m), bb.max + Vec2::new(m, m));
                let slab = Region2 { outer: Loop2::polygon(&[lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]), holes: Vec::new() };
                let (a, b) = if s > 0.0 { (-t, m) } else { (-m, t) };
                (if s > 0.0 { (-t, 0.0) } else { (0.0, t) }, Some((slab, a, b)))
            }
        };
        planned.push(Planned { body: anchor.target.body.clone(), plane, region: cap_region, cap, cut });
    }
    for (k, p) in planned.into_iter().enumerate() {
        if let Some((slab, a, b)) = &p.cut {
            let i = st.bodies.iter().position(|b| b.name == p.body).ok_or_else(|| invalid("the capped member's body no longer exists"))?;
            let mb = st.bodies.get(i).cloned().ok_or_else(|| invalid("the capped member's body no longer exists"))?;
            let tool = one(kernel::extrude(&p.plane, std::slice::from_ref(slab), *a, *b)?)?;
            let tool = crate::lineage::numbered_tool(tool);
            let cut = kernel::boolean(&mb.body, &tool.body, BoolOp::Cut)?
                .ok_or_else(|| invalid("cutting the member back for an inward end cap left nothing"))?;
            let cut = crate::lineage::Traced::step(cut, &[&mb.traced(), &tool]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, cut, mb.feature);
            }
        }
        let body = one(kernel::extrude(&p.plane, std::slice::from_ref(&p.region), p.cap.0, p.cap.1)?)?;
        let name = cap_name(st, f, k, &p.body);
        st.bodies.push(ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    }
    Ok(())
}

fn one(mut bodies: Vec<solidcraft_kernel::Body>) -> Result<solidcraft_kernel::Body> {
    let body = bodies.pop().ok_or_else(|| invalid("the end cap is not one solid"))?;
    if !bodies.is_empty() {
        return Err(invalid("the end cap is not one solid"));
    }
    Ok(body)
}
