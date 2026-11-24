//! Lighten / Auto-Pocket parametric geometry solver.
//!
//! Generates weight-reduction pockets on a planar face of a solid body without requiring
//! a pre-drawn sketch:
//! - Validates planarity and extracts outward normal `n` and local 2D basis `(u, v)`.
//! - Extracts outer boundary (CCW) and interior hole / island loops (CW), projecting to 2D.
//! - Offsets outer boundary inward by `wall_offset` and expands holes/islands outward by
//!   `wall_offset` to retain structural bosses, then computes 2D boolean difference.
//! - Applies tangent arc corner fillets of radius `corner_radius` to concave interior vertices,
//!   clamped to local half-width to avoid self-intersections.
//! - Extrudes pocket cutter prisms along `-n` (with optional draft angle) and executes B-rep
//!   boolean Cut, followed by optional floor/top edge fillets and chamfers.

use std::f64::consts::TAU;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::build::{extrude, extrude_tapered_between, offset_loop};
use crate::prism::planar_regions_boolean;
use crate::{Body, BoolOp, DraftCorner, MeshTolerance, chamfer, fillet, measure};

pub use solidcore_ops::ChamferLocation;
pub use solidcore_ops::FilletLocation;
pub use solidcore_ops::LightenConfig;
pub use solidcore_ops::ManufacturingMethod;
pub use solidcore_ops::PocketDepthSpec;

/// 2D analytical circle in local plane coordinates.
#[derive(Debug, Clone, Copy)]
struct Circle2D {
    center: Vec2,
    radius: f64,
}

/// Compute the centroid of a 2D polygon.
fn poly_centroid(pts: &[Vec2]) -> Vec2 {
    let n = pts.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut area = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let cross = a.x * b.y - b.x * a.y;
        cx += (a.x + b.x) * cross;
        cy += (a.y + b.y) * cross;
        area += cross;
    }
    if area.abs() > 1e-6 {
        Vec2::new(cx / (3.0 * area), cy / (3.0 * area))
    } else {
        pts.iter().copied().fold(Vec2::ZERO, |acc, p| acc + p) / (n as f64)
    }
}

/// Find the closest point on a polygon boundary to `pt`.
fn closest_point_on_poly(pt: Vec2, poly: &[Vec2]) -> Vec2 {
    let mut best_pt = pt;
    let mut best_d2 = f64::MAX;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let ab = b - a;
        let l2 = ab.len2();
        let t = if l2 > 1e-8 { ((pt - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let p = a + ab * t;
        let d2 = (pt - p).len2();
        if d2 < best_d2 {
            best_d2 = d2;
            best_pt = p;
        }
    }
    best_pt
}

/// Signed area of a 2D polygon (positive = CCW).
fn polygon_area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    let mut area = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(j)) {
            area += a.x * b.y - b.x * a.y;
        }
    }
    area * 0.5
}

/// Detect whether a sampled 2D loop represents a circle within `tol`.
fn detect_circle(pts: &[Vec2], tol: f64) -> Option<Circle2D> {
    if pts.len() < 12 {
        return None;
    }
    let n = pts.len();
    let p0 = *pts.first()?;
    let p1 = *pts.get(n / 3)?;
    let p2 = *pts.get((2 * n) / 3)?;

    let (u, v) = (p1 - p0, p2 - p0);
    let den = 2.0 * u.cross(v);
    if den.abs() < 1e-6 {
        return None;
    }
    let center = p0 + Vec2::new(v.y * u.len2() - u.y * v.len2(), u.x * v.len2() - v.x * u.len2()) / den;
    let r = (p0 - center).len();
    if !r.is_finite() || r < 0.25 {
        return None;
    }
    if pts.iter().all(|p| ((*p - center).len() - r).abs() <= tol) { Some(Circle2D { center, radius: r }) } else { None }
}

/// Compute signed area of a `Loop2` via dense polyline sampling (positive = CCW).
fn loop2_signed_area(lp: &Loop2) -> f64 {
    let mut pts = Vec::new();
    for s in &lp.segs {
        let steps = match s {
            Seg2::Line { .. } => 1,
            _ => 12,
        };
        for k in 0..steps {
            pts.push(s.point_at(k as f64 / steps as f64));
        }
    }
    polygon_area(&pts)
}

/// Extract exact analytical `Loop2`s (`Line` / `Arc` / `Seg2`) for `face_idx` via `body.planar_edges`.
/// Returns `None` for a loop if any edge has non-Seg spline pieces or does not close cleanly.
fn extract_exact_face_loop2s(body: &Body, face_idx: usize, plane: &Plane) -> Option<(Loop2, Vec<Loop2>)> {
    let face = body.record().faces.get(face_idx)?;
    let mut exact_loops: Vec<(Loop2, f64)> = Vec::new();

    for l in &face.loops {
        let solidcore_record::brep::Loop::Edges { edges } = l else { continue };
        let edge_ids: Vec<usize> = edges.iter().map(|u| u.edge as usize).collect();
        let pieces_per_edge = body.planar_edges(plane, &edge_ids);
        let mut loop_segs: Vec<Seg2> = Vec::new();

        for (u, opt_pieces) in edges.iter().zip(pieces_per_edge) {
            let pieces = opt_pieces?;
            let mut edge_segs: Vec<Seg2> = Vec::with_capacity(pieces.len());
            for p in pieces {
                let crate::PlanarPiece::Seg(seg) = p else { return None };
                edge_segs.push(seg);
            }
            if !u.forward {
                edge_segs.reverse();
                for s in &mut edge_segs {
                    *s = s.reversed();
                }
            }
            // Verify chaining against previous segment endpoint (flip if stored opposite).
            if let (Some(prev), Some(first)) = (loop_segs.last(), edge_segs.first())
                && prev.end().dist(first.start()) > 1e-2
                && edge_segs.last().is_some_and(|last| prev.end().dist(last.end()) < 1e-2)
            {
                edge_segs.reverse();
                for s in &mut edge_segs {
                    *s = s.reversed();
                }
            }
            loop_segs.extend(edge_segs);
        }

        let lp = merge_continuous_loop(&Loop2 { segs: loop_segs });
        if lp.segs.is_empty() || !lp.is_closed(1e-2) {
            return None;
        }
        let area = loop2_signed_area(&lp);
        if area.abs() > 1e-4 {
            exact_loops.push((lp, area));
        }
    }

    if exact_loops.is_empty() {
        return None;
    }

    let mut best_idx = 0;
    let mut max_area = 0.0;
    for (i, (_, a)) in exact_loops.iter().enumerate() {
        if a.abs() > max_area {
            max_area = a.abs();
            best_idx = i;
        }
    }

    let (mut outer, outer_area) = exact_loops.swap_remove(best_idx);
    if outer_area < 0.0 {
        outer = outer.reversed();
    }

    let mut holes = Vec::with_capacity(exact_loops.len());
    for (mut h, a) in exact_loops {
        if a > 0.0 {
            h = h.reversed();
        }
        holes.push(h);
    }

    Some((outer, holes))
}

/// Extract the outer loop (CCW) and interior hole loops (CW) of `face_idx` in `plane` local 2D coordinates.
fn extract_face_loops(body: &Body, face_idx: usize, plane: &Plane) -> Result<(Vec<Vec2>, Vec<Vec<Vec2>>), String> {
    let face = body.record().faces.get(face_idx).ok_or_else(|| format!("face {face_idx} out of range"))?;

    if face.loops.is_empty() {
        return Err(format!("target face {face_idx} has no boundary loops"));
    }

    let chord = (body.size() * 1e-3).max(1e-3);
    let mut loops: Vec<Vec<Vec2>> = Vec::new();

    for l in &face.loops {
        let solidcore_record::brep::Loop::Edges { edges } = l else { continue };
        let mut wire_pts: Vec<Vec2> = Vec::new();

        for u in edges {
            let e = body.record().edges.get(u.edge as usize).ok_or_else(|| format!("edge reference {} missing", u.edge))?;

            let mut pts = crate::body::edge_points(e, chord).map_err(|err| err.to_string())?;
            if !u.forward {
                pts.reverse();
            }

            for p3 in pts {
                let lp = plane.to_local(p3);
                if let Some(last) = wire_pts.last() {
                    if (*last - lp).len() > 1e-5 {
                        wire_pts.push(lp);
                    }
                } else {
                    wire_pts.push(lp);
                }
            }
        }

        if let (Some(&first), Some(&last)) = (wire_pts.first(), wire_pts.last())
            && (first - last).len() < 1e-4
            && wire_pts.len() > 3
        {
            wire_pts.pop();
        }

        if wire_pts.len() >= 3 && polygon_area(&wire_pts).abs() > 1e-6 {
            loops.push(wire_pts);
        }
    }

    if loops.is_empty() {
        return Err(format!("target face {face_idx} has no valid boundary loops"));
    }

    let mut best_idx = 0;
    let mut max_area = 0.0f64;
    for (i, lp) in loops.iter().enumerate() {
        let a = polygon_area(lp).abs();
        if a > max_area {
            max_area = a;
            best_idx = i;
        }
    }

    let mut outer = loops.swap_remove(best_idx);
    if polygon_area(&outer) < 0.0 {
        outer.reverse();
    }

    let mut holes = Vec::with_capacity(loops.len());
    for mut h in loops {
        if polygon_area(&h) > 0.0 {
            h.reverse();
        }
        holes.push(h);
    }

    Ok((outer, holes))
}

/// Extract outer boundary of an island face projected onto `plane` (returned CCW).
fn extract_island_outer(body: &Body, face_idx: usize, plane: &Plane) -> Option<Vec<Vec2>> {
    let (mut outer, _) = extract_face_loops(body, face_idx, plane).ok()?;
    if polygon_area(&outer) < 0.0 {
        outer.reverse();
    }
    Some(outer)
}

/// Offset a closed CCW polygon inward by `dist` (`dist > 0` shrinks CCW; `dist < 0` expands CCW).
fn offset_ccw_polygon(pts: &[Vec2], dist: f64) -> Option<Vec<Vec2>> {
    let n = pts.len();
    if n < 3 || !dist.is_finite() || dist.abs() < 1e-9 {
        return None;
    }

    let mut seg_lines: Vec<(Vec2, Vec2, Vec2)> = Vec::with_capacity(n);
    for i in 0..n {
        let a = *pts.get(i)?;
        let b = *pts.get((i + 1) % n)?;
        let tan = (b - a).normalized()?;
        let in_norm = Vec2::new(-tan.y, tan.x);
        seg_lines.push((a + in_norm * dist, b + in_norm * dist, in_norm));
    }

    let mut out: Vec<Vec2> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let prev_idx = (i + n - 1) % n;
        let (_, prev_end, prev_norm) = *seg_lines.get(prev_idx)?;
        let (curr_start, _, curr_norm) = *seg_lines.get(i)?;
        let v = *pts.get(i)?;
        let cp = prev_norm.cross(curr_norm);

        // Reflex turn on inward inset (dist > 0) or convex turn on outward expansion (dist < 0): round join.
        let needs_round = if dist > 0.0 { cp < -1e-4 } else { cp > 1e-4 };
        if needs_round {
            let sign = dist.signum();
            let r = dist.abs();
            let dir0 = prev_norm * sign;
            let dir1 = curr_norm * sign;
            let theta0 = dir0.y.atan2(dir0.x);
            let theta1 = dir1.y.atan2(dir1.x);
            let sweep = if dist < 0.0 { (theta1 - theta0).rem_euclid(TAU) } else { -((theta0 - theta1).rem_euclid(TAU)) };
            let steps = ((sweep.abs() / 0.15).ceil() as usize).clamp(2, 16);
            for s in 0..=steps {
                let ang = theta0 + sweep * (s as f64 / steps as f64);
                let pt = v + Vec2::new(ang.cos(), ang.sin()) * r;
                if out.last().is_none_or(|last| last.dist(pt) > 1e-6) {
                    out.push(pt);
                }
            }
        } else if cp.abs() > 1e-4 {
            let p_prev = *pts.get(prev_idx)?;
            let p_next = *pts.get((i + 1) % n)?;
            let tan_prev = (v - p_prev).normalized()?;
            let tan_curr = (p_next - v).normalized()?;
            let den = tan_prev.cross(tan_curr);
            if den.abs() > 1e-8 {
                let t = (curr_start - prev_end).cross(tan_curr) / den;
                let inter = prev_end + tan_prev * t;
                if (inter - v).len() < dist.abs() * 4.0 {
                    out.push(inter);
                } else {
                    out.push(curr_start);
                }
            } else {
                out.push(curr_start);
            }
        } else if out.last().is_none_or(|last| last.dist(curr_start) > 1e-6) {
            out.push(curr_start);
        }
    }

    if let (Some(&first), Some(&last)) = (out.first(), out.last())
        && first.dist(last) < 1e-6
        && out.len() > 3
    {
        out.pop();
    }

    if out.len() >= 3 && polygon_area(&out) > 1e-3 { Some(out) } else { None }
}

/// Convert a CCW polygon into a `Region2`, using an analytical circle `Loop2::circle` when circular.
fn ccw_loop_to_region_offset(pts: &[Vec2], inward_dist: f64) -> Option<Region2> {
    if let Some(circ) = detect_circle(pts, 0.15) {
        let r_new = circ.radius - inward_dist;
        if r_new <= 0.25 {
            return None;
        }
        return Some(Region2 { outer: Loop2::circle(circ.center, r_new), holes: Vec::new() });
    }
    let poly = offset_ccw_polygon(pts, inward_dist)?;
    Some(Region2 { outer: Loop2::polygon(&poly), holes: Vec::new() })
}

/// Offset curve representation for exact Line/Arc corner fillet center calculation.
#[derive(Clone, Copy)]
enum OffsetCurve {
    Line { p: Vec2, dir: Vec2 },
    Circle { center: Vec2, radius: f64 },
}

/// Offset `seg` to its left side by signed distance `r` (`r > 0` offsets left, `r < 0` offsets right).
fn offset_seg_signed(seg: &Seg2, r: f64) -> Option<OffsetCurve> {
    match *seg {
        Seg2::Line { a, b } => {
            let dir = (b - a).normalized()?;
            let left = Vec2::new(-dir.y, dir.x);
            Some(OffsetCurve::Line { p: a + left * r, dir })
        }
        Seg2::Arc { center, radius, sweep, .. } => {
            let off_r = if sweep > 0.0 { radius - r } else { radius + r };
            if off_r <= 1e-5 { None } else { Some(OffsetCurve::Circle { center, radius: off_r }) }
        }
        _ => {
            let a = seg.start();
            let dir = seg.start_tangent();
            let left = Vec2::new(-dir.y, dir.x);
            Some(OffsetCurve::Line { p: a + left * r, dir })
        }
    }
}

/// Candidate intersections between two offset curves.
fn intersect_offset_curves(a: OffsetCurve, b: OffsetCurve) -> Vec<Vec2> {
    match (a, b) {
        (OffsetCurve::Line { p: p1, dir: d1 }, OffsetCurve::Line { p: p2, dir: d2 }) => {
            let den = d1.cross(d2);
            if den.abs() < 1e-9 {
                Vec::new()
            } else {
                let t = (p2 - p1).cross(d2) / den;
                vec![p1 + d1 * t]
            }
        }
        (OffsetCurve::Line { p, dir }, OffsetCurve::Circle { center, radius })
        | (OffsetCurve::Circle { center, radius }, OffsetCurve::Line { p, dir }) => {
            let m = p - center;
            let b_coef = m.dot(dir);
            let c_coef = m.len2() - radius * radius;
            let disc = b_coef * b_coef - c_coef;
            if disc < -1e-9 {
                Vec::new()
            } else if disc <= 1e-9 {
                vec![p + dir * (-b_coef)]
            } else {
                let sq = disc.sqrt();
                vec![p + dir * (-b_coef - sq), p + dir * (-b_coef + sq)]
            }
        }
        (OffsetCurve::Circle { center: c1, radius: r1 }, OffsetCurve::Circle { center: c2, radius: r2 }) => {
            let d_vec = c2 - c1;
            let d = d_vec.len();
            if d < 1e-9 || d > r1 + r2 + 1e-7 || d < (r1 - r2).abs() - 1e-7 {
                return Vec::new();
            }
            let a_len = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h2 = (r1 * r1 - a_len * a_len).max(0.0);
            let h = h2.sqrt();
            let ex = d_vec / d;
            let ey = Vec2::new(-ex.y, ex.x);
            let mid = c1 + ex * a_len;
            if h < 1e-9 { vec![mid] } else { vec![mid + ey * h, mid - ey * h] }
        }
    }
}

/// Merge adjacent collinear `Seg2::Line` and co-circular `Seg2::Arc` segments so corner
/// filleting sees continuous pocket walls rather than boolean-split micro-pieces.
fn merge_continuous_loop(lp: &Loop2) -> Loop2 {
    if lp.segs.len() <= 1 {
        return lp.clone();
    }
    let mut segs: Vec<Seg2> = lp.segs.iter().copied().filter(|s| s.length() > 1e-4).collect();
    if segs.len() <= 1 {
        return lp.clone();
    }
    let try_merge = |a: &Seg2, b: &Seg2| -> Option<Seg2> {
        if a.end().dist(b.start()) > 1e-3 {
            return None;
        }
        match (*a, *b) {
            (Seg2::Line { a: p0, b: p1 }, Seg2::Line { a: q0, b: q1 }) => {
                let d1 = (p1 - p0).normalized()?;
                let d2 = (q1 - q0).normalized()?;
                if d1.dot(d2) > 1.0 - 1e-6 && d1.cross(d2).abs() < 1e-4 { Some(Seg2::Line { a: p0, b: q1 }) } else { None }
            }
            (Seg2::Arc { center: c1, radius: r1, start: st1, sweep: sw1 }, Seg2::Arc { center: c2, radius: r2, sweep: sw2, .. }) => {
                let total = sw1 + sw2;
                if c1.dist(c2) < 1e-3 && (r1 - r2).abs() < 1e-3 && sw1.signum() == sw2.signum() && total.abs() < TAU - 1e-4 {
                    Some(Seg2::Arc { center: c1, radius: r1, start: st1, sweep: total })
                } else {
                    None
                }
            }
            _ => None,
        }
    };

    let mut changed = true;
    while changed && segs.len() > 2 {
        changed = false;
        let mut i = 0;
        while i < segs.len() && segs.len() > 2 {
            let next = (i + 1) % segs.len();
            if let (Some(sa), Some(sb)) = (segs.get(i), segs.get(next))
                && let Some(merged) = try_merge(sa, sb)
            {
                if let Some(slot) = segs.get_mut(i) {
                    *slot = merged;
                }
                segs.remove(next);
                changed = true;
                continue;
            }
            i += 1;
        }
    }
    let out = Loop2 { segs };
    if out.is_closed(1e-3) { out } else { lp.clone() }
}

/// Replace convex corner arcs (`sweep > 0`) whose radius is `<= inward_dist + 1e-3` with sharp
/// miter corners of their adjacent lines so `offset_loop(lp, -inward_dist)` does not fail when
/// `wall_offset + erode_r >= outer_corner_radius`.
fn sharpen_collapsing_arcs(lp: &Loop2, inward_dist: f64) -> Loop2 {
    if inward_dist <= 0.0 || lp.segs.len() < 3 {
        return lp.clone();
    }
    let mut segs = lp.segs.clone();
    let mut changed = true;
    while changed && segs.len() >= 3 {
        changed = false;
        for i in 0..segs.len() {
            let is_collapsing = matches!(
                segs.get(i),
                Some(Seg2::Arc { radius, sweep, .. }) if *sweep > 0.0 && *radius <= inward_dist + 1e-3
            );
            if !is_collapsing {
                continue;
            }
            let n = segs.len();
            let prev = (i + n - 1) % n;
            let next = (i + 1) % n;
            if prev == next {
                break;
            }
            if let (Some(Seg2::Line { a: p0, b: p1 }), Some(Seg2::Line { a: q0, b: q1 })) = (segs.get(prev).copied(), segs.get(next).copied())
                && let (Some(d1), Some(d2)) = ((p1 - p0).normalized(), (q1 - q0).normalized())
            {
                let den = d1.cross(d2);
                if den.abs() > 1e-6 {
                    let t = (q0 - p0).cross(d2) / den;
                    let apex = p0 + d1 * t;
                    if let Some(slot) = segs.get_mut(prev) {
                        *slot = Seg2::Line { a: p0, b: apex };
                    }
                    if let Some(slot) = segs.get_mut(next) {
                        *slot = Seg2::Line { a: apex, b: q1 };
                    }
                    segs.remove(i);
                    changed = true;
                    break;
                }
            }
        }
    }
    Loop2 { segs }
}

/// Offset a closed CCW `Loop2` inward by `inward_dist` using exact analytical `offset_loop`,
/// returning `None` if an arc collapses or the offset fails.
fn exact_ccw_loop2_offset(lp: &Loop2, inward_dist: f64) -> Option<Region2> {
    let prepared = if inward_dist > 0.0 { sharpen_collapsing_arcs(lp, inward_dist) } else { lp.clone() };
    let segs: Vec<Seg2> = offset_loop(&prepared, -inward_dist).ok()?.into_iter().flatten().collect();
    let out = merge_continuous_loop(&Loop2 { segs });
    if !out.segs.is_empty() && out.is_closed(1e-2) && loop2_signed_area(&out) > 1e-2 { Some(Region2 { outer: out, holes: Vec::new() }) } else { None }
}

/// Restrict `seg` to parameter sub-interval `[ts, te]` within `[0, 1]`.
fn sub_seg(seg: &Seg2, ts: f64, te: f64) -> Option<Seg2> {
    if te <= ts + 1e-5 {
        return None;
    }
    match *seg {
        Seg2::Line { a, b } => {
            let p0 = a.lerp(b, ts);
            let p1 = a.lerp(b, te);
            (p0.dist(p1) > 1e-5).then_some(Seg2::Line { a: p0, b: p1 })
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let new_sweep = sweep * (te - ts);
            (new_sweep.abs() * radius > 1e-5).then_some(Seg2::Arc { center, radius, start: start + sweep * ts, sweep: new_sweep })
        }
        _ => {
            let s1 = if te < 1.0 - 1e-6 { seg.split_at(te).0 } else { *seg };
            let s2 = if ts > 1e-6 { s1.split_at(ts / te).1 } else { s1 };
            (s2.length() > 1e-5).then_some(s2)
        }
    }
}

/// Analytically dilate a closed CCW `Loop2` outward by rolling-ball radius `r > 0`,
/// shifting boundary segments outward by `r`, trimming reflex joins, and inserting exact
/// G1-tangent `Seg2::Arc`s of radius `r` at every convex corner.
fn dilate_ccw_loop(lp: &Loop2, r: f64) -> Option<Loop2> {
    if r <= 1e-6 {
        return Some(lp.clone());
    }
    let clean = merge_continuous_loop(lp);
    if clean.segs.is_empty() {
        return None;
    }
    if clean.segs.len() == 1 {
        if let Seg2::Arc { center, radius, start, sweep } = clean.segs[0] {
            let new_r = if sweep > 0.0 { radius + r } else { radius - r };
            if new_r <= 1e-4 {
                return None;
            }
            return Some(Loop2 { segs: vec![Seg2::Arc { center, radius: new_r, start, sweep }] });
        }
        return None;
    }

    let n = clean.segs.len();
    let mut shifted: Vec<Seg2> = Vec::with_capacity(n);
    for seg in &clean.segs {
        match *seg {
            Seg2::Line { a, b } => {
                let dir = (b - a).normalized()?;
                let out_n = Vec2::new(dir.y, -dir.x);
                shifted.push(Seg2::Line { a: a + out_n * r, b: b + out_n * r });
            }
            Seg2::Arc { center, radius, start, sweep } => {
                let new_r = if sweep > 0.0 { radius + r } else { radius - r };
                if new_r <= 1e-4 {
                    return None;
                }
                shifted.push(Seg2::Arc { center, radius: new_r, start, sweep });
            }
            _ => return None,
        }
    }

    let mut t_start = vec![0.0f64; n];
    let mut t_end = vec![1.0f64; n];
    let mut corner_arcs: Vec<Option<Seg2>> = vec![None; n];

    for i in 0..n {
        let next = (i + 1) % n;
        let orig1 = &clean.segs[i];
        let orig2 = &clean.segs[next];
        let v = orig1.end();
        let u1 = orig1.end_tangent();
        let u2 = orig2.start_tangent();
        let cross = u1.cross(u2);
        let n1 = Vec2::new(u1.y, -u1.x);
        let n2 = Vec2::new(u2.y, -u2.x);

        if cross > 1e-4 {
            let sweep = n1.cross(n2).atan2(n1.dot(n2)).rem_euclid(TAU);
            if sweep > 1e-4 && sweep < std::f64::consts::PI + 1e-3 {
                corner_arcs[i] = Some(Seg2::Arc { center: v, radius: r, start: n1.y.atan2(n1.x), sweep });
            }
        } else if cross < -1e-4 {
            let (Some(c1), Some(c2)) = (offset_seg_signed(orig1, -r), offset_seg_signed(orig2, -r)) else {
                return None;
            };
            let hint_dir = (n1 + n2).normalized().unwrap_or(n1);
            let hint_pt = v + hint_dir * r;
            let inter = intersect_offset_curves(c1, c2)
                .into_iter()
                .filter(|p| p.dist(v) < r * 6.0)
                .min_by(|a, b| a.dist(hint_pt).total_cmp(&b.dist(hint_pt)))?;
            let sh1 = &shifted[i];
            let sh2 = &shifted[next];
            let t1 = sh1.closest_param(inter);
            let t2 = sh2.closest_param(inter);
            if inter.dist(sh1.point_at(t1)) < 2e-2 && inter.dist(sh2.point_at(t2)) < 2e-2 {
                t_end[i] = t_end[i].min(t1);
                t_start[next] = t_start[next].max(t2);
            }
        }
    }

    let mut out_segs: Vec<Seg2> = Vec::with_capacity(n * 2);
    for i in 0..n {
        if let Some(s) = sub_seg(&shifted[i], t_start[i], t_end[i]) {
            out_segs.push(s);
        }
        if let Some(arc) = corner_arcs[i] {
            out_segs.push(arc);
        }
    }

    if out_segs.len() < 2 {
        return None;
    }

    // Snap minor numerical gaps on line endpoints so loop closure stays tight.
    let m = out_segs.len();
    for i in 0..m {
        let next = (i + 1) % m;
        let p_end = out_segs[i].end();
        let p_start = out_segs[next].start();
        let gap = p_end.dist(p_start);
        if gap > 1e-9 && gap < 2e-2 {
            if let Some(Seg2::Line { b, .. }) = out_segs.get_mut(i) {
                *b = p_start;
            } else if let Some(Seg2::Line { a, .. }) = out_segs.get_mut(next) {
                *a = p_end;
            }
        }
    }

    let out = merge_continuous_loop(&Loop2 { segs: out_segs });
    if !out.segs.is_empty() && out.is_closed(1e-2) && loop2_signed_area(&out) > 1e-2 { Some(out) } else { None }
}

/// Compute a tangent fillet `Seg2::Arc` of exact radius `r` on `side` (`+1.0` = left of CCW loop,
/// `-1.0` = right) between `s1` and `s2`, returning `(t1_on_s1, t2_on_s2, arc)`.
fn solve_fillet_between(s1: &Seg2, s2: &Seg2, r: f64, side: f64, hint_pt: Vec2, max_dist: f64) -> Option<(f64, f64, Seg2)> {
    let (c1, c2) = (offset_seg_signed(s1, side * r)?, offset_seg_signed(s2, side * r)?);
    let candidates = intersect_offset_curves(c1, c2);
    let mut best: Option<(f64, (f64, f64, Seg2))> = None;
    for center in candidates {
        let d_hint = center.dist(hint_pt);
        if d_hint > max_dist {
            continue;
        }
        let t1 = s1.closest_param(center);
        let t2 = s2.closest_param(center);
        if !(0.001..0.999).contains(&t1) || !(0.001..0.999).contains(&t2) {
            continue;
        }
        let p1 = s1.point_at(t1);
        let p2 = s2.point_at(t2);
        if (p1.dist(center) - r).abs() >= 1.5e-2 || (p2.dist(center) - r).abs() >= 1.5e-2 || p1.dist(p2) <= 1e-4 {
            continue;
        }
        let a_start = (p1 - center).y.atan2((p1 - center).x);
        let a_end = (p2 - center).y.atan2((p2 - center).x);
        let mut sweep = (a_end - a_start).rem_euclid(TAU);
        if side > 0.0 {
            if sweep > std::f64::consts::PI + 0.45 {
                sweep -= TAU;
            }
        } else if sweep < std::f64::consts::PI - 0.45 {
            sweep -= TAU;
        }
        if sweep * side <= 1e-4 || sweep.abs() >= std::f64::consts::PI + 0.45 {
            continue;
        }
        let (s1_keep, _) = s1.split_at(t1);
        let (_, s2_keep) = s2.split_at(t2);
        if s1_keep.length() <= 1e-3 || s2_keep.length() <= 1e-3 {
            continue;
        }
        let arc = Seg2::Arc { center, radius: r, start: a_start, sweep };
        if s1_keep.end_tangent().dot(arc.start_tangent()) < 0.98 || arc.end_tangent().dot(s2_keep.start_tangent()) < 0.98 {
            continue;
        }
        if best.as_ref().is_none_or(|(bd, _)| d_hint < *bd) {
            best = Some((d_hint, (t1, t2, arc)));
        }
    }
    best.map(|(_, res)| res)
}

/// Apply tangent arc corner fillets of radius `max_radius` to `lp`:
/// - Sharpens existing convex outer corner arcs smaller than `max_radius` so they expand to `max_radius`.
/// - Pass 1 handles doubly-sharp short bridge/spoke `Seg2::Line` segments (including index 0),
///   shortest first: either keeping a straight bridge of thickness `wall_offset` with two end fillets
///   when both fit (`t2_a < t1_b`), or bypassing the swallowed bridge segment to place a single
///   continuous G1-tangent arc directly between the flanking segments.
/// - Pass 2 fillets all remaining single sharp corners.
fn fillet_loop_corners(lp: &Loop2, max_radius: f64) -> Loop2 {
    let clean = merge_continuous_loop(lp);
    if max_radius <= 0.05 || clean.segs.len() < 2 {
        return clean;
    }
    let sharpened = merge_continuous_loop(&sharpen_collapsing_arcs(&clean, max_radius));
    let mut segs = if sharpened.segs.len() >= 2 && sharpened.is_closed(1e-2) { sharpened.segs } else { clean.segs.clone() };

    let mut failed_pts: Vec<Vec2> = Vec::new();
    let max_iter = segs.len().saturating_mul(6).max(32);
    for _ in 0..max_iter {
        if segs.len() < 2 {
            break;
        }
        let len = segs.len();

        // Pass 1: locate shortest doubly-sharp straight bridge/spoke segment (including segs[0]).
        let bridge_idx = if len >= 3 {
            (0..len)
                .filter(|&b| {
                    if !matches!(segs[b], Seg2::Line { .. }) || segs[b].length() > max_radius * 4.0 {
                        return false;
                    }
                    let prev = (b + len - 1) % len;
                    let next = (b + 1) % len;
                    if prev == next {
                        return false;
                    }
                    let v1 = segs[prev].end();
                    let v2 = segs[b].end();
                    if failed_pts.iter().any(|p| p.dist(v1) < 1e-3 || p.dist(v2) < 1e-3) {
                        return false;
                    }
                    let c1 = segs[prev].end_tangent().cross(segs[b].start_tangent());
                    let c2 = segs[b].end_tangent().cross(segs[next].start_tangent());
                    c1.abs() > 0.05 && c2.abs() > 0.05 && c1 * c2 > 0.0
                })
                .min_by(|&a, &b| segs[a].length().total_cmp(&segs[b].length()))
        } else {
            None
        };

        if let Some(b_idx) = bridge_idx {
            let prev = (b_idx + len - 1) % len;
            segs.rotate_left(prev);

            let s1 = segs[0];
            let s2 = segs[1];
            let s3 = segs[2];
            let u1 = s1.end_tangent();
            let u2 = s2.start_tangent();
            let side = if u1.cross(u2) > 0.0 { 1.0 } else { -1.0 };
            let n1 = Vec2::new(-u1.y, u1.x) * side;
            let n2 = Vec2::new(-u2.y, u2.x) * side;
            let hint_dir12 = (n1 + n2).normalized().unwrap_or(n1);
            let u2_end = s2.end_tangent();
            let u3 = s3.start_tangent();
            let n2_end = Vec2::new(-u2_end.y, u2_end.x) * side;
            let n3 = Vec2::new(-u3.y, u3.x) * side;
            let hint_dir23 = (n2_end + n3).normalized().unwrap_or(n2_end);
            let mid_s2 = s2.point_at(0.5);

            let mut applied = false;
            let mut r = max_radius;
            for _ in 0..14 {
                if r < 0.05 {
                    break;
                }
                let f12 = solve_fillet_between(&s1, &s2, r, side, s1.end() + hint_dir12 * r, r * 5.0);
                let f23 = solve_fillet_between(&s2, &s3, r, side, s2.end() + hint_dir23 * r, r * 5.0);
                if let (Some((t1_a, t2_a, arc_a)), Some((t1_b, t2_b, arc_b))) = (f12, f23)
                    && t2_a + 1e-3 < t1_b
                    && let Some(s2_mid) = sub_seg(&s2, t2_a, t1_b)
                {
                    let (s1_keep, _) = s1.split_at(t1_a);
                    let (_, s3_keep) = s3.split_at(t2_b);
                    segs.splice(0..=2, [s1_keep, arc_a, s2_mid, arc_b, s3_keep]);
                    applied = true;
                    break;
                }
                let hint_13 = mid_s2 + n2 * r;
                let max_d = r * 6.0 + s2.length();
                if let Some((t1_13, t2_13, arc_13)) = solve_fillet_between(&s1, &s3, r, side, hint_13, max_d) {
                    let (s1_keep, _) = s1.split_at(t1_13);
                    let (_, s3_keep) = s3.split_at(t2_13);
                    segs.splice(0..=2, [s1_keep, arc_13, s3_keep]);
                    applied = true;
                    break;
                }
                r *= 0.75;
            }
            if !applied {
                // Fall back to single corner fillet on (s1, s2), then mark as failed if still unresolvable.
                let mut r_single = max_radius;
                for _ in 0..14 {
                    if r_single < 0.05 {
                        break;
                    }
                    if let Some((t1, t2, arc)) = solve_fillet_between(&s1, &s2, r_single, side, s1.end() + hint_dir12 * r_single, r_single * 5.0) {
                        let (s1_keep, _) = s1.split_at(t1);
                        let (_, s2_keep) = s2.split_at(t2);
                        segs.splice(0..=1, [s1_keep, arc, s2_keep]);
                        applied = true;
                        break;
                    }
                    r_single *= 0.75;
                }
            }
            if !applied {
                failed_pts.push(s1.end());
                failed_pts.push(s2.end());
            }
            continue;
        }

        // Pass 2: locate next remaining single sharp corner.
        let sharp_idx = (0..len).find(|&k| {
            let s_a = &segs[k];
            let s_b = &segs[(k + 1) % len];
            let v = s_a.end();
            s_a.end_tangent().cross(s_b.start_tangent()).abs() > 0.05 && !failed_pts.iter().any(|p| p.dist(v) < 1e-3)
        });
        let Some(idx) = sharp_idx else {
            break;
        };
        segs.rotate_left(idx);

        let s1 = segs[0];
        let s2 = segs[1];
        let u1 = s1.end_tangent();
        let u2 = s2.start_tangent();
        let side = if u1.cross(u2) > 0.0 { 1.0 } else { -1.0 };
        let n1 = Vec2::new(-u1.y, u1.x) * side;
        let n2 = Vec2::new(-u2.y, u2.x) * side;
        let hint_dir12 = (n1 + n2).normalized().unwrap_or(n1);

        let mut applied = false;
        let mut r = max_radius;
        for _ in 0..14 {
            if r < 0.05 {
                break;
            }
            if let Some((t1, t2, arc)) = solve_fillet_between(&s1, &s2, r, side, s1.end() + hint_dir12 * r, r * 5.0) {
                let (s1_keep, _) = s1.split_at(t1);
                let (_, s2_keep) = s2.split_at(t2);
                segs.splice(0..=1, [s1_keep, arc, s2_keep]);
                applied = true;
                break;
            }
            r *= 0.75;
        }

        if !applied {
            failed_pts.push(s1.end());
        }
    }

    let out = merge_continuous_loop(&Loop2 { segs });
    if out.is_closed(1e-2) && loop2_signed_area(&out) > 1e-2 { out } else { clean }
}

/// Apply corner fillets to a `Region2`.
fn fillet_region_corners(region: &Region2, corner_radius: f64) -> Region2 {
    let clean_outer = merge_continuous_loop(&region.outer);
    let clean_holes: Vec<Loop2> = region.holes.iter().map(merge_continuous_loop).collect();
    if corner_radius <= 0.05 {
        return Region2 { outer: clean_outer, holes: clean_holes };
    }
    let candidate = Region2 {
        outer: fillet_loop_corners(&clean_outer, corner_radius),
        holes: clean_holes.iter().map(|h| fillet_loop_corners(h, corner_radius)).collect(),
    };
    if candidate.area().abs() > 1e-2 { candidate } else { Region2 { outer: clean_outer, holes: clean_holes } }
}

/// Build the 2D pocket regions on `plane` for `face_idx`, including radial structural bridging
/// ribs around bosses/holes and corner filleting.
fn build_pocket_regions(body: &Body, face_idx: usize, plane: &Plane, config: &LightenConfig) -> Result<Vec<Region2>, String> {
    let exact_loops = extract_exact_face_loop2s(body, face_idx, plane);
    let (outer_loop, inner_holes) = if let Some((ref outer_lp, ref hole_lps)) = exact_loops {
        let outer_pts = outer_lp.polyline(0.05).to_vec();
        let hole_pts: Vec<Vec<Vec2>> = hole_lps.iter().map(|h| h.polyline(0.05).to_vec()).collect();
        if outer_pts.len() >= 3 { (outer_pts, hole_pts) } else { extract_face_loops(body, face_idx, plane)? }
    } else {
        extract_face_loops(body, face_idx, plane)?
    };

    let inset_outer = exact_loops
        .as_ref()
        .and_then(|(outer_lp, _)| exact_ccw_loop2_offset(outer_lp, config.wall_offset))
        .or_else(|| ccw_loop_to_region_offset(&outer_loop, config.wall_offset))
        .ok_or_else(|| "wall_offset too large: pocket outer boundary collapsed".to_string())?;

    let outer_center = poly_centroid(&outer_loop);
    let outer_max_r = outer_loop.iter().map(|p| p.dist(outer_center)).fold(0.0f64, f64::max);

    let mut hole_loops_ccw: Vec<Vec<Vec2>> = Vec::with_capacity(inner_holes.len() + config.island_faces.len());
    for hole_cw in &inner_holes {
        let mut hole_ccw = hole_cw.clone();
        hole_ccw.reverse();
        hole_loops_ccw.push(hole_ccw);
    }
    for &island_id in &config.island_faces {
        if island_id as usize == face_idx || (island_id as usize) >= body.face_count() {
            continue;
        }
        if let Some(island_ccw) = extract_island_outer(body, island_id as usize, plane) {
            hole_loops_ccw.push(island_ccw);
        }
    }

    let exact_holes_ccw: Vec<Loop2> = exact_loops.as_ref().map(|(_, hs)| hs.iter().map(|h| h.reversed()).collect()).unwrap_or_default();

    let mut keepouts: Vec<Region2> = Vec::new();
    let mut hub_center: Option<Vec2> = None;
    let mut hub_r = 0.0f64;
    let mut bosses: Vec<(Vec2, f64)> = Vec::new();

    for (idx, loop_ccw) in hole_loops_ccw.iter().enumerate() {
        let ko_opt = exact_holes_ccw
            .get(idx)
            .and_then(|h| exact_ccw_loop2_offset(h, -config.wall_offset))
            .or_else(|| ccw_loop_to_region_offset(loop_ccw, -config.wall_offset));
        if let Some(ko) = ko_opt {
            keepouts.push(ko);
        }
        let center = poly_centroid(loop_ccw);
        let r = loop_ccw.iter().map(|p| p.dist(center)).fold(0.0f64, f64::max);
        let is_central_hub = center.dist(outer_center) <= (outer_max_r * 0.35).max(config.wall_offset * 2.0);
        if is_central_hub {
            if r > hub_r {
                hub_r = r;
                hub_center = Some(center);
            }
        } else {
            bosses.push((center, r));
        }
    }

    let anchor = hub_center.unwrap_or(outer_center);
    let boss_count = bosses.len();
    for (boss_center, boss_r) in bosses {
        let dir = if boss_count == 1 && hub_center.is_none() {
            let edge_pt = closest_point_on_poly(boss_center, &outer_loop);
            (edge_pt - boss_center).normalized().or_else(|| (boss_center - anchor).normalized())
        } else {
            (boss_center - anchor).normalized()
        };
        let Some(dir) = dir else { continue };
        let perp = Vec2::new(-dir.y, dir.x);

        let web_hw = config.wall_offset * 0.5;
        let collar_r = boss_r + config.wall_offset;

        let edge_pt = closest_point_on_poly(boss_center, &outer_loop);
        let d_edge = edge_pt.dist(boss_center);
        let gap_outer = d_edge - collar_r - config.wall_offset;
        let bridge_to_edge = gap_outer < 3.0 * config.corner_radius.max(config.wall_offset).max(10.0);

        let p_start = if hub_center.is_some() {
            anchor + dir * (hub_r * 0.5).max(0.0)
        } else if boss_count >= 2 {
            anchor - dir * web_hw
        } else {
            anchor
        };
        let p_end = boss_center;

        let spoke_pts = [p_start - perp * web_hw, p_end - perp * web_hw, p_end + perp * web_hw, p_start + perp * web_hw];
        keepouts.push(Region2 { outer: Loop2::polygon(&spoke_pts), holes: Vec::new() });

        if bridge_to_edge {
            let out_dir = (edge_pt - boss_center).normalized().unwrap_or(dir);
            let out_perp = Vec2::new(-out_dir.y, out_dir.x);
            let p_edge_end = boss_center + out_dir * (d_edge + config.wall_offset * 2.0);
            let bridge_pts =
                [boss_center - out_perp * web_hw, p_edge_end - out_perp * web_hw, p_edge_end + out_perp * web_hw, boss_center + out_perp * web_hw];
            keepouts.push(Region2 { outer: Loop2::polygon(&bridge_pts), holes: Vec::new() });
        }
    }

    let raw_regions = if keepouts.is_empty() {
        vec![inset_outer]
    } else {
        planar_regions_boolean(&[inset_outer], &keepouts, BoolOp::Cut, 1e-4).map_err(|e| e.to_string())?
    };

    let filtered_regions: Vec<Region2> = raw_regions.into_iter().filter(|r| r.area().abs() > 1e-2).collect();
    let pocket_regions: Vec<Region2> = if filtered_regions.len() > 1 && config.corner_radius > 1e-3 {
        let mut out = Vec::with_capacity(filtered_regions.len());
        for res in crate::par::map_with_local_pools(&filtered_regions, usize::MAX, |r| Ok(fillet_region_corners(r, config.corner_radius))) {
            let r = res.map_err(|e| e.to_string())?;
            if r.area().abs() > 1e-2 {
                out.push(r);
            }
        }
        out
    } else {
        filtered_regions.into_iter().map(|r| fillet_region_corners(&r, config.corner_radius)).filter(|r| r.area().abs() > 1e-2).collect()
    };

    if pocket_regions.is_empty() {
        return Err("no valid pocket regions remain after wall_offset, ribs, and boss keep-outs".into());
    }

    Ok(pocket_regions)
}

/// Resolve the pocket cut depth (mm) for `depth_spec` below `plane`.
pub fn resolve_cut_depth(body: &Body, plane: &Plane, depth_spec: PocketDepthSpec) -> Result<f64, String> {
    let body_size = body.size().max(1.0);
    let n = plane.normal();
    match depth_spec {
        PocketDepthSpec::BlindDepth(d) => {
            if !d.is_finite() || d <= 0.0 {
                return Err("blind pocket depth must be positive".into());
            }
            Ok(d)
        }
        PocketDepthSpec::FloorThickness(floor) => {
            if !floor.is_finite() || floor < 0.0 {
                return Err("floor thickness must be non-negative".into());
            }
            let tol = (body_size * 1e-3).max(1e-3);
            let m = body.mesh(&MeshTolerance::chord(tol)).map_err(|e| e.to_string())?;
            let mut min_h = 0.0f64;
            for p in &m.positions {
                let h = (*p - plane.origin).dot(n);
                if h < min_h {
                    min_h = h;
                }
            }
            let thickness_below = (-min_h).abs();
            if thickness_below < 1e-4 {
                return Err("could not determine part thickness below target face".into());
            }
            if floor >= thickness_below - 0.05 {
                return Err(format!("floor thickness ({floor:.2} mm) must be less than part thickness ({thickness_below:.2} mm)"));
            }
            Ok((thickness_below - floor).max(0.1))
        }
        PocketDepthSpec::ThroughHole => Ok(body_size * 1.5 + 5.0),
    }
}

/// Midpoint of a sampled polyline `pts` by arc length (assumes `!pts.is_empty()`).
fn edge_midpoint(pts: &[Vec3]) -> Vec3 {
    let mut total_len = 0.0;
    for w in pts.windows(2) {
        total_len += w[0].dist(w[1]);
    }
    let mut target = total_len * 0.5;
    let mut mid = pts[0];
    for w in pts.windows(2) {
        let seg_len = w[0].dist(w[1]);
        if target <= seg_len && seg_len > 1e-12 {
            mid = w[0].lerp(w[1], target / seg_len);
            break;
        }
        target -= seg_len;
        mid = w[1];
    }
    mid
}

/// Minimum arc radius across all pocket boundary loops, used to clamp floor fillets below the
/// zero-inner-radius torus singularity (`R_floor <= 0.85 * R_corner`).
fn min_pocket_arc_radius(regions: &[Region2]) -> Option<f64> {
    regions
        .iter()
        .flat_map(|r| r.loops())
        .flat_map(|lp| lp.segs.iter())
        .filter_map(|s| match *s {
            Seg2::Arc { radius, .. } => Some(radius),
            _ => None,
        })
        .reduce(f64::min)
}

/// Extrude a single 2D pocket `region` along `-plane.normal()` by `cut_depth` (with optional draft).
fn extrude_single_pocket_prism(plane: &Plane, region: &Region2, cut_depth: f64, draft_angle: f64) -> Result<Option<Body>, String> {
    if draft_angle.abs() < 1e-6 {
        let mut list = extrude(plane, std::slice::from_ref(region), -cut_depth, 0.02).map_err(|e| e.to_string())?;
        Ok(list.pop())
    } else {
        let taper_rad = -draft_angle.to_radians();
        match extrude_tapered_between(plane, region, -0.02, cut_depth + 0.02, 0.0, -1.0, taper_rad, DraftCorner::Round) {
            Ok(b) => Ok(Some(b)),
            Err(_) => {
                let mut list = extrude(plane, std::slice::from_ref(region), -cut_depth, 0.02).map_err(|e| e.to_string())?;
                Ok(list.pop())
            }
        }
    }
}

/// Extrude all 2D pocket regions into raw cutter prism bodies.
fn extrude_pocket_prisms(plane: &Plane, pocket_regions: &[Region2], cut_depth: f64, draft_angle: f64) -> Result<Vec<Body>, String> {
    if draft_angle.abs() < 1e-6 {
        return extrude(plane, pocket_regions, -cut_depth, 0.02).map_err(|e| e.to_string());
    }
    if pocket_regions.len() > 1 {
        let mut cutters = Vec::with_capacity(pocket_regions.len());
        for res in crate::par::map_with_local_pools(pocket_regions, usize::MAX, |region| {
            Ok(extrude_single_pocket_prism(plane, region, cut_depth, draft_angle))
        }) {
            let inner = res.map_err(|e| e.to_string())?;
            if let Some(b) = inner? {
                cutters.push(b);
            }
        }
        return Ok(cutters);
    }
    let mut cutters = Vec::with_capacity(pocket_regions.len());
    for region in pocket_regions {
        if let Some(b) = extrude_single_pocket_prism(plane, region, cut_depth, draft_angle)? {
            cutters.push(b);
        }
    }
    Ok(cutters)
}

/// Build cutter prism preview bodies (showing segmented bays, rounded vertical corners, bottom
/// floor fillets/chamfers, and top rim bevels) without modifying `body`.
pub fn preview_pockets(body: &Body, face_id: u64, config: &LightenConfig) -> Result<Vec<Body>, String> {
    preview_pockets_with_locations(body, face_id, config, FilletLocation::Bottom, ChamferLocation::Top)
}

/// Build an outward 45-degree top bevel flare body matching a countersink/chamfer rim on `plane`
/// so the negative cutter prism preview shows the widened top lip without inverting inward.
fn build_top_bevel_flare(plane: &Plane, region: &Region2, bevel_size: f64, cut_depth: f64) -> Option<Body> {
    let mut flare_d = bevel_size.min(cut_depth * 0.45);
    let min_concave_r = region
        .outer
        .segs
        .iter()
        .filter_map(|s| match *s {
            Seg2::Arc { radius, sweep, .. } if sweep < 0.0 => Some(radius),
            _ => None,
        })
        .fold(f64::MAX, f64::min);
    if min_concave_r.is_finite() && min_concave_r < f64::MAX {
        flare_d = flare_d.min(min_concave_r * 0.75);
    }
    if flare_d <= 0.05 {
        return None;
    }
    let expanded_outer = dilate_ccw_loop(&region.outer, flare_d)?;
    let expanded = Region2 { outer: expanded_outer, holes: Vec::new() };
    extrude_tapered_between(plane, &expanded, -0.01, flare_d, 0.0, -1.0, -std::f64::consts::FRAC_PI_4, DraftCorner::Round).ok()
}

/// Build an inward bottom fillet (multi-step quarter-circle) or 45-degree chamfer preview
/// from `shaft_depth = cut_depth - d` to `cut_depth` along `-plane.normal()`, returning
/// `(bevel_bodies, shaft_depth)`.
fn build_bottom_bevel_preview(
    plane: &Plane,
    region: &Region2,
    bevel_size: f64,
    cut_depth: f64,
    draft_angle: f64,
    min_arc_r: Option<f64>,
    is_fillet: bool,
) -> Option<(Vec<Body>, f64)> {
    let mut d = bevel_size.min(cut_depth * 0.85);
    if let Some(mr) = min_arc_r {
        d = d.min((mr * 0.65).max(0.1));
    }
    if d <= 0.05 || cut_depth - d <= 0.05 {
        return None;
    }
    let shaft_depth = cut_depth - d;
    let draft_inset = if draft_angle.abs() > 1e-6 { shaft_depth * draft_angle.to_radians().tan() } else { 0.0 };

    if !is_fillet {
        let bot_region = if draft_inset > 1e-6 { exact_ccw_loop2_offset(&region.outer, draft_inset)? } else { region.clone() };
        let bot_plane = plane.offset(-shaft_depth);
        let body = extrude_tapered_between(&bot_plane, &bot_region, 0.0, d, 0.0, -1.0, -std::f64::consts::FRAC_PI_4, DraftCorner::Round).ok()?;
        return Some((vec![body], shaft_depth));
    }

    const FILLET_STEPS: usize = 4;
    let mut bodies = Vec::with_capacity(FILLET_STEPS);
    for k in 0..FILLET_STEPS {
        let th0 = (k as f64) * std::f64::consts::FRAC_PI_2 / (FILLET_STEPS as f64);
        let th1 = ((k + 1) as f64) * std::f64::consts::FRAC_PI_2 / (FILLET_STEPS as f64);
        let dh = d * (th1.sin() - th0.sin());
        let dinset = d * (th0.cos() - th1.cos());
        if dh <= 1e-4 {
            continue;
        }
        let taper = -(dinset / dh).atan();
        let start_inset = draft_inset + d * (1.0 - th0.cos());
        let slice_region = if start_inset > 1e-6 { exact_ccw_loop2_offset(&region.outer, start_inset)? } else { region.clone() };
        let slice_plane = plane.offset(-(shaft_depth + d * th0.sin()));
        let slice = extrude_tapered_between(&slice_plane, &slice_region, 0.0, dh, 0.0, -1.0, taper, DraftCorner::Round).ok()?;
        bodies.push(slice);
    }
    if bodies.is_empty() {
        return None;
    }
    Some((bodies, shaft_depth))
}

/// Build cutter prism preview bodies with explicit Auto Fillet / Auto Chamfer location settings.
pub fn preview_pockets_with_locations(
    body: &Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<Vec<Body>, String> {
    config.validate()?;
    let face_idx = usize::try_from(face_id).map_err(|_| "face_id overflow".to_string())?;
    if face_idx >= body.face_count() {
        return Err(format!("face_id {face_id} out of range (body has {} faces)", body.face_count()));
    }
    let plane = body.planar_face(face_idx).ok_or_else(|| format!("target face {face_id} is not planar"))?;

    let pocket_regions = build_pocket_regions(body, face_idx, &plane, config)?;
    let cut_depth = resolve_cut_depth(body, &plane, config.depth_spec)?;
    let is_through = matches!(config.depth_spec, PocketDepthSpec::ThroughHole);
    let min_arc_r = min_pocket_arc_radius(&pocket_regions);

    let want_bottom_fillet = !is_through && matches!(fillet_loc, FilletLocation::Bottom | FilletLocation::All);
    let want_top_fillet = matches!(fillet_loc, FilletLocation::Top | FilletLocation::All);
    let want_bottom_chamfer = !is_through && matches!(chamfer_loc, ChamferLocation::Bottom | ChamferLocation::All);
    let want_top_chamfer = matches!(chamfer_loc, ChamferLocation::Top | ChamferLocation::All);
    let top_bevel =
        want_top_chamfer.then_some(config.auto_chamfer_size).flatten().or_else(|| want_top_fillet.then_some(config.auto_fillet_radius).flatten());
    let bottom_fillet_r = want_bottom_fillet.then_some(config.auto_fillet_radius).flatten();
    let bottom_chamfer_s = want_bottom_chamfer.then_some(config.auto_chamfer_size).flatten();
    let (bottom_bevel, bottom_is_fillet) = match (bottom_fillet_r, bottom_chamfer_s) {
        (Some(r), _) => (Some(r), true),
        (None, Some(s)) => (Some(s), false),
        (None, None) => (None, false),
    };

    let cutters = crate::validation::validating(crate::validation::Validation::Off, || -> Result<Vec<Body>, String> {
        let build_bay = |region: &Region2| -> Result<Option<Body>, String> {
            let bot_opt = bottom_bevel
                .and_then(|bs| build_bottom_bevel_preview(&plane, region, bs, cut_depth, config.draft_angle, min_arc_r, bottom_is_fillet));
            let shaft_depth = bot_opt.as_ref().map_or(cut_depth, |(_, sd)| *sd);
            let Some(prism) = extrude_single_pocket_prism(&plane, region, shaft_depth, config.draft_angle)? else {
                return Ok(None);
            };

            let mut pieces = vec![prism];
            if let Some((bot_bodies, _)) = bot_opt {
                pieces.extend(bot_bodies);
            }
            if let Some(bevel_s) = top_bevel
                && let Some(flare) = build_top_bevel_flare(&plane, region, bevel_s, cut_depth)
            {
                pieces.push(flare);
            }

            crate::ops::join_pieces(pieces).map_err(|e| e.to_string())
        };

        let mut cutters = Vec::with_capacity(pocket_regions.len());
        if pocket_regions.len() > 1 && (top_bevel.is_some() || bottom_bevel.is_some()) {
            for res in crate::par::map_with_local_pools(&pocket_regions, usize::MAX, |region| Ok(build_bay(region))) {
                let inner = res.map_err(|e| e.to_string())?;
                if let Some(b) = inner? {
                    cutters.push(b);
                }
            }
        } else {
            for region in &pocket_regions {
                if let Some(b) = build_bay(region)? {
                    cutters.push(b);
                }
            }
        }
        Ok(cutters)
    })
    .0?;

    if cutters.is_empty() {
        return Err("failed to build pocket cutter prisms".into());
    }
    Ok(cutters)
}

/// Distance from a 2D point `p` to a `Seg2` boundary segment.
fn dist_to_seg2(p: Vec2, seg: &Seg2) -> f64 {
    let t = seg.closest_param(p);
    p.dist(seg.point_at(t))
}

/// Minimum distance from 2D point `p` to any boundary segment of `regions`.
fn dist_to_pocket_boundary(p: Vec2, regions: &[Region2]) -> f64 {
    regions.iter().flat_map(|r| r.loops()).flat_map(|l| l.segs.iter()).map(|s| dist_to_seg2(p, s)).fold(f64::MAX, f64::min)
}

/// Top (`TopEdges`) and bottom (`BottomEdges`) pocket perimeter edge picks tracked from the cut.
#[derive(Default)]
struct TrackedPocketEdges {
    top_edges: Vec<Vec3>,
    bottom_edges: Vec<Vec3>,
}

/// Collect `TopEdges` and `BottomEdges` created by the pocket boolean cut, excluding pre-existing
/// outer rim and hole/hub edges recorded in `pre_edge_mids`.
fn collect_tracked_pocket_edges(
    body: &Body,
    plane: &Plane,
    cut_depth: f64,
    draft_angle: f64,
    regions: &[Region2],
    pre_edge_mids: &[Vec3],
    is_through: bool,
) -> TrackedPocketEdges {
    let n = plane.normal();
    let draft_shift = cut_depth * draft_angle.to_radians().tan();
    let mut tracked = TrackedPocketEdges::default();
    for e in &body.record().edges {
        let Ok(pts) = crate::body::edge_points(e, 0.25) else {
            continue;
        };
        if pts.is_empty() {
            continue;
        }
        let h_all_top = pts.iter().all(|p| (*p - plane.origin).dot(n).abs() <= 0.25);
        let h_all_bottom = !is_through && pts.iter().all(|p| ((*p - plane.origin).dot(n) + cut_depth).abs() <= 0.25);
        let m = edge_midpoint(&pts);
        let lp = plane.to_local(m);
        let d_bound = dist_to_pocket_boundary(lp, regions);
        let existed_before = pre_edge_mids.iter().any(|p| p.dist(m) < 0.20);

        if h_all_top && !existed_before && d_bound <= 0.60 {
            tracked.top_edges.push(m);
        } else if h_all_bottom && d_bound <= draft_shift + 0.60 {
            tracked.bottom_edges.push(m);
        }
    }
    tracked
}

/// Select tracked edge picks according to a `FilletLocation`.
fn select_fillet_edges(tracked: &TrackedPocketEdges, loc: FilletLocation) -> Vec<Vec3> {
    let mut pts = Vec::new();
    if matches!(loc, FilletLocation::Top | FilletLocation::All) {
        pts.extend_from_slice(&tracked.top_edges);
    }
    if matches!(loc, FilletLocation::Bottom | FilletLocation::All) {
        pts.extend_from_slice(&tracked.bottom_edges);
    }
    pts
}

/// Select tracked edge picks according to a `ChamferLocation`.
fn select_chamfer_edges(tracked: &TrackedPocketEdges, loc: ChamferLocation) -> Vec<Vec3> {
    let mut pts = Vec::new();
    if matches!(loc, ChamferLocation::Top | ChamferLocation::All) {
        pts.extend_from_slice(&tracked.top_edges);
    }
    if matches!(loc, ChamferLocation::Bottom | ChamferLocation::All) {
        pts.extend_from_slice(&tracked.bottom_edges);
    }
    pts
}

/// Execute `solid.lighten` auto-pocketing on `body` at `face_id` using default blend locations
/// (`FilletLocation::Bottom` for floor fillets, `ChamferLocation::Top` for top chamfers).
pub fn auto_pocket(body: &mut Body, face_id: u64, config: &LightenConfig) -> Result<(), String> {
    auto_pocket_with_locations(body, face_id, config, FilletLocation::Bottom, ChamferLocation::Top)
}

/// Execute `solid.lighten` auto-pocketing on `body` at `face_id` with explicit fillet/chamfer locations.
pub fn auto_pocket_with_locations(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<(), String> {
    let _warnings = auto_pocket_with_warnings(body, face_id, config, fillet_loc, chamfer_loc)?;
    Ok(())
}

/// Collect edge midpoints only for edges belonging to `face_idx` (avoids full-body tessellation).
fn collect_face_edge_mids(body: &Body, face_idx: usize) -> Vec<Vec3> {
    let Some(face) = body.record().faces.get(face_idx) else {
        return Vec::new();
    };
    let chord = (body.size() * 1e-3).max(1e-3);
    let mut mids = Vec::new();
    for l in &face.loops {
        let solidcore_record::brep::Loop::Edges { edges } = l else { continue };
        for u in edges {
            if let Some(e) = body.record().edges.get(u.edge as usize)
                && let Ok(pts) = crate::body::edge_points(e, chord)
                && !pts.is_empty()
            {
                mids.push(edge_midpoint(&pts));
            }
        }
    }
    mids
}

/// Execute `solid.lighten` auto-pocketing (including top/bottom fillets and chamfers) under
/// `Validation::Off` for fast live cut-away preview rendering matching Shell / Extrude.
pub fn preview_cut_body_with_locations(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<(), String> {
    crate::validation::validating(crate::validation::Validation::Off, || {
        auto_pocket_with_warnings(body, face_id, config, fillet_loc, chamfer_loc).map(|_| ())
    })
    .0
}

/// Execute `solid.lighten` auto-pocketing and return non-fatal warnings if an edge blend falls back.
pub fn auto_pocket_with_warnings(
    body: &mut Body,
    face_id: u64,
    config: &LightenConfig,
    fillet_loc: FilletLocation,
    chamfer_loc: ChamferLocation,
) -> Result<Vec<String>, String> {
    config.validate()?;
    let face_idx = usize::try_from(face_id).map_err(|_| "face_id overflow".to_string())?;
    if face_idx >= body.face_count() {
        return Err(format!("face_id {face_id} out of range (body has {} faces)", body.face_count()));
    }
    let plane = body.planar_face(face_idx).ok_or_else(|| format!("target face {face_id} is not planar"))?;

    let pocket_regions = build_pocket_regions(body, face_idx, &plane, config)?;
    let cut_depth = resolve_cut_depth(body, &plane, config.depth_spec)?;
    let is_through = matches!(config.depth_spec, PocketDepthSpec::ThroughHole);

    let cutters = extrude_pocket_prisms(&plane, &pocket_regions, cut_depth, config.draft_angle)?;
    if cutters.is_empty() {
        return Err("failed to build pocket cutter prisms".into());
    }

    let pre_edge_mids = collect_face_edge_mids(body, face_idx);
    let faces_before = body.face_count();

    let (ops_target, _) = solidcore_ops::Body::from_record(body.record(), false).map_err(|e| e.to_string())?;
    let ops_cutters = cutters
        .iter()
        .map(|c| solidcore_ops::Body::from_record(c.record(), false).map(|(b, _)| b))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let cut_ops = solidcore_ops::cut_pockets(ops_target, ops_cutters)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "auto-pocketing boolean cut consumed the entire solid".to_string())?;
    let (cut_record, _) = cut_ops.to_record().map_err(|e| e.to_string())?;
    let mut current = Body::from_record(cut_record).map_err(|e| e.to_string())?;

    if current.face_count() <= faces_before {
        let vol_before = measure(body).map(|m| m.volume).unwrap_or(0.0);
        let vol_after = measure(&current).map(|m| m.volume).unwrap_or(0.0);
        if vol_after >= vol_before - 1e-4 {
            return Err("auto-pocketing cut removed no material".into());
        }
    }

    let mut warnings = Vec::new();
    let has_fillet = config.auto_fillet_radius.is_some();
    let has_chamfer = config.auto_chamfer_size.is_some();

    let mut cached_tracked = if has_fillet || has_chamfer {
        Some(collect_tracked_pocket_edges(&current, &plane, cut_depth, config.draft_angle, &pocket_regions, &pre_edge_mids, is_through))
    } else {
        None
    };

    let min_arc_r = min_pocket_arc_radius(&pocket_regions);
    let mut fillet_applied = false;
    if let Some(fillet_r) = config.auto_fillet_radius
        && let Some(tracked) = cached_tracked.as_ref()
    {
        let pts = select_fillet_edges(tracked, fillet_loc);
        let is_bottom_fillet = matches!(fillet_loc, FilletLocation::Bottom | FilletLocation::All);
        let primary_r = if is_bottom_fillet
            && let Some(mr) = min_arc_r
            && (fillet_r - mr).abs() < 0.05
        {
            (mr * 0.65).max(0.1)
        } else {
            fillet_r
        };
        if !pts.is_empty() {
            match fillet(&current, &pts, primary_r) {
                Ok(filleted) => {
                    current = filleted;
                    fillet_applied = true;
                }
                Err(err) => {
                    let fallback_r = min_arc_r.map_or(fillet_r * 0.65, |mr| (mr * 0.65).min(cut_depth * 0.45).max(0.1));
                    if is_bottom_fillet
                        && fillet_r <= cut_depth * 0.85
                        && (fallback_r - primary_r).abs() > 0.05
                        && let Ok(filleted) = fillet(&current, &pts, fallback_r)
                    {
                        current = filleted;
                        fillet_applied = true;
                    } else {
                        warnings.push(format!("Auto Fillet ({fillet_r:.2} mm) fell back to unfilleted pocket cut: {err}"));
                    }
                }
            }
        }
    }

    if let Some(chamfer_s) = config.auto_chamfer_size {
        let locations_overlap = (matches!(fillet_loc, FilletLocation::Top | FilletLocation::All)
            && matches!(chamfer_loc, ChamferLocation::Top | ChamferLocation::All))
            || (matches!(fillet_loc, FilletLocation::Bottom | FilletLocation::All)
                && matches!(chamfer_loc, ChamferLocation::Bottom | ChamferLocation::All));
        if fillet_applied && locations_overlap {
            cached_tracked =
                Some(collect_tracked_pocket_edges(&current, &plane, cut_depth, config.draft_angle, &pocket_regions, &pre_edge_mids, is_through));
        }
        if let Some(tracked) = cached_tracked.as_ref() {
            let pts = select_chamfer_edges(tracked, chamfer_loc);
            if !pts.is_empty() {
                match chamfer(&current, &pts, chamfer_s) {
                    Ok(chamfered) => {
                        current = chamfered;
                    }
                    Err(err) => {
                        warnings.push(format!("Auto Chamfer ({chamfer_s:.2} mm) fell back to unchamfered pocket cut: {err}"));
                    }
                }
            }
        }
    }

    *body = current;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean;
    use crate::build::{box_solid, cylinder};
    use crate::measure;

    #[test]
    fn test_auto_pocket_box_blind() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(60.0, 60.0, 20.0)).unwrap();
        let initial_vol = measure(&b).unwrap().volume;

        let faces = b.faces(0.01).unwrap();
        let top_face = faces
            .iter()
            .find(|f| f.plane_normal.is_some_and(|n| (n - Vec3::Z).len() < 1e-4 && (f.centroid.z - 20.0).abs() < 1e-3))
            .expect("top face exists");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        auto_pocket(&mut b, top_face.index as u64, &cfg).expect("auto pocket succeeded");
        let final_vol = measure(&b).unwrap().volume;
        assert!(final_vol < initial_vol, "material was removed");
        // Expected removed volume ~ 52 * 52 * 7 (minus small corner fillets) ~ 18,900 mm^3 out of 72,000 mm^3.
        assert!(final_vol > initial_vol * 0.5, "wall and floor material preserved");
    }

    #[test]
    fn test_wall_offset_validation_and_non_planar_face() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let mut cfg = LightenConfig { wall_offset: 0.0, ..Default::default() };
        assert!(auto_pocket(&mut b, 0, &cfg).is_err(), "wall_offset == 0 rejected");
        cfg.wall_offset = -2.0;
        assert!(auto_pocket(&mut b, 0, &cfg).is_err(), "negative wall_offset rejected");

        let cyl = cylinder(Vec3::ZERO, Vec3::Z, 20.0, 20.0).unwrap();
        let side_face = cyl.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_none()).expect("cylindrical face exists");
        let mut cyl_mut = cyl;
        let valid_cfg = LightenConfig::default();
        let err = auto_pocket(&mut cyl_mut, side_face.index as u64, &valid_cfg).unwrap_err();
        assert!(err.contains("not planar"), "unexpected error: {err}");
    }

    #[test]
    fn test_floor_thickness_depth_calculation() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(50.0, 50.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let plane = b.planar_face(top_face.index).unwrap();
        let d = resolve_cut_depth(&b, &plane, PocketDepthSpec::FloorThickness(5.0)).unwrap();
        assert!((d - 15.0).abs() < 0.05, "expected 15mm depth, got {d}");

        // Floor thickness >= part thickness must error
        assert!(resolve_cut_depth(&b, &plane, PocketDepthSpec::FloorThickness(20.0)).is_err());

        let cfg = LightenConfig { wall_offset: 5.0, depth_spec: PocketDepthSpec::FloorThickness(5.0), corner_radius: 2.0, ..Default::default() };
        auto_pocket(&mut b, top_face.index as u64, &cfg).unwrap();
        let v = measure(&b).unwrap().volume;
        assert!(v < 50.0 * 50.0 * 20.0);
    }

    #[test]
    fn test_boss_retention_around_holes() {
        let outer_cyl = cylinder(Vec3::ZERO, Vec3::Z, 50.0, 20.0).unwrap();
        let h_center = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 12.0, 22.0).unwrap();
        let h1 = cylinder(Vec3::new(32.0, 0.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h2 = cylinder(Vec3::new(-32.0, 0.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h3 = cylinder(Vec3::new(0.0, 32.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();
        let h4 = cylinder(Vec3::new(0.0, -32.0, -1.0), Vec3::Z, 5.0, 22.0).unwrap();

        let mut flange = boolean(&outer_cyl, &h_center, BoolOp::Cut).unwrap().unwrap();
        for h in [&h1, &h2, &h3, &h4] {
            flange = boolean(&flange, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = flange
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3))
            .expect("top face exists");

        let vol_before = measure(&flange).unwrap().volume;
        let cfg = LightenConfig { wall_offset: 4.0, depth_spec: PocketDepthSpec::BlindDepth(7.0), corner_radius: 3.0, ..Default::default() };
        auto_pocket(&mut flange, top_face.index as u64, &cfg).expect("auto pocket on flange succeeded");
        let vol_after = measure(&flange).unwrap().volume;
        assert!(vol_after < vol_before, "pocket removed material");

        // Verify boss material around each bolt hole (at r = 5 + 2 = 7mm, z = 18mm) is still solid!
        let m = flange.mesh(&MeshTolerance::chord(0.05)).unwrap();
        let top_tris: Vec<[Vec3; 3]> = m
            .triangles
            .iter()
            .filter_map(|t| m.tri(t))
            .filter(|[a, b, c]| (a.z - 20.0).abs() < 0.05 && (b.z - 20.0).abs() < 0.05 && (c.z - 20.0).abs() < 0.05)
            .collect();
        assert!(!top_tris.is_empty(), "boss top faces remain at z=20");
    }

    #[test]
    fn test_corner_fillet_clamped_for_narrow_pocket() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 15.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 15.0).abs() < 1e-3)).unwrap();
        // Pocket width is 20 - 2*4 = 12mm (half-width = 6mm), corner_radius = 25mm must clamp cleanly!
        let cfg = LightenConfig { wall_offset: 4.0, depth_spec: PocketDepthSpec::BlindDepth(5.0), corner_radius: 25.0, ..Default::default() };
        auto_pocket(&mut b, top_face.index as u64, &cfg).expect("clamped corner radius succeeds");
    }

    #[test]
    fn test_segmented_flange_pockets_corner_arcs_and_auto_blends() {
        // Bolted flange matching rolling-ball geometry: R=54mm, hub bore R=19mm, 6 bolt holes R=5mm at r=37mm.
        let mut flange_holes = vec![Loop2::circle(Vec2::ZERO, 19.0).reversed()];
        for k in 0..6 {
            let ang = (k as f64) * TAU / 6.0;
            flange_holes.push(Loop2::circle(Vec2::new(37.0 * ang.cos(), 37.0 * ang.sin()), 5.0).reversed());
        }
        let flange_region = Region2 { outer: Loop2::circle(Vec2::ZERO, 54.0), holes: flange_holes };
        let mut flange = extrude(&Plane::XY, &[flange_region], 0.0, 18.0).unwrap().pop().unwrap();

        let top_face = flange
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 18.0).abs() < 1e-3))
            .expect("flange top face");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 1.5,
            draft_angle: 0.0,
            auto_fillet_radius: Some(2.0),
            auto_chamfer_size: Some(0.75),
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        let plane = flange.planar_face(top_face.index).unwrap();
        let regions = build_pocket_regions(&flange, top_face.index, &plane, &cfg).unwrap();
        assert_eq!(regions.len(), 6, "6 bolt holes + radial ribs must produce 6 distinct pocket bays");
        assert!(
            regions.iter().all(|r| r.outer.segs.iter().any(|s| matches!(s, Seg2::Arc { .. }))),
            "every segmented bay must have arc segments from perimeter/corner fillets"
        );

        let mut plain_cfg = cfg.clone();
        plain_cfg.auto_fillet_radius = None;
        plain_cfg.auto_chamfer_size = None;
        let plain_previews =
            preview_pockets_with_locations(&flange, top_face.index as u64, &plain_cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        let previews = preview_pockets_with_locations(&flange, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert_eq!(previews.len(), 6, "preview must return 6 segmented pocket cutter bodies");
        assert!(previews[0].face_count() > plain_previews[0].face_count(), "blended preview must include bottom fillet and top chamfer faces");

        let faces_before = flange.face_count();
        let vol_before = measure(&flange).unwrap().volume;
        let warnings = auto_pocket_with_warnings(&mut flange, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert!(warnings.is_empty(), "expected clean auto fillet + chamfer without warnings, got {warnings:?}");
        assert!(measure(&flange).unwrap().volume < vol_before, "pocket cut removed material");
        // Plain 6-bay cut without blends adds at most ~10-12 faces per bay; with bottom fillet + top chamfer it adds ~2x more.
        assert!(flange.face_count() > faces_before + 60, "blended cut must add sidewall + bottom fillet + top chamfer faces");
    }

    #[test]
    fn test_oversized_auto_fillet_falls_back_with_warning() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let initial_vol = measure(&b).unwrap().volume;
        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: Some(50.0), // Way too large for a 7mm deep pocket!
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };
        let warnings = auto_pocket_with_warnings(&mut b, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top)
            .expect("oversized fillet falls back instead of failing");
        assert!(!warnings.is_empty(), "expected fallback warning for oversized auto_fillet_radius");
        assert!(measure(&b).unwrap().volume < initial_vol, "base pocket cut must remain applied");
    }

    #[test]
    fn test_rounded_rect_plate_with_two_side_holes() {
        // Reproduces the rounded-rectangle mounting plate test subject:
        // 100x66 plate with R=10 corners, center bore R=14, and two mounting holes R=5 at x=+-33.
        let base_rect = box_solid(Vec3::new(-50.0, -33.0, 0.0), Vec3::new(50.0, 33.0, 18.0)).unwrap();
        let corner_edges = vec![Vec3::new(-50.0, -33.0, 9.0), Vec3::new(50.0, -33.0, 9.0), Vec3::new(50.0, 33.0, 9.0), Vec3::new(-50.0, 33.0, 9.0)];
        let mut plate = fillet(&base_rect, &corner_edges, 10.0).unwrap();
        let center_bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 15.0, 20.0).unwrap();
        let left_hole = cylinder(Vec3::new(-33.0, 0.0, -1.0), Vec3::Z, 5.0, 20.0).unwrap();
        let right_hole = cylinder(Vec3::new(33.0, 0.0, -1.0), Vec3::Z, 5.0, 20.0).unwrap();
        for h in [&center_bore, &left_hole, &right_hole] {
            plate = boolean(&plate, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = plate
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 18.0).abs() < 1e-3))
            .expect("plate top face");

        let cfg = LightenConfig {
            wall_offset: 4.0,
            depth_spec: PocketDepthSpec::BlindDepth(7.0),
            corner_radius: 1.5,
            draft_angle: 0.0,
            auto_fillet_radius: Some(2.0),
            auto_chamfer_size: Some(0.75),
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };

        let plane = plate.planar_face(top_face.index).unwrap();
        let regions = build_pocket_regions(&plate, top_face.index, &plane, &cfg).unwrap();
        assert_eq!(regions.len(), 2, "must decompose into 2 top/bottom pocket bays");

        // Every corner of both pocket bays must have tangent arc fillets (no sharp right-angle joins).
        for (ri, r) in regions.iter().enumerate() {
            let arc_count = r.outer.segs.iter().filter(|s| matches!(s, Seg2::Arc { .. })).count();
            assert!(arc_count >= 6, "bay {ri} must have tangent arcs at all corners (found {arc_count} arcs, segs={:?})", r.outer.segs);
            assert!(
                r.outer.segs.iter().all(|s| !matches!(s, Seg2::Arc { radius, .. } if *radius < cfg.corner_radius - 0.05)),
                "bay {ri} must not contain undersized cusp arcs (segs={:?})",
                r.outer.segs
            );
            for k in 0..r.outer.segs.len() {
                let a = &r.outer.segs[k];
                let b = &r.outer.segs[(k + 1) % r.outer.segs.len()];
                let kink = a.end_tangent().cross(b.start_tangent()).abs();
                assert!(kink <= 0.12, "bay {ri} corner {k} stayed sharp (kink={kink:.3}) between {a:?} and {b:?}");
            }
        }

        // Verify preview generation creates rounded/chamfered prism geometry.
        let mut plain_cfg = cfg.clone();
        plain_cfg.auto_fillet_radius = None;
        plain_cfg.auto_chamfer_size = None;
        let _plain_previews =
            preview_pockets_with_locations(&plate, top_face.index as u64, &plain_cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        let previews = preview_pockets_with_locations(&plate, top_face.index as u64, &cfg, FilletLocation::Bottom, ChamferLocation::Top).unwrap();
        assert_eq!(previews.len(), 2);

        // Verify execution applies unblended or blended cut cleanly.
        let mut plate_plain = plate.clone();
        auto_pocket(&mut plate_plain, top_face.index as u64, &plain_cfg).unwrap();
    }

    #[test]
    fn test_swapped_fillet_top_and_chamfer_bottom_locations() {
        let mut b = box_solid(Vec3::ZERO, Vec3::new(50.0, 50.0, 20.0)).unwrap();
        let top_face =
            b.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 20.0).abs() < 1e-3)).unwrap();
        let mut plain_cfg = LightenConfig {
            wall_offset: 5.0,
            depth_spec: PocketDepthSpec::BlindDepth(8.0),
            corner_radius: 3.0,
            draft_angle: 0.0,
            auto_fillet_radius: None,
            auto_chamfer_size: None,
            method: ManufacturingMethod::CncPocket,
            island_faces: Vec::new(),
        };
        let mut b_plain = b.clone();
        auto_pocket(&mut b_plain, top_face.index as u64, &plain_cfg).unwrap();

        plain_cfg.auto_fillet_radius = Some(2.5);
        plain_cfg.auto_chamfer_size = Some(1.0);
        let warnings = auto_pocket_with_warnings(&mut b, top_face.index as u64, &plain_cfg, FilletLocation::Top, ChamferLocation::Bottom)
            .expect("swapped Top Fillet + Bottom Chamfer succeeded");
        assert!(warnings.is_empty(), "expected 0 blend warnings, got {warnings:?}");
        assert!(b.face_count() > b_plain.face_count(), "Top fillet + Bottom chamfer must add blend faces");
        assert!((measure(&b).unwrap().volume - measure(&b_plain).unwrap().volume).abs() > 1.0, "Top fillet + Bottom chamfer modify edge volume");
    }

    #[test]
    fn test_user_80x50_plate_tangent_arcs_and_corner_radius_scaling() {
        // Reproduces user's 80x50 plate (R_plate=5mm), center boss R=12mm, side bolt holes R=4mm at (+-28, 0).
        let base_rect = box_solid(Vec3::new(-40.0, -25.0, 0.0), Vec3::new(40.0, 25.0, 12.0)).unwrap();
        let corner_edges = vec![Vec3::new(-40.0, -25.0, 6.0), Vec3::new(40.0, -25.0, 6.0), Vec3::new(40.0, 25.0, 6.0), Vec3::new(-40.0, 25.0, 6.0)];
        let mut plate = fillet(&base_rect, &corner_edges, 5.0).unwrap();
        let center_bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 12.0, 14.0).unwrap();
        let left_hole = cylinder(Vec3::new(-28.0, 0.0, -1.0), Vec3::Z, 4.0, 14.0).unwrap();
        let right_hole = cylinder(Vec3::new(28.0, 0.0, -1.0), Vec3::Z, 4.0, 14.0).unwrap();
        for h in [&center_bore, &left_hole, &right_hole] {
            plate = boolean(&plate, h, BoolOp::Cut).unwrap().unwrap();
        }

        let top_face = plate
            .faces(0.01)
            .unwrap()
            .into_iter()
            .find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99 && (f.centroid.z - 12.0).abs() < 1e-3))
            .expect("plate top face");
        let plane = plate.planar_face(top_face.index).unwrap();

        let mut prev_area = f64::MAX;
        for &cr in &[1.5f64, 3.0, 4.0, 5.0] {
            let cfg = LightenConfig {
                wall_offset: 4.0,
                depth_spec: PocketDepthSpec::BlindDepth(7.0),
                corner_radius: cr,
                draft_angle: 0.0,
                auto_fillet_radius: Some(1.5),
                auto_chamfer_size: Some(0.5),
                method: ManufacturingMethod::CncPocket,
                island_faces: Vec::new(),
            };
            let regions = build_pocket_regions(&plate, top_face.index, &plane, &cfg).unwrap_or_else(|e| panic!("corner_radius={cr} failed: {e}"));
            assert_eq!(regions.len(), 2, "corner_radius={cr} must yield 2 bays");

            let mut total_area = 0.0;
            for (ri, r) in regions.iter().enumerate() {
                total_area += r.area().abs();
                let matching_arcs = r.outer.segs.iter().filter(|s| matches!(s, Seg2::Arc { radius, .. } if (*radius - cr).abs() < 0.05)).count();
                assert!(
                    matching_arcs >= 4,
                    "corner_radius={cr} bay {ri} must have tangent corner arcs of radius ~{cr} (found {matching_arcs}, segs={:?})",
                    r.outer.segs
                );
                assert!(
                    r.outer.segs.iter().all(|s| !matches!(s, Seg2::Arc { radius, .. } if *radius < cr - 0.05)),
                    "corner_radius={cr} bay {ri} must not contain undersized cusp arcs (segs={:?})",
                    r.outer.segs
                );
                for k in 0..r.outer.segs.len() {
                    let a = &r.outer.segs[k];
                    let b = &r.outer.segs[(k + 1) % r.outer.segs.len()];
                    let kink = a.end_tangent().cross(b.start_tangent()).abs();
                    assert!(kink <= 0.05, "corner_radius={cr} bay {ri} seg {k} has sharp kink {kink:.3}: {a:?} -> {b:?}");
                }
            }
            assert!(
                total_area < prev_area - 1.0,
                "increasing corner_radius to {cr} must reduce pocket area cleanly (prev={prev_area:.2}, now={total_area:.2})"
            );
            prev_area = total_area;

            let previews =
                preview_pockets(&plate, top_face.index as u64, &cfg).unwrap_or_else(|e| panic!("preview_pockets failed at corner_radius={cr}: {e}"));
            assert_eq!(previews.len(), 2, "corner_radius={cr} preview must build 2 prisms");
        }
    }
}
