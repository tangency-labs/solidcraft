//! Unified obstacle-aware glyph placement, expansion, nudging, and tooltip positioning.

use egui::Pos2;

/// Distance from the pointer a cursor reaches down and right (#10794).
pub(crate) const CURSOR_REACH: f32 = 18.0;

const DIRS_8: [egui::Vec2; 8] = [
    egui::vec2(0.0, -1.0),
    egui::vec2(1.0, 0.0),
    egui::vec2(-1.0, 0.0),
    egui::vec2(0.0, 1.0),
    egui::vec2(0.7, -0.7),
    egui::vec2(-0.7, -0.7),
    egui::vec2(0.7, 0.7),
    egui::vec2(-0.7, 0.7),
];

/// Grid `(cols, rows)` packing `count` glyphs as squarely as possible (`cols = ceil(sqrt(count))`).
pub(crate) fn square_grid_dims(count: usize) -> (usize, usize) {
    if count <= 1 {
        return (count, count);
    }
    let cols = (count as f32).sqrt().ceil() as usize;
    (cols, count.div_ceil(cols))
}

pub(crate) fn clamp_rect_to_bounds(r: egui::Rect, bounds: egui::Rect) -> egui::Rect {
    let (w, h) = (r.width().min(bounds.width()), r.height().min(bounds.height()));
    let max_x = (bounds.right() - w).max(bounds.left());
    let max_y = (bounds.bottom() - h).max(bounds.top());
    egui::Rect::from_min_size(egui::pos2(r.left().clamp(bounds.left(), max_x), r.top().clamp(bounds.top(), max_y)), egui::vec2(w, h))
}

fn rect_overlap_area(a: egui::Rect, b: egui::Rect) -> f32 {
    let i = a.intersect(b);
    if i.width() > 0.0 && i.height() > 0.0 { i.width() * i.height() } else { 0.0 }
}

fn rect_dist_to_point(r: egui::Rect, p: Pos2) -> f32 {
    (r.left() - p.x).max(p.x - r.right()).max(0.0).hypot((r.top() - p.y).max(p.y - r.bottom()).max(0.0))
}

fn segment_rect_clip_len(a: Pos2, b: Pos2, r: egui::Rect) -> f32 {
    if a.x.max(b.x) < r.left() || a.x.min(b.x) > r.right() || a.y.max(b.y) < r.top() || a.y.min(b.y) > r.bottom() {
        return 0.0;
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [(-dx, a.x - r.left()), (dx, r.right() - a.x), (-dy, a.y - r.top()), (dy, r.bottom() - a.y)] {
        if p.abs() <= 1e-6 {
            if q < 0.0 {
                return 0.0;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return 0.0;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return 0.0;
                }
                t1 = t1.min(t);
            }
        }
    }
    if t1 <= t0 { 0.0 } else { (t1 - t0) * dx.hypot(dy) }
}

/// Shared screen-space obstacle scene for placing constraint badges, expanded boxes, and nudges.
pub(crate) struct PlacementScene {
    pub(crate) dim_texts: Vec<egui::Rect>,
    pub(crate) points: Vec<egui::Rect>,
    pub(crate) curves: Vec<(egui::Rect, Vec<Pos2>)>,
    pub(crate) placed: Vec<egui::Rect>,
}

impl PlacementScene {
    fn curves_overlap_pair(curves: &[&(egui::Rect, Vec<Pos2>)], r: egui::Rect, r_exp: egui::Rect) -> (f32, f32) {
        let (mut inner, mut outer) = (0.0_f32, 0.0_f32);
        for (aabb, poly) in curves {
            if !aabb.intersects(r_exp) {
                continue;
            }
            let hit_inner = aabb.intersects(r);
            for w in poly.windows(2) {
                let d_exp = segment_rect_clip_len(w[0], w[1], r_exp);
                if d_exp > 0.0 {
                    outer += d_exp;
                    if hit_inner {
                        inner += segment_rect_clip_len(w[0], w[1], r);
                    }
                }
            }
        }
        (inner, outer)
    }

    fn seg_crosses_curve(curves: &[&(egui::Rect, Vec<Pos2>)], a: Pos2, b: Pos2) -> bool {
        let seg_box = egui::Rect::from_two_pos(a, b);
        let r = b - a;
        for (aabb, poly) in curves {
            if !aabb.intersects(seg_box) {
                continue;
            }
            for w in poly.windows(2) {
                let s = w[1] - w[0];
                let rxs = r.x * s.y - r.y * s.x;
                if rxs.abs() <= 1e-4 {
                    continue;
                }
                let qmp = w[0] - a;
                let t = (qmp.x * s.y - qmp.y * s.x) / rxs;
                let u = (qmp.x * r.y - qmp.y * r.x) / rxs;
                if (0.12..=1.0).contains(&t) && (-1e-3..=1.0 + 1e-3).contains(&u) {
                    return true;
                }
            }
        }
        false
    }

    fn seg_hits_point(points: &[egui::Rect], a: Pos2, b: Pos2) -> bool {
        let ab = b - a;
        let l2 = ab.length_sq();
        if l2 <= 1.0 {
            return false;
        }
        points.iter().any(|pt| {
            let p = pt.center();
            let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
            t > 0.08 && (a + ab * t).distance(p) < 7.5
        })
    }

    /// Unified avoidance cost: strong point avoidance, moderate line/curve avoidance, weak crowded-area
    /// avoidance, plus optional curve-leader obstruction check and temporal direction hysteresis.
    #[allow(clippy::too_many_arguments)]
    fn soft_cost(
        &self,
        r: egui::Rect,
        near_curves: &[&(egui::Rect, Vec<Pos2>)],
        near_points: &[egui::Rect],
        near_neighbors: &[egui::Rect],
        leader_base: Option<Pos2>,
    ) -> (f32, bool) {
        let r_exp = r.expand(3.5);
        let (mut pt_ov, mut tight_pt) = (0.0_f32, 0.0_f32);
        for pt in near_points {
            let t = rect_overlap_area(pt.expand(3.5), r);
            if t > 0.0 {
                tight_pt += t;
                pt_ov += rect_overlap_area(*pt, r);
            }
        }
        let (line_len, tight_line) = Self::curves_overlap_pair(near_curves, r, r_exp);
        let dim_ov: f32 = self.dim_texts.iter().map(|d| rect_overlap_area(*d, r)).sum();
        let tight_neighbor: f32 = near_neighbors.iter().map(|p| rect_overlap_area(p.expand(4.0), r)).sum();

        let leader_cost = leader_base.map_or(0.0, |base| {
            let c = r.center();
            (if Self::seg_hits_point(near_points, base, c) { 150.0 } else { 0.0 })
                + (if Self::seg_crosses_curve(near_curves, base, c) { 85.0 } else { 0.0 })
        });
        let pt_cost = pt_ov * 24.0 + if pt_ov > 0.0 { 65.0 } else { 0.0 };
        let line_cost = line_len * 3.2 + if line_len > 0.5 { 14.0 } else { 0.0 };
        let close_cost = dim_ov * 1.8 + tight_neighbor * 0.35 + tight_pt * 0.55 + tight_line * 0.45;
        let clean = leader_cost == 0.0 && pt_cost == 0.0 && line_cost == 0.0 && close_cost == 0.0;
        (leader_cost + pt_cost + line_cost + close_cost, clean)
    }

    /// Place a base group rect (badge, box, or single glyph) near `base` with temporal hysteresis.
    pub(crate) fn place_group(
        &self,
        base: Pos2,
        dir_screen: Option<Pos2>,
        size: egui::Vec2,
        glyph: f32,
        cursor: Option<egui::Rect>,
        prev_dir: Option<egui::Vec2>,
    ) -> egui::Rect {
        let mut candidates: Vec<egui::Rect> = Vec::new();
        match dir_screen {
            Some(q) => {
                let t = (q - base).normalized();
                let side = if t.length() > 0.5 { egui::vec2(-t.y, t.x) } else { egui::vec2(0.7, -0.7) };
                let step = egui::vec2(side.y, -side.x);
                let clearance = |s: egui::Vec2, extra: f32| 0.5 * (s.x.abs() * size.x + s.y.abs() * size.y) + glyph * 0.4 + extra;
                let slide_step = (glyph * 0.75).max(8.0);
                let mut offsets = vec![0.0_f32];
                for k in 1..=3 {
                    offsets.push(k as f32);
                    offsets.push(-(k as f32));
                }
                for ring in 0..=2 {
                    let extra = ring as f32 * slide_step;
                    for &k in &offsets {
                        for s in [side, -side] {
                            candidates.push(egui::Rect::from_center_size(base + s * clearance(s, extra) + step * (k * slide_step), size));
                        }
                    }
                }
            }
            None => {
                let gap = glyph * 0.4;
                let step = (glyph * 0.75).max(8.0);
                for ring in 0..=3 {
                    let extra = ring as f32 * step;
                    for d in DIRS_8 {
                        let s = d.normalized();
                        let c = if d.x.abs() > 0.1 && d.y.abs() > 0.1 {
                            base + egui::vec2(d.x.signum() * (size.x * 0.5 + gap + extra * 0.7), d.y.signum() * (size.y * 0.5 + gap + extra * 0.7))
                        } else {
                            base + s * (0.5 * (s.x.abs() * size.x + s.y.abs() * size.y) + gap + extra)
                        };
                        candidates.push(egui::Rect::from_center_size(c, size));
                    }
                }
            }
        }
        if let Some(c) = cursor {
            let cb = c.expand(2.0);
            for min in [
                egui::pos2(base.x - size.x * 0.5, cb.top() - size.y),
                egui::pos2(base.x - size.x * 0.5, cb.bottom()),
                egui::pos2(cb.left() - size.x, base.y - size.y * 0.5),
                egui::pos2(cb.right(), base.y - size.y * 0.5),
            ] {
                candidates.push(egui::Rect::from_min_size(min, size));
            }
        }

        let search = egui::Rect::from_center_size(base, egui::Vec2::splat(size.max_elem() + glyph * 16.0));
        let near_curves: Vec<&(egui::Rect, Vec<Pos2>)> = self.curves.iter().filter(|(a, _)| a.intersects(search)).collect();
        let near_points: Vec<egui::Rect> = self.points.iter().copied().filter(|pt| pt.intersects(search)).collect();
        let near_placed: Vec<egui::Rect> = self.placed.iter().copied().filter(|p| p.expand(6.0).intersects(search)).collect();

        let ring0_len = if dir_screen.is_some() { 2 } else { 8 };
        let mut best: Option<(f32, egui::Rect, bool)> = None;
        for (idx, cand) in candidates.into_iter().enumerate() {
            if idx >= ring0_len && best.is_some_and(|(_, _, clean)| clean) {
                break;
            }
            let glyph_ov: f32 = near_placed.iter().map(|p| rect_overlap_area(p.expand(1.0), cand)).sum();
            let cur_ov: f32 = cursor.map_or(0.0, |c| rect_overlap_area(c, cand));
            let hard = cur_ov * 10_000.0 + glyph_ov * 10_000.0;
            if hard > 0.0 && best.is_some_and(|(bs, _, _)| hard >= bs) {
                continue;
            }
            let (soft, soft_clean) = self.soft_cost(cand, &near_curves, &near_points, &near_placed, dir_screen.map(|_| base));
            let dist_cost = rect_dist_to_point(cand, base) * 1.15 + cand.center().distance(base) * 0.25;
            let hysteresis = prev_dir.map_or(0.0, |pd| {
                let cd = (cand.center() - base).normalized();
                if cd.length_sq() > 0.1 { (1.0 - cd.dot(pd)) * 4.5 } else { 0.0 }
            });
            let s = hard + soft + dist_cost + hysteresis;
            let clean = cur_ov == 0.0 && glyph_ov == 0.0 && soft_clean;
            if best.is_none_or(|(bs, _, _)| s + 1e-3 < bs) {
                best = Some((s, cand, clean));
            }
        }
        best.map(|(_, r, _)| r)
            .unwrap_or_else(|| egui::Rect::from_center_size(base + egui::vec2(size.x * 0.5 + glyph * 0.4, -(size.y * 0.5 + glyph * 0.4)), size))
    }

    /// Place an expanded group container clear of `hard_blockers` while balancing avoidance and nearness.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn place_expanded(
        &self,
        badge_rect: egui::Rect,
        anchor_base: Pos2,
        box_size: egui::Vec2,
        bounds: egui::Rect,
        must_enclose_badge: bool,
        hard_blockers: &[egui::Rect],
        neighbors: &[egui::Rect],
        pad_margin: f32,
    ) -> egui::Rect {
        let bc = badge_rect.center();
        let mut candidates: Vec<egui::Rect> = Vec::new();
        for y in [bc.y - box_size.y * 0.5, badge_rect.bottom() - box_size.y, badge_rect.top()] {
            for x in [bc.x - box_size.x * 0.5, badge_rect.left(), badge_rect.right() - box_size.x] {
                let cand = clamp_rect_to_bounds(egui::Rect::from_min_size(egui::pos2(x, y), box_size), bounds);
                if !must_enclose_badge || cand.contains(bc) {
                    candidates.push(cand);
                }
            }
        }
        if !must_enclose_badge {
            for ring in 0..=10 {
                let extra = ring as f32 * 10.0;
                for d in DIRS_8 {
                    let s = d.normalized();
                    let c = if d.x.abs() > 0.1 && d.y.abs() > 0.1 {
                        anchor_base
                            + egui::vec2(d.x.signum() * (box_size.x * 0.5 + 6.0 + extra * 0.7), d.y.signum() * (box_size.y * 0.5 + 6.0 + extra * 0.7))
                    } else {
                        anchor_base + s * (0.5 * (s.x.abs() * box_size.x + s.y.abs() * box_size.y) + 6.0 + extra)
                    };
                    candidates.push(clamp_rect_to_bounds(egui::Rect::from_center_size(c, box_size), bounds));
                }
            }
            for b in hard_blockers {
                let pb = b.expand(pad_margin);
                for min in [
                    egui::pos2(anchor_base.x - box_size.x * 0.5, pb.top() - box_size.y),
                    egui::pos2(anchor_base.x - box_size.x * 0.5, pb.bottom()),
                    egui::pos2(pb.left() - box_size.x, anchor_base.y - box_size.y * 0.5),
                    egui::pos2(pb.right(), anchor_base.y - box_size.y * 0.5),
                ] {
                    candidates.push(clamp_rect_to_bounds(egui::Rect::from_min_size(min, box_size), bounds));
                }
            }
        }

        let mut search = egui::Rect::NOTHING;
        for cand in &candidates {
            search = search.union(*cand);
        }
        let search = search.expand(6.0);
        let near_curves: Vec<&(egui::Rect, Vec<Pos2>)> = self.curves.iter().filter(|(a, _)| a.intersects(search)).collect();
        let near_points: Vec<egui::Rect> = self.points.iter().copied().filter(|pt| pt.intersects(search)).collect();
        let mut best: Option<(f32, egui::Rect)> = None;
        for cand in candidates {
            let hard_ov: f32 = hard_blockers.iter().map(|b| rect_overlap_area(b.expand(pad_margin), cand)).sum();
            let hard = hard_ov * 10_000.0;
            if hard > 0.0 && best.is_some_and(|(bs, _)| hard >= bs) {
                continue;
            }
            let nbr_ov: f32 = neighbors.iter().map(|n| rect_overlap_area(n.expand(2.0), cand)).sum();
            let (soft, _) = self.soft_cost(cand, &near_curves, &near_points, neighbors, None);
            let dist_cost = rect_dist_to_point(cand, anchor_base) * 1.15 + cand.center().distance(anchor_base) * 0.25;
            let s = hard + nbr_ov * 20.0 + soft + dist_cost;
            if best.is_none_or(|(bs, _)| s + 1e-3 < bs) {
                best = Some((s, cand));
            }
        }
        best.map(|(_, r)| r).unwrap_or_else(|| clamp_rect_to_bounds(egui::Rect::from_center_size(bc, box_size), bounds))
    }

    /// Nudge `rect` clear of `blockers` and `settled`, balancing avoidance with minimal displacement.
    pub(crate) fn nudge_clear(&self, rect: egui::Rect, blockers: &[egui::Rect], settled: &[egui::Rect], bounds: egui::Rect) -> egui::Rect {
        if blockers.iter().all(|b| !rect.intersects(*b)) {
            return rect;
        }
        let mut candidates: Vec<egui::Rect> = Vec::new();
        for b in blockers {
            for min in [
                egui::pos2(b.left() - rect.width(), rect.top()),
                egui::pos2(b.right(), rect.top()),
                egui::pos2(rect.left(), b.top() - rect.height()),
                egui::pos2(rect.left(), b.bottom()),
                egui::pos2(b.left() - rect.width(), b.top() - rect.height()),
                egui::pos2(b.right(), b.top() - rect.height()),
                egui::pos2(b.left() - rect.width(), b.bottom()),
                egui::pos2(b.right(), b.bottom()),
            ] {
                candidates.push(clamp_rect_to_bounds(egui::Rect::from_min_size(min, rect.size()), bounds));
            }
        }
        let step = rect.width().max(rect.height()).max(16.0) + 4.0;
        for ring in 1..=12 {
            let d = ring as f32 * step;
            for dir in DIRS_8 {
                candidates.push(clamp_rect_to_bounds(egui::Rect::from_center_size(rect.center() + dir.normalized() * d, rect.size()), bounds));
            }
        }
        let near_curves: Vec<&(egui::Rect, Vec<Pos2>)> = self.curves.iter().collect();
        let pick = |avoid_settled: bool| {
            let mut best: Option<(f32, egui::Rect)> = None;
            for &c in &candidates {
                if blockers.iter().any(|b| c.intersects(*b)) {
                    continue;
                }
                if avoid_settled && settled.iter().any(|s| c.intersects(s.expand(2.0))) {
                    continue;
                }
                let (soft, _) = self.soft_cost(c, &near_curves, &self.points, settled, None);
                let s = soft + c.center().distance(rect.center()) * 1.1;
                if best.is_none_or(|(bs, _)| s + 1e-3 < bs) {
                    best = Some((s, c));
                }
            }
            best.map(|(_, r)| r)
        };
        pick(true).or_else(|| pick(false)).unwrap_or(rect)
    }
}

/// Text wrap width for a hovered glyph's label (#10794).
pub(crate) fn glyph_label_wrap_width(bounds: egui::Rect) -> f32 {
    (bounds.width() - 16.0).max(40.0)
}

/// Where a hovered glyph's label chip sits: clear of the pointer, the cursor, and the view edges (#10794).
pub(crate) fn glyph_label_rect(pointer: Pos2, glyph: egui::Rect, size: egui::Vec2, bounds: egui::Rect) -> egui::Rect {
    let cursor = egui::Rect::from_min_size(pointer, egui::vec2(CURSOR_REACH, CURSOR_REACH));
    let fit_x = |x: f32| x.min(bounds.right() - size.x).max(bounds.left());
    let fit_y = |y: f32| y.min(bounds.bottom() - size.y).max(bounds.top());
    let (above, below) = (glyph.top().min(pointer.y) - size.y - 4.0, glyph.bottom().max(cursor.bottom()) + 4.0);
    let (left, right) = (glyph.left().min(pointer.x) - size.x - 4.0, glyph.right().max(cursor.right()) + 4.0);
    for at in [
        egui::pos2(fit_x(glyph.left()), above),
        egui::pos2(left, fit_y(glyph.center().y - size.y * 0.5)),
        egui::pos2(fit_x(glyph.left()), below),
        egui::pos2(right, fit_y(glyph.center().y - size.y * 0.5)),
    ] {
        let r = egui::Rect::from_min_size(at, size);
        if bounds.contains_rect(r) && !r.intersects(cursor) && !r.contains(pointer) {
            return r;
        }
    }
    let beside = glyph.center().y - size.y * 0.5;
    let sides = [
        (egui::Rect::from_min_max(bounds.min, egui::pos2(bounds.right(), above + size.y)), egui::pos2(glyph.left(), above)),
        (egui::Rect::from_min_max(bounds.min, egui::pos2(left + size.x, bounds.bottom())), egui::pos2(left, beside)),
        (egui::Rect::from_min_max(egui::pos2(bounds.left(), below), bounds.max), egui::pos2(glyph.left(), below)),
        (egui::Rect::from_min_max(egui::pos2(right, bounds.top()), bounds.max), egui::pos2(right, beside)),
    ];
    let room = |s: &egui::Rect| s.width().clamp(0.0, size.x) * s.height().clamp(0.0, size.y);
    if let Some((side, at)) = sides.into_iter().max_by(|(a, _), (b, _)| room(a).total_cmp(&room(b))) {
        let x = at.x.min(side.right() - size.x).max(side.left());
        let y = at.y.min(side.bottom() - size.y).max(side.top());
        return egui::Rect::from_min_size(egui::pos2(x, y), size).intersect(side);
    }
    let fitted = egui::Rect::from_min_size(egui::pos2(fit_x(glyph.left()), fit_y(below)), size);
    egui::Rect::from_min_max(fitted.min, egui::pos2(fitted.max.x, fitted.max.y.min(bounds.bottom())))
}
