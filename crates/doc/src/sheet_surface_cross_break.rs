//! Cross Break (#8345) on a sheet made from surfaces (#6748): two shallow creases pressed corner
//! to corner across one flat panel, so that the panel bows a little and stops drumming. They are
//! marks on the panel, as a perforation field is: kept in the panel's chart, laid out with it into
//! the flat (`sheet.flat_pattern` lists them, the DXF writes them on layers of their own) and
//! carried with it into space for the view. The B-rep, the outline and the volume do not change.
//!
//! The creases are the diagonals of the smallest rectangle round the panel's material as the flat
//! has it when the cross break is made (short of its bends, less its reliefs), clipped to that
//! material (a corner relief or a notch shortens them instead of leaving them on air), each then
//! stopping `inset` short of the edge at both ends.

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{Development, Surface};
use solidcraft_geom::prelude::*;

use crate::sheet_surface::Piece;
use crate::{DocError, Result, SurfaceSheet};

/// The pick is on no flat face of a panel of the sheet.
pub const CROSS_BREAK_OFF_PANEL: &str =
    "a cross break needs a point on the top or bottom face of a flat panel of the sheet (not on a bend, a rim or off the sheet)";
/// The panel already carries one.
pub const CROSS_BREAK_TWICE: &str = "this panel already has a cross break";
/// A cross break whose pick lies on the faces of two sheets at once.
pub const CROSS_BREAK_SEVERAL: &str = "not supported yet: a cross break picked on the faces of two sheets at once; name the sheet's body";
/// A fold whose bend zone crosses a cross break's creases.
pub const CROSS_BREAK_IN_FOLD: &str = "the fold's bend zone runs through a cross break: not supported yet";

/// The most outline points the rectangle round a panel is fitted to.
const MAX_OUTLINE_POINTS: usize = 20_000;
/// A crease shorter than this (mm) is no crease.
const MIN_CREASE: f64 = 1e-3;

/// A cross break on one panel of a sheet (`sheet.cross_break`): in the sheet's record its creases
/// are in the panel's chart; laid out (see [`SurfaceSheet::flat_cross_breaks`]) they are in the
/// flat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossBreak {
    pub feature: u64,
    /// The panel it is on.
    pub panel: usize,
    /// The two creases, each from one end to the other.
    pub lines: [(Vec2, Vec2); 2],
    /// How far the middle of the panel bows (mm), for the drawing and the press.
    pub depth: f64,
    /// The panel bows toward its top face: the sheet's surface, the side up bends turn to.
    pub up: bool,
}

impl SurfaceSheet {
    /// Mark a cross break on the flat panel whose top or bottom face `pick` (the body's
    /// coordinates) lies on, for feature `feature`: two creases corner to corner, each `inset`
    /// short of the panel's edge, the panel bowing `depth` toward its top face when `up`. Returns
    /// the panel. Refused off a flat panel's face, on a panel that already has one, and when the
    /// inset leaves nothing of a crease; a refusal leaves the sheet as it was.
    pub fn add_cross_break(&mut self, feature: u64, pick: Vec3, inset: f64, depth: f64, up: bool) -> Result<usize> {
        if !(inset.is_finite() && inset >= 0.0) {
            return Err(DocError::Invalid(format!("a cross break's inset must be 0 or more (got {inset})")));
        }
        if !(depth.is_finite() && depth > 0.0) {
            return Err(DocError::Invalid(format!("a cross break's depth must be more than 0 (got {depth})")));
        }
        if !(pick.x.is_finite() && pick.y.is_finite() && pick.z.is_finite()) {
            return Err(DocError::Invalid(CROSS_BREAK_OFF_PANEL.into()));
        }
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let (panel, outline) = self.panel_under(&dev, pick)?;
        if self.cross_breaks.iter().any(|c| c.panel == panel) {
            return Err(DocError::Invalid(CROSS_BREAK_TWICE.into()));
        }
        if outline.len() < 3 || outline.len() > MAX_OUTLINE_POINTS {
            return Err(DocError::Invalid(format!("a cross break needs a panel outline of 3 to {MAX_OUTLINE_POINTS} points")));
        }
        let [c0, c1, c2, c3] = enclosing_rectangle(&outline).ok_or_else(|| DocError::Invalid("the panel has no area for a cross break".into()))?;
        let mut lines = [(c0, c2), (c3, c1)];
        for (a, b) in &mut lines {
            let (lo, hi) = inside_span(&outline, *a, *b)
                .ok_or_else(|| DocError::Invalid("the panel's middle is not material: no room for a cross break".into()))?;
            let len = a.dist(*b);
            let (s0, s1) = (lo + inset / len, hi - inset / len);
            if !((s1 - s0) * len >= MIN_CREASE) {
                return Err(DocError::Invalid(format!("an inset of {inset} mm leaves nothing of the cross break on this panel")));
            }
            (*a, *b) = (a.lerp(*b, s0), a.lerp(*b, s1));
        }
        self.cross_breaks.push(CrossBreak { feature, panel, lines, depth, up });
        self.lay_out_cross_breaks(&dev);
        Ok(panel)
    }

    /// The flat panel whose top or bottom face a point lies on (within a quarter of the
    /// thickness, inside the panel's material in the flat), with that material's outline in the
    /// panel's chart: the nearest face when two are that close.
    fn panel_under(&self, dev: &Development, pick: Vec3) -> Result<(usize, Vec<Vec2>)> {
        let frames = crate::sheet_surface_cuts::frames(self, dev)?;
        let pieces = crate::sheet_surface::pieces(dev, &mut crate::sheet::FlatWork::new())?;
        let t = self.thickness;
        let mut best: Option<(f64, usize, Vec<Vec2>)> = None;
        for (i, (panel, frame)) in self.sheet.panels.iter().zip(&frames).enumerate() {
            let (Surface::Plane, Some(frame), Some(Some(at))) = (&panel.surface, frame, dev.placements.get(i)) else { continue };
            let q = frame.inverse().point(pick);
            let off = q.z.abs().min((q.z - t).abs());
            if off > t * 0.25 || best.as_ref().is_some_and(|(d, ..)| *d <= off) {
                continue;
            }
            let flat = at.point(Vec2::new(q.x, q.y));
            let back = at.inverse();
            let held = pieces.iter().find(|(p, r)| {
                matches!(p, Piece::Panel(k) if *k == i) && r.outer.ccw().contains(flat) && !r.holes.iter().any(|h| h.ccw().contains(flat))
            });
            if let Some((_, r)) = held {
                let outline: Vec<Vec2> = (&r.outer.ccw().polyline(1e-2)).into_iter().map(|p| back.point(*p)).collect();
                best = Some((off, i, outline));
            }
        }
        best.map(|(_, i, outline)| (i, outline)).ok_or_else(|| DocError::Invalid(CROSS_BREAK_OFF_PANEL.into()))
    }

    /// Lays the cross breaks out into the flat, each crease running from its left end as the flat
    /// is read (see [`SurfaceSheet::shown`]).
    pub(crate) fn lay_out_cross_breaks(&mut self, dev: &Development) {
        self.flat_cross_breaks = self
            .cross_breaks
            .iter()
            .filter_map(|c| {
                let at = dev.placements.get(c.panel).copied().flatten()?;
                Some(CrossBreak { lines: left_to_right(c.lines.map(|(a, b)| (at.point(a), at.point(b)))), ..c.clone() })
            })
            .collect();
    }

    /// The key of a cross break's panel (what made it, see `Panel::name`), for the flat pattern's
    /// listing.
    pub fn cross_break_panel(&self, c: &CrossBreak) -> String {
        self.sheet.panels.get(c.panel).map(|p| p.name.clone()).unwrap_or_default()
    }

    /// What a fold across panel `panel` would do to its cross breaks: one wholly past the fold's
    /// zone (`side` of a point at least `ba`) goes to the piece that moves there (`piece_at`), as
    /// each one's index and that piece's panel; one wholly on the side that stays stays, and one
    /// the zone runs through is refused.
    pub(crate) fn fold_cross_breaks(
        &self,
        panel: usize,
        side: impl Fn(Vec2) -> f64,
        ba: f64,
        tol: f64,
        piece_at: impl Fn(Vec2) -> Option<usize>,
    ) -> Result<Vec<(usize, usize)>> {
        let mut out = Vec::new();
        for (k, c) in self.cross_breaks.iter().enumerate().filter(|(_, c)| c.panel == panel) {
            let s: Vec<f64> = c.lines.iter().flat_map(|(a, b)| [side(*a), side(*b)]).collect();
            if s.iter().all(|x| *x >= ba - tol) {
                if let Some(p) = piece_at(c.lines[0].0) {
                    out.push((k, p));
                }
            } else if !s.iter().all(|x| *x <= tol) {
                return Err(DocError::Invalid(CROSS_BREAK_IN_FOLD.into()));
            }
        }
        Ok(out)
    }

    /// The creases of every cross break in the body's coordinates, a hair off the top face and
    /// off the bottom one so that they read on either side of the panel: for each crease, the
    /// top's segment and then the bottom's. What the view draws.
    pub fn cross_break_segments(&self) -> Vec<(Vec3, Vec3)> {
        if self.cross_breaks.is_empty() {
            return Vec::new();
        }
        let Ok(dev) = self.develop() else { return Vec::new() };
        let Ok(frames) = crate::sheet_surface_cuts::frames(self, &dev) else { return Vec::new() };
        let t = self.thickness;
        let lift = (t * 0.01).clamp(1e-3, 0.05);
        let mut out = Vec::new();
        for c in &self.cross_breaks {
            let Some(Some(frame)) = frames.get(c.panel) else { continue };
            for (a, b) in c.lines {
                // The panel's chart runs into the material: its top face is z 0.
                for z in [-lift, t + lift] {
                    let (p, q) = (frame.point(Vec3::new(a.x, a.y, z)), frame.point(Vec3::new(b.x, b.y, z)));
                    if [p, q].iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
                        out.push((p, q));
                    }
                }
            }
        }
        out
    }
}

/// Each line from its end that comes first as the flat is read (seen from the surface, y the
/// other way): the smaller x, then the larger y.
pub(crate) fn left_to_right(lines: [(Vec2, Vec2); 2]) -> [(Vec2, Vec2); 2] {
    lines.map(|(a, b)| if (b.x, -b.y) < (a.x, -a.y) { (b, a) } else { (a, b) })
}

/// The corners (in order round it) of the smallest rectangle round the points, whose sides run
/// along one side of their convex hull. None for points with no area.
fn enclosing_rectangle(pts: &[Vec2]) -> Option<[Vec2; 4]> {
    let hull = convex_hull(pts);
    if hull.len() < 3 {
        return None;
    }
    let mut best: Option<(f64, [Vec2; 4])> = None;
    for k in 0..hull.len() {
        let (Some(&a), Some(&b)) = (hull.get(k), hull.get((k + 1) % hull.len())) else { continue };
        let Some(u) = (b - a).normalized() else { continue };
        let v = u.perp();
        let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in &hull {
            let (s, t) = (p.dot(u), p.dot(v));
            (u0, u1, v0, v1) = (u0.min(s), u1.max(s), v0.min(t), v1.max(t));
        }
        let area = (u1 - u0) * (v1 - v0);
        // A nearly equal rectangle keeps the earlier one, so a square-cornered panel keeps its
        // own sides however its points happen to be ordered.
        if best.as_ref().is_none_or(|(b, _)| area < *b - 1e-9 * b.abs().max(1.0)) {
            let at = |s: f64, t: f64| u * s + v * t;
            best = Some((area, [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)]));
        }
    }
    best.filter(|(area, _)| *area > 1e-9).map(|(_, r)| r)
}

/// The convex hull of the points, counter-clockwise (monotone chain).
fn convex_hull(pts: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = pts.iter().copied().filter(|q| q.x.is_finite() && q.y.is_finite()).collect();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup_by(|a, b| a.dist(*b) < 1e-12);
    if p.len() < 3 {
        return p;
    }
    let turn = |o: Vec2, a: Vec2, b: Vec2| (a - o).cross(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(p.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let points: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(p.iter()) } else { Box::new(p.iter().rev()) };
        for &q in points {
            while hull.len() >= start + 2 {
                let (Some(&o), Some(&a)) = (hull.get(hull.len() - 2), hull.last()) else { break };
                if turn(o, a, q) > 1e-12 {
                    break;
                }
                hull.pop();
            }
            hull.push(q);
        }
        hull.pop();
    }
    hull
}

/// Along the line from `a` to `b` (0 at `a`, 1 at `b`), the stretch inside the closed polygon
/// `pts` that holds the middle: between the outline crossings nearest the middle on either
/// side. None when the middle is outside the polygon.
fn inside_span(pts: &[Vec2], a: Vec2, b: Vec2) -> Option<(f64, f64)> {
    let mid = a.lerp(b, 0.5);
    if !Loop2::polygon(pts).ccw().contains(mid) {
        return None;
    }
    let d = b - a;
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    let n = pts.len();
    for k in 0..n {
        let (Some(&p), Some(&q)) = (pts.get(k), pts.get((k + 1) % n)) else { continue };
        let e = q - p;
        let den = d.cross(e);
        if den.abs() < 1e-12 {
            continue;
        }
        let s = (p - a).cross(e) / den;
        let t = (p - a).cross(d) / den;
        if !(-1e-9..=1.0 + 1e-9).contains(&t) {
            continue;
        }
        if s < 0.5 {
            lo = lo.max(s);
        } else {
            hi = hi.min(s);
        }
    }
    (hi > lo).then_some((lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn the_rectangle_round_a_turned_square_is_the_square() {
        let c = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
        let r = enclosing_rectangle(&[v(0.0, 0.0), v(c, c), v(0.0, 2.0 * c), v(-c, c)]).unwrap_or_else(|| panic!("no rectangle"));
        assert!((r[0].dist(r[1]) - 10.0).abs() < 1e-9 && (r[1].dist(r[2]) - 10.0).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn a_crease_stops_at_a_notched_corner() {
        // A 100 x 50 panel with a 10 x 10 notch at (0, 0): the diagonal from there starts where
        // it meets the notch's side, a tenth of the way along.
        let pts = [v(10.0, 0.0), v(100.0, 0.0), v(100.0, 50.0), v(0.0, 50.0), v(0.0, 10.0), v(10.0, 10.0)];
        let (lo, hi) = inside_span(&pts, v(0.0, 0.0), v(100.0, 50.0)).unwrap_or_else(|| panic!("no span"));
        assert!((lo - 0.1).abs() < 1e-9 && (hi - 1.0).abs() < 1e-9, "{lo} {hi}");
        // The middle off the material: no span.
        let ring = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 10.0), v(10.0, 10.0), v(10.0, 50.0), v(0.0, 50.0)];
        assert!(inside_span(&ring, v(0.0, 0.0), v(100.0, 50.0)).is_none());
    }
}
