//! Exact keep-or-drop of a cell: whether the field's boundary comes within a cell's reach, measured
//! on the boundary's own curves and the cell's own outline (a round cell is a circle, not a
//! polygon round it), not on the chords the planner flattens a spline or a conic to.
//!
//! Lifted from h0witzer/solidcraft#8 by @alfonsotames (Apache-2.0, as SolidCraft is), the part
//! of `crates/doc/src/sheet_surface_perforation_shapes.rs` that measures a hole against the
//! edges of a sheet: [`Edge`] (a line, an arc, or a curve), its distance tests, `sweeps`,
//! `to_segment` and `segment_distance`. What differs: the vector and curve types are solidcore's,
//! the edges are bucketed over the field for a query to look only at the edges near a cell, the
//! test is the planner's (the cell shrunk by [`EPS`] meets an edge, or lies nearer one than the
//! `margin`), and a conic, cubic or Bézier is measured on its chords, not by a search for its
//! nearest point (which can miss a basin between samples): the chords' distance less [`CHORD`] is
//! a lower bound of the curve's, so a cell is kept only when that clears the margin. A curve
//! whose chords cannot be cut to [`CHORD`] is refused ([`Edge::of`] is `None`). Lines and arcs
//! are measured in closed form, from the nearer end of a segment so that large coordinates keep
//! their clearance.

use std::f64::consts::{FRAC_PI_2, TAU};

use crate::math::Vec2;
use crate::prism::profile::{cross2, Seg2};

use super::region::{Convex, EPS};
use super::vec::ring_next;

/// The room (mm) a cell is looked at with beyond its box: a curve is found in the buckets of its
/// control points' box, and this keeps a boundary just outside a cell in view.
const CHORD: f64 = 1e-3;

/// How far inside its margin a cell may come and still be kept: a rounding, not room.
const GRAZE: f64 = 1e-9;

/// The most buckets along each side of the grid that holds the edges near each cell.
const MAX_BUCKETS: f64 = 256.0;

/// A piece of the field's boundary: a line, an arc of a circle, or any other curve (a conic or a
/// Bézier), each measured on itself.
#[derive(Clone, Debug)]
pub(crate) enum Edge {
    Line(Vec2, Vec2),
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// A curve (a conic or a Bézier) and its chords: a polyline through points of it, each chord
    /// within [`CHORD`] of the curve (see `flatten`).
    Curve {
        seg: Seg2,
        chords: Vec<Vec2>,
    },
}

/// The segment `seg` moved as the planner moves the field into the lattice frame: turned by
/// `turn` about the origin, then shifted.
pub(crate) fn moved_seg(seg: &Seg2, turn: f64, shift: Vec2) -> Seg2 {
    let (s, c) = turn.sin_cos();
    let f = |p: Vec2| Vec2::new(c * p.x - s * p.y, s * p.x + c * p.y) + shift;
    match *seg {
        Seg2::Line { a, b } => Seg2::Line { a: f(a), b: f(b) },
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Seg2::Arc {
            center: f(center),
            radius,
            start: start + turn,
            sweep,
        },
        Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic {
            p0: f(p0),
            p1: f(p1),
            p2: f(p2),
            p3: f(p3),
        },
        Seg2::Conic { a, apex, b, w } => Seg2::Conic {
            a: f(a),
            apex: f(apex),
            b: f(b),
            w,
        },
        Seg2::Bezier { n, p } => {
            let mut q = p;
            for v in &mut q {
                *v = f(*v);
            }
            Seg2::Bezier { n, p: q }
        }
    }
}

fn unit(a: f64) -> Vec2 {
    Vec2::new(a.cos(), a.sin())
}

fn perp(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// Whether an arc from the angle `start` sweeping `sweep` passes the direction at `angle`.
fn sweeps(start: f64, sweep: f64, angle: f64) -> bool {
    if sweep.abs() >= TAU {
        return true;
    }
    let (from, span) = if sweep >= 0.0 {
        (start, sweep)
    } else {
        (start + sweep, -sweep)
    };
    (angle - from).rem_euclid(TAU) <= span
}

/// The distance from `p` to the segment `ab`, worked out from the nearer end so that a segment
/// far from the origin keeps its clearance: the perpendicular is `|cross(f, d)| / |d|` with `f`
/// the vector from `p` to the nearer end, not the difference of two large coordinates. Within
/// `4 u (|f| + |d|)` of the true distance (`u` the unit roundoff).
pub(super) fn to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let (o, e) = if (a - p).dot(a - p) <= (b - p).dot(b - p) {
        (a, b)
    } else {
        (b, a)
    };
    let (f, d) = (o - p, e - o);
    let len2 = d.dot(d);
    if !(len2 > 0.0) {
        return f.length();
    }
    // The foot's place along `o -> e`, in lengths of `d`; the nearer end only leaves it if <= 0.
    let t = -f.dot(d) / len2;
    if t <= 0.0 {
        f.length()
    } else if t >= 1.0 {
        (f + d).length()
    } else {
        cross2(f, d).abs() / len2.sqrt()
    }
}

/// The distance between segments `ab` and `cd` (0 where they cross).
fn segment_distance(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    let side = |p: Vec2, q: Vec2, r: Vec2| cross2(q - p, r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return 0.0;
    }
    to_segment(a, c, d)
        .min(to_segment(b, c, d))
        .min(to_segment(c, a, b))
        .min(to_segment(d, a, b))
}

/// Whether a curve whose polyline is `chords` comes within `limit` of what `dist` measures a
/// chord's distance to. Every chord lies within [`CHORD`] of the curve (and a point of the curve
/// within [`CHORD`] of a chord), so the curve is at least the chords' distance less [`CHORD`]
/// away: the curve counts as within when that bound does not clear `limit`, a decision on the
/// dropping side, short of the curve's own distance by at most `2 * CHORD`. A distance that is
/// not a number counts as within.
fn chords_within(chords: &[Vec2], limit: f64, dist: impl Fn(Vec2, Vec2) -> f64) -> bool {
    let nearest = chords
        .windows(2)
        .filter_map(|w| Some(dist(*w.first()?, *w.get(1)?)))
        .fold(
            f64::INFINITY,
            |m, d| if d < m || d.is_nan() { d } else { m },
        );
    !(nearest - CHORD > limit)
}

impl Edge {
    /// The edge of a segment (in the frame it is given in).
    pub fn of(seg: &Seg2) -> Option<Edge> {
        Some(match *seg {
            Seg2::Line { a, b } => Edge::Line(a, b),
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Edge::Arc {
                center,
                radius,
                start,
                sweep,
            },
            other => Edge::Curve {
                seg: other,
                chords: super::flatten(&other, CHORD)?,
            },
        })
    }

    fn start(&self) -> Vec2 {
        match *self {
            Edge::Line(a, _) => a,
            Edge::Arc {
                center,
                radius,
                start,
                ..
            } => center + unit(start) * radius,
            Edge::Curve { ref seg, .. } => seg.start(),
        }
    }

    fn end(&self) -> Vec2 {
        match *self {
            Edge::Line(_, b) => b,
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + unit(start + sweep) * radius,
            Edge::Curve { ref seg, .. } => seg.end(),
        }
    }

    /// Its box (`[x0, y0, x1, y1]`): its ends, the points of an arc's circle furthest along each
    /// axis that the arc passes, and a curve's chords' box a little grown (the curve lies within
    /// [`CHORD`] of them).
    fn bounds(&self) -> [f64; 4] {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut add = |p: Vec2| {
            b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        };
        add(self.start());
        add(self.end());
        match *self {
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                for k in 0..4 {
                    let a = f64::from(k) * FRAC_PI_2;
                    if sweeps(start, sweep, a) {
                        add(center + unit(a) * radius);
                    }
                }
            }
            Edge::Curve { ref chords, .. } => {
                for &p in chords {
                    add(p);
                }
                b = [
                    b[0] - 2.0 * CHORD,
                    b[1] - 2.0 * CHORD,
                    b[2] + 2.0 * CHORD,
                    b[3] + 2.0 * CHORD,
                ];
            }
            Edge::Line(..) => {}
        }
        b
    }

    /// Whether the edge comes within `limit` of the point `p` (a limit or a point that is not a
    /// number counts as within). Lines and arcs are measured in closed form, a curve on its chords.
    pub fn within_point(&self, p: Vec2, limit: f64) -> bool {
        match *self {
            Edge::Line(a, b) => !(to_segment(p, a, b) > limit),
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let v = p - center;
                let d = if sweeps(start, sweep, v.y.atan2(v.x)) {
                    (v.length() - radius).abs()
                } else {
                    p.distance(self.start()).min(p.distance(self.end()))
                };
                !(d > limit)
            }
            Edge::Curve { ref chords, .. } => {
                chords_within(chords, limit, |a, b| to_segment(p, a, b))
            }
        }
    }

    /// Whether the edge comes within `limit` of the segment `ab` (0 where they meet).
    pub fn within_segment(&self, a: Vec2, b: Vec2, limit: f64) -> bool {
        match *self {
            Edge::Line(p, q) => !(segment_distance(a, b, p, q) > limit),
            Edge::Curve { ref chords, .. } => {
                chords_within(chords, limit, |p, q| segment_distance(a, b, p, q))
            }
            Edge::Arc { .. } => !(self.arc_to_segment(a, b) > limit),
        }
    }

    /// The distance from an arc edge to the segment `ab` (0 where they meet).
    ///
    /// Worked out from the segment's nearer end to the circle's centre, `f`, along `d` to the
    /// other end, with `u = d / |d|`: the line's distance from the centre is
    /// `h = |cross(u, f)|`, not a discriminant `(f.d)^2 - |d|^2 (|f|^2 - r^2)`, whose terms are
    /// each of the size of the coordinates squared and lose the radius at large coordinates.
    /// `h` is within `4 u |f|` of the true value (`u` the unit roundoff, `|f| <= |f + d|`); the
    /// line is taken to meet the circle when `h <= r + 4 u |f|`, and the crossings to lie on the
    /// segment when their place along it is within `16 u (|f| + |d|)` of `[0, |d|]`, so the
    /// error is on the side of a hit by that much. The crossing points relative to the centre are
    /// `n * cross(u, f) +- u * sqrt((r - h)(r + h))`, of the size of the radius.
    fn arc_to_segment(&self, a: Vec2, b: Vec2) -> f64 {
        let Edge::Arc {
            center,
            radius,
            start,
            sweep,
        } = *self
        else {
            return f64::NAN;
        };
        let (o, e) = if (a - center).dot(a - center) <= (b - center).dot(b - center) {
            (a, b)
        } else {
            (b, a)
        };
        let (f, d) = (o - center, e - o);
        let len = d.length();
        let mut m = self
            .end_distance(a)
            .min(self.end_distance(b))
            .min(to_segment(self.start(), a, b))
            .min(to_segment(self.end(), a, b));
        if !(len > 0.0) {
            return m;
        }
        let u = d / len;
        let n = perp(u);
        let side = cross2(u, f);
        let h = side.abs();
        // Where along the segment, from `o`, the line comes nearest the centre.
        let foot_at = -f.dot(u);
        let foot = n * side;
        let tol_h = 4.0 * f64::EPSILON * f.length();
        let tol_t = 16.0 * f64::EPSILON * (f.length() + len);
        if h <= radius + tol_h {
            let half = if h < radius {
                ((radius - h) * (radius + h)).sqrt()
            } else {
                0.0
            };
            for place in [foot_at - half, foot_at + half] {
                if place >= -tol_t && place <= len + tol_t {
                    let q = foot + u * (place - foot_at);
                    if sweeps(start, sweep, q.y.atan2(q.x)) {
                        return 0.0;
                    }
                }
            }
        }
        // Nearest where the segment comes nearest the circle's centre.
        if h > 1e-300
            && foot_at > 0.0
            && foot_at < len
            && sweeps(start, sweep, foot.y.atan2(foot.x))
        {
            m = m.min((h - radius).abs());
        }
        m
    }

    /// The distance from the point `p` to this arc edge as a curve (its ends where the direction
    /// is not on the arc).
    fn end_distance(&self, p: Vec2) -> f64 {
        let Edge::Arc {
            center,
            radius,
            start,
            sweep,
        } = *self
        else {
            return f64::NAN;
        };
        let v = p - center;
        if sweeps(start, sweep, v.y.atan2(v.x)) {
            (v.length() - radius).abs()
        } else {
            p.distance(self.start()).min(p.distance(self.end()))
        }
    }
}

/// A cell's outline for a nearness test.
pub(crate) enum CellOutline<'a> {
    /// A round cell: its true circle.
    Circle { c: Vec2, r: f64 },
    /// A cell with straight sides.
    Polygon(&'a Convex),
}

/// The field's boundary edges, bucketed over the field so a cell looks only at the edges near it.
pub(crate) struct ExactField {
    edges: Vec<Edge>,
    boxes: Vec<[f64; 4]>,
    lo: Vec2,
    size: f64,
    nx: usize,
    ny: usize,
    buckets: Vec<Vec<usize>>,
}

impl ExactField {
    /// The edges `edges`, bucketed at least `pitch` wide.
    pub fn new(edges: Vec<Edge>, pitch: f64) -> ExactField {
        let boxes: Vec<[f64; 4]> = edges.iter().map(Edge::bounds).collect();
        let (mut lo, mut hi) = (
            Vec2::new(f64::INFINITY, f64::INFINITY),
            Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for b in &boxes {
            let [x0, y0, x1, y1] = *b;
            lo = Vec2::new(lo.x.min(x0), lo.y.min(y0));
            hi = Vec2::new(hi.x.max(x1), hi.y.max(y1));
        }
        if !(lo.x.is_finite() && lo.y.is_finite() && hi.x.is_finite() && hi.y.is_finite()) {
            return ExactField {
                edges: Vec::new(),
                boxes: Vec::new(),
                lo: Vec2::ZERO,
                size: 1.0,
                nx: 1,
                ny: 1,
                buckets: vec![Vec::new()],
            };
        }
        let size = (2.0 * pitch)
            .max((hi.x - lo.x) / MAX_BUCKETS)
            .max((hi.y - lo.y) / MAX_BUCKETS)
            .max(1e-6);
        let (nx, ny) = (
            ((hi.x - lo.x) / size) as usize + 1,
            ((hi.y - lo.y) / size) as usize + 1,
        );
        let mut field = ExactField {
            edges,
            boxes,
            lo,
            size,
            nx,
            ny,
            buckets: vec![Vec::new(); nx * ny],
        };
        for k in 0..field.edges.len() {
            let Some(&b) = field.boxes.get(k) else {
                continue;
            };
            let ((i0, j0), (i1, j1)) = (field.index(b[0], b[1]), field.index(b[2], b[3]));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    if let Some(bucket) = field.buckets.get_mut(j * nx + i) {
                        bucket.push(k);
                    }
                }
            }
        }
        field
    }

    fn index(&self, x: f64, y: f64) -> (usize, usize) {
        let clamp = |v: f64, n: usize| (v.floor().max(0.0) as usize).min(n.saturating_sub(1));
        (
            clamp((x - self.lo.x) / self.size, self.nx),
            clamp((y - self.lo.y) / self.size, self.ny),
        )
    }

    /// Whether any edge reaches `outline`: it meets the outline shrunk by [`EPS`] (crosses it, or
    /// lies inside it), or, with a `margin`, lies nearer than that to it. Measured exactly, to
    /// lines and arcs in closed form and to other curves on the curves themselves.
    pub fn meets(&self, outline: &CellOutline, margin: f64) -> bool {
        let reach = margin.max(0.0) + 4.0 * CHORD;
        let (b0, b1, b2, b3) = match *outline {
            CellOutline::Circle { c, r } => (c.x - r, c.y - r, c.x + r, c.y + r),
            CellOutline::Polygon(p) => (p.bounds.x0, p.bounds.y0, p.bounds.x1, p.bounds.y1),
        };
        let ((i0, j0), (i1, j1)) = (
            self.index(b0 - reach, b1 - reach),
            self.index(b2 + reach, b3 + reach),
        );
        let mut near: Vec<usize> = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                if let Some(bucket) = self.buckets.get(j * self.nx + i) {
                    near.extend_from_slice(bucket);
                }
            }
        }
        near.sort_unstable();
        near.dedup();
        let inner = match outline {
            CellOutline::Polygon(p) => Some(p.shrunk(EPS)),
            CellOutline::Circle { .. } => None,
        };
        for k in near {
            let (Some(edge), Some(&bx)) = (self.edges.get(k), self.boxes.get(k)) else {
                continue;
            };
            if bx[0] > b2 + reach || bx[2] < b0 - reach || bx[1] > b3 + reach || bx[3] < b1 - reach
            {
                continue;
            }
            // A clearance that is not a number counts as a hit: the cell is dropped, not kept.
            let hit = match (outline, &inner) {
                (CellOutline::Circle { c, r }, _) => {
                    let shrunk = (r - EPS).max(0.0);
                    let limit = if margin > 0.0 {
                        shrunk.max(r + margin - GRAZE)
                    } else {
                        shrunk
                    };
                    edge.within_point(*c, limit)
                }
                (CellOutline::Polygon(p), Some(inner)) => {
                    polygon_within(inner, edge, 0.0)
                        || (margin > 0.0 && polygon_within(p, edge, margin - GRAZE))
                }
                (CellOutline::Polygon(_), None) => true,
            };
            if hit {
                return true;
            }
        }
        false
    }
}

/// Whether `edge` comes within `limit` of the convex polygon `poly`: it lies inside it or crosses
/// it, or comes that near a side.
fn polygon_within(poly: &Convex, edge: &Edge, limit: f64) -> bool {
    if poly.contains(edge.start(), 0.0) {
        return true;
    }
    let n = poly.pts.len();
    for k in 0..n {
        let (Some(&a), Some(&b)) = (poly.pts.get(k), poly.pts.get(ring_next(k, n))) else {
            continue;
        };
        if edge.within_segment(a, b, limit) {
            return true;
        }
    }
    false
}
