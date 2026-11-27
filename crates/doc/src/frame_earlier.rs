//! Earlier Frame Members' tubes as cutting tools (#4003 Goal 6, decisions 1-2): what Fit Ends to Earlier Frames
//! fits a later Frame Member's ends to.
//!
//! **How a copy records its source.** It doesn't, on the body: a copy is a new [`ModelBody`] whose `feature` is the
//! copying feature, its geometry the source's moved by `kernel::transform_matrix`.
//! - `ModelBody.source_transform` is not a copy's transform: it is where an assembly placement or a component frame
//!   puts a body (`in_frame`, placed occurrences, `body_transfer`), and it stays identity through Move, Mirror and
//!   Pattern.
//! - The lineage names the source only for face names: `moved_copy` steps the copy from
//!   `Operand::Copy { body, instance }` (the source's name), and a copied feature's own faces become
//!   `Operand::CopiedTool { feature, instance }` (`Traced::copied`). Neither keeps the transform, and a body's
//!   lineage is only the feature's own (`ModelBody.made`).
//! - The transform lives in the copying feature alone: a Move's `translate`/`rotate_axis`/`origin`/`angle` (a
//!   Move without `copy` moves the body in place, which keeps its name and its `feature`); a Pattern's
//!   `pattern_transforms`; a Mirror's `mirror_matrix` of its plane. A Pattern or Mirror of bodies copies the bodies
//!   it names, one of features (`replay`, `copied_features`) copies every body a Frame Member or a Move copy among
//!   those features made.
//!
//! So [`earlier_tools`] walks the timeline up to the feature, as evaluation did: a Frame Member's bodies start at its
//! own place; each Move, Pattern and Mirror after it moves or copies them by its own transforms. A copy may come
//! from any of the bodies its feature copied, by any of its transforms: each is a pose, (source Frame Member, matrix
//! from its frame to the body's, and the member when it is known). A Frame Member's own body, moved or not, is its
//! own member ([`super::member_of_body`]); a copy is matched to the member and pose it is exactly: points in that
//! member's material a quarter, half and three quarters along it, placed by the pose, all lie inside the body (exact
//! point classification, never a mesh), so a member crossing the body (an X brace's other diagonal) is not it. A
//! known member is checked the same way at its middle. A piece of a tube an overlap split (`L` or `L>X`, #4003), or a
//! known member whose middle is gone, is checked at the middle of the body's own span along the member instead, and
//! its tool is the member's cut to that span (if the kernel can't cut it, it is not fitted to). Only
//! then is the member's tool built (once per member in a call) and placed (once per pose). This also refuses a body
//! moved some way the walk doesn't follow (Align, Rest, a transfer to another component, a mirror at a placement): it
//! is not fitted to, rather than fitted to in the wrong place.
//!
//! The tool is the source member as built ([`super::member_tool`]): its feature's members laid out and its corner
//! plane cuts made as [`super::member_section`] makes them, then filled and squared ([`frame_multi_trim::fit_tool`]); for
//! a Coped frame (#17854), filled with its radii kept ([`frame_multi_trim::coped_tool`]), whatever the earlier frame says.
//! Under Auto its members are its pieces (#4003 stage 3, [`super::pieced`]): each piece is a member as above, its
//! span, quarter points and tool its own, so a copy of a piece is matched to that piece. A piece's end at a crossing
//! is fitted, so its tool stops at its line's end there, as a T-junction's does.
//! Earlier tubes and their copies are only read, never changed.

use std::collections::{BTreeMap, HashMap, HashSet};

use solidcore_record::{Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, PointClass};

use super::super::{ModelState, mirror_matrix, pattern_transforms, point, val};
use super::{
    Axes, CutEnd, Layout, Reach, copes, frame_ends, frame_member_id, gap_value, layout, member_line_ids, member_lines_of, member_tool, members_at,
    overlap_overrides, overlap_parent, overrides, pieced, placer, plans, section_axes, section_reach, turned_at,
};
use crate::expr::{Kind, Value};
use crate::frame::MAX_MEMBERS;
use crate::lineage::Traced;
use crate::{DocError, Document, Feature, FeatureKind, Mat, Result};

#[cfg(test)]
thread_local! {
    /// How many member tools [`Members::tool`] has built on this thread.
    pub(super) static TOOL_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A body's place: the Frame Member it is a tube of, the matrix from that feature's frame to the body's, and which
/// of its members the body is, when known (a Frame Member's own body, not a copy).
type Pose = (u64, Mat, Option<usize>);

/// Every tube made before feature `f` by a FrameMember feature, and every moved / mirrored / patterned copy of
/// one, as a cutting tool placed where that body is now: (body name, tool), and each one's id as an overlap piece's id
/// names it (ruling O4): a Frame Member's own tube `<feature id>:<member id>` ([`frame_member_id`]), a copy (or a tube
/// whose member isn't found) its body name; and each one's [`Reach`], its member's line placed where the tool is.
/// Tools are built once per call, each the tube's real shape when `f` copes ([`copes`]), grown by the weld gap `gap`
/// (the fitted frame's, for what its ends are fitted to; 0 for the tubes as they are).
pub fn earlier_tools(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    gap: f64,
) -> Result<(Vec<(String, Traced)>, Vec<String>, Vec<Reach>)> {
    let Some(here) = doc.features.iter().position(|g| g.id == f.id) else { return Ok(Default::default()) };
    let coped = copes(&f.kind);
    // Each Frame Member's own tube's id.
    let mut ids_of: HashMap<String, String> = HashMap::new();
    // Each tracked body's feature (a Frame Member, or what copied it) and the poses it may have.
    let mut tracked: BTreeMap<String, (u64, Vec<Pose>)> = BTreeMap::new();
    // The bodies that are pieces of a tube an overlap split (#4003): `L` and its `L>X`s.
    let mut pieces: HashSet<String> = HashSet::new();
    for g in doc.features.iter().take(here).filter(|g| !g.suppressed) {
        let made: Vec<String> = st.bodies.iter().filter(|b| b.feature == g.id).map(|b| b.name.clone()).collect();
        let (named, features, mats): (&[crate::BodyName], &[crate::FeatureId], Vec<Mat>) = match &g.kind {
            FeatureKind::FrameMember { .. } => {
                let ids = member_line_ids(st, &g.kind).unwrap_or_default();
                // Each body's member: an overlap piece `L>X` is a piece of L.
                let refs: Vec<&str> = made.iter().map(String::as_str).collect();
                // One frame back from `f`, as [`super::member_of_body`] of `f`'s pieces finds them.
                let found = members_at(doc, st, g, &refs, 1).unwrap_or_else(|_| vec![None; made.len()]);
                let lines: Vec<(String, Option<String>, bool)> = made
                    .into_iter()
                    .zip(found)
                    .map(|(n, id)| {
                        if let Some(id) = &id {
                            ids_of.insert(n.clone(), frame_member_id(g.id, id));
                        }
                        let parent = id.as_ref().and_then(|id| overlap_parent(&ids, id)).cloned();
                        let piece = parent.is_some();
                        (n, parent.or(id), piece)
                    })
                    .collect();
                let split: HashSet<String> = lines.iter().filter(|l| l.2).filter_map(|l| l.1.clone()).collect();
                for (n, line, _) in lines {
                    if line.as_ref().is_some_and(|l| split.contains(l)) {
                        pieces.insert(n.clone());
                    }
                    let k = line.and_then(|l| ids.iter().position(|i| *i == l));
                    tracked.insert(n, (g.id, vec![(g.id, crate::IDENTITY, k)]));
                }
                continue;
            }
            FeatureKind::Move { bodies, copy: false, .. } => {
                let r = move_matrix(vals, &g.kind);
                for n in bodies {
                    if let Some((_, poses)) = tracked.get_mut(n.as_ref()) {
                        // A move the walk can't work out: those bodies' places are no longer known.
                        *poses = r.map_or_else(Vec::new, |r| poses.iter().map(|(s, m, k)| (*s, crate::mat_mul(&r, m), *k)).collect());
                    }
                }
                continue;
            }
            FeatureKind::Move { bodies, .. } => (bodies.as_slice(), &[], move_matrix(vals, &g.kind).into_iter().collect()),
            FeatureKind::Pattern { features, pattern, bodies, .. } => {
                (bodies.as_slice(), features.as_slice(), pattern_transforms(doc, vals, st, g.component, pattern, bodies).unwrap_or_default())
            }
            FeatureKind::Mirror { features, plane, bodies, combine: false, .. } => {
                let mirror = doc.resolve_plane_in(vals, plane, 0, g.component, Some(st)).map(|pl| mirror_matrix(&pl));
                (bodies.as_slice(), features.as_slice(), mirror.into_iter().collect())
            }
            _ => continue,
        };
        // The copies' poses: each source's, moved by each transform, once each.
        let sources = tracked.values().filter(|(by, _)| features.iter().any(|s| s.0 == *by));
        let sources = sources.chain(named.iter().filter_map(|n| tracked.get(n.as_ref())));
        let mut seen = HashSet::new();
        let mut poses = Vec::new();
        for (s, m, _) in sources.flat_map(|(_, poses)| poses) {
            for t in &mats {
                // A copy may be of any member the feature copied.
                let pose = (*s, crate::mat_mul(t, m), None);
                if poses.len() < MAX_MEMBERS && seen.insert((pose.0, bits(&pose.1))) {
                    poses.push(pose);
                }
            }
        }
        if !poses.is_empty() {
            for n in made {
                tracked.insert(n, (g.id, poses.clone()));
            }
        }
    }
    let into_f = |component: u64| -> Option<Mat> {
        if component == f.component {
            return Some(crate::IDENTITY);
        }
        crate::mat_inverse(&doc.component_transform(f.component)).map(|inv| crate::mat_mul(&inv, &doc.component_transform(component)))
    };
    let mut members: HashMap<u64, Option<Members>> = HashMap::new();
    let mut placed: HashMap<(u64, usize, [u64; 16]), Traced> = HashMap::new();
    let mut out = Vec::new();
    let mut ids = Vec::new();
    let mut reach = Vec::new();
    for b in &st.bodies {
        let Some((_, poses)) = tracked.get(&b.name) else { continue };
        let (Some(to_f), false, Ok(bounds)) = (into_f(b.component), b.body.is_mesh(), b.body.exact_bounds()) else { continue };
        'body: for (g, m, known) in poses {
            // An earlier feature that no longer lays out has no tubes to fit to.
            let Some(source) =
                members.entry(*g).or_insert_with(|| doc.feature(*g).and_then(|g| Members::of(doc, vals, g, st, (coped, gap)).ok())).as_mut()
            else {
                continue;
            };
            let near = |lo: f64, x: f64, hi: f64| lo - 1e-6 <= x && x <= hi + 1e-6;
            let (lo, hi) = (bounds.min, bounds.max);
            // A body the kernel can't classify against is not fitted to, rather than failing this frame.
            let inside = |q: &Vec3| {
                let p = crate::apply_point(m, *q);
                near(lo.x, p.x, hi.x) && near(lo.y, p.y, hi.y) && near(lo.z, p.z, hi.z) && matches!(b.body.classify_point(p), Ok(PointClass::Inside))
            };
            let candidates: Vec<usize> = match known {
                Some(k) => vec![*k],
                // Longest first: a short piece past a crossing (#4003 ruling C4) may have all three points inside the
                // tube it crosses, whose copy is that tube's, not the piece's.
                None => {
                    let mut all: Vec<usize> = (0..source.points.len()).collect();
                    let length = |k: &usize| source.segments.get(*k).map_or(0.0, |(_, a, b)| a.dist(*b));
                    all.sort_by(|j, k| length(k).total_cmp(&length(j)));
                    all
                }
            };
            for k in candidates {
                let (Some(&points), Some((a, z))) = (source.points.get(k), source.segments.get(k).map(|s| (s.1, s.2))) else { continue };
                let spanned = match known {
                    // A copy: all three points.
                    None if points.iter().all(inside) => false,
                    None => continue,
                    // A member's own body, whole: its middle, as before overlaps (#4003).
                    Some(_) if !pieces.contains(&b.name) && points.get(1).is_some_and(inside) => false,
                    // A piece of a tube an overlap split, or a body a cut or a notch left without its middle: the point
                    // at the middle of the body's own span along the member (points run linearly along it), and the
                    // tool cut to that span.
                    Some(_) => {
                        let (pa, pz) = (crate::apply_point(m, a), crate::apply_point(m, z));
                        let Some((lo, hi)) = extent((pa, pz), &bounds, &crate::IDENTITY) else { continue };
                        let t = (lo + hi) * 0.5 / pa.dist(pz).max(f64::MIN_POSITIVE);
                        let [p1, _, p3] = points;
                        if !inside(&(p1 + (p3 - p1) * ((t - 0.25) / 0.5))) {
                            continue;
                        }
                        true
                    }
                };
                let at = crate::mat_mul(&to_f, m);
                let tool = match placed.entry((*g, k, bits(&at))) {
                    std::collections::hash_map::Entry::Occupied(e) => e.get().clone(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        let base = source.tool(k)?;
                        let body = if crate::is_identity(&at) { base.clone() } else { kernel::transform_matrix(base, at)? };
                        e.insert(crate::lineage::numbered_tool(body)).clone()
                    }
                };
                let tool = if spanned {
                    // A tool the kernel can't cut to the span is not fitted to, rather than failing this frame.
                    match span(&tool, (crate::apply_point(&at, a), crate::apply_point(&at, z)), &bounds, &to_f) {
                        Ok(t) => t,
                        Err(_) => continue,
                    }
                } else {
                    tool
                };
                out.push((b.name.clone(), tool));
                ids.push(ids_of.get(&b.name).cloned().unwrap_or_else(|| b.name.clone()));
                reach.push((crate::apply_point(&at, a), crate::apply_point(&at, z), source.reach(k) * stretch(&at)));
                break 'body;
            }
        }
    }
    Ok((out, ids, reach))
}

/// How much `m` may lengthen a distance: 1 for a rigid move or a mirror, else infinite (no pair is then ruled out by
/// distance).
fn stretch(m: &Mat) -> f64 {
    let [x, y, z] = [Vec3::X, Vec3::Y, Vec3::Z].map(|v| crate::apply_vector(m, v));
    let unit = [x, y, z].iter().all(|v| (v.len() - 1.0).abs() < 1e-9);
    let square = x.dot(y).abs() < 1e-9 && y.dot(z).abs() < 1e-9 && z.dot(x).abs() < 1e-9;
    if unit && square { 1.0 } else { f64::INFINITY }
}

/// How far along the line `a`…`b` the box `piece` begins and ends, its corners placed by `to`.
fn extent((a, b): (Vec3, Vec3), piece: &solidcraft_geom::Aabb3, to: &Mat) -> Option<(f64, f64)> {
    let d = (b - a).normalized()?;
    let (lo, hi) = (piece.min, piece.max);
    let corners = [lo.x, hi.x].into_iter().flat_map(|x| [lo.y, hi.y].into_iter().flat_map(move |y| [lo.z, hi.z].map(|z| Vec3::new(x, y, z))));
    let (from, to) =
        corners.map(|c| (crate::apply_point(to, c) - a).dot(d)).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), t| (l.min(t), h.max(t)));
    (from.is_finite() && to.is_finite()).then_some((from, to))
}

/// `tool` (a member's, placed, its line `a`…`b`) cut square to that line where the box `piece` (in its body's
/// component frame, which `to_f` places) begins and ends along it: the tool of a piece of a tube an overlap split
/// (#4003). What it loses lies in the tube that cut the piece.
fn span(tool: &Traced, (a, b): (Vec3, Vec3), piece: &solidcraft_geom::Aabb3, to_f: &Mat) -> Result<Traced> {
    let d = (b - a).normalized().ok_or_else(|| DocError::Invalid("an earlier tube's line has no length".into()))?;
    let (from, to) = extent((a, b), piece, to_f).ok_or_else(|| DocError::Invalid("an earlier piece has no span".into()))?;
    let mut body = tool.body.clone();
    for (at, normal) in [(from, d), (to, -d)] {
        let plane =
            solidcore_record::Plane::from_normal(a + d * at, normal).ok_or_else(|| DocError::Invalid("an earlier piece has no span".into()))?;
        body = kernel::half_space(&body, &plane, true)?.0.ok_or_else(|| DocError::Invalid("an earlier piece's tool is empty".into()))?;
    }
    Ok(crate::lineage::numbered_tool(body))
}

/// A Move's rigid transform, `None` when its values don't evaluate.
fn move_matrix(vals: &BTreeMap<String, Value>, kind: &FeatureKind) -> Option<Mat> {
    let FeatureKind::Move { translate: [x, y, z], rotate_axis, origin, angle, .. } = kind else { return None };
    let len = |e: &str| val(vals, e, Kind::Length).ok();
    let t = Vec3::new(len(x)?, len(y)?, len(z)?);
    let angle = match angle {
        Some(a) => val(vals, a, Kind::Angle).ok()?,
        None => 0.0,
    };
    let origin = match origin {
        Some(p) => point(vals, p).ok()?,
        None => Vec3::ZERO,
    };
    Some(crate::rigid(t, origin, rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z), angle))
}

/// A matrix's bits, to key it by.
fn bits(m: &Mat) -> [u64; 16] {
    let mut out = [0u64; 16];
    for (slot, v) in out.iter_mut().zip(m.iter().flatten()) {
        *slot = v.to_bits();
    }
    out
}

/// An earlier Frame Member's members, laid out and cut at their corners as [`super::member_section`] lays them, in
/// its own frame (its corners set back by its own weld gap): points in each one's material a quarter, half and three
/// quarters along it, and each one's tool once built, its real shape when `coped` (the frame fitted to it copes) and
/// grown by `gap` (that frame's weld gap).
struct Members {
    groups: Vec<Members>,
    segments: Vec<(String, Vec3, Vec3)>,
    regions: Vec<Region2>,
    axes: Vec<Axes>,
    /// Each member's profile point on its line, a turned member's moved ([`turned_at`]).
    ats: Vec<Vec2>,
    ends: Vec<[Option<CutEnd>; 2]>,
    points: Vec<[Vec3; 3]>,
    tools: Vec<Option<kernel::Body>>,
    coped: bool,
    gap: f64,
}

impl Members {
    fn of(doc: &Document, vals: &BTreeMap<String, Value>, g: &Feature, st: &ModelState, (coped, gap): (bool, f64)) -> Result<Members> {
        if let Some(features) = crate::frame::group_features(g)? {
            let groups = features.iter().map(|f| Self::of(doc, vals, f, st, (coped, gap))).collect::<Result<Vec<_>>>()?;
            let segments = groups.iter().flat_map(|g| g.segments.iter().cloned()).collect();
            let points = groups.iter().flat_map(|g| g.points.iter().copied()).collect();
            return Ok(Members {
                groups,
                segments,
                points,
                regions: Vec::new(),
                axes: Vec::new(),
                ats: Vec::new(),
                ends: Vec::new(),
                tools: Vec::new(),
                coped,
                gap,
            });
        }
        let FeatureKind::FrameMember { corner, corners, gap: own_gap, .. } = &g.kind else {
            return Err(DocError::Invalid("not a frame member feature".into()));
        };
        let Layout { segments, regions, at, turn, sketch_x, drawn, planes, turns, centre, up, .. } = layout(doc, vals, g, st)?;
        let own_gap = gap_value(vals, own_gap.as_ref(), "`gap`")?;
        let corners = &overrides(*corner, (&segments, &planes, &drawn, sketch_x), corners, Some((vals, own_gap)))?;
        // ponytail: an earlier frame's corner or chain corner that its own Fit Ends left Free is cut here as jointed
        // (finding it would build the tools of the frames before that one too); the tool then reaches on inside the
        // tube that corner lies in, which the fitted end is cut by anyway.
        let (plan, crossing) = plans(*corner, &segments, &planes, corners, &drawn)?;
        let overlaps = overlap_overrides(*corner, &segments, &planes, corners);
        // Where crossings cut its lines its members are their pieces, as `member_line_ids` lists them.
        // Its members turned by its Rotate steps (#15070), as it built them.
        let own: Vec<f64> =
            member_lines_of(&segments, plan.as_ref().or(crossing.as_ref())).iter().map(|l| turns.get(l).copied().unwrap_or(0.0)).collect();
        let ats: Vec<Vec2> = own.iter().map(|t| turned_at(at, centre, *t)).collect();
        let (segments, planes, drawn) = pieced(segments, planes, drawn, plan.as_ref().or(crossing.as_ref()))?;
        let (axes, _) = section_axes(&segments, &planes, plan.as_ref(), turn, &own, up)?;
        let inside = regions.first().map(|r| r.interior_point()).ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
        let (ends, points) = {
            let placed = placer(&axes, &ats);
            let ends =
                frame_ends(&g.kind, corners, &segments, &regions, &placed, (plan.or(crossing), &planes, &overlaps), (sketch_x, &drawn), own_gap)?;
            let along = |m: usize, a: Vec3, b: Vec3, t: f64| placed(m, a + (b - a) * t).to_world(inside);
            (ends, segments.iter().enumerate().map(|(m, (_, a, b))| [0.25, 0.5, 0.75].map(|t| along(m, *a, *b, t))).collect())
        };
        let tools = vec![None; segments.len()];
        Ok(Members { groups: Vec::new(), segments, regions, axes, ats, ends, points, tools, coped, gap })
    }

    /// How far member m's tool reaches from its line: its section, and its ends built past the line's.
    fn reach(&self, mut m: usize) -> f64 {
        for group in &self.groups {
            if m < group.segments.len() {
                return group.reach(m);
            }
            m = m.saturating_sub(group.segments.len());
        }
        let ext = self.ends.get(m).map_or(0.0, |e| e.iter().flatten().map(|c| c.1).fold(0.0, f64::max));
        section_reach(&self.regions, self.ats.get(m).copied().unwrap_or(Vec2::ZERO)) + ext
    }

    /// Member m's tool, built the first time it is asked for.
    fn tool(&mut self, mut m: usize) -> Result<&kernel::Body> {
        for group in &mut self.groups {
            if m < group.segments.len() {
                return group.tool(m);
            }
            m = m.saturating_sub(group.segments.len());
        }
        let slot = self.tools.get_mut(m).ok_or_else(|| DocError::Invalid(format!("no member {m}")))?;
        if slot.is_none() {
            #[cfg(test)]
            TOOL_BUILDS.set(TOOL_BUILDS.get() + 1);
            let placed = placer(&self.axes, &self.ats);
            *slot = Some(member_tool(&self.segments, &self.regions, &placed, &self.ends, m, self.coped, self.gap)?.body);
        }
        slot.as_ref().ok_or_else(|| DocError::Invalid(format!("no member {m}")))
    }
}

#[cfg(test)]
#[path = "frame_earlier_tests.rs"]
mod tests;
