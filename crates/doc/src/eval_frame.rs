//! Frame members (#4003): one body per straight line of a frame sketch, its cross-section a
//! profile laid square to the line (`frame::member_axes`) and run from one end to the other; and
//! their trims, which rebuild a member with its ends moved to the planes they meet.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use solidcraft_geom::{Aabb2, Plane, Region2, Ring, Vec2, Vec3};
use solidcraft_kernel::{self as kernel, Body};
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Kind, Value};
use crate::frame::{self, MAX_MEMBERS, MemberProfile, MemberRecord, MemberRef, Side, TrimEnd, TrimTarget};
use crate::{DocError, Document, Feature, FeatureKind, ProfileSel, Result};

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;
/// Shortest member (mm).
const MIN_LENGTH: f64 = 1e-6;
/// How far (mm) a 3D sketch curve may stray from the line between its ends and still be a line.
const STRAIGHT_TOL: f64 = 1e-3;
/// Farthest (mm) a trim may move a member's end from its line's start.
const MAX_REACH: f64 = 1e6;
/// A plane meets a member when its normal leans at least this far (the cosine) from square to the
/// member's line: a plane closer to running along the member does not cut it.
const MIN_CROSSING: f64 = 1e-6;

/// The members a `FrameMember` feature lays out, before they are built.
pub(super) struct Layout {
    pub members: Vec<Laid>,
    /// The section, in its own plane's coordinates (mirrored if the feature mirrors it).
    pub regions: Vec<Region2>,
    /// The section's bounding box in those coordinates.
    pub lo: Vec2,
    pub hi: Vec2,
    /// What members without a name of their own are named after.
    pub description: String,
}

/// One member laid out: its line and the plane of its section at the line's start.
pub(super) struct Laid {
    pub id: String,
    /// The direction of the line (a unit vector) and its length.
    pub d: Vec3,
    pub len: f64,
    /// The section's plane at the line's start (the profile's own origin at its origin), its x × y
    /// along the line.
    pub plane: Plane,
}

impl Layout {
    fn member(&self, line: &str) -> Option<&Laid> {
        self.members.iter().find(|m| m.id == line)
    }
}

impl Laid {
    /// The line through the middle of the section's bounding box: its point at the line's start,
    /// and how far along `d` from there each end of `ends` crosses it.
    fn axis(&self, lay: &Layout, ends: &[Option<Plane>; 2]) -> Result<(Vec3, f64, f64)> {
        let c0 = self.plane.to_world((lay.lo + lay.hi) * 0.5);
        let at = |end: &Option<Plane>, default: f64| -> Result<f64> {
            let Some(pl) = end else { return Ok(default) };
            let nd = pl.normal().dot(self.d);
            if !(nd.abs() >= MIN_CROSSING) {
                return Err(DocError::Invalid(format!("the end of the member on line `{}` runs along it", self.id)));
            }
            Ok(pl.normal().dot(pl.origin - c0) / nd)
        };
        Ok((c0, at(&ends[0], 0.0)?, at(&ends[1], self.len)?))
    }
}

/// Lay out a `FrameMember` feature's members: its lines in its component's frame, and its section.
pub(super) fn layout(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Layout> {
    let FeatureKind::FrameMember { path_sketch, lines, profile, anchor, angle, mirror, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    // The frame sketch in this feature's component frame: its plane, and its 3D lines.
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let segments = member_lines(mapped.as_ref(), lines)?;
    let (mut regions, description) = profile_regions(vals, st, profile)?;
    if *mirror {
        regions = regions.iter().map(frame::mirrored).collect();
    }
    let (lo, hi) = frame::profile_bounds(&regions).ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    let at = frame::anchor_point(&regions, *anchor);
    let turn = match angle {
        Some(e) => val(vals, e, Kind::Angle)?,
        None => 0.0,
    };
    let mut members = Vec::with_capacity(segments.len());
    for (id, a, b) in segments {
        let (x, y) = frame::member_axes(b - a, turn).ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?;
        let plane = Plane::new(a - x * at.x - y * at.y, x, y).ok_or_else(|| DocError::Invalid(format!("line `{id}`: no plane square to it")))?;
        let d = (b - a).normalized().ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?;
        members.push(Laid { len: a.dist(b), d, plane, id });
    }
    Ok(Layout { members, regions, lo, hi, description })
}

/// Build a member: its section run along its line, each end square at the line's end or cut in
/// its plane (`ends`, normals out of the member).
pub(super) fn build(m: &Laid, lay: &Layout, ends: &[Option<Plane>; 2]) -> Result<Body> {
    let corners = [lay.lo, Vec2::new(lay.hi.x, lay.lo.y), lay.hi, Vec2::new(lay.lo.x, lay.hi.y)].map(|c| m.plane.to_world(c));
    // How far along the line an end plane is at the section's corners (nearest, farthest), and
    // whether it slants across the section.
    let reach = |end: &Option<Plane>, default: f64, start: bool| -> Result<(f64, f64, bool)> {
        let Some(pl) = end else { return Ok((default, default, false)) };
        let n = pl.normal();
        let nd = n.dot(m.d);
        // A start's normal points back along the line, an end's on along it.
        if !(n.is_finite() && pl.origin.is_finite() && nd.abs() >= MIN_CROSSING) || (nd < 0.0) != start {
            return Err(DocError::Invalid(format!("the member on line `{}`: an end plane faces the wrong way", m.id)));
        }
        let (lo, hi) =
            corners.iter().map(|c| n.dot(pl.origin - *c) / nd).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let slants = hi - lo > 1e-9 * (1.0 + lo.abs());
        Ok(if slants { (lo, hi, true) } else { (lo, lo, false) })
    };
    let (s_lo, s_hi, s_cut) = reach(&ends[0], 0.0, true)?;
    let (e_lo, e_hi, e_cut) = reach(&ends[1], m.len, false)?;
    if ![s_lo, s_hi, e_lo, e_hi].iter().all(|t| t.is_finite() && t.abs() <= MAX_REACH) {
        return Err(DocError::Invalid(format!("the member on line `{}` would reach more than {MAX_REACH} mm", m.id)));
    }
    // The ends must leave the whole section between them.
    if !(e_lo - s_hi > MIN_LENGTH) {
        return Err(DocError::Invalid(format!("trimming leaves nothing of the member on line `{}` (its ends cross)", m.id)));
    }
    // An end cut at a slant: run the section past it, then cut it there.
    let from = if s_cut { s_lo - (s_hi - s_lo) - 1.0 } else { s_lo };
    let to = if e_cut { e_hi + (e_hi - e_lo) + 1.0 } else { e_hi };
    let mut bodies = kernel::extrude(&m.plane, &lay.regions, from, to)?;
    let mut body = match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => body,
        _ => return Err(DocError::Invalid(format!("the member on line `{}` is not one solid", m.id))),
    };
    for (end, cut) in ends.iter().zip([s_cut, e_cut]) {
        if let (Some(pl), true) = (end, cut) {
            // Keep the side the normal points away from.
            body = kernel::half_space(&body, pl, false)?
                .0
                .ok_or_else(|| DocError::Invalid(format!("trimming leaves nothing of the member on line `{}`", m.id)))?;
        }
    }
    Ok(body)
}

/// The record of a member just built.
fn record(feature: u64, line: &str, body: &str, ends: [Option<Plane>; 2], built: &Body) -> MemberRecord {
    let (faces, bounds) = shape(built);
    MemberRecord { feature, line: line.to_string(), body: body.to_string(), ends, faces, bounds }
}

/// What tells a member's body from one a later feature changed: its face count and outline.
fn shape(body: &Body) -> (usize, [f64; 6]) {
    let b = body.outline_bounds();
    (body.face_count(), [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z])
}

/// Whether `body` is still the body the member was last built as.
fn unchanged(rec: &MemberRecord, body: &Body) -> bool {
    let (faces, bounds) = shape(body);
    faces == rec.faces && bounds.iter().zip(rec.bounds).all(|(a, b)| (a - b).abs() <= 1e-7 * (1.0 + b.abs()))
}

/// Evaluate a `FrameMember` feature: one new body per line, each named
/// ([`member_names`], or by position for a copy whose `body_names` were set).
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { body_name, names, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let lay = layout(doc, vals, f, st)?;
    let made = lay.members.iter().map(|m| build(m, &lay, &[None, None])).collect::<Result<Vec<_>>>()?;
    let ids: Vec<String> = lay.members.iter().map(|m| m.id.clone()).collect();
    let add = |st: &mut ModelState, name: String, id: &str, body: Body| {
        st.frame_members.push(record(f.id, id, &name, [None, None], &body));
        st.bodies.push(ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    };
    if !f.body_names.is_empty() {
        // A copy (pasted, or in a copied component) names its members by position, as any
        // feature's bodies.
        for (k, (body, id)) in made.into_iter().zip(&ids).enumerate() {
            let name = new_name(st, f, k);
            add(st, name, id, body);
        }
        return Ok(());
    }
    let base = body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or(lay.description.clone(), str::to_string);
    for ((name, body), id) in member_names(st, &ids, names, &base).into_iter().zip(made).zip(&ids) {
        add(st, name, id, body);
    }
    Ok(())
}

/// A member a trim reads: its record in the model, the feature that lays it out and its place there.
struct Found {
    record: usize,
    feature: u64,
    name: String,
}

/// What a trim cuts to: a side of a member, or a plane.
enum Tool {
    Member { at: Found, side: Option<Side> },
    Plane(Plane),
}

/// Evaluate a `FrameTrim` feature: rebuild each member with the end that meets each target moved
/// to it, `gap` short of it. The member keeps its body's name and its record.
pub(super) fn trim(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameTrim { members, to, end, gap } = &f.kind else {
        return Err(DocError::Invalid("not a frame trim feature".into()));
    };
    if members.is_empty() || members.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("a trim takes 1…{MAX_MEMBERS} members")));
    }
    if to.is_empty() || to.len() > frame::MAX_TRIM_TARGETS {
        return Err(DocError::Invalid(format!("a trim takes 1…{} targets", frame::MAX_TRIM_TARGETS)));
    }
    let gap = match gap {
        Some(e) => val(vals, e, Kind::Length)?,
        None => 0.0,
    };
    if !(0.0..=frame::MAX_TRIM_GAP).contains(&gap) {
        return Err(DocError::Invalid(format!("the gap must be 0…{} mm", frame::MAX_TRIM_GAP)));
    }
    // Each member feature laid out once.
    let mut layouts: BTreeMap<u64, Layout> = BTreeMap::new();
    let tools = to
        .iter()
        .map(|t| match t {
            TrimTarget::Member { member, side } => Ok(Tool::Member { at: find(doc, vals, f, st, &mut layouts, member, false)?, side: *side }),
            TrimTarget::Plane { plane } => Ok(Tool::Plane(doc.resolve_plane_in(vals, plane, 0, Some(st))?)),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    for r in members {
        if !seen.insert(r) {
            return Err(DocError::Invalid(format!("member `{}` of {} is listed twice", r.line, r.feature)));
        }
        let m = find(doc, vals, f, st, &mut layouts, r, true)?;
        let rec = st.frame_members.get(m.record).cloned().ok_or_else(|| DocError::Invalid("member record".into()))?;
        let lay = layouts.get(&m.feature).ok_or_else(|| DocError::Invalid("member layout".into()))?;
        let laid = lay.member(&rec.line).ok_or_else(|| DocError::Unknown(format!("line `{}` of {}", rec.line, r.feature)))?;
        let (c0, t0, t1) = laid.axis(lay, &rec.ends)?;
        // The end each target moves, and the plane it moves to.
        let mut ends = rec.ends;
        let mut by: [Option<usize>; 2] = [None, None];
        for (k, tool) in tools.iter().enumerate() {
            let cut = match tool {
                Tool::Member { at, side } => {
                    if at.record == m.record {
                        return Err(DocError::Invalid(format!("`{}` can't be trimmed to itself", m.name)));
                    }
                    let tr = st.frame_members.get(at.record).ok_or_else(|| DocError::Invalid("member record".into()))?;
                    let tl = layouts.get(&at.feature).ok_or_else(|| DocError::Invalid("member layout".into()))?;
                    let tm = tl.member(&tr.line).ok_or_else(|| DocError::Unknown(format!("line `{}`", tr.line)))?;
                    to_member(laid, (c0, t0, t1), *end, (tm, tl, &tr.ends), *side, gap, &at.name)?
                }
                Tool::Plane(pl) => to_plane(laid, (c0, t0, t1), *end, pl, gap),
            };
            let Some((which, plane)) = cut else { continue };
            if let Some(first) = by.get(which).copied().flatten() {
                return Err(DocError::Invalid(format!(
                    "targets {} and {} both trim the {} of `{}` (trim them in separate features)",
                    first + 1,
                    k + 1,
                    if which == 0 { "start" } else { "end" },
                    m.name
                )));
            }
            if let (Some(slot), Some(e)) = (by.get_mut(which), ends.get_mut(which)) {
                *slot = Some(k);
                *e = Some(plane);
            }
        }
        if by.iter().all(Option::is_none) {
            return Err(DocError::Invalid(format!("`{}` meets none of the targets", m.name)));
        }
        let body = build(laid, lay, &ends)?;
        let i = st.bodies.iter().position(|b| b.name == rec.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", rec.body)))?;
        let Some(mb) = st.bodies.get(i) else { return Err(DocError::Unknown(format!("body `{}`", rec.body))) };
        let made = record(rec.feature, &rec.line, &rec.body, ends, &body);
        let rebuilt = ModelBody::traced_new(mb.name.clone(), crate::lineage::numbered_tool(body), mb.feature);
        if let (Some(slot), Some(r)) = (st.bodies.get_mut(i), st.frame_members.get_mut(m.record)) {
            *slot = rebuilt;
            *r = made;
        }
    }
    Ok(())
}

/// Find a member in the model: its record, and its feature laid out (into `layouts`). It must be
/// a member of an earlier feature of the same component. A member to be `rebuilt` must also have
/// its body as that feature (or an earlier trim) left it; a target is worked out from its line and
/// profile, so what later features did to its body does not matter.
fn find(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    layouts: &mut BTreeMap<u64, Layout>,
    r: &MemberRef,
    rebuilt: bool,
) -> Result<Found> {
    let mf = doc.find_feature(&r.feature).ok_or_else(|| DocError::Unknown(format!("feature `{}`", r.feature)))?;
    if !matches!(mf.kind, FeatureKind::FrameMember { .. }) {
        return Err(DocError::Invalid(format!("`{}` is not a frame member feature", r.feature)));
    }
    if mf.component != f.component {
        return Err(DocError::Invalid(format!("not supported yet: trimming members of another component (`{}`)", r.feature)));
    }
    let record = st
        .frame_members
        .iter()
        .position(|m| m.feature == mf.id && m.line == r.line)
        .ok_or_else(|| DocError::Unknown(format!("member on line `{}` of `{}` (it must come earlier in the timeline)", r.line, r.feature)))?;
    let rec = st.frame_members.get(record).ok_or_else(|| DocError::Invalid("member record".into()))?;
    if rebuilt {
        let body =
            st.body(&rec.body).ok_or_else(|| DocError::Unknown(format!("body `{}` (member on line `{}` of `{}`)", rec.body, r.line, r.feature)))?;
        if !unchanged(rec, &body.body) {
            return Err(DocError::Invalid(format!("`{}` was changed after it was made: trim it before the features that change it", rec.body)));
        }
    }
    if let std::collections::btree_map::Entry::Vacant(slot) = layouts.entry(mf.id) {
        slot.insert(layout(doc, vals, mf, st)?);
    }
    Ok(Found { record, feature: mf.id, name: rec.body.clone() })
}

/// The end of a member (whose middle line runs from `c0` along its `d`, between `t0` and `t1`)
/// nearer `t` (along it), or the one asked for: 0 the start, 1 the end.
fn which_end(t: f64, (t0, t1): (f64, f64), end: TrimEnd) -> usize {
    match end {
        TrimEnd::Start => 0,
        TrimEnd::End => 1,
        TrimEnd::Auto => usize::from((t - t0).abs() > (t - t1).abs()),
    }
}

/// The plane an end of member `m` moves to against a side of member `target` (its laid-out
/// line, its layout and its ends): `None` when the middle line of `m` misses the target.
fn to_member(
    m: &Laid,
    (c0, t0, t1): (Vec3, f64, f64),
    end: TrimEnd,
    (tm, tl, tends): (&Laid, &Layout, &[Option<Plane>; 2]),
    side: Option<Side>,
    gap: f64,
    name: &str,
) -> Result<Option<(usize, Plane)>> {
    let (tc0, s0, s1) = tm.axis(tl, tends)?;
    // Where the two middle lines come closest.
    let (d, td) = (m.d, tm.d);
    let b = d.dot(td);
    let denom = 1.0 - b * b;
    if !(denom > 1e-12) {
        // Alongside the target: a butt joint needs it across.
        return Ok(None);
    }
    let w = c0 - tc0;
    let t = (b * td.dot(w) - d.dot(w)) / denom;
    let s = td.dot(w) + b * t;
    // The point must lie in the target's section and along its length.
    let p = c0 + d * t;
    let (u, v) = (tm.plane.x.dot(p - tm.plane.origin), tm.plane.y.dot(p - tm.plane.origin));
    let tol = 1e-6 * (1.0 + (tl.hi - tl.lo).len() + tm.len);
    let inside = u >= tl.lo.x - tol && u <= tl.hi.x + tol && v >= tl.lo.y - tol && v <= tl.hi.y + tol;
    if !(inside && s >= s0 - tol && s <= s1 + tol) {
        return Ok(None);
    }
    let which = which_end(t, (t0, t1), end);
    // The member stays on the side the kept end is.
    let keep = if which == 0 { d } else { -d };
    let (x, y, o) = (tm.plane.x, tm.plane.y, tm.plane.origin);
    let sides =
        [(Side::Right, x, o + x * tl.hi.x), (Side::Left, -x, o + x * tl.lo.x), (Side::Top, y, o + y * tl.hi.y), (Side::Bottom, -y, o + y * tl.lo.y)];
    let pick = match side {
        Some(want) => sides.iter().find(|(s, _, _)| *s == want),
        None => sides.iter().max_by(|a, b| a.1.dot(keep).total_cmp(&b.1.dot(keep))),
    };
    let Some(&(_, n, q)) = pick else { return Ok(None) };
    if !(n.dot(keep) >= MIN_CROSSING) {
        return Err(DocError::Invalid(format!("that side of `{name}` faces away from the member trimmed to it")));
    }
    // The cut faces out of the member: back towards the target.
    Ok(Plane::from_normal(q + n * gap, -n).map(|pl| (which, pl)))
}

/// The plane an end of member `m` moves to against plane `pl`: `None` when `m` runs along it.
fn to_plane(m: &Laid, (c0, t0, t1): (Vec3, f64, f64), end: TrimEnd, pl: &Plane, gap: f64) -> Option<(usize, Plane)> {
    let n = pl.normal();
    let nd = n.dot(m.d);
    if !(nd.abs() >= MIN_CROSSING) {
        return None;
    }
    let t = n.dot(pl.origin - c0) / nd;
    let which = which_end(t, (t0, t1), end);
    // The member stays on the side the kept end is: the normal turned towards it.
    let keep = if which == 0 { m.d } else { -m.d };
    let n = if n.dot(keep) < 0.0 { -n } else { n };
    Plane::from_normal(pl.origin + n * gap, -n).map(|cut| (which, cut))
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

/// The ids of a `FrameMember` feature's lines in `st`, in the order its members are made.
pub fn member_line_ids(st: &ModelState, kind: &FeatureKind) -> Result<Vec<String>> {
    let FeatureKind::FrameMember { path_sketch, lines, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch}")))?;
    Ok(member_lines(ps, lines)?.into_iter().map(|(id, _, _)| id).collect())
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
