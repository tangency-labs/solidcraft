//! The dark area of a Gerber image layer (copper, solder mask, silkscreen) as exact closed
//! contours of lines and arcs.
//!
//! The reader (`gerber.rs`) turns the file into ordered objects: flashed apertures, drawn
//! strokes and regions, each dark or clear. Each object becomes closed polylines with arc
//! segments (the `cavalier_contours` crate, MIT OR Apache-2.0), which are added to or cut from
//! the image in file order. The result is a set of islands, each an outer contour with holes; an
//! island can sit inside another island's hole, so the contours nest and alternate between
//! material and hole.

use std::collections::HashMap;

use cavalier_contours::polyline::{BooleanOp, BooleanResultInfo, PlineSource, PlineSourceMut, Polyline};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::Geom2;

pub(crate) type Pline = Polyline<f64>;

const TAU: f64 = std::f64::consts::TAU;
/// Most primitives one aperture macro may have.
const MAX_MACRO_PRIMITIVES: usize = 4096;
/// Most outline points one macro primitive may have.
const MAX_OUTLINE_POINTS: usize = 10_000;
/// Deepest nesting of parentheses in a macro expression.
const MAX_DEPTH: usize = 32;
/// Most contours one region may split into.
const MAX_REGION_LOOPS: usize = 4096;
/// Contours smaller than this (mm²) are left out: they are numerical dust.
const MIN_AREA: f64 = 1e-10;

/// One segment of a drawn path, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seg {
    Line(Vec2, Vec2),
    /// An arc around `c`; `from == to` is a full circle.
    Arc {
        from: Vec2,
        to: Vec2,
        c: Vec2,
        ccw: bool,
    },
}

impl Seg {
    fn from(&self) -> Vec2 {
        match *self {
            Seg::Line(a, _) | Seg::Arc { from: a, .. } => a,
        }
    }

    fn to(&self) -> Vec2 {
        match *self {
            Seg::Line(_, b) | Seg::Arc { to: b, .. } => b,
        }
    }
}

/// What one Gerber object is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Shape {
    /// The aperture flashed at a point.
    Flash(Vec2, i32),
    /// A line or arc drawn with an aperture.
    Stroke(Seg, i32),
    /// A filled region's contour (G36/G37).
    Region(Vec<Seg>),
}

/// An object of the image, in file order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    /// Loaded with clear polarity (`%LPC%`): it erases what is under it.
    pub clear: bool,
    pub shape: Shape,
}

/// A defined aperture, in file units around its flash point.
#[derive(Clone, Debug, Default)]
pub(crate) struct Aperture {
    islands: Vec<Island>,
    /// A round aperture's radius, which strokes use.
    round: Option<f64>,
    /// A rectangle's width and height, which strokes may use (deprecated but allowed).
    rect: Option<(f64, f64)>,
}

/// Material bounded by an outer contour, less its holes.
#[derive(Clone, Debug)]
pub(crate) struct Island {
    outer: Pline,
    holes: Vec<Pline>,
    lo: Vec2,
    hi: Vec2,
}

impl Island {
    fn new(outer: Pline, holes: Vec<Pline>) -> Option<Island> {
        let e = outer.extents()?;
        Some(Island { outer, holes, lo: Vec2::new(e.min_x, e.min_y), hi: Vec2::new(e.max_x, e.max_y) })
    }

    fn overlaps(&self, other: &Island) -> bool {
        boxes_overlap((self.lo, self.hi), (other.lo, other.hi))
    }

    fn moved(&self, scale: f64, at: Vec2) -> Option<Island> {
        let mv = |p: &Pline| {
            let mut p = p.clone();
            p.scale_mut(scale);
            p.translate_mut(at.x, at.y);
            p
        };
        Island::new(mv(&self.outer), self.holes.iter().map(mv).collect())
    }
}

/// A dark area being built: islands that never overlap.
#[derive(Default)]
pub(crate) struct Area {
    islands: Vec<Island>,
    /// Boolean steps the library could not do; the shapes were kept apart.
    pub failed: usize,
}

impl Area {
    /// Add material.
    pub fn add(&mut self, new: Island) {
        let mut cur = new;
        let mut i = 0;
        while let Some(island) = self.islands.get(i) {
            if island.overlaps(&cur)
                && let Some(merged) = merge(island, &cur, &mut self.failed)
            {
                cur = merged;
                self.islands.swap_remove(i);
                // The grown island may now reach islands already passed.
                i = 0;
                continue;
            }
            i += 1;
        }
        self.islands.push(cur);
    }

    /// Erase material (clear polarity).
    pub fn cut(&mut self, p: &Pline) {
        let Some(e) = p.extents() else { return };
        let bounds = (Vec2::new(e.min_x, e.min_y), Vec2::new(e.max_x, e.max_y));
        let mut out = Vec::with_capacity(self.islands.len());
        for island in std::mem::take(&mut self.islands) {
            if !boxes_overlap((island.lo, island.hi), bounds) {
                out.push(island);
                continue;
            }
            let r = island.outer.boolean(p, BooleanOp::Not);
            match r.result_info {
                BooleanResultInfo::Disjoint => out.push(island),
                BooleanResultInfo::InvalidInput => {
                    self.failed += 1;
                    out.push(island);
                }
                BooleanResultInfo::Pline1InsidePline2 | BooleanResultInfo::Overlapping => {}
                BooleanResultInfo::Pline2InsidePline1 => {
                    if in_a_hole(&island, p) {
                        out.push(island);
                        continue;
                    }
                    // A new hole, joined with the holes it reaches.
                    let mut hole = p.clone();
                    let mut holes = Vec::new();
                    for h in island.holes {
                        let u = h.boolean(&hole, BooleanOp::Or);
                        match u.result_info {
                            BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => holes.push(h),
                            _ => {
                                if let Some(joined) =
                                    u.pos_plines.into_iter().map(|q| q.pline).max_by(|x, y| x.area().abs().total_cmp(&y.area().abs()))
                                {
                                    hole = joined;
                                }
                            }
                        }
                    }
                    holes.push(hole);
                    out.extend(Island::new(island.outer, holes));
                }
                BooleanResultInfo::Intersected => {
                    let mut pieces: Vec<Island> = r.pos_plines.into_iter().filter_map(|q| Island::new(q.pline, Vec::new())).collect();
                    // Holes the cut reaches open onto the outside; the others stay holes of the
                    // piece they are in.
                    let (open, closed): (Vec<Pline>, Vec<Pline>) = island.holes.into_iter().partition(|h| touches(h, p));
                    for h in &open {
                        pieces = pieces.into_iter().flat_map(|piece| self.less(piece, h)).collect();
                    }
                    for h in closed {
                        let at = h.at(0).pos();
                        if let Some(piece) = pieces.iter_mut().find(|q| q.outer.winding_number(at) != 0) {
                            piece.holes.push(h);
                        }
                    }
                    out.extend(pieces);
                }
            }
        }
        self.islands = out;
    }

    /// An island with no holes less a contour.
    fn less(&mut self, piece: Island, h: &Pline) -> Vec<Island> {
        let r = piece.outer.boolean(h, BooleanOp::Not);
        match r.result_info {
            BooleanResultInfo::Disjoint => vec![piece],
            BooleanResultInfo::InvalidInput => {
                self.failed += 1;
                vec![piece]
            }
            BooleanResultInfo::Pline1InsidePline2 | BooleanResultInfo::Overlapping => Vec::new(),
            BooleanResultInfo::Pline2InsidePline1 => {
                let mut piece = piece;
                piece.holes.push(h.clone());
                vec![piece]
            }
            BooleanResultInfo::Intersected => r.pos_plines.into_iter().filter_map(|q| Island::new(q.pline, Vec::new())).collect(),
        }
    }

    /// The islands as closed contours: each outer contour, then its holes.
    pub fn loops(&self) -> Vec<Vec<Geom2>> {
        let mut out = Vec::new();
        for island in &self.islands {
            out.push(pline_geom(&island.outer));
            out.extend(island.holes.iter().map(pline_geom));
        }
        out.retain(|l| !l.is_empty());
        out
    }

    pub fn islands(self) -> Vec<Island> {
        self.islands
    }
}

/// The union of two islands, or `None` when they stay apart.
fn merge(a: &Island, b: &Island, failed: &mut usize) -> Option<Island> {
    let r = a.outer.boolean(&b.outer, BooleanOp::Or);
    let (outer, mut holes) = match r.result_info {
        BooleanResultInfo::Disjoint => return None,
        BooleanResultInfo::InvalidInput => {
            *failed += 1;
            return None;
        }
        // One island inside the other's outer contour: inside one of its holes, it stays
        // its own island.
        BooleanResultInfo::Pline2InsidePline1 if in_a_hole(a, &b.outer) => return None,
        BooleanResultInfo::Pline1InsidePline2 if in_a_hole(b, &a.outer) => return None,
        BooleanResultInfo::Pline2InsidePline1 | BooleanResultInfo::Overlapping => (a.outer.clone(), Vec::new()),
        BooleanResultInfo::Pline1InsidePline2 => (b.outer.clone(), Vec::new()),
        BooleanResultInfo::Intersected => {
            let mut pos = r.pos_plines.into_iter().map(|p| p.pline);
            let outer = pos.next()?;
            if pos.next().is_some() {
                *failed += 1;
                return None;
            }
            (outer, r.neg_plines.into_iter().map(|p| p.pline).collect())
        }
    };
    // What stays empty: a hole of one less the other's material, and where both have holes.
    for (x, y) in [(a, b), (b, a)] {
        for h in &x.holes {
            holes.extend(minus(h, &y.outer));
        }
    }
    for ha in &a.holes {
        for hb in &b.holes {
            if plines_overlap(ha, hb) {
                holes.extend(ha.boolean(hb, BooleanOp::And).pos_plines.into_iter().map(|p| p.pline));
            }
        }
    }
    holes.retain(|h| h.area().abs() > MIN_AREA);
    Island::new(outer, holes)
}

/// Whether `p` lies inside one of the island's holes.
fn in_a_hole(island: &Island, p: &Pline) -> bool {
    island
        .holes
        .iter()
        .any(|h| matches!(h.boolean(p, BooleanOp::Or).result_info, BooleanResultInfo::Pline2InsidePline1 | BooleanResultInfo::Overlapping))
}

/// Whether `b` reaches into `a` (crosses it or covers it).
fn touches(a: &Pline, b: &Pline) -> bool {
    plines_overlap(a, b)
        && matches!(
            a.boolean(b, BooleanOp::Not).result_info,
            BooleanResultInfo::Intersected | BooleanResultInfo::Pline1InsidePline2 | BooleanResultInfo::Overlapping
        )
}

/// What is left of `a` outside `b`.
fn minus(a: &Pline, b: &Pline) -> Vec<Pline> {
    if !plines_overlap(a, b) {
        return vec![a.clone()];
    }
    let r = a.boolean(b, BooleanOp::Not);
    match r.result_info {
        BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => vec![a.clone()],
        BooleanResultInfo::Pline1InsidePline2 | BooleanResultInfo::Overlapping => Vec::new(),
        // `b` inside `a`: `a` with a hole, which holes cannot have; keep `a` whole (the
        // material inside is its own island already).
        BooleanResultInfo::Pline2InsidePline1 => vec![a.clone()],
        BooleanResultInfo::Intersected => r.pos_plines.into_iter().map(|q| q.pline).collect(),
    }
}

fn plines_overlap(a: &Pline, b: &Pline) -> bool {
    match (a.extents(), b.extents()) {
        (Some(x), Some(y)) => x.min_x <= y.max_x && y.min_x <= x.max_x && x.min_y <= y.max_y && y.min_y <= x.max_y,
        _ => false,
    }
}

fn boxes_overlap(a: (Vec2, Vec2), b: (Vec2, Vec2)) -> bool {
    a.0.x <= b.1.x && b.0.x <= a.1.x && a.0.y <= b.1.y && b.0.y <= a.1.y
}

// ---- building the image ----

/// The image's dark area from its objects, in file order. `unit` is millimetres per file unit
/// (aperture sizes are in file units). Returns the contours and what could not be drawn.
pub(crate) fn build(items: &[Item], apertures: &HashMap<i32, Aperture>, unit: f64, warnings: &mut Vec<String>) -> Vec<Vec<Geom2>> {
    let mut area = Area::default();
    let (mut missing, mut unstrokable, mut bad) = (0usize, 0usize, 0usize);
    for item in items {
        let islands: Vec<Island> = match &item.shape {
            Shape::Flash(at, code) => match apertures.get(code) {
                Some(ap) => ap.islands.iter().filter_map(|i| i.moved(unit, *at)).collect(),
                None => {
                    missing += 1;
                    continue;
                }
            },
            Shape::Stroke(seg, code) => match apertures.get(code) {
                Some(Aperture { round: Some(r), .. }) => stroke_round(seg, r * unit),
                Some(Aperture { rect: Some((w, h)), .. }) => match *seg {
                    Seg::Line(a, b) => Island::new(stroke_rect(a, b, w * unit, h * unit), Vec::new()).into_iter().collect(),
                    Seg::Arc { .. } => {
                        unstrokable += 1;
                        continue;
                    }
                },
                Some(_) => {
                    unstrokable += 1;
                    continue;
                }
                None => {
                    missing += 1;
                    continue;
                }
            },
            Shape::Region(segs) => {
                let r = region(segs);
                if r.is_empty() {
                    bad += 1;
                }
                r
            }
        };
        for island in islands {
            if item.clear {
                area.cut(&island.outer);
            } else {
                area.add(island);
            }
        }
    }
    if missing > 0 {
        warnings.push(format!("{missing} objects use an aperture the file never defines and were skipped."));
    }
    if unstrokable > 0 {
        warnings.push(format!("{unstrokable} lines or arcs drawn with an aperture that cannot draw (only round apertures can) were skipped."));
    }
    if bad > 0 {
        warnings.push(format!("{bad} regions enclose no area and were skipped."));
    }
    if area.failed > 0 {
        warnings.push(format!("{} overlapping shapes could not be joined exactly and were kept apart.", area.failed));
    }
    area.loops()
}

/// A closed polyline through the points, counter-clockwise.
fn closed(points: impl IntoIterator<Item = (Vec2, f64)>) -> Option<Pline> {
    let mut p = Pline::new_closed();
    for (q, bulge) in points {
        if !(q.x.is_finite() && q.y.is_finite() && bulge.is_finite()) {
            return None;
        }
        p.add(q.x, q.y, bulge);
    }
    let mut p = p.remove_repeat_pos(1e-9).unwrap_or(p);
    if p.vertex_count() < 2 || p.area().abs() <= MIN_AREA {
        return None;
    }
    if p.area() < 0.0 {
        p.invert_direction_mut();
    }
    Some(p)
}

fn circle(c: Vec2, r: f64) -> Option<Pline> {
    if !r.is_finite() || r <= 0.0 {
        return None;
    }
    closed([(Vec2::new(c.x - r, c.y), 1.0), (Vec2::new(c.x + r, c.y), 1.0)])
}

fn polygon(points: &[Vec2]) -> Option<Pline> {
    closed(points.iter().map(|p| (*p, 0.0)))
}

/// A line drawn with a round aperture: a stadium.
fn stadium(a: Vec2, b: Vec2, r: f64) -> Option<Pline> {
    let len = (b.x - a.x).hypot(b.y - a.y);
    if len <= 1e-12 {
        return circle(a, r);
    }
    let (nx, ny) = (-(b.y - a.y) / len * r, (b.x - a.x) / len * r);
    let at = |p: Vec2, s: f64| Vec2::new(p.x + s * nx, p.y + s * ny);
    closed([(at(a, -1.0), 0.0), (at(b, -1.0), 1.0), (at(b, 1.0), 0.0), (at(a, 1.0), 1.0)])
}

/// A segment drawn with a round aperture of radius `r`.
fn stroke_round(seg: &Seg, r: f64) -> Vec<Island> {
    let one = |p: Option<Pline>| p.and_then(|p| Island::new(p, Vec::new())).into_iter().collect::<Vec<_>>();
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    match *seg {
        Seg::Line(a, b) => one(stadium(a, b, r)),
        Seg::Arc { from, to, c, ccw } => {
            let radius = from.dist(c);
            if from == to {
                // A full ring: the outer circle less the inner one.
                let outer = circle(c, radius + r);
                let hole = if radius > r { circle(c, radius - r) } else { None };
                return outer.and_then(|o| Island::new(o, hole.into_iter().collect())).into_iter().collect();
            }
            let sweep = if ccw { ccw_sweep(c, from, to) } else { ccw_sweep(c, to, from) };
            // Counter-clockwise from `s` to `e`.
            let (s, e) = if ccw { (from, to) } else { (to, from) };
            if radius <= r {
                // The inside closes up; joined stadia along short chords are within a hair.
                return chords(c, s, sweep, radius, r);
            }
            let unit = |p: Vec2| Vec2::new((p.x - c.x) / radius, (p.y - c.y) / radius);
            let (us, ue) = (unit(s), unit(e));
            let on = |u: Vec2, rad: f64| Vec2::new(c.x + u.x * rad, c.y + u.y * rad);
            // Arcs of at most half a turn each.
            let half = sweep > std::f64::consts::PI;
            let k = if half { 2.0 } else { 1.0 };
            let bulge = (sweep / (4.0 * k)).tan();
            let t = us.y.atan2(us.x) + sweep / 2.0;
            let um = Vec2::new(t.cos(), t.sin());
            let mut pts = vec![(on(us, radius + r), bulge)];
            if half {
                pts.push((on(um, radius + r), bulge));
            }
            pts.push((on(ue, radius + r), 1.0));
            pts.push((on(ue, radius - r), -bulge));
            if half {
                pts.push((on(um, radius - r), -bulge));
            }
            pts.push((on(us, radius - r), 1.0));
            one(closed(pts))
        }
    }
}

/// An arc too tight for a band, as the union of stadia along chords.
fn chords(c: Vec2, s: Vec2, sweep: f64, radius: f64, r: f64) -> Vec<Island> {
    let steps = ((sweep / TAU) * 64.0).ceil().clamp(4.0, 64.0) as usize;
    let t0 = (s.y - c.y).atan2(s.x - c.x);
    let at = |k: usize| {
        let t = t0 + sweep * k as f64 / steps as f64;
        Vec2::new(c.x + t.cos() * radius, c.y + t.sin() * radius)
    };
    let mut area = Area::default();
    for k in 0..steps {
        if let Some(p) = stadium(at(k), at(k + 1), r).and_then(|p| Island::new(p, Vec::new())) {
            area.add(p);
        }
    }
    area.islands()
}

/// A line drawn with a rectangular aperture: the convex hull of the rectangle at both ends.
fn stroke_rect(a: Vec2, b: Vec2, w: f64, h: f64) -> Pline {
    let (x, y) = (w / 2.0, h / 2.0);
    let mut pts = Vec::with_capacity(8);
    for p in [a, b] {
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            pts.push(Vec2::new(p.x + sx * x, p.y + sy * y));
        }
    }
    polygon(&hull(pts)).unwrap_or_default()
}

/// Convex hull, counter-clockwise (monotone chain).
fn hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut out: Vec<Vec2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = out.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while out.len() >= start + 2 && out.get(out.len() - 2).zip(out.last()).is_some_and(|(o, a)| cross(*o, *a, p) <= 0.0) {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// A region's contour as islands. A contour may visit its holes through cut-in lines (drawn
/// there and back): those pairs are dropped and the rest split into separate contours, which
/// nest into outer contours and holes.
fn region(segs: &[Seg]) -> Vec<Island> {
    // Cut-ins: a line and the same line backwards.
    let mut back: HashMap<[u64; 4], usize> = HashMap::new();
    let key = |a: Vec2, b: Vec2| [a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()];
    let mut keep = vec![true; segs.len()];
    for (i, s) in segs.iter().enumerate() {
        if let Seg::Line(a, b) = *s {
            if let Some(j) = back.remove(&key(b, a)) {
                keep[i] = false;
                if let Some(k) = keep.get_mut(j) {
                    *k = false;
                }
            } else {
                back.insert(key(a, b), i);
            }
        }
    }
    let rest: Vec<Seg> = segs.iter().zip(&keep).filter(|(_, k)| **k).map(|(s, _)| *s).collect();
    // Chain what is left into closed contours.
    let mut starts: HashMap<[u64; 2], Vec<usize>> = HashMap::new();
    for (i, s) in rest.iter().enumerate() {
        let p = s.from();
        starts.entry([p.x.to_bits(), p.y.to_bits()]).or_default().push(i);
    }
    let mut used = vec![false; rest.len()];
    let mut loops: Vec<Pline> = Vec::new();
    for first in 0..rest.len() {
        if used.get(first).copied().unwrap_or(true) {
            continue;
        }
        let mut path = Vec::new();
        let mut i = first;
        while let Some(s) = rest.get(i) {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            path.push(*s);
            let end = s.to();
            if end == rest.get(first).map_or(end, Seg::from) {
                break;
            }
            let next =
                starts.get(&[end.x.to_bits(), end.y.to_bits()]).and_then(|c| c.iter().copied().find(|k| !used.get(*k).copied().unwrap_or(true)));
            match next {
                Some(k) => i = k,
                None => break,
            }
        }
        if let Some(p) = contour(&path) {
            loops.push(p);
        }
        if loops.len() > MAX_REGION_LOOPS {
            break;
        }
    }
    // Nesting: an even depth is material, an odd one a hole of the contour around it.
    loops.sort_by(|a, b| b.area().abs().total_cmp(&a.area().abs()));
    let depth_parent: Vec<(usize, Option<usize>)> = (0..loops.len())
        .map(|i| {
            let Some(p) = loops.get(i) else { return (0, None) };
            let at = p.at(0).pos();
            let around: Vec<usize> = (0..i).filter(|j| loops.get(*j).is_some_and(|q| q.winding_number(at) != 0)).collect();
            (around.len(), around.last().copied())
        })
        .collect();
    let mut islands: Vec<(usize, Island)> = Vec::new();
    for (i, p) in loops.iter().enumerate() {
        let Some(&(depth, parent)) = depth_parent.get(i) else { continue };
        if depth % 2 == 0 {
            islands.extend(Island::new(p.clone(), Vec::new()).map(|isl| (i, isl)));
        } else if let Some(parent) = parent
            && let Some((_, isl)) = islands.iter_mut().find(|(k, _)| *k == parent)
        {
            isl.holes.push(p.clone());
        }
    }
    islands.into_iter().map(|(_, i)| i).collect()
}

/// A closed polyline along a chain of segments.
fn contour(path: &[Seg]) -> Option<Pline> {
    let mut pts = Vec::with_capacity(path.len() + 4);
    for s in path {
        match *s {
            Seg::Line(a, _) => pts.push((a, 0.0)),
            Seg::Arc { from, to, c, ccw } => {
                let sweep = if from == to {
                    TAU
                } else if ccw {
                    ccw_sweep(c, from, to)
                } else {
                    ccw_sweep(c, to, from)
                };
                let sign = if ccw { 1.0 } else { -1.0 };
                // Pieces of at most a half turn (a bulge of at most 1).
                let n = if sweep > std::f64::consts::PI + 1e-9 { 2 } else { 1 };
                let piece = sweep / n as f64;
                let radius = from.dist(c);
                let t0 = (from.y - c.y).atan2(from.x - c.x);
                for k in 0..n {
                    let t = t0 + sign * piece * k as f64;
                    let p = if k == 0 { from } else { Vec2::new(c.x + t.cos() * radius, c.y + t.sin() * radius) };
                    pts.push((p, sign * (piece / 4.0).tan()));
                }
            }
        }
    }
    closed(pts)
}

/// A closed polyline as sketch geometry: lines, arcs, and a circle when it is one.
fn pline_geom(p: &Pline) -> Vec<Geom2> {
    let n = p.vertex_count();
    if n == 2 {
        let (a, b) = (p.at(0), p.at(1));
        if (a.bulge - b.bulge).abs() < 1e-9 && (a.bulge.abs() - 1.0).abs() < 1e-9 {
            let (pa, pb) = (a.pos(), b.pos());
            let c = Vec2::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0);
            return vec![Geom2::Circle(c, (pa.x - c.x).hypot(pa.y - c.y))];
        }
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let v = p.at(i);
        let w = p.at(p.next_wrapping_index(i));
        let (a, b) = (Vec2::new(v.x, v.y), Vec2::new(w.x, w.y));
        if a == b {
            continue;
        }
        if v.bulge.abs() < 1e-12 {
            out.push(Geom2::Line(a, b));
        } else {
            let c = bulge_centre(a, b, v.bulge);
            out.push(if v.bulge > 0.0 { Geom2::Arc { c, a, b } } else { Geom2::Arc { c, a: b, b: a } });
        }
    }
    out
}

/// The centre of the arc from `a` to `b` with this bulge (the tangent of a quarter of its sweep,
/// positive counter-clockwise).
fn bulge_centre(a: Vec2, b: Vec2, bulge: f64) -> Vec2 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    // From the chord's midpoint to the centre, along the chord's left normal.
    let s = (1.0 - bulge * bulge) / (4.0 * bulge);
    Vec2::new((a.x + b.x) / 2.0 - dy * s, (a.y + b.y) / 2.0 + dx * s)
}

/// Counter-clockwise angle from `a` to `b` around `c`, in `[0, 2π)`.
fn ccw_sweep(c: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ta = (a.y - c.y).atan2(a.x - c.x);
    let tb = (b.y - c.y).atan2(b.x - c.x);
    (tb - ta).rem_euclid(TAU)
}

// ---- apertures ----

/// An aperture definition's body (`D10C,0.5`, `D11RoundRect,0.1X...`), in file units; macros by
/// name with their bodies.
pub(crate) fn aperture(def: &str, macros: &HashMap<String, String>) -> Result<(i32, Aperture), String> {
    let rest = def.strip_prefix('D').ok_or("an aperture definition must start with D")?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let code: i32 = rest.get(..digits).and_then(|d| d.parse().ok()).filter(|c| *c >= 10).ok_or("an aperture's number must be 10 or more")?;
    let rest = rest.get(digits..).unwrap_or("");
    let (name, args) = match rest.split_once(',') {
        Some((n, a)) => (n.trim(), a.trim()),
        None => (rest.trim(), ""),
    };
    let nums = |sep: char| -> Result<Vec<f64>, String> {
        if args.is_empty() {
            return Ok(Vec::new());
        }
        args.split(sep)
            .map(|v| {
                v.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|f| f.is_finite() && f.abs() < 1e6)
                    .ok_or_else(|| format!("aperture D{code} has a size that is not a number"))
            })
            .collect()
    };
    let with_hole = |outer: Option<Pline>, hole: Option<&f64>| -> Vec<Island> {
        let mut area = Area::default();
        if let Some(i) = outer.and_then(|o| Island::new(o, Vec::new())) {
            area.add(i);
        }
        if let Some(h) = hole.and_then(|d| circle(Vec2::ZERO, d / 2.0)) {
            area.cut(&h);
        }
        area.islands()
    };
    let ap = match name {
        "C" => {
            let v = nums('X')?;
            let d = v.first().copied().unwrap_or(0.0).abs();
            Aperture { islands: with_hole(circle(Vec2::ZERO, d / 2.0), v.get(1)), round: Some(d / 2.0), rect: None }
        }
        "R" | "O" => {
            let v = nums('X')?;
            let (w, h) = (v.first().copied().unwrap_or(0.0).abs(), v.get(1).copied().unwrap_or(0.0).abs());
            let outer = if name == "R" { rect(Vec2::ZERO, w, h, 0.0) } else { obround(w, h) };
            Aperture { islands: with_hole(outer, v.get(2)), round: None, rect: (name == "R").then_some((w, h)) }
        }
        "P" => {
            let v = nums('X')?;
            let d = v.first().copied().unwrap_or(0.0).abs();
            let n = v.get(1).copied().unwrap_or(0.0).round();
            if !(3.0..=12.0).contains(&n) {
                return Err(format!("aperture D{code} is a polygon with {n} corners; 3 to 12 are allowed"));
            }
            let outer = regular(Vec2::ZERO, d / 2.0, n as usize, v.get(2).copied().unwrap_or(0.0));
            Aperture { islands: with_hole(outer, v.get(3)), round: None, rect: None }
        }
        _ => {
            let body = macros.get(name).ok_or_else(|| format!("aperture D{code} uses the macro `{name}`, which the file does not define"))?;
            let params: Vec<f64> = if args.is_empty() {
                Vec::new()
            } else {
                args.split('X').map(|a| a.trim().parse::<f64>().ok().filter(|f| f.is_finite()).unwrap_or(0.0)).collect()
            };
            Aperture { islands: run_macro(body, params)?, round: None, rect: None }
        }
    };
    Ok((code, ap))
}

/// A rectangle centred at `c`, turned `deg` degrees about the origin.
fn rect(c: Vec2, w: f64, h: f64, deg: f64) -> Option<Pline> {
    let (x, y) = (w / 2.0, h / 2.0);
    let pts: Vec<Vec2> = [(-x, -y), (x, -y), (x, y), (-x, y)].iter().map(|(dx, dy)| turn(Vec2::new(c.x + dx, c.y + dy), deg)).collect();
    polygon(&pts)
}

fn obround(w: f64, h: f64) -> Option<Pline> {
    if (w - h).abs() < 1e-12 {
        return circle(Vec2::ZERO, w / 2.0);
    }
    if w > h {
        let x = (w - h) / 2.0;
        stadium(Vec2::new(-x, 0.0), Vec2::new(x, 0.0), h / 2.0)
    } else {
        let y = (h - w) / 2.0;
        stadium(Vec2::new(0.0, -y), Vec2::new(0.0, y), w / 2.0)
    }
}

/// A regular polygon around `c` with its first corner at `deg` degrees.
fn regular(c: Vec2, r: f64, n: usize, deg: f64) -> Option<Pline> {
    let pts: Vec<Vec2> = (0..n)
        .map(|k| {
            let t = deg.to_radians() + TAU * k as f64 / n as f64;
            Vec2::new(c.x + r * t.cos(), c.y + r * t.sin())
        })
        .collect();
    polygon(&pts)
}

/// A point turned `deg` degrees counter-clockwise about the origin.
fn turn(p: Vec2, deg: f64) -> Vec2 {
    if deg == 0.0 {
        return p;
    }
    let (s, c) = deg.to_radians().sin_cos();
    Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

/// An aperture macro's shape for these parameters: its primitives in order, each adding (exposure
/// on) or erasing (off) area.
fn run_macro(body: &str, params: Vec<f64>) -> Result<Vec<Island>, String> {
    let mut vars: HashMap<u32, f64> = params.into_iter().enumerate().map(|(i, v)| (i as u32 + 1, v)).collect();
    let mut area = Area::default();
    let mut skipped = false;
    for (n, prim) in body.split('*').map(str::trim).filter(|p| !p.is_empty()).enumerate() {
        if n >= MAX_MACRO_PRIMITIVES {
            return Err(format!("an aperture macro has more than {MAX_MACRO_PRIMITIVES} primitives"));
        }
        // A comment (code 0; no primitive code starts with a 0).
        if prim.starts_with('0') {
            continue;
        }
        if let Some(def) = prim.strip_prefix('$')
            && let Some((var, expr)) = def.split_once('=')
        {
            let var: u32 = var.trim().parse().map_err(|_| "an aperture macro sets a variable with no number".to_string())?;
            let v = eval(expr, &vars)?;
            vars.insert(var, v);
            continue;
        }
        let words: Vec<f64> = prim.split(',').map(|w| eval(w, &vars)).collect::<Result<_, _>>()?;
        let code = words.first().copied().unwrap_or(-1.0);
        let arg = |k: usize| words.get(k).copied().unwrap_or(0.0);
        let dark = arg(1) != 0.0;
        let shape: Option<Pline> = match code as i64 {
            1 => circle(turn(Vec2::new(arg(3), arg(4)), arg(5)), arg(2).abs() / 2.0),
            20 => {
                let (a, b, w) = (Vec2::new(arg(3), arg(4)), Vec2::new(arg(5), arg(6)), arg(2).abs());
                let len = (b.x - a.x).hypot(b.y - a.y);
                if len <= 1e-12 {
                    None
                } else {
                    let (nx, ny) = (-(b.y - a.y) / len * w / 2.0, (b.x - a.x) / len * w / 2.0);
                    let at = |p: Vec2, s: f64| turn(Vec2::new(p.x + s * nx, p.y + s * ny), arg(7));
                    polygon(&[at(a, -1.0), at(b, -1.0), at(b, 1.0), at(a, 1.0)])
                }
            }
            21 => rect(Vec2::new(arg(4), arg(5)), arg(2).abs(), arg(3).abs(), arg(6)),
            4 => {
                let count = arg(2).max(0.0) as usize;
                if count > MAX_OUTLINE_POINTS {
                    return Err(format!("an aperture macro outline has more than {MAX_OUTLINE_POINTS} points"));
                }
                let deg = arg(5 + 2 * count);
                let pts: Vec<Vec2> = (0..=count).map(|k| turn(Vec2::new(arg(3 + 2 * k), arg(4 + 2 * k)), deg)).collect();
                polygon(&pts)
            }
            5 => {
                let n = arg(2).round();
                if (3.0..=12.0).contains(&n) {
                    let c = turn(Vec2::new(arg(3), arg(4)), arg(6));
                    regular(c, arg(5).abs() / 2.0, n as usize, arg(6))
                } else {
                    None
                }
            }
            7 => {
                // A thermal: a ring with two crossing gaps; its words have no exposure.
                let (c, outer, inner, gap, deg) = (Vec2::new(arg(1), arg(2)), arg(3).abs(), arg(4).abs(), arg(5).abs(), arg(6));
                let centre = turn(c, deg);
                if let Some(o) = circle(centre, outer / 2.0).and_then(|o| Island::new(o, Vec::new())) {
                    let mut ring = Area::default();
                    ring.add(o);
                    for cut in [circle(centre, inner / 2.0), rect(c, outer * 1.01, gap, deg), rect(c, gap, outer * 1.01, deg)].into_iter().flatten() {
                        ring.cut(&cut);
                    }
                    for i in ring.islands() {
                        area.add(i);
                    }
                }
                continue;
            }
            _ => {
                skipped = true;
                None
            }
        };
        let Some(shape) = shape else { continue };
        if dark {
            area.add(Island::new(shape, Vec::new()).ok_or("an aperture macro shape has no size")?);
        } else {
            area.cut(&shape);
        }
    }
    let islands = area.islands();
    if islands.is_empty() && skipped {
        return Err("an aperture macro uses only primitives this reader does not draw (moiré)".into());
    }
    Ok(islands)
}

/// A macro expression: numbers, `$n` variables, `+ - x /` with the usual precedence (`x` and
/// `X` multiply) and parentheses.
fn eval(expr: &str, vars: &HashMap<u32, f64>) -> Result<f64, String> {
    let chars: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Expr { s: &chars, k: 0, vars, depth: 0 };
    let v = p.sum()?;
    if p.k != chars.len() || !v.is_finite() {
        return Err(format!("an aperture macro has an expression that cannot be read: `{}`", expr.chars().take(40).collect::<String>()));
    }
    Ok(v)
}

struct Expr<'a> {
    s: &'a [char],
    k: usize,
    vars: &'a HashMap<u32, f64>,
    depth: usize,
}

impl Expr<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.k).copied()
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.k += 1;
            let r = self.product()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        while let Some(op @ ('x' | 'X' | '/')) = self.peek() {
            self.k += 1;
            let r = self.unary()?;
            v = if op == '/' { if r == 0.0 { 0.0 } else { v / r } } else { v * r };
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('-') => {
                self.k += 1;
                Ok(-self.unary()?)
            }
            Some('+') => {
                self.k += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        let bad = || "an aperture macro has an expression that cannot be read".to_string();
        match self.peek() {
            Some('(') => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(bad());
                }
                self.k += 1;
                let v = self.sum()?;
                if self.peek() != Some(')') {
                    return Err(bad());
                }
                self.k += 1;
                self.depth -= 1;
                Ok(v)
            }
            Some('$') => {
                self.k += 1;
                let start = self.k;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.k += 1;
                }
                let n: String = self.s.get(start..self.k).unwrap_or(&[]).iter().collect();
                let n: u32 = n.parse().map_err(|_| bad())?;
                // An unset variable is 0, as the specification says.
                Ok(self.vars.get(&n).copied().unwrap_or(0.0))
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.k;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.k += 1;
                }
                let n: String = self.s.get(start..self.k).unwrap_or(&[]).iter().collect();
                n.parse().map_err(|_| bad())
            }
            _ => Err(bad()),
        }
    }
}
