//! Frame members (#4003): separate sketch curves, or one continuous tangent line/arc path.

use solidcraft_geom::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use solidcore_ops::frame_member as stock;
use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_geom::{Aabb2, Ring};
use solidcraft_kernel as kernel;
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Kind, Value};
use crate::frame::{self, MAX_MEMBERS, MemberProfile};
use crate::{DocError, Document, Feature, FeatureKind, ProfileSel, Result};

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;

/// Evaluate a `FrameMember` feature: one body per selected curve, or one for a continuous path.
/// Bodies use [`member_names`], or positional names for a copy whose `body_names` were set.
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { body_name, names, continuous, profile, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { paths, regions, description, at, turn } = layout(doc, vals, f, st)?;
    let mut made = Vec::with_capacity(paths.len());
    let mut round = Vec::with_capacity(paths.len());
    for path in &paths {
        let first = path.first().ok_or_else(|| DocError::Invalid("a member path is empty".into()))?;
        let plane = stock::section_plane(&first.geometry, at, turn).map_err(|e| path_error(path, e))?;
        let body = if let [piece] = path.as_slice()
            && let Some(length) = piece.geometry.line_length().map_err(|e| path_error(path, e))?
        {
            let mut bodies = kernel::extrude(&plane, &regions, 0.0, length)?;
            match (bodies.pop(), bodies.is_empty()) {
                (Some(body), true) => body,
                _ => return Err(DocError::Invalid("the member is not one solid".into())),
            }
        } else {
            let region = regions.first().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
            let geometry: Vec<_> = path.iter().map(|p| p.geometry).collect();
            stock::check_bend_clearance(&geometry, region, at, turn).map_err(|e| path_error(path, e))?;
            kernel::sweep(&plane, region, &path.iter().map(path_segment).collect::<Vec<_>>())?
        };
        round.push(round_record(vals, profile, at, turn, path)?);
        made.push(crate::lineage::numbered_tool(body));
    }
    if !f.body_names.is_empty() {
        // A copy (pasted, or in a copied component) names its members by position, as any
        // feature's bodies.
        for (k, (t, record)) in made.into_iter().zip(round).enumerate() {
            let name = new_name(st, f, k);
            let mut body = ModelBody::traced_new(name, t, f.id);
            if let Some(record) = record {
                body.certify_round_member(record);
            }
            st.bodies.push(body);
        }
        return Ok(());
    }
    let base = body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or(description, str::to_string);
    let ids = path_keys(&paths, *continuous);
    for ((name, t), record) in member_names(st, &ids, names, &base).into_iter().zip(made).zip(round) {
        let mut body = ModelBody::traced_new(name, t, f.id);
        if let Some(record) = record {
            body.certify_round_member(record);
        }
        st.bodies.push(body);
    }
    Ok(())
}

/// The feature's path selection and section settings, shared by evaluation and end caps.
struct Layout {
    paths: Vec<Vec<Piece>>,
    regions: Vec<Region2>,
    description: String,
    at: Vec2,
    turn: f64,
}

/// Resolve `f`'s path sketch, selected curves, profile regions, anchor point and turn angle.
fn layout(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Layout> {
    let FeatureKind::FrameMember { path_sketch, lines, continuous, profile, anchor, locate, angle, mirror, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let paths = member_paths(mapped.as_ref(), lines, *continuous)?;
    let (mut regions, description, origin) = profile_regions(vals, st, profile)?;
    if *mirror {
        regions = regions.iter().map(frame::mirrored).collect();
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
    Ok(Layout { paths, regions, description, at, turn })
}

/// The section of a straight member, used by the existing end-cap command.
/// Curved members have different terminal frames and are refused by that command for now.
pub(crate) fn member_section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
) -> Result<(Plane, f64, Region2)> {
    let FeatureKind::FrameMember { continuous, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { paths, regions, at, turn, .. } = layout(doc, vals, f, st)?;
    let keys = path_keys(&paths, *continuous);
    let path = keys
        .iter()
        .zip(&paths)
        .find_map(|(key, path)| (key == line).then_some(path))
        .ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let [piece] = path.as_slice() else {
        return Err(DocError::Invalid("not supported yet: end caps on curved frame members".into()));
    };
    let length = piece
        .geometry
        .line_length()
        .map_err(|e| path_error(path, e))?
        .ok_or_else(|| DocError::Invalid("not supported yet: end caps on curved frame members".into()))?;
    let plane = stock::section_plane(&piece.geometry, at, turn).map_err(|e| path_error(path, e))?;
    let region = regions.into_iter().next().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    Ok((plane, length, region))
}

/// Nominal section and terminal datums, from the same transport the sweep uses.
fn round_record(
    vals: &BTreeMap<String, Value>,
    profile: &MemberProfile,
    at: Vec2,
    turn: f64,
    path: &[Piece],
) -> Result<Option<crate::frame_cope::RoundMember>> {
    use crate::frame_cope::{Bounds, RoundMember, Terminal};
    use solidcore_ops::frame_member::Segment;
    use solidcore_ops::tube_cope;
    let MemberProfile::Library { profile } = profile else { return Ok(None) };
    let frame::Profile::RoundTube { diameter, wall } = profile.resolve(|_, e, kind| val(vals, e, kind))? else { return Ok(None) };
    // An off-centre circular profile sweeps about a different centreline at a bend.
    if at.x.abs() > 1e-9 || at.y.abs() > 1e-9 {
        return Ok(None);
    }
    let radius = diameter / 2.0;
    let fail = |e: &dyn std::fmt::Display| DocError::Invalid(format!("the member's cope datum could not be placed: {e}"));
    let pieces: Vec<solidcore_ops::frame_member::Piece> = path.iter().map(|p| p.geometry).collect();
    let bounds: Vec<Bounds> = pieces
        .iter()
        .map(|p| match p.data().segment {
            Segment::Line { a, b } => tube_cope::line_box(a, b, radius).into(),
            Segment::Arc { a, center, .. } => tube_cope::arc_box(a, center, radius).into(),
        })
        .collect();
    let [first, last] = match (pieces.first(), pieces.last()) {
        (Some(f), Some(l)) => [f, l],
        _ => return Err(DocError::Invalid("empty member path".into())),
    };
    let [start_plane, end_plane] = solidcore_ops::frame_member::terminal_planes(&pieces, Vec2::ZERO, turn).map_err(|e| fail(&e))?;
    let start = match first.line_length().map_err(|e| fail(&e))? {
        Some(length) => {
            Some(Terminal::new(tube_cope::terminal_at(&start_plane, length, false).map_err(|e| fail(&e))?, bounds.iter().skip(1).copied().collect()))
        }
        None => None,
    };
    let end = match last.line_length().map_err(|e| fail(&e))? {
        Some(length) => Some(Terminal::new(
            tube_cope::terminal_at(&end_plane, length, true).map_err(|e| fail(&e))?,
            bounds.iter().take(bounds.len().saturating_sub(1)).copied().collect(),
        )),
        None => None,
    };
    let straight = pieces.len() == 1 && start.is_some();
    Ok(Some(RoundMember { radius, wall, start, end, straight, cuts: Vec::new() }))
}

/// The mating member's bore is filled for the cut: a cope meets its outside wall and must
/// never retain the disconnected stub a subtraction of hollow stock would leave inside it.
pub(super) fn cope(doc: &Document, f: &Feature, st: &mut ModelState) -> Result<()> {
    use crate::frame_cope::{Cylinder, End, MAX_CUTTERS, Mate};
    let FeatureKind::FrameCope { member, tools, end } = &f.kind else { return Err(DocError::Invalid("not a cope".into())) };
    if tools.is_empty() || tools.len() > MAX_CUTTERS {
        return Err(DocError::Invalid("a cope needs 1..16 mating members".into()));
    }
    let mb = st.body(member).ok_or_else(|| DocError::Unknown(format!("member {member}")))?.clone();
    let mut data = mb.round_member()?.clone();
    if doc.body_offsets.contains_key(member.as_ref()) {
        return Err(DocError::Invalid("not supported yet: cope of an offset member".into()));
    }
    let mut seen = HashSet::new();
    let mut cylinders = Vec::new();
    let mut mates = Vec::new();
    for name in tools {
        if name == member || !seen.insert(name.as_ref()) {
            return Err(DocError::Invalid("cope members must be distinct".into()));
        }
        let tool = st.body(name).ok_or_else(|| DocError::Unknown(format!("member {name}")))?;
        if tool.component != mb.component || doc.body_offsets.contains_key(name.as_ref()) {
            return Err(DocError::Invalid("not supported yet: cope members must share a component and have no body offsets".into()));
        }
        let mate = tool.round_member()?;
        if !mate.straight {
            return Err(DocError::Invalid(format!("not supported yet: a mating member must be straight; `{}` bends", name.as_ref())));
        }
        // A mate may already be coped at its ends; prepare refuses a joint that reaches one.
        let reach = |at: End| mate.cuts.iter().filter(|c| c.end == at).map(|c| c.depth_bound).fold(0.0, f64::max);
        let terminal = mate.terminal(End::Start)?;
        cylinders.push(Cylinder { origin: terminal.origin, axis: terminal.axis, radius: mate.radius, length: terminal.length });
        mates.push(Mate { name: name.as_ref().to_string(), start: reach(End::Start), end: reach(End::End) });
    }
    let cut = data.prepare(*end, cylinders, &mates)?;
    let mut traced = mb.traced();
    for c in &cut.cylinders {
        let tool = crate::lineage::numbered_tool(kernel::cylinder(c.origin, c.axis, c.radius, c.length)?);
        let result = kernel::boolean(&traced.body, &tool.body, kernel::BoolOp::Cut)?
            .ok_or_else(|| DocError::Invalid("the cope removed the whole member".into()))?;
        traced = crate::lineage::Traced::step(result, &[&traced, &tool]);
    }
    data.cuts.push(cut);
    let mut body = ModelBody::traced_new(mb.name.clone(), traced, mb.feature);
    body.component = mb.component;
    body.certify_round_member(data);
    let slot = st.bodies.iter_mut().find(|b| b.name == mb.name).ok_or(DocError::Unknown(mb.name))?;
    *slot = body;
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

/// A sketch identity paired with exact geometry validated and prepared by solidcore.
struct Piece {
    id: String,
    geometry: stock::Piece,
}

/// Convert the prepared segment to the existing sweep boundary, field for field.
fn path_segment(piece: &Piece) -> kernel::PathSeg {
    match piece.geometry.data().segment {
        stock::Segment::Line { a, b } => kernel::PathSeg::Line { a, b },
        stock::Segment::Arc { a, center, axis, angle } => kernel::PathSeg::Arc { a, center, axis, angle },
    }
}

/// Name the sketch curve a path refusal points at, so the user can find the bad pick.
fn path_error(path: &[Piece], e: stock::Failure) -> DocError {
    let id = |i: usize| path.get(i).map_or("?", |p| p.id.as_str());
    let message = match e {
        stock::Failure::Geometry(stock::Refusal::Connection(i)) => {
            format!("continuous member: `{}` is not connected end to end in path order; pick the curves in order along the path", id(i))
        }
        stock::Failure::Geometry(stock::Refusal::Tangency(i)) => {
            format!("continuous member: `{}` is not tangent to the previous curve; draw a bend arc", id(i))
        }
        stock::Failure::Geometry(stock::Refusal::Bend(i)) => format!("arc `{}`: the bend radius is too small for the anchored profile", id(i)),
        e => e.to_string(),
    };
    DocError::Invalid(message)
}

/// The exact geometry of sketch curve or 3D wire `id`: a line, or an open circular arc.
/// Splines and other curves are refused; display polylines never decide the kind.
fn curve_piece(ss: &SolvedSketch, id: &str) -> Result<Piece> {
    let sk = &ss.sketch;
    let geometry = if let Some(w) = sk.wire_index(id).and_then(|i| sk.wires.get(i)) {
        if let Some(arc) = w.arc {
            stock::spatial_arc(arc.start, arc.end, arc.center, arc.normal, arc.radius).map_err(|e| DocError::Invalid(format!("arc `{id}`: {e}")))?
        } else if let Some(line) = wire_line(w)? {
            line
        } else {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an exact circular arc")));
        }
    } else if let Some(ci) = sk.curve_index(id) {
        if !sk.curves.get(ci).is_some_and(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. })) {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc")));
        }
        let segments = sk.segs(ci);
        let [segment] = segments.as_slice() else {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc")));
        };
        stock::planar(&ss.plane, segment).map_err(|e| DocError::Invalid(format!("curve `{id}`: {e}")))?
    } else {
        return Err(DocError::Unknown(format!("path curve `{id}` in {}", ss.name)));
    };
    Ok(Piece { id: id.into(), geometry })
}

/// The member paths of a feature: one single-curve path per member, or one oriented,
/// connected and tangent path when `continuous` is set (which needs explicit `ids`).
fn member_paths(ss: &SolvedSketch, ids: &[String], continuous: bool) -> Result<Vec<Vec<Piece>>> {
    if ids.is_empty() {
        if continuous {
            return Err(DocError::Invalid("a continuous member needs explicit `lines` in path order".into()));
        }
        return Ok(default_member_lines(ss)?.into_iter().map(|p| vec![p]).collect());
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
    let mut geometry: Vec<_> = pieces.iter().map(|p| p.geometry).collect();
    stock::orient(&mut geometry).map_err(|e| path_error(&pieces, e))?;
    for (p, geometry) in pieces.iter_mut().zip(geometry) {
        p.geometry = geometry;
    }
    Ok(vec![pieces])
}

/// Drawn straight lines with a length. Splines are never classified from display samples.
fn default_member_lines(ss: &SolvedSketch) -> Result<Vec<Piece>> {
    let sk = &ss.sketch;
    let mut out = Vec::new();
    for (ci, c) in sk.curves.iter().enumerate() {
        if !c.construction && !c.centerline && c.link.is_none() && matches!(c.kind, CurveKind::Line { .. }) {
            let segments = sk.segs(ci);
            if let [segment] = segments.as_slice() {
                match stock::planar(&ss.plane, segment) {
                    Ok(geometry) => out.push(Piece { id: c.id.clone(), geometry }),
                    Err(stock::Failure::Geometry(stock::Refusal::Line)) => {}
                    Err(e) => return Err(path_error(&[], e)),
                }
            }
        }
    }
    for w in sk.wires.iter().filter(|w| w.link.is_none()) {
        if let Some(geometry) = wire_line(w)? {
            out.push(Piece { id: w.id.clone(), geometry });
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

/// A drawn line's defining points, or one linked exact line on its retained interval.
fn wire_line(w: &solidcraft_sketch::Wire) -> Result<Option<stock::Piece>> {
    if w.arc.is_some() {
        return Ok(None);
    }
    let line = if w.link.is_none()
        && w.exact.is_empty()
        && let [a, b] = w.fit.as_slice()
    {
        stock::line(*a, *b)
    } else if let [solidcraft_sketch::WireCurve { curve: solidcore_record::brep::Curve::Line { start, end }, interval }] = w.exact.as_slice() {
        stock::retained_line(Vec3::from(*start), Vec3::from(*end), *interval)
    } else {
        return Ok(None);
    };
    match line {
        Ok(line) => Ok(Some(line)),
        Err(stock::Failure::Geometry(stock::Refusal::Line)) => Ok(None),
        Err(e) => Err(path_error(&[], e)),
    }
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
