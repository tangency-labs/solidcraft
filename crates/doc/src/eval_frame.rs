//! Frame members (#4003): one body per straight line of a frame sketch, and per pair of points
//! (#15705), its cross-section a profile laid square to the line (`section_axes`) and run
//! from one end to the other.

use solidcraft_geom::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::{Aabb2, Ring};
use solidcraft_kernel::{self as kernel, BoolOp};
use solidcraft_sketch::{CurveKind, Sketch};

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, select_profiles, sketch_in_feature_frame, val};
use crate::expr::{Expr, Kind, Value};
use crate::frame::{self, CornerOverride, CornerType, MAX_MEMBERS, MemberGroup, MemberPair, MemberProfile, MemberRef};
use crate::frame_corners::{self, Corner, Cut, Override};
use crate::frame_layout::{self, ChainCorner, Fit, Joint, MemberPlane, Plan};
use crate::frame_multi_trim;
use crate::lineage::Traced;
use crate::{DocError, Document, Feature, FeatureId, FeatureKind, ProfileSel, Result};

#[path = "frame_earlier.rs"]
mod earlier;
#[path = "frame_paths.rs"]
mod paths;
#[path = "eval_frame_trim.rs"]
mod trimming;

pub(super) fn trim(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    trimming::trim(doc, vals, f, st, warning)
}

/// Only rebuildable ordinary members emit the records general Trim needs. Auto and path
/// bodies deliberately keep no nominal Trim records; their absence is valid in saved states.
pub(super) fn needs_trim_records(st: &ModelState, kind: &FeatureKind) -> Result<bool> {
    Ok(trimming::can_rebuild(kind) && !paths::uses(st, kind)?)
}

/// Most profiles a profile sketch may have for its material to be worked out.
const MAX_PROFILE_PIECES: usize = 256;
/// Shortest member (mm).
const MIN_LENGTH: f64 = 1e-6;

/// Evaluate a `FrameMember` feature: one new body per member (a line, a pair, or under every type but None each
/// piece of a line cut where tubes cross, [`pieced`], but for a piece left wholly inside the tubes it is fitted to),
/// each named ([`member_names`], or by position for a copy whose `body_names` were set). Under every type but None, a
/// tube that still overlaps another is notched or split where it gives way ([`resolve_overlaps`]), a later piece named
/// `<its name> · <the other's>`. An error about a piece names it by its line and the tube it starts past
/// ([`pieces_by_line`]).
pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let emitted_from = st.bodies.len();
    let path_route = paths::uses(st, &f.kind)?;
    evaluate_members(doc, vals, f, st)?;
    // Use the final naming/identity pass: Auto frames may split, notch or drop members.
    // Metadata must not change whether the frame itself can build.
    let names: Vec<String> = st.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.to_string()).collect();
    let picked: Vec<&str> = names.iter().map(String::as_str).collect();
    if let Ok(lines) = members_of_bodies(doc, st, f, &picked)
        && lines.len() == names.len()
    {
        for (body, line) in st.bodies.iter_mut().filter(|b| b.feature == f.id).zip(lines) {
            body.frame_member_line = line.map(Into::into);
        }
    }
    certify_stock(doc, vals, f, st, emitted_from, path_route);
    Ok(())
}

// Preserve main's complete flat/grouped evaluation before adding optional provenance.
fn evaluate_members(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    if paths::uses(st, &f.kind)? {
        return paths::evaluate(doc, vals, f, st);
    }
    if let Some(groups) = frame::group_features(f)? {
        let FeatureKind::FrameMember { profile, names, .. } = &f.kind else {
            return Err(DocError::Invalid("not a frame member feature".into()));
        };
        let (ids, _) = member_parts(st, &f.kind)?;
        let default = profile_regions(vals, st, profile)?.1;
        let bases = group_bases(&f.kind, &default, ids.len());
        let pinned = ids.iter().cloned().zip(group_member_names(st, &ids, &bases, names)).collect::<BTreeMap<_, _>>();
        let (start, counter) = (st.bodies.len(), st.body_counter);
        let records = st.frame_members.len();
        let result = (|| {
            let mut offset = 0;
            for mut group in groups {
                if let FeatureKind::FrameMember { names, .. } = &mut group.kind {
                    names.clone_from(&pinned);
                }
                let before = st.bodies.len();
                built(doc, vals, &group, st, offset)?;
                offset += st.bodies.len().saturating_sub(before);
            }
            Ok(())
        })();
        if result.is_err() {
            st.bodies.truncate(start);
            st.body_counter = counter;
            st.frame_members.truncate(records);
        }
        return result;
    }
    built(doc, vals, f, st, 0).map_err(|e| pieces_by_line(st, f, e))
}

/// `e` with each later piece of a cut line (`L>X`, [`LaterPieces`]) told as "the piece of `L` past `X`" (ruling C4):
/// a piece is no line of the sketch.
fn pieces_by_line(st: &ModelState, f: &Feature, e: DocError) -> DocError {
    let DocError::Invalid(mut msg) = e else { return e };
    let Ok((ids, later)) = member_parts(st, &f.kind) else { return DocError::Invalid(msg) };
    for (id, (l, x)) in ids.iter().zip(&later).filter_map(|(id, p)| Some((id, (*p)?))) {
        let (Some(l), Some(x)) = (ids.get(l), ids.get(x)) else { continue };
        let piece = format!("the piece of `{l}` past `{x}`");
        msg = msg.replace(&format!("the member on line `{id}`"), &piece).replace(&format!("line `{id}`"), &piece);
    }
    DocError::Invalid(msg)
}

/// [`evaluate`], its errors naming pieces by their ids.
fn built(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, copy_offset: usize) -> Result<()> {
    let FeatureKind::FrameMember { body_name, names, corner, flip, corners, fit_ends, gap, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { segments, regions, description, at, turn, sketch_x, drawn, planes, turns, centre, up, .. } = layout(doc, vals, f, st)?;
    if corners.iter().flat_map(|c| &c.members).any(|m| m.feature.as_ref().is_some_and(|n| *n != f.name)) {
        return Err(DocError::Invalid("not supported yet: a corner with a member of another frame".into()));
    }
    let gap = gap_value(vals, gap.as_ref(), "`gap`")?;
    let records = corners;
    let corners = &overrides(*corner, (&segments, &planes, &drawn, sketch_x), records, Some((vals, gap)))?;
    let coped = copes(&f.kind);
    let (mut plan, crossing) = plans(*corner, &segments, &planes, corners, &drawn)?;
    // The plan that cuts the lines into pieces: Auto's, or the crossings' of a Mitre, Butt or Coped frame.
    let cutting = plan.as_ref().or(crossing.as_ref());
    let parts = cutting.map(|p| later_pieces(&p.pieces, &segments)).unwrap_or_default();
    // Every type but None resolves the tubes that still overlap ([`resolve_overlaps`]): the overrides that name an
    // overlap, and each member's line (a piece's, its line's).
    let resolving = !corner.is_none();
    let overlap_overrides = overlap_overrides(*corner, &segments, &planes, corners);
    if overlap_overrides.iter().any(|&k| records.get(k).is_some_and(|r| r.gap.is_some())) {
        return Err(DocError::Invalid("A corner override of tubes that overlap takes no `gap`: an overlap is notched, not welded".into()));
    }
    let lines = member_lines_of(&segments, cutting);
    let (segments, planes, drawn) = pieced(segments, planes, drawn, cutting)?;
    let crossed = cutting.map(cut_pieces).unwrap_or_default();
    // Each member's place in the laying order: a piece takes its line's.
    let mut laying = vec![usize::MAX; if resolving { segments.len() } else { 0 }];
    if resolving {
        let ranked = cutting.map_or_else(|| frame_layout::ranks(&segments, &planes, &drawn), |p| p.ranks.clone());
        for (i, r) in ranked.iter().enumerate() {
            if let Some(p) = laying.get_mut(r.member) {
                *p = i;
            }
        }
    }
    // Each member's Rotate turn (#15070), by its line: a piece of a turned line turns with it.
    let own: Vec<f64> = lines.iter().map(|l| turns.get(l).copied().unwrap_or(0.0)).collect();
    let ats: Vec<Vec2> = own.iter().map(|t| turned_at(at, centre, *t)).collect();
    let (axes, plain) = section_axes(&segments, &planes, plan.as_ref(), turn, &own, up)?;
    let (placed, unreversed) = (placer(&axes, &ats), unreversed_placer(&plain, &ats));
    // Fit Ends to Earlier Frames (None never fits): the earlier Frame Members' tubes and their copies, as cutting
    // tools, and this frame's own tubes laid before each free end (ruling A14), whether or not an earlier one exists.
    let fits = *fit_ends && !corner.is_none();
    let (earlier, earlier_ids, earlier_reach) = if fits { earlier::earlier_tools(doc, vals, f, st, 0.0)? } else { Default::default() };
    // What free ends are fitted to stands off by this frame's gap: the earlier tubes grown by it.
    let grown_earlier = if fits && gap > 0.0 { Some(earlier::earlier_tools(doc, vals, f, st, gap)?.0) } else { None };
    if let Some(plan) = plan.as_mut() {
        free_corners(plan, &earlier, &placed, centre);
    }
    // How far a member's tool may reach from its line: four times its section's farthest point from the line, plus
    // its corner extensions (its cut `ends`) and 1 mm.
    let section = regions
        .iter()
        .flat_map(|r| &r.outer.segs)
        .flat_map(|g| [g.start(), g.point_at(0.5), g.end()])
        .flat_map(|q| ats.iter().map(move |at| q.dist(*at)))
        .fold(0.0, f64::max);
    let near_with = |ends: &[[Option<CutEnd>; 2]], k: usize, p: Vec3| {
        let ext = ends.get(k).map_or(0.0, |e| e.iter().flatten().map(|c| c.1).fold(0.0, f64::max));
        segments.get(k).is_some_and(|(_, a, b)| {
            let d = *b - *a;
            let t = ((p - *a).dot(d) / d.dot(d).max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
            p.dist(*a + d * t) <= 4.0 * section + ext + 1.0
        })
    };
    // This frame's own members' cut ends, to build their tools from, and each tool once built.
    let mut own_ends = if fits {
        frame_ends(
            &f.kind,
            corners,
            &segments,
            &regions,
            &placed,
            (plan.clone().or(crossing.clone()), &planes, &overlap_overrides),
            (sketch_x, &drawn),
            gap,
        )?
    } else {
        Vec::new()
    };
    let mut tools: Vec<Option<kernel::Body>> = vec![None; segments.len()];
    if fits && let Some(plan) = plan.as_mut() {
        // A chain corner that turns Free changes only its chain's members' ends, never a group member's (whose tools
        // it builds): the tools built stay right.
        let tool = |k: usize| member_tool(&segments, &regions, &placed, &own_ends, k, coped, 0.0);
        if free_chains(plan, &earlier, (&placed, centre), &|k, p| near_with(&own_ends, k, p), &tool, &mut tools, &segments)? {
            own_ends = frame_ends(
                &f.kind,
                corners,
                &segments,
                &regions,
                &placed,
                (Some(plan.clone()), &planes, &overlap_overrides),
                (sketch_x, &drawn),
                gap,
            )?;
        }
    }
    // Those tools are the tubes as they are; a free end is fitted to them grown by the gap.
    if gap > 0.0 {
        tools = vec![None; segments.len()];
    }
    let unjointed = if fits { unjointed_ends(&f.kind, &segments, plan.as_ref().or(crossing.as_ref()), (&planes, &drawn)) } else { Vec::new() };
    // This frame's own members, for its free ends: their laying order (Auto's, else the drawing order, never the
    // pick order), and their cut ends to build their tools from.
    let mut laid = vec![usize::MAX; segments.len()];
    let order: Vec<usize> = match plan.as_ref() {
        Some(plan) => plan.ranks.iter().map(|r| r.member).collect(),
        None => {
            let mut order: Vec<usize> = (0..segments.len()).collect();
            order.sort_by_key(|&m| (drawn.get(m).copied().unwrap_or(m), m));
            order
        }
    };
    for (i, m) in order.into_iter().enumerate() {
        if let Some(p) = laid.get_mut(m) {
            *p = i;
        }
    }
    // The pairs a corner, crossing or fit joins: no overlap between them is resolved.
    let joined = if resolving { joints(&segments, &planes, &drawn, plan.as_ref(), crossing.as_ref()) } else { HashSet::new() };
    // Only an Auto frame builds by Auto's rules; an Auto override elsewhere is the frame's own type (corner_ends).
    // The cut ends each member is built with ([`frame_ends`] gives the same), for the tools of tubes that overlap others.
    let (mut cut, dropped, ends) = if let Some(plan) = plan {
        auto_members(&segments, &regions, (&placed, &unreversed), plan, (sketch_x, &drawn), coped, gap)?
    } else if corner.is_none() && corners.is_empty() {
        // A frame with no corners to finish is built exactly as it always was.
        (Vec::new(), Vec::new(), Vec::new())
    } else {
        let tie = (planes.as_slice(), sketch_x, drawn.as_slice());
        let overrides = (corners.as_slice(), overlap_overrides.as_slice());
        let ((ends, copes), fits) = corner_ends(&segments, &regions, &placed, (*corner, *flip, gap), overrides, tie, crossing.as_ref(), coped)?;
        let mut cut = cut_members(&segments, &regions, (&placed, &|_, _| None), &ends, copes)?;
        let mut dropped = vec![false; segments.len()];
        fit_members(&segments, &regions, &placed, &ends, &fits, (&mut cut, &crossed, &mut dropped), (coped, gap))?;
        (cut, dropped, ends)
    };
    // Each member's body; `None` for a piece left wholly inside the tubes it is fitted to.
    let mut made = Vec::with_capacity(segments.len());
    for (m, (id, a, b)) in segments.iter().enumerate() {
        if dropped.get(m) == Some(&true) {
            made.push(None);
            continue;
        }
        if let Some(t) = cut.get_mut(m).and_then(Option::take) {
            made.push(Some(t));
            continue;
        }
        let (x, y) = axes.get(m).copied().ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?;
        let at = ats.get(m).copied().unwrap_or(at);
        let plane = Plane::new(*a - x * at.x - y * at.y, x, y).ok_or_else(|| DocError::Invalid(format!("line `{id}`: no plane square to it")))?;
        made.push(Some(member_body(plane, unreversed(m, *a), &regions, *b - *a, (0.0, a.dist(*b)), id)?));
    }
    // A point in the section's material (a tube's wall, not its bore), half way along member m.
    let inside = regions.first().map(|r| r.interior_point()).ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    let middle = |m: usize| segments.get(m).map_or(Vec3::ZERO, |(_, a, b)| placed(m, (*a + *b) * 0.5).to_world(inside));
    let near = |k: usize, p: Vec3| near_with(&own_ends, k, p);
    let tool = |k: usize| member_tool(&segments, &regions, &placed, &own_ends, k, coped, gap);
    let fitting = grown_earlier.as_deref().unwrap_or(&earlier);
    fit_to_earlier(&mut made, &segments, &unjointed, fitting, &mut Own { laid, near: &near, tool: &tool, tools }, &middle, &crossed)?;
    let mut split = if resolving {
        // How far each member reaches from its line: its section, and its ends built past the line's.
        let section = ats.iter().map(|at| section_reach(&regions, *at)).fold(0.0, f64::max);
        let reach: Vec<f64> =
            (0..segments.len()).map(|m| section + ends.get(m).map_or(0.0, |e| e.iter().flatten().map(|c| c.1).fold(0.0, f64::max))).collect();
        // Overlaps are notched, never welded: no gap.
        let tool = |k: usize| member_tool(&segments, &regions, &placed, &ends, k, coped, 0.0);
        let overrides = (corners.as_slice(), overlap_overrides.as_slice());
        let earlier = Earlier { tools: &earlier, reach: &earlier_reach };
        resolve_overlaps(&mut made, &segments, (&laying, &lines, &joined, &reach), overrides, &earlier, &tool)?
    } else {
        Vec::new()
    };
    let mut later_of = |m: usize| split.get_mut(m).map(std::mem::take).unwrap_or_default();
    if !f.body_names.is_empty() {
        // A copy (pasted, or in a copied component) names its members by position, as any
        // feature's bodies.
        let all: Vec<Traced> = made.into_iter().enumerate().flat_map(|(m, t)| t.into_iter().chain(later_of(m).into_iter().map(|p| p.1))).collect();
        for (k, t) in all.into_iter().enumerate() {
            let name = new_name(st, f, copy_offset + k);
            if trimming::can_rebuild(&f.kind)
                && let Some((line, ..)) = segments.get(k)
            {
                st.frame_members.push(trimming::record(f.id, line, &name, [None, None], &t.body));
            }
            st.bodies.push(ModelBody::traced_new(name, t, f.id));
        }
        return Ok(());
    }
    let base = body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or(description, str::to_string);
    let ids: Vec<String> = segments.into_iter().map(|(id, _, _)| id).collect();
    let named = member_names(st, &ids, &parts, names, &base);
    let piece_pins: Vec<(&String, &str)> = names.iter().filter(|(k, _)| k.contains('>')).map(|(k, n)| (k, n.trim())).collect();
    // A piece with no body leaves its name unused.
    for (m, (name, t)) in named.iter().zip(made).enumerate() {
        if let Some(t) = t {
            if trimming::can_rebuild(&f.kind)
                && let Some(line) = ids.get(m)
            {
                st.frame_members.push(trimming::record(f.id, line, name, [None, None], &t.body));
            }
            st.bodies.push(ModelBody::traced_new(name.clone(), t, f.id));
        }
        // A later piece of a tube split where another overlaps it, `L>X` (X: the other's line or pair id, a piece's
        // line's, or an earlier tube's id, [`earlier::earlier_tools`]): its pinned name, else `<its name> · <the
        // other's>`, a piece's line's name, so a crossing elsewhere on that line renames nothing ([`member_of_body`]
        // finds it by either).
        for (by, t) in later_of(m) {
            let (x, x_name) = match by {
                Cutter::Own(k) => {
                    let line = lines.get(k);
                    (line, line.and_then(|l| ids.iter().position(|i| i == l)).and_then(|k| named.get(k)))
                }
                Cutter::Earlier(e) => (earlier_ids.get(e), earlier.get(e).map(|e| &e.0)),
            };
            let id = format!("{}>{}", ids.get(m).map_or("", String::as_str), x.map_or("", String::as_str));
            // A name pinned to another piece is kept for it, whichever of the two is named first; of two
            // pinned to one name, the first named takes it.
            let own = names.get(&id).map(|n| n.trim());
            let free = |n: &String| {
                st.body(n).is_none() && !named.contains(n) && !piece_pins.iter().any(|(k, p)| **k != id && *p == n.as_str() && Some(*p) != own)
            };
            let pinned = names.get(&id).map(|n| n.trim().to_string()).filter(|n| !n.is_empty() && free(n));
            let joined = format!("{name} · {}", x_name.map_or("", String::as_str));
            let piece = (1..=MAX_MEMBERS + 1).map(|k| if k == 1 { joined.clone() } else { format!("{joined} ({k})") }).find(free);
            st.bodies.push(ModelBody::traced_new(pinned.or(piece).unwrap_or(joined), t, f.id));
        }
    }
    Ok(())
}

/// Whether frame `kind` copes (#17854): Coped ticked, under every type but None. Its butts are cut as Coped Butt
/// ([`CornerType::coped`]), and every tool it fits, notches or splits a tube by is the other tube's real shape
/// ([`member_tool`]).
pub(super) fn copes(kind: &FeatureKind) -> bool {
    matches!(kind, FeatureKind::FrameMember { coped: true, corner, .. } if !corner.is_none())
}

/// A weld gap as saved (none: 0), evaluated and checked, `what` naming it: a design file is hostile input.
fn gap_value(vals: &BTreeMap<String, Value>, e: Option<&crate::Expr>, what: &str) -> Result<f64> {
    e.map_or(Ok(0.0), |e| frame::checked_gap(val(vals, e, Kind::Length)?, what))
}

/// What cuts a tube where it overlaps another ([`resolve_overlaps`]): one of this frame's members, or an earlier Frame
/// Member's tube (an index into the earlier tools).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cutter {
    Own(usize),
    Earlier(usize),
}

/// Least overlap (mm³) that counts: tubes touching face to face, as fitted ends do, don't overlap.
const MIN_OVERLAP: f64 = 1e-6;

/// Whether two bodies overlap, `ba` and `bb` their boxes: boxes sharing a volume first, then exact (the kernel's
/// intersection, of more than [`MIN_OVERLAP`]), never a mesh.
fn overlapping(a: &kernel::Body, ba: &solidcraft_geom::Aabb3, b: &kernel::Body, bb: &solidcraft_geom::Aabb3) -> bool {
    let both = ba.intersection(bb);
    let size = both.max - both.min;
    // Written as `!(… > …)`, so a box that isn't finite shares nothing.
    if !(size.x > 1e-6 && size.y > 1e-6 && size.z > 1e-6) {
        return false;
    }
    // ponytail: a pair the kernel can't intersect or measure is left overlapping, as the frame built before; report it
    // if one turns up.
    matches!(kernel::boolean(a, b, BoolOp::Intersect), Ok(Some(c)) if kernel::measure_mass(&c).is_ok_and(|m| m.volume > MIN_OVERLAP))
}

/// A tube's line and how far its tube reaches from it (mm): its section's farthest point from the line, plus how far
/// an end is built past the line's end.
pub(super) type Reach = (Vec3, Vec3, f64);

/// Whether two tubes are too far apart to overlap: their lines farther apart than both reaches and 1 mm. A reach
/// that isn't finite never rules a pair out.
// ponytail: a capsule round each line, so crossing diagonals with wide boxes skip the exact test; a pair whose
// capsules meet still pays for it.
fn apart((a, b, r): Reach, (c, d, s): Reach) -> bool {
    let (p, q) = solidcraft_geom::mesh_distance::closest_on_segments(a, b, c, d);
    p.dist(q) > r + s + 1.0
}

/// How far the section `regions` reaches from the point `at` that lies on a member's line, at most: its outlines'
/// line ends, arcs' centres plus radii and curves' control points. Infinite when a number isn't finite.
pub(super) fn section_reach(regions: &[Region2], at: Vec2) -> f64 {
    let far = |pts: &[Vec2]| pts.iter().map(|p| p.dist(at)).fold(0.0, f64::max);
    let mut most = 0.0f64;
    for g in regions.iter().flat_map(|r| &r.outer.segs) {
        let r = match *g {
            Seg2::Line { a, b } => far(&[a, b]),
            Seg2::Arc { center, radius, .. } => center.dist(at) + radius.abs(),
            Seg2::Cubic { p0, p1, p2, p3 } => far(&[p0, p1, p2, p3]),
            // A rational quadratic of positive weight lies in its control points' hull.
            Seg2::Conic { a, apex, b, w } if w > 0.0 => far(&[a, apex, b]),
            Seg2::Conic { .. } => f64::INFINITY,
            Seg2::Bezier { n, p } => far(p.get(..=usize::from(n)).unwrap_or(&p)),
        };
        most = if r.is_finite() { most.max(r) } else { f64::INFINITY };
    }
    most
}

/// The earlier Frame Members' tubes a frame's members give way to ([`earlier::earlier_tools`]), each with its
/// [`Reach`]. A tube Fit Ends has fitted an end to is tested again: it may also overlap the member past the end, as a
/// tube lying along an earlier one does.
struct Earlier<'a> {
    tools: &'a [(String, Traced)],
    reach: &'a [Reach],
}

/// Each member's later pieces where tubes overlapping it split it, each with the tube that cut it at its start.
type Split = Vec<Vec<(Cutter, Traced)>>;

/// The frame's tubes that still overlap once corners, crossings and fits are made (#4003), resolved: tubes in no
/// sketch plane crossing others, and tubes that only clip each other where their lines miss. `made`: each member as
/// built (`None`: no body). Of two members that overlap and that nothing joins (`joined`, [`joints`]; never two pieces
/// of one line), the later in the laying order (`place`) gives way, or the earlier where an override names their two lines
/// (`lines`) with Flip; with Fit Ends, each member also gives way to every earlier Frame Member's tube it overlaps
/// (`earlier`, which never change). Two tubes too far apart to meet (each member's `reach` from its line, [`apart`])
/// are never tested. The member giving way is cut by the other's filled tool (`tool(k)`, [`member_tool`]; the earlier
/// tool itself), so only over their overlap; a tool or cut the kernel can't make leaves the piece as it was. One piece
/// left is a notch, in `made` as before; two or more, a split: the first from its line's drawn start stays in `made`,
/// and the others come back in order along it, each with the tube cut at its start; a piece wholly inside the tool is
/// gone. The cut faces are named `overlap:<tube>:face:<k>`. Every override in `overrides` listed in `pending` (an
/// overlap's, [`overlap_overrides`]) must name two members that overlap.
fn resolve_overlaps(
    made: &mut [Option<Traced>],
    segments: &[(String, Vec3, Vec3)],
    (place, lines, joined, reach): (&[usize], &[String], &HashSet<(usize, usize)>, &[f64]),
    (overrides, pending): (&[Override], &[usize]),
    earlier: &Earlier,
    tool: &dyn Fn(usize) -> Result<Traced>,
) -> Result<Split> {
    let n = segments.len();
    let bounds: Vec<Option<solidcraft_geom::Aabb3>> = made.iter().map(|t| t.as_ref().and_then(|t| t.body.exact_bounds().ok())).collect();
    let earlier_bounds: Vec<Option<solidcraft_geom::Aabb3>> = earlier.tools.iter().map(|(_, t)| t.body.exact_bounds().ok()).collect();
    let body = |m: usize| made.get(m).and_then(Option::as_ref).zip(bounds.get(m).copied().flatten());
    // Member m's line and reach; one not known reaches everywhere.
    let own =
        |m: usize| segments.get(m).map_or((Vec3::ZERO, Vec3::ZERO, f64::INFINITY), |s| (s.1, s.2, reach.get(m).copied().unwrap_or(f64::INFINITY)));
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&m| (place.get(m).copied().unwrap_or(usize::MAX), m));
    let line = |m: usize| lines.get(m).map_or("", String::as_str);
    let names = |o: &Override, i: usize, j: usize| match o.members.as_slice() {
        [a, b] => (a == line(i) && b == line(j)) || (a == line(j) && b == line(i)),
        _ => false,
    };
    // What cuts each member, in order: earlier frames' tubes, laid before any of this frame's, then its own by place.
    let mut cuts: Vec<Vec<Cutter>> = vec![Vec::new(); n];
    for &i in &order {
        let (Some((t, bi)), Some(slot)) = (body(i), cuts.get_mut(i)) else { continue };
        for (e, (_, tool)) in earlier.tools.iter().enumerate() {
            if earlier.reach.get(e).is_some_and(|&r| apart(own(i), r)) {
                continue;
            }
            if earlier_bounds.get(e).copied().flatten().is_some_and(|be| overlapping(&t.body, &bi, &tool.body, &be)) {
                slot.push(Cutter::Earlier(e));
            }
        }
    }
    let mut used = vec![false; overrides.len()];
    for (k, &i) in order.iter().enumerate() {
        for &j in order.get(..k).unwrap_or_default() {
            if line(i) == line(j) || joined.contains(&(i.min(j), i.max(j))) || apart(own(i), own(j)) {
                continue;
            }
            let (Some((ti, bi)), Some((tj, bj))) = (body(i), body(j)) else { continue };
            if !overlapping(&ti.body, &bi, &tj.body, &bj) {
                continue;
            }
            let mut flip = false;
            for (u, o) in used.iter_mut().zip(overrides).filter(|(_, o)| names(o, i, j)) {
                *u = true;
                flip |= o.flip;
            }
            let (giving, by) = if flip { (j, i) } else { (i, j) };
            if let Some(c) = cuts.get_mut(giving) {
                c.push(Cutter::Own(by));
            }
        }
    }
    if pending.iter().any(|&k| used.get(k) == Some(&false)) {
        return Err(DocError::Invalid("A corner override is not at a corner of the frame".into()));
    }
    // Each cutting tool once, its faces named for the tube it is; `None` for one the kernel can't build.
    let mut tools: BTreeMap<Cutter, Option<Traced>> = BTreeMap::new();
    let mut out: Split = (0..n).map(|_| Vec::new()).collect();
    let mut members = made.iter().flatten().count();
    for (m, by) in cuts.iter().enumerate().filter(|(_, c)| !c.is_empty()) {
        let (Some(slot), Some((_, a, b))) = (made.get_mut(m), segments.get(m)) else { continue };
        let Some(member) = slot.take() else { continue };
        let d = *b - *a;
        let mut pieces: Vec<(Option<Cutter>, Traced)> = vec![(None, member)];
        for &c in by {
            let cutting = tools.entry(c).or_insert_with(|| {
                let (x, body) = match c {
                    Cutter::Own(k) => (segments.get(k).map_or("", |s| s.0.as_str()), tool(k).ok()?.body),
                    Cutter::Earlier(k) => earlier.tools.get(k).map(|(x, t)| (x.as_str(), t.body.clone()))?,
                };
                let roles = (0..body.face_count()).map(|k| vec![format!("overlap:{x}:face:{k}")]).collect();
                Some(Traced::made(body, Some(roles)))
            });
            // ponytail: a tool or cut the kernel can't make leaves the piece uncut, overlapping as the frame built
            // before; report it if one turns up.
            let Some((cutting, reach)) = cutting.as_ref().and_then(|t| Some((t, t.body.exact_bounds().ok()?))) else { continue };
            let mut next = Vec::with_capacity(pieces.len() + 1);
            for (start, piece) in pieces {
                if !piece.body.exact_bounds().is_ok_and(|b| b.meets(&reach)) {
                    next.push((start, piece));
                    continue;
                }
                let cut = match kernel::boolean(&piece.body, &cutting.body, BoolOp::Cut) {
                    Ok(Some(cut)) => Traced::step(cut, &[&piece, cutting]),
                    // Nothing left: the piece lies wholly inside the tool.
                    Ok(None) => continue,
                    Err(_) => {
                        next.push((start, piece));
                        continue;
                    }
                };
                match ordered(&cut, *a, d) {
                    // The first from the drawn start keeps the piece's own start; each other starts at this tube.
                    Some(along) if along.len() > 1 => {
                        next.extend(along.into_iter().enumerate().map(|(k, t)| (if k == 0 { start } else { Some(c) }, t)))
                    }
                    Some(_) => next.push((start, cut)),
                    None => next.push((start, piece)),
                }
            }
            pieces = next;
            if members + pieces.len() > MAX_MEMBERS + 1 {
                return Err(too_many_pieces());
            }
        }
        let mut pieces = pieces.into_iter();
        *slot = pieces.next().map(|p| p.1);
        let later: Vec<(Cutter, Traced)> = pieces.map(|(start, t)| (start.unwrap_or(Cutter::Own(m)), t)).collect();
        members += later.len();
        if let Some(o) = out.get_mut(m) {
            *o = later;
        }
    }
    Ok(out)
}

/// `cut`'s lumps in order along the line from `a` along `d`, each placed by its box's centre (exact and cheap): one
/// lump left empty, and `None` when the kernel can't split or bound them.
fn ordered(cut: &Traced, a: Vec3, d: Vec3) -> Option<Vec<Traced>> {
    let lumps = cut.body.lumps().ok()?;
    if lumps.len() < 2 {
        return Some(Vec::new());
    }
    let lineages = crate::lineage::Lineage::pieces(&cut.body, cut.lineage.as_ref(), &lumps);
    let mut along = Vec::with_capacity(lumps.len());
    for (body, lineage) in lumps.into_iter().zip(lineages) {
        let at = body.exact_bounds().ok()?.center();
        along.push(((at - a).dot(d), Traced { body, lineage, recipe: None }));
    }
    along.sort_by(|p, q| p.0.total_cmp(&q.0));
    Some(along.into_iter().map(|p| p.1).collect())
}

/// The one body of an extruded member.
fn one_solid(mut bodies: Vec<kernel::Body>, id: &str) -> Result<kernel::Body> {
    match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => Ok(body),
        _ => Err(DocError::Invalid(format!("the member on line `{id}` is not one solid"))),
    }
}

/// A member's cut end: its plane cut (`None`: a coped or fitted end, built out but not cut by a plane), how
/// far past the end it is built, whether an End Cap may go on it (a through member's end of
/// a Butt or Coped corner, cut square to its own line), and whether the member as built keeps that length (a plane
/// cut or fitted end does; a coped end is trimmed back to the tube it meets), so its cutting tool does too.
type CutEnd = (Option<Cut>, f64, bool, bool);

/// Each member's cut at its [start, end] and each Coped corner's (T, U, corner point, n, weld gap). Shared
/// by [`cut_members`] and [`member_section`], so both see the same corners.
type CornerEnds = (Vec<[Option<CutEnd>; 2]>, Vec<(usize, usize, Vec3, Vec3, f64)>);

/// What breaks a tie for which member runs through: the sketch's X axis (`None`: a 3D sketch) and
/// each member's place in the sketch's drawing order.
type Tie<'a> = (Option<Vec3>, &'a [usize]);

/// Stage 1's ends ([`frame_corners::find_corners`], each corner finished as the frame or its override says), and under
/// Mitre, Butt and Coped those at each point where three or more members end ([`crowded`]): its corner finished as the
/// frame's, every other end there fitted (`(None, reach, false, true)`, [`fit_reach`], as Auto's), with the fits in laying order, and each end on
/// another member's middle fitted to it ([`tees`]). `planes`: each
/// member's plane, for the laying order. `crossing`: the frame's crossings ([`plans`]), whose pieces `segments` are:
/// each end where a crossing cuts a line is fitted as Auto fits it ([`crossing_fits`]), a piece ranks by its whole
/// line (its place in the laying order, its line's length for which member of a butt runs through), a corner is named by
/// its two lines (crossings rule: an override names lines, so one at a cut line's far end still holds), and a
/// crossing's override names no corner. Nor does an overlap's (`overlaps`, [`overlap_overrides`]). Under `coped`
/// ([`copes`]) every Butt, the frame's or an override's, is cut as Coped Butt. `finish`: the frame's type, Flip and
/// weld gap; a corner an override names takes the override's, as a crossing's fitted ends do theirs ([`Fit::gap`]).
fn corner_ends(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    finish: (CornerType, bool, f64),
    (overrides, overlaps): (&[Override], &[usize]),
    (planes, sketch_x, drawn): (&[MemberPlane], Option<Vec3>, &[usize]),
    crossing: Option<&Plan>,
    coped: bool,
) -> Result<(CornerEnds, Vec<Fit>)> {
    let finish = (finish.0.coped(coped), finish.1, finish.2);
    // A design file is hostile input: the command checks the list, a file may not.
    if overrides.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} corner overrides in one feature")));
    }
    fn pair<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
        if a <= b { (a, b) } else { (b, a) }
    }
    // Member m's line: under `crossing`, the id of its line's first piece, which is the line's own.
    let line = |m: usize| -> &str {
        let first = crossing.and_then(|p| p.pieces.get(m).and_then(|pc| p.pieces.iter().find(|q| q.parent == pc.parent)));
        first.map_or_else(|| segments.get(m).map_or("", |s| s.0.as_str()), |q| q.id.as_str())
    };
    let corners = frame_corners::find_corners(segments);
    let at: HashMap<(&str, &str), usize> = corners
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let [(a, _), (b, _)] = c.ends;
            (pair(line(a), line(b)), k)
        })
        .collect();
    let mut finishes = vec![finish; corners.len()];
    // A piece ranks by its whole line, as Auto ranks it.
    let length = |m: usize| line_length(segments, crossing, m);
    // A crossing's override (its Flip used by the crossing plan) names no corner, nor does an overlap's.
    let crossing_override = |k: usize| crossing.is_some_and(|p| p.crossing_overrides.contains(&k));
    let named = |(k, _): &(usize, &Override)| !crossing_override(*k) && !overlaps.contains(k);
    for o in overrides.iter().enumerate().filter(named).map(|(_, o)| o) {
        // Two members: the corner they meet at. Three or more are an Auto frame's alone (which never comes here).
        let [a, b] = o.members.as_slice() else {
            if o.members.len() < 2 {
                return Err(DocError::Invalid("A corner override names at least two members".into()));
            }
            return Err(DocError::Invalid("A corner override is not at a corner of the frame".into()));
        };
        let k = at.get(&pair(a, b)).ok_or_else(|| DocError::Invalid("A corner override is not at a corner of the frame".into()))?;
        if let Some(f) = finishes.get_mut(*k) {
            // An Auto override on a frame that isn't Auto is the frame's own type, with its own Flip.
            *f = (if o.corner == CornerType::Auto { finish.0 } else { o.corner.coped(coped) }, o.flip, o.gap);
        }
    }
    // Each member's cut ends, and each Coped Butt's (T, U): U loses T's filled shape once every plane cut is made.
    let mut out: CornerEnds = (vec![[None, None]; segments.len()], Vec::new());
    for (c, (kind, flip, gap)) in corners.iter().zip(finishes) {
        let cuts = frame_corners::corner_cuts(kind, flip, gap, c, segments, &length, placed, regions, sketch_x, drawn)?;
        add_corner(&mut out, c, kind.flips(), cuts, segments, regions, placed, gap)?;
    }
    // None leaves every point as it is. No override names a point of three or more ends (Auto's alone).
    let (crowded, fits) = if finish.0.is_none() { (Vec::new(), Vec::new()) } else { stage_fits(segments, planes, drawn, crossing) };
    for c in &crowded {
        let cuts = frame_corners::corner_cuts(finish.0, finish.1, finish.2, c, segments, &length, placed, regions, sketch_x, drawn)?;
        add_corner(&mut out, c, finish.0.flips(), cuts, segments, regions, placed, finish.2)?;
    }
    for f in &fits {
        if let Some(e) = out.0.get_mut(f.member).and_then(|e| e.get_mut(usize::from(!f.at_start)))
            && e.is_none()
        {
            *e = Some((None, fit_reach(segments, regions, placed, f), false, true));
        }
    }
    Ok((out, fits))
}

/// Under Mitre, Butt and Coped (#4003), each point where three or more members end and none passes through
/// ([`frame_corners::find_crowded`]): the two laid first ([`frame_layout::ranks`]: outer tubes first, longest first,
/// never the pick order; under `crossing`, whose pieces `segments` are, its ranks: each piece in its whole line's
/// place, as Auto lays them) make its corner, its ends by member index as stage 1's, and each other end there is cut to
/// fit the tubes laid before it there, as Auto fits the third end at a point. The corners, and the fits in laying
/// order.
fn crowded(segments: &[(String, Vec3, Vec3)], planes: &[MemberPlane], drawn: &[usize], crossing: Option<&Plan>) -> (Vec<Corner>, Vec<Fit>) {
    let points = frame_corners::find_crowded(segments);
    if points.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut laid = vec![usize::MAX; segments.len()];
    let ranked = crossing.map_or_else(|| frame_layout::ranks(segments, planes, drawn), |p| p.ranks.clone());
    for (i, r) in ranked.iter().enumerate() {
        if let Some(p) = laid.get_mut(r.member) {
            *p = i;
        }
    }
    let place = |m: usize| (laid.get(m).copied().unwrap_or(usize::MAX), m);
    let (mut corners, mut fits) = (Vec::new(), Vec::new());
    for (point, mut ends) in points {
        ends.sort_by_key(|e| place(e.0));
        let [a, b, rest @ ..] = ends.as_slice() else { continue };
        let mut pair = [*a, *b];
        pair.sort_unstable();
        corners.push(Corner { point, ends: pair });
        for (j, e) in rest.iter().enumerate() {
            fits.push(Fit { member: e.0, at_start: e.1, against: ends.iter().take(j + 2).map(|e| e.0).collect(), gap: None });
        }
    }
    fits.sort_by_key(|f| place(f.member));
    (corners, fits)
}

/// Under Mitre, Butt and Coped: the corners where three or more members end ([`crowded`]), and every end cut to fit:
/// there, where a crossing cuts a line ([`crossing_fits`]), and at a T-junction ([`tees`]).
fn stage_fits(segments: &[(String, Vec3, Vec3)], planes: &[MemberPlane], drawn: &[usize], crossing: Option<&Plan>) -> (Vec<Corner>, Vec<Fit>) {
    let (corners, mut fits) = crowded(segments, planes, drawn, crossing);
    fits.extend(crossing.into_iter().flat_map(|p| crossing_fits(p, segments)).cloned());
    // An end at a corner (stage 1's, or where three or more end) is finished there, never fitted again as a tee.
    let at_corners = frame_corners::find_corners(segments).into_iter().map(|c| c.ends).chain(corners.iter().map(|c| c.ends)).flatten();
    let fitted: HashSet<(usize, bool)> = fits.iter().map(|f| (f.member, f.at_start)).chain(at_corners).collect();
    fits.extend(tees(segments, planes, drawn, crossing, &fitted));
    (corners, fits)
}

/// Under Mitre, Butt and Coped (#4003 ruling O1), each end of a member lying on another member's middle that nothing
/// else fits (`fitted`): a T-junction. The member ending there gives way, whatever its place in the laying order and
/// whether or not Fit Ends is ticked: its end is cut to fit the members passing there and the ends there laid before it
/// (as [`crowded`] lays them), as Auto fits a T-junction.
fn tees(
    segments: &[(String, Vec3, Vec3)],
    planes: &[MemberPlane],
    drawn: &[usize],
    crossing: Option<&Plan>,
    fitted: &HashSet<(usize, bool)>,
) -> Vec<Fit> {
    let tol = frame_corners::CORNER_TOL;
    let ends: Vec<(usize, bool, Vec3)> = segments.iter().enumerate().flat_map(|(m, (_, a, b))| [(m, true, *a), (m, false, *b)]).collect();
    // ponytail: every end against every member, O(n²) on at most MAX_MEMBERS; a grid as `meetings` has if it shows.
    let found: Vec<(usize, bool, Vec3, Vec<usize>)> = ends
        .iter()
        .filter(|e| !fitted.contains(&(e.0, e.1)))
        .filter_map(|&(m, at_start, p)| {
            let passing: Vec<usize> = (segments.iter().enumerate())
                .filter(|(k, (_, a, b))| *k != m && p.dist_to_segment(*a, *b) <= tol && p.dist(*a) > tol && p.dist(*b) > tol)
                .map(|(k, _)| k)
                .collect();
            (!passing.is_empty()).then_some((m, at_start, p, passing))
        })
        .collect();
    if found.is_empty() {
        return Vec::new();
    }
    let mut laid = vec![usize::MAX; segments.len()];
    let ranked = crossing.map_or_else(|| frame_layout::ranks(segments, planes, drawn), |p| p.ranks.clone());
    for (i, r) in ranked.iter().enumerate() {
        if let Some(p) = laid.get_mut(r.member) {
            *p = i;
        }
    }
    let place = |m: usize| (laid.get(m).copied().unwrap_or(usize::MAX), m);
    found
        .into_iter()
        .map(|(m, at_start, p, mut against)| {
            against.extend(ends.iter().filter(|e| e.0 != m && e.2.dist(p) <= tol && place(e.0) < place(m)).map(|e| e.0));
            against.sort_by_key(|&k| place(k));
            against.dedup();
            Fit { member: m, at_start, against, gap: None }
        })
        .collect()
}

/// The pairs of members (lower index first) a corner, chain corner, crossing or fit joins (#4003 ruling O1): under
/// Auto, its `plan`'s, and every pair where an end of one lies on the other's line ([`ends_meet`], ruling O2: Auto's
/// T-junctions its plan leaves unfitted overlap as they always did); else stage 1's corners and [`stage_fits`] over
/// `crossing`'s pieces, where two members only touching (an end on the other's line with nothing jointing them) are
/// not joined.
fn joints(
    segments: &[(String, Vec3, Vec3)],
    planes: &[MemberPlane],
    drawn: &[usize],
    plan: Option<&Plan>,
    crossing: Option<&Plan>,
) -> HashSet<(usize, usize)> {
    let pair = |a: usize, b: usize| (a.min(b), a.max(b));
    let fitted = |fits: &[Fit]| fits.iter().flat_map(|f| f.against.iter().map(move |&k| pair(f.member, k))).collect::<Vec<_>>();
    match plan {
        Some(p) => {
            let corners = p.corners.iter().map(|c| pair(c.long.0, c.short.0));
            let n = segments.len();
            let touching = (0..n).flat_map(|i| (i + 1..n).filter(move |&j| ends_meet(segments, i, j)).map(move |j| (i, j)));
            corners.chain(p.chained.iter().map(|c| pair(c.a.0, c.b.0))).chain(fitted(&p.fits)).chain(touching).collect()
        }
        None => {
            let (crowded, fits) = stage_fits(segments, planes, drawn, crossing);
            let corners = frame_corners::find_corners(segments).into_iter().chain(crowded);
            corners.map(|c| pair(c.ends[0].0, c.ends[1].0)).chain(fitted(&fits)).collect()
        }
    }
}

/// A corner's cuts as [`frame_corners::corner_cuts`] gives them, put into `ends` (each cut end with how far past
/// its line's end it is built) and `copes`. `butt`: the first cut is a butt's T's, square to its line, which must not
/// run back past U's far end, and is the one cut end an End Cap may go on. `gap`: the weld gap a cope stands off by.
fn add_corner(
    (ends, copes): &mut CornerEnds,
    c: &Corner,
    butt: bool,
    (cuts, cope): (Vec<(usize, bool, Cut)>, Option<(usize, usize)>),
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    gap: f64,
) -> Result<()> {
    let id = |m: usize| segments.get(m).map_or("", |s| s.0.as_str());
    for (k, (m, at_start, cut)) in cuts.into_iter().enumerate() {
        let away = frame_corners::away(segments, (m, at_start));
        let ext = away
            .and_then(|w| frame_corners::extension(&placed(m, c.point), regions, c.point, w, &cut))
            .ok_or_else(|| DocError::Invalid(format!("the member on line `{}` can't be built out to its corner cut", id(m))))?;
        // A butt's first cut is T's. T must not run back past the far end of U, the corner's other member.
        if k == 0 && butt {
            let [(e0, _), (e1, _)] = c.ends;
            let u = if e0 == m { e1 } else { e0 };
            let u_len = segments.get(u).map_or(0.0, |(_, a, b)| a.dist(*b));
            let u_reach = away.map_or(0.0, |w| frame_corners::reach(&placed(u, c.point), regions, c.point, -w));
            if ext - 1.0 > u_len + u_reach {
                return Err(frame_corners::corner_error(c.point, "too flat to butt"));
            }
        }
        // A butt's first cut is T's, square to its line: the one cut end a cap may go on.
        let cap = k == 0 && butt;
        if let Some(end) = ends.get_mut(m).and_then(|e| e.get_mut(usize::from(!at_start))) {
            *end = Some((Some(cut), ext, cap, true));
        }
    }
    let Some((t, u)) = cope else { return Ok(()) };
    // U is built out across the whole of T, to past T's far side: the cope trims it back.
    let (te, ue) = (frame_corners::end_at(c, t)?, frame_corners::end_at(c, u)?);
    // n: square to T, towards U's side of it; T's near face lies `near` along it.
    let reached = match (frame_corners::away(segments, te), frame_corners::away(segments, ue)) {
        (Some(wt), Some(wu)) => (wu - wt * wu.dot(wt)).normalized().and_then(|n| {
            let far = Cut { origin: c.point - n * frame_corners::reach(&placed(t, c.point), regions, c.point, -n), normal: n };
            let near = frame_corners::reach(&placed(t, c.point), regions, c.point, n);
            frame_corners::extension(&placed(u, c.point), regions, c.point, wu, &far).map(|ext| (n, near, ext))
        }),
        _ => None,
    };
    let (n, near, ext) = reached.ok_or_else(|| DocError::Invalid(format!("the member on line `{}` can't be built out to its corner", id(u))))?;
    // U wholly inside T (its far end short of T's near face) keeps nothing of its own.
    let far_end = segments.get(u).map(|(_, a, b)| if ue.1 { *b } else { *a });
    if far_end.is_none_or(|q| (q - c.point).dot(n) <= near) {
        return Err(DocError::Invalid(format!("the member on line `{}` was cut away completely", id(u))));
    }
    if let Some(end) = ends.get_mut(u).and_then(|e| e.get_mut(usize::from(!ue.1))) {
        *end = Some((None, ext, false, false));
    }
    copes.push((t, u, c.point, n, gap));
    Ok(())
}

/// Each member cut at its corners (#4003) as `ends` and `copes` say ([`corner_ends`], [`auto_ends`]), or `None` for
/// a member with no cut end (built square, as without corners). A cut member is built past each cut end far enough
/// to reach the whole cut, then cut there; the cut face is named `corner_start` or `corner_end`. A Coped Butt's U is
/// built out across T and, after every plane cut, loses T's filled shape, keeping its largest piece. `placed(m, p)`
/// is member m's section plane, square to its line, placed at `p`.
fn cut_members(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    (placed, unreversed): Placing,
    ends: &[[Option<CutEnd>; 2]],
    copes: Vec<(usize, usize, Vec3, Vec3, f64)>,
) -> Result<Vec<Option<Traced>>> {
    let id = |m: usize| segments.get(m).map_or("", |s| s.0.as_str());
    // Member m built out at its cut ends and cut there, from `regions`.
    let build = |m: usize, regions: &[Region2]| -> Result<Traced> {
        let ((id, a, b), [start, end]) = segments.get(m).zip(ends.get(m)).ok_or_else(|| DocError::Invalid(format!("no member {m}")))?;
        let (lo, hi) = (start.map_or(0.0, |c| c.1), end.map_or(0.0, |c| c.1));
        let mut t = member_body(placed(m, *a), unreversed(m, *a), regions, *b - *a, (-lo, a.dist(*b) + hi), id)?;
        for (cut, role) in [(start, "corner_start"), (end, "corner_end")] {
            let Some((Some(cut), _, _, _)) = cut else { continue };
            let plane =
                Plane::from_normal(cut.origin, cut.normal).ok_or_else(|| DocError::Invalid(format!("line `{id}`: a corner cut has no plane")))?;
            let (part, tool) = kernel::half_space(&t.body, &plane, true)?;
            let part = part.ok_or_else(|| DocError::Invalid(format!("the member on line `{id}` was cut away completely")))?;
            // The cut face is the cap of the half-space box in the plane: its start cap.
            let tool =
                crate::lineage::ProfileRoles { sketch: None, region: None, caps: [Some(role.into()), None], cap_synonyms: [None, None], thin: None }
                    .tool(tool, None);
            t = Traced::step(part, &[&t, &tool]);
        }
        Ok(t)
    };
    let mut out = Vec::with_capacity(segments.len());
    for (m, (id, _, _)) in segments.iter().enumerate() {
        if ends.get(m).is_none_or(|e| e.iter().all(Option::is_none)) {
            out.push(None);
            continue;
        }
        let t = build(m, regions)?;
        if t.body.lumps()?.len() > 1 {
            return Err(DocError::Invalid(format!("the member on line `{id}` is cut into pieces by its corners")));
        }
        out.push(Some(t));
    }
    // Copes after every plane cut. T filled is T built from its outlines alone, holes closed (with a weld gap, grown by
    // it: [`member_tool`]); U keeps its largest piece on its own side of T, its centre of mass on U's side (what lies
    // past T, and slivers on T's far side, go).
    let outlines = frame_multi_trim::filled_outline(regions);
    for (t, u, point, n, gap) in copes {
        let filled = if gap > 0.0 {
            member_tool(segments, regions, placed, ends, t, true, gap)?
        } else {
            crate::lineage::numbered_tool(build(t, &outlines)?.body)
        };
        let gone = || DocError::Invalid(format!("the member on line `{}` was cut away completely", id(u)));
        let traced = out.get_mut(u).and_then(Option::as_mut).ok_or_else(gone)?;
        let cut = kernel::boolean(&traced.body, &filled.body, BoolOp::Cut)?.ok_or_else(gone)?;
        let cut = Traced::step(cut, &[traced, &filled]);
        let lumps = cut.body.lumps()?;
        let lineages = crate::lineage::Lineage::lumps(&cut.body, cut.lineage.as_ref(), &lumps);
        let mut best: Option<(f64, Traced)> = None;
        for (body, lineage) in lumps.into_iter().zip(lineages) {
            let mass = kernel::measure_mass(&body)?;
            let v = mass.volume;
            if (mass.centroid - point).dot(n) > 0.0 && best.as_ref().is_none_or(|(most, _)| v > *most) {
                best = Some((v, Traced { body, lineage, recipe: None }));
            }
        }
        *traced = best.ok_or_else(gone)?.1;
    }
    Ok(out)
}

/// A frame's plans: an Auto frame's ([`frame_layout::plan`]), as only an Auto frame builds by Auto's rules and lines
/// its sections up; and a Mitre, Butt or Coped frame's crossings (#4003), cut as Auto cuts them: the same plan, of
/// which only its pieces, their ranks, the fits at its crossings ([`crossing_fits`]) and its crossing overrides are
/// used. `None` for a frame with no crossing, which builds exactly as it always did, and under None, which leaves
/// crossings overlapping.
fn plans(
    corner: CornerType,
    segments: &[(String, Vec3, Vec3)],
    planes: &[MemberPlane],
    overrides: &[Override],
    drawn: &[usize],
) -> Result<(Option<Plan>, Option<Plan>)> {
    if corner.is_none() {
        return Ok((None, None));
    }
    // A design file is hostile input: the command checks the list, a file may not.
    if overrides.len() > MAX_MEMBERS || overrides.iter().any(|o| o.members.len() > MAX_MEMBERS) {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} corner overrides of at most {MAX_MEMBERS} members in one feature")));
    }
    if corner != CornerType::Auto {
        // Every override but a crossing's names a corner, which corner_ends checks.
        let plan = frame_layout::planned(segments, planes, drawn, &|_, _| true, overrides).0;
        return Ok((None, (!plan.crossings.is_empty()).then_some(plan)));
    }
    if overrides.iter().any(|o| o.members.len() < 2) {
        return Err(DocError::Invalid("A corner override names at least two members".into()));
    }
    // An overlap's override names no point of the plan ([`resolve_overlaps`] checks it).
    let overrides: Vec<Override> = overrides.iter().filter(|o| !overlap_override(segments, planes, o)).cloned().collect();
    // One feature's members share one section.
    frame_layout::plan(segments, planes, drawn, &|_, _| true, &overrides).map(|p| (Some(p), None))
}

/// Whether an end of member i or j lies on the other's line: they meet at a corner, a fit or where a crossing cuts
/// one of them, which joins them there.
fn ends_meet(segments: &[(String, Vec3, Vec3)], i: usize, j: usize) -> bool {
    let tol = frame_corners::CORNER_TOL;
    let (Some((_, a, b)), Some((_, c, d))) = (segments.get(i), segments.get(j)) else { return false };
    a.dist_to_segment(*c, *d) <= tol || b.dist_to_segment(*c, *d) <= tol || c.dist_to_segment(*a, *b) <= tol || d.dist_to_segment(*a, *b) <= tol
}

/// Whether override `o` names two of `segments` that no end joins ([`ends_meet`]) and that don't cross in one sketch
/// plane (a planar crossing's): an overlap's, saying which of the two gives way where their tubes overlap
/// ([`resolve_overlaps`]), never a corner's or a crossing's.
fn overlap_override(segments: &[(String, Vec3, Vec3)], planes: &[MemberPlane], o: &Override) -> bool {
    let [a, b] = o.members.as_slice() else { return false };
    let at = |id: &String| segments.iter().position(|s| s.0 == *id);
    let (Some(i), Some(j)) = (at(a), at(b)) else { return false };
    if i == j || ends_meet(segments, i, j) {
        return false;
    }
    let planar = planes.get(i).is_some_and(Option::is_some) && planes.get(j).is_some_and(Option::is_some);
    let cross = || match (segments.get(i), segments.get(j)) {
        (Some((_, a, b)), Some((_, c, d))) => {
            let (p, q) = solidcraft_geom::mesh_distance::closest_on_segments(*a, *b, *c, *d);
            p.dist(q) <= frame_corners::CORNER_TOL
        }
        _ => false,
    };
    !(planar && cross())
}

/// Which of `overrides` name an overlap ([`overlap_override`]), by index, worked out on the frame's whole lines
/// `segments` before crossings cut them: the one list [`corner_ends`] and [`resolve_overlaps`] both go by. None under
/// None, which resolves no overlap.
fn overlap_overrides(corner: CornerType, segments: &[(String, Vec3, Vec3)], planes: &[MemberPlane], overrides: &[Override]) -> Vec<usize> {
    if corner.is_none() {
        return Vec::new();
    }
    overrides.iter().enumerate().filter(|(_, o)| overlap_override(segments, planes, o)).map(|(k, _)| k).collect()
}

/// A frame's whole lines as laid out ([`segments`]): each with its plane, the drawing order and the sketch's X axis.
type Lines<'a> = (&'a [(String, Vec3, Vec3)], &'a [MemberPlane], &'a [usize], Option<Vec3>);

/// Each record's lines in the order today's rules lay them with no Flip (#17854): [0] runs through (a butt's through
/// member, Auto's long tube, the line a crossing keeps whole, the tube an overlap leaves whole), [1] is the other of
/// the pair the record's first two name (`laid_pair`: the two of its lines laid first instead), then the rest in
/// laying order. `None` for a record that names no corner, crossing or overlap of the frame (the build says so) or
/// fewer than two members. The one source of [`default_order`] and of the build's Flip ([`overrides`]), each corner
/// worked out as the build works it out: Auto's plan ([`frame_layout::planned`]), stage 1's corners
/// ([`corner_ends`]: [`frame_corners::through`] by line length), chained corners as [`auto_ends`] cuts them, and the
/// laying order for crossings and overlaps.
fn orders(corner: CornerType, (segments, planes, drawn, sketch_x): Lines, records: &[CornerOverride], laid_pair: bool) -> Vec<Option<Vec<String>>> {
    let mut out = vec![None; records.len()];
    // A design file is hostile input: past the caps the build says so.
    if records.len() > MAX_MEMBERS || records.iter().any(|r| r.members.len() > MAX_MEMBERS) {
        return out;
    }
    let place: HashMap<&str, usize> =
        frame_layout::ranks(segments, planes, drawn).iter().enumerate().filter_map(|(i, r)| Some((segments.get(r.member)?.0.as_str(), i))).collect();
    let laid = |lines: &[String]| {
        let mut lines = lines.to_vec();
        lines.sort_by_key(|l| place.get(l.as_str()).copied().unwrap_or(usize::MAX));
        lines
    };
    let unflipped: Vec<Override> = records
        .iter()
        .map(|r| {
            let lines: Vec<String> = r.lines().map(str::to_string).collect();
            Override { members: if laid_pair { laid(&lines) } else { lines }, corner: r.kind, flip: false, gap: 0.0 }
        })
        .collect();
    let (plan, matched) = if corner.is_none() {
        (None, Vec::new())
    } else {
        let (plan, matched) = frame_layout::planned(segments, planes, drawn, &|_, _| true, &unflipped);
        (Some(plan), matched)
    };
    // The plan that cuts the lines into pieces, as [`plans`] keeps it.
    let cutting = plan.as_ref().filter(|p| corner == CornerType::Auto || !p.crossings.is_empty());
    let overlaps = overlap_overrides(corner, segments, planes, &unflipped);
    // A member's line, a piece's its line's.
    let line = |m: usize| match cutting {
        Some(p) => p.pieces.get(m).and_then(|pc| segments.get(pc.parent)).map_or("", |s| s.0.as_str()),
        None => segments.get(m).map_or("", |s| s.0.as_str()),
    };
    let pieces = pieced(segments.to_vec(), planes.to_vec(), drawn.to_vec(), cutting).ok();
    let stage1 = match (&pieces, corner) {
        (Some(p), c) if c != CornerType::Auto => frame_corners::find_corners(&p.0),
        _ => Vec::new(),
    };
    // Which member of corner c runs through, as `through` picks it with `length`.
    let through = |c: &Corner, length: &dyn Fn(usize) -> f64| {
        let (segs, _, drawn) = pieces.as_ref()?;
        c.ends.get(frame_corners::through(segs, c, length, sketch_x, drawn)).map(|e| e.0)
    };
    for (k, (o, slot)) in unflipped.iter().zip(out.iter_mut()).enumerate() {
        let (a, b) = match o.members.get(..2) {
            Some([a, b]) => (a.as_str(), b.as_str()),
            _ => continue,
        };
        let rest = laid(o.members.get(2..).unwrap_or_default());
        let is_pair = |x: usize, y: usize| (line(x), line(y)) == (a, b) || (line(x), line(y)) == (b, a);
        // The pair with member `first`'s line first, then the rest.
        let ordered = |first: usize| {
            let first = line(first);
            let second = if first == a { b } else { a };
            [first.to_string(), second.to_string()].into_iter().chain(rest.iter().cloned()).collect::<Vec<String>>()
        };
        *slot = if overlaps.contains(&k) || cutting.is_some_and(|p| p.crossing_overrides.contains(&k)) {
            // The one laid first stays whole.
            Some(laid(&[a.to_string(), b.to_string()]).into_iter().chain(rest).collect())
        } else if let (Some(plan), CornerType::Auto) = (plan.as_ref(), corner) {
            // A chained corner's pieces are whole lines: their own lengths rank them, as `auto_ends` cuts them.
            let chain = |ch: &ChainCorner| {
                let length = frame_corners::segment_length(&pieces.as_ref()?.0);
                through(&Corner { point: Vec3::ZERO, ends: [ch.a, ch.b] }, &length)
            };
            if let Some(c) = plan.corners.iter().find(|c| c.overridden && is_pair(c.long.0, c.short.0)) {
                Some(ordered(c.long.0))
            } else if let Some(m) = plan.chained.iter().find(|c| c.overridden && is_pair(c.a.0, c.b.0)).and_then(chain) {
                Some(ordered(m))
            } else {
                // Matched where no corner is made (one group end alone): nothing runs through.
                matched.get(k).is_some_and(|m| *m).then(|| laid(&o.members))
            }
        } else if o.members.len() == 2 {
            let length = |m: usize| pieces.as_ref().map_or(0.0, |p| line_length(&p.0, cutting, m));
            stage1.iter().find(|c| is_pair(c.ends[0].0, c.ends[1].0)).and_then(|c| through(c, &length)).map(ordered)
        } else {
            None
        };
    }
    out
}

/// `records` as the build reads them: each its lines in order, its type, Flip where its first line is not the one
/// [`orders`] puts first of its first two, and its weld gap, else the frame's (`gaps`: the values to evaluate it with and
/// the frame's gap, the gap checked; none: 0, for what cuts nothing).
fn overrides(corner: CornerType, lines: Lines, records: &[CornerOverride], gaps: Option<(&BTreeMap<String, Value>, f64)>) -> Result<Vec<Override>> {
    let orders = orders(corner, lines, records, false);
    records
        .iter()
        .zip(orders)
        .map(|(r, order)| {
            let members: Vec<String> = r.lines().map(str::to_string).collect();
            let flip = order.is_some_and(|d| d.get(..2) != members.get(..2));
            let gap = match gaps {
                Some((vals, _)) if r.gap.is_some() => gap_value(vals, r.gap.as_ref(), "a corner's `gap`")?,
                Some((_, frame)) => frame,
                None => 0.0,
            };
            Ok(Override { members, corner: r.kind, flip, gap })
        })
        .collect()
}

/// Members of the corner, crossing or overlap `members` names (any order), in the order today's rules lay them
/// ([`orders`]): [0] runs through or stays whole. Errors as evaluation does for a set that names nothing. `f` is the
/// Frame Member (its corner records aren't read), `st` the model before it.
pub fn default_order(doc: &Document, st: &ModelState, f: &Feature, members: &[String]) -> Result<Vec<String>> {
    if members.len() < 2 {
        return Err(DocError::Invalid("A corner override names at least two members".into()));
    }
    let order = default_orders(doc, st, f, std::slice::from_ref(&members.to_vec()))?.pop().flatten();
    order.ok_or_else(|| DocError::Invalid("A corner override is not at a corner of the frame".into()))
}

/// [`default_order`] of each set in `sets`, the frame laid out once: `None` for a set that names no corner, crossing
/// or overlap of the frame, or fewer than two members.
pub fn default_orders(doc: &Document, st: &ModelState, f: &Feature, sets: &[Vec<String>]) -> Result<Vec<Option<Vec<String>>>> {
    let FeatureKind::FrameMember { corner, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let (vals, _) = super::evaluation_values(doc);
    let (segments, sketch_x, drawn, planes) = segments(doc, &vals, f, st)?;
    let records: Vec<CornerOverride> = sets
        .iter()
        .map(|set| CornerOverride {
            members: set.iter().map(|l| MemberRef { feature: None, line: l.clone() }).collect(),
            kind: CornerType::Auto,
            gap: None,
        })
        .collect();
    Ok(orders(*corner, (&segments, &planes, &drawn, sketch_x), &records, true))
}

/// The fits of `plan` where its crossings cut a line, exactly as Auto fits them: each piece's end cut there, and each
/// tube's end there (a post up to the middle of an X brace), fitted to the tube that stays whole there and the ends
/// laid before it. `segments`: the plan's pieces.
fn crossing_fits<'a>(plan: &'a Plan, segments: &'a [(String, Vec3, Vec3)]) -> impl Iterator<Item = &'a Fit> {
    let at_crossing = |p: Vec3| plan.crossings.iter().any(|c| c.dist(p) <= frame_corners::CORNER_TOL);
    plan.fits.iter().filter(move |f| {
        let cut = plan.pieces.get(f.member).is_some_and(|p| if f.at_start { p.from > 0.0 } else { p.to < 1.0 });
        cut || segments.get(f.member).is_some_and(|(_, a, b)| at_crossing(if f.at_start { *a } else { *b }))
    })
}

/// Member m's length as Auto ranks it: under `plan` (whose pieces `segments` are) its whole line's, from its line's
/// first piece's start to its last piece's end, which are the line's own ends ([`pieced`]); else its own.
fn line_length(segments: &[(String, Vec3, Vec3)], plan: Option<&Plan>, m: usize) -> f64 {
    let own = frame_corners::segment_length(segments)(m);
    let Some((plan, parent)) = plan.and_then(|p| Some((p, p.pieces.get(m)?.parent))) else { return own };
    let mut line = plan.pieces.iter().zip(segments).filter(|(p, _)| p.parent == parent);
    match (line.clone().find(|(p, _)| p.from <= 0.0), line.find(|(p, _)| p.to >= 1.0)) {
        (Some((_, (_, a, _))), Some((_, (_, _, b)))) => a.dist(*b),
        _ => own,
    }
}

/// The members a frame builds from its lines (`segments`, with each line's plane and place in the drawing order). Under
/// a plan (Auto's, or a Mitre, Butt or Coped frame's crossings', [`plans`]) they are the plan's pieces
/// ([`frame_layout::Piece`], #4003 stage 3), which every member index of the plan indexes: each piece is its line's
/// stretch [from, to], keeping its line's plane and drawing place, so it is placed as its line is ([`section_axes`])
/// and builds, cuts and fits as a member of its own. Any other frame's members are its lines, as they are.
fn pieced(segments: Vec<(String, Vec3, Vec3)>, planes: Vec<MemberPlane>, drawn: Vec<usize>, plan: Option<&Plan>) -> Result<Pieced> {
    let Some(plan) = plan else { return Ok((segments, planes, drawn)) };
    let n = plan.pieces.len();
    if n > MAX_MEMBERS {
        return Err(too_many_pieces());
    }
    // A sketch id may hold `>` itself: a line `a>b` and the piece of a cut by b can't both be members.
    let mut seen = HashSet::new();
    if let Some(p) = plan.pieces.iter().find(|p| !seen.insert(p.id.as_str())) {
        return Err(DocError::Invalid(format!("member id `{}` is used twice (a line's, and a piece's where a tube crosses)", p.id)));
    }
    let mut out: Pieced = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n.min(drawn.len())));
    for p in &plan.pieces {
        let (_, a, b) = segments.get(p.parent).ok_or_else(|| DocError::Invalid(format!("no member {}", p.parent)))?;
        // A line's own ends exactly, so a line left whole is built exactly as it was.
        let at = |t: f64| {
            if t <= 0.0 {
                *a
            } else if t >= 1.0 {
                *b
            } else {
                *a + (*b - *a) * t
            }
        };
        out.0.push((p.id.clone(), at(p.from), at(p.to)));
        out.1.push(planes.get(p.parent).copied().flatten());
        out.2.extend(drawn.get(p.parent));
    }
    Ok(out)
}

/// A frame's members as [`pieced`] gives them: (id, start, end), each one's plane and its place in the drawing order.
type Pieced = (Vec<(String, Vec3, Vec3)>, Vec<MemberPlane>, Vec<usize>);

/// For each of `plan`'s pieces, whether it was cut where tubes cross (at either end): its ends are fitted by
/// [`frame_multi_trim::fit_piece_end`].
fn cut_pieces(plan: &Plan) -> Vec<bool> {
    plan.pieces.iter().map(|p| p.from > 0.0 || p.to < 1.0).collect()
}

/// More pieces than a feature may have members: crossings cut a line in as many pieces as cross it.
fn too_many_pieces() -> DocError {
    DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature, counting the pieces crossings make"))
}

/// For each member, which members are later pieces of a cut line ([`frame_layout::Piece`]: `L>X`, not a line or pair
/// of the frame's own, whatever its id holds), and for each, the index of its line's first piece (L's) and of the
/// crossing line's (X's), whose names name it ([`member_names`]). `None` for every other member.
pub type LaterPieces = Vec<Option<(usize, usize)>>;

/// [`LaterPieces`] of `pieces`, cut from the lines `segments`.
fn later_pieces(pieces: &[frame_layout::Piece], segments: &[(String, Vec3, Vec3)]) -> LaterPieces {
    // Each line's first piece, which has the line's own id.
    let mut first: HashMap<usize, usize> = HashMap::new();
    for (k, p) in pieces.iter().enumerate() {
        first.entry(p.parent).or_insert(k);
    }
    let line: HashMap<&str, usize> = segments.iter().enumerate().map(|(m, s)| (s.0.as_str(), m)).collect();
    pieces
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let at = *first.get(&p.parent)?;
            if at == k {
                return None;
            }
            // X follows L's own id and `>`: L's id may hold `>` too.
            let x = p.id.strip_prefix(segments.get(p.parent)?.0.as_str())?.strip_prefix('>')?;
            Some((at, *first.get(line.get(x)?)?))
        })
        .collect()
}

/// Each member's section axes (x, y), decided here alone so that its build, its corner cuts, its fit tool and End Cap
/// ([`member_section`]) all place it alike. Under Auto (`plan`) sections line up (#4003 Goal 7): a member in a sketch
/// plane takes the sketch's normal as up and runs the way its group lays it, an outer member round the outline
/// ([`frame_layout::Rank`]); members in no sketch plane chained in one plane take that plane's normal, pointing up
/// ([`chain_ups`]). Any other member, and every member of a frame that isn't Auto, keeps [`frame::member_axes`] with `frame_up`.
/// Also, for each member Auto runs against its line, its axes running the way the line is drawn, to name its faces
/// by ([`member_body`]).
fn section_axes(
    segments: &[(String, Vec3, Vec3)],
    planes: &[MemberPlane],
    plan: Option<&Plan>,
    turn: f64,
    own: &[f64],
    frame_up: frame::UpAxis,
) -> Result<(Vec<Axes>, Vec<Option<Axes>>)> {
    let mut ups: Vec<Option<Vec3>> = vec![None; segments.len()];
    let mut reverse = vec![false; segments.len()];
    if let Some(plan) = plan {
        ups = chain_ups(segments, &plan.chained);
        for (up, plane) in ups.iter_mut().zip(planes) {
            if let Some((normal, _)) = plane {
                *up = Some(*normal);
            }
        }
        for r in &plan.ranks {
            if let Some(v) = reverse.get_mut(r.member) {
                *v = r.reverse;
            }
        }
    }
    let (mut axes, mut plain) = (Vec::with_capacity(segments.len()), Vec::with_capacity(segments.len()));
    for (m, (id, a, b)) in segments.iter().enumerate() {
        let reversed = reverse.get(m).copied().unwrap_or(false);
        let up = ups.get(m).copied().flatten();
        // A Rotate step (#15070) turns the member further.
        let turn = turn + own.get(m).copied().unwrap_or(0.0).rem_euclid(TAU);
        let lined = up.and_then(|up| frame::group_axes(if reversed { *a - *b } else { *b - *a }, up, turn));
        plain.push(up.filter(|_| reversed && lined.is_some()).and_then(|up| frame::group_axes(*b - *a, up, turn)));
        axes.push(
            lined.or_else(|| frame::member_axes(*b - *a, turn, frame_up)).ok_or_else(|| DocError::Invalid(format!("line `{id}` has no length")))?,
        );
    }
    Ok((axes, plain))
}

/// A section's (x, y) in the world.
type Axes = (Vec3, Vec3);

/// Each member's section plane placed on its line ([`placer`]), and for a member Auto runs against its line, its
/// plane as it would be placed running the way its line is drawn ([`unreversed_placer`]).
type Placing<'a> = (&'a dyn Fn(usize, Vec3) -> Plane, &'a dyn Fn(usize, Vec3) -> Option<Plane>);

/// Each member's up when it is in a chain of members in no sketch plane (joined at `chained`'s corners) that lies in
/// one plane: that plane's normal, pointing up ([`plane_up`]). `None` for any other member.
fn chain_ups(segments: &[(String, Vec3, Vec3)], chained: &[ChainCorner]) -> Vec<Option<Vec3>> {
    // Each member's chain, as the lowest member in it: joining two chains points the higher one at the lower, so a
    // walk down `chain` always ends.
    let mut chain: Vec<usize> = (0..segments.len()).collect();
    let root = |chain: &[usize], mut m: usize| {
        while let Some(&p) = chain.get(m).filter(|&&p| p < m) {
            m = p;
        }
        m
    };
    for c in chained {
        let (ra, rb) = (root(&chain, c.a.0), root(&chain, c.b.0));
        if let Some(slot) = chain.get_mut(ra.max(rb)) {
            *slot = ra.min(rb);
        }
    }
    let mut chains: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for m in 0..segments.len() {
        chains.entry(root(&chain, m)).or_default().push(m);
    }
    let mut out = vec![None; segments.len()];
    for ms in chains.values().filter(|ms| ms.len() > 1) {
        let points: Vec<Vec3> = ms.iter().filter_map(|&m| segments.get(m)).flat_map(|(_, a, b)| [*a, *b]).collect();
        let up = plane_up(&points);
        for &m in ms {
            if let Some(o) = out.get_mut(m) {
                *o = up;
            }
        }
    }
    out
}

/// The unit normal of the one plane `points` lie in (within CORNER_TOL), pointing up: +Z, else +Y, else +X
/// (claude_fusion's `chain_up`). `None` when they lie along one line, or in no one plane.
fn plane_up(points: &[Vec3]) -> Option<Vec3> {
    let tol = frame_corners::CORNER_TOL;
    let p0 = *points.first()?;
    let v1 = points.iter().map(|p| *p - p0).find(|v| v.len() > tol)?;
    let n = points.iter().map(|p| v1.cross(*p - p0)).find(|c| c.len() > tol * v1.len())?.normalized()?;
    // `!(… <= tol)`, so a point that isn't finite is in no plane.
    if points.iter().any(|p| !((*p - p0).dot(n).abs() <= tol)) {
        return None;
    }
    let first = [n.z, n.y, n.x].into_iter().find(|c| c.abs() > 1e-9).unwrap_or(1.0);
    Some(if first < 0.0 { n * -1.0 } else { n })
}

/// Member m's section plane, square to its line, placed at `p` on it: `axes` from [`section_axes`], `at` the profile
/// point that sits on the line. A member Auto runs against its line has x × y against it too ([`along`]).
fn placer<'a>(axes: &'a [Axes], ats: &'a [Vec2]) -> impl Fn(usize, Vec3) -> Plane + 'a {
    move |m, p| section_plane(axes.get(m).copied().unwrap_or((Vec3::X, Vec3::Y)), ats.get(m).copied().unwrap_or(Vec2::ZERO), p)
}

/// [`placer`] for the members [`section_axes`] gives unreversed axes: `None` for every other member.
fn unreversed_placer<'a>(plain: &'a [Option<Axes>], ats: &'a [Vec2]) -> impl Fn(usize, Vec3) -> Option<Plane> + 'a {
    move |m, p| plain.get(m).copied().flatten().map(|xy| section_plane(xy, ats.get(m).copied().unwrap_or(Vec2::ZERO), p))
}

/// The section plane of axes (x, y) whose profile point `at` sits at `p`.
fn section_plane((x, y): Axes, at: Vec2, p: Vec3) -> Plane {
    let origin = p - x * at.x - y * at.y;
    Plane::new(origin, x, y).unwrap_or(Plane { origin, x, y })
}

/// A member's body: `regions` on its section `plane` at its start, built along its line `d` ([`along`]) from `lo` to
/// `hi`, each face named `face:<k>`. A member Auto runs against its line is built from its regions mirrored, which
/// the kernel numbers otherwise, so its faces are named as the same member built on `unreversed` (its plane running
/// the way its line is drawn) numbers them ([`matched_numbers`]), each cap as the cap at the same end:
/// - each side face as the face at the same place in the section, measured from the centre of the section's box: a
///   section symmetric about its own centre line is then the same solid either way, or moved across its line, and
///   each face keeps its name;
/// - failing that (a section that is not, such as an angle or a channel), as the face swept from the same edge of
///   the section;
/// - failing both, as the kernel numbers it.
///
/// The kernel numbers an extrusion's faces by where it lies, not by profile segment, so they can't be renumbered by
/// index alone.
fn member_body(plane: Plane, unreversed: Option<Plane>, regions: &[Region2], d: Vec3, (lo, hi): (f64, f64), id: &str) -> Result<Traced> {
    let (plane, built) = along(plane, regions, d);
    let body = one_solid(kernel::extrude(&plane, &built, lo, hi)?, id)?;
    let (Cow::Owned(_), Some(unreversed)) = (&built, unreversed) else {
        return Ok(crate::lineage::numbered_tool(body));
    };
    let plain = one_solid(kernel::extrude(&unreversed, regions, lo, hi)?, id)?;
    let (mid_m, mid_p) = (frame::anchor_point(&built, frame::Anchor::Center), frame::anchor_point(regions, frame::Anchor::Center));
    let at_place = matched_numbers(&body, &plain, |q| plane.dir_to_world(q - mid_m), |q| unreversed.dir_to_world(q - mid_p));
    let by_edge = || matched_numbers(&body, &plain, |q| Vec3::new(-q.x, q.y, 0.0), |q| Vec3::new(q.x, q.y, 0.0));
    Ok(match at_place.or_else(by_edge) {
        Some(k) => Traced::made(body, Some(k.into_iter().map(|k| vec![format!("face:{k}")]).collect())),
        None => crate::lineage::numbered_tool(body),
    })
}

/// For each face of `mirrored` (a section's regions mirrored and extruded), the face of `plain` (the same section
/// extruded unmirrored) whose profile segment `fp` maps where `fm` maps the mirrored face's (both ends and its
/// middle), or the cap at the same end. `None` unless each face finds its own.
fn matched_numbers(mirrored: &kernel::Body, plain: &kernel::Body, fm: impl Fn(Vec2) -> Vec3, fp: impl Fn(Vec2) -> Vec3) -> Option<Vec<usize>> {
    use kernel::FaceSource::{ExtrudeCap, ExtrudeSide};
    fn source(b: &kernel::Body, i: usize) -> Option<&kernel::FaceSource> {
        b.face_sources().and_then(|s| s.get(i)).and_then(Option::as_ref)
    }
    let near = |p: Vec2, q: Vec2| fm(p).dist(fp(q)) <= 1e-6;
    let same = |i: usize, j: usize| match (source(mirrored, i), source(plain, j)) {
        (Some(ExtrudeCap { high: a, .. }), Some(ExtrudeCap { high: b, .. })) => a == b,
        (Some(ExtrudeSide { segment: s, .. }), Some(ExtrudeSide { segment: t, .. })) => {
            let (a, b) = (s.start(), s.end());
            near(s.point_at(0.5), t.point_at(0.5)) && ((near(a, t.start()) && near(b, t.end())) || (near(a, t.end()) && near(b, t.start())))
        }
        _ => false,
    };
    let n = plain.face_count();
    if mirrored.face_count() != n {
        return None;
    }
    let mut taken = vec![false; n];
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let j = (0..n).find(|&j| taken.get(j) == Some(&false) && same(i, j))?;
        if let Some(t) = taken.get_mut(j) {
            *t = true;
        }
        out.push(j);
    }
    Some(out)
}

/// A member's section `plane` and `regions` to build along its line `d`. A member Auto runs against its line
/// ([`section_axes`]) is placed with x × y against the line: here its x is turned round and its regions mirrored, the
/// same section in a plane whose x × y runs along the line, so every member is built from its line's drawn start, its
/// ends and their names tied to its drawn ends. Any other plane is given back as it is.
fn along(plane: Plane, regions: &[Region2], d: Vec3) -> (Plane, Cow<'_, [Region2]>) {
    if plane.normal().dot(d) >= 0.0 {
        return (plane, Cow::Borrowed(regions));
    }
    (Plane { x: plane.x * -1.0, ..plane }, Cow::Owned(regions.iter().map(frame::mirrored).collect()))
}

/// How far past its line's end fitted end `f` is built: on an open section (an angle, a channel), out to the far side of
/// the members it fits against (where the end's line leaves the box round each one's section, at any angle), so the fit
/// leaves the end reaching across their open side to the face beyond, not stopping at their lines. A tube's or a bar's
/// fit fills its whole side, so their ends (and their faces' names) stay as they were: built to the line's end.
fn fit_reach(segments: &[(String, Vec3, Vec3)], regions: &[Region2], placed: &dyn Fn(usize, Vec3) -> Plane, f: &Fit) -> f64 {
    if !frame_multi_trim::is_open(regions) {
        return 0.0;
    }
    let Some((_, a, b)) = segments.get(f.member) else { return 0.0 };
    let (end, toward) = if f.at_start { (*a, *a - *b) } else { (*b, *b - *a) };
    let length = a.dist(*b);
    let corners = frame_multi_trim::box_corners(regions);
    let reach = f
        .against
        .iter()
        .filter_map(|&t| {
            let (_, c, d) = segments.get(t)?;
            let at = solidcraft_geom::mesh_distance::closest_on_segments(end, end, *c, *d).1;
            let plane = placed(t, at);
            let exits = corners
                .as_chunks::<4>()
                .0
                .iter()
                .filter_map(|r| solidcraft_kernel::planar::exit_through_box(end, toward, r.map(|q| plane.to_world(q))));
            exits.reduce(f64::max)
        })
        .fold(0.0, f64::max);
    // Never more than the member's own length: a far side that far off is no joint this end makes.
    if reach.is_finite() { reach.clamp(0.0, length) } else { 0.0 }
}

/// Auto's ends (#4003 spec A4-A6), as [`corner_ends`] gives stage 1's, and the ends cut to fit, in laying order:
/// [`frame_layout::plan`]'s corners jointed by the table (an angled butt, a mitre, or a butt whose short tube is
/// fitted to the long one) or by their override; its chained corners as stage 1 finishes them; its fits. A fitted
/// end is built to its line's end, or on an open section to the far face it meets ([`fit_reach`]), and not plane cut, as `(None, reach, false, true)`. Under `coped` ([`copes`]) every butt,
/// the table's or an override's, is cut as Coped Butt: its short tube coped to the long one, not fitted.
fn auto_ends(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    plan: Plan,
    (sketch_x, drawn): Tie,
    coped: bool,
    gap: f64,
) -> Result<(CornerEnds, Vec<Fit>)> {
    let mut out: CornerEnds = (vec![[None, None]; segments.len()], Vec::new());
    let mut fits = plan.fits;
    // `through` only works out the Flip that keeps `long` through: any length does.
    let length = frame_corners::segment_length(segments);
    for ac in &plan.corners {
        let c = Corner { point: ac.point, ends: [ac.long, ac.short] };
        let inside = frame_corners::interior(segments, &c);
        // In line (the table's angled butt at 0°): both left square, whatever the joint. A Free corner is not
        // jointed here either.
        if inside > PI - frame_corners::IN_LINE_DEG.to_radians() || (ac.kind, ac.joint) == (CornerType::Auto, Joint::Free) {
            continue;
        }
        if ac.kind != CornerType::None && inside < frame_corners::MIN_INTERIOR_DEG.to_radians() {
            return Err(frame_corners::corner_error(c.point, "too sharp to finish"));
        }
        // corner_cuts runs a butt's longer member through, then Flip swaps it; `long` already is the one to run
        // through (the plan applied Flip), so ask for whichever Flip keeps it.
        let flip = frame_corners::through(segments, &c, &length, sketch_x, drawn) != 0;
        // Its override's weld gap, else the frame's.
        let g = ac.gap.unwrap_or(gap);
        let cuts = |kind| frame_corners::corner_cuts(kind, flip, g, &c, segments, &length, placed, regions, sketch_x, drawn);
        match (ac.kind, ac.joint) {
            (CornerType::Auto, Joint::AngledButt) => {
                let angled = frame_corners::angled_butt_cuts(&c, ac.long.0, ac.short.0, segments, g);
                add_corner(&mut out, &c, false, (angled, None), segments, regions, placed, g)?;
            }
            (CornerType::Auto, Joint::Mitre) => add_corner(&mut out, &c, false, cuts(CornerType::Mitre)?, segments, regions, placed, g)?,
            (CornerType::Auto, _) if coped => add_corner(&mut out, &c, true, cuts(CornerType::CopedButt)?, segments, regions, placed, g)?,
            (CornerType::Auto, _) => {
                // A butt: the long tube runs through, cut as stage 1's T; the short one is cut to fit against it.
                let (mut through, _) = cuts(CornerType::Butt)?;
                through.truncate(1);
                add_corner(&mut out, &c, true, (through, None), segments, regions, placed, g)?;
                fits.push(Fit { member: ac.short.0, at_start: ac.short.1, against: vec![ac.long.0], gap: ac.gap });
            }
            (kind, _) => add_corner(&mut out, &c, kind.flips(), cuts(kind.coped(coped))?, segments, regions, placed, g)?,
        }
    }
    // Members in no group plane, chained: mitred, or as their override says, by stage 1's rules.
    let end = |(m, at_start): (usize, bool)| segments.get(m).map(|(_, a, b)| if at_start { *a } else { *b });
    for ch in &plan.chained {
        let (Some(pa), Some(pb)) = (end(ch.a), end(ch.b)) else { continue };
        let c = Corner { point: (pa + pb) * 0.5, ends: [ch.a, ch.b] };
        let g = ch.gap.unwrap_or(gap);
        let cuts = frame_corners::corner_cuts(ch.kind.coped(coped), ch.flip, g, &c, segments, &length, placed, regions, sketch_x, drawn)?;
        add_corner(&mut out, &c, ch.kind.flips(), cuts, segments, regions, placed, g)?;
    }
    let mut place = vec![usize::MAX; segments.len()];
    for (i, r) in plan.ranks.iter().enumerate() {
        if let Some(p) = place.get_mut(r.member) {
            *p = i;
        }
    }
    fits.sort_by_key(|f| place.get(f.member).copied().unwrap_or(usize::MAX));
    for f in &fits {
        if let Some(e) = out.0.get_mut(f.member).and_then(|e| e.get_mut(usize::from(!f.at_start)))
            && e.is_none()
        {
            *e = Some((None, fit_reach(segments, regions, placed, f), false, true));
        }
    }
    Ok((out, fits))
}

/// An Auto frame's members (#4003 stage 2): cut at their corners ([`auto_ends`], [`cut_members`]), then each end to
/// fit cut, in laying order, to the members it meets as their plane cuts build them (each member's cutting tool,
/// [`member_tool`], made once). A fitted face is named for the end it finishes and the id of the member it
/// fits (a piece's own id, [`pieced`]), `corner_start:<member>:face:<k>` (`corner_end:…`), so a member's fitted faces
/// keep names of their own whatever the laying or pick order, and where three tubes cross. `plan`: the frame's
/// [`frame_layout::plan`], over `segments` [`pieced`] by it. A piece of a line cut where tubes cross keeps what lies
/// past the tubes it is fitted to ([`frame_multi_trim::fit_piece_end`], ruling C4): with nothing left it is dropped, as
/// the second list says (`true`: no body). The third: each member's cut ends, as it was built with them. `coped`:
/// the frame copes ([`copes`]); `gap`: its weld gap.
pub(crate) fn auto_members(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    (placed, unreversed): Placing,
    plan: Plan,
    tie: Tie,
    coped: bool,
    gap: f64,
) -> Result<(Vec<Option<Traced>>, Vec<bool>, Vec<[Option<CutEnd>; 2]>)> {
    let crossed = cut_pieces(&plan);
    let mut dropped = vec![false; segments.len()];
    let ((ends, copes), fits) = auto_ends(segments, regions, placed, plan, tie, coped, gap)?;
    let mut out = cut_members(segments, regions, (placed, unreversed), &ends, copes)?;
    fit_members(segments, regions, placed, &ends, &fits, (&mut out, &crossed, &mut dropped), (coped, gap))?;
    Ok((out, dropped, ends))
}

/// Each end of `fits` (in laying order) of `out` (each member as built) cut to fit the members it meets as their plane
/// cuts (`ends`) build them (each member's cutting tool, [`member_tool`], made once). A fitted face is named for the
/// end it finishes and the id of the member it fits, `corner_start:<member>:face:<k>` (`corner_end:…`). A piece cut
/// where tubes cross (`crossed`) keeps what lies past them ([`frame_multi_trim::fit_piece_end`]); with nothing left it has
/// no body, and `dropped` says so. `coped`: each tool is its member's real shape ([`member_tool`]). Each fit stands off
/// by its weld gap ([`Fit::gap`], else the frame's `gap`), its tools grown by it.
fn fit_members(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    ends: &[[Option<CutEnd>; 2]],
    fits: &[Fit],
    (out, crossed, dropped): (&mut [Option<Traced>], &[bool], &mut [bool]),
    (coped, gap): (bool, f64),
) -> Result<()> {
    let no_member = |m: usize| DocError::Invalid(format!("no member {m}"));
    // Each member's tool for each gap, by the gap's bits.
    let mut tools: HashMap<(usize, u64), kernel::Body> = HashMap::new();
    let gap_of = |f: &Fit| f.gap.unwrap_or(gap);
    for f in fits {
        for &t in &f.against {
            if let std::collections::hash_map::Entry::Vacant(slot) = tools.entry((t, gap_of(f).to_bits())) {
                slot.insert(member_tool(segments, regions, placed, ends, t, coped, gap_of(f))?.body);
            }
        }
    }
    // A point in the section's material (a tube's wall, not its bore).
    let inside = regions.first().map(|r| r.interior_point()).ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    for f in fits {
        if dropped.get(f.member) == Some(&true) {
            continue;
        }
        let (id, a, b) = segments.get(f.member).ok_or_else(|| no_member(f.member))?;
        let (end, toward) = if f.at_start { (*a, *a - *b) } else { (*b, *b - *a) };
        let middle = placed(f.member, (*a + *b) * 0.5).to_world(inside);
        let end_role = if f.at_start { "corner_start" } else { "corner_end" };
        let named: Vec<Traced> = f
            .against
            .iter()
            .filter_map(|&t| {
                let (body, line) = (tools.get(&(t, gap_of(f).to_bits()))?, &segments.get(t)?.0);
                let roles = (0..body.face_count()).map(|k| vec![format!("{end_role}:{line}:face:{k}")]).collect();
                Some(Traced::made(body.clone(), Some(roles)))
            })
            .collect();
        let against: Vec<&Traced> = named.iter().collect();
        let slot = out.get_mut(f.member).ok_or_else(|| no_member(f.member))?;
        let member = slot.take().ok_or_else(|| no_member(f.member))?;
        if crossed.get(f.member) == Some(&true) {
            *slot = frame_multi_trim::fit_piece_end(&member, end, toward, middle, &against, id)?;
            if let (None, Some(d)) = (slot.as_ref(), dropped.get_mut(f.member)) {
                *d = true;
            }
        } else {
            *slot = Some(frame_multi_trim::fit_end(&member, end, toward, middle, &against, id)?);
        }
    }
    Ok(())
}

/// Member m's cutting tool ([`frame_multi_trim::fit_tool`], or under `coped` its real shape, [`frame_multi_trim::coped_tool`]) as
/// its ends (`ends`) build it: out to a plane cut or fitted end's length (on an open section, a fit reaches the far
/// face of the member it meets), but a coped end stops at its line's end, inside the tube it meets. Grown by
/// the weld gap `gap` of the end it cuts.
fn member_tool(
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    ends: &[[Option<CutEnd>; 2]],
    m: usize,
    coped: bool,
    gap: f64,
) -> Result<Traced> {
    let (_, a, b) = segments.get(m).ok_or_else(|| DocError::Invalid(format!("no member {m}")))?;
    // No ends listed: a frame with no corners to finish, its members square.
    let [start, end] = ends.get(m).copied().unwrap_or([None, None]);
    let kept = |e: Option<CutEnd>| e.filter(|e| e.3).map(|(cut, ext, _, _)| (cut, ext));
    let (start, end) = (kept(start), kept(end));
    let cuts: Vec<Cut> = [start, end].into_iter().flatten().filter_map(|c| c.0).collect();
    let (plane, regions) = along(placed(m, *a), regions, *b - *a);
    let tool = if coped { frame_multi_trim::coped_tool } else { frame_multi_trim::fit_tool };
    tool(&plane, &regions, -start.map_or(0.0, |c| c.1), a.dist(*b) + end.map_or(0.0, |c| c.1), &cuts, gap)
}

/// Each member's cut ends as the feature's corners make them ([`auto_ends`] for an Auto frame, `plan` its plan, else
/// [`corner_ends`], `plan` its crossings', `overlaps` its overlaps' overrides; none for a frame with no corners to
/// finish). `gap`: the frame's weld gap.
fn frame_ends(
    f: &FeatureKind,
    corners: &[Override],
    segments: &[(String, Vec3, Vec3)],
    regions: &[Region2],
    placed: &dyn Fn(usize, Vec3) -> Plane,
    (plan, planes, overlaps): (Option<Plan>, &[MemberPlane], &[usize]),
    tie: Tie,
    gap: f64,
) -> Result<Vec<[Option<CutEnd>; 2]>> {
    let FeatureKind::FrameMember { corner, flip, .. } = f else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let coped = copes(f);
    Ok(match plan {
        Some(plan) if *corner == CornerType::Auto => auto_ends(segments, regions, placed, plan, tie, coped, gap)?.0.0,
        _ if corner.is_none() && corners.is_empty() => Vec::new(),
        crossing => {
            let finish = (*corner, *flip, gap);
            corner_ends(segments, regions, placed, finish, (corners, overlaps), (planes, tie.0, tie.1), crossing.as_ref(), coped)?.0.0
        }
    })
}

/// An Auto frame's corners that lie inside the earlier Frame Members' tubes (`earlier`, [`earlier::earlier_tools`]):
/// the long tube's section centre there [`holds`] in them. Each becomes [`Joint::Free`], not jointed, unless an
/// override names it (#4003 decision 1).
fn free_corners(plan: &mut Plan, earlier: &[(String, Traced)], placed: &dyn Fn(usize, Vec3) -> Plane, centre: Vec2) {
    if earlier.is_empty() {
        return;
    }
    for c in plan.corners.iter_mut().filter(|c| c.kind == CornerType::Auto && !c.overridden) {
        if holds(placed(c.long.0, c.point).to_world(centre), earlier.iter().map(|(_, t)| &t.body)) {
            c.joint = Joint::Free;
        }
    }
}

/// An Auto frame's chain corners (between members in no sketch plane, [`frame_layout::ChainCorner`]) that no override
/// names and whose section centre [`holds`] in a tube, an earlier Frame Member's (`earlier`) or one of this frame's
/// own group members' as built (`tool`, each built once into `tools`, only for members `near` the point): taken out
/// of `plan.chained`, so neither end is jointed there and both are free ends, fitted with the others (ruling A15;
/// claude_fusion `_chain_members`). `placed` and `centre` place the section's centre. Whether any was.
fn free_chains(
    plan: &mut Plan,
    earlier: &[(String, Traced)],
    (placed, centre): (&dyn Fn(usize, Vec3) -> Plane, Vec2),
    near: &dyn Fn(usize, Vec3) -> bool,
    tool: &dyn Fn(usize) -> Result<Traced>,
    tools: &mut [Option<kernel::Body>],
    segments: &[(String, Vec3, Vec3)],
) -> Result<bool> {
    // The members in no group plane share the group after the last one.
    let rank = |m: usize| plan.ranks.iter().find(|r| r.member == m);
    let Some(rest) = plan.chained.first().and_then(|c| rank(c.a.0)).map(|r| r.group) else { return Ok(false) };
    let grouped: Vec<usize> = plan.ranks.iter().filter(|r| r.group < rest).map(|r| r.member).collect();
    let end = |(m, at_start): (usize, bool)| segments.get(m).map(|(_, a, b)| if at_start { *a } else { *b });
    let before = plan.chained.len();
    let mut kept = Vec::with_capacity(before);
    for c in std::mem::take(&mut plan.chained) {
        let (Some(pa), Some(pb), false) = (end(c.a), end(c.b), c.overridden) else {
            kept.push(c);
            continue;
        };
        let p = placed(c.a.0, (pa + pb) * 0.5).to_world(centre);
        let mine: Vec<usize> = grouped.iter().copied().filter(|&k| near(k, p)).collect();
        for &k in &mine {
            if let Some(slot) = tools.get_mut(k)
                && slot.is_none()
            {
                *slot = Some(tool(k)?.body);
            }
        }
        let own = mine.iter().filter_map(|&k| tools.get(k)?.as_ref());
        if !holds(p, earlier.iter().map(|(_, t)| &t.body).chain(own)) {
            kept.push(c);
        }
    }
    plan.chained = kept;
    Ok(plan.chained.len() < before)
}

/// Whether `p` lies in the tubes `tools` (claude_fusion `_Tubes.holds`): inside one, or on two or more, as an earlier
/// frame's mitred or square corner lies on both its tubes' cut faces and inside neither; on one tool's skin alone (a
/// frame sitting on a tube) it does not. Exact point classification, each tool's box first (a point on its skin lies
/// on the box). A tool the kernel can't bound or classify is passed over, as [`earlier::earlier_tools`] passes over a
/// body it can't classify.
fn holds<'a>(p: Vec3, tools: impl IntoIterator<Item = &'a kernel::Body>) -> bool {
    let mut on = 0;
    for tool in tools {
        let Ok(b) = tool.exact_bounds() else { continue };
        // `!(… <= …)`: a point that isn't finite is in no box.
        if !(p.dist(p.max(b.min).min(b.max)) <= 1e-6) {
            continue;
        }
        match tool.classify_point(p) {
            Ok(kernel::PointClass::Inside) => return true,
            Ok(kernel::PointClass::OnBoundary) => on += 1,
            _ => {}
        }
        if on >= 2 {
            return true;
        }
    }
    false
}

/// The ends a frame doesn't joint within itself, which Fit Ends to Earlier Frames fits to earlier tubes (#4003
/// decision 1), each with whether it is a free end, fitted to this frame's members laid before it too
/// (claude_fusion `fit`: `pool, more = tubes, mine`). Under Auto (`plan`): every end but those of a jointed corner or
/// a chained one; the ends fitted to this frame's own tubes (`plan.fits`: T-junctions, the third and later ends at a
/// point) are not free, as Auto fits them to their own; lone ends and a Free corner's are. Under Mitre, Butt and
/// Coped: the ends at no corner, free, and those fitted where three or more end ([`crowded`]), where a crossing cuts
/// a line (`plan`, the crossings', [`crossing_fits`]) or on another tube's middle ([`tees`]), not free. Under None:
/// none. (member, at_start, free).
fn unjointed_ends(
    f: &FeatureKind,
    segments: &[(String, Vec3, Vec3)],
    plan: Option<&Plan>,
    (planes, drawn): (&[MemberPlane], &[usize]),
) -> Vec<(usize, bool, bool)> {
    let FeatureKind::FrameMember { corner, .. } = f else { return Vec::new() };
    let (jointed, fitted): (HashSet<(usize, bool)>, HashSet<(usize, bool)>) = match plan {
        Some(plan) if *corner == CornerType::Auto => (
            plan.corners
                .iter()
                .filter(|c| c.joint != Joint::Free || c.kind != CornerType::Auto)
                .flat_map(|c| [c.long, c.short])
                .chain(plan.chained.iter().flat_map(|c| [c.a, c.b]))
                .collect(),
            plan.fits.iter().map(|f| (f.member, f.at_start)).collect(),
        ),
        _ if *corner == CornerType::None => return Vec::new(),
        crossing => {
            let (crowded, fits) = stage_fits(segments, planes, drawn, crossing);
            let corners = frame_corners::find_corners(segments).into_iter().chain(crowded);
            (corners.flat_map(|c| c.ends).collect(), fits.iter().map(|f| (f.member, f.at_start)).collect())
        }
    };
    (0..segments.len())
        .flat_map(|m| [(m, true), (m, false)])
        .filter(|e| !jointed.contains(e))
        .map(|(m, at_start)| (m, at_start, !fitted.contains(&(m, at_start))))
        .collect()
}

/// This frame's own members as tools for its free ends ([`fit_to_earlier`]): each member's place in the laying order,
/// whether member k may reach a point (its line within reach of it, its section and corner extensions counted),
/// member k's tool ([`member_tool`]), and each tool once built.
struct Own<'a> {
    laid: Vec<usize>,
    near: &'a dyn Fn(usize, Vec3) -> bool,
    tool: &'a dyn Fn(usize) -> Result<Traced>,
    tools: Vec<Option<kernel::Body>>,
}

/// `made` (each member as built, by member) with each of `ends` fitted to the earlier Frame Members' tubes
/// ([`frame_multi_trim::fit_end`]), a free end to this frame's members laid before its own too (`own`), each tool built
/// once. A fitted face is named for the end it finishes and the body or line it fits, `corner_start:<body>:face:<k>`
/// (`corner_end:…`). `middle(m)`: a point in member m's material half way along. A piece cut where tubes cross
/// (`crossed`, [`cut_pieces`]) is fitted as [`frame_multi_trim::fit_piece_end`] fits it, and has no body (`None` in `made`)
/// when nothing of it is left.
fn fit_to_earlier(
    made: &mut [Option<Traced>],
    segments: &[(String, Vec3, Vec3)],
    ends: &[(usize, bool, bool)],
    earlier: &[(String, Traced)],
    own: &mut Own,
    middle: &dyn Fn(usize) -> Vec3,
    crossed: &[bool],
) -> Result<()> {
    let named = |role: &str, name: &str, body: &kernel::Body| {
        let roles = (0..body.face_count()).map(|k| vec![format!("{role}:{name}:face:{k}")]).collect();
        Traced::made(body.clone(), Some(roles))
    };
    for &(m, at_start, free) in ends {
        if made.get(m).is_some_and(Option::is_none) {
            continue;
        }
        let (id, a, b) = segments.get(m).ok_or_else(|| DocError::Invalid(format!("no member {m}")))?;
        let (end, toward, role) = if at_start { (*a, *a - *b, "corner_start") } else { (*b, *b - *a, "corner_end") };
        let mut against: Vec<Traced> = earlier.iter().map(|(body, t)| named(role, body, &t.body)).collect();
        let place = |k: usize| own.laid.get(k).copied().unwrap_or(usize::MAX);
        for k in (0..segments.len()).filter(|&k| free && k != m && place(k) < place(m) && (own.near)(k, end)) {
            let (Some(slot), Some((line, _, _))) = (own.tools.get_mut(k), segments.get(k)) else { continue };
            if slot.is_none() {
                *slot = Some((own.tool)(k)?.body);
            }
            if let Some(body) = slot.as_ref() {
                against.push(named(role, line, body));
            }
        }
        if against.is_empty() {
            continue;
        }
        let against: Vec<&Traced> = against.iter().collect();
        let slot = made.get_mut(m).ok_or_else(|| DocError::Invalid(format!("no member {m}")))?;
        let Some(member) = slot.as_ref() else { continue };
        *slot = if crossed.get(m) == Some(&true) {
            frame_multi_trim::fit_piece_end(member, end, toward, middle(m), &against, id)?
        } else {
            Some(frame_multi_trim::fit_end(member, end, toward, middle(m), &against, id)?)
        };
    }
    Ok(())
}

/// A member feature's lines (in its component frame), profile regions, default name, the
/// profile point that sits on the lines, the profile's turn about them and the frame sketch's x
/// axis (`None`: a 3D sketch, or no sketch), each member's place in the drawing order, each
/// member's sketch plane ([`segments`]), each line's further turn (radians) from the Rotate steps after the feature
/// ([`member_turns`]: a step whose angle fails is passed over) and the profile point those turn about (the centre of
/// its bounding box), and the axis the profile takes as up.
struct Layout {
    segments: Vec<(String, Vec3, Vec3)>,
    regions: Vec<Region2>,
    description: String,
    at: Vec2,
    turn: f64,
    sketch_x: Option<Vec3>,
    drawn: Vec<usize>,
    planes: Vec<MemberPlane>,
    turns: BTreeMap<String, f64>,
    centre: Vec2,
    /// The profile's own origin, which `locate` is measured from.
    origin: Vec2,
    up: frame::UpAxis,
}

fn layout(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Layout> {
    if matches!(&f.kind, FeatureKind::FrameMember { groups, .. } if !groups.is_empty()) {
        return Err(DocError::Invalid("not supported yet: joint layout across frame member groups".into()));
    }
    let (segments, sketch_x, drawn, planes) = segments(doc, vals, f, st)?;
    let Section { regions, description, at, turn, turns, centre, origin } = section(doc, vals, f, st)?;
    let FeatureKind::FrameMember { up, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    Ok(Layout { segments, regions, description, at, turn, sketch_x, drawn, planes, turns, centre, origin, up: *up })
}

/// Shared section settings; Auto seam handling remains in the unchanged section preparation.
struct Section {
    /// The unmirrored profile origin, also used by the straight-member section view.
    origin: Vec2,
    regions: Vec<Region2>,
    description: String,
    at: Vec2,
    turn: f64,
    turns: BTreeMap<String, f64>,
    centre: Vec2,
}

fn section(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Section> {
    let FeatureKind::FrameMember { profile, anchor, locate, angle, mirror, corner, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let (mut regions, description, origin) = profile_regions(vals, st, profile)?;
    if *mirror {
        regions = regions.iter().map(frame::mirrored).collect();
    }
    let at = match locate {
        Some([x, y]) => {
            // A design file is hostile input: the command checks the point, a file may not.
            if !(frame::within_profile_size(*x) && frame::within_profile_size(*y)) {
                return Err(DocError::Invalid(format!("frame member `locate` must be two finite lengths within {} mm", frame::MAX_PROFILE_SIZE)));
            }
            located_point(origin, *mirror, [*x, *y])
        }
        None => frame::anchor_point(&regions, *anchor),
    };
    let turn = match angle {
        Some(e) => val(vals, e, Kind::Angle)?,
        None => 0.0,
    };
    if *corner == CornerType::Auto {
        // A full circle is one arc, and its tube's one wall has its seam where the arc starts. Unmirrored and
        // unturned that is the section's +x, the inside of an outline Auto lays anticlockwise; Mirror or a turn
        // carries it round, to the outside on a mirror or a half turn, where each through tube's flat end touches
        // the tube fitted to it along that tube's outer side. SMLib's check refuses the fit there ("Face Near
        // missing Seam"), so under Auto every circle keeps its seam on the section's +x as placed (#4003).
        regions = regions.iter().map(|r| seamed(r, -turn)).collect();
    }
    let turns = member_turns(doc, vals, FeatureId(f.id)).into_iter().map(|(line, (_, a))| (line, a)).collect();
    let centre = frame::anchor_point(&regions, frame::Anchor::Center);
    Ok(Section { regions, description, at, turn, turns, centre, origin })
}

/// The profile point on the line of a member a Rotate step turns `t` (radians) further (#15070): the profile turns
/// about `centre`, which stays where it was, so the point on the line moves from `at`.
pub(super) fn turned_at(at: Vec2, centre: Vec2, t: f64) -> Vec2 {
    let (c, s) = (t.cos(), t.sin());
    let d = centre - at;
    centre - Vec2::new(c * d.x + s * d.y, c * d.y - s * d.x)
}

/// Each member's line id: a piece's, the line it was cut from (`cutting`'s pieces), else its own.
pub(super) fn member_lines_of(segments: &[(String, Vec3, Vec3)], cutting: Option<&Plan>) -> Vec<String> {
    match cutting {
        Some(p) => p.pieces.iter().map(|pc| segments.get(pc.parent).map_or_else(|| pc.id.clone(), |s| s.0.clone())).collect(),
        None => segments.iter().map(|s| s.0.clone()).collect(),
    }
}

/// `region` with each full circle (a loop of one whole-turn arc) starting at angle `start`, turning the way it did.
fn seamed(region: &Region2, start: f64) -> Region2 {
    let seam = |l: &Loop2| match l.segs.as_slice() {
        [Seg2::Arc { center, radius, sweep, .. }] if (sweep.abs() - TAU).abs() < 1e-9 => {
            Loop2 { segs: vec![Seg2::Arc { center: *center, radius: *radius, start, sweep: *sweep }] }
        }
        _ => l.clone(),
    };
    Region2 { outer: seam(&region.outer), holes: region.holes.iter().map(seam).collect() }
}

/// Each member's line in the feature's component frame: (member id, start, end), the sketch's
/// lines then the pairs'; the frame sketch's x axis (`None`: a 3D sketch, or no sketch); each
/// member's place in the sketch's drawing order ([`drawn_order`]); and each member's sketch plane:
/// a 2D sketch's own front normal (x × y, whichever way a face it is drawn on faces) and x axis
/// for its lines, `None` for a 3D sketch's lines and for pairs.
fn segments(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Path> {
    if let Some(groups) = frame::group_features(f)? {
        let (mut all, mut drawn_all, mut planes_all) = (Vec::new(), Vec::new(), Vec::new());
        let mut sketch_x = None;
        for group in groups {
            let (segments, x, drawn, planes) = segments(doc, vals, &group, st)?;
            sketch_x = sketch_x.or(x);
            all.extend(segments);
            drawn_all.extend(drawn);
            planes_all.extend(planes);
        }
        return Ok((all, sketch_x, drawn_all, planes_all));
    }
    let FeatureKind::FrameMember { path_sketch, lines, pairs, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let (mut segments, sketch, plane) = match path_sketch {
        Some(path_sketch) => {
            let ps =
                st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            // The frame sketch in this feature's component frame: its plane, and its 3D lines.
            let mapped = if ps.sketch.wires.is_empty() {
                Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?)
            } else {
                path_in_frame(doc, ps, f.component)?
            };
            let plane = ps.sketch.wires.is_empty().then(|| (mapped.plane.normal(), mapped.plane.x));
            (member_lines(mapped.as_ref(), lines)?, Some(&ps.sketch), plane)
        }
        None => (Vec::new(), None, None),
    };
    let mut planes = vec![plane; segments.len()];
    segments.extend(pair_lines(doc, vals, f, st, pairs)?);
    planes.resize(segments.len(), None);
    let mut seen = HashSet::new();
    if let Some((id, _, _)) = segments.iter().find(|(id, _, _)| !seen.insert(id.as_str())) {
        return Err(DocError::Invalid(format!("member id `{id}` is used twice")));
    }
    if segments.is_empty() {
        return Err(DocError::Invalid("the frame member has no members (give it a sketch or pairs of points)".into()));
    }
    if segments.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature")));
    }
    let drawn = sketch.map(|sk| drawn_order(sk, &segments)).unwrap_or_default();
    Ok((segments, plane.map(|p| p.1), drawn, planes))
}

/// A Frame Member's members, as [`segments`] finds them.
type Path = (Vec<(String, Vec3, Vec3)>, Option<Vec3>, Vec<usize>, Vec<MemberPlane>);

/// Each member's centreline where the view shows it: (member id, start, end), in the world.
/// `st` is the model the feature was evaluated on (`Model::state_before`).
pub fn member_segments(doc: &Document, st: &ModelState, f: &Feature) -> Result<Vec<(String, Vec3, Vec3)>> {
    if paths::uses(st, &f.kind)? {
        return paths::member_segments(doc, st, f);
    }
    let m = doc.component_transform(f.component);
    let place = |p: Vec3| crate::apply_point(&m, p);
    Ok(segments(doc, &doc.evaluation_values(), f, st)?.0.into_iter().map(|(id, a, b)| (id, place(a), place(b))).collect())
}

/// The section point a member's `locate` puts on its line, in the section's coordinates as drawn
/// (mirrored when the member is). `locate` is measured from the profile's own origin
/// (`Profile::origin`), before the mirror. The built-in region is centred on its bounding box, so
/// that origin moves when the section's size changes, and the located point moves with it: the
/// member stays attached at the same point of its section. [`SectionView::locate_of`] is the
/// inverse.
pub fn located_point(origin: Vec2, mirror: bool, locate: [f64; 2]) -> Vec2 {
    let p = origin + Vec2::new(locate[0], locate[1]);
    if mirror { Vec2::new(-p.x, p.y) } else { p }
}

/// A member's section as a dialog shows it before the member is made: the section of the member
/// on the feature's first line, in world coordinates, laid out by the same code that makes the
/// body, so what the dialog shows is where the member goes.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionView {
    /// The first line's start, where the section sits, and its end.
    pub start: Vec3,
    pub end: Vec3,
    /// The section's axes (unit, x × y along the line).
    pub x: Vec3,
    pub y: Vec3,
    /// The section's regions in its own coordinates, mirrored when the member is.
    pub regions: Vec<Region2>,
    /// The section point that sits on the line.
    pub at: Vec2,
    /// The profile's own origin, which `locate` is measured from.
    pub origin: Vec2,
    pub mirror: bool,
}

impl SectionView {
    /// The world point of section point `p`.
    pub fn world(&self, p: Vec2) -> Vec3 {
        self.start + self.x * (p.x - self.at.x) + self.y * (p.y - self.at.y)
    }

    /// The section point at world point `q` (projected onto the section's plane).
    pub fn section_point(&self, q: Vec3) -> Vec2 {
        let d = q - self.start;
        Vec2::new(self.at.x + d.dot(self.x), self.at.y + d.dot(self.y))
    }

    /// The `locate` that puts section point `p` on the line: measured from the profile's own
    /// origin, before the mirror, as the member reads it ([`located_point`] is the inverse).
    pub fn locate_of(&self, p: Vec2) -> [f64; 2] {
        let q = if self.mirror { Vec2::new(-p.x, p.y) } else { p };
        [q.x - self.origin.x, q.y - self.origin.y]
    }

    /// The section point that `locate` puts on the line: the inverse of [`Self::locate_of`].
    pub fn locate_point(&self, locate: [f64; 2]) -> Vec2 {
        located_point(self.origin, self.mirror, locate)
    }

    /// Every anchor's section point, in [`frame::Anchor::ALL`] order.
    pub fn anchors(&self) -> Vec<(frame::Anchor, Vec2)> {
        frame::Anchor::ALL.iter().map(|a| (*a, frame::anchor_point(&self.regions, *a))).collect()
    }
}

/// The section member feature `f` (not made yet: no Rotate step turns it) puts on its first line
/// (see [`SectionView`]), carried to world by `placed` (the component's frame as shown: the active
/// occurrence's, with its pending move); an error when the feature could not be made as it stands.
pub fn section_view(doc: &Document, st: &ModelState, f: &Feature, placed: &crate::Mat) -> Result<SectionView> {
    let FeatureKind::FrameMember { corner, corners, gap, mirror, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let (vals, _) = doc.param_values();
    let Layout { segments, regions, at, turn, sketch_x, drawn, planes, origin, up, .. } = layout(doc, &vals, f, st)?;
    let (line, a, b) = segments.first().cloned().ok_or_else(|| DocError::Invalid("the member feature has no lines".into()))?;
    // The axes as the body is made ([`member_pivot`]'s way): under Auto they follow the frame's plan.
    let gap = gap_value(&vals, gap.as_ref(), "`gap`")?;
    let corners = &overrides(*corner, (&segments, &planes, &drawn, sketch_x), corners, Some((&vals, gap)))?;
    let (plan, crossing) = plans(*corner, &segments, &planes, corners, &drawn)?;
    let (segments, planes, _) = pieced(segments, planes, drawn, plan.as_ref().or(crossing.as_ref()))?;
    // The piece from the first line's start keeps the line's id.
    let m = segments.iter().position(|s| s.0 == line).ok_or_else(|| DocError::Invalid("the member feature has no lines".into()))?;
    let (axes, _) = section_axes(&segments, &planes, plan.as_ref(), turn, &vec![0.0; segments.len()], up)?;
    let (x, y) = axes.get(m).copied().ok_or_else(|| DocError::Invalid("the first line has no length".into()))?;
    // The whole carried to world as the component is shown.
    let t = placed;
    let unit = |v: Vec3| crate::apply_vector(t, v).normalized().ok_or_else(|| DocError::Invalid("the component frame is degenerate".into()));
    let (x, y) = (unit(x)?, unit(y)?);
    Ok(SectionView { start: crate::apply_point(t, a), end: crate::apply_point(t, b), x, y, regions, at, origin, mirror: *mirror })
}

/// The member of feature `f` with id `line` (a line, a pair, or a piece of a line cut where tubes cross, [`pieced`]),
/// as its body was made: the plane square to its line at its start as cut (x × y along the line; a square corner cut moves the start
/// to the cut), its length as cut, its profile in that plane, and whether an End Cap may go on its [start, end]: an
/// uncut end, or a through member's end of a Butt or Coped corner, Auto's included (not the butting member's end,
/// which lies against the through member, a cope, a fit or a mitre, nor a piece's end where it is cut, fitted to the
/// tube that crosses there). A piece split off where another tube overlaps it (`L>X`) is laid as L, only L's end open.
pub(crate) fn member_section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
) -> Result<(Plane, f64, Region2, [bool; 2])> {
    let original = untrimmed_member_section(doc, vals, f, st, line)?;
    trimming::section(doc, vals, f, st, line, original)
}

fn untrimmed_member_section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
) -> Result<(Plane, f64, Region2, [bool; 2])> {
    if paths::uses(st, &f.kind)? {
        return paths::member_section(doc, vals, f, st, line);
    }
    if let Some(groups) = frame::group_features(f)? {
        let group = groups
            .into_iter()
            .find(|g| {
                matches!(&g.kind,
            FeatureKind::FrameMember { lines, .. } if lines.is_empty() || lines.iter().any(|id| id == line))
            })
            .ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
        return untrimmed_member_section(doc, vals, &group, st, line);
    }
    let FeatureKind::FrameMember { corner, corners, gap, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { segments, regions, at, turn, sketch_x, drawn, planes, turns, centre, up, .. } = layout(doc, vals, f, st)?;
    let gap = gap_value(vals, gap.as_ref(), "`gap`")?;
    let corners = &overrides(*corner, (&segments, &planes, &drawn, sketch_x), corners, Some((vals, gap)))?;
    let (plan, crossing) = plans(*corner, &segments, &planes, corners, &drawn)?;
    let overlaps = overlap_overrides(*corner, &segments, &planes, corners);
    // Each member's Rotate turn (#15070), by its line, as [`built`] turns it.
    let own: Vec<f64> =
        member_lines_of(&segments, plan.as_ref().or(crossing.as_ref())).iter().map(|l| turns.get(l).copied().unwrap_or(0.0)).collect();
    let ats: Vec<Vec2> = own.iter().map(|t| turned_at(at, centre, *t)).collect();
    let (segments, planes, drawn) = pieced(segments, planes, drawn, plan.as_ref().or(crossing.as_ref()))?;
    // A piece split off where another tube overlaps it (`L>X`) lies along L, never from L's start: its ends at the cuts
    // lie inside L, so only L's end may be open.
    let ids: Vec<String> = segments.iter().map(|s| s.0.clone()).collect();
    let (m, later) = match ids.iter().position(|id| id == line) {
        Some(m) => (Some(m), false),
        None => (overlap_parent(&ids, line).and_then(|l| ids.iter().position(|id| id == l)), true),
    };
    let m = m.ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let (axes, _) = section_axes(&segments, &planes, plan.as_ref(), turn, &own, up)?;
    let (x, y) = axes.get(m).copied().ok_or_else(|| DocError::Invalid(format!("line `{line}` has no length")))?;
    let (a, b) = segments.get(m).map(|s| (s.1, s.2)).ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let placed = placer(&axes, &ats);
    // Each end where the member is cut, when the cut is square to its line (a mitre is not: End
    // Cap refuses it from the face itself).
    // ponytail: a corner or chain corner Fit Ends turns Free (inside a tube) is read here as jointed, as is an end
    // fitted to a tube as open; finding them builds every earlier tool, which End Cap would pay on every pick.
    let ends = frame_ends(&f.kind, corners, &segments, &regions, &placed, (plan.or(crossing), &planes, &overlaps), (sketch_x, &drawn), gap)?;
    let dir = (b - a).normalized().ok_or_else(|| DocError::Invalid(format!("line `{line}` has no length")))?;
    let [start, end] = ends.get(m).copied().unwrap_or([None, None]);
    let square = |e: Option<CutEnd>, p: Vec3| match e {
        Some((Some(c), _, _, _)) if c.normal.dot(dir).abs() > 1.0 - 1e-9 => (c.origin - p).dot(dir),
        _ => 0.0,
    };
    let (lo, hi) = (square(start, a), square(end, b));
    let at = ats.get(m).copied().unwrap_or(at);
    let plane =
        Plane::new(a + dir * lo - x * at.x - y * at.y, x, y).ok_or_else(|| DocError::Invalid(format!("line `{line}`: no plane square to it")))?;
    let (plane, sections) = along(plane, &regions, dir);
    let region = sections.first().cloned().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    let open = |e: Option<CutEnd>| e.is_none_or(|e| e.2);
    Ok((plane, a.dist(b) + hi - lo, region, [open(start) && !later, open(end)]))
}

/// The point member `line` of `f` turns about, halfway along it: the centre of the profile's
/// bounding box, which a Rotate step's turn leaves where it is ([`turned_at`]).
pub(crate) fn member_pivot(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState, line: &str) -> Result<Vec3> {
    if paths::uses(st, &f.kind)? {
        return paths::member_pivot(doc, vals, f, st, line);
    }
    if let Some(groups) = frame::group_features(f)? {
        let group = groups
            .into_iter()
            .find(|g| {
                matches!(&g.kind,
            FeatureKind::FrameMember { lines, .. } if lines.is_empty() || lines.iter().any(|id| id == line))
            })
            .ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
        return member_pivot(doc, vals, &group, st, line);
    }
    let FeatureKind::FrameMember { corner, corners, gap, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { segments, at, turn, sketch_x, drawn, planes, centre, up, .. } = layout(doc, vals, f, st)?;
    let gap = gap_value(vals, gap.as_ref(), "`gap`")?;
    let corners = &overrides(*corner, (&segments, &planes, &drawn, sketch_x), corners, Some((vals, gap)))?;
    let (plan, crossing) = plans(*corner, &segments, &planes, corners, &drawn)?;
    let (segments, planes, _) = pieced(segments, planes, drawn, plan.as_ref().or(crossing.as_ref()))?;
    let unknown = || DocError::Unknown(format!("line `{line}` of the member feature"));
    let m = segments.iter().position(|s| s.0 == line).ok_or_else(unknown)?;
    let (axes, _) = section_axes(&segments, &planes, plan.as_ref(), turn, &vec![0.0; segments.len()], up)?;
    let (x, y) = axes.get(m).copied().ok_or_else(|| DocError::Invalid(format!("line `{line}` has no length")))?;
    let (a, b) = segments.get(m).map(|s| (s.1, s.2)).ok_or_else(unknown)?;
    Ok(a + x * (centre.x - at.x) + y * (centre.y - at.y) + (b - a) * 0.5)
}

/// The body name of the member on each of `lines` (member ids): its own name from `pinned` (by
/// member id) when no other body has it, else the first free one of `base`, `base (2)`, `base (3)`…
/// that no member has pinned. A later piece of a cut line (`later`, [`LaterPieces`]; none for a
/// None frame) is named after its line and the tube it starts at, `<L's name> · <X's
/// name>`, when that is free. A member therefore keeps its name when other lines are added,
/// deleted or reordered, or cut.
pub fn member_names(
    st: &ModelState,
    lines: &[String],
    later: &[Option<(usize, usize)>],
    pinned: &BTreeMap<String, String>,
    base: &str,
) -> Vec<String> {
    names_with(&|n| st.body(n).is_some(), st.bodies.len(), lines, later, pinned, base)
}

/// [`member_names`], `taken` saying which names other bodies have (`bodies` of them at most).
fn names_with(
    taken: &dyn Fn(&str) -> bool,
    bodies: usize,
    lines: &[String],
    later: &[Option<(usize, usize)>],
    pinned: &BTreeMap<String, String>,
    base: &str,
) -> Vec<String> {
    let reserved: HashSet<&str> = pinned.values().map(String::as_str).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<Option<String>> = lines
        .iter()
        .map(|line| {
            let n = pinned.get(line).map(|n| n.trim()).filter(|n| !n.is_empty() && !taken(n))?;
            used.insert(n.to_string()).then(|| n.to_string())
        })
        .collect();
    let base = if base.trim().is_empty() { "Member" } else { base.trim() };
    let free = |n: &str, used: &HashSet<String>| !taken(n) && !used.contains(n) && !reserved.contains(n);
    // Each candidate is refused at most once per body, used or reserved name: past that many,
    // a free one has been found for every member.
    let limit = bodies + reserved.len() + lines.len() + 2;
    let mut next = 1usize;
    let mut number = |used: &mut HashSet<String>| {
        while next <= limit {
            let candidate = if next == 1 { base.to_string() } else { format!("{base} ({next})") };
            next += 1;
            if free(&candidate, used) {
                used.insert(candidate.clone());
                return Some(candidate);
            }
        }
        None
    };
    let piece = |k: usize| later.get(k).copied().flatten();
    // Every line and pair first, so a piece never takes a number from one.
    for (k, slot) in out.iter_mut().enumerate() {
        if slot.is_none() && piece(k).is_none() {
            *slot = number(&mut used);
        }
    }
    for k in 0..out.len() {
        let Some((l, x)) = piece(k).filter(|_| out.get(k).is_some_and(Option::is_none)) else { continue };
        let name_of = |i: usize| out.get(i).cloned().flatten();
        let joined = name_of(l).zip(name_of(x)).map(|(l, x)| format!("{l} · {x}")).filter(|n| free(n, &used));
        let name = match joined {
            Some(n) => {
                used.insert(n.clone());
                Some(n)
            }
            None => number(&mut used),
        };
        if let Some(slot) = out.get_mut(k) {
            *slot = name;
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
    if paths::uses(st, &source.kind)? {
        paths::member_pivot(doc, vals, source, st, &member.line)?;
    }
    turn_angle(vals, angle).map(|_| ())
}

/// The ids of a `FrameMember` feature's members in `st`, in the order they are made: its lines',
/// under every type but None each line's pieces' ([`frame_layout::Piece`]), then its pairs'.
pub fn member_line_ids(st: &ModelState, kind: &FeatureKind) -> Result<Vec<String>> {
    member_parts(st, kind).map(|(ids, _)| ids)
}

/// [`member_line_ids`], and which of them are later pieces of a cut line ([`LaterPieces`]).
pub fn member_parts(st: &ModelState, kind: &FeatureKind) -> Result<(Vec<String>, LaterPieces)> {
    if paths::uses(st, kind)? {
        return paths::member_parts(st, kind);
    }
    if let FeatureKind::FrameMember { groups, path_sketch: Some(path), .. } = kind
        && !groups.is_empty()
    {
        frame::check_group_options(kind)?;
        let ps = st.sketch(path.0).ok_or_else(|| DocError::Unknown(format!("sketch {path}")))?;
        let lines = group_lines(ps, groups)?;
        let ids: Vec<String> = lines.into_iter().flatten().map(|s| s.0).collect();
        let later = vec![None; ids.len()];
        return Ok((ids, later));
    }
    frame::check_group_options(kind)?;
    let FeatureKind::FrameMember { path_sketch, lines, pairs, corner, corners, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let (mut ids, mut later): (Vec<String>, LaterPieces) = match path_sketch {
        Some(path_sketch) => {
            let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch}")))?;
            let segments = member_lines(ps, lines)?;
            // Every type but None cuts crossings (#4003).
            if !corner.is_none() {
                // A design file is hostile input: as `plans` checks it.
                if corners.len() > MAX_MEMBERS || corners.iter().any(|o| o.members.len() > MAX_MEMBERS) {
                    return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} corner overrides of at most {MAX_MEMBERS} members in one feature")));
                }
                // Only the sketch's own lines cut each other, so they give every piece. Laid out where the sketch
                // is, not in the feature's frame: moved as one, the lines cross where they did.
                let planes = vec![ps.sketch.wires.is_empty().then(|| (ps.plane.normal(), ps.plane.x)); segments.len()];
                let drawn = drawn_order(&ps.sketch, &segments);
                // The sketch's own X breaks no tie that cuts a line: only a crossing's order does.
                let corners = overrides(*corner, (&segments, &planes, &drawn, None), corners, None)?;
                let pieces = frame_layout::pieces(&segments, &planes, &drawn, &corners);
                if pieces.len() + pairs.len() > MAX_MEMBERS {
                    return Err(too_many_pieces());
                }
                let later = later_pieces(&pieces, &segments);
                (pieces.into_iter().map(|p| p.id).collect(), later)
            } else {
                let n = segments.len();
                (segments.into_iter().map(|(id, _, _)| id).collect(), vec![None; n])
            }
        }
        None => (Vec::new(), Vec::new()),
    };
    ids.extend(pairs.iter().map(|p| p.id.clone()));
    later.resize(ids.len(), None);
    Ok((ids, later))
}

/// The member (line, pair or piece id) of Frame Member `f` whose body is `body`, if one is: by
/// position when the feature names its bodies itself, else by the name evaluation gives it ([`member_names`]: its
/// `names` entry, or for a later piece of a crossing with none, `<its line's name> · <its crossing tube's>`). A piece
/// split off where another tube overlaps it, found only as the frame is built, is `L>X` ([`overlap_parent`]): by its
/// `names` entry, or by its name `<L's name> · <X's name>`, numbered `(n)` or not (X a line's or pair's, never a
/// piece's). Names are worked out as they were when `f` was built: against the state just before it, when its bodies
/// keep it.
pub fn member_of_body(doc: &Document, st: &ModelState, f: &Feature, body: &str) -> Result<Option<String>> {
    Ok(members_of_bodies(doc, st, f, &[body])?.into_iter().next().flatten())
}

/// [`member_of_body`] of each of `bodies`, the frame laid out once.
pub fn members_of_bodies(doc: &Document, st: &ModelState, f: &Feature, bodies: &[&str]) -> Result<Vec<Option<String>>> {
    members_at(doc, st, f, bodies, 0)
}

/// Most earlier frames an overlap piece's id is followed back through ([`earlier_id`]): the piece's cutter is an
/// earlier frame's piece, whose own cutter is a piece of a frame before that, and so on. Past this many the cutter is
/// named by its body name, so the walk never runs deep enough to overflow the stack, which nothing catches.
const MAX_NAME_DEPTH: usize = 16;

#[cfg(test)]
thread_local! {
    /// The deepest [`members_at`] has gone on this thread.
    pub(crate) static NAME_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`members_of_bodies`], `depth` frames back along overlap pieces' cutters ([`MAX_NAME_DEPTH`]).
pub(super) fn members_at(doc: &Document, st: &ModelState, f: &Feature, bodies: &[&str], depth: usize) -> Result<Vec<Option<String>>> {
    #[cfg(test)]
    NAME_DEPTH.set(NAME_DEPTH.get().max(depth));
    let FeatureKind::FrameMember { names, body_name, profile, .. } = &f.kind else { return Ok(vec![None; bodies.len()]) };
    let (ids, later) = member_parts(st, &f.kind)?;
    if !f.body_names.is_empty() {
        // Position pairs a body with a member only when every member made one; a piece left with no body shifts
        // the rest, and the caller then matches by shape.
        let made = st.bodies.iter().filter(|b| b.feature == f.id).count();
        let at = |body: &str| f.body_names.iter().position(|name| name == body).and_then(|i| ids.get(i)).cloned();
        return Ok(bodies.iter().map(|b| if made == ids.len() { at(b) } else { None }).collect());
    }
    if matches!(&f.kind, FeatureKind::FrameMember { groups, .. } if !groups.is_empty()) {
        let default = profile_regions(&doc.evaluation_values(), st, profile).map(|r| r.1).unwrap_or_default();
        let bases = group_bases(&f.kind, &default, ids.len());
        let before = state_before(doc, st, f);
        let named = match before.as_deref() {
            Some(b) => group_member_names(b, &ids, &bases, names),
            None => group_names_with(&|n| st.body(n).is_some_and(|b| b.feature != f.id), st.bodies.len(), &ids, &bases, names),
        };
        return Ok(bodies.iter().map(|b| named.iter().position(|n| n == b).and_then(|k| ids.get(k)).cloned()).collect());
    }
    // The names evaluation gives (`built`): against the bodies of everything but this feature.
    let base = match body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => profile_regions(&doc.evaluation_values(), st, profile).map(|r| r.1).unwrap_or_default(),
    };
    // Against the state just before the frame was built when its bodies keep it, so a later join that frees a name
    // moves no unpinned name; else against everything but this feature's bodies now.
    let before = state_before(doc, st, f);
    let named = match before.as_deref() {
        Some(b) => names_with(&|n| b.body(n).is_some(), b.bodies.len(), &ids, &later, names, &base),
        None => names_with(&|n| st.body(n).is_some_and(|b| b.feature != f.id), st.bodies.len(), &ids, &later, names, &base),
    };
    let then = before.as_deref().unwrap_or(st);
    let here = doc.features.iter().position(|g| g.id == f.id);
    // X's part of a piece id, from X's name: one of this frame's members (not L), by its line (a piece's line's), else
    // an earlier tube ([`earlier_id`]).
    let cutter = |l: usize, x: &str| match named.iter().enumerate().find(|(k, n)| *k != l && *n == x) {
        Some((k, _)) => ids.get(later.get(k).copied().flatten().map_or(k, |(line, _)| line)).cloned(),
        None => earlier_id(doc, then, here, x, depth),
    };
    // A piece split off where another tube overlaps it ([`resolve_overlaps`]), with no pin of its own:
    // `<L's name> · <X's name>`. The longest L's name wins: a name may hold ` · ` itself.
    let overlap = |body: &str| {
        let mut best: Option<(usize, String)> = None;
        for (l, n) in named.iter().enumerate() {
            let Some(x) = body.strip_prefix(n.as_str()).and_then(|r| r.strip_prefix(" · ")) else { continue };
            if best.as_ref().is_some_and(|(len, _)| *len >= n.len()) {
                continue;
            }
            if let (Some(id), Some(x)) = (ids.get(l), cutter(l, x)) {
                best = Some((n.len(), format!("{id}>{x}")));
            }
        }
        best.map(|b| b.1)
    };
    let one = |body: &str| {
        if let Some(k) = named.iter().position(|n| n == body) {
            return ids.get(k).cloned();
        }
        // A renamed piece is pinned under its piece id.
        if let Some((id, _)) = names.iter().find(|(id, n)| *n == body && overlap_parent(&ids, id).is_some()) {
            return Some(id.clone());
        }
        // Numbered `(n)` when another body had its name.
        overlap(body).or_else(|| unnumbered(body).and_then(overlap))
    };
    Ok(bodies.iter().map(|b| one(b)).collect())
}

/// `name` without a trailing ` (n)`, if it has one.
fn unnumbered(name: &str) -> Option<&str> {
    let (rest, n) = name.strip_suffix(')')?.rsplit_once(" (")?;
    (!n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())).then_some(rest)
}

/// What an overlap piece's id calls the earlier tube `body` that cut it (ruling O4), for the feature at `here` in the
/// timeline, `depth` frames back ([`members_at`]): an earlier Frame Member's own tube by its feature and member
/// ([`frame_member_id`]), anything else (a Move, Mirror or Pattern copy, a tube whose member isn't found, or one past
/// [`MAX_NAME_DEPTH`]) by its body name. `None` when no earlier body has it.
fn earlier_id(doc: &Document, st: &ModelState, here: Option<usize>, body: &str, depth: usize) -> Option<String> {
    let g = doc.feature(st.body(body)?.feature)?;
    if doc.features.iter().position(|h| h.id == g.id) >= here {
        return None;
    }
    if !matches!(g.kind, FeatureKind::FrameMember { .. }) || depth >= MAX_NAME_DEPTH {
        return Some(body.to_string());
    }
    let found = members_at(doc, st, g, &[body], depth + 1).ok().and_then(|m| m.into_iter().next().flatten());
    Some(found.map_or_else(|| body.to_string(), |m| frame_member_id(g.id, &m)))
}

/// The state just before Frame Member `f` was built, as its bodies' naming origins keep it (a body a later feature
/// changed is followed back through the state before each change, at most once per feature). `None` when none does.
fn state_before(doc: &Document, st: &ModelState, f: &Feature) -> Option<std::sync::Arc<ModelState>> {
    for b in st.bodies.iter().filter(|b| b.feature == f.id) {
        let mut origin = b.names.origin.clone();
        for _ in 0..=doc.features.len() {
            let Some(o) = origin else { break };
            if o.feature.id == f.id {
                return Some(o.before.clone());
            }
            origin = o.before.body(&b.name).and_then(|x| x.names.origin.clone());
        }
    }
    None
}

/// An earlier Frame Member's own tube as an overlap piece's id names its cutter: `<feature id>:<member id>`.
pub(crate) fn frame_member_id(feature: u64, member: &str) -> String {
    format!("{feature}:{member}")
}

/// The member of `ids` that `piece`, a piece split off where another tube overlaps it (`L>X`, [`resolve_overlaps`]),
/// is cut from: the longest id that `piece` starts with, then `>`. `None` for one of `ids` itself.
pub fn overlap_parent<'a>(ids: &'a [String], piece: &str) -> Option<&'a String> {
    if ids.iter().any(|id| id == piece) {
        return None;
    }
    ids.iter().filter(|id| piece.strip_prefix(id.as_str()).is_some_and(|x| x.len() > 1 && x.starts_with('>'))).max_by_key(|id| id.len())
}

/// Each pair's ends, found again as construction geometry finds its points: in this feature's
/// component frame, where its lines are laid out, so both take the same "up". A pair whose point
/// is gone, or whose ends now meet, fails the feature, naming the pair.
fn pair_lines(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    pairs: &[MemberPair],
) -> Result<Vec<(String, Vec3, Vec3)>> {
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    if pairs.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature")));
    }
    let cst = super::construction_state(doc, st, f.component)?;
    let ctx = crate::construct::Ctx { doc, vals, st: &cst, depth: 0, component: f.component };
    pairs
        .iter()
        .map(|p| {
            let end = |g, which: &str| match crate::construct::point(&ctx, g) {
                Ok(v) if v.is_finite() => Ok(v),
                Ok(_) => Err(DocError::Invalid(format!("{}: its {which} point is not a finite point", p.id))),
                Err(e) => Err(DocError::Invalid(format!("{}: its {which} point is gone ({e})", p.id))),
            };
            let (a, b) = (end(&p.a, "first")?, end(&p.b, "second")?);
            if !(a.dist(b) > MIN_LENGTH) {
                return Err(DocError::Invalid(format!("{}: its two points are in one place (it has no length)", p.id)));
            }
            Ok((p.id.clone(), a, b))
        })
        .collect()
}

/// The member lines that end at the sketch's point `point` (a point id, or a 3D sketch's
/// `wire:index` node), among `lines` (none: every line of the sketch, as a feature with no lines
/// takes them). A Corner Override names its corner by these. Under `auto`, two or more lines end
/// there and none passes through (spec decision 3); otherwise exactly two, a corner as
/// [`frame_corners::find_corners`] finds them. Under `auto` a crossing is one too: a point two or
/// more lines pass through (others may end there), named by the lines passing through. Only the sketch's lines are
/// looked at: a corner where a point-to-point member ends is named through `corners` with the pair's id.
pub fn corner_at_point(st: &ModelState, sketch: u64, lines: &[String], point: &str, auto: bool) -> Result<Vec<String>> {
    let ss = st.sketch(sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let at =
        crate::construct::sketch_point_position(st, sketch, point).ok_or_else(|| DocError::Unknown(format!("point `{point}` in {}", ss.name)))?;
    let segments = member_lines(ss, lines)?;
    if auto {
        return match ending_and_through(&segments, at) {
            // A crossing, whether or not other lines end there too: the lines crossing name it.
            (_, through) if through.len() >= 2 => Ok(through),
            (ending, through) if ending.len() >= 2 && through.is_empty() => Ok(ending),
            _ => Err(DocError::Invalid("Pick a corner of the frame: a point where two or more members end, or a crossing".into())),
        };
    }
    let tol = frame_corners::CORNER_TOL;
    let not_a_corner = || DocError::Invalid("Pick a corner of the frame: a point where exactly two members end".into());
    let corners = frame_corners::find_corners(&segments);
    let corner = corners.iter().find(|c| c.point.dist(at) <= tol).ok_or_else(not_a_corner)?;
    let [(a, _), (b, _)] = corner.ends;
    let id = |m: usize| segments.get(m).map(|s| s.0.clone());
    id(a).zip(id(b)).map(|(a, b)| vec![a, b]).ok_or_else(not_a_corner)
}

/// The lines among `segments` that end at `at`, and those that pass through it.
fn ending_and_through(segments: &[(String, Vec3, Vec3)], at: Vec3) -> (Vec<String>, Vec<String>) {
    let tol = frame_corners::CORNER_TOL;
    let (mut ending, mut through) = (Vec::new(), Vec::new());
    for (id, a, b) in segments {
        if a.dist(at) <= tol || b.dist(at) <= tol {
            ending.push(id.clone());
        } else if at.dist_to_segment(*a, *b) <= tol {
            through.push(id.clone());
        }
    }
    (ending, through)
}

/// Whether the sketch's point `point` is a crossing of `lines` (none: every line of the sketch):
/// two or more pass through it, whether or not others end there. An Auto override there is a crossing's.
pub fn is_crossing(st: &ModelState, sketch: u64, lines: &[String], point: &str) -> bool {
    let (Some(ss), Some(at)) = (st.sketch(sketch), crate::construct::sketch_point_position(st, sketch, point)) else { return false };
    member_lines(ss, lines).is_ok_and(|segments| matches!(ending_and_through(&segments, at), (_, through) if through.len() >= 2))
}

/// Whether the sketch's lines `members` (two or more) all pass through one point, which none of them ends at: the
/// crossing an Auto override of those lines names, whether or not a sketch point marks it (typically none does
/// where an X brace's diagonals cross).
pub fn members_cross(st: &ModelState, sketch: u64, members: &[String]) -> bool {
    let Some(lines) = st.sketch(sketch).and_then(|ss| member_lines(ss, members).ok()) else { return false };
    let [(_, a, b), (_, c, d), ..] = lines.as_slice() else { return false };
    let (p, q) = solidcraft_geom::mesh_distance::closest_on_segments(*a, *b, *c, *d);
    p.dist(q) <= frame_corners::CORNER_TOL
        && matches!(ending_and_through(&lines, (p + q) * 0.5), (ending, through) if ending.is_empty() && through.len() == lines.len())
}

/// The sketch point where the lines `members` (two or more) all end together (an end point of
/// one of them, named as a pick names it), or else a point of the sketch where they all cross
/// with none ending there; `None` when they no longer meet.
pub fn corner_point(st: &ModelState, sketch: u64, members: &[String]) -> Option<String> {
    let ss = st.sketch(sketch)?;
    if members.len() < 2 {
        return None;
    }
    let lines = member_lines(ss, members).ok()?;
    let tol = frame_corners::CORNER_TOL;
    let (_, a, b) = lines.first()?;
    let Some(point) = [*a, *b].into_iter().find(|&p| lines.iter().all(|(_, a, b)| a.dist(p) <= tol || b.dist(p) <= tol)) else {
        // ponytail: every point of the sketch is tried; find the lines' crossing first if a large sketch makes edits slow.
        let crossed = |p: &str| {
            crate::construct::sketch_point_position(st, sketch, p)
                .is_some_and(|at| matches!(ending_and_through(&lines, at), (ending, through) if ending.is_empty() && through.len() == lines.len()))
        };
        return ss.sketch.points.iter().map(|p| p.id.clone()).find(|p| crossed(p));
    };
    let sk = &ss.sketch;
    let ends = |id: &String| -> Vec<String> {
        if let Some(w) = sk.wire_index(id).and_then(|i| sk.wires.get(i)) {
            return vec![format!("{id}:0"), format!("{id}:{}", w.fit.len().saturating_sub(1))];
        }
        let ends = sk.curve_index(id).and_then(|i| sk.curves.get(i)).and_then(|c| c.kind.ends());
        ends.map(|(a, b)| [a, b].iter().filter_map(|&i| sk.points.get(i).map(|p| p.id.clone())).collect()).unwrap_or_default()
    };
    members.iter().flat_map(ends).find(|p| crate::construct::sketch_point_position(st, sketch, p).is_some_and(|at| at.dist(point) <= tol))
}

/// Whether curve `ci` of `sk` is one a frame member is laid on unless lines are picked: a straight
/// line drawn in it (not construction, not a centreline, not projected or otherwise linked). A
/// marquee in the Frame Member dialog picks these.
pub fn is_member_line(sk: &Sketch, ci: usize) -> bool {
    sk.curves.get(ci).is_some_and(|c| !c.construction && !c.centerline && c.link.is_none() && matches!(c.kind, CurveKind::Line { .. }))
}

/// Whether wire `wi` of `sk` is a 3D line a frame member is laid on unless lines are picked: a
/// straight one drawn in it (not projected or otherwise linked), as [`member_lines`] takes them.
pub fn is_member_wire(sk: &Sketch, wi: usize) -> bool {
    sk.wires.get(wi).is_some_and(|w| w.link.is_none() && wire_line(w).is_ok_and(|line| line.is_some()))
}

/// Each member's place in the sketch's drawing order: a 2D line's curve index, a 3D line's wire
/// index after them (looked up as [`member_lines`] looks them up). A pair's member isn't drawn in
/// the sketch: it ranks after every line, and pairs tie among themselves, which `frame_corners::
/// through` then settles by member index, the pairs' own order. (With no sketch, `drawn` is empty
/// and every member ranks by its index alike.)
fn drawn_order(sk: &Sketch, segments: &[(String, Vec3, Vec3)]) -> Vec<usize> {
    let rank = |id: &str| sk.wire_index(id).map(|w| sk.curves.len() + w).or_else(|| sk.curve_index(id)).unwrap_or(usize::MAX);
    segments.iter().map(|(id, _, _)| rank(id)).collect()
}

/// The member lines of the frame sketch, each as (curve id, start, end) in its frame: the lines
/// asked for, or every straight line drawn in it ([`is_member_line`], with a length).
fn member_lines(ss: &SolvedSketch, ids: &[String]) -> Result<Vec<(String, Vec3, Vec3)>> {
    let sk = &ss.sketch;
    let mut out = Vec::new();
    if ids.is_empty() {
        for (ci, c) in sk.curves.iter().enumerate() {
            if is_member_line(sk, ci)
                && let Some((a, b)) = planar_line(ss, ci)
                && a.dist(b) > MIN_LENGTH
            {
                out.push((c.id.clone(), a, b));
            }
        }
        for w in sk.wires.iter().filter(|w| w.link.is_none()) {
            if let Some((a, b)) = wire_line(w)?
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
            match sk.wires.get(wi) {
                Some(w) => wire_line(w)?,
                None => None,
            }
        } else {
            let ci = sk.curve_index(id).ok_or_else(|| DocError::Unknown(format!("line `{id}` in {}", ss.name)))?;
            planar_line(ss, ci)
        };
        let (a, b) = line.ok_or_else(|| DocError::Invalid(format!("`{id}` is not a straight line or an exact circular arc")))?;
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

/// Exact straight stock only: defining line points or one retained analytic line.
/// A display polyline never turns a spline into straight member stock.
fn wire_line(w: &solidcraft_sketch::Wire) -> Result<Option<(Vec3, Vec3)>> {
    paths::wire_line(w).map(|line| line.map(|p| (p.a, p.b)))
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
        return Err(crate::eval::no_closed_profiles(ss));
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

pub fn group_member_names(st: &ModelState, lines: &[String], bases: &[String], pinned: &BTreeMap<String, String>) -> Vec<String> {
    group_names_with(&|n| st.body(n).is_some(), st.bodies.len(), lines, bases, pinned)
}

fn group_names_with(
    taken: &dyn Fn(&str) -> bool,
    bodies: usize,
    lines: &[String],
    bases: &[String],
    pinned: &BTreeMap<String, String>,
) -> Vec<String> {
    let reserved: HashSet<&str> = pinned.values().map(String::as_str).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<Option<String>> = lines
        .iter()
        .map(|line| {
            let n = pinned.get(line).map(|n| n.trim()).filter(|n| !n.is_empty() && !taken(n))?;
            used.insert(n.to_string()).then(|| n.to_string())
        })
        .collect();
    // Each candidate is refused at most once per body, used or reserved name: past that many,
    // a free one has been found for every member.
    let limit = bodies + reserved.len() + lines.len() + 2;
    let mut next: HashMap<&str, usize> = HashMap::new();
    for (slot, base) in out.iter_mut().zip(bases.iter().map(|b| if b.trim().is_empty() { "Member" } else { b.trim() })) {
        if slot.is_some() {
            continue;
        }
        let next = next.entry(base).or_insert(1);
        while *next <= limit {
            let candidate = if *next == 1 { base.to_string() } else { format!("{base} ({next})") };
            *next += 1;
            if !taken(&candidate) && !used.contains(&candidate) && !reserved.contains(candidate.as_str()) {
                used.insert(candidate.clone());
                *slot = Some(candidate);
                break;
            }
        }
    }
    let base = |k: usize| bases.get(k).map(|b| b.trim()).filter(|b| !b.is_empty()).unwrap_or("Member").to_string();
    out.into_iter().enumerate().map(|(k, n)| n.unwrap_or_else(|| base(k))).collect()
}
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

fn group_bases(kind: &FeatureKind, default: &str, count: usize) -> Vec<String> {
    let groups = frame::member_groups(kind);
    let base = |g: &MemberGroup| g.body_name.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(default).to_string();
    match groups.as_slice() {
        [one] => vec![base(one); count],
        several => several.iter().flat_map(|g| std::iter::repeat_n(base(g), g.lines.len())).collect(),
    }
}

/// Nominal section and terminal datums, from the same transport the sweep uses.
fn round_record(
    vals: &BTreeMap<String, Value>,
    profile: &MemberProfile,
    at: Vec2,
    turn: f64,
    path: &[solidcore_record::frame::PieceData],
    straight_frame: Option<(Plane, f64)>,
) -> Result<Option<crate::frame_cope::RoundMember>> {
    use crate::frame_cope::{Bounds, RoundMember, Terminal};
    use solidcore_record::frame::Segment;
    use solidcraft_kernel::services::tube_cope;
    let MemberProfile::Library { profile } = profile else { return Ok(None) };
    let frame::Profile::RoundTube { diameter, wall } = profile.resolve(|_, e, kind| val(vals, e, kind))? else { return Ok(None) };
    // An off-centre circular profile sweeps about a different centreline at a bend.
    // Straight stock uses the actual extrusion frame, including its centre-pivot translation.
    if straight_frame.is_none() && (at.x.abs() > 1e-9 || at.y.abs() > 1e-9) {
        return Ok(None);
    }
    let radius = diameter / 2.0;
    let fail = |e: &dyn std::fmt::Display| DocError::Invalid(format!("the member's cope datum could not be placed: {e}"));
    let pieces = path;
    let bounds: Vec<Bounds> = pieces
        .iter()
        .map(|p| match p.segment {
            Segment::Line { a, b } => tube_cope::line_box(a, b, radius).into(),
            Segment::Arc { a, center, .. } => tube_cope::arc_box(a, center, radius).into(),
        })
        .collect();
    let [first, last] = match (pieces.first(), pieces.last()) {
        (Some(f), Some(l)) => [f, l],
        _ => return Err(DocError::Invalid("empty member path".into())),
    };
    let [start_plane, end_plane] = match straight_frame {
        // Reuse the very plane extruded above: recomputing from the path and global turn
        // would lose both the Rotate step's seam direction and the profile-centre pivot.
        Some((plane, length)) => [plane, plane.offset(length)],
        None => kernel::stock_terminal_planes(pieces, Vec2::ZERO, turn).map_err(|e| fail(&e))?,
    };
    let start = match kernel::stock_line_length(first).map_err(|e| fail(&e))? {
        Some(length) => {
            Some(Terminal::new(tube_cope::terminal_at(&start_plane, length, false).map_err(|e| fail(&e))?, bounds.iter().skip(1).copied().collect()))
        }
        None => None,
    };
    let end = match kernel::stock_line_length(last).map_err(|e| fail(&e))? {
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

/// Optional manufacturing records are attached only after the complete construction route
/// emitted its final bodies. A cap's open-end hints are not proof of pristine stock.
fn certify_stock(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, emitted_from: usize, path_route: bool) {
    if !crate::frame_cope::stock_eligible(&f.kind) {
        return;
    }
    let FeatureKind::FrameMember { profile, .. } = &f.kind else { return };
    let produced: Vec<_> =
        st.bodies.iter().enumerate().skip(emitted_from).filter(|(_, b)| b.feature == f.id).map(|(i, b)| (i, b.name.clone())).collect();
    let names: Vec<_> = produced.iter().map(|(_, name)| name.as_str()).collect();
    let Ok(ids) = members_of_bodies(doc, st, f, &names) else { return };
    let path_records = if path_route {
        let Ok(records) = paths::cope_records(doc, vals, f, st) else { return };
        records
    } else {
        BTreeMap::new()
    };
    let records: Vec<_> = produced
        .iter()
        .zip(ids)
        .map(|((index, _), id)| {
            let record = id.and_then(|id| {
                if path_route {
                    return path_records.get(&id).cloned();
                }
                let (plane, length, _, _) = member_section(doc, vals, f, st, &id).ok()?;
                let piece = kernel::stock_line(plane.origin, plane.offset(length).origin).ok()?;
                round_record(vals, profile, Vec2::ZERO, 0.0, &[piece], Some((plane, length))).ok().flatten()
            });
            (*index, record)
        })
        .collect();
    for (index, record) in records {
        if let (Some(body), Some(record)) = (st.bodies.get_mut(index), record) {
            body.certify_round_member(record);
        }
    }
}
