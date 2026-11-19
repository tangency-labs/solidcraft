//! Frame members (#4003): one body per straight line of a frame sketch, its cross-section a
//! profile laid square to the line (`frame::member_axes`) and run from one end to the other; and
//! their trims, which rebuild a member with its ends moved to the planes they meet.

use solidcraft_geom::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_geom::{Aabb2, Ring};
use solidcraft_kernel::{self as kernel, Body};

use crate::lineage::Traced;
use solidcore_ops::frame_trim::{self, Target, TrimRefusal};
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Expr, Kind, Value};
use crate::frame::{self, MAX_MEMBERS, MemberProfile, MemberRecord, MemberRef, Side, TrimEnd, TrimTarget};
use crate::{DocError, Document, Feature, FeatureId, FeatureKind, ProfileSel, Result};

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;
/// Shortest member (mm).
const MIN_LENGTH: f64 = 1e-6;
/// How far (mm) a 3D sketch curve may stray from the line between its ends and still be a line.
const STRAIGHT_TOL: f64 = 1e-3;

/// The members a `FrameMember` feature lays out, before they are built.
pub(super) struct Layout {
    pub members: Vec<Laid>,
    /// The section, in its own plane's coordinates (mirrored if the feature mirrors it).
    pub regions: Vec<Region2>,
    /// What members without a name of their own are named after.
    pub description: String,
}

/// One member laid out: its line and the plane of its section at the line's start.
pub(super) struct Laid {
    pub id: String,
    /// The section's plane at the line's start (the profile's own origin at its origin), its x × y
    /// along the line.
    pub plane: Plane,
    /// The member as the kernel lays it out, for its middle line, ends and trims.
    pub member: frame_trim::Member,
}

impl Layout {
    fn member(&self, line: &str) -> Option<&Laid> {
        self.members.iter().find(|m| m.id == line)
    }
}

impl Laid {
    /// The line through the middle of the section's bounding box, and where each end of `ends`
    /// crosses it (the kernel's).
    fn axis(&self, ends: &[Option<Plane>; 2]) -> Result<frame_trim::Axis> {
        frame_trim::axis(&self.member, ends).map_err(|why| refused(&self.id, why))
    }
}

/// The kernel's refusal of the member on line `line`, worded.
fn refused(line: &str, why: frame_trim::Refusal) -> DocError {
    use frame_trim::Refusal as R;
    DocError::Invalid(match why {
        R::Degenerate => format!("line `{line}` has no length"),
        R::RunsAlong => format!("the end of the member on line `{line}` runs along it"),
        R::WrongWay => format!("the member on line `{line}`: an end plane faces the wrong way"),
        R::TooFar => format!("the member on line `{line}` would reach more than {} mm", frame_trim::MAX_REACH),
        R::EndsCross => format!("trimming leaves nothing of the member on line `{line}` (its ends cross)"),
        R::FacesAway => format!("a side faces away from the member on line `{line}`"),
    })
}

/// A trim's end choice as the kernel's.
fn end_choice(end: TrimEnd) -> frame_trim::End {
    match end {
        TrimEnd::Auto => frame_trim::End::Nearer,
        TrimEnd::Start => frame_trim::End::Start,
        TrimEnd::End => frame_trim::End::End,
    }
}

/// A member's side as the kernel's.
fn side_choice(side: Side) -> frame_trim::Side {
    match side {
        Side::Right => frame_trim::Side::Right,
        Side::Left => frame_trim::Side::Left,
        Side::Top => frame_trim::Side::Top,
        Side::Bottom => frame_trim::Side::Bottom,
    }
}

/// Lay out a `FrameMember` feature's members: its lines in its component's frame, and its section.
pub(super) fn layout(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Layout> {
    let FeatureKind::FrameMember { path_sketch, lines, profile, anchor, locate, angle, mirror, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    // The frame sketch in this feature's component frame: its plane, and its 3D lines.
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let segments = member_lines(mapped.as_ref(), lines)?;
    let (mut regions, description, origin) = profile_regions(vals, st, profile)?;
    if *mirror {
        regions = regions.iter().map(frame::mirrored).collect();
    }
    // Trim planes and reach limits must enclose the exact section, including spline extrema.
    // A sampled profile can miss an extremum between stations.
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for body in kernel::extrude(&Plane::XY, &regions, 0.0, 1.0)? {
        let bounds = body.exact_bounds()?;
        lo = Vec2::new(lo.x.min(bounds.min.x), lo.y.min(bounds.min.y));
        hi = Vec2::new(hi.x.max(bounds.max.x), hi.y.max(bounds.max.y));
    }
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return Err(DocError::Invalid("the member profile has no area".into()));
    }
    let at = match locate {
        Some([x, y]) => {
            // A design file is hostile input: the command checks the point, a file may not.
            if !(x.is_finite() && y.is_finite() && x.abs() <= frame::MAX_PROFILE_SIZE && y.abs() <= frame::MAX_PROFILE_SIZE) {
                return Err(DocError::Invalid(format!("frame member `locate` must be two finite lengths within {} mm", frame::MAX_PROFILE_SIZE)));
            }
            // `locate` is the profile point that sits on the line, measured from the profile's
            // own origin (`Profile::origin`). The built-in region is centred on its bounding
            // box, so that origin moves when the section's size changes, and the located point
            // moves with it: the member stays attached at the same point of its section.
            let p = origin + Vec2::new(*x, *y);
            if *mirror { Vec2::new(-p.x, p.y) } else { p }
        }
        None => frame::anchor_point(&regions, *anchor),
    };
    let turn = match angle {
        Some(e) => val(vals, e, Kind::Angle)?,
        None => 0.0,
    };
    let turns = member_turns(doc, vals, FeatureId(f.id));
    let centre = frame::anchor_point(&regions, frame::Anchor::Center);
    let mut members = Vec::with_capacity(segments.len());
    for (id, a, b) in segments {
        let plane = member_plane(a, b, turn, at, centre, turns.get(&id).map(|(_, angle)| *angle))
            .ok_or_else(|| DocError::Invalid(format!("line `{id}`: no plane square to it")))?;
        let member = frame_trim::Member::along(&plane, a, b, lo, hi).map_err(|why| refused(&id, why))?;
        members.push(Laid { plane, member, id });
    }
    Ok(Layout { members, regions, description })
}

/// The plane a member's profile is laid in at `a`, square to the line on to `b`: profile point
/// `at` on the line, the profile turned `turn` about it. A Rotate step's turn `own` turns it on
/// about the profile point `centre`, which stays where it was.
fn member_plane(a: Vec3, b: Vec3, turn: f64, at: Vec2, centre: Vec2, own: Option<f64>) -> Option<Plane> {
    let (x, y) = frame::member_axes(b - a, turn)?;
    let origin = a - x * at.x - y * at.y;
    let Some(t) = own else { return Plane::new(origin, x, y) };
    let pivot = origin + x * centre.x + y * centre.y;
    let (x, y) = frame::member_axes(b - a, turn + t.rem_euclid(std::f64::consts::TAU))?;
    Plane::new(pivot - x * centre.x - y * centre.y, x, y)
}

/// Build a member: its section run along its line, each end square at the line's end or cut in
/// its plane (`ends`, normals out of the member).
pub(super) fn build(m: &Laid, lay: &Layout, ends: &[Option<Plane>; 2], feature: u64) -> Result<Traced> {
    // The extrusion range, and which ends are then cut at a slant (the kernel's: the ends must
    // leave the whole section between them).
    let frame_trim::Extent { from, to, cut: [s_cut, e_cut] } = frame_trim::extent(&m.member, ends).map_err(|why| refused(&m.id, why))?;
    let mut bodies = kernel::extrude(&m.plane, &lay.regions, from, to)?;
    let body = match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => body,
        _ => return Err(DocError::Invalid(format!("the member on line `{}` is not one solid", m.id))),
    };
    // Start from the extrusion's original numbered roles, before a boolean can reorder its
    // faces. Cut caps take the role of the cap they replace; surviving sides follow history.
    let caps = [false, true].map(|high| {
        body.history().and_then(|h| {
            h.faces.iter().position(|face| face.origins.iter().any(|o| matches!(o, kernel::history::FaceOrigin::Cap { end } if *end == high)))
        })
    });
    let mut body = crate::lineage::numbered_rebuilt(body, feature);
    for ((end, cut), cap) in ends.iter().zip([s_cut, e_cut]).zip(caps) {
        if let (Some(pl), true) = (end, cut) {
            // Keep the side the normal points away from. The half-space tool's end cap is
            // the cut face, with the replaced member cap's name.
            let (part, tool) = kernel::half_space(&body.body, pl, false)?;
            let part = part.ok_or_else(|| DocError::Invalid(format!("trimming leaves nothing of the member on line `{}`", m.id)))?;
            let cap = cap.ok_or_else(|| DocError::Invalid("the member's end face has no construction identity".into()))?;
            let roles = crate::lineage::ProfileRoles {
                sketch: None,
                region: None,
                caps: [None, Some(format!("face:{cap}"))],
                cap_synonyms: [None, None],
                thin: None,
            };
            let tool = roles.tool(tool, None).rebuilt(feature);
            body = Traced::step(part, &[&body, &tool]);
            if body.lineage.is_none() {
                return Err(DocError::Invalid("the trimmed member's face identities could not be preserved".into()));
            }
        }
    }
    Ok(body)
}

/// The record of a member just built.
fn record(feature: u64, line: &str, body: &str, ends: [Option<Plane>; 2], built: &Body) -> MemberRecord {
    MemberRecord { feature, line: line.to_string(), body: body.to_string(), ends, built: Some(built.clone()) }
}

/// Evaluate a `FrameMember` feature: one new body per line, each named
/// ([`member_names`], or by position for a copy whose `body_names` were set).
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { body_name, names, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let lay = layout(doc, vals, f, st)?;
    let made = lay.members.iter().map(|m| build(m, &lay, &[None, None], f.id)).collect::<Result<Vec<_>>>()?;
    let ids: Vec<String> = lay.members.iter().map(|m| m.id.clone()).collect();
    let add = |st: &mut ModelState, name: String, id: &str, body: Traced| {
        st.frame_members.push(record(f.id, id, &name, [None, None], &body.body));
        st.bodies.push(ModelBody::traced_new(name, body, f.id));
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

/// The member of feature `f` on line `line` as its body is now: the plane square to the line at
/// its start (x × y along the line), its length between its ends (where trims moved them, along
/// the middle of the section) and its profile in that plane.
pub(crate) fn member_section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
) -> Result<(Plane, f64, Region2)> {
    let lay = layout(doc, vals, f, st)?;
    let laid = lay.member(line).ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let ends = st.frame_members.iter().rev().find(|m| m.feature == f.id && m.line == line).map_or([None, None], |m| m.ends);
    let length = laid.axis(&ends)?.length;
    let region = lay.regions.first().cloned().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    Ok((laid.plane, length, region))
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
pub(super) fn trim(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
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
            TrimTarget::Member { member, side } => Ok(Tool::Member { at: find(doc, vals, f, st, &mut layouts, member)?, side: *side }),
            TrimTarget::Plane { plane } => Ok(Tool::Plane(target_plane(doc, vals, f, st, plane, warning)?)),
        })
        .collect::<Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    for r in members {
        if !seen.insert(r) {
            return Err(DocError::Invalid(format!("member `{}` of {} is listed twice", r.line, r.feature)));
        }
        let m = find(doc, vals, f, st, &mut layouts, r)?;
        let rec = st.frame_members.get(m.record).cloned().ok_or_else(|| DocError::Invalid("member record".into()))?;
        let lay = layouts.get(&m.feature).ok_or_else(|| DocError::Invalid("member layout".into()))?;
        let laid = lay.member(&rec.line).ok_or_else(|| DocError::Unknown(format!("line `{}` of {}", rec.line, r.feature)))?;
        // Its own ends are checked before its targets, as they always were.
        laid.axis(&rec.ends)?;
        // The targets as the trim workflow takes them: a member (its layout and its ends as they
        // are now) or a plane.
        let mut targets = Vec::with_capacity(tools.len());
        for tool in &tools {
            targets.push(match tool {
                Tool::Member { at, side } => {
                    if at.record == m.record {
                        return Err(DocError::Invalid(format!("`{}` can't be trimmed to itself", m.name)));
                    }
                    let tr = st.frame_members.get(at.record).ok_or_else(|| DocError::Invalid("member record".into()))?;
                    let tl = layouts.get(&at.feature).ok_or_else(|| DocError::Invalid("member layout".into()))?;
                    let tm = tl.member(&tr.line).ok_or_else(|| DocError::Unknown(format!("line `{}`", tr.line)))?;
                    Target::Member { member: &tm.member, ends: &tr.ends, side: side.map(side_choice) }
                }
                Tool::Plane(pl) => Target::Plane(pl),
            });
        }
        let cuts = frame_trim::trim(&laid.member, &rec.ends, end_choice(*end), &targets, gap).map_err(|why| match why {
            TrimRefusal::Member { target: None, why } => refused(&rec.line, why),
            TrimRefusal::Member { target: Some(k), why } => match (tools.get(k), why) {
                (Some(Tool::Member { at, .. }), frame_trim::Refusal::FacesAway) => {
                    DocError::Invalid(format!("that side of `{}` faces away from the member trimmed to it", at.name))
                }
                (Some(Tool::Member { at, .. }), why) => refused(st.frame_members.get(at.record).map_or("", |r| r.line.as_str()), why),
                (_, why) => refused(&rec.line, why),
            },
            TrimRefusal::SameEnd { first, second, end } => DocError::Invalid(format!(
                "targets {} and {} both trim the {} of `{}` (trim them in separate features)",
                first + 1,
                second + 1,
                if end == 0 { "start" } else { "end" },
                m.name
            )),
            TrimRefusal::MeetsNone => DocError::Invalid(format!("`{}` meets none of the targets", m.name)),
        })?;
        let mut ends = rec.ends;
        for (e, cut) in ends.iter_mut().zip(cuts) {
            if let Some(cut) = cut {
                *e = Some(Plane::from_normal(cut.origin, cut.normal).ok_or_else(|| DocError::Invalid("the cut plane is degenerate".into()))?);
            }
        }
        let body = build(laid, lay, &ends, rec.feature)?;
        let i = st.bodies.iter().position(|b| b.name == rec.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", rec.body)))?;
        let Some(mb) = st.bodies.get(i) else { return Err(DocError::Unknown(format!("body `{}`", rec.body))) };
        let made = record(rec.feature, &rec.line, &rec.body, ends, &body.body);
        // Its faces keep the names the member feature gave them, so what later features picked
        // on it stays picked.
        let rebuilt = ModelBody::traced_new(mb.name.clone(), body, mb.feature);
        if let (Some(slot), Some(r)) = (st.bodies.get_mut(i), st.frame_members.get_mut(m.record)) {
            *slot = rebuilt;
            *r = made;
        }
    }
    Ok(())
}

/// A plane a member is trimmed to, where it is now: a body face's plane follows the face, as a
/// sketch on that face does.
fn target_plane(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    plane: &crate::PlaneRef,
    warning: &mut Option<String>,
) -> Result<Plane> {
    let Some(anchor) = crate::face_anchor::plane_face_anchor(plane) else { return doc.resolve_plane_in(vals, plane, 0, f.component, Some(st)) };
    let fst = crate::state_in_face_anchor_frame(doc, st, anchor, f.component)?;
    let (pl, note) = doc.resolve_attached_plane_noted(vals, plane, f.component, st, &fst)?;
    if let Some(note) = note {
        super::append_warning(warning, note);
    }
    Ok(pl)
}

/// Find a member in the model: its record, and its feature laid out (into `layouts`). It must be
/// a member of an earlier feature of the same component, with its body as that feature (or an
/// earlier trim) left it. Changed targets also need refusal: their line and profile no longer
/// establish where their sides are. A planar face target can follow such a body instead.
fn find(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    layouts: &mut BTreeMap<u64, Layout>,
    r: &MemberRef,
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
    {
        // A later feature may have removed the member's body and given its name to another.
        let body = st
            .body(&rec.body)
            .filter(|b| b.feature == rec.feature)
            .ok_or_else(|| DocError::Unknown(format!("body `{}` (member on line `{}` of `{}`)", rec.body, r.line, r.feature)))?;
        if body.component != mf.component || body.transferred || !rec.is_unchanged(&body.body) {
            return Err(DocError::Invalid(format!("`{}` was changed after it was made: trim it before the features that change it", rec.body)));
        }
    }
    if let std::collections::btree_map::Entry::Vacant(slot) = layouts.entry(mf.id) {
        slot.insert(layout(doc, vals, mf, st)?);
    }
    Ok(Found { record, feature: mf.id, name: rec.body.clone() })
}

/// The point member `line` of `f` turns about, halfway along it: the centre of the profile's
/// bounding box, which a Rotate step's turn leaves where it is ([`member_plane`]).
pub(crate) fn member_pivot(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState, line: &str) -> Result<Vec3> {
    let lay = layout(doc, vals, f, st)?;
    let m = lay.member(line).ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let centre = frame::anchor_point(&lay.regions, frame::Anchor::Center);
    Ok(m.plane.to_world(centre) + m.member.dir() * (m.member.length() * 0.5))
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

/// The angle each line of the frame member `frame` is turned to by active Rotate steps after
/// it: line id to the angle's expression and its value (radians), a later step replacing an
/// earlier one. A step whose angle doesn't evaluate with `vals` turns nothing (it reports why
/// itself, `check_turn`): an earlier step for the line, if any, stands.
pub fn member_turns(doc: &Document, vals: &BTreeMap<String, Value>, frame: FeatureId) -> BTreeMap<String, (Expr, f64)> {
    turn_steps(doc, frame).filter_map(|(line, e)| Some((line.to_string(), (e.clone(), turn_angle(vals, e).ok()?)))).collect()
}

/// The active Rotate steps of the frame member `frame`, in timeline order: each one's line id and
/// angle, whether it evaluates or not. Active: after the frame, before the marker, not suppressed.
pub(super) fn turn_steps(doc: &Document, frame: FeatureId) -> impl Iterator<Item = (&str, &Expr)> {
    let at = doc.feature_index(frame.0);
    let end = doc.marker.unwrap_or(doc.features.len()).min(doc.features.len());
    let after = at.map_or(end, |at| at.saturating_add(1));
    doc.features.iter().take(end).skip(after).filter_map(move |f| match &f.kind {
        FeatureKind::FrameMemberTurn { member, angle } if !f.suppressed && member.feature == frame => Some((member.line.as_str(), angle)),
        _ => None,
    })
}

/// A Rotate step's angle (radians), refused unless it evaluates to a finite number.
fn turn_angle(vals: &BTreeMap<String, Value>, angle: &Expr) -> Result<f64> {
    let a = val(vals, angle, Kind::Angle)?;
    if !a.is_finite() {
        return Err(DocError::Invalid("The angle must be a finite number".into()));
    }
    Ok(a)
}

/// A Rotate step's checks: its frame member is there, earlier, and still has the line, and its
/// angle evaluates.
pub(super) fn check_turn(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<()> {
    let FeatureKind::FrameMemberTurn { member, angle } = &f.kind else {
        return Err(DocError::Invalid("not a rotate member feature".into()));
    };
    let gone = || DocError::Invalid("Its frame member is gone".into());
    let source = doc.feature(member.feature.0).filter(|m| !m.suppressed && matches!(m.kind, FeatureKind::FrameMember { .. })).ok_or_else(gone)?;
    let (Some(from), Some(here)) = (doc.feature_index(source.id), doc.feature_index(f.id)) else { return Err(gone()) };
    if from >= here {
        return Err(DocError::Invalid("Its frame member must come earlier in the timeline".into()));
    }
    if !member_line_ids(st, &source.kind)?.contains(&member.line) {
        return Err(DocError::Invalid(format!("Line `{}` of its frame member is gone", member.line)));
    }
    turn_angle(vals, angle).map(|_| ())
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
fn profile_regions(vals: &BTreeMap<String, Value>, st: &ModelState, profile: &MemberProfile) -> Result<(Vec<Region2>, String, Vec2)> {
    let (regions, description, origin) = match profile {
        MemberProfile::Library { profile } => {
            let p = profile.resolve(|name, e, kind| val(vals, e, kind).map_err(|err| DocError::Invalid(format!("profile `{name}`: {err}"))))?;
            let origin = p.origin();
            (vec![p.region()?], profile.size.clone().unwrap_or_else(|| p.description()), origin)
        }
        MemberProfile::Sketch { sketch, profiles } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            let picked = match profiles {
                ProfileSel::All => material(ss)?,
                sel => select_profiles(ss, sel)?,
            };
            (solidcraft_sketch::merge_regions(&picked), ss.name.clone(), Vec2::ZERO)
        }
    };
    match regions.len() {
        1 => Ok((regions, description, origin)),
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
