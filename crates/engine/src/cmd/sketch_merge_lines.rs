//! Merge Lines (#20317): sketch lines joined end to end on one straight line become one line.
//!
//! A frame run drawn as several pieces makes one tube per piece; merged, it is one line and one
//! tube at its full length. The first line given keeps its id and runs from one outer end of the
//! run to the other; the others go. What held for every piece's direction (horizontal, parallel,
//! an angle, collinear with a line outside the run) moves onto the merged line; what measured one
//! piece (its length, an equal length) goes. A point between two pieces that something else
//! still uses (another line ending there, a dimension) stays, held on the merged line.
//! Frame Members built on the sketch name the merged line in place of the ones that went.

use std::f64::consts::{PI, TAU};

use serde_json::Value;
use solidcraft_doc::{FeatureKind, SketchId};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use super::sketch::{add_c, coincident_points, edit};
use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, string_list};
use crate::schema::sketch_merge_lines::{MergeLinesParams, MergeLinesResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CONTRACT: TypedContract<MergeLinesParams> = TypedContract::new();

/// Most lines one merge takes.
const MAX_LINES: usize = 1_000;
/// How far a line's end may sit off the run's line and still be on it, in mm.
const ON_LINE: f64 = 1e-6;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("sketch.merge_lines", "Merge Lines", crate::checked!("sketch.merge_lines", merge_lines))
    .in_sketch_part()
    .at("SKETCH", "MODIFY")
    .icon("merge_lines")
    .contract(&CONTRACT)
    .output(output_schema_for::<MergeLinesResult>)
    .enabled(in_sketch)
    .params(concat!(
        "curves: [line ids] (at least two, joined end to end on one straight line). The first keeps its id and runs from one outer end of the run to the other; the others go. ",
        "Direction relations of the pieces move onto it, their lengths go, and a point between pieces that another curve or a constraint uses stays on it. ",
        "Frame Members built on the sketch name it in place of the lines that went, and Rotate steps on those lines turn it. Returns line: the merged line's id, removed: [ids], frames: [Frame Members and Rotate steps changed]",
        created_doc!()
    ))];

fn merge_lines(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.merge_lines";
    let ids = string_list(cmd, p, "curves")?;
    if ids.len() > MAX_LINES {
        return Err(bad(cmd, format!("at most {MAX_LINES} lines at once")));
    }
    let frame_sketch = if s.sheet_sketching() { None } else { Some(sketch_store::target(s, p, cmd)?) };
    let ((line, removed, frames), info) = edit(s, p, cmd, true, |sk, doc| {
        let (line, removed) = merge(sk, &ids, cmd)?;
        let frames = frame_sketch.map(|id| follow_frames(doc, id, &line, &removed)).unwrap_or_default();
        Ok((line, removed, frames))
    })?;
    let reply = MergeLinesResult { line, removed, frames, sketch: info, created: None, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// One piece of the run: where it starts and ends along the run's direction, and the points there.
struct Piece {
    lo: f64,
    hi: f64,
    p_lo: usize,
    p_hi: usize,
}

/// Merge the lines `ids` into the first: its id and the ids of the lines that went.
fn merge(sk: &mut Sketch, ids: &[String], cmd: &str) -> Result<(String, Vec<String>)> {
    let mut run: Vec<usize> = Vec::new();
    for id in ids {
        let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
        let curve = sk.curves.get(c).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
        if !matches!(curve.kind, CurveKind::Line { .. }) {
            return Err(bad(cmd, format!("`{id}` is not a line: only straight lines merge")));
        }
        if curve.infinite {
            return Err(bad(cmd, format!("`{id}` is an infinite line: it has no ends to merge")));
        }
        if curve.link.is_some() {
            return Err(bad(cmd, format!("`{id}` is projected from other geometry, which sets its ends: break its link first")));
        }
        if sk.offset_distance(id).is_some() {
            return Err(bad(cmd, format!("`{id}` is an offset, and its ends follow its source curves: break the offset association first")));
        }
        if !run.contains(&c) {
            run.push(c);
        }
    }
    let [kept, ..] = run[..] else { return Err(bad(cmd, "`curves` must name at least two lines to merge")) };
    if run.len() < 2 {
        return Err(bad(cmd, "`curves` must name at least two lines to merge"));
    }
    let construction = |c: usize| sk.curves.get(c).is_some_and(|c| c.construction);
    if run.iter().any(|&c| construction(c) != construction(kept)) {
        return Err(bad(cmd, "some of these lines are construction lines and some are not: make them one kind first"));
    }
    let line_ends = |c: usize| match sk.curves.get(c).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    let pos = |p: usize| sk.point(p).ok_or_else(|| bad(cmd, "point"));
    let (ka, kb) = line_ends(kept).ok_or_else(|| bad(cmd, "curve"))?;
    let origin = pos(ka)?;
    let along = pos(kb)? - origin;
    let length = along.len();
    if length <= ON_LINE {
        return Err(bad(cmd, format!("`{}` has no length", ids.first().map_or("", String::as_str))));
    }
    let dir = along * (1.0 / length);
    let mut pieces = Vec::new();
    // The pieces drawn against the kept line's direction.
    let mut reversed = Vec::new();
    for &c in &run {
        let (a, b) = line_ends(c).ok_or_else(|| bad(cmd, "curve"))?;
        let (qa, qb) = (pos(a)? - origin, pos(b)? - origin);
        if [qa, qb].iter().any(|q| dir.cross(*q).abs() > ON_LINE) {
            return Err(bad(cmd, "these lines don't lie on one straight line"));
        }
        let (ta, tb) = (qa.dot(dir), qb.dot(dir));
        if ta > tb {
            reversed.push(c);
        }
        pieces.push(if ta <= tb { Piece { lo: ta, hi: tb, p_lo: a, p_hi: b } } else { Piece { lo: tb, hi: ta, p_lo: b, p_hi: a } });
    }
    pieces.sort_by(|x, y| x.lo.total_cmp(&y.lo));
    // Each piece must start where the one before it ends: at one point, or points held together.
    let groups = coincident_points(sk);
    let root = |p: usize| groups.get(p).copied().unwrap_or(p);
    let mut joints: Vec<(usize, usize)> = Vec::new();
    for w in pieces.windows(2) {
        let [prev, next] = w else { continue };
        if root(prev.p_hi) != root(next.p_lo) || next.hi <= prev.hi || (next.lo - prev.hi).abs() > ON_LINE {
            return Err(bad(cmd, "these lines don't join end to end: each must start where another ends, with nothing overlapping"));
        }
        joints.push((prev.p_hi, next.p_lo));
    }
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else { return Err(bad(cmd, "curve")) };
    let (lo, hi) = (first.p_lo, last.p_hi);
    let kept_id = sk.curves.get(kept).map(|c| c.id.clone()).unwrap_or_default();
    let gone: Vec<usize> = run.iter().copied().filter(|&c| c != kept).collect();
    let gone_ids: Vec<String> = gone.iter().filter_map(|&c| sk.curves.get(c).map(|c| c.id.clone())).collect();
    // Every point where two pieces meet, by id: indices move when points go.
    let joint_ids: Vec<String> = joints.iter().flat_map(|(p, q)| [*p, *q]).filter_map(|p| sk.points.get(p).map(|p| p.id.clone())).collect();

    // The pieces' constraints, onto the kept line. Its index stays put until the others go.
    let in_run = |c: usize| run.contains(&c);
    let pmap = |p: usize| Some(joints.iter().find(|(_, q)| *q == p).map_or(p, |(rep, _)| *rep));
    let cmap = |c: usize| Some(if in_run(c) { kept } else { c });
    let distinct = |v: &[usize]| v.iter().enumerate().all(|(i, x)| !v[..i].contains(x));
    sk.constraints.retain_mut(|k| {
        match k.kind {
            ConstraintKind::Midpoint { p, l: c } | ConstraintKind::PointAlong { p, c, .. } if in_run(c) => {
                k.kind = ConstraintKind::PointOnCurve { p, c: kept };
            }
            ConstraintKind::Length { l, .. } if in_run(l) => return false,
            // An angle measured along a piece drawn the other way is half a turn off on the kept line.
            ConstraintKind::Angle { a, b, .. } if reversed.contains(&a) != reversed.contains(&b) => {
                if let ConstraintKind::Angle { flip, .. } = &mut k.kind {
                    *flip = !*flip;
                }
            }
            ConstraintKind::LineAngle { l, .. } if reversed.contains(&l) && k.param.is_some() => return false,
            ConstraintKind::LineAngle { l, value } if reversed.contains(&l) => {
                k.kind = ConstraintKind::LineAngle { l, value: (value + 2.0 * PI).rem_euclid(TAU) - PI };
            }
            ConstraintKind::Equal { a, b } | ConstraintKind::SlotOverallLength { a, b, .. } if in_run(a) || in_run(b) => return false,
            _ => {}
        }
        if !k.kind.remap(&pmap, &cmap) {
            return false;
        }
        if let ConstraintKind::PointOnCurve { p, c } = k.kind
            && c == kept
            && (p == lo || p == hi)
        {
            return false;
        }
        let (ps, cs) = k.kind.indices();
        distinct(&ps) && distinct(&cs)
    });
    // What every piece had (each one horizontal) is now said of the kept line more than once.
    let mut seen: Vec<ConstraintKind> = Vec::new();
    sk.constraints.retain(|k| {
        if !k.kind.indices().1.contains(&kept) {
            return true;
        }
        if seen.contains(&k.kind) {
            return false;
        }
        seen.push(k.kind.clone());
        true
    });
    if let Some(c) = sk.curves.get_mut(kept) {
        c.kind = CurveKind::Line { a: lo, b: hi };
    }
    for &(rep, other) in &joints {
        if rep != other {
            for (i, c) in sk.curves.iter_mut().enumerate() {
                if !in_run(i) {
                    c.kind.map_points(&|p| if p == other { rep } else { p });
                }
            }
        }
    }
    // A point between pieces stays when another curve or a constraint still uses it, held on the
    // merged line. The pieces about to go let go of it first, or it would go with them.
    let used = |sk: &Sketch, p: usize| {
        sk.curves.iter().enumerate().any(|(i, c)| !in_run(i) && c.kind.point_ids().contains(&p))
            || sk.constraints.iter().any(|k| k.kind.refers_to_point(p))
    };
    let stay: Vec<usize> = joints.iter().map(|(rep, _)| *rep).filter(|&p| used(sk, p)).collect();
    for &p in &stay {
        if !sk.constraints.iter().any(|k| k.kind == ConstraintKind::PointOnCurve { p, c: kept }) {
            add_c(sk, ConstraintKind::PointOnCurve { p, c: kept })?;
        }
        for &c in &gone {
            if let Some(curve) = sk.curves.get_mut(c) {
                curve.kind.map_points(&|q| if q == p { lo } else { q });
            }
        }
    }
    sk.remove_curves(&gone);
    // The kept line's own old ends between pieces, when nothing uses them now.
    let loose: Vec<usize> = joint_ids
        .iter()
        .filter_map(|id| sk.point_index(id))
        .filter(|&p| !sk.curves.iter().any(|c| c.kind.point_ids().contains(&p)) && !sk.constraints.iter().any(|k| k.kind.refers_to_point(p)))
        .collect();
    sk.remove_points(&loose);
    Ok((kept_id, gone_ids))
}

/// Frame Members built on sketch `sketch` that listed, named or finished a corner of a line that
/// went: the line list names the merged line `kept` in its place, and its names and corner
/// overrides go. A Rotate step on a line that went turns the merged line. The names of the
/// features changed.
fn follow_frames(doc: &mut solidcraft_doc::Document, sketch: u64, kept: &str, removed: &[String]) -> Vec<String> {
    let gone = |l: &str| removed.iter().any(|r| r == l);
    let mut changed = Vec::new();
    let mut frames = Vec::new();
    for f in &mut doc.features {
        let FeatureKind::FrameMember { path_sketch: Some(SketchId(id)), lines, names, corners, .. } = &mut f.kind else { continue };
        if *id != sketch {
            continue;
        }
        frames.push(f.id);
        let touched = lines.iter().any(|l| gone(l)) || names.keys().any(|n| gone(n)) || corners.iter().any(|c| c.lines().any(gone));
        if !touched {
            continue;
        }
        let mut follow = Vec::new();
        for l in lines.drain(..) {
            let l = if gone(&l) { kept.to_string() } else { l };
            if !follow.contains(&l) {
                follow.push(l);
            }
        }
        *lines = follow;
        names.retain(|n, _| !gone(n));
        corners.retain(|c| !c.lines().any(gone));
        changed.push(f.name.clone());
    }
    for f in &mut doc.features {
        if let FeatureKind::FrameMemberTurn { member, .. } = &mut f.kind
            && frames.contains(&member.feature.0)
            && gone(&member.line)
        {
            member.line = kept.to_string();
            changed.push(f.name.clone());
        }
    }
    changed
}

#[cfg(test)]
#[path = "sketch_merge_lines_tests.rs"]
mod tests;
