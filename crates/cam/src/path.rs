//! Toolpaths: the moves of the tool tip's centre in work coordinates.

use std::f64::consts::{FRAC_PI_2, TAU};

use serde::{Deserialize, Serialize};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::piece::wrap;

/// Most moves in one toolpath.
pub const MAX_MOVES: usize = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Move {
    /// Positioning at the machine's rapid rate; never through material.
    Rapid { to: Vec3 },
    /// Straight cut at `feed` mm/min.
    Feed { to: Vec3, feed: f64 },
    /// Circular cut about `center` in XY (a helix when Z changes), always less than a full
    /// turn: an arc that ends where it starts is no move at all.
    Arc { to: Vec3, center: Vec2, ccw: bool, feed: f64 },
    /// Circular cut in `plane` about `center` (only its coordinates in the plane count), less
    /// than a full turn: `G18` (ZX) and `G19` (YZ) arcs, as a vertical lead into or out of a
    /// cut. The coordinate along the plane's normal moves evenly.
    PlaneArc { to: Vec3, center: Vec3, plane: ArcPlane, ccw: bool, feed: f64 },
    /// Pause with the spindle running.
    Dwell { seconds: f64 },
}

impl Move {
    /// Where the tool is after the move (`None`: it stays).
    pub fn target(&self) -> Option<Vec3> {
        match *self {
            Move::Rapid { to } | Move::Feed { to, .. } | Move::Arc { to, .. } | Move::PlaneArc { to, .. } => Some(to),
            Move::Dwell { .. } => None,
        }
    }
}

/// Signed angle (positive = counter-clockwise) of an arc move from `from` to `to`; zero when
/// the two coincide in XY.
pub fn arc_sweep(from: Vec3, to: Vec3, center: Vec2, ccw: bool) -> f64 {
    let (a, b) = (Vec2::new(from.x, from.y) - center, Vec2::new(to.x, to.y) - center);
    if a.dist(b) <= 1e-9 {
        return 0.0;
    }
    let delta = wrap(b.angle() - a.angle());
    if ccw { delta } else { delta - TAU }
}

/// The plane an arc turns in: `G17` (XY), `G18` (ZX) or `G19` (YZ).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcPlane {
    #[default]
    Xy,
    Zx,
    Yz,
}

impl ArcPlane {
    /// The plane's first and second axes and its normal (0 X, 1 Y, 2 Z): turning from the
    /// first towards the second is counter-clockwise (`G3`) seen from the normal's + end.
    pub fn axes(self) -> (usize, usize, usize) {
        match self {
            ArcPlane::Xy => (0, 1, 2),
            ArcPlane::Zx => (2, 0, 1),
            ArcPlane::Yz => (1, 2, 0),
        }
    }

    /// The G-code that selects the plane (17, 18 or 19).
    pub fn code(self) -> u8 {
        match self {
            ArcPlane::Xy => 17,
            ArcPlane::Zx => 18,
            ArcPlane::Yz => 19,
        }
    }

    /// The vertical plane a horizontal direction `d` lies in, when it runs along X or Y.
    pub fn vertical_along(d: Vec2) -> Option<ArcPlane> {
        let len = d.len();
        if !(len > 1e-12) {
            return None;
        }
        if d.y.abs() <= 1e-9 * len {
            Some(ArcPlane::Zx)
        } else if d.x.abs() <= 1e-9 * len {
            Some(ArcPlane::Yz)
        } else {
            None
        }
    }

    /// `p` in the plane's coordinates (first, second) and along its normal.
    pub fn split(self, p: Vec3) -> (Vec2, f64) {
        let (u, v, w) = self.axes();
        (Vec2::new(axis(p, u), axis(p, v)), axis(p, w))
    }

    /// The point with in-plane coordinates `uv` and `w` along the normal.
    pub fn join(self, uv: Vec2, w: f64) -> Vec3 {
        let (u, v, n) = self.axes();
        let mut c = [0.0; 3];
        for (i, value) in [(u, uv.x), (v, uv.y), (n, w)] {
            if let Some(slot) = c.get_mut(i) {
                *slot = value;
            }
        }
        Vec3::new(c[0], c[1], c[2])
    }
}

/// Component `i` of `p` (0 X, 1 Y, else Z).
fn axis(p: Vec3, i: usize) -> f64 {
    match i {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

/// An arc from `from` to `to` about `center` in `plane` (only the centre's in-plane
/// coordinates count), moving evenly along the plane's normal: always less than a full turn,
/// as [`arc_sweep`] takes one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneArc {
    pub from: Vec3,
    pub to: Vec3,
    pub center: Vec3,
    pub plane: ArcPlane,
    pub ccw: bool,
}

impl PlaneArc {
    fn rel(&self, p: Vec3) -> Vec2 {
        self.plane.split(p).0 - self.plane.split(self.center).0
    }

    /// The radius, from the start.
    pub fn radius(&self) -> f64 {
        self.rel(self.from).len()
    }

    /// The signed turn (counter-clockwise in the plane positive); zero when the ends coincide
    /// in the plane.
    pub fn sweep(&self) -> f64 {
        let (a, b) = (self.rel(self.from), self.rel(self.to));
        if a.dist(b) <= 1e-9 {
            return 0.0;
        }
        let delta = wrap(b.angle() - a.angle());
        if self.ccw { delta } else { delta - TAU }
    }

    /// The length along the arc (a helix when the normal coordinate changes).
    pub fn length(&self) -> f64 {
        let dw = self.plane.split(self.to).1 - self.plane.split(self.from).1;
        (self.radius() * self.sweep()).hypot(dw)
    }

    /// The point a fraction `t` of the way along.
    pub fn at(&self, t: f64) -> Vec3 {
        if t >= 1.0 {
            return self.to;
        }
        let start = self.rel(self.from);
        let (c, _) = self.plane.split(self.center);
        let (_, w0) = self.plane.split(self.from);
        let (_, w1) = self.plane.split(self.to);
        let uv = c + Vec2::from_angle(start.angle() + self.sweep() * t) * start.len();
        self.plane.join(uv, w0 + (w1 - w0) * t)
    }

    /// The unit direction of travel in the plane a fraction `t` of the way along (`None` for an
    /// arc of no radius).
    pub fn tangent(&self, t: f64) -> Option<Vec3> {
        let start = self.rel(self.from);
        let a = start.angle() + self.sweep() * t;
        let d = Vec2::from_angle(a).perp();
        let d = if self.ccw { d } else { -d };
        (start.len() > 1e-12).then(|| self.plane.join(d, 0.0))
    }

    /// Points along the arc within `tol` of it, after the start, the last at the end.
    pub fn points(&self, tol: f64) -> Vec<Vec3> {
        let (r, sweep) = (self.radius(), self.sweep());
        let tol = tol.max(1e-6);
        let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { TAU / 8.0 };
        let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
        (1..=n).map(|i| self.at(i as f64 / n as f64)).collect()
    }
}

/// How far the lines of a vertical lead along a direction not along X or Y may be from the arc
/// (mm).
const LEAD_CHORD: f64 = 0.001;

/// A quarter turn from `from` to `to` about `center` in the vertical plane of the horizontal
/// direction `along`, at `feed`: one `G18`/`G19` arc when `along` runs along X or Y, else lines
/// within [`LEAD_CHORD`] of the arc. A vertical lead into a cut or out of one.
pub(crate) fn vertical_quarter(from: Vec3, to: Vec3, center: Vec3, along: Vec2, feed: f64) -> Vec<Move> {
    if let Some(plane) = ArcPlane::vertical_along(along) {
        let (s, e) = (plane.split(from).0 - plane.split(center).0, plane.split(to).0 - plane.split(center).0);
        return vec![Move::PlaneArc { to, center, plane, ccw: s.x * e.y - s.y * e.x > 0.0, feed }];
    }
    // In the plane of `along` (its first axis) and Z (its second).
    let d = along.normalized().unwrap_or(Vec2::new(1.0, 0.0));
    let local = |q: Vec3| Vec2::new((q.x - center.x) * d.x + (q.y - center.y) * d.y, q.z - center.z);
    let (ls, le) = (local(from), local(to));
    let r = ls.len();
    let sweep = if ls.x * le.y - ls.y * le.x > 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 };
    let step = if LEAD_CHORD < r { 2.0 * (1.0 - LEAD_CHORD / r).acos() } else { FRAC_PI_2 };
    let n = ((FRAC_PI_2 / step.max(1e-3)).ceil() as usize).clamp(1, 256);
    (1..=n)
        .map(|i| {
            let q = if i == n {
                to
            } else {
                let w = Vec2::from_angle(ls.angle() + sweep * i as f64 / n as f64) * r;
                Vec3::new(center.x + d.x * w.x, center.y + d.y * w.x, center.z + w.y)
            };
            Move::Feed { to: q, feed }
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Toolpath {
    pub moves: Vec<Move>,
}

/// Lengths and time of a toolpath.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathStats {
    /// Distance travelled at feed (mm).
    pub cut_length: f64,
    /// Distance travelled at rapid (mm).
    pub rapid_length: f64,
    /// Time at feed, including dwells (minutes).
    pub cut_minutes: f64,
    /// Lowest Z reached (mm); `None` for a path without moves.
    pub lowest_z: Option<f64>,
}

impl Toolpath {
    pub fn stats(&self) -> PathStats {
        let mut s = PathStats::default();
        let mut pos: Option<Vec3> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid { to } => s.rapid_length += pos.map_or(0.0, |p| p.dist(to)),
                Move::Feed { to, feed } => {
                    let len = pos.map_or(0.0, |p| p.dist(to));
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                }
                Move::Arc { to, center, ccw, feed } => {
                    let len = pos.map_or(0.0, |p| {
                        let r = Vec2::new(p.x, p.y).dist(center);
                        (r * arc_sweep(p, to, center, ccw)).hypot(to.z - p.z)
                    });
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                }
                Move::PlaneArc { to, center, plane, ccw, feed } => {
                    let len = pos.map_or(0.0, |from| PlaneArc { from, to, center, plane, ccw }.length());
                    s.cut_length += len;
                    s.cut_minutes += if feed > 0.0 { len / feed } else { 0.0 };
                }
                Move::Dwell { seconds } => s.cut_minutes += seconds.max(0.0) / 60.0,
            }
            if let Some(to) = m.target() {
                s.lowest_z = Some(s.lowest_z.map_or(to.z, |z: f64| z.min(to.z)));
                pos = Some(to);
            }
        }
        s
    }

    /// The path as points for display and checking: `(point, cutting)` where `cutting` says
    /// whether the tool reached the point at feed. Arcs are followed within `tol`.
    pub fn points(&self, tol: f64) -> Vec<(Vec3, bool)> {
        let mut out = Vec::new();
        let mut pos: Option<Vec3> = None;
        for m in &self.moves {
            match *m {
                Move::Rapid { to } => out.push((to, false)),
                Move::Feed { to, .. } => out.push((to, true)),
                Move::Arc { to, center, ccw, .. } => {
                    if let Some(p) = pos {
                        let start = Vec2::new(p.x, p.y) - center;
                        let (r, sweep) = (start.len(), arc_sweep(p, to, center, ccw));
                        let tol = tol.max(1e-6);
                        let step = if tol < r { 2.0 * (1.0 - tol / r).acos() } else { TAU / 8.0 };
                        let n = ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 4096);
                        for i in 1..n {
                            let t = i as f64 / n as f64;
                            let q = center + Vec2::from_angle(start.angle() + sweep * t) * r;
                            out.push((q.extend(p.z + (to.z - p.z) * t), true));
                        }
                    }
                    out.push((to, true));
                }
                Move::PlaneArc { to, center, plane, ccw, .. } => match pos {
                    Some(from) => out.extend(PlaneArc { from, to, center, plane, ccw }.points(tol).into_iter().map(|q| (q, true))),
                    None => out.push((to, true)),
                },
                Move::Dwell { .. } => {}
            }
            if let Some(to) = m.target() {
                pos = Some(to);
            }
        }
        out
    }
}
