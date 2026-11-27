//! General Trim/Extend on unjointed frame members. The parent owns all ordinary
//! frame layout, including pairs, auto corners, crossings, fitting and coping.
//! Rebuilding such modified members is refused explicitly until their full cuts
//! can be retained; a planar face on them remains a valid plane target.

use super::*;
use crate::frame::{MemberRecord, Side, TrimEnd, TrimMemberRef, TrimTarget};
use kernel::Body;
use solidcore_record::frame::{self as data, MemberData};
use solidcraft_kernel::{MemberRefusal, TrimRefusal, TrimTarget as Target};

/// The members a `FrameMember` feature lays out, before they are built.
pub(super) struct Layout {
    pub members: Vec<Laid>,
}

/// One member laid out: its line and the plane of its section at the line's start.
pub(super) struct Laid {
    pub id: String,
    /// The section's plane at the line's start (the profile's own origin at its origin), its x × y
    /// along the line.
    pub plane: Plane,
    pub region: Region2,
    /// The member as the kernel lays it out, for its middle line, ends and trims.
    pub member: MemberData,
}

impl Layout {
    fn member(&self, line: &str) -> Option<&Laid> {
        self.members.iter().find(|m| m.id == line)
    }
}

impl Laid {
    /// The line through the middle of the section's bounding box, and where each end of `ends`
    /// crosses it (the kernel's).
    fn axis(&self, ends: &[Option<Plane>; 2]) -> Result<data::Axis> {
        solidcraft_kernel::member_axis(&self.member, ends).map_err(|why| refused(&self.id, why))
    }
}

/// The kernel's refusal of the member on line `line`, worded.
fn refused(line: &str, why: MemberRefusal) -> DocError {
    use MemberRefusal as R;
    DocError::Invalid(match why {
        R::Degenerate => format!("line `{line}` has no length"),
        R::RunsAlong => format!("the end of the member on line `{line}` runs along it"),
        R::WrongWay => format!("the member on line `{line}`: an end plane faces the wrong way"),
        R::TooFar => format!("the member on line `{line}` would reach more than {} mm", solidcraft_kernel::FRAME_MAX_REACH),
        R::EndsCross => format!("trimming leaves nothing of the member on line `{line}` (its ends cross)"),
        R::FacesAway => format!("a side faces away from the member on line `{line}`"),
    })
}

/// A trim's end choice as the kernel's.
fn end_choice(end: TrimEnd) -> data::End {
    match end {
        TrimEnd::Auto => data::End::Nearer,
        TrimEnd::Start => data::End::Start,
        TrimEnd::End => data::End::End,
    }
}

/// A member's side as the kernel's.
fn side_choice(side: Side) -> data::Side {
    match side {
        Side::Right => data::Side::Right,
        Side::Left => data::Side::Left,
        Side::Top => data::Side::Top,
        Side::Bottom => data::Side::Bottom,
    }
}

/// Whether the feature has only unjointed stock that general Trim can rebuild.
/// These settings stay supported by the parent's ordinary frame evaluator.
pub(super) fn can_rebuild(kind: &FeatureKind) -> bool {
    matches!(kind, FeatureKind::FrameMember { corner, corners, fit_ends: false, coped: false, .. }
        if corner.is_none() && corners.is_empty())
}

fn layout(f: &Feature) -> Result<Layout> {
    if !can_rebuild(&f.kind) {
        return Err(DocError::Invalid(format!(
            "not supported yet: Trim/Extend on `{}` with corner overrides, Auto, Mitre, Butt, Fit Ends or Coped; its existing cuts must be kept (a planar face can still be a plane target)",
            f.name
        )));
    }
    Ok(Layout { members: Vec::new() })
}

/// Use the parent's actual section and orientation, including point pairs and
/// per-member rotation. Its unmodified section avoids recursive Trim lookup.
fn laid_member(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState, line: &str) -> Result<Laid> {
    layout(f)?;
    let (plane, _, region, _) = super::untrimmed_member_section(doc, vals, f, st, line)?;
    let (segments, ..) = super::segments(doc, vals, f, st)?;
    let (_, a, b) = segments.iter().find(|s| s.0 == line).ok_or_else(|| DocError::Unknown(format!("line `{line}` of {}", f.name)))?;
    // Reuse the PR's exact section-bound query; no sampled mesh decisions.
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for body in kernel::extrude(&Plane::XY, std::slice::from_ref(&region), 0.0, 1.0)? {
        let bounds = body.exact_bounds()?;
        lo = Vec2::new(lo.x.min(bounds.min.x), lo.y.min(bounds.min.y));
        hi = Vec2::new(hi.x.max(bounds.max.x), hi.y.max(bounds.max.y));
    }
    let member = solidcraft_kernel::member_along(&plane, *a, *b, lo, hi).map_err(|why| refused(line, why))?;
    Ok(Laid { id: line.to_string(), plane, region, member })
}

/// The parent keeps its four-field End Cap contract. Only actual general trims
/// change the start and length; ordinary auto-frame open-end rules pass through.
pub(super) fn section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
    original: (Plane, f64, Region2, [bool; 2]),
) -> Result<(Plane, f64, Region2, [bool; 2])> {
    let Some(record) = st.frame_members.iter().rev().find(|m| m.feature == f.id && m.line == line && m.ends.iter().any(Option::is_some)) else {
        return Ok(original);
    };
    let laid = laid_member(doc, vals, f, st, line)?;
    let axis = laid.axis(&record.ends)?;
    let plane = Plane::new(laid.plane.origin + laid.member.dir * axis.t0, laid.plane.x, laid.plane.y)
        .ok_or_else(|| DocError::Invalid("the trimmed member's section is degenerate".into()))?;
    Ok((plane, axis.length, original.2, original.3))
}

/// Build a member: its section run along its line, each end square at the line's end or cut in
/// its plane (`ends`, normals out of the member).
fn build(m: &Laid, ends: &[Option<Plane>; 2], feature: u64) -> Result<Traced> {
    // The extrusion range, and which ends are then cut at a slant (the kernel's: the ends must
    // leave the whole section between them).
    let data::Extent { from, to, cut: [s_cut, e_cut] } = solidcraft_kernel::member_extent(&m.member, ends).map_err(|why| refused(&m.id, why))?;
    let mut bodies = kernel::extrude(&m.plane, std::slice::from_ref(&m.region), from, to)?;
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
pub(super) fn record(feature: u64, line: &str, body: &str, ends: [Option<Plane>; 2], built: &Body) -> MemberRecord {
    MemberRecord { feature, line: line.to_string(), body: body.to_string(), ends, built: Some(built.clone()) }
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
        let cuts = solidcraft_kernel::trim_member(&laid.member, &rec.ends, end_choice(*end), &targets, gap).map_err(|why| match why {
            TrimRefusal::Member { target: None, why } => refused(&rec.line, why),
            TrimRefusal::Member { target: Some(k), why } => match (tools.get(k), why) {
                (Some(Tool::Member { at, .. }), MemberRefusal::FacesAway) => {
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
        let body = build(laid, &ends, rec.feature)?;
        let i = st.bodies.iter().position(|b| b.name == rec.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", rec.body)))?;
        let Some(mb) = st.bodies.get(i) else { return Err(DocError::Unknown(format!("body `{}`", rec.body))) };
        let made = record(rec.feature, &rec.line, &rec.body, ends, &body.body);
        // Its faces keep the names the member feature gave them, so what later features picked
        // on it stays picked.
        let mut rebuilt = ModelBody::traced_new(mb.name.clone(), body, mb.feature);
        // Trimming preserves this physical member, which later gusset/plate attachments identify.
        rebuilt.frame_member_line = mb.frame_member_line.clone();
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
    let fst = if crate::eval::plane_outer_reads_geometry(plane) {
        crate::state_in_face_anchor_frame(doc, st, anchor, f.component)?
    } else {
        crate::anchor_frame::state_for_face_anchor_only(doc, st, anchor, f.component)?
    };
    let (pl, note) = doc.resolve_attached_plane_noted(vals, plane, f.component, st, &fst)?;
    if let Some(note) = note {
        crate::eval::append_warning(warning, note);
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
    r: &TrimMemberRef,
) -> Result<Found> {
    let mf = doc.find_feature(&r.feature).ok_or_else(|| DocError::Unknown(format!("feature `{}`", r.feature)))?;
    if !matches!(mf.kind, FeatureKind::FrameMember { .. }) {
        return Err(DocError::Invalid(format!("`{}` is not a frame member feature", r.feature)));
    }
    if mf.component != f.component {
        return Err(DocError::Invalid(format!("not supported yet: trimming members of another component (`{}`)", r.feature)));
    }
    // Refuse before interpreting a modified member as pristine stock.
    layout(mf)?;
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
        slot.insert(layout(mf)?);
    }
    let lay = layouts.get_mut(&mf.id).ok_or_else(|| DocError::Invalid("member layout".into()))?;
    if lay.member(&r.line).is_none() {
        lay.members.push(laid_member(doc, vals, mf, st, &r.line)?);
    }
    Ok(Found { record, feature: mf.id, name: rec.body.clone() })
}
