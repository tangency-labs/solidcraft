//! Frame members (#4003): separate sketch curves, or one continuous tangent line/arc path.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use solidcraft_geom::{Aabb2, Plane, Region2, Ring, Seg2, Vec2, Vec3};
use solidcraft_kernel as kernel;
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Kind, Value};
use crate::frame::{self, MAX_MEMBERS, MemberProfile};
use crate::{DocError, Document, Feature, FeatureKind, ProfileSel, Result};

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;
/// Shortest member (mm).
const MIN_LENGTH: f64 = 1e-6;
/// How far (mm) a 3D sketch curve may stray from the line between its ends and still be a line.
const STRAIGHT_TOL: f64 = 1e-3;

/// Evaluate a `FrameMember` feature: one body per selected curve, or one for a continuous path.
/// Bodies use [`member_names`], or positional names for a copy whose `body_names` were set.
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { path_sketch, lines, continuous, profile, anchor, angle, mirror, body_name, names } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    // The frame sketch in this feature's component frame: its plane, and its 3D lines.
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let paths = member_paths(mapped.as_ref(), lines, *continuous)?;
    let (mut regions, description) = profile_regions(vals, st, profile)?;
    if *mirror {
        regions = regions.iter().map(frame::mirrored).collect();
    }
    let at = frame::anchor_point(&regions, *anchor);
    let turn = match angle {
        Some(e) => val(vals, e, Kind::Angle)?,
        None => 0.0,
    };
    let mut made = Vec::with_capacity(paths.len());
    for path in &paths {
        let first = path.first().ok_or_else(|| DocError::Invalid("a member path is empty".into()))?;
        let (x, y) = frame::member_axes(first.t0, turn).ok_or_else(|| DocError::Invalid("the member has no start direction".into()))?;
        let plane = Plane::new(first.a - x * at.x - y * at.y, x, y).ok_or_else(|| DocError::Invalid("no plane square to the member".into()))?;
        let body = if let [Piece { seg: kernel::PathSeg::Line { a, b }, .. }] = path.as_slice() {
            let mut bodies = kernel::extrude(&plane, &regions, 0.0, a.dist(*b))?;
            match (bodies.pop(), bodies.is_empty()) {
                (Some(body), true) => body,
                _ => return Err(DocError::Invalid("the member is not one solid".into())),
            }
        } else {
            let region = regions.first().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
            check_bend_clearance(path, region, at, x, y)?;
            kernel::sweep(&plane, region, &path.iter().map(|p| p.seg).collect::<Vec<_>>())?
        };
        made.push(crate::lineage::numbered_tool(body));
    }
    if !f.body_names.is_empty() {
        // A copy (pasted, or in a copied component) names its members by position, as any
        // feature's bodies.
        for (k, t) in made.into_iter().enumerate() {
            let name = new_name(st, f, k);
            st.bodies.push(ModelBody::traced_new(name, t, f.id));
        }
        return Ok(());
    }
    let base = body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or(description, str::to_string);
    let ids = path_keys(&paths, *continuous);
    for (name, t) in member_names(st, &ids, names, &base).into_iter().zip(made) {
        st.bodies.push(ModelBody::traced_new(name, t, f.id));
    }
    Ok(())
}

/// The body name of the member on each of `lines`: its own name from `pinned` (by line id) when no
/// other body has it, else the first free one of `base`, `base (2)`, `base (3)`… that no line has
/// pinned. A member therefore keeps its name when other lines are added, deleted or reordered.
pub fn member_names(st: &ModelState, lines: &[String], pinned: &BTreeMap<String, String>, base: &str) -> Vec<String> {
    let reserved: HashSet<&str> = pinned.values().map(String::as_str).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<Option<String>> = lines
        .iter()
        .map(|line| {
            let n = pinned.get(line).map(|n| n.trim()).filter(|n| !n.is_empty() && st.body(n).is_none())?;
            used.insert(n.to_string()).then(|| n.to_string())
        })
        .collect();
    let base = if base.trim().is_empty() { "Member" } else { base.trim() };
    // Each candidate is refused at most once per body, used or reserved name: past that many,
    // a free one has been found for every member.
    let limit = st.bodies.len() + reserved.len() + lines.len() + 2;
    let mut next = 1usize;
    for slot in out.iter_mut().filter(|n| n.is_none()) {
        while next <= limit {
            let candidate = if next == 1 { base.to_string() } else { format!("{base} ({next})") };
            next += 1;
            if st.body(&candidate).is_none() && !used.contains(&candidate) && !reserved.contains(candidate.as_str()) {
                used.insert(candidate.clone());
                *slot = Some(candidate);
                break;
            }
        }
    }
    out.into_iter().map(|n| n.unwrap_or_else(|| base.to_string())).collect()
}

/// Body-name keys for a `FrameMember` feature in `st`, in the order its members are made:
/// curve ids for separate members, or `$path` for one continuous member.
pub fn member_line_ids(st: &ModelState, kind: &FeatureKind) -> Result<Vec<String>> {
    let FeatureKind::FrameMember { path_sketch, lines, continuous, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch}")))?;
    Ok(path_keys(&member_paths(ps, lines, *continuous)?, *continuous))
}

/// A continuous member belongs to its feature, independently of traversal order or which
/// end segment is selected first. Keep using per-curve keys for separate members.
fn path_keys(paths: &[Vec<Piece>], continuous: bool) -> Vec<String> {
    if continuous { vec!["$path".into()] } else { paths.iter().filter_map(|p| p.first().map(|p| p.id.clone())).collect() }
}

/// One exact segment, retaining analytic end tangents even for spatial arcs.
struct Piece {
    id: String,
    a: Vec3,
    b: Vec3,
    t0: Vec3,
    t1: Vec3,
    seg: kernel::PathSeg,
}

impl Piece {
    fn line(id: &str, a: Vec3, b: Vec3) -> Result<Self> {
        let t = (b - a).normalized().filter(|_| a.dist(b) > MIN_LENGTH);
        let t = t.ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?;
        if !point_ok(a) || !point_ok(b) {
            return Err(DocError::Invalid(format!("line `{id}` has invalid coordinates")));
        }
        Ok(Self { id: id.into(), a, b, t0: t, t1: t, seg: kernel::PathSeg::Line { a, b } })
    }

    fn arc(id: &str, a: Vec3, b: Vec3, center: Vec3, axis: Vec3, angle: f64) -> Result<Self> {
        let k = axis.normalized().ok_or_else(|| DocError::Invalid(format!("arc `{id}` has no normal")))?;
        let radius = a.dist(center);
        if !point_ok(a)
            || !point_ok(b)
            || !point_ok(center)
            || !radius.is_finite()
            || radius <= MIN_LENGTH
            || !angle.is_finite()
            || angle <= MIN_LENGTH
            || angle >= std::f64::consts::TAU - 1e-9
            || (b.dist(center) - radius).abs() > 1e-6 * radius.max(1.0)
            || (a - center).dot(k).abs() > 1e-6
            || (b - center).dot(k).abs() > 1e-6
            || rotate(a - center, k, angle).dist(b - center) > 1e-6 * radius.max(1.0)
        {
            return Err(DocError::Invalid(format!("arc `{id}` needs a finite, open circular path")));
        }
        let t0 = k.cross(a - center).normalized().ok_or_else(|| DocError::Invalid(format!("arc `{id}` has no tangent")))?;
        let t1 = k.cross(b - center).normalized().ok_or_else(|| DocError::Invalid(format!("arc `{id}` has no tangent")))?;
        Ok(Self { id: id.into(), a, b, t0, t1, seg: kernel::PathSeg::Arc { a, center, axis: k, angle } })
    }

    fn reverse(&mut self) {
        self.seg = match self.seg {
            kernel::PathSeg::Line { a, b } => kernel::PathSeg::Line { a: b, b: a },
            kernel::PathSeg::Arc { center, axis, angle, .. } => kernel::PathSeg::Arc { a: self.b, center, axis: -axis, angle },
        };
        std::mem::swap(&mut self.a, &mut self.b);
        let old = self.t0;
        self.t0 = -self.t1;
        self.t1 = -old;
    }
}

fn point_ok(p: Vec3) -> bool {
    p.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 1e9
}

fn rotate(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

fn curve_piece(ss: &SolvedSketch, id: &str) -> Result<Piece> {
    let sk = &ss.sketch;
    if let Some(w) = sk.wire_index(id).and_then(|i| sk.wires.get(i)) {
        if let Some(arc) = w.arc {
            if !arc.radius.is_finite() || (arc.normal.len() - 1.0).abs() > 1e-6 || (arc.start.dist(arc.center) - arc.radius).abs() > 1e-6 {
                return Err(DocError::Invalid(format!("arc `{id}` has invalid circle data")));
            }
            let u = arc.start - arc.center;
            let v = arc.end - arc.center;
            let angle = arc.normal.dot(u.cross(v)).atan2(u.dot(v)).rem_euclid(std::f64::consts::TAU);
            return Piece::arc(id, arc.start, arc.end, arc.center, arc.normal, angle);
        }
        if w.pts.len() <= solidcraft_sketch::MAX_WIRE_POINTS
            && w.pts.iter().copied().all(point_ok)
            && let Some((a, b)) = straight(&w.pts)
        {
            return Piece::line(id, a, b);
        }
    } else if let Some(ci) = sk.curve_index(id) {
        if !sk.curves.get(ci).is_some_and(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. })) {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc")));
        }
        return match sk.segs(ci).as_slice() {
            [Seg2::Line { a, b }] => Piece::line(id, ss.plane.to_world(*a), ss.plane.to_world(*b)),
            [s @ Seg2::Arc { center, sweep, .. }] => Piece::arc(
                id,
                ss.plane.to_world(s.start()),
                ss.plane.to_world(s.end()),
                ss.plane.to_world(*center),
                ss.plane.normal() * sweep.signum(),
                sweep.abs(),
            ),
            _ => Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc"))),
        };
    } else {
        return Err(DocError::Unknown(format!("path curve `{id}` in {}", ss.name)));
    }
    Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an exact circular arc")))
}

fn member_paths(ss: &SolvedSketch, ids: &[String], continuous: bool) -> Result<Vec<Vec<Piece>>> {
    // Preserve the existing meaning of an unselected feature, including on older designs.
    if ids.is_empty() {
        if continuous {
            return Err(DocError::Invalid("a continuous member needs explicit `lines` in path order".into()));
        }
        return default_member_lines(ss)?.into_iter().map(|(id, a, b)| Ok(vec![Piece::line(&id, a, b)?])).collect();
    }
    if ids.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} curves in one member feature")));
    }
    let mut seen = HashSet::new();
    let mut pieces = Vec::with_capacity(ids.len());
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(DocError::Invalid(format!("curve `{id}` is listed twice")));
        }
        pieces.push(curve_piece(ss, id)?);
    }
    if !continuous {
        return Ok(pieces.into_iter().map(|p| vec![p]).collect());
    }
    if let (Some(first), Some(next)) = (pieces.first(), pieces.get(1)) {
        let flip = first.a.dist(next.a).min(first.a.dist(next.b)) < first.b.dist(next.a).min(first.b.dist(next.b));
        if flip && let Some(first) = pieces.first_mut() {
            first.reverse();
        }
    }
    let mut end = None;
    for p in &mut pieces {
        if let Some((b, tangent)) = end {
            if p.b.dist(b) < p.a.dist(b) {
                p.reverse();
            }
            if p.a.dist(b) > 1e-6 {
                return Err(DocError::Invalid(format!("continuous member: `{}` is not connected end to end in path order", p.id)));
            }
            if p.t0.dot(tangent) < (1e-6_f64).cos() {
                return Err(DocError::Invalid(format!("continuous member: `{}` is not tangent to the previous curve; draw a bend arc", p.id)));
            }
        }
        end = Some((p.b, p.t1));
    }
    if pieces.first().zip(pieces.last()).is_some_and(|(a, b)| a.a.dist(b.b) <= 1e-6) {
        return Err(DocError::Invalid("not supported yet: a continuous member must have two open ends".into()));
    }
    Ok(vec![pieces])
}

/// Refuse a section crossing a bend axis. Transport the section axes exactly as the sweep
/// does; an anchored or rotated rectangular section need not be symmetric about the path.
fn check_bend_clearance(path: &[Piece], region: &Region2, at: Vec2, mut x: Vec3, mut y: Vec3) -> Result<()> {
    let bb = region.outer.polyline(1e-3).bounds();
    let corners = [bb.min, Vec2::new(bb.max.x, bb.min.y), bb.max, Vec2::new(bb.min.x, bb.max.y)];
    for p in path {
        if let kernel::PathSeg::Arc { center, axis, angle, .. } = p.seg {
            let radial = (p.a - center).normalized().ok_or_else(|| DocError::Invalid("the bend has no radius".into()))?;
            let inward = corners.iter().map(|q| (x * (q.x - at.x) + y * (q.y - at.y)).dot(radial)).fold(0.0_f64, f64::min);
            // The sampled box can undershoot a curved profile by the chord tolerance.
            if p.a.dist(center) + inward <= MIN_LENGTH + 2e-3 {
                return Err(DocError::Invalid(format!("arc `{}`: the bend radius is too small for the anchored profile", p.id)));
            }
            x = rotate(x, axis, angle);
            y = rotate(y, axis, angle);
        }
    }
    Ok(())
}

/// The unchanged all-lines default: drawn straight lines with a length, in the sketch's frame.
fn default_member_lines(ss: &SolvedSketch) -> Result<Vec<(String, Vec3, Vec3)>> {
    let sk = &ss.sketch;
    let mut out = Vec::new();
    for (ci, c) in sk.curves.iter().enumerate() {
        if !c.construction
            && !c.centerline
            && c.link.is_none()
            && matches!(c.kind, CurveKind::Line { .. })
            && let Some((a, b)) = planar_line(ss, ci)
            && a.dist(b) > MIN_LENGTH
        {
            out.push((c.id.clone(), a, b));
        }
    }
    for w in sk.wires.iter().filter(|w| w.link.is_none()) {
        if w.pts.len() <= solidcraft_sketch::MAX_WIRE_POINTS
            && let Some((a, b)) = straight(&w.pts)
            && a.dist(b) > MIN_LENGTH
        {
            out.push((w.id.clone(), a, b));
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid(format!("{} has no lines for members", ss.name)));
    }
    if out.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature (pick the lines)")));
    }
    Ok(out)
}

/// A sketch line's ends in the sketch's frame.
fn planar_line(ss: &SolvedSketch, ci: usize) -> Option<(Vec3, Vec3)> {
    let CurveKind::Line { a, b } = ss.sketch.curves.get(ci)?.kind else { return None };
    let (pa, pb) = (ss.sketch.points.get(a)?.pos, ss.sketch.points.get(b)?.pos);
    Some((ss.plane.to_world(pa), ss.plane.to_world(pb)))
}

/// The ends of a 3D polyline that runs straight from its first point to its last: every point
/// within [`STRAIGHT_TOL`] of the line between them, none behind the start, past the end or back
/// from a point before it.
fn straight(pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let (a, b) = (*pts.first()?, *pts.last()?);
    let (len, d) = (a.dist(b), (b - a).normalized()?);
    let mut reached = 0.0_f64;
    for p in pts {
        let q = *p - a;
        let t = q.dot(d);
        if (q - d * t).len() > STRAIGHT_TOL || t < reached - STRAIGHT_TOL || t > len + STRAIGHT_TOL {
            return None;
        }
        reached = reached.max(t);
    }
    Some((a, b))
}

/// The regions of a member's profile, in the profile's own coordinates: one piece of material,
/// holes and all; and what its members are named after when they have no name of their own.
fn profile_regions(vals: &BTreeMap<String, Value>, st: &ModelState, profile: &MemberProfile) -> Result<(Vec<Region2>, String)> {
    let (regions, description) = match profile {
        MemberProfile::Library { profile } => {
            let p = profile.resolve(|name, e| val(vals, e, Kind::Length).map_err(|err| DocError::Invalid(format!("profile `{name}`: {err}"))))?;
            (vec![p.region()?], p.description())
        }
        MemberProfile::Sketch { sketch, profiles } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            let picked = match profiles {
                ProfileSel::All => material(ss)?,
                sel => select_profiles(ss, sel)?,
            };
            (solidcraft_sketch::merge_regions(&picked), ss.name.clone())
        }
    };
    match regions.len() {
        1 => Ok((regions, description)),
        0 => Err(DocError::Invalid("the member profile has no area".into())),
        n => Err(DocError::Invalid(format!("the member profile is {n} separate pieces (pick one with `profiles`)"))),
    }
}

/// The material of a profile sketch: of what was drawn (as `solid.extrude` takes all profiles:
/// projected reference geometry only when the drawn curves make no profile, letters without
/// their counters filled in), each profile inside an even number of other profiles' outlines,
/// so nested loops alternate between material and hole.
fn material(ss: &SolvedSketch) -> Result<Vec<Region2>> {
    let drawn = solidcraft_sketch::find_drawn_profiles(&ss.sketch);
    let ps = solidcraft_sketch::without_text_counters(&ss.sketch, if drawn.is_empty() { ss.profiles.clone() } else { drawn });
    if ps.is_empty() {
        return Err(DocError::Invalid(format!("{} has no closed profiles", ss.name)));
    }
    if ps.len() > MAX_PROFILE_PIECES {
        return Err(DocError::Invalid(format!("{} has more than {MAX_PROFILE_PIECES} profiles (pick the member's with `profiles`)", ss.name)));
    }
    // Outer polylines and their boxes, once.
    let polys: Vec<Ring<Vec2>> = ps.iter().map(|p| p.region.outer.polyline(1e-3)).collect();
    let boxes: Vec<Aabb2> = polys.iter().map(|poly| poly.bounds()).collect();
    Ok(ps
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            let at = p.region.interior_point();
            let depth = polys.iter().zip(&boxes).enumerate().filter(|(j, (poly, bb))| j != i && bb.contains(at) && poly.contains(at)).count();
            depth % 2 == 0
        })
        .map(|(_, p)| p.region.clone())
        .collect())
}
