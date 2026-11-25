//! Multi-trim (#4003 `frame.multi_trim`): a tube cut by the shapes of other tubes — end cuts, copes, notches in a
//! tube's middle, overlaps and fits to earlier frames.
//!
//! A tube end is cut to the tubes it meets, as a fabricator fits it. The tool is the other tube with its hole filled
//! and its corner radii squared off, built from its section ([`squared_outline`]): a fitted end stops flat on a flat
//! side, and is an exact fishmouth on a round tube. A Coped frame (#17854) cuts to the other tube's real shape
//! instead, its radii kept ([`coped_tool`]). A weld gap grows the tool, so the end stands off by it. Exact geometry
//! only, never a mesh.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, BoolOp, PointClass};

use crate::frame_corners::Cut;
use crate::lineage::{Lineage, Traced, numbered_tool};
use crate::{DocError, Result};

/// The section's outer outlines as they are, radii kept; holes dropped: the tube filled.
pub fn filled_outline(regions: &[Region2]) -> Vec<Region2> {
    regions.iter().map(|r| Region2 { outer: r.outer.clone(), holes: Vec::new() }).collect()
}

/// The section's outer outlines with every outside corner arc that sits between two straight sides replaced by the
/// corner they meet at (an inside corner's fillet kept); holes dropped. A round section (no straight sides) keeps its arcs.
pub fn squared_outline(regions: &[Region2]) -> Vec<Region2> {
    regions.iter().map(|r| Region2 { outer: squared(&r.outer), holes: Vec::new() }).collect()
}

/// `outline` with each arc between two lines replaced by the point where the lines meet, when that point lies within
/// twice the arc's radius of both its ends: lines nearly parallel round an arc meet far out, and that corner would
/// stand out of the section and cut ends nowhere near the tube, so the arc stays (claude_fusion `fill.squared` keeps
/// the shape when a squared corner stands out of its radius).
fn squared(outline: &Loop2) -> Loop2 {
    let segs = &outline.segs;
    let n = segs.len();
    let line = |k: usize| match segs.get(k.checked_rem(n)?) {
        Some(Seg2::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    // Only an outside corner's arc is squared: squaring an inside corner's (an angle's or a channel's root fillet)
    // would cut into the member's material.
    let inside = solidcraft_kernel::services::section_corners::inside_corner_arcs(outline);
    // The corner replacing segment k, when it is an outside corner's arc between two lines that meet near it.
    let corner = |k: usize| {
        let at = k.checked_rem(n)?;
        let arc = segs.get(at)?;
        let Seg2::Arc { radius, .. } = *arc else { return None };
        if inside.get(at).copied().unwrap_or(false) {
            return None;
        }
        let at = meet(line(k + n - 1)?, line(k + 1)?)?;
        (at.dist(arc.start()).max(at.dist(arc.end())) <= 2.0 * radius.abs()).then_some(at)
    };
    let segs = segs
        .iter()
        .enumerate()
        .filter_map(|(k, seg)| match *seg {
            Seg2::Line { a, b } => Some(Seg2::Line { a: corner(k + n - 1).unwrap_or(a), b: corner(k + 1).unwrap_or(b) }),
            Seg2::Arc { .. } if corner(k).is_some() => None,
            _ => Some(*seg),
        })
        .collect();
    Loop2 { segs }
}

/// Where the lines through (a, b) and (c, d) meet; `None` for parallel lines.
fn meet((a, b): (Vec2, Vec2), (c, d): (Vec2, Vec2)) -> Option<Vec2> {
    let (u, v) = (b - a, d - c);
    let across = u.cross(v);
    if across.abs() <= 1e-12 * u.len() * v.len() {
        return None;
    }
    Some(a + u * ((c - a).cross(v) / across))
}

/// Whether the section is open, like an angle, a channel or a beam: some corner of an outer outline turns inward
/// (a line meeting the next at an inside corner, or an inside corner's fillet). A tube's or a bar's never does.
pub fn is_open(regions: &[Region2]) -> bool {
    regions.iter().any(|r| solidcraft_kernel::services::section_corners::is_open(&r.outer))
}

/// The corners of the rectangle round each of the section's outer outlines (an arc's whole circle's box, a curve's
/// control points'): what bounds the section, however open.
pub fn box_corners(regions: &[Region2]) -> Vec<Vec2> {
    regions
        .iter()
        .flat_map(|r| {
            let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
            let mut add = |p: Vec2| {
                if p.x.is_finite() && p.y.is_finite() {
                    lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                    hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
                }
            };
            for g in &r.outer.segs {
                match *g {
                    Seg2::Arc { center, radius, .. } => {
                        let r = radius.abs();
                        add(Vec2::new(center.x - r, center.y - r));
                        add(Vec2::new(center.x + r, center.y + r));
                    }
                    _ => {
                        add(g.start());
                        add(g.end());
                    }
                }
            }
            if lo.x > hi.x { Vec::new() } else { vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)] }
        })
        .collect()
}

/// A tube's cutting tool: the squared outline extruded on `plane` from `start` to `end` (as the tube is), then
/// the same plane cuts as the tube (stage 1 Cut list), traced as a numbered tool. A weld gap `gap` (mm) grows it by
/// that much all round ([`tool`]).
pub fn fit_tool(plane: &Plane, regions: &[Region2], start: f64, end: f64, cuts: &[Cut], gap: f64) -> Result<Traced> {
    tool(plane, &squared_outline(regions), start, end, cuts, gap)
}

/// [`fit_tool`] of the tube's real shape ([`filled_outline`]): what a Coped frame (#17854) cuts a tube to.
pub fn coped_tool(plane: &Plane, regions: &[Region2], start: f64, end: f64, cuts: &[Cut], gap: f64) -> Result<Traced> {
    tool(plane, &filled_outline(regions), start, end, cuts, gap)
}

/// `outline` extruded on `plane` from `start` to `end`, then cut by `cuts`, traced as a numbered tool. A weld gap
/// `gap` (mm) grows the outline outward by it and moves each cut out by it, built out far enough to reach the moved
/// cut, so the tool stands `gap` clear of the tube everywhere, its own cut ends included; at 0 the tube as it is.
fn tool(plane: &Plane, outline: &[Region2], start: f64, end: f64, cuts: &[Cut], gap: f64) -> Result<Traced> {
    if gap > 0.0 {
        // The kernel's message speaks of a taper: say what the gap needs.
        let refuse = |_| {
            DocError::Invalid(format!(
                "not supported yet: a weld gap of {gap} mm round this member profile, whose outline can't grow by it (an inside \
                 corner's radius smaller than the gap, or an arc meeting a side at a sharp corner)"
            ))
        };
        let grown = outline
            .iter()
            .map(|r| {
                let ccw = if r.outer.is_ccw() { r.outer.clone() } else { r.outer.reversed() };
                Ok(Region2 {
                    outer: Loop2 { segs: kernel::offset_loop(&ccw, gap).map_err(refuse)?.into_iter().flatten().collect() },
                    holes: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // A cut keeping the tool's far side (its normal along the extrusion) trims its start. Moved out by the gap, it
        // meets the grown section up to twice the gap over its slope further out.
        let (mut start, mut end) = (start, end);
        let z = plane.normal();
        for c in cuts {
            let along = c.normal.dot(z);
            if along.abs() > 1e-9 {
                let more = 2.0 * gap / along.abs();
                if along > 0.0 { start -= more } else { end += more }
            }
        }
        let moved: Vec<Cut> = cuts.iter().map(|c| Cut { origin: c.origin - c.normal * gap, normal: c.normal }).collect();
        return tool(plane, &grown, start, end, &moved, 0.0);
    }
    let mut bodies = kernel::extrude(plane, outline, start, end)?;
    let (Some(mut body), true) = (bodies.pop(), bodies.is_empty()) else {
        return Err(DocError::Invalid("a tube's fitting tool is not one solid".into()));
    };
    for cut in cuts {
        let plane = Plane::from_normal(cut.origin, cut.normal).ok_or_else(|| DocError::Invalid("a tube's corner cut has no plane".into()))?;
        body = kernel::half_space(&body, &plane, true)?.0.ok_or_else(|| DocError::Invalid("a tube's fitting tool was cut away completely".into()))?;
    }
    Ok(numbered_tool(body))
}

/// `member` with its end at `end_point` (its line's end, `toward` pointing out of the member) fitted to `tools`:
/// only tools whose exact bounds come within `w` of the end (the member's section diagonal) and that meet the end
/// zone ([`zone`], never deeper than a quarter of the member) take part, each cut only within a zone deep enough to
/// hold its whole crossing, so a tool crossing the member mid-span leaves it whole there (claude_fusion `cope_end`);
/// the piece holding `middle` (a point inside the member's material near its line's middle) is kept, or where fitting
/// took the middle away, the piece reaching farthest from the end ([`fit_piece_end`]); unchanged when no tool reaches
/// the end.
pub fn fit_end(member: &Traced, end_point: Vec3, toward: Vec3, middle: Vec3, tools: &[&Traced], id: &str) -> Result<Traced> {
    fitted(member, (end_point, toward, middle), tools, id, false)?.ok_or_else(|| gone(id))
}

/// [`fit_end`] for a piece of a line cut where tubes cross (#4003 stage 3, ruling C4): the piece kept is the one
/// reaching farthest from the end (its centre of mass farthest along the member, away from the end), so a short
/// piece past a crossing keeps what lies past the face of the tube it crosses, though its middle lies inside that
/// tube; `None` when nothing of it is left (it lies wholly inside the tubes it is fitted to).
pub fn fit_piece_end(member: &Traced, end_point: Vec3, toward: Vec3, middle: Vec3, tools: &[&Traced], id: &str) -> Result<Option<Traced>> {
    fitted(member, (end_point, toward, middle), tools, id, true)
}

fn gone(id: &str) -> DocError {
    DocError::Invalid(format!("the member on line `{id}` was cut away completely"))
}

/// [`fit_end`], keeping the farthest piece (`far`, [`fit_piece_end`]) or the one holding `middle`, else the farthest;
/// `None` when nothing is kept.
fn fitted(member: &Traced, (end_point, toward, middle): (Vec3, Vec3, Vec3), tools: &[&Traced], id: &str, far: bool) -> Result<Option<Traced>> {
    let toward = toward.normalized().ok_or_else(|| DocError::Invalid(format!("the member on line `{id}` has no direction")))?;
    let w = diagonal(&member.body, end_point, toward, middle);
    let half = end_point.dist(middle);
    // The tools that reach the end zone, each with how deep its crossing may run: the section plus the tool's
    // own size.
    let mut reach = Vec::new();
    for &tool in tools {
        let b = tool.body.exact_bounds()?;
        // A cheap first look at the tool's box; fails closed on a non-finite distance.
        if !(end_point.dist(end_point.max(b.min).min(b.max)) <= w) {
            continue;
        }
        let size = b.max - b.min;
        let depth = w + size.x.min(size.y).min(size.z);
        let gate = zone(end_point, toward, w, depth.min(half / 2.0))?;
        if kernel::boolean(&tool.body, &gate, BoolOp::Intersect)?.is_some() {
            reach.push((tool, depth));
        }
    }
    if reach.is_empty() {
        return Ok(Some(member.clone()));
    }
    // A tool that reaches the end is cut over its whole crossing, an oblique brace's included, and nowhere else.
    // Its faces are named for the zone, apart from the member's own `face:<k>`, where it leaves one on the member.
    let clip = zone(end_point, toward, w, 2.0 * reach.iter().map(|r| r.1).fold(0.0, f64::max))?;
    let roles = (0..clip.face_count()).map(|k| vec![format!("fit_zone:{k}")]).collect();
    let clip = Traced::made(clip, Some(roles));
    let mut fitted: Option<Traced> = None;
    for &(tool, _) in &reach {
        // ponytail: every tool is clipped, even one wholly inside the zone; skip those if the boolean shows.
        let Some(part) = kernel::boolean(&tool.body, &clip.body, BoolOp::Intersect)? else { continue };
        let part = Traced::step(part, &[tool, &clip]);
        let from = fitted.as_ref().unwrap_or(member);
        let Some(cut) = kernel::boolean(&from.body, &part.body, BoolOp::Cut)? else { return Ok(None) };
        fitted = Some(Traced::step(cut, &[from, &part]));
    }
    let Some(fitted) = fitted else { return Ok(Some(member.clone())) };
    let lumps = fitted.body.lumps()?;
    let lineages = Lineage::lumps(&fitted.body, fitted.lineage.as_ref(), &lumps);
    let mut pieces: Vec<Traced> = lumps.into_iter().zip(lineages).map(|(body, lineage)| Traced { body, lineage, recipe: None }).collect();
    if !far {
        let mut holding = None;
        for (k, piece) in pieces.iter().enumerate() {
            if piece.body.classify_point(middle)? != PointClass::Outside {
                holding = Some(k);
                break;
            }
        }
        if let Some(k) = holding {
            return Ok(pieces.into_iter().nth(k));
        }
    }
    let far_piece = if pieces.len() == 1 {
        pieces.pop()
    } else {
        let mut best: Option<(f64, Traced)> = None;
        for piece in pieces {
            let along = (end_point - kernel::measure_mass(&piece.body)?.centroid).dot(toward);
            if best.as_ref().is_none_or(|(most, _)| along > *most) {
                best = Some((along, piece));
            }
        }
        best.map(|b| b.1)
    };
    if far {
        return Ok(far_piece);
    }
    let Some(piece) = far_piece else { return Ok(None) };
    // Fitting took the middle away, which lies inside the tools: the piece reaching farthest from the end is kept
    // when it comes out of them, and dropped when it lies wholly inside them (ruling C4).
    // ponytail: a coplanar cut's sliver counts as sticking out and keeps the piece; measure what is left if one shows.
    let mut rest = Some(piece.body.clone());
    for &(tool, _) in &reach {
        let Some(b) = rest else { break };
        rest = kernel::boolean(&b, &tool.body, BoolOp::Cut)?;
    }
    Ok(rest.map(|_| piece))
}

/// The end zone: a box `w` out from the member's line each way, from `w` past `end` back `depth` into the
/// member, `toward` pointing out of it.
fn zone(end: Vec3, toward: Vec3, w: f64, depth: f64) -> Result<kernel::Body> {
    let u = toward.any_perp();
    // x × y = u × (-toward × u) = -toward: the box runs back into the member.
    let plane = Plane::new(end + toward * w, u, (-toward).cross(u)).ok_or_else(|| DocError::Invalid("a tube end has no zone".into()))?;
    let square = [Vec2::new(-w, -w), Vec2::new(w, -w), Vec2::new(w, w), Vec2::new(-w, w)];
    let mut bodies = kernel::extrude(&plane, &[Region2 { outer: Loop2::polygon(&square), holes: Vec::new() }], 0.0, depth + w)?;
    match (bodies.pop(), bodies.is_empty()) {
        (Some(body), true) => Ok(body),
        _ => Err(DocError::Invalid("a tube end's zone is not one solid".into())),
    }
}

/// The member's section diagonal at its end: twice its farthest vertex from the line through `end` along unit
/// `toward`, of the vertices nearer the end than `middle` is (all of them when none is).
fn diagonal(body: &kernel::Body, end: Vec3, toward: Vec3, middle: Vec3) -> f64 {
    let half = end.dist(middle);
    let points: Vec<Vec3> = body.record().vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect();
    let off = |p: &Vec3| {
        let d = *p - end;
        (d - toward * d.dot(toward)).len()
    };
    let near = points.iter().filter(|p| p.dist(end) < half).map(off).fold(f64::NEG_INFINITY, f64::max);
    2.0 * if near.is_finite() { near } else { points.iter().map(off).fold(0.0, f64::max) }
}

#[cfg(test)]
#[path = "frame_multi_trim_tests.rs"]
mod tests;
