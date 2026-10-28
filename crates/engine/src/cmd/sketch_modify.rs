//! Sketch modify tools: trim, extend, break, fillet, chamfer, offset, mirror, circular and
//! rectangular patterns, move/copy and scale.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde_json::{Value, json};
use solidcraft_doc::ProfileSel;
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, Curve, CurveKind, Shape, Sketch, intersections, intersections_within};

use super::sketch::{add_c, edit, ids_of, solve_current};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, expr, num, req_expr, req_vec2, str_, string_list, vec2, vec2_list};
use crate::{Result, Session};

#[path = "sketch_pattern.rs"]
mod pattern;
#[path = "sketch_support.rs"]
mod support;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.fillet", "Fillet", fillet)
        .at("SKETCH", "MODIFY")
        .icon("sketch_fillet")
        .enabled(in_sketch)
        .params(concat!("point: corner shared by two curves (\"l1.end\") | points: [corners] (all rounded alike: one radius, the rest equal; a corner is a point reference or {a, b}, the two curves meeting there) | a, b: two lines or arcs meeting at a corner; radius: expr. A corner that dimensions or constraints measure to stays as a virtual sharp corner on both curves' extensions (`virtual_corners`; the moved ones in `moved_to_virtual_corner`)", created_doc!())),
    CommandSpec::new("sketch.chamfer.equal_distance", "Equal Distance Chamfer", chamfer_equal)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer")
        .enabled(in_sketch)
        .params(concat!("point | a, b: a corner of two lines; distance", created_doc!())),
    CommandSpec::new("sketch.chamfer.distance_angle", "Distance and Angle Chamfer", chamfer_dist_angle)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer_angle")
        .enabled(in_sketch)
        .params(concat!("point | a, b: a corner of two lines; distance: along a, angle: deg from a", created_doc!())),
    CommandSpec::new("sketch.chamfer.two_distance", "Two Distance Chamfer", chamfer_two)
        .at("SKETCH", "MODIFY")
        .icon("sketch_chamfer_two_distances")
        .enabled(in_sketch)
        .params(concat!("point | a, b: a corner of two lines; distance: along a, distance2: along b", created_doc!())),
    CommandSpec::new("sketch.offset", "Offset", offset)
        .at("SKETCH", "MODIFY")
        .icon("offset")
        .key("O")
        .enabled(in_sketch)
        .params(concat!(
            "curves: [ids], distance: number in mm or length expression (creates an editable dimension; positive = left of the chain, or toward `side`), side?: [x,y], chain?: bool (default true: the connected chain); ",
            "direction?: one_side|symmetric (default one_side) or symmetric?: bool (the same distance each way; no `side`, whose job it takes over), ",
            "distance2?: positive length expression (Two Sides: an independent distance the other way, as for `solid.extrude`). Each side is its own offset association, listed in `sides`; flip?: bool reverses the primary side; `param`/`param2` name the driving dimensions",
            created_doc!()
        )),
    CommandSpec::new("sketch.offset.break", "Break Offset Association", break_offset).icon("sketch_break")
        .enabled(in_sketch)
        .params("curve: derived offset curve id (retains geometry and makes its points editable)"),
    CommandSpec::new("sketch.trim", "Trim", trim)
        .at("SKETCH", "MODIFY")
        .icon("trim")
        .key("T")
        .enabled(in_sketch)
        .params(concat!("curve: id, at: [x,y] (the piece to remove: between the nearest intersections around it)", created_doc!())),
    CommandSpec::new("sketch.trim.path", "Trim Along Path", trim_path)
        .icon("trim")
        .enabled(in_sketch)
        .params(concat!(
            "points: [[x,y]…] (2…2000 positions: a path drawn across the sketch). Every piece the path crosses is removed as `sketch.trim` removes the piece at that crossing, in the order the path crosses them; a piece crossed again after it is gone is not trimmed twice; projected geometry, infinite lines, offset outputs and pieces trim refuses are passed over (why in `skipped`). dry_run?: bool (report without changing the design). One undo step → trimmed: [each trim's result], removed: [[[x,y]…]] (each removed piece as a polyline)",
            created_doc!()
        )),
    CommandSpec::new("sketch.extend", "Extend", extend)
        .at("SKETCH", "MODIFY")
        .icon("extend")
        .enabled(in_sketch)
        .params("curve: id, at: [x,y] near the end to extend (a line runs on to the nearest curve, or the previewed reach when nothing is in the way; an arc grows along its circle and closes into a whole circle when nothing stops it)"),
    CommandSpec::new("sketch.break", "Break", break_cmd)
        .at("SKETCH", "MODIFY")
        .icon("sketch_break")
        .enabled(in_sketch)
        .params(concat!("curve: id, at: [x,y] (split at intersections) | points: [[x,y]…] (1…1000 positions projected onto the curve; duplicates are merged)", created_doc!())),
    CommandSpec::new("sketch.scale", "Sketch Scale", scale)
        .at("SKETCH", "MODIFY")
        .icon("sketch_scale")
        .enabled(in_sketch)
        .params("entities: [curve/point ids], base: [x,y] or point ref, factor: number"),
    CommandSpec::new("sketch.transform", "Transform", transform)
        .at("SKETCH", "MODIFY")
        .icon("sketch_move")
        .key("G")
        .enabled(in_sketch)
        .params("entities: [curve/point ids], factor?: number (scale), angle?: degrees (turn), center?: [x,y] or point ref (what scales and turns stay put; default [0,0]), translate?: [dx,dy] (applied last); scales, turns and moves in one step"),
    CommandSpec::new("sketch.move", "Move/Copy Sketch Entities", move_copy)
        .at("SKETCH", "MODIFY")
        .icon("sketch_move")
        .enabled(in_sketch)
        .params(concat!("entities: [ids], translate?: [dx,dy], angle?: deg, center?: [x,y], copy?: bool, rotate_constraints?: bool (default true; false keeps constraints unchanged); a rotation turns the constraints tied to the sketch axes when all their geometry moves (horizontal and vertical relations and dimensions swap at odd quarter turns and stay at half turns, line angles turn with the line) and removes the horizontal and vertical ones that cannot hold at other angles; picking a text outline or linked point transforms its whole text source once, and copies remain editable text", created_doc!())),
    CommandSpec::new("sketch.mirror", "Mirror", mirror)
        .at("SKETCH", "PATTERN")
        .icon("sketch_mirror")
        .enabled(in_sketch)
        .params(concat!("entities: [curve/point ids], line: mirror line id (symmetry constraints are added)", created_doc!())),
    CommandSpec::new("sketch.pattern.circular", "Circular Pattern", pattern_circular)
        .at("SKETCH", "PATTERN")
        .icon("sketch_circ_pattern")
        .enabled(in_sketch)
        .params(concat!("entities: [ids], center: [x,y] or point ref, count: 2…500 (number or unitless expression, retained through edits), angle?: total deg, number or expression (default 360); alignment?: left|center|right (default right); reversed?: bool (default false). Quantity includes the original. Center balances copies around it; reversed flips the direction and extra side for even totals. Partial arcs span angle with angle/(count-1) spacing; full turns use angle/count. Adds associative pattern constraints; a coordinate center creates a constrainable point. Returns pattern and center ids. pattern?: existing pattern id to edit (omitted values retained; surviving instance ids retained)", created_doc!())),
    CommandSpec::new("sketch.pattern.rectangular", "Rectangular Pattern", pattern_rect)
        .at("SKETCH", "PATTERN")
        .icon("sketch_rect_pattern")
        .enabled(in_sketch)
        .params(concat!("entities: [ids], count, spacing; count2?, spacing2?; counts: positive integer numbers or expressions, at most 500 instances total; spacings: length numbers or expressions; direction?, direction2?: sketch line ids (live references); dir?, dir2?: [x,y] vectors (defaults: sketch x, perpendicular to direction 1); alignment?, alignment2?: left|center|right (default right). Signed spacings reverse each direction. reversed?, reversed2?: bool (default false); reverses placement and the extra side of even centered totals. Quantities include the original. Adds associative pattern constraints. Returns pattern id. pattern?: existing pattern id to edit (omitted values retained; surviving instance ids retained)", created_doc!())),
];

const TOL: f64 = 1e-7;
const MAX_COPIES: usize = 500;

fn curve_arg(sk: &Sketch, p: &Value, cmd: &str) -> Result<usize> {
    let id = str_(p, "curve").ok_or_else(|| bad(cmd, "`curve` must be a curve id"))?;
    let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
    if sk.is_linked_curve(c) {
        return Err(bad(cmd, format!("`{id}` is projected geometry: break its link first")));
    }
    if sk.freeform_offsets.iter().any(|offset| offset.output == id)
        || sk.offsets.iter().filter_map(|offset| offset.topology.as_ref()).any(|topology| topology.roles.iter().any(|role| role.output == id))
    {
        return Err(bad(cmd, "break the offset association before editing its output"));
    }
    Ok(c)
}

/// Parameter of `p` on the bounded shape, ends included.
fn on_bounded(sh: &Shape, p: Vec2) -> Option<f64> {
    if sh.dist(p) > TOL * 10.0 {
        return None;
    }
    let t = sh.param(p);
    let (t0, t1) = sh.range();
    let slack = match *sh {
        Shape::Line { a, b } => TOL * 10.0 / a.dist(b).max(1e-12),
        Shape::Round { r, .. } => TOL * 10.0 / r.max(1e-12),
    };
    if sh.is_full() || (t >= t0 - slack && t <= t1 + slack) { Some(t) } else { None }
}

/// Points where other curves meet curve `ci` (crossings and end points touching it), with the
/// curve that makes each one and the parameter on `ci`.
fn cut_points(sk: &Sketch, ci: usize) -> Vec<(f64, Vec2, usize)> {
    let Some(sh) = sk.shape(ci) else { return Vec::new() };
    let mut out: Vec<(f64, Vec2, usize)> = Vec::new();
    for (oi, _) in sk.curves.iter().enumerate() {
        if oi == ci {
            continue;
        }
        let Some(other) = sk.shape(oi) else { continue };
        // A line or circle the solver made tangent to a circle touches it within rounding noise,
        // and the exact roots miss it whenever the noise falls outside: the touch counts as a
        // cut, as profile detection counts it (#588), or trim deletes the whole curve (#5074).
        let mut pts = intersections_within(&sh, &other, TOL * 10.0);
        // Overlapping or touching ends that the analytic intersection misses (parallel lines).
        if let Shape::Line { a, b } = other {
            pts.extend([a, b]);
        }
        if let Shape::Round { sweep, .. } = other
            && sweep < TAU - 1e-12
        {
            pts.extend([other.point(0.0), other.point(sweep)]);
        }
        // An infinite line (#1359) cuts wherever its line crosses, beyond its two points too.
        let other_infinite = sk.curves.get(oi).is_some_and(|c| c.infinite);
        for q in pts {
            if (other_infinite || on_bounded(&other, q).is_some())
                && let Some(t) = on_bounded(&sh, q)
                && !out.iter().any(|(_, x, _)| x.dist(q) < TOL * 10.0)
            {
                out.push((t, q, oi));
            }
        }
    }
    for (q, oi) in super::sketch_freeform::analytic_cut_points(sk, ci) {
        if let Some(t) = on_bounded(&sh, q)
            && !out.iter().any(|(_, x, _)| x.dist(q) < TOL * 10.0)
        {
            out.push((t, q, oi));
        }
    }
    out
}

/// Constrain a new end point onto the curve that cut there.
fn pin(sk: &mut Sketch, p: usize, cutter: usize) -> Result<()> {
    let at = sk.point(p).unwrap_or_default();
    // An end point of the cutter there: coincident with it; otherwise on the curve.
    let ends: Vec<usize> = sk.curves.get(cutter).and_then(|c| c.kind.ends()).map(|(a, b)| vec![a, b]).unwrap_or_default();
    if let Some(e) = ends.into_iter().find(|e| sk.point(*e).is_some_and(|q| q.dist(at) < TOL * 10.0)) {
        if e != p {
            add_c(sk, ConstraintKind::Coincident { p, q: e })?;
        }
        return Ok(());
    }
    add_c(sk, ConstraintKind::PointOnCurve { p, c: cutter })?;
    Ok(())
}

/// Split curve `ci` at interior parameters `ts` (sorted). Returns the pieces in parameter
/// order (the first keeps the original index) and the new points.
fn split_at(sk: &mut Sketch, ci: usize, ts: &[f64]) -> Result<(Vec<usize>, Vec<usize>)> {
    let sh = sk.shape(ci).ok_or_else(|| bad("split", "curve"))?;
    let kind = sk.curves.get(ci).map(|c| c.kind.clone()).ok_or_else(|| bad("split", "curve"))?;
    let construction = sk.curves.get(ci).is_some_and(|c| c.construction);
    let mut pts = Vec::new();
    for t in ts {
        pts.push(sk.add_point(sh.point(*t), None)?);
    }
    let mut pieces = vec![ci];
    let mut push = |sk: &mut Sketch, kind: CurveKind, prefix: &str| -> Result<()> {
        let id = sk.fresh(prefix)?;
        sk.curves.push(Curve { id, kind, construction, reversed: false, link: None, centerline: false, fixed: false, infinite: false });
        pieces.push(sk.curves.len() - 1);
        Ok(())
    };
    match kind {
        CurveKind::Line { a, b } => {
            let mut chain = vec![a];
            chain.extend(&pts);
            chain.push(b);
            if let Some(c) = sk.curves.get_mut(ci) {
                c.kind = CurveKind::Line { a, b: chain.get(1).copied().unwrap_or(b) };
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Line { a: w[0], b: w[1] }, "l")?;
            }
        }
        CurveKind::Arc { c, a, b } => {
            let mut chain = vec![a];
            chain.extend(&pts);
            chain.push(b);
            if let Some(cu) = sk.curves.get_mut(ci) {
                cu.kind = CurveKind::Arc { c, a, b: chain.get(1).copied().unwrap_or(b) };
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Arc { c, a: w[0], b: w[1] }, "a")?;
            }
        }
        CurveKind::Circle { c, .. } => {
            if pts.len() < 2 {
                return Err(bad("split", "a circle needs two points to split at"));
            }
            let mut chain = pts.clone();
            if let Some(f) = pts.first() {
                chain.push(*f);
            }
            if let Some(cu) = sk.curves.get_mut(ci) {
                cu.kind = CurveKind::Arc { c, a: chain[0], b: chain[1] };
                cu.reversed = false;
            }
            for w in chain.windows(2).skip(1) {
                push(sk, CurveKind::Arc { c, a: w[0], b: w[1] }, "a")?;
            }
        }
        _ => return Err(bad("split", "not supported yet for ellipses, splines and conics")),
    }
    Ok((pieces, pts))
}

/// Keep the pieces of a split tied together: lines collinear, arcs on one circle.
fn tie(sk: &mut Sketch, pieces: &[usize]) -> Result<()> {
    for w in pieces.windows(2) {
        let (a, b) = (w[0], w[1]);
        let line = matches!(sk.curves.get(a).map(|c| &c.kind), Some(CurveKind::Line { .. }));
        add_c(sk, if line { ConstraintKind::Collinear { a, b } } else { ConstraintKind::Equal { a, b } })?;
    }
    Ok(())
}

/// Dimensions and relations that measure the whole of curve `ci` (a line's length, an equal
/// length, its midpoint, an arc's length or sweep) would stay on the piece that keeps its id
/// after a trim and stretch that piece back to the old size, moving the rest of the sketch. A
/// length becomes the distance between the curve's old ends, as a fillet keeps it to its virtual
/// corner (it goes with an end the trim removes); the others go.
fn release_extent(sk: &mut Sketch, ci: usize) {
    let ends = match sk.curves.get(ci).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    sk.constraints.retain_mut(|k| match k.kind {
        ConstraintKind::Length { l, value } if l == ci => match ends {
            Some((p, q)) => {
                k.kind = ConstraintKind::Distance { p, q, value };
                true
            }
            None => false,
        },
        ConstraintKind::Equal { a, b } if ends.is_some() && (a == ci || b == ci) => false,
        ConstraintKind::Midpoint { l, .. } if l == ci => false,
        ConstraintKind::ArcLength { c, .. } | ConstraintKind::ArcSweep { c, .. } if c == ci => false,
        _ => true,
    });
}

/// The cut parameters bracketing `tq` on curve `ci`: (before, after) with their cutter curves.
fn bracket(sk: &Sketch, ci: usize, tq: f64) -> (Vec<(f64, Vec2, usize)>, bool) {
    let Some(sh) = sk.shape(ci) else { return (Vec::new(), false) };
    let (t0, t1) = sh.range();
    let full = sh.is_full();
    let span = t1 - t0;
    let mut cuts: Vec<(f64, Vec2, usize)> =
        cut_points(sk, ci).into_iter().filter(|(t, _, _)| full || (*t > t0 + 1e-9 * span && *t < t1 - 1e-9 * span)).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    if full {
        if cuts.len() < 2 {
            return (Vec::new(), true);
        }
        // Cuts around tq on the circle (wrapping).
        let after = cuts.iter().position(|c| c.0 > tq).unwrap_or(0);
        let before = (after + cuts.len() - 1) % cuts.len();
        let mut two = vec![cuts[before], cuts[after]];
        two.sort_by(|a, b| a.0.total_cmp(&b.0));
        return (two, true);
    }
    let before = cuts.iter().rev().find(|c| c.0 < tq).copied();
    let after = cuts.iter().find(|c| c.0 > tq).copied();
    (before.into_iter().chain(after).collect(), false)
}

/// The piece of `curve` a trim at `at` takes away, as points along it, for the Trim tool to show
/// under the cursor (#3052): the part between the crossings either side of `at`, as `sketch.trim`
/// cuts it, or the whole curve where nothing crosses it. `None` where trim works otherwise (a
/// free-form or infinite curve) or the curve isn't in the sketch: the tool shows the curve.
pub fn trim_piece(sk: &Sketch, curve: &str, at: Vec2) -> Option<Vec<Vec2>> {
    let ci = sk.curve_index(curve)?;
    let c = sk.curves.get(ci)?;
    if c.infinite || c.kind.is_freeform() {
        return None;
    }
    let sh = sk.shape(ci)?;
    let tq = sh.param(sh.project(at));
    let (t0, t1) = sh.range();
    let (cuts, full) = bracket(sk, ci, tq);
    let (a, b) = match (cuts.as_slice(), full) {
        ([], _) => (t0, t1),
        // A whole circle: the arc between the two crossings round `at`.
        ([x, y], true) => {
            if tq > x.0 && tq < y.0 {
                (x.0, y.0)
            } else {
                (y.0, x.0 + TAU)
            }
        }
        (_, true) => return None,
        (cuts, false) => (cuts.iter().rev().find(|c| c.0 < tq).map_or(t0, |c| c.0), cuts.iter().find(|c| c.0 > tq).map_or(t1, |c| c.0)),
    };
    let n = if matches!(sh, Shape::Line { .. }) { 1 } else { 64 };
    Some((0..=n).map(|i| sh.point(a + (b - a) * f64::from(i) / f64::from(n))).collect())
}

/// Trim and extend refuse an offset's output: its ends are recomputed from its source curves on
/// every solve, so a new end there would be pulled straight back (the trim reported success and
/// left the curve whole).
fn refuse_offset_ends(sk: &Sketch, ci: usize, what: &str, cmd: &str) -> Result<()> {
    match sk.curves.get(ci).filter(|c| sk.offset_distance(&c.id).is_some()) {
        Some(c) => Err(bad(
            cmd,
            format!(
                "`{}` is an offset, and its ends follow its source curves: {what} the source curves instead, or break the offset association first",
                c.id
            ),
        )),
        None => Ok(()),
    }
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.trim";
    let at = req_vec2(cmd, p, "at")?;
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let ci = curve_arg(sk, p, cmd)?;
        let (out, _) = remove_piece(sk, ci, at, cmd)?;
        doc.prune_model_params();
        Ok(out)
    })?;
    Ok(json!({"result": out, "sketch": info}))
}

/// Remove the piece of curve `ci` around `at`: between the nearest intersections on either
/// side, or the whole curve when nothing crosses it. Returns what `sketch.trim` reports and the
/// removed piece as a polyline.
fn remove_piece(sk: &mut Sketch, ci: usize, at: Vec2, cmd: &str) -> Result<(Value, Vec<Vec2>)> {
    let id = sk.curves.get(ci).map(|c| c.id.clone()).unwrap_or_default();
    if sk.curves.get(ci).is_some_and(|c| c.infinite) {
        return Err(bad(cmd, "an infinite line has no ends to trim back to: make it a normal line first"));
    }
    if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
        let whole = sk.polyline(ci);
        let r = super::sketch_freeform::trim_or_break(sk, ci, at, true, cmd)?;
        let removed = match r.get("removed").and_then(Value::as_array) {
            Some(pts) => pts.iter().filter_map(|q| Some(Vec2::new(q.get(0)?.as_f64()?, q.get(1)?.as_f64()?))).collect(),
            None => whole,
        };
        return Ok((json!({"trimmed": id, "pieces": r["curves"]}), removed));
    }
    let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
    let tq = sh.param(sh.project(at));
    let (cuts, full) = bracket(sk, ci, tq);
    if cuts.is_empty() {
        // Nothing crosses it: the whole curve goes.
        let removed = sk.polyline(ci);
        sk.remove_curves(&[ci]);
        return Ok((json!({"deleted": id}), removed));
    }
    refuse_offset_ends(sk, ci, "trim", cmd)?;
    release_extent(sk, ci);
    let ts: Vec<f64> = cuts.iter().map(|c| c.0).collect();
    let (pieces, pts) = split_at(sk, ci, &ts)?;
    // The piece containing tq.
    let k = if full {
        let (a, b) = (ts[0], ts[1]);
        if tq > a && tq < b { 0 } else { 1 }
    } else {
        ts.iter().filter(|t| **t < tq).count()
    };
    for (pi, c) in pts.iter().zip(&cuts) {
        pin(sk, *pi, c.2)?;
    }
    let gone = pieces.get(k).copied().ok_or_else(|| bad(cmd, "piece"))?;
    let keep: Vec<usize> = pieces.iter().copied().filter(|x| *x != gone).collect();
    // The original curve keeps its id (and constraints): swap geometry into it if needed.
    let gone = match keep.first() {
        Some(&other) if gone == ci => {
            let ka = sk.curves.get(ci).map(|c| c.kind.clone());
            let kb = sk.curves.get(other).map(|c| c.kind.clone());
            if let (Some(ka), Some(kb)) = (ka, kb) {
                if let Some(c) = sk.curves.get_mut(ci) {
                    c.kind = kb;
                }
                if let Some(c) = sk.curves.get_mut(other) {
                    c.kind = ka;
                }
            }
            other
        }
        _ => gone,
    };
    let keep: Vec<usize> = pieces.iter().copied().filter(|x| *x != gone).collect();
    tie(sk, &keep)?;
    let removed = sk.polyline(gone);
    sk.remove_curves(&[gone]);
    Ok((json!({"trimmed": id}), removed))
}

/// Most crossings one `sketch.trim.path` trims (each one splits a curve, so this also caps the
/// work a hostile path can ask for).
const MAX_PATH_TRIMS: usize = 500;
/// Most positions in a trim path.
const MAX_PATH_POINTS: usize = 2_000;

/// The first place along `path` after `after` (segment, parameter along it) where the path
/// crosses a curve that a trim may change: (segment, parameter, curve, the crossing point). Crossings are found on the
/// curves' display polylines; the trim projects them onto the exact curve.
fn next_crossing(sk: &Sketch, path: &[Vec2], after: (usize, f64)) -> Option<(usize, f64, usize, Vec2)> {
    let bounds = |pts: &[Vec2]| {
        pts.iter().fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y)))
        })
    };
    let trimmable: Vec<(usize, Vec<Vec2>, (Vec2, Vec2))> = (0..sk.curves.len())
        .filter(|ci| curve_ok(sk, *ci))
        .map(|ci| {
            let poly = sk.polyline(ci);
            let b = bounds(&poly);
            (ci, poly, b)
        })
        .collect();
    for (si, w) in path.windows(2).enumerate().skip(after.0) {
        let (a, b) = (w[0], w[1]);
        let from = if si == after.0 { after.1 } else { f64::NEG_INFINITY };
        let (lo, hi) = bounds(&[a, b]);
        let mut best: Option<(f64, usize, Vec2)> = None;
        for (ci, poly, (plo, phi)) in &trimmable {
            if plo.x > hi.x || plo.y > hi.y || phi.x < lo.x || phi.y < lo.y {
                continue;
            }
            for e in poly.windows(2) {
                let Some((t, q)) = seg_cross(a, b, e[0], e[1]) else { continue };
                if t > from + 1e-9 && best.is_none_or(|x| t < x.0) {
                    best = Some((t, *ci, q));
                }
            }
        }
        if let Some((t, ci, q)) = best {
            return Some((si, t, ci, q));
        }
    }
    None
}

/// Whether trim may change curve `ci` (what [`curve_arg`] accepts, and not an infinite line).
fn curve_ok(sk: &Sketch, ci: usize) -> bool {
    let Some(c) = sk.curves.get(ci) else { return false };
    !c.infinite
        && !sk.is_linked_curve(ci)
        && !sk.freeform_offsets.iter().any(|offset| offset.output == c.id)
        && !sk.offsets.iter().filter_map(|offset| offset.topology.as_ref()).any(|topology| topology.roles.iter().any(|role| role.output == c.id))
}

/// Where segment `a`–`b` crosses segment `c`–`d`: the parameter along `a`–`b` and the point.
fn seg_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f64, Vec2)> {
    let (r, s) = (b - a, d - c);
    let den = r.cross(s);
    if den.abs() <= 1e-15 * r.len2().max(s.len2()).max(1e-30) {
        return None;
    }
    let t = (c - a).cross(s) / den;
    let u = (c - a).cross(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| (t, a + r * t))
}

fn trim_path(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.trim.path";
    let path = vec2_list(cmd, p, "points", MAX_PATH_POINTS)?;
    if path.len() < 2 {
        return Err(bad(cmd, "`points` needs at least two positions"));
    }
    if bool_(p, "dry_run").unwrap_or(false) {
        let mut t = s.scratch();
        let mut q = p.clone();
        if let Some(o) = q.as_object_mut() {
            o.remove("dry_run");
        }
        let mut out = trim_path(&mut t, &q)?;
        out["dry_run"] = json!(true);
        return Ok(out);
    }
    let ((trimmed, removed, skipped), info) = edit(s, p, cmd, false, |sk, doc| {
        let mut trimmed: Vec<Value> = Vec::new();
        let mut removed: Vec<Value> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut after = (0, f64::NEG_INFINITY);
        for _ in 0..MAX_PATH_TRIMS {
            let Some((si, t, ci, q)) = next_crossing(sk, &path, after) else { break };
            after = (si, t);
            // A piece trim refuses (a closed spline crossed once) stays as it was.
            let before = sk.clone();
            match remove_piece(sk, ci, q, cmd) {
                Ok((out, gone)) => {
                    trimmed.push(out);
                    removed.push(json!(gone.iter().map(|v| [v.x, v.y]).collect::<Vec<_>>()));
                }
                Err(e) => {
                    *sk = before;
                    let why = e.to_string().replace(&format!("{cmd}: "), "");
                    if !skipped.contains(&why) {
                        skipped.push(why);
                    }
                }
            }
        }
        if trimmed.is_empty() {
            return Err(bad(cmd, skipped.first().cloned().unwrap_or_else(|| "the path crosses no curve that can be trimmed".into())));
        }
        doc.prune_model_params();
        Ok((trimmed, removed, skipped))
    })?;
    Ok(json!({"trimmed": trimmed, "removed": removed, "skipped": skipped, "sketch": info}))
}

/// Cuts closer than this to a curve's end or to each other are one place (mm).
const BREAK_GAP: f64 = 1e-6;

fn break_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.break";
    let points = if p.get("points").is_some() { Some(vec2_list(cmd, p, "points", 1000)?) } else { None };
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let ci = curve_arg(sk, p, cmd)?;
        if sk.curves.get(ci).is_some_and(|c| c.kind.is_freeform()) {
            let r = if let Some(points) = &points {
                super::sketch_freeform::break_at_points(sk, ci, points, cmd)?
            } else {
                let at = req_vec2(cmd, p, "at")?;
                super::sketch_freeform::trim_or_break(sk, ci, at, false, cmd)?
            };
            doc.prune_model_params();
            return Ok(r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default());
        }
        let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
        let (ts, cutters): (Vec<f64>, Vec<Option<usize>>) = if let Some(list) = &points {
            let mut v: Vec<f64> = list.iter().map(|q| sh.param(sh.project(*q))).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            v.dedup_by(|a, b| (*a - *b).abs() <= 1e-9);
            let n = v.len();
            (v, vec![None; n])
        } else {
            let at = req_vec2(cmd, p, "at")?;
            let (cuts, _) = bracket(sk, ci, sh.param(sh.project(at)));
            (cuts.iter().map(|c| c.0).collect(), cuts.iter().map(|c| Some(c.2)).collect())
        };
        let (t0, t1) = sh.range();
        if ts.is_empty() || (!sh.is_full() && ts.iter().any(|t| *t <= t0 || *t >= t1)) {
            return Err(bad(cmd, "nothing to break at there"));
        }
        // A cut on (or a hair from) an end or another cut would make a piece of no length, and
        // its ties: where a curve ends, or touches a tangent arc, it is already broken.
        let ends = if sh.is_full() { Vec::new() } else { vec![sh.point(t0), sh.point(t1)] };
        let mut kept: Vec<(f64, Option<usize>)> = Vec::new();
        for (t, c) in ts.iter().zip(&cutters) {
            let q = sh.point(*t);
            if !ends.iter().chain(kept.iter().map(|k| sh.point(k.0)).collect::<Vec<_>>().iter()).any(|e| e.dist(q) < BREAK_GAP) {
                kept.push((*t, *c));
            }
        }
        if kept.is_empty() {
            return Err(bad(cmd, "the curve is already broken there"));
        }
        let (ts, cutters): (Vec<f64>, Vec<Option<usize>>) = kept.into_iter().unzip();
        // Its length (and its midpoint, an equal length) measured the whole curve: left on the
        // first piece, it would stretch that piece back to the old size and skew the sketch.
        release_extent(sk, ci);
        let (pieces, pts) = split_at(sk, ci, &ts)?;
        for (pi, c) in pts.iter().zip(&cutters) {
            if let Some(c) = c {
                pin(sk, *pi, *c)?;
            }
        }
        tie(sk, &pieces)?;
        Ok(ids_of(sk, &pieces))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

/// How far a line with nothing in its way extends: the reach its preview draws, and the length
/// `sketch.extend` adds, so clicking a preview that overhangs its target adds that line instead
/// of nothing.
const EXTEND_REACH: f64 = 50.0;

/// One end of a line growing along its own support, and what stops it.
///
/// A line extends past either of its ends, running on along its own straight support until the
/// first geometry in the way. `sketch.extend` and its preview both take this aim, so the preview
/// draws the line the click adds, never one the command refuses.
#[derive(Clone, Copy, Debug)]
struct LineAim {
    /// The end that grows.
    end: usize,
    /// The point the extension stops at: the first geometry on the way, or the reach past the end
    /// when nothing is in the way.
    hit: Vec2,
    /// The curve that stops it there, and the stop as the sketch's own point when the sketch
    /// already has one there.
    cutter: Option<usize>,
    stop: Option<usize>,
}

/// The first geometry a line meets when it grows past the end of `ci` nearest `at`, or the reach
/// past that end when nothing is in the way. Two curves stopping it equally far are one stop: the
/// first of them is taken.
fn line_aim(sk: &Sketch, ci: usize, at: Vec2) -> Result<Option<LineAim>> {
    let cmd = "sketch.extend";
    // An infinite line has no end to grow: moving either of its points moves the whole line.
    let Some(curve) = sk.curves.get(ci).filter(|c| !c.infinite) else { return Ok(None) };
    let CurveKind::Line { a: pa, b: pb } = curve.kind else { return Ok(None) };
    let sh = sk.shape(ci).ok_or_else(|| bad(cmd, "curve"))?;
    let (qa, qb) = (sk.point(pa).unwrap_or_default(), sk.point(pb).unwrap_or_default());
    let at_b = at.dist(qb) <= at.dist(qa);
    let (end, end_pt) = if at_b { (pb, qb) } else { (pa, qa) };
    let dir = (end_pt - if at_b { qa } else { qb }).normalized().ok_or_else(|| bad(cmd, "cannot determine extension direction"))?;

    // A neighbour running on the same way as the extension has it nowhere to go.
    for (oi, c) in sk.curves.iter().enumerate() {
        if oi == ci || !uses(c, end) {
            continue;
        }
        // Arcs do not block a line: the line runs past the point they share.
        if let CurveKind::Line { a: na, b: nb } = c.kind {
            let other_idx = if na == end { nb } else { na };
            let v = sk.point(other_idx).unwrap_or_default() - end_pt;
            if v.cross(dir).abs() < 1e-6 && v.dot(dir) > 1e-6 {
                return Err(bad(cmd, "that end is connected to other geometry"));
            }
        }
    }

    // Candidate stops on the line's unbounded support beyond the chosen end.
    let length = qa.dist(qb);
    let mut best: Option<(f64, Vec2, usize)> = None;
    let mut budget = 1_000_000usize;
    let mut segment_count = 0usize;
    for (oi, other) in sk.curves.iter().enumerate() {
        if oi == ci {
            continue;
        }
        let mut pieces = sk.exact_segs(oi).unwrap_or_else(|| sk.segs(oi));
        // An infinite line (#1359) is a boundary along its whole line.
        if other.infinite {
            pieces = pieces.iter().map(|s| support::endless(s, sh.point(sh.param(at)))).collect();
        }
        segment_count += pieces.len();
        if segment_count > 16_384 {
            return Err(bad(cmd, "too many exact curve pieces to extend safely"));
        }
        for piece in &pieces {
            for q in support::hits(&sh, piece, &mut budget)? {
                let t = sh.param(q);
                let travel = if at_b { (t - 1.0) * length } else { -t * length };
                if travel <= TOL * 10.0 {
                    continue;
                }
                // Equally far stops are both on the way: the first one found is the stop.
                if best.is_none_or(|x| travel < x.0 - TOL) {
                    best = Some((travel, q, oi));
                }
            }
        }
    }

    let (hit, cutter, stop) = match best {
        Some((_, q, oi)) => (q, Some(oi), stop_point(sk, ci, q)),
        None => (end_pt + dir * EXTEND_REACH, None, None),
    };
    Ok(Some(LineAim { end, hit, cutter, stop }))
}

/// Preview the extend operation without modifying the sketch: the point the line past the end
/// nearest `hover_pos` reaches, from the same search `sketch.extend` makes. `None` when the curve
/// is not a line to extend, when it is an infinite line, or when the extension has nowhere to go
/// (the end is connected to a line running on from it).
pub fn extend_preview(sk: &solidcraft_sketch::Sketch, ci: usize, hover_pos: Vec2) -> Option<Vec2> {
    line_aim(sk, ci, hover_pos).ok().flatten().map(|aim| aim.hit)
}

/// One end of an arc growing along the arc's own circle, as a new arc, and what stops it.
///
/// Arcs extend both ways: past their `b` end counter-clockwise and past their `a` end clockwise,
/// each as far as the first geometry on that path. The path ends at the arc's far end, where the
/// arc itself begins again, so an extension never runs over the arc it grows from; reaching it
/// closes the gap between the two ends.
#[derive(Clone, Copy, Debug)]
struct ArcAim {
    /// The arc's end that grows (a point index), and the far end the extension must not pass.
    end: usize,
    far: usize,
    /// Whether the growing end is the arc's `b` end: `b` grows counter-clockwise, `a` clockwise.
    at_b: bool,
    /// The arc's circle: the centre itself and the radius.
    centre: Vec2,
    radius: f64,
    /// The angle the growing end sits at, and how far round the circle the extension travels.
    end_angle: f64,
    travel: f64,
    /// The point the extension stops at, and the curve that stops it there.
    hit: Vec2,
    cutter: usize,
}

impl ArcAim {
    /// The extension as points along the circle, the growing end first.
    fn points(&self) -> Vec<Vec2> {
        const N: usize = 64;
        let dir = if self.at_b { 1.0 } else { -1.0 };
        (0..=N).map(|i| self.centre + Vec2::from_angle(self.end_angle + dir * self.travel * i as f64 / N as f64) * self.radius).collect()
    }
}

/// The first geometry an arc meets when it grows from the end of `ci` nearest `at`. Both ends
/// grow: past `b` counter-clockwise, past `a` clockwise, and only as far as the nearest curve on
/// that path stops them. With nothing in the way the extension runs on to the arc's own far end,
/// closing the arc into a full circle. Two curves stopping it equally far are one stop: the first
/// is taken.
fn arc_aim(sk: &Sketch, ci: usize, at: Vec2, cmd: &str) -> Result<Option<ArcAim>> {
    let Some(CurveKind::Arc { a, b, .. }) = sk.curves.get(ci).map(|c| c.kind.clone()) else { return Ok(None) };
    let Some(Shape::Round { c: centre, r, start, sweep }) = sk.shape(ci) else { return Ok(None) };
    if !(r.is_finite() && r > 0.0) || !start.is_finite() || !sweep.is_finite() {
        return Err(bad(cmd, "the arc's circle is not usable"));
    }
    // A whole circle has no end to grow from.
    if sweep >= TAU - 1e-12 {
        return Ok(None);
    }
    let (Some(qa), Some(qb)) = (sk.point(a), sk.point(b)) else { return Ok(None) };
    let at_b = at.dist(qb) <= at.dist(qa);
    let (end, far, end_angle) = if at_b { (b, a, start + sweep) } else { (a, b, start) };
    // The extension runs from the growing end round to the far end, where the arc itself begins
    // again: it may stop on it (closing the gap between the two ends) but not pass it.
    let reach = TAU - sweep;
    let angle_tol = TOL / r;
    let far_tol = TOL * 10.0 / r;
    let circle = Shape::Round { c: centre, r, start: 0.0, sweep: TAU };
    let mut best: Option<(f64, Vec2, usize)> = None;
    let mut budget = 1_000_000usize;
    let mut segment_count = 0usize;
    for (oi, curve) in sk.curves.iter().enumerate() {
        if oi == ci {
            continue;
        }
        // A circle passes over arcs lying on it, so no intersection reports them: an arc on the
        // same circle stops the extension where it begins along the way, and the gap ends there.
        if matches!(curve.kind, CurveKind::Arc { .. })
            && let Some(Shape::Round { c: oc, r: or, start: os, sweep: ow }) = sk.shape(oi)
            && oc.dist(centre) <= TOL * 10.0
            && (or - r).abs() <= TOL * 10.0
        {
            if !(os.is_finite() && ow.is_finite() && ow > 0.0) {
                return Err(bad(cmd, "another arc on the same circle is not usable"));
            }
            let (entry, travel) = if at_b { (os, (os - end_angle).rem_euclid(TAU)) } else { (os + ow, (end_angle - os - ow).rem_euclid(TAU)) };
            // Lying on that arc already, or on the point it goes on from, leaves no gap to fill:
            // the extension could only double the arc that is there.
            let into = if at_b { (end_angle - os).rem_euclid(TAU) } else { (os + ow - end_angle).rem_euclid(TAU) };
            if travel <= angle_tol || into <= angle_tol || into < ow - angle_tol {
                return Err(bad(cmd, "the arc already continues into another arc on its circle"));
            }
            if travel > reach + far_tol {
                continue;
            }
            let entry = centre + Vec2::from_angle(entry) * r;
            keep_stop(&mut best, travel, entry, oi, angle_tol);
            continue;
        }
        let mut pieces = sk.exact_segs(oi).unwrap_or_else(|| sk.segs(oi));
        // An infinite line (#1359) is a boundary along its whole line.
        if curve.infinite {
            pieces = pieces.iter().map(|s| support::endless(s, centre)).collect();
        }
        segment_count += pieces.len();
        if segment_count > 16_384 {
            return Err(bad(cmd, "too many exact curve pieces to extend safely"));
        }
        for piece in &pieces {
            for q in support::hits(&circle, piece, &mut budget)? {
                let travel = if at_b { (q - centre).angle() - end_angle } else { end_angle - (q - centre).angle() }.rem_euclid(TAU);
                // The growing end itself, or past the arc's far end (where the extension would
                // run over the arc).
                if travel <= angle_tol || travel > reach + far_tol {
                    continue;
                }
                keep_stop(&mut best, travel, q, oi, angle_tol);
            }
        }
    }
    // Nothing stops the arc on its way round: it grows all the way to its own far end, closing
    // the gap between its ends into a full circle.
    let far_pt = if at_b { qa } else { qb };
    let (travel, hit, cutter) = best.unwrap_or((reach, far_pt, ci));
    Ok(Some(ArcAim { end, far, at_b, centre, radius: r, end_angle, travel, hit, cutter }))
}

/// Keep the stop the nearest of the candidates. Two curves stopping the extension equally far are
/// both on the way, so the first one found is taken instead of refusing to choose.
fn keep_stop(best: &mut Option<(f64, Vec2, usize)>, travel: f64, q: Vec2, oi: usize, angle_tol: f64) {
    if best.is_none_or(|x| travel < x.0 - angle_tol) {
        *best = Some((travel, q, oi));
    }
}

/// The point the extension ends at when the sketch already has one there: the arc's own far end
/// (closing the gap between its two ends), or the end of another curve the extension runs into —
/// an arc on the same circle across the gap, or any other curve crossing its path. Joining that
/// point keeps the geometry one chain instead of laying a second point over a point that is
/// there, so a circle met exactly where another curve already ends closes onto that curve.
/// `None` when the stop needs a new point.
fn arc_stop(sk: &Sketch, ci: usize, aim: &ArcAim) -> Option<usize> {
    let at_hit = |p: usize| sk.point(p).is_some_and(|pt| pt.dist(aim.hit) <= TOL * 10.0);
    if at_hit(aim.far) {
        return Some(aim.far);
    }
    sk.curves.iter().enumerate().filter(|(i, _)| *i != ci).find_map(|(_, c)| {
        let (a, b) = c.kind.ends()?;
        [a, b].into_iter().find(|p| at_hit(*p))
    })
}

/// Add the arc an extension needs: a new arc on the same circle as `ci`, from the growing end to
/// the stop, sweeping the way the extension went. The arc being extended keeps its geometry and
/// its constraints; the extension is new geometry.
fn add_arc_extension(sk: &mut Sketch, ci: usize, aim: &ArcAim, stop: Option<usize>) -> Result<String> {
    let centre = match sk.curves.get(ci).map(|c| c.kind.clone()) {
        Some(CurveKind::Arc { c, .. }) => c,
        _ => return Err(bad("sketch.extend", "only arcs extend along their circle")),
    };
    let stop = match stop {
        Some(p) => p,
        None => sk.add_point(aim.hit, None)?,
    };
    // `CurveKind::Arc` always sweeps counter-clockwise from `a` to `b`: growing `b` runs from the
    // arc's end to the stop, growing `a` runs the other way round, from the stop back to the end.
    let kind = if aim.at_b { CurveKind::Arc { c: centre, a: aim.end, b: stop } } else { CurveKind::Arc { c: centre, a: stop, b: aim.end } };
    sk.add_curve(kind, None)?;
    Ok(sk.points.get(stop).map(|x| x.id.clone()).unwrap_or_default())
}

/// The point the extension stops at when the sketch already has one there: an end of another
/// curve at the hit. Joining that point keeps the extension attached to the geometry that is
/// there instead of laying a second point over it. `None` when the stop needs a new point.
fn stop_point(sk: &Sketch, ci: usize, q: Vec2) -> Option<usize> {
    sk.curves.iter().enumerate().filter(|(i, _)| *i != ci).find_map(|(_, c)| {
        let (a, b) = c.kind.ends()?;
        [a, b].into_iter().find(|p| sk.point(*p).is_some_and(|pt| pt.dist(q) <= TOL * 10.0))
    })
}

/// Add the line an extension needs, from the growing end to the stop: the sketch's own point
/// when the stop already is one, a new point otherwise.
fn add_line_extension(sk: &mut Sketch, end: usize, q: Vec2, stop: Option<usize>) -> Result<String> {
    let b = match stop {
        Some(p) => p,
        None => sk.add_point(q, None)?,
    };
    sk.add_curve(CurveKind::Line { a: end, b }, None)?;
    Ok(sk.points.get(b).map(|x| x.id.clone()).unwrap_or_default())
}

/// Whether the growing end of an arc already carries geometry: another curve starts there, or a
/// constraint holds the point. Such an end keeps its place, so the extension has to be new
/// geometry.
fn arc_end_held(sk: &Sketch, ci: usize, end: usize) -> bool {
    if sk.curves.iter().enumerate().any(|(i, c)| i != ci && uses(c, end)) {
        return true;
    }
    sk.constraints.iter().any(|c| {
        use solidcraft_sketch::ConstraintKind::*;
        match c.kind {
            Coincident { p, q } => p == end || q == end,
            Fix { p } | PointOnCurve { p, .. } | Midpoint { p, .. } => p == end,
            HorizontalPoints { p, q } | VerticalPoints { p, q } | Symmetric { p, q, .. } => p == end || q == end,
            _ => false,
        }
    })
}

/// Extend an arc, in either direction, up to the first geometry on the way. A free end grows in
/// place, so the sketch keeps one arc; an end that already carries geometry, or a stop where the
/// sketch already has a point, is joined by a new arc instead.
fn extend_arc(sk: &mut Sketch, ci: usize, at: Vec2) -> Result<String> {
    let cmd = "sketch.extend";
    let Some(aim) = arc_aim(sk, ci, at, cmd)? else {
        return Err(bad(cmd, "an arc that is already a whole circle has no end to extend"));
    };
    let stop = arc_stop(sk, ci, &aim);
    // The arc's own far end is where the arc begins again: moving the end onto it would leave a
    // whole circle, so the gap is filled by a new arc joining that end.
    if stop.is_some_and(|p| p == aim.far) || arc_end_held(sk, ci, aim.end) {
        return add_arc_extension(sk, ci, &aim, stop);
    }
    // A free end moves along instead, if the solver lets it.
    let original = sk.point(aim.end).unwrap_or_default();
    if let Some(pt) = sk.points.get_mut(aim.end) {
        pt.pos = aim.hit;
    }
    let before = sk.points.iter().map(|p| p.pos).collect::<Vec<_>>();
    let hold = solidcraft_sketch::Hold { points: (0..sk.points.len()).filter(|i| *i != aim.end).collect(), radii: (0..sk.curves.len()).collect() };
    let moved = solidcraft_sketch::solve_with(sk, &hold, None, solidcraft_sketch::Analysis::StatusOnly, None).ok()
        && sk.point(aim.end).is_some_and(|point| point.dist(aim.hit) <= TOL)
        && sk.points.iter().enumerate().all(|(i, point)| i == aim.end || point.pos.dist(before[i]) <= TOL);
    if !moved {
        // The solve could not put the end where it belongs: the extension is a new arc instead.
        if let Some(pt) = sk.points.get_mut(aim.end) {
            pt.pos = original;
        }
        return add_arc_extension(sk, ci, &aim, stop);
    }
    // Held to the point that is already there or to what else stopped it, the end stays on the
    // geometry it reached.
    if let Some(stop) = stop {
        if stop != aim.end {
            add_c(sk, ConstraintKind::Coincident { p: aim.end, q: stop })?;
        }
    } else {
        pin(sk, aim.end, aim.cutter)?;
    }
    Ok(sk.points.get(aim.end).map(|x| x.id.clone()).unwrap_or_default())
}

/// The arc `sketch.extend` would add from the end of `ci` nearest `hover_pos`, for the viewport to
/// show before the click: the new arc as points, the growing end first. `None` when the arc cannot
/// grow (it is already a whole circle).
pub fn extend_arc_preview(sk: &solidcraft_sketch::Sketch, ci: usize, hover_pos: Vec2) -> Option<Vec<Vec2>> {
    let aim = arc_aim(sk, ci, hover_pos, "sketch.extend").ok().flatten()?;
    Some(aim.points())
}

fn extend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.extend";
    let at = req_vec2(cmd, p, "at")?;
    let (out, info) = edit(s, p, cmd, true, |sk, doc| {
        let rep = solve_current(sk, doc)?;
        if !rep.keeps_orientation() {
            return Err(bad(cmd, rep.failure(|_| "solve the existing sketch constraints before extending".into())));
        }
        let ci = curve_arg(sk, p, cmd)?;
        refuse_offset_ends(sk, ci, "extend", cmd)?;
        // An infinite line already reaches across the view: moving one of its two points would
        // move the whole line instead of extending it.
        if sk.curves.get(ci).is_some_and(|c| c.infinite) {
            return Err(bad(cmd, "an infinite line has no end to extend: make it a normal line first"));
        }
        // An arc grows along its own circle, either way round: in place when its end is free, as
        // a new arc when the end already carries geometry.
        if matches!(sk.curves.get(ci).map(|c| &c.kind), Some(CurveKind::Arc { .. })) {
            return extend_arc(sk, ci, at);
        }
        // The line grows past one end, out to the first geometry on the way, or the reach the
        // preview draws when nothing is in the way. A stop where the sketch already has a point
        // (another curve's end) joins that point instead of doubling it.
        let Some(aim) = line_aim(sk, ci, at)? else {
            return Err(bad(cmd, "only lines and arcs can be extended"));
        };
        let end = aim.end;
        let (q, cutter, stop) = (aim.hit, aim.cutter, aim.stop);
        let neighbors: Vec<_> = sk.curves.iter().enumerate().filter(|(i, c)| *i != ci && uses(c, end)).collect();

        // The end joined to other geometry stays where it is: add a line from it.
        if !neighbors.is_empty() {
            return add_line_extension(sk, end, q, stop);
        }

        // An end held by a constraint, or lying on a circle or arc, stays where it is too: the
        // extension is a new line from it.
        let end_is_held = sk.constraints.iter().any(|c| {
            use solidcraft_sketch::ConstraintKind::*;
            match c.kind {
                Coincident { p, q } => p == end || q == end,
                Fix { p } => p == end,
                PointOnCurve { p, .. } => p == end,
                Midpoint { p, .. } => p == end,
                HorizontalPoints { p, q } | VerticalPoints { p, q } => p == end || q == end,
                Symmetric { p, q, .. } => p == end || q == end,
                _ => false,
            }
        }) || {
            // Geometric check: does the endpoint lie on any circle or arc?
            let ep = sk.point(end).unwrap_or_default();
            sk.curves.iter().any(|curve| match curve.kind {
                CurveKind::Circle { c, r } => (ep - sk.point(c).unwrap_or_default()).len().abs() - r < 1e-6,
                // This is an arc's own start or end: it sits on that arc.
                CurveKind::Arc { a, b, .. } => ep.dist(sk.point(a).unwrap_or_default()) < 1e-6 || ep.dist(sk.point(b).unwrap_or_default()) < 1e-6,
                _ => false,
            })
        };

        if end_is_held {
            return add_line_extension(sk, end, q, stop);
        }

        // A free end moves along instead, if the solver lets it. The attempt is made on a copy,
        // so an end the solver cannot put where it belongs leaves the sketch as it was for the
        // new line below.
        let mut attempt = sk.clone();
        if let Some(pt) = attempt.points.get_mut(end) {
            pt.pos = q;
        }
        // Held to the point that is already there or to what else stopped it, an end stays where
        // the extension put it; an end with nothing to hold it is simply placed there.
        if let Some(stop) = stop {
            if stop != end {
                add_c(&mut attempt, ConstraintKind::Coincident { p: end, q: stop })?;
            }
        } else if let Some(cutter) = cutter {
            pin(&mut attempt, end, cutter)?;
        }
        let before = sk.points.iter().map(|p| p.pos).collect::<Vec<_>>();
        let hold =
            solidcraft_sketch::Hold { points: (0..attempt.points.len()).filter(|i| *i != end).collect(), radii: (0..attempt.curves.len()).collect() };
        let moved = solidcraft_sketch::solve_with(&mut attempt, &hold, None, solidcraft_sketch::Analysis::StatusOnly, None).ok()
            && attempt.point(end).is_some_and(|point| point.dist(q) <= TOL)
            && attempt.points.iter().enumerate().all(|(i, point)| i == end || point.pos.dist(before[i]) <= TOL);
        if moved {
            *sk = attempt;
            return Ok(sk.points.get(end).map(|x| x.id.clone()).unwrap_or_default());
        }
        // The solve could not put the end where it belongs: add a new line instead.
        add_line_extension(sk, end, q, stop)
    })?;
    Ok(json!({"moved": out, "sketch": info}))
}

fn uses(c: &Curve, p: usize) -> bool {
    c.kind.uses(p)
}

// ---------------------------------------------------------------------------------------------
// Corners: fillet and chamfer

/// A corner drawn as two curves whose ends are separate points held together by a Coincident
/// constraint (a line drawn to where an arc starts, #1208) becomes one shared point, so it can be
/// rounded or cut like a corner drawn through one point. Only a partner held by nothing but that
/// constraint is welded; anything else is left as it is.
fn weld_corner(sk: &mut Sketch, p: &Value) {
    let ends = |sk: &Sketch, c: usize| match sk.curves.get(c).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) | Some(CurveKind::Arc { a, b, .. }) => Some([*a, *b]),
        _ => None,
    };
    let q = if let Some(r) = str_(p, "point") {
        sk.resolve_point(r)
    } else {
        let curve = |k: &str| str_(p, k).and_then(|i| sk.curve_index(i)).and_then(|c| ends(sk, c));
        let (Some(ea), Some(eb)) = (curve("a"), curve("b")) else { return };
        if ea.iter().any(|x| eb.contains(x)) {
            return;
        }
        // The end of `a` held coincident with an end of `b`.
        ea.into_iter().find(|x| {
            sk.constraints
                .iter()
                .any(|k| matches!(k.kind, ConstraintKind::Coincident { p, q } if (p == *x && eb.contains(&q)) || (q == *x && eb.contains(&p))))
        })
    };
    let Some(mut q) = q else { return };
    // Each pass welds one partner; a pass removes a constraint, so this ends.
    for _ in 0..sk.constraints.len() {
        let partner = sk.constraints.iter().enumerate().find_map(|(i, k)| {
            let o = match k.kind {
                ConstraintKind::Coincident { p, q: r } if p == q && r != q => r,
                ConstraintKind::Coincident { p, q: r } if r == q && p != q => p,
                _ => return None,
            };
            let held_elsewhere = sk.constraints.iter().enumerate().any(|(j, k)| j != i && k.kind.refers_to_point(o));
            let linked = (0..sk.curves.len()).any(|c| sk.is_linked_curve(c) && sk.curves.get(c).is_some_and(|x| x.kind.uses(o)));
            (o != 0 && !held_elsewhere && !linked).then_some((i, o))
        });
        let Some((i, o)) = partner else { return };
        for c in sk.curves.iter_mut() {
            c.kind.map_points(&|x| if x == o { q } else { x });
        }
        sk.constraints.remove(i);
        sk.remove_points(&[o]);
        if o < q {
            q -= 1;
        }
    }
}

/// Fillets and chamfers refuse a corner on an infinite line (#1359): it has no end there.
fn refuse_infinite_corner(sk: &Sketch, p: &Value, cmd: &str) -> Result<()> {
    let infinite = |c: Option<usize>| c.and_then(|c| sk.curves.get(c)).is_some_and(|c| c.infinite);
    let at_corner = match str_(p, "point").and_then(|r| sk.resolve_point(r)) {
        Some(q) => sk.curves.iter().any(|c| c.infinite && c.kind.uses(q)),
        None => ["a", "b"].iter().any(|k| infinite(str_(p, k).and_then(|i| sk.curve_index(i)))),
    };
    if at_corner { Err(bad(cmd, "an infinite line has no corner to round or cut: make it a normal line first")) } else { Ok(()) }
}

/// Fillets and chamfers refuse a corner of an offset's output (#2874). The output's ends are
/// recomputed from its source curves on every solve, so a corner cut there would be pulled back
/// to the sharp corner, leaving full circles and an unsolved sketch.
fn refuse_offset_corner(sk: &Sketch, p: &Value, cmd: &str) -> Result<()> {
    let output = |c: usize| sk.curves.get(c).filter(|c| sk.offset_distance(&c.id).is_some()).map(|c| c.id.clone());
    let found = match str_(p, "point").and_then(|r| sk.resolve_point(r)) {
        Some(q) => (0..sk.curves.len()).filter(|c| sk.curves.get(*c).is_some_and(|x| x.kind.uses(q))).find_map(output),
        None => ["a", "b"].iter().find_map(|k| str_(p, k).and_then(|i| sk.curve_index(i)).and_then(output)),
    };
    match found {
        Some(id) => Err(bad(
            cmd,
            format!(
                "`{id}` is an offset, and its corners follow its source curves: round or cut the source corners before offsetting, or break the offset association first"
            ),
        )),
        None => Ok(()),
    }
}

/// A corner of two lines: (line a, line b, corner point, far end of a, far end of b).
fn corner(sk: &Sketch, p: &Value, cmd: &str) -> Result<(usize, usize, usize, usize, usize)> {
    let ends = |l: usize| match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    let (la, lb) = if let Some(r) = str_(p, "point") {
        let q = sk.resolve_point(r).ok_or_else(|| bad(cmd, format!("unknown point `{r}`")))?;
        let lines: Vec<usize> = (0..sk.curves.len()).filter(|i| ends(*i).is_some_and(|(a, b)| a == q || b == q) && !sk.is_linked_curve(*i)).collect();
        // A construction line ending there too (a diagonal that centres something) is not one of
        // the corner's sides: it keeps its end on the virtual sharp corner (#1026).
        let sides: Vec<usize> =
            if lines.len() == 2 { lines } else { lines.into_iter().filter(|i| sk.curves.get(*i).is_some_and(|c| !c.construction)).collect() };
        match sides[..] {
            [a, b] => (a, b),
            _ => return Err(bad(cmd, "the corner must join exactly two lines (or give the two lines as `a` and `b`)")),
        }
    } else {
        let a = str_(p, "a").and_then(|i| sk.curve_index(i)).ok_or_else(|| bad(cmd, "`a` must be a line"))?;
        let b = str_(p, "b").and_then(|i| sk.curve_index(i)).ok_or_else(|| bad(cmd, "`b` must be a line"))?;
        (a, b)
    };
    let (a0, a1) = ends(la).ok_or_else(|| bad(cmd, "fillets and chamfers work on two lines"))?;
    let (b0, b1) = ends(lb).ok_or_else(|| bad(cmd, "fillets and chamfers work on two lines"))?;
    let shared = [a0, a1].into_iter().find(|x| *x == b0 || *x == b1).ok_or_else(|| bad(cmd, "the lines must share a corner point"))?;
    let fa = if a0 == shared { a1 } else { a0 };
    let fb = if b0 == shared { b1 } else { b0 };
    Ok((la, lb, shared, fa, fb))
}

/// Replace the corner end of line `l` with point `np`.
fn reattach(sk: &mut Sketch, l: usize, corner: usize, np: usize) {
    if let Some(Curve { kind: CurveKind::Line { a, b }, .. }) = sk.curves.get_mut(l) {
        if *a == corner {
            *a = np;
        } else if *b == corner {
            *b = np;
        }
    }
}

/// Equal lengths and midpoints describe the original side, not its shortened tangent
/// segment. Keep that side as construction geometry sharing the original corner points.
/// Later corner cuts retain those points through `keep_corner`, so this also works when
/// the other end is rounded in a separate command.
fn keep_side_relations(sk: &mut Sketch, line: usize) -> Result<Vec<String>> {
    let affected = |k: &ConstraintKind| {
        matches!(*k, ConstraintKind::Equal { a, b } if a == line || b == line) || matches!(*k, ConstraintKind::Midpoint { l, .. } if l == line)
    };
    if !sk.constraints.iter().any(|k| affected(&k.kind)) {
        return Ok(Vec::new());
    }
    let Some(CurveKind::Line { a, b }) = sk.curves.get(line).map(|c| c.kind.clone()) else { return Ok(Vec::new()) };
    let support = sk.add_line_pts(a, b, None)?;
    if let Some(c) = sk.curves.get_mut(support) {
        c.construction = true;
    }
    let mut moved = Vec::new();
    for k in &mut sk.constraints {
        if !affected(&k.kind) {
            continue;
        }
        match &mut k.kind {
            ConstraintKind::Equal { a, b } => {
                if *a == line {
                    *a = support;
                }
                if *b == line {
                    *b = support;
                }
            }
            ConstraintKind::Midpoint { l, .. } => *l = support,
            _ => {}
        }
        moved.push(k.id.clone());
    }
    Ok(moved)
}

/// What became of a rounded or cut corner's point: kept as a virtual sharp corner (its id),
/// and the constraints and dimensions that now measure to it.
#[derive(Default)]
struct CornerKept {
    virtual_corner: Option<String>,
    moved: Vec<String>,
}

/// Keep corner point `c` of curves `ca`, `cb` (whose corner ends were just moved to `na`, `nb`)
/// when anything measures to it or places it: a constraint or dimension on the point, the length
/// of either line, a fixed or expression-driven position, or it is the origin or still ends another
/// curve. It stays as a virtual sharp corner, held on both curves' extensions (their
/// intersection), and the line lengths become distances from the far end to it, so the part
/// keeps its size. Otherwise None: the caller removes the point.
fn keep_corner(sk: &mut Sketch, c: usize, [(ca, na), (cb, nb)]: [(usize, usize); 2]) -> Result<Option<CornerKept>> {
    let used = sk.curves.iter().any(|x| uses(x, c));
    let mut moved: Vec<String> = sk.constraints.iter().filter(|k| k.kind.refers_to_point(c)).map(|k| k.id.clone()).collect();
    for k in sk.constraints.iter_mut() {
        let ConstraintKind::Length { l, value } = k.kind else { continue };
        let new_end = if l == ca {
            na
        } else if l == cb {
            nb
        } else {
            continue;
        };
        let Some(CurveKind::Line { a, b }) = sk.curves.get(l).map(|x| &x.kind) else { continue };
        // Same direction as the line, so the dimension keeps its side and text place.
        k.kind = if *a == new_end { ConstraintKind::Distance { p: c, q: *b, value } } else { ConstraintKind::Distance { p: *a, q: c, value } };
        moved.push(k.id.clone());
    }
    let positioned = sk.points.get(c).is_some_and(|pt| pt.fixed || pt.exprs.is_some());
    if moved.is_empty() && !used && c != 0 && !positioned {
        return Ok(None);
    }
    for cu in [ca, cb] {
        let k = ConstraintKind::PointOnCurve { p: c, c: cu };
        if !sk.constraints.iter().any(|x| x.kind == k) {
            add_c(sk, k)?;
        }
    }
    // A point of its own now: `l1.end` names the line's new end, not the virtual corner.
    let virtual_corner = if used || c == 0 {
        None
    } else {
        let id = sk.fresh("corner")?;
        if let Some(pt) = sk.points.get_mut(c) {
            pt.id = id.clone();
        }
        Some(id)
    };
    Ok(Some(CornerKept { virtual_corner, moved }))
}

/// Points at distances `da` along a and `db` along b from the corner; lines shortened to them.
fn cut_corner(
    sk: &mut Sketch,
    la: usize,
    lb: usize,
    c: usize,
    fa: usize,
    fb: usize,
    da: f64,
    db: f64,
    cmd: &str,
) -> Result<(usize, usize, Vec2, CornerKept)> {
    let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
    if !(da > 1e-9 && db > 1e-9 && da < pc.dist(pa) - 1e-9 && db < pc.dist(pb) - 1e-9) {
        return Err(bad(cmd, "too large for these lines"));
    }
    let u = (pa - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let v = (pb - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let ta = sk.add_point(pc + u * da, None)?;
    let tb = sk.add_point(pc + v * db, None)?;
    let mut moved = keep_side_relations(sk, la)?;
    moved.extend(keep_side_relations(sk, lb)?);
    reattach(sk, la, c, ta);
    reattach(sk, lb, c, tb);
    let mut kept = keep_corner(sk, c, [(la, ta), (lb, tb)])?;
    if let Some(k) = &mut kept {
        k.moved.extend(moved);
    }
    let removed = kept.is_none();
    if removed {
        sk.remove_points(&[c]);
    }
    // Indices shift when the corner point was removed.
    let fix = |i: usize| if removed && i > c { i - 1 } else { i };
    Ok((fix(ta), fix(tb), pc, kept.unwrap_or_default()))
}

/// A `points` entry: the two curves meeting at the corner (`a` and `b`) as given, or a point
/// reference to round.
fn corner_arg(q: &Value) -> Value {
    match q {
        Value::Object(o) if o.contains_key("a") || o.contains_key("b") || o.contains_key("point") => q.clone(),
        _ => json!({ "point": q }),
    }
}

fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.fillet";
    let r_expr = req_expr(cmd, p, "radius")?;
    // Several corners at once (`points`), each like a single `point`.
    let corners: Vec<Value> = match p.get("points").and_then(Value::as_array) {
        Some(a) if !a.is_empty() && a.len() <= 500 => a.iter().map(corner_arg).collect(),
        Some(_) => return Err(bad(cmd, "`points` must list 1…500 corners")),
        None => vec![p.clone()],
    };
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let r = doc.eval(&r_expr, solidcraft_doc::expr::Kind::Length).map_err(|e| bad(cmd, format!("radius: {e}")))?;
        if !(r > 1e-9 && r < 1e8) {
            return Err(bad(cmd, "radius must be positive"));
        }
        let pname = doc.new_model_param(&r_expr, "mm");
        let (mut arcs, mut cons, mut kept) = (Vec::new(), Vec::new(), Vec::new());
        for q in &corners {
            let (arc, mut c, k) = fillet_corner(sk, q, r, cmd)?;
            kept.push(k);
            // One radius for them all: the first carries the dimension, the rest are equal to it.
            match arcs.first() {
                None => c.push(sk.add_constraint(ConstraintKind::Radius { c: arc, value: r }, Some(pname.clone()))?),
                Some(first) => c.push(add_c(sk, ConstraintKind::Equal { a: *first, b: arc })?),
            }
            cons.extend(c);
            arcs.push(arc);
        }
        Ok((ids_of(sk, &arcs), cons, pname, kept_json(kept)))
    })?;
    let (vc, moved) = out.3;
    Ok(json!({"curves": out.0, "constraints": out.1, "param": out.2, "virtual_corners": vc, "moved_to_virtual_corner": moved, "sketch": info}))
}

/// The virtual corners kept and the constraints moved to them, for a response.
fn kept_json(kept: Vec<CornerKept>) -> (Vec<String>, Vec<String>) {
    let vcs = kept.iter().filter_map(|k| k.virtual_corner.clone()).collect();
    let mut moved: Vec<String> = Vec::new();
    for id in kept.into_iter().flat_map(|k| k.moved) {
        if !moved.contains(&id) {
            moved.push(id);
        }
    }
    (vcs, moved)
}

/// Round one corner (`point` or `a`, `b`) with an arc of radius `r` tangent to both curves:
/// the arc, the tangent constraints and what became of the corner point.
fn fillet_corner(sk: &mut Sketch, p: &Value, r: f64, cmd: &str) -> Result<(usize, Vec<String>, CornerKept)> {
    refuse_infinite_corner(sk, p, cmd)?;
    refuse_offset_corner(sk, p, cmd)?;
    weld_corner(sk, p);
    let (la, lb, c, fa, fb) = match corner(sk, p, cmd) {
        Ok(x) => x,
        // Not two lines: a corner with an arc.
        Err(e) => return round_corner(sk, p, r, cmd)?.ok_or(e),
    };
    let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
    let u = (pa - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let v = (pb - pc).normalized().ok_or_else(|| bad(cmd, "degenerate line"))?;
    let half = 0.5 * u.cross(v).atan2(u.dot(v)).abs();
    if !(1e-6..=std::f64::consts::FRAC_PI_2 - 1e-9).contains(&half) {
        return Err(bad(cmd, "the lines are parallel"));
    }
    let d = r / half.tan();
    if d >= pa.dist(pc) - 1e-9 || d >= pb.dist(pc) - 1e-9 {
        return Err(bad(cmd, format!("the radius is too large for that corner (it needs {d:.3} mm of each line)")));
    }
    let bis = (u + v).normalized().ok_or_else(|| bad(cmd, "the lines are parallel"))?;
    let center = pc + bis * (r / half.sin());
    let (ta, tb, _, kept) = cut_corner(sk, la, lb, c, fa, fb, d, d, cmd)?;
    let (qa, qb) = (sk.point(ta).unwrap_or_default(), sk.point(tb).unwrap_or_default());
    let ccw = (qa - center).cross(qb - center) > 0.0;
    let arc = if ccw {
        sk.add_arc(center, qa, qb, [None, Some(ta), Some(tb)], None)?
    } else {
        sk.add_arc(center, qb, qa, [None, Some(tb), Some(ta)], None)?
    };
    let cons = vec![add_c(sk, ConstraintKind::Tangent { a: la, b: arc })?, add_c(sk, ConstraintKind::Tangent { a: lb, b: arc })?];
    Ok((arc, cons, kept))
}

/// Fillet a corner where a line meets an arc, or two arcs meet: the arc of radius `r` touching
/// both, nearest the corner; both curves are cut back to it. None when the corner is not of
/// that kind.
fn round_corner(sk: &mut Sketch, p: &Value, r: f64, cmd: &str) -> Result<Option<(usize, Vec<String>, CornerKept)>> {
    let ends = |c: usize| match sk.curves.get(c).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) | Some(CurveKind::Arc { a, b, .. }) => Some((*a, *b)),
        _ => None,
    };
    let (ca, cb, q) = if let Some(rf) = str_(p, "point") {
        let Some(q) = sk.resolve_point(rf) else { return Ok(None) };
        let cs: Vec<usize> = (0..sk.curves.len()).filter(|i| ends(*i).is_some_and(|(a, b)| a == q || b == q) && !sk.is_linked_curve(*i)).collect();
        match cs[..] {
            [a, b] => (a, b, q),
            _ => return Ok(None),
        }
    } else {
        let (Some(a), Some(b)) = (str_(p, "a").and_then(|i| sk.curve_index(i)), str_(p, "b").and_then(|i| sk.curve_index(i))) else {
            return Ok(None);
        };
        let (Some((a0, a1)), Some((b0, b1))) = (ends(a), ends(b)) else { return Ok(None) };
        let Some(q) = [a0, a1].into_iter().find(|x| *x == b0 || *x == b1) else { return Ok(None) };
        (a, b, q)
    };
    let (Some(sa), Some(sb), Some(pc)) = (sk.shape(ca), sk.shape(cb), sk.point(q)) else { return Ok(None) };
    let offsets = |sh: &Shape| -> Vec<Shape> {
        match *sh {
            Shape::Line { a, b } => match (b - a).normalized() {
                Some(d) => [1.0, -1.0].iter().map(|s| Shape::Line { a: a + d.perp() * (r * s), b: b + d.perp() * (r * s) }).collect(),
                None => Vec::new(),
            },
            Shape::Round { c, r: rr, .. } => {
                let mut v = vec![Shape::Round { c, r: rr + r, start: 0.0, sweep: TAU }];
                if rr - r > 1e-9 {
                    v.push(Shape::Round { c, r: rr - r, start: 0.0, sweep: TAU });
                }
                v
            }
        }
    };
    let mut best: Option<(f64, Vec2, Vec2, Vec2)> = None;
    for oa in offsets(&sa) {
        for ob in offsets(&sb) {
            for cen in intersections(&oa, &ob) {
                let (ta, tb) = (sa.project(cen), sb.project(cen));
                if on_bounded(&sa, ta).is_none() || on_bounded(&sb, tb).is_none() || ta.dist(pc) < 1e-9 || tb.dist(pc) < 1e-9 {
                    continue;
                }
                let score = ta.dist(pc) + tb.dist(pc);
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, cen, ta, tb));
                }
            }
        }
    }
    let (_, cen, ta, tb) = best.ok_or_else(|| bad(cmd, "that radius does not fit the corner"))?;
    let (na, nb) = (sk.add_point(ta, None)?, sk.add_point(tb, None)?);
    for (c, n) in [(ca, na), (cb, nb)] {
        if let Some(cu) = sk.curves.get_mut(c) {
            cu.kind.map_points(&|i| if i == q { n } else { i });
        }
    }
    let ccw = (ta - cen).cross(tb - cen) > 0.0;
    let arc =
        if ccw { sk.add_arc(cen, ta, tb, [None, Some(na), Some(nb)], None)? } else { sk.add_arc(cen, tb, ta, [None, Some(nb), Some(na)], None)? };
    let cons = vec![add_c(sk, ConstraintKind::Tangent { a: ca, b: arc })?, add_c(sk, ConstraintKind::Tangent { a: cb, b: arc })?];
    match keep_corner(sk, q, [(ca, na), (cb, nb)])? {
        Some(kept) => Ok(Some((arc, cons, kept))),
        None => {
            let arc_id = sk.curves.get(arc).map(|c| c.id.clone()).unwrap_or_default();
            sk.remove_points(&[q]);
            let arc = sk.curve_index(&arc_id).unwrap_or(arc);
            Ok(Some((arc, cons, CornerKept::default())))
        }
    }
}

fn chamfer(s: &mut Session, p: &Value, cmd: &str, dists: impl FnOnce(f64) -> Result<(f64, f64)>) -> Result<Value> {
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        refuse_infinite_corner(sk, p, cmd)?;
        refuse_offset_corner(sk, p, cmd)?;
        weld_corner(sk, p);
        let (la, lb, c, fa, fb) = corner(sk, p, cmd)?;
        let (pc, pa, pb) = (sk.point(c).unwrap_or_default(), sk.point(fa).unwrap_or_default(), sk.point(fb).unwrap_or_default());
        let (u, v) = ((pa - pc).normalized().unwrap_or(Vec2::X), (pb - pc).normalized().unwrap_or(Vec2::Y));
        let corner_angle = u.cross(v).atan2(u.dot(v)).abs();
        let (da, db) = dists(corner_angle)?;
        let (ta, tb, _, kept) = cut_corner(sk, la, lb, c, fa, fb, da, db, cmd)?;
        let l = sk.add_line_pts(ta, tb, None)?;
        Ok((ids_of(sk, &[l]), kept_json(vec![kept])))
    })?;
    let (curves, (vc, moved)) = out;
    Ok(json!({"curves": curves, "virtual_corners": vc, "moved_to_virtual_corner": moved, "sketch": info}))
}

fn chamfer_equal(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.chamfer.equal_distance";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    chamfer(s, p, cmd, |_| Ok((d, d)))
}

fn chamfer_two(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.chamfer.two_distance";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    let d2 = num(p, "distance2").ok_or_else(|| bad(cmd, "`distance2` must be a number"))?;
    chamfer(s, p, cmd, |_| Ok((d, d2)))
}

fn chamfer_dist_angle(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.chamfer.distance_angle";
    let d = num(p, "distance").ok_or_else(|| bad(cmd, "`distance` must be a number"))?;
    let a = num(p, "angle").ok_or_else(|| bad(cmd, "`angle` must be a number (degrees)"))?.to_radians();
    chamfer(s, p, cmd, |phi| {
        // Triangle corner–Ta–Tb: angle phi at the corner, `a` at Ta.
        let opp = std::f64::consts::PI - a - phi;
        if !(a > 1e-6 && opp > 1e-6) {
            return Err(bad(cmd, "that angle does not meet the other line"));
        }
        Ok((d, d * a.sin() / opp.sin()))
    })
}

// ---------------------------------------------------------------------------------------------
// Offset

/// Curves of the connected chain through `start` (joined where exactly two curves meet), in
/// order, with whether each is walked from its stored start (`a`) to its end (`b`).
fn chain(sk: &Sketch, start: usize, pool: &[usize]) -> (Vec<(usize, bool)>, bool) {
    let ends = |c: usize| sk.curves.get(c).and_then(|c| c.kind.ends());
    let Some((s0, s1)) = ends(start) else { return (vec![(start, true)], true) };
    let next = |from: usize, at: usize, used: &[usize]| -> Option<usize> {
        let users: Vec<usize> = pool.iter().copied().filter(|c| ends(*c).is_some_and(|(a, b)| a == at || b == at)).collect();
        if users.len() != 2 {
            return None;
        }
        users.into_iter().find(|c| *c != from && !used.contains(c))
    };
    let mut fwd = vec![(start, true)];
    let mut used = vec![start];
    let (mut cur, mut at) = (start, s1);
    let mut closed = false;
    while let Some(n) = next(cur, at, &used) {
        let Some((a, b)) = ends(n) else { break };
        let forward = a == at;
        fwd.push((n, forward));
        used.push(n);
        cur = n;
        at = if forward { b } else { a };
        if used.len() > 10_000 {
            break;
        }
    }
    if at == s0 && fwd.len() > 1 {
        closed = true;
    } else {
        // Walk backwards from the start.
        let (mut cur, mut at) = (start, s0);
        let mut back = Vec::new();
        while let Some(n) = next(cur, at, &used) {
            let Some((a, b)) = ends(n) else { break };
            let forward = b == at;
            back.push((n, forward));
            used.push(n);
            cur = n;
            at = if forward { a } else { b };
            if used.len() > 10_000 {
                break;
            }
        }
        back.reverse();
        back.extend(fwd);
        fwd = back;
    }
    (fwd, closed)
}

/// An offset piece: shape walked in chain direction, offset by `d` to the left.
#[derive(Clone)]
enum Piece {
    Line(Vec2, Vec2),
    /// Centre point index, centre, radius, start point, end point (walk direction), ccw walk.
    Arc(usize, Vec2, f64, Vec2, Vec2, bool),
    /// A free-form curve's offset, sampled in walk direction (made into a fit spline).
    Free(Vec<Vec2>),
}

fn offset_piece(sk: &Sketch, c: usize, forward: bool, d: f64) -> Option<Piece> {
    match sk.curves.get(c)?.kind {
        CurveKind::Line { a, b } => {
            let (mut p, mut q) = (sk.point(a)?, sk.point(b)?);
            if !forward {
                std::mem::swap(&mut p, &mut q);
            }
            let n = (q - p).normalized()?.perp();
            Some(Piece::Line(p + n * d, q + n * d))
        }
        CurveKind::Arc { c: ci, a, b } => {
            let (cc, pa, pb) = (sk.point(ci)?, sk.point(a)?, sk.point(b)?);
            let r = cc.dist(pa);
            // Walking counter-clockwise, left is toward the centre.
            let nr = if forward { r - d } else { r + d };
            if nr <= 1e-9 {
                return None;
            }
            let at = |q: Vec2| cc + (q - cc).normalized().unwrap_or(Vec2::X) * nr;
            let (s, e) = if forward { (at(pa), at(pb)) } else { (at(pb), at(pa)) };
            Some(Piece::Arc(ci, cc, nr, s, e, forward))
        }
        CurveKind::Circle { .. } => None,
        // Free-form: offset the curve's samples along their normals.
        _ => {
            let mut poly = sk.polyline(c);
            if !forward {
                poly.reverse();
            }
            let n = poly.len();
            let pts: Vec<Vec2> = (0..n)
                .filter_map(|i| {
                    let (a, b) = (poly.get(i.saturating_sub(1))?, poly.get((i + 1).min(n - 1))?);
                    let t = (*b - *a).normalized()?;
                    Some(*poly.get(i)? + t.perp() * d)
                })
                .collect();
            (pts.len() >= 2).then_some(Piece::Free(pts))
        }
    }
}

fn piece_shape(p: &Piece) -> Shape {
    match p {
        Piece::Line(a, b) => Shape::Line { a: *a, b: *b },
        Piece::Arc(_, c, r, ..) => Shape::Round { c: *c, r: *r, start: 0.0, sweep: TAU },
        Piece::Free(v) => Shape::Line { a: v.first().copied().unwrap_or_default(), b: v.last().copied().unwrap_or_default() },
    }
}

/// Distance from `q` to an offset piece.
fn piece_dist(p: &Piece, q: Vec2) -> f64 {
    match p {
        Piece::Free(v) => v
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let t = if (b - a).len2() > 0.0 { ((q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0) } else { 0.0 };
                q.dist(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min),
        _ => piece_shape(p).dist(q),
    }
}

/// One side of an offset. The curves created at the signed distance, the parallel constraints
/// added with them, and the association when the boundary carries an offset topology.
struct Side {
    distance: f64,
    curves: Vec<String>,
    constraints: Vec<String>,
    association: Option<String>,
    param: Option<String>,
    /// Set when the offset is not linked to its source: a free-form curve whose plain offset
    /// folds over a tight bend or crosses itself (trimmed), or a chain of curves that
    /// includes free-form ones. Its curves then have no association and no distance to
    /// drive, and this names the source curves they were made from.
    unlinked_sources: Option<Vec<String>>,
}

/// The signed distances an offset runs at. One side is the distance as given. `symmetric` is the
/// same distance each way. `distance2` is an
/// independent distance the other way, as `distance2` means for `solid.extrude`. The primary side
/// comes first, so a one-sided caller reads the same first entry it always did.
fn offset_runs(d: f64, symmetric: bool, distance2: Option<f64>) -> Vec<f64> {
    match (symmetric, distance2) {
        (true, _) => vec![d.abs(), -d.abs()],
        (_, Some(second)) => vec![d, -second * d.signum()],
        _ => vec![d],
    }
}

/// Build one side of a chain offset at the signed distance `d`. `order` and `sources` are the
/// walked chain, computed once before anything is created so a second side walks the same
/// curves rather than a pool the first side has already grown.
fn offset_side(sk: &mut Sketch, cmd: &str, order: &[(usize, bool)], sources: &[(String, bool)], closed: bool, convex: bool, d: f64) -> Result<Side> {
    let mut made = Vec::new();
    let mut cons = Vec::new();
    if convex {
        let created = sk.add_convex_offset(sources.to_vec(), d)?;
        let roles = sk
            .offsets
            .iter()
            .filter_map(|offset| offset.topology.as_ref())
            .find(|topology| topology.id == created.association)
            .map(|topology| topology.roles.clone())
            .ok_or_else(|| bad(cmd, "new offset association is missing"))?;
        for (source, role) in sources.iter().zip(&roles) {
            if role.kind == solidcraft_sketch::OffsetRoleKind::Line {
                let a = sk.curve_index(&source.0).ok_or_else(|| bad(cmd, "offset source line vanished"))?;
                let b = sk.curve_index(&role.output).ok_or_else(|| bad(cmd, "offset output line vanished"))?;
                cons.push(add_c(sk, ConstraintKind::Parallel { a, b })?);
            }
        }
        return Ok(Side {
            distance: d,
            curves: created.curves,
            constraints: cons,
            association: Some(created.association),
            param: None,
            unlinked_sources: None,
        });
    }
    let pieces: Vec<Piece> =
        order.iter().map(|(c, f)| offset_piece(sk, *c, *f, d)).collect::<Option<_>>().ok_or_else(|| bad(cmd, "the offset collapses an arc"))?;
    let n = pieces.len();
    let start_of = |p: &Piece| match p {
        Piece::Line(a, _) => *a,
        Piece::Arc(_, _, _, s, _, _) => *s,
        Piece::Free(v) => v.first().copied().unwrap_or_default(),
    };
    let end_of = |p: &Piece| match p {
        Piece::Line(_, b) => *b,
        Piece::Arc(_, _, _, _, e, _) => *e,
        Piece::Free(v) => v.last().copied().unwrap_or_default(),
    };
    // Junction points between consecutive pieces.
    let joint = |x: &Piece, y: &Piece| -> Vec2 {
        let (e, s0) = (end_of(x), start_of(y));
        if e.dist(s0) < TOL * 10.0 {
            return e;
        }
        let guess = (e + s0) * 0.5;
        if matches!(x, Piece::Free(_)) || matches!(y, Piece::Free(_)) {
            return guess;
        }
        intersections(&piece_shape(x), &piece_shape(y)).into_iter().min_by(|a, b| a.dist(guess).total_cmp(&b.dist(guess))).unwrap_or(guess)
    };
    let (Some(head), Some(tail)) = (pieces.first(), pieces.last()) else {
        return Err(bad(cmd, "no curves"));
    };
    let mut joints: Vec<Vec2> = Vec::new();
    joints.push(if closed { joint(tail, head) } else { start_of(head) });
    for k in 0..n.saturating_sub(1) {
        let (Some(x), Some(y)) = (pieces.get(k), pieces.get(k + 1)) else {
            return Err(bad(cmd, "the offset chain changed while it was built"));
        };
        joints.push(joint(x, y));
    }
    if !closed {
        joints.push(end_of(tail));
    }
    // A straight piece whose joints come out reversed (or meet) was overrun by its
    // neighbours: the offset is larger than the shape allows there.
    for (k, pc) in pieces.iter().enumerate() {
        if let Piece::Line(a, b) = pc {
            let (ja, jb) = (joints.get(k).copied(), joints.get(if closed { (k + 1) % n } else { k + 1 }).copied());
            if let (Some(ja), Some(jb)) = (ja, jb)
                && (jb - ja).dot(*b - *a) <= TOL * a.dist(*b)
            {
                return Err(bad(cmd, "the offset is larger than the shape allows (the offset curves would cross)"));
            }
        }
    }
    let pts: Vec<usize> = joints.iter().map(|q| sk.add_point(*q, None)).collect::<std::result::Result<_, _>>()?;
    for (k, pc) in pieces.iter().enumerate() {
        let (Some(pa), Some(pb)) = (pts.get(k).copied(), pts.get(if closed { (k + 1) % n } else { k + 1 }).copied()) else {
            return Err(bad(cmd, "the offset chain changed while it was built"));
        };
        let orig = order.get(k).map(|(c, _)| *c).ok_or_else(|| bad(cmd, "the offset chain changed while it was built"))?;
        match pc.clone() {
            Piece::Free(v) => {
                // A fit spline through a few of the samples, ending at the joints.
                let step = (v.len() / 24).max(1);
                let mut idx = vec![pa];
                for q in v.iter().skip(step).step_by(step) {
                    if q.dist(*v.last().unwrap_or(q)) > 1e-6 {
                        idx.push(sk.add_point(*q, None)?);
                    }
                }
                idx.push(pb);
                idx.dedup();
                if idx.len() >= 2 {
                    made.push(sk.add_curve(CurveKind::Spline { pts: idx, control: false, degree: 3, periodic: false }, None)?);
                }
            }
            Piece::Line(..) => {
                let l = sk.add_line_pts(pa, pb, None)?;
                cons.push(add_c(sk, ConstraintKind::Parallel { a: orig, b: l })?);
                made.push(l);
            }
            Piece::Arc(ci, cc, _, _, _, ccw) => {
                let (qa, qb) = (sk.point(pa).unwrap_or_default(), sk.point(pb).unwrap_or_default());
                let a = if ccw {
                    sk.add_arc(cc, qa, qb, [Some(ci), Some(pa), Some(pb)], None)?
                } else {
                    sk.add_arc(cc, qb, qa, [Some(ci), Some(pb), Some(pa)], None)?
                };
                made.push(a);
            }
        }
    }
    let outputs = ids_of(sk, &made);
    if pieces.iter().all(|p| !matches!(p, Piece::Free(_))) {
        sk.offsets.push(solidcraft_sketch::OffsetChain { sources: sources.to_vec(), outputs: outputs.clone(), distance: d, closed, topology: None });
        sk.refresh_offsets()?;
    }
    Ok(Side { distance: d, curves: outputs, constraints: cons, association: None, param: None, unlinked_sources: None })
}

/// Numbers remain millimetres for programmatic callers; expressions use design units.
fn offset_length(s: &Session, p: &Value, key: &str, positive: bool) -> Result<(f64, String)> {
    let cmd = "sketch.offset";
    let expression = match num(p, key) {
        Some(v) => format!("{v} mm"),
        None => expr(p, key).ok_or_else(|| bad(cmd, format!("`{key}` must be a length expression")))?.into_string(),
    };
    let expression = s.doc.with_design_unit(&expression, solidcraft_doc::expr::Kind::Length);
    let value = s.doc.eval(&expression, solidcraft_doc::expr::Kind::Length)?;
    if !value.is_finite() || value.abs() <= 1e-9 || value.abs() > solidcraft_sketch::MAX_COORD || (positive && value < 0.0) {
        return Err(bad(cmd, format!("`{key}` must be a {}length within the sketch bounds", if positive { "positive " } else { "finite nonzero " })));
    }
    Ok((value, expression))
}

#[derive(Clone, Copy)]
struct OffsetOptions {
    side: Option<Vec2>,
    chain: bool,
    distance: f64,
    symmetric: bool,
    distance2: Option<f64>,
}

/// Build every side of one connected source group. The pool is frozen before any output
/// is created, so later groups cannot accidentally walk a newly created offset.
fn offset_group(sk: &mut Sketch, ids: &[String], eligible: &std::collections::HashSet<String>, options: OffsetOptions) -> Result<Vec<Side>> {
    let OffsetOptions { side, chain: use_chain, distance: d0, symmetric, distance2 } = options;
    let cmd = "sketch.offset";
    sk.refresh_offsets()?;
    let sel: Vec<usize> = ids.iter().map(|i| sk.curve_index(i).ok_or_else(|| bad(cmd, format!("unknown curve `{i}`")))).collect::<Result<_>>()?;
    let first = *sel.first().ok_or_else(|| bad(cmd, "no curves"))?;
    if sk.offsets.iter().filter_map(|offset| offset.topology.as_ref()).any(|topology| {
        topology.roles.iter().any(|role| role.retired) && topology.roles.iter().any(|role| !role.retired && ids.contains(&role.output))
    }) {
        return Err(bad(
            cmd,
            "offsets of a boundary with retired corner arcs are not supported yet; break its offset association to use the current fixed topology",
        ));
    }
    let mut sides: Vec<Side> = Vec::new();
    // Circles offset on their own, concentrically. Both sides are then a smaller circle and
    // a larger one; the one toward the centre still has to have a size.
    if let Some(CurveKind::Circle { c, r }) = sk.curves.get(first).map(|c| c.kind.clone()) {
        let cc = sk.point(c).unwrap_or_default();
        let source = sk.curves.get(first).map(|c| c.id.clone()).ok_or_else(|| bad(cmd, "offset source circle vanished"))?;
        let grow = match side {
            Some(q) => q.dist(cc) > r,
            None => d0 < 0.0,
        };
        let radii: Vec<f64> = match (symmetric, distance2) {
            (true, _) => vec![r - d0.abs(), r + d0.abs()],
            (_, Some(second)) if grow => vec![r + d0.abs(), r - second],
            (_, Some(second)) => vec![r - d0.abs(), r + second],
            _ if grow => vec![r + d0.abs()],
            _ => vec![r - d0.abs()],
        };
        for nr in radii {
            if nr <= 1e-9 {
                return Err(bad(cmd, "the offset circle would have no size"));
            }
            let ci = sk.add_circle(cc, nr, Some(c), None)?;
            let outputs = ids_of(sk, &[ci]);
            sk.offsets.push(solidcraft_sketch::OffsetChain {
                sources: vec![(source.clone(), true)],
                outputs: outputs.clone(),
                distance: r - nr,
                closed: true,
                topology: None,
            });
            sides.push(Side { distance: r - nr, curves: outputs, constraints: Vec::new(), association: None, param: None, unlinked_sources: None });
        }
        return Ok(sides);
    }
    // The chain is walked once, before anything is created: a second side has to follow the
    // same source curves, not a pool the first side has already added its output to.
    let pool: Vec<usize> = if use_chain {
        (0..sk.curves.len())
            .filter(|i| {
                sk.curves.get(*i).is_some_and(|c| eligible.contains(&c.id) && c.construction == sk.curves.get(first).is_some_and(|f| f.construction))
            })
            .collect()
    } else {
        sel.clone()
    };
    let (order, closed) = chain(sk, first, &pool);
    // Side: toward `side` (compare both offsets' distance to it), else the sign.
    let d = match side {
        Some(q) => {
            let dist = |dd: f64| {
                offset_piece(sk, first, order.iter().find(|x| x.0 == first).is_none_or(|x| x.1), dd).map(|pc| piece_dist(&pc, q)).unwrap_or(f64::MAX)
            };
            if dist(d0.abs()) <= dist(-d0.abs()) { d0.abs() } else { -d0.abs() }
        }
        None => d0,
    };
    let runs = offset_runs(d, symmetric, distance2);
    if order.iter().any(|(i, _)| sk.shape(*i).is_none()) {
        // A smooth free-form entity is offset from exact pieces. One curve gets a linked
        // offset where its plain offset is smooth and simple; otherwise (a fold or a
        // crossing to trim, or a chain with corners) the offset is the trimmed parallel
        // boundary, rounded at outer corners, and not linked to its source.
        let source_ids = order
            .iter()
            .map(|(i, _)| sk.curves.get(*i).map(|c| c.id.clone()).ok_or_else(|| bad(cmd, "offset source vanished")))
            .collect::<Result<Vec<_>>>()?;
        let unlinked = |sk: &Sketch, dd: f64, curves: &[usize]| Side {
            distance: dd,
            curves: ids_of(sk, curves),
            constraints: Vec::new(),
            association: None,
            param: None,
            unlinked_sources: Some(source_ids.clone()),
        };
        for dd in runs {
            let side = match order.as_slice() {
                [(source, forward)] => match sk.offset_freeform(*source, *forward, dd)? {
                    solidcraft_sketch::FreeformOffsetOutput::Linked(output) => Side {
                        distance: dd,
                        curves: ids_of(sk, &[output]),
                        constraints: Vec::new(),
                        association: None,
                        param: None,
                        unlinked_sources: None,
                    },
                    solidcraft_sketch::FreeformOffsetOutput::Unlinked(curves) => unlinked(sk, dd, &curves),
                },
                _ => {
                    let curves = sk.offset_freeform_chain(&order, dd)?;
                    unlinked(sk, dd, &curves)
                }
            };
            sides.push(side);
        }
        return Ok(sides);
    }
    let sources = order
        .iter()
        .map(|(index, forward)| {
            sk.curves.get(*index).map(|curve| (curve.id.clone(), *forward)).ok_or_else(|| bad(cmd, "offset chain source vanished"))
        })
        .collect::<Result<Vec<_>>>()?;
    let convex = closed && sk.convex_offset_supported(&sources)?;
    for dd in runs {
        sides.push(offset_side(sk, cmd, &order, &sources, closed, convex, dd)?);
    }
    Ok(sides)
}

fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.offset";
    let ids = string_list(p, "curves");
    if ids.is_empty() || ids.len() > 500 {
        return Err(bad(cmd, "`curves` must list 1…500 curves"));
    }
    let (value, expression) = offset_length(s, p, "distance", false)?;
    let expression = if value > 0.0 { expression } else { format!("-({expression})") };
    let d0 = if bool_(p, "flip").unwrap_or(false) { -value } else { value };
    let side = p.get("side").and_then(vec2);
    let use_chain = bool_(p, "chain").unwrap_or(true);
    // Both sides at once: `symmetric` is
    // one distance applied each way, `distance2` is an independent distance the other way.
    let symmetric = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
        None | Some("positive") | Some("one_side") => bool_(p, "symmetric").unwrap_or(false),
        Some("symmetric") => true,
        Some("negative" | "reverse" | "flip") => {
            return Err(bad(cmd, "an offset has no negative direction: give a negative `distance`, or a `side` to offset toward"));
        }
        Some(other) => return Err(bad(cmd, format!("unknown direction `{other}` (one_side, symmetric)"))),
    };
    let second = if p.get("distance2").is_some() { Some(offset_length(s, p, "distance2", true)?) } else { None };
    let distance2 = second.as_ref().map(|(value, _)| *value);
    let expression2 = second.map(|(_, expression)| expression);
    if symmetric && distance2.is_some() {
        return Err(bad(cmd, "give `distance2` or `symmetric`, not both"));
    }
    if symmetric && side.is_some() {
        return Err(bad(cmd, "`symmetric` offsets both sides, so `side` has no side left to pick: drop one of them"));
    }
    let ((sides, param, param2), info) = edit(s, p, cmd, true, |sk, doc| {
        sk.refresh_offsets()?;
        let rep = solve_current(sk, doc)?;
        if !rep.keeps_orientation() {
            return Err(bad(cmd, rep.failure(|_| "solve existing constraints before offsetting".into())));
        }
        sk.refresh_offsets()?;
        let eligible = sk.curves.iter().map(|c| c.id.clone()).collect::<std::collections::HashSet<_>>();
        let param = doc.new_model_param(&expression, "mm");
        let param2 = expression2.as_ref().map(|e| doc.new_model_param(e, "mm"));
        let made: Vec<String> = std::iter::once(param.clone()).chain(param2.clone()).collect();
        let mut pending = ids.clone();
        let mut sides = Vec::new();
        while !pending.is_empty() {
            let mut group = offset_group(sk, &pending, &eligible, OffsetOptions { side, chain: use_chain, distance: d0, symmetric, distance2 })?;
            let output = group.first().and_then(|side| side.curves.first()).ok_or_else(|| bad(cmd, "offset produced no curves"))?;
            let sources = sk
                .offsets
                .iter()
                .find(|o| o.owns_output(output))
                .map(|o| o.sources.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>())
                .or_else(|| sk.freeform_offsets.iter().find(|o| o.output == *output).map(|o| vec![o.source.clone()]))
                .or_else(|| group.iter().find_map(|side| side.unlinked_sources.clone()))
                .ok_or_else(|| bad(cmd, "offset association is missing"))?;
            // An offset of infinite lines is infinite lines too (#1359).
            let infinite = sources.iter().all(|id| sk.curve_index(id).and_then(|c| sk.curves.get(c)).is_some_and(|c| c.infinite));
            if infinite {
                for id in group.iter().flat_map(|side| side.curves.iter()) {
                    if let Some(cu) = sk.curve_index(id).and_then(|c| sk.curves.get_mut(c))
                        && matches!(cu.kind, CurveKind::Line { .. })
                    {
                        cu.infinite = true;
                        cu.construction = true;
                    }
                }
            }
            for (index, side) in group.iter_mut().enumerate() {
                // A trimmed offset is not linked to its source: it has no distance to drive.
                if side.unlinked_sources.is_some() {
                    continue;
                }
                let name = if index == 1 { param2.as_ref().unwrap_or(&param) } else { &param };
                let output = side.curves.first().ok_or_else(|| bad(cmd, "offset produced no curves"))?;
                side.constraints.push(sk.add_constraint(
                    ConstraintKind::OffsetDistance { output: output.clone(), value: side.distance.abs(), reverse: side.distance < 0.0 },
                    Some(name.clone()),
                )?);
                side.param = Some(name.clone());
            }
            let before = pending.len();
            pending.retain(|id| !sources.contains(id));
            if pending.len() == before {
                return Err(bad(cmd, "offset did not consume its source selection"));
            }
            sides.extend(group);
        }
        // Parameters made for distances that only trimmed (unlinked) offsets used drive nothing.
        let used = |name: &String| sides.iter().any(|side: &Side| side.param.as_ref() == Some(name));
        let param = used(&param).then_some(param);
        let param2 = param2.filter(|name| used(name));
        doc.params.retain(|p| !made.contains(&p.name) || param.as_ref() == Some(&p.name) || param2.as_ref() == Some(&p.name));
        Ok((sides, param, param2))
    })?;
    let curves: Vec<&String> = sides.iter().flat_map(|side| side.curves.iter()).collect();
    let constraints: Vec<&String> = sides.iter().flat_map(|side| side.constraints.iter()).collect();
    let each: Vec<Value> = sides
        .iter()
        .map(|side| {
            let mut entry = json!({"distance": side.distance, "curves": side.curves, "param": side.param});
            if let Some(association) = &side.association {
                entry["association"] = json!(association);
            }
            entry
        })
        .collect();
    let mut response = json!({"curves":curves,"constraints":constraints,"sketch":info,"sides":each,"param":param});
    if let Some(param2) = param2 {
        response["param2"] = json!(param2);
    }
    // One side keeps the single `association` it always returned. Two sides have one each, in
    // `sides`, so there is no single association left to name.
    if let [only] = sides.as_slice()
        && let Some(association) = &only.association
    {
        response["association"] = json!(association);
    }
    Ok(response)
}

fn break_offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.offset.break";
    let sketch = super::sketch::target_sketch(s, p, cmd)?;
    let (out, info) = edit(s, p, cmd, true, |sk, doc| {
        let output = str_(p, "curve").ok_or_else(|| bad(cmd, "`curve` must name an offset output"))?;
        let record = sk.freeform_offsets.iter().position(|record| record.output == output);
        let topology = sk
            .offsets
            .iter()
            .filter_map(|record| record.topology.as_ref())
            .any(|record| record.id == output || record.roles.iter().any(|role| role.output == output));
        let analytic = sk.offsets.iter().any(|o| o.outputs.iter().any(|id| id == output));
        if record.is_none() && !topology && !analytic {
            return Err(bad(cmd, "curve is not an offset output"));
        }
        // A parameter edit may have changed the evaluated source without
        // rewriting the stored fit points. Break must freeze the displayed,
        // current dimensioned geometry, rather than its stale stored predecessor.
        let rep = solve_current(sk, doc)?;
        if !rep.keeps_orientation() {
            return Err(bad(cmd, rep.failure(|_| "cannot break an offset with conflicting dimensions".into())));
        }
        sk.validate_periodic_splines()?;
        if let Some(association) = sk
            .offsets
            .iter()
            .filter_map(|record| record.topology.as_ref())
            .find(|record| record.id == output || record.roles.iter().any(|role| role.output == output))
            .map(|record| record.id.clone())
        {
            freeze_offset_profiles(doc, sketch, sk, &association)?;
        }
        if let Some(curves) = sk.break_offset_topology(output)? {
            return Ok((output.to_string(), Some(curves)));
        }
        let ordinary = sk.offsets.iter().position(|o| o.topology.is_none() && o.outputs.iter().any(|id| id == output));
        if let Some(index) = ordinary {
            let record = sk.offsets.remove(index);
            let centres = record
                .outputs
                .iter()
                .filter_map(|id| sk.curve_index(id).and_then(|i| sk.curves.get(i)))
                .filter_map(|curve| match curve.kind {
                    CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => Some(c),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>();
            for centre in centres {
                let position = sk.point(centre).ok_or_else(|| bad(cmd, "missing offset centre"))?;
                let detached = sk.add_point(position, None)?;
                for curve in &mut sk.curves {
                    if record.outputs.contains(&curve.id) {
                        match &mut curve.kind {
                            CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } if *c == centre => *c = detached,
                            _ => {}
                        }
                    }
                }
            }
            sk.constraints.retain(|c| !matches!(&c.kind, ConstraintKind::OffsetDistance { output, .. } if record.outputs.contains(output)));
            for id in &record.outputs {
                if let Some(curve) = sk.curve_index(id).and_then(|i| sk.curves.get(i)) {
                    let points = curve.kind.point_ids();
                    for i in points {
                        if i != 0
                            && sk.curves.iter().all(|c| !c.kind.uses(i) || record.outputs.contains(&c.id))
                            && let Some(p) = sk.points.get_mut(i)
                        {
                            p.fixed = false;
                        }
                    }
                }
            }
            return Ok((output.to_string(), Some(record.outputs)));
        }
        let record = record.ok_or_else(|| bad(cmd, "offset association changed while materializing its current geometry"))?;
        sk.constraints.retain(|c| !matches!(&c.kind, ConstraintKind::OffsetDistance { output: id, .. } if id == output));
        let ci = sk.curve_index(output).ok_or_else(|| bad(cmd, "missing offset output"))?;
        let indices = sk.curves[ci].kind.point_ids();
        sk.freeform_offsets.remove(record);
        for i in indices {
            if let Some(point) = sk.points.get_mut(i) {
                point.fixed = false;
            }
        }
        Ok((output.to_string(), None))
    })?;
    s.doc_mut().prune_model_params();
    let mut response = json!({"curve":out.0,"sketch":info});
    if let Some(curves) = out.1 {
        response["curves"] = json!(curves);
    }
    Ok(response)
}

/// A broken logical boundary becomes ordinary geometry. Keep downstream features
/// on that exact current boundary while other associations retain their full roles.
fn freeze_offset_profiles(doc: &mut solidcraft_doc::Document, sketch: u64, boundary: &Sketch, association: &str) -> Result<()> {
    let cmd = "sketch.offset.break";
    let mut groups = std::collections::BTreeMap::new();
    let mut work = 0usize;
    for offset in &boundary.offsets {
        let Some(topology) = &offset.topology else { continue };
        work = work.saturating_add(topology.roles.len());
        if groups.len() >= 128 || topology.roles.len() > 512 || work > 1_000_000 {
            return Err(bad(cmd, "too many offset roles to freeze safely"));
        }
        let reserved = topology.roles.iter().map(|role| role.output.clone()).collect::<Vec<_>>();
        if groups.insert(topology.id.clone(), (offset.outputs.clone(), reserved)).is_some() {
            return Err(bad(cmd, "ambiguous offset association identity"));
        }
    }
    let (active, reserved) = groups.get(association).ok_or_else(|| bad(cmd, "offset association disappeared before freezing"))?;
    let mut reserved_sorted = reserved.clone();
    reserved_sorted.sort();
    for feature in &mut doc.features {
        for (source, selection) in feature.kind.profile_sels_mut() {
            if source != sketch {
                continue;
            }
            match selection {
                ProfileSel::OffsetBoundaries { associations } if associations.iter().any(|id| id == association) => {
                    let mut loops = Vec::with_capacity(associations.len());
                    for id in associations.iter() {
                        let (current, all_roles) = groups.get(id).ok_or_else(|| bad(cmd, format!("cannot freeze missing offset boundary `{id}`")))?;
                        let roles = if id == association { current } else { all_roles };
                        work = work.saturating_add(roles.len());
                        if work > 1_000_000 {
                            return Err(bad(cmd, "too many downstream offset references to freeze safely"));
                        }
                        loops.push(roles.clone());
                    }
                    *selection = ProfileSel::Curves { loops };
                }
                ProfileSel::Curves { loops } => {
                    for ids in loops {
                        work = work.saturating_add(ids.len());
                        if work > 1_000_000 {
                            return Err(bad(cmd, "too many downstream curve references to freeze safely"));
                        }
                        if ids.len() == reserved_sorted.len() {
                            let mut sorted = ids.clone();
                            sorted.sort();
                            if sorted == reserved_sorted {
                                *ids = active.clone();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Copies: mirror, patterns, move, scale

/// Curves and lone points named in `entities`.
fn entities(sk: &Sketch, p: &Value, cmd: &str) -> Result<(Vec<usize>, Vec<usize>)> {
    let ids = string_list(p, "entities");
    if ids.is_empty() {
        return Err(bad(cmd, "`entities` must list sketch curves or points"));
    }
    let (mut cs, mut ps) = (Vec::new(), Vec::new());
    for id in &ids {
        if let Some(c) = sk.curve_index(id) {
            cs.push(c);
        } else if let Some(q) = sk.resolve_point(id) {
            ps.push(q);
        } else {
            return Err(bad(cmd, format!("unknown sketch entity `{id}`")));
        }
    }
    Ok((cs, ps))
}

/// Points used by curves plus lone points, in a stable order.
fn involved_points(sk: &Sketch, cs: &[usize], ps: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for c in cs {
        if let Some(k) = sk.curves.get(*c).map(|c| &c.kind) {
            v.extend(k.point_ids());
        }
    }
    v.extend(ps);
    let mut out = Vec::new();
    for q in v {
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

/// Copy curves and points through `f`. `flip` reverses arcs (mirroring). `keep(p)`: reuse the
/// original point instead of copying it (points on a mirror line). Returns the new curves and
/// the point map (original → copy).
fn copy_through(
    sk: &mut Sketch,
    cs: &[usize],
    ps: &[usize],
    f: &dyn Fn(Vec2) -> Vec2,
    flip: bool,
    keep: &dyn Fn(Vec2) -> bool,
    scale_r: f64,
) -> Result<(Vec<usize>, Vec<(usize, usize)>)> {
    if sk.curves.len().saturating_add(cs.len()) > solidcraft_sketch::MAX_CURVES {
        return Err(solidcraft_sketch::SketchError::TooLarge.into());
    }
    let pts = involved_points(sk, cs, ps);
    let mut map: Vec<(usize, usize)> = Vec::new();
    for q in &pts {
        let pos = sk.point(*q).unwrap_or_default();
        let n = if keep(pos) { *q } else { sk.add_point(f(pos), None)? };
        map.push((*q, n));
    }
    let m = |q: usize| map.iter().find(|x| x.0 == q).map(|x| x.1).unwrap_or(q);
    let mut made = Vec::new();
    for c in cs {
        let Some(cu) = sk.curves.get(*c).cloned() else { continue };
        let mut kind = cu.kind.clone();
        kind.map_points(&m);
        let (kind, prefix) = match cu.kind {
            CurveKind::Ellipse { r, .. } => {
                if let CurveKind::Ellipse { r: slot, .. } = &mut kind {
                    *slot = r * scale_r;
                }
                (kind, "e")
            }
            CurveKind::Spline { .. } => (kind, "s"),
            CurveKind::Conic { .. } => (kind, "k"),
            CurveKind::Line { a, b } => (CurveKind::Line { a: m(a), b: m(b) }, "l"),
            CurveKind::Circle { c, r } => (CurveKind::Circle { c: m(c), r: r * scale_r }, "c"),
            CurveKind::Arc { c, a, b } => {
                if flip {
                    (CurveKind::Arc { c: m(c), a: m(b), b: m(a) }, "a")
                } else {
                    (CurveKind::Arc { c: m(c), a: m(a), b: m(b) }, "a")
                }
            }
        };
        let id = sk.fresh(prefix)?;
        // A mirrored or patterned infinite line is an infinite line too (#1359).
        let infinite = cu.infinite && matches!(kind, CurveKind::Line { .. });
        sk.curves.push(Curve {
            id,
            kind,
            construction: cu.construction,
            reversed: if flip { !cu.reversed } else { cu.reversed },
            link: None,
            centerline: false,
            fixed: false,
            infinite,
        });
        made.push(sk.curves.len() - 1);
    }
    Ok((made, map))
}

fn mirror(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.mirror";
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let lid = str_(p, "line").ok_or_else(|| bad(cmd, "`line` must be the mirror line"))?;
        let l = sk.curve_index(lid).ok_or_else(|| bad(cmd, format!("unknown curve `{lid}`")))?;
        let Some(Shape::Line { a, b }) = sk.shape(l) else { return Err(bad(cmd, "the mirror must be a line")) };
        let u = (b - a).normalized().ok_or_else(|| bad(cmd, "degenerate mirror line"))?;
        let refl = move |q: Vec2| {
            let foot = a + u * (q - a).dot(u);
            foot * 2.0 - q
        };
        let on_line = move |q: Vec2| u.cross(q - a).abs() < TOL * 10.0;
        let cs: Vec<usize> = cs.into_iter().filter(|c| *c != l).collect();
        let (made, map) = copy_through(sk, &cs, &ps, &refl, true, &on_line, 1.0)?;
        let mut cons = Vec::new();
        for (o, n) in &map {
            if o != n {
                cons.push(add_c(sk, ConstraintKind::Symmetric { p: *o, q: *n, l })?);
            } else if !sk.curves.get(l).is_some_and(|curve| curve.kind.point_ids().contains(o)) {
                // A shared endpoint must stay on the mirror line as the source changes.
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: *o, c: l })?);
            }
        }
        for (o, n) in cs.iter().zip(&made) {
            if matches!(sk.curves.get(*o).map(|c| &c.kind), Some(CurveKind::Circle { .. })) {
                cons.push(add_c(sk, ConstraintKind::Equal { a: *o, b: *n })?);
            }
        }
        Ok((ids_of(sk, &made), cons))
    })?;
    Ok(json!({"curves": out.0, "constraints": out.1, "sketch": info}))
}

fn point_or_ref(sk: &Sketch, p: &Value, k: &str, cmd: &str) -> Result<Vec2> {
    match p.get(k) {
        Some(Value::String(r)) => sk.resolve_point(r).and_then(|i| sk.point(i)).ok_or_else(|| bad(cmd, format!("unknown point `{r}`"))),
        Some(v) => vec2(v).ok_or_else(|| bad(cmd, format!("`{k}` must be [x, y] or a point id"))),
        None => Err(bad(cmd, format!("missing `{k}`"))),
    }
}

fn rotate(c: Vec2, ang: f64) -> impl Fn(Vec2) -> Vec2 {
    move |q: Vec2| {
        let d = q - c;
        let (s, co) = ang.sin_cos();
        c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }
}

fn pattern_circular(s: &mut Session, p: &Value) -> Result<Value> {
    pattern::run(s, p, true)
}

fn pattern_rect(s: &mut Session, p: &Value) -> Result<Value> {
    pattern::run(s, p, false)
}

/// Move the points of the entities (in place) through `f`; circle radii scale by `k`.
fn transform_in_place(sk: &mut Sketch, cs: &[usize], ps: &[usize], f: &dyn Fn(Vec2) -> Vec2, k: f64, cmd: &str) -> Result<()> {
    let pts = involved_points(sk, cs, ps);
    if pts.iter().any(|q| sk.points.get(*q).is_some_and(|x| x.fixed || x.link.is_some())) {
        return Err(bad(cmd, "fixed or projected geometry cannot move"));
    }
    // A point the origin holds goes where the move takes it: the coincidence that held it
    // there is dropped (#5034).
    let moved: Vec<usize> = pts
        .iter()
        .copied()
        .filter(|q| {
            let p = sk.point(*q).unwrap_or_default();
            (f(p) - p).len() > TOL
        })
        .collect();
    release_origin_holds(sk, &moved);
    for q in pts {
        if let Some(x) = sk.points.get_mut(q) {
            x.pos = f(x.pos);
        }
    }
    for c in cs {
        if let Some(Curve { kind: CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }, .. }) = sk.curves.get_mut(*c) {
            *r *= k;
        }
    }
    sk.follow_pattern_seeds();
    Ok(())
}

/// Turn the constraints tied to the sketch axes with geometry rotated by `angle` (radians), when
/// every point they refer to is in `moved`: horizontal and vertical relations and dimensions swap
/// at odd quarter turns and stay at half turns, and a line's angle dimension turns with the line,
/// its parameter too. Horizontal and vertical relations and dimensions cannot hold at any other
/// angle and are removed. Every other constraint keeps its meaning under a rigid move.
fn rotate_axis_constraints(sk: &mut Sketch, doc: &mut solidcraft_doc::Document, moved: &[usize], angle: f64) -> Result<()> {
    let quarters = angle.rem_euclid(TAU) / FRAC_PI_2;
    let turns = quarters.round();
    if turns.rem_euclid(4.0) == 0.0 && (quarters - turns).abs() < 1e-10 {
        return Ok(());
    }
    let square = (quarters - turns).abs() < 1e-10;
    let odd = square && turns.rem_euclid(2.0) == 1.0;
    let curves = &sk.curves;
    let mut angles = Vec::new();
    sk.constraints.retain_mut(|c| {
        if c.driven {
            return true;
        }
        let (mut points, refs) = c.kind.indices();
        for l in refs {
            match curves.get(l) {
                Some(curve) => points.extend(curve.kind.point_ids()),
                None => return true,
            }
        }
        if points.is_empty() || !points.iter().all(|q| moved.contains(q)) {
            return true;
        }
        use ConstraintKind::*;
        let swapped = match c.kind {
            Horizontal { l } => Vertical { l },
            Vertical { l } => Horizontal { l },
            HorizontalPoints { p, q } => VerticalPoints { p, q },
            VerticalPoints { p, q } => HorizontalPoints { p, q },
            DistanceX { p, q, value } => DistanceY { p, q, value },
            DistanceY { p, q, value } => DistanceX { p, q, value },
            LineAngle { l, value } => {
                let turned = (value + angle + PI).rem_euclid(TAU) - PI;
                c.kind = LineAngle { l, value: turned };
                if let Some(name) = &c.param {
                    angles.push((name.clone(), turned));
                }
                return true;
            }
            _ => return true,
        };
        if odd {
            c.kind = swapped;
        }
        square
    });
    for (name, turned) in angles {
        let Some(param) = doc.param(&name) else { continue };
        let degrees = (angle.to_degrees() * 1e6).round() / 1e6;
        // A literal becomes the new angle; a formula keeps its references and gains the turn.
        let expr = if solidcraft_doc::expr::references(&param.expr).is_empty() {
            format!("{} deg", (turned.to_degrees() * 1e6).round() / 1e6)
        } else {
            format!("({}) + {degrees} deg", param.expr)
        };
        doc.set_param(&name, &expr, None, None)?;
    }
    Ok(())
}

/// Drop the coincidences that held the moved points to the sketch's origin: a move translates
/// the geometry it is given, and the origin does not travel with it (#5034).
fn release_origin_holds(sk: &mut Sketch, pts: &[usize]) {
    let Some(origin) = sk.point_index("origin") else { return };
    sk.constraints.retain(
        |c| !matches!(&c.kind, ConstraintKind::Coincident { p, q } if (pts.contains(p) && *q == origin) || (pts.contains(q) && *p == origin)),
    );
}

fn move_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move";
    let t = if p.get("translate").is_some() { req_vec2(cmd, p, "translate")? } else { Vec2::ZERO };
    let ang = if p.get("angle").is_some() { num(p, "angle").ok_or_else(|| bad(cmd, "`angle` must be finite"))? } else { 0.0 }.to_radians();
    let copy = bool_(p, "copy").unwrap_or(false);
    let rotate_constraints = bool_(p, "rotate_constraints").unwrap_or(true);
    let (out, info) = edit(s, p, cmd, false, |sk, doc| {
        let (mut cs, mut ps) = entities(sk, p, cmd)?;
        let c = if p.get("center").is_some() { point_or_ref(sk, p, "center", cmd)? } else { Vec2::ZERO };
        let rot = rotate(c, ang);
        let f = move |q: Vec2| rot(q) + t;
        let mut links = Vec::new();
        for entity in string_list(p, "entities") {
            if let Some(link) = super::sketch_create::text_link(sk, &entity)
                && !links.iter().any(|old| old == link)
            {
                links.push(link.to_owned());
            }
        }
        cs.retain(|&i| !sk.curves.get(i).and_then(|c| c.link.as_ref()).is_some_and(|link| links.contains(link)));
        ps.retain(|&i| !sk.points.get(i).and_then(|p| p.link.as_ref()).is_some_and(|link| links.contains(link)));
        let mut text_curves = Vec::new();
        for link in links {
            text_curves.extend(super::sketch_create::move_text(sk, &link, t, ang, c, copy)?);
        }
        if copy {
            let (mut m, _) = copy_through(sk, &cs, &ps, &f, false, &|_| false, 1.0)?;
            m.extend(text_curves);
            return Ok(ids_of(sk, &m));
        }
        transform_in_place(sk, &cs, &ps, &f, 1.0, cmd)?;
        if rotate_constraints {
            let moved = involved_points(sk, &cs, &ps);
            rotate_axis_constraints(sk, doc, &moved, ang)?;
        }
        cs.extend(text_curves);
        Ok(ids_of(sk, &cs))
    })?;
    s.doc_mut().prune_model_params();
    Ok(json!({"curves": out, "sketch": info}))
}

/// Scale, turn and move the entities in one step: scaled about `center`, then turned about it,
/// then moved by `translate`.
fn transform(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.transform";
    let k = match p.get("factor") {
        Some(v) => v.as_f64().filter(|k| *k > 1e-9 && *k < 1e6).ok_or_else(|| bad(cmd, "`factor` must be a positive number"))?,
        None => 1.0,
    };
    let ang = match p.get("angle") {
        Some(v) => v.as_f64().filter(|a| a.is_finite()).ok_or_else(|| bad(cmd, "`angle` must be a number (degrees)"))?.to_radians(),
        None => 0.0,
    };
    let t = if p.get("translate").is_some() { req_vec2(cmd, p, "translate")? } else { Vec2::ZERO };
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let c = if p.get("center").is_some() { point_or_ref(sk, p, "center", cmd)? } else { Vec2::ZERO };
        let rot = rotate(c, ang);
        let f = move |q: Vec2| rot(c + (q - c) * k) + t;
        transform_in_place(sk, &cs, &ps, &f, k, cmd)?;
        Ok(ids_of(sk, &cs))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.scale";
    let k = num(p, "factor").filter(|k| *k > 1e-9 && *k < 1e6).ok_or_else(|| bad(cmd, "`factor` must be a positive number"))?;
    let (out, info) = edit(s, p, cmd, false, |sk, _| {
        let (cs, ps) = entities(sk, p, cmd)?;
        let b = if p.get("base").is_some() { point_or_ref(sk, p, "base", cmd)? } else { Vec2::ZERO };
        let f = move |q: Vec2| b + (q - b) * k;
        transform_in_place(sk, &cs, &ps, &f, k, cmd)?;
        Ok(ids_of(sk, &cs))
    })?;
    Ok(json!({"curves": out, "sketch": info}))
}

#[cfg(test)]
#[path = "sketch_modify_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sketch_robust_tests.rs"]
mod robust_tests;

#[cfg(test)]
#[path = "sketch_corner_tests.rs"]
mod corner_tests;

#[cfg(test)]
#[path = "sketch_trim_path_tests.rs"]
mod trim_path_tests;
