//! Frame members (#4003): one body per straight line of a frame sketch, its cross-section a
//! profile laid square to the line (`frame::member_axes`) and run from one end to the other.

use solidcraft_geom::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_geom::{Aabb2, Ring};
use solidcraft_kernel as kernel;
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Kind, Value};
use crate::frame::{self, MAX_MEMBERS, MemberGroup, MemberProfile};
use crate::{DocError, Document, Feature, FeatureKind, ProfileSel, Result};

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;
/// Shortest member (mm).
const MIN_LENGTH: f64 = 1e-6;
/// How far (mm) a 3D sketch curve may stray from the line between its ends and still be a line.
const STRAIGHT_TOL: f64 = 1e-3;

/// Evaluate a `FrameMember` feature: one new body per line, group by group, each placed as its
/// group says and named ([`member_names`], or by position for a copy whose `body_names` were set).
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { path_sketch, profile, names, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    // The frame sketch in this feature's component frame: its plane, and its 3D lines.
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let groups = frame::member_groups(&f.kind);
    let lines = group_lines(mapped.as_ref(), &groups)?;
    let (section, description) = profile_regions(vals, st, profile)?;
    let mut made = Vec::new();
    let mut ids = Vec::new();
    let mut bases = Vec::new();
    for (group, segments) in groups.iter().zip(lines) {
        let regions: Cow<[Region2]> = if group.mirror { Cow::Owned(section.iter().map(frame::mirrored).collect()) } else { Cow::Borrowed(&section) };
        let at = frame::anchor_point(&regions, group.anchor);
        let turn = match &group.angle {
            Some(e) => val(vals, e, Kind::Angle)?,
            None => 0.0,
        };
        let base = group.body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&description);
        for (id, a, b) in segments {
            let (x, y) = frame::member_axes(b - a, turn).ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?;
            let plane = Plane::new(a - x * at.x - y * at.y, x, y).ok_or_else(|| DocError::Invalid(format!("line `{id}`: no plane square to it")))?;
            let mut bodies = kernel::extrude(&plane, &regions, 0.0, a.dist(b))?;
            let body = match (bodies.pop(), bodies.is_empty()) {
                (Some(body), true) => body,
                _ => return Err(DocError::Invalid(format!("the member on line `{id}` is not one solid"))),
            };
            made.push(crate::lineage::numbered_tool(body));
            ids.push(id);
            bases.push(base.to_string());
        }
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
    for (name, t) in group_member_names(st, &ids, &bases, names).into_iter().zip(made) {
        st.bodies.push(ModelBody::traced_new(name, t, f.id));
    }
    Ok(())
}

/// The body name of the member on each of `lines`: its own name from `pinned` (by line id) when no
/// other body has it, else the first free one of `base`, `base (2)`, `base (3)`… that no line has
/// pinned. A member therefore keeps its name when other lines are added, deleted or reordered.
pub fn member_names(st: &ModelState, lines: &[String], pinned: &BTreeMap<String, String>, base: &str) -> Vec<String> {
    group_member_names(st, lines, &vec![base.to_string(); lines.len()], pinned)
}

/// [`member_names`] with a base of its own for each line (its group's), the numbers counting on
/// per base across the groups.
pub fn group_member_names(st: &ModelState, lines: &[String], bases: &[String], pinned: &BTreeMap<String, String>) -> Vec<String> {
    let reserved: HashSet<&str> = pinned.values().map(String::as_str).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<Option<String>> = lines
        .iter()
        .map(|line| {
            let n = pinned.get(line).map(|n| n.trim()).filter(|n| !n.is_empty() && st.body(n).is_none())?;
            used.insert(n.to_string()).then(|| n.to_string())
        })
        .collect();
    // Each candidate is refused at most once per body, used or reserved name: past that many,
    // a free one has been found for every member.
    let limit = st.bodies.len() + reserved.len() + lines.len() + 2;
    let mut next: HashMap<&str, usize> = HashMap::new();
    for (slot, base) in out.iter_mut().zip(bases.iter().map(|b| if b.trim().is_empty() { "Member" } else { b.trim() })) {
        if slot.is_some() {
            continue;
        }
        let next = next.entry(base).or_insert(1);
        while *next <= limit {
            let candidate = if *next == 1 { base.to_string() } else { format!("{base} ({next})") };
            *next += 1;
            if st.body(&candidate).is_none() && !used.contains(&candidate) && !reserved.contains(candidate.as_str()) {
                used.insert(candidate.clone());
                *slot = Some(candidate);
                break;
            }
        }
    }
    let base = |k: usize| bases.get(k).map(|b| b.trim()).filter(|b| !b.is_empty()).unwrap_or("Member").to_string();
    out.into_iter().enumerate().map(|(k, n)| n.unwrap_or_else(|| base(k))).collect()
}

/// The ids of a `FrameMember` feature's lines in `st`, in the order its members are made.
pub fn member_line_ids(st: &ModelState, kind: &FeatureKind) -> Result<Vec<String>> {
    let FeatureKind::FrameMember { path_sketch, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch}")))?;
    Ok(group_lines(ps, &frame::member_groups(kind))?.into_iter().flatten().map(|(id, _, _)| id).collect())
}

/// Each group's member lines ([`member_lines`]). A feature's single group may leave its lines out
/// (every line); of several, each lists its own, no line in two of them.
fn group_lines(ss: &SolvedSketch, groups: &[MemberGroup]) -> Result<Vec<Vec<(String, Vec3, Vec3)>>> {
    if let [only] = groups {
        return Ok(vec![member_lines(ss, &only.lines)?]);
    }
    if groups.iter().map(|g| g.lines.len()).sum::<usize>() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature")));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(groups.len());
    for (k, g) in groups.iter().enumerate() {
        if g.lines.is_empty() {
            return Err(DocError::Invalid(format!("group {} has no lines", k + 1)));
        }
        if let Some(twice) = g.lines.iter().find(|l| !seen.insert(l.as_str())) {
            return Err(DocError::Invalid(format!("line `{twice}` is listed twice")));
        }
        out.push(member_lines(ss, &g.lines)?);
    }
    Ok(out)
}

/// The member lines of the frame sketch, each as (curve id, start, end) in its frame: the lines
/// asked for, or every straight line drawn in it (not construction, not projected or otherwise
/// linked, and with a length).
fn member_lines(ss: &SolvedSketch, ids: &[String]) -> Result<Vec<(String, Vec3, Vec3)>> {
    let sk = &ss.sketch;
    let mut out = Vec::new();
    if ids.is_empty() {
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
            if let Some((a, b)) = straight(&w.pts)
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
        return Ok(out);
    }
    if ids.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature")));
    }
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(DocError::Invalid(format!("line `{id}` is listed twice")));
        }
        let line = if let Some(wi) = sk.wire_index(id) {
            sk.wires.get(wi).and_then(|w| straight(&w.pts))
        } else {
            let ci = sk.curve_index(id).ok_or_else(|| DocError::Unknown(format!("line `{id}` in {}", ss.name)))?;
            planar_line(ss, ci)
        };
        let (a, b) = line.ok_or_else(|| DocError::Invalid(format!("`{id}` is not a straight line (members run along lines)")))?;
        if !(a.dist(b) > MIN_LENGTH) {
            return Err(DocError::Invalid(format!("line `{id}` has no length")));
        }
        out.push((id.clone(), a, b));
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
