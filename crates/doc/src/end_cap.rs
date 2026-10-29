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

/// `outline`, a counter-clockwise convex loop of lines and arcs, moved in by `d` all round:
/// lines move along their inward normal, arcs lose `d` of radius, and an arc that shrinks away
/// leaves its neighbouring lines meeting at a corner. Arcs must meet their neighbours
/// tangentially; lines may meet at corners.
pub(crate) fn inset(outline: &Loop2, d: f64) -> Result<Loop2> {
    let no_plate = || invalid(format!("an inset of {} mm leaves no plate inside the profile's outline", crate::frame::short(d)));
    let segs = &outline.segs;
    let n = segs.len();
    if n == 0 || outline.signed_area() <= 0.0 {
        return Err(invalid("the profile's outline has no area"));
    }
    // A full circle (arcs on one circle): its radius shrinks.
    if let Some(Seg2::Arc { center, radius, .. }) = segs.first().copied()
        && segs.iter().all(|s| matches!(*s, Seg2::Arc { center: c, radius: r, sweep, .. } if c.dist(center) < 1e-9 && (r - radius).abs() < 1e-9 && sweep > 0.0))
    {
        return if radius - d > 1e-6 { Ok(Loop2::circle(center, radius - d)) } else { Err(no_plate()) };
    }
    for s in segs {
        match *s {
            Seg2::Line { a, b } if a.dist(b) > 1e-12 => {}
            Seg2::Arc { sweep, .. } if sweep > 0.0 => {}
            Seg2::Line { .. } => return Err(invalid("the profile's outline has an edge of no length")),
            Seg2::Arc { .. } => return Err(not_convex()),
            _ => return Err(invalid("end caps follow outlines of lines and arcs (a spline or conic outline is not supported)")),
        }
    }
    // Each junction: tangent (the new point is the old one moved in), or a convex corner between
    // two lines.
    let mut tangent = vec![false; n];
    for (i, flag) in tangent.iter_mut().enumerate() {
        let (Some(prev), Some(this)) = (segs.get((i + n - 1) % n), segs.get(i)) else { return Err(no_plate()) };
        let (tin, tout) = (prev.end_tangent(), this.start_tangent());
        let turn = tin.cross(tout);
        if turn.abs() < 1e-7 && tin.dot(tout) > 0.0 {
            *flag = true;
        } else if turn < 0.0 {
            return Err(not_convex());
        } else if !matches!((prev, this), (Seg2::Line { .. }, Seg2::Line { .. })) {
            return Err(invalid("end caps follow outlines whose arcs meet their neighbours smoothly"));
        }
    }
    // Arcs that shrink away.
    let gone: Vec<bool> = segs.iter().map(|s| matches!(*s, Seg2::Arc { radius, .. } if radius - d <= 1e-6)).collect();
    let kept: Vec<usize> = (0..n).filter(|i| !gone.get(*i).copied().unwrap_or(true)).collect();
    if kept.is_empty() {
        return Err(no_plate());
    }
    let moved_line = |s: &Seg2| match *s {
        Seg2::Line { a, b } => {
            let t = (b - a).normalized()?;
            Some((a + t.perp() * d, t))
        }
        _ => None,
    };
    // Where kept segment `k` starts: after its tangent predecessor, the old point moved in;
    // otherwise (a corner, or arcs shrunk away between) where its line meets the previous kept
    // line.
    let m = kept.len();
    let mut starts = Vec::with_capacity(m);
    for (j, &k) in kept.iter().enumerate() {
        let p = kept.get((j + m - 1) % m).copied().ok_or_else(no_plate)?;
        let this = segs.get(k).ok_or_else(no_plate)?;
        let adjacent = (p + 1) % n == k;
        if adjacent && tangent.get(k).copied().unwrap_or(false) {
            starts.push(this.start() + this.start_tangent().perp() * d);
            continue;
        }
        let (Some((p1, t1)), Some((p2, t2))) = (segs.get(p).and_then(moved_line), moved_line(this)) else { return Err(no_plate()) };
        let den = t1.cross(t2);
        if den.abs() < 1e-9 {
            return Err(no_plate());
        }
        starts.push(p1 + t1 * ((p2 - p1).cross(t2) / den));
    }
    let mut out = Vec::with_capacity(m);
    for (j, &k) in kept.iter().enumerate() {
        let (Some(&a), Some(&b)) = (starts.get(j), starts.get((j + 1) % m)) else { return Err(no_plate()) };
        match segs.get(k).copied() {
            Some(Seg2::Line { a: oa, b: ob }) => {
                if (b - a).dot(ob - oa) <= 1e-6 * oa.dist(ob) || a.dist(b) < 1e-6 {
                    return Err(no_plate());
                }
                out.push(Seg2::Line { a, b });
            }
            Some(Seg2::Arc { center, radius, start, sweep }) => out.push(Seg2::Arc { center, radius: radius - d, start, sweep }),
            _ => return Err(no_plate()),
        }
    }
    let lp = Loop2 { segs: out };
    if !(lp.signed_area() > 1e-6) {
        return Err(no_plate());
    }
    Ok(lp)
}

fn not_convex() -> DocError {
    invalid("end caps follow convex outlines (round and rectangular tubes and bars); this profile's outline turns inward")
}

/// `outline` (as [`inset`] makes it) with every corner cut by a chamfer `c` long along each
/// edge, measured from where the edges meet. A rounded corner is cut away whole, so `c` must be
/// at least as long as its rounding.
pub(crate) fn chamfered(outline: &Loop2, c: f64) -> Result<Loop2> {
    if c <= 0.0 {
        return Ok(outline.clone());
    }
    let lines: Vec<(Vec2, Vec2)> = outline.segs.iter().filter_map(|s| if let Seg2::Line { a, b } = *s { Some((a, b)) } else { None }).collect();
    if lines.len() < 3 {
        return Err(invalid("a round end cap has no corners to chamfer (set the chamfer to 0)"));
    }
    for (i, s) in outline.segs.iter().enumerate() {
        let n = outline.segs.len();
        if matches!(s, Seg2::Arc { .. }) && !(matches!(outline.segs.get((i + n - 1) % n), Some(Seg2::Line { .. })) && matches!(outline.segs.get((i + 1) % n), Some(Seg2::Line { .. }))) {
            return Err(invalid("chamfered end caps need straight edges between their corners"));
        }
    }
    // Each line's own corner at its end: where it meets the next line.
    let k = lines.len();
    let mut corners = Vec::with_capacity(k);
    for (i, &(a1, b1)) in lines.iter().enumerate() {
        let (a2, b2) = lines.get((i + 1) % k).copied().ok_or_else(|| invalid("end cap outline"))?;
        let (t1, t2) = ((b1 - a1).normalized(), (b2 - a2).normalized());
        let (Some(t1), Some(t2)) = (t1, t2) else { return Err(invalid("the end cap's outline has an edge of no length")) };
        let den = t1.cross(t2);
        if den.abs() < 1e-9 {
            return Err(invalid("the end cap's outline has parallel neighbouring edges"));
        }
        let v = a1 + t1 * ((a2 - a1).cross(t2) / den);
        let setback = v.dist(b1).max(v.dist(a2));
        if c + 1e-9 < setback {
            return Err(invalid(format!(
                "the chamfer must be at least the corner rounding it cuts away ({} mm), or 0",
                crate::frame::short(setback)
            )));
        }
        corners.push((v, t1, t2));
    }
    let mut pts = Vec::with_capacity(2 * k);
    for (i, &(v, t1, t2)) in corners.iter().enumerate() {
        let (prev, _, _) = corners.get((i + k - 1) % k).copied().ok_or_else(|| invalid("end cap outline"))?;
        if v.dist(prev) <= 2.0 * c + 1e-6 {
            return Err(invalid("the chamfer is too large for the end cap's edges"));
        }
        pts.push(v - t1 * c);
        pts.push(v + t2 * c);
    }
    let lp = Loop2::polygon(&pts);
    if !(lp.signed_area() > 1e-6) {
        return Err(invalid("the chamfer leaves no plate"));
    }
    Ok(lp)
}

/// A free body name for the cap on `member`: `<member> End Cap`, then `… (2)`, `… (3)`…
fn cap_name(st: &mut ModelState, f: &Feature, k: usize, member: &str) -> String {
    if f.body_names.get(k).is_some_and(|n| !n.is_empty()) {
        return crate::eval::new_name(st, f, k);
    }
    let base = format!("{member} End Cap");
    (1..=st.bodies.len() + 2)
        .map(|i| if i == 1 { base.clone() } else { format!("{base} ({i})") })
        .find(|n| st.body(n).is_none())
        .unwrap_or(base)
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
        let PlaneRef::Face { anchor: Some(anchor), .. } = &end.plane else { return Err(invalid("an end cap needs a planar face picked on a frame member")) };
        if member(doc, st, &end.plane)? != end.member {
            return Err(invalid("the capped face is no longer on the member it was picked on"));
        }
        let face = doc.resolve_face_anchor(anchor, f.component, st).map_err(|e| invalid(format!("the capped end face: {e}")))?;
        let body = st.body(&anchor.target.body).ok_or_else(|| invalid("the capped member's body no longer exists"))?;
        let source = doc.feature(end.member.feature.0).ok_or_else(|| invalid("the capped member's feature no longer exists"))?;
        let (section, len, region) = crate::eval::frame_member::member_section(doc, vals, source, st, &end.member.line)?;
        let to_author = anchor.source_to_author(doc, f.component, body)?;
        let section = Plane::new(crate::apply_point(&to_author, section.origin), crate::apply_vector(&to_author, section.x), crate::apply_vector(&to_author, section.y))
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
            let cut = kernel::boolean(&mb.body, &tool.body, BoolOp::Cut)?.ok_or_else(|| invalid("cutting the member back for an inward end cap left nothing"))?;
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
