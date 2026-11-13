//! Where a sketch dimension is drawn: extension lines from the measured geometry, the dimension
//! line (or arc) with its arrows, and the value text. Shared by the UI (drawing and hit testing)
//! and the engine (storing a dragged text position), all in sketch coordinates.
//!
//! A dimension's text position is kept relative to its own frame ([`Constraint::text`]), so it
//! follows the geometry: for a linear dimension it is (along, across) from the middle of the
//! measured points, for the others an offset from the centre or the vertex.
//!
//! [`Constraint::text`]: crate::Constraint::text

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::model::{ConstraintKind, CurveKind, MAX_COORD, Sketch, TwoCircles};
use crate::union_find::{find, union};

/// What a dimension measures, in a form that can be drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimFrame {
    /// The distance from `p0` to `p1` along the unit direction `dir` (aligned, horizontal or
    /// vertical).
    Linear { p0: Vec2, p1: Vec2, dir: Vec2 },
    /// Radius or diameter of a circle or arc. `arc` is the arc's (start angle, counter-clockwise
    /// sweep), None for a full circle: an arrow past the arc's ends gets an extension of the arc.
    Radial { center: Vec2, radius: f64, diameter: bool, arc: Option<(f64, f64)> },
    /// The angle at `vertex` from the unit ray `d0`, counter-clockwise by `sweep` radians.
    Angular { vertex: Vec2, d0: Vec2, sweep: f64 },
    /// Length along an arc around `center` from angle `start` by `sweep`.
    ArcLength { center: Vec2, radius: f64, start: f64, sweep: f64 },
    /// An ellipse's minor half-axis: `len` from `center` along the unit `dir` or against it,
    /// drawn as a linear dimension to the minor axis end on the text's side, as the distance
    /// to that end is when the ellipse has its axes. The text is stored from `center`, as a
    /// radius's is, so a radius placed before it was drawn this way keeps its text.
    HalfAxis { center: Vec2, dir: Vec2, len: f64 },
}

/// A drawn dimension (sketch coordinates).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimLayout {
    /// Extension lines, dimension lines and arcs as polylines.
    pub lines: Vec<Vec<Vec2>>,
    /// Arrowheads: tip and the unit direction they point in.
    pub arrows: Vec<(Vec2, Vec2)>,
    /// Centre of the value text.
    pub text: Vec2,
}

fn line_ends(sk: &Sketch, l: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(l)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

/// The frame of a dimension constraint (None for geometric constraints).
pub fn dim_frame(sk: &Sketch, k: &ConstraintKind) -> Option<DimFrame> {
    use ConstraintKind::*;
    let unit = |v: Vec2, or: Vec2| v.normalized().unwrap_or(or);
    Some(match *k {
        OffsetDistance { ref output, .. } => {
            let (p0, p1) = sk.offset_dimension_points(output)?;
            DimFrame::Linear { p0, p1, dir: unit(p1 - p0, Vec2::X) }
        }
        Distance { p, q, .. } => {
            let (a, b) = (sk.point(p)?, sk.point(q)?);
            DimFrame::Linear { p0: a, p1: b, dir: unit(b - a, Vec2::X) }
        }
        DistanceX { p, q, .. } => DimFrame::Linear { p0: sk.point(p)?, p1: sk.point(q)?, dir: Vec2::X },
        DistanceY { p, q, .. } => DimFrame::Linear { p0: sk.point(p)?, p1: sk.point(q)?, dir: Vec2::Y },
        Length { l, .. } => {
            let (a, b) = line_ends(sk, l)?;
            DimFrame::Linear { p0: a, p1: b, dir: unit(b - a, Vec2::X) }
        }
        LineAngle { l, value } => {
            let (a, b) = line_ends(sk, l)?;
            let d = unit(b - a, Vec2::X);
            if value < 0.0 {
                DimFrame::Angular { vertex: a, d0: d, sweep: (-value).rem_euclid(std::f64::consts::TAU) }
            } else {
                DimFrame::Angular { vertex: a, d0: Vec2::X, sweep: value.rem_euclid(std::f64::consts::TAU) }
            }
        }
        ArcSweep { c, .. } => match sk.segs(c).first()? {
            solidcore_record::Seg2::Arc { center, start, sweep, .. } => {
                DimFrame::Angular { vertex: *center, d0: Vec2::from_angle(*start), sweep: *sweep }
            }
            _ => return None,
        },
        SlotOverallLength { a, b, .. } => {
            let (ca, cb) = (sk.center(a)?, sk.center(b)?);
            let dir = unit(cb - ca, Vec2::X);
            DimFrame::Linear { p0: ca - dir * sk.radius(a)?, p1: cb + dir * sk.radius(b)?, dir }
        }
        PointLineDistance { p, l, .. } | LinearDiameter { p, l, .. } => {
            let (a, b) = line_ends(sk, l)?;
            let q = sk.point(p)?;
            let d = unit(b - a, Vec2::X);
            let foot = a + d * (q - a).dot(d);
            let p0 = if matches!(k, LinearDiameter { .. }) { foot * 2.0 - q } else { foot };
            DimFrame::Linear { p0, p1: q, dir: unit(q - foot, d.perp()) }
        }
        LineCircleDistance { l, c, .. } => {
            // From the line to the circle's edge point nearest it (beyond it when the line cuts
            // the circle), along the line's normal.
            let (a, b) = line_ends(sk, l)?;
            let (centre, r) = (sk.center(c)?, sk.radius(c)?);
            let d = unit(b - a, Vec2::X);
            let normal = unit(centre - (a + d * (centre - a).dot(d)), d.perp());
            let p1 = centre - normal * r;
            let p0 = a + d * (p1 - a).dot(d);
            DimFrame::Linear { p0, p1, dir: unit(p1 - p0, normal) }
        }
        PointCircleDistance { p, c, .. } => {
            // From the circle's edge, where the radius through the point crosses it, to the point.
            let q = sk.point(p)?;
            let (centre, r) = (sk.center(c)?, sk.radius(c)?);
            let u = unit(q - centre, Vec2::X);
            let edge = centre + u * r;
            DimFrame::Linear { p0: edge, p1: q, dir: unit(q - edge, u) }
        }
        CircleCircleDistance { a, b, circles, .. } => {
            // Along the line through the centres, between the edge points that face each other:
            // for nested circles, where the inner one comes nearest the outer one.
            let ((ca, ra), (cb, rb)) = ((sk.center(a)?, sk.radius(a)?), (sk.center(b)?, sk.radius(b)?));
            let u = unit(cb - ca, Vec2::X);
            let (p0, p1) = match circles {
                TwoCircles::Nested if ra >= rb => (cb + u * rb, ca + u * ra),
                TwoCircles::Nested => (ca - u * ra, cb - u * rb),
                TwoCircles::Apart | TwoCircles::Crossing => (ca + u * ra, cb - u * rb),
            };
            DimFrame::Linear { p0, p1, dir: u }
        }
        ConcentricDistance { a, b, .. } => {
            let center = sk.center(a)?;
            let ra = sk.radius(a)?;
            let rb = sk.radius(b)?;
            let mid_angle = match sk.curves.get(a).map(|c| &c.kind) {
                Some(CurveKind::Arc { .. }) => match sk.segs(a).first() {
                    Some(solidcore_record::Seg2::Arc { start, sweep, .. }) => start + sweep * 0.5,
                    _ => std::f64::consts::FRAC_PI_4,
                },
                _ => match sk.curves.get(b).map(|c| &c.kind) {
                    Some(CurveKind::Arc { .. }) => match sk.segs(b).first() {
                        Some(solidcore_record::Seg2::Arc { start, sweep, .. }) => start + sweep * 0.5,
                        _ => std::f64::consts::FRAC_PI_4,
                    },
                    _ => std::f64::consts::FRAC_PI_4,
                },
            };
            let u = Vec2::from_angle(mid_angle);
            let p0 = center + u * ra;
            let p1 = center + u * rb;
            DimFrame::Linear { p0, p1, dir: unit(p1 - p0, u) }
        }
        Radius { c, .. } | Diameter { c, .. } => {
            if let (Radius { .. }, CurveKind::Ellipse { c: centre, m, r }) = (k, &sk.curves.get(c)?.kind) {
                let center = sk.point(*centre)?;
                return Some(DimFrame::HalfAxis { center, dir: unit(sk.point(*m)? - center, Vec2::X).perp(), len: *r });
            }
            let (radius, arc) = match sk.curves.get(c)?.kind {
                CurveKind::Ellipse { r, .. } => (r, None),
                CurveKind::Arc { .. } => match sk.segs(c).first()? {
                    solidcore_record::Seg2::Arc { radius, start, sweep, .. } => (*radius, Some((*start, *sweep))),
                    _ => return None,
                },
                _ => (sk.radius(c)?, None),
            };
            DimFrame::Radial { center: sk.center(c)?, radius, diameter: matches!(k, Diameter { .. }), arc }
        }
        ArcLength { c, .. } => match sk.segs(c).first()? {
            solidcore_record::Seg2::Arc { center, radius, start, sweep } => {
                DimFrame::ArcLength { center: *center, radius: *radius, start: *start, sweep: *sweep }
            }
            _ => return None,
        },
        Angle { a, b, flip, .. } => {
            let (a0, a1) = line_ends(sk, a)?;
            let (b0, b1) = line_ends(sk, b)?;
            let (da, mut db) = (unit(a1 - a0, Vec2::X), unit(b1 - b0, Vec2::Y));
            if flip {
                db = -db;
            }
            let den = da.cross(db);
            let (ma, mb) = ((a0 + a1) * 0.5, (b0 + b1) * 0.5);
            let vertex = if den.abs() < 1e-12 { (ma + mb) * 0.5 } else { a0 + da * ((b0 - a0).cross(db) / den) };
            // The angle from `da` to `db` equals the one from `-da` to `-db`: draw the pair of
            // rays that point at the lines themselves.
            let score = |s: f64| (ma - vertex).dot(da * s) + (mb - vertex).dot(db * s);
            let s = if score(1.0) >= score(-1.0) { 1.0 } else { -1.0 };
            let (d0, d1) = (da * s, db * s);
            DimFrame::Angular { vertex, d0, sweep: d0.cross(d1).atan2(d0.dot(d1)).rem_euclid(std::f64::consts::TAU) }
        }
        _ => return None,
    })
}

/// The frame of a dimension constraint drawn with its stored text `text` (frame-relative, as
/// in [`encode_text`]). An angle between two lines measures the same value in two vertically
/// opposite sectors around their crossing; its arc goes in the one the text sits in (#11800),
/// otherwise in the one whose rays point at the lines, as [`dim_frame`] has it.
pub fn dim_frame_at(sk: &Sketch, k: &ConstraintKind, text: Option<Vec2>) -> Option<DimFrame> {
    let frame = dim_frame(sk, k)?;
    let (ConstraintKind::Angle { .. }, DimFrame::Angular { vertex, d0, sweep }, Some(t)) = (k, frame, text) else {
        return Some(frame);
    };
    let inside = |d0: Vec2| t.len() > 1e-12 && t.is_finite() && (t.angle() - d0.angle()).rem_euclid(std::f64::consts::TAU) <= sweep;
    Some(if !inside(d0) && inside(-d0) { DimFrame::Angular { vertex, d0: -d0, sweep } } else { frame })
}

/// The stored text position for text centred at `at`.
pub fn encode_text(f: &DimFrame, at: Vec2) -> Vec2 {
    match *f {
        DimFrame::Linear { p0, p1, dir } => {
            let m = (p0 + p1) * 0.5;
            Vec2::new((at - m).dot(dir), (at - m).dot(dir.perp()))
        }
        DimFrame::Radial { center, .. } | DimFrame::ArcLength { center, .. } => at - center,
        DimFrame::Angular { vertex, .. } => at - vertex,
        DimFrame::HalfAxis { center, .. } => at - center,
    }
}

/// The linear frame a [`DimFrame::HalfAxis`] is drawn as for the stored text `t`: to the end
/// on the text's side, with `t` re-expressed for that frame.
fn half_axis_side(center: Vec2, dir: Vec2, len: f64, t: Vec2) -> (DimFrame, Vec2) {
    let dir = if t.dot(dir) < 0.0 { -dir } else { dir };
    let linear = DimFrame::Linear { p0: center, p1: center + dir * len, dir };
    (linear, encode_text(&linear, center + t))
}

/// Per point: the centre of the connected geometry it belongs to (points joined by curves),
/// which a dimension's default place keeps away from.
pub fn chain_centres(sk: &Sketch) -> Vec<Vec2> {
    let n = sk.points.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for c in &sk.curves {
        let ids = c.kind.point_ids();
        for w in ids.windows(2) {
            union(&mut parent, w[0], w[1]);
        }
    }
    let mut sum = vec![(Vec2::ZERO, 0usize); n];
    let roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    for (i, r) in roots.iter().enumerate() {
        if let (Some(s), Some(p)) = (sum.get_mut(*r), sk.points.get(i)) {
            s.0 += p.pos;
            s.1 += 1;
        }
    }
    roots.iter().map(|r| sum.get(*r).map(|(s, k)| *s / (*k).max(1) as f64).unwrap_or_default()).collect()
}

/// The default text place of a dimension: a linear one on the side of its points away from
/// `away` (the centre of the geometry it measures), the others where [`dim_layout`] puts them.
/// `half` is the half size of the text box (sketch units): the text sits far enough out that
/// its box stays clear of the measured points (#12482).
pub fn default_text(f: &DimFrame, away: Vec2, px: f64, half: Vec2) -> Option<Vec2> {
    match *f {
        DimFrame::Linear { p0, p1, dir } => {
            let side = if ((p0 + p1) * 0.5 - away).dot(dir.perp()) < -1e-9 { -1.0 } else { 1.0 };
            Some(Vec2::new(0.0, side * linear_offset(dir, px, half)))
        }
        _ => None,
    }
}

/// How far across from its measured points a linear dimension's unplaced text sits: 22 pixels,
/// or more when the text box (drawn level on screen, `half` its half size) reaches further
/// across `dir` than that, so the box keeps 8 pixels clear of the measured line.
fn linear_offset(dir: Vec2, px: f64, half: Vec2) -> f64 {
    let n = dir.perp();
    let across = half.x.abs() * n.x.abs() + half.y.abs() * n.y.abs();
    let across = if across.is_finite() { across } else { 0.0 };
    (22.0 * px).max(across + 8.0 * px)
}

/// The text offset [`dim_layout_reaching`] uses when a dimension's text was never placed:
/// frame-relative, as in [`encode_text`]. `half` is the half size of the text box.
pub fn default_text_offset(f: &DimFrame, px: f64, half: Vec2) -> Vec2 {
    match *f {
        DimFrame::Linear { dir, .. } => Vec2::new(0.0, linear_offset(dir, px, half)),
        // Where a linear dimension to the minor axis end puts it.
        DimFrame::HalfAxis { dir, len, .. } => dir * (len * 0.5) + dir.perp() * linear_offset(dir, px, half),
        DimFrame::Radial { radius, .. } => Vec2::new(1.0, 1.0) * ((radius + 18.0 * px) * std::f64::consts::FRAC_1_SQRT_2),
        DimFrame::Angular { d0, sweep, .. } => Vec2::from_angle(d0.angle() + sweep * 0.5) * (48.0 * px),
        DimFrame::ArcLength { radius, start, sweep, .. } => Vec2::from_angle(start + sweep * 0.5) * (radius + 20.0 * px),
    }
}

/// Points along an arc around `c` (counter-clockwise from `a0` by `sweep`).
fn arc_pts(c: Vec2, r: f64, a0: f64, sweep: f64) -> Vec<Vec2> {
    let n = ((sweep.abs() / 0.08).ceil() as usize).clamp(2, 200);
    (0..=n).map(|i| c + Vec2::from_angle(a0 + sweep * i as f64 / n as f64) * r).collect()
}

/// The arc around `c` from the nearer end of the arc (`start`, counter-clockwise by `sweep`) to
/// angle `at`, and `over` past it; None when `at` lies on the arc.
fn arc_extension(c: Vec2, r: f64, start: f64, sweep: f64, at: f64, over: f64) -> Option<Vec<Vec2>> {
    if !(r > 0.0 && r.is_finite() && sweep.is_finite() && at.is_finite()) {
        return None;
    }
    let ta = (at - start).rem_euclid(std::f64::consts::TAU);
    if ta <= sweep + 1e-9 {
        return None;
    }
    let past = over / r;
    let (after_end, before_start) = (ta - sweep, std::f64::consts::TAU - ta);
    Some(if after_end <= before_start { arc_pts(c, r, start + sweep, after_end + past) } else { arc_pts(c, r, at - past, before_start + past) })
}

/// The lines a dimension's two extension lines may start from (#1361), for its `p0` and `p1`
/// sides: a dimension to a line reaches the nearest part of that line, not the foot of the
/// measured point, and a dimension between two parallel lines reaches the nearest part of the
/// line whose end it measures from. An angle dimension's two rays reach the lines it measures
/// (#15610). `None` for a side that measures a point.
pub fn dim_reach(sk: &Sketch, k: &ConstraintKind) -> [Option<(Vec2, Vec2)>; 2] {
    use ConstraintKind::*;
    let (p, l, to_line) = match *k {
        PointLineDistance { p, l, .. } => (p, l, true),
        LinearDiameter { p, l, .. } => (p, l, false),
        LineCircleDistance { l, .. } => return [line_ends(sk, l), None],
        ConcentricDistance { .. } => return [None, None],
        Angle { a, b, .. } => return [line_ends(sk, a), line_ends(sk, b)],
        _ => return [None, None],
    };
    let Some((a, b)) = line_ends(sk, l) else { return [None, None] };
    let d = b - a;
    // A line ending at `p` that runs parallel to `l`.
    let parallel = sk.curves.iter().enumerate().find_map(|(i, c)| match c.kind {
        CurveKind::Line { a: pa, b: pb } if i != l && (pa == p || pb == p) => {
            let (u, v) = line_ends(sk, i)?;
            let e = v - u;
            (e.len() > 1e-12 && d.cross(e).abs() <= 1e-9 * d.len() * e.len()).then_some((u, v))
        }
        _ => None,
    });
    [to_line.then_some((a, b)), parallel]
}

/// The point of segment `s` nearest `q`.
fn nearest_on(s: (Vec2, Vec2), q: Vec2) -> Vec2 {
    let d = s.1 - s.0;
    let l2 = d.dot(d);
    if !(l2 > 1e-24) {
        return s.0;
    }
    s.0 + d * ((q - s.0).dot(d) / l2).clamp(0.0, 1.0)
}

/// How far the ray from `vertex` along `u` reaches the segment `s`: the distance to the point
/// of `s` on the ray's line, negative when that point lies behind the vertex. Zero when the
/// segment does not lie on the ray's line, as when the two lines are parallel and have no
/// vertex to meet at.
fn ray_reach(vertex: Vec2, u: Vec2, s: (Vec2, Vec2)) -> f64 {
    let d = nearest_on(s, vertex) - vertex;
    let along = d.dot(u);
    if (d - u * along).len() > 1e-6 * (1.0 + along.abs()) {
        return 0.0;
    }
    along
}

/// How far a line from the centre of an upright text box of half size `half` runs along the
/// unit `u` before it leaves the box. An axis `u` doesn't move along gives `h / 0` (infinite,
/// or NaN for an empty box), which `min` passes over.
fn box_exit(half: Vec2, u: Vec2) -> f64 {
    (half.x / u.x.abs()).min(half.y / u.y.abs())
}

/// Lay a dimension out. `text` is the stored text position (None: the default place), `px`
/// the sketch length of one screen pixel and `half` the half size of the text box (sketch
/// units), which the dimension line leaves a gap for.
pub fn dim_layout(f: &DimFrame, text: Option<Vec2>, px: f64, half: Vec2) -> DimLayout {
    dim_layout_reaching(f, text, px, half, [None, None])
}

/// [`dim_layout`] with the extension lines of a linear dimension starting from the nearest
/// point of the geometry in `reach` ([`dim_reach`]) instead of the measured points, and an
/// angle dimension's rays reaching the lines they measure.
pub fn dim_layout_reaching(f: &DimFrame, text: Option<Vec2>, px: f64, half: Vec2, reach: [Option<(Vec2, Vec2)>; 2]) -> DimLayout {
    let px = if px.is_finite() && px > 0.0 { px } else { 1.0 };
    // A text box is no larger than a sketch can be.
    let size = |h: f64| if h.is_finite() { h.clamp(0.0, MAX_COORD) } else { 0.0 };
    let half = Vec2::new(size(half.x), size(half.y));
    let arrow = 9.0 * px;
    let gap = 3.0 * px;
    let over = 4.0 * px;
    let mut out = DimLayout::default();
    match *f {
        DimFrame::HalfAxis { center, dir, len } => {
            let t = text.unwrap_or_else(|| default_text_offset(f, px, half));
            let (linear, t) = half_axis_side(center, dir, len, t);
            return dim_layout_reaching(&linear, Some(t), px, half, reach);
        }
        DimFrame::Linear { p0, p1, dir } => {
            let n = dir.perp();
            let m = (p0 + p1) * 0.5;
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Linear { p0, p1, dir }, px, half));
            let (s, h) = (t.x, t.y);
            // The dimension line sits `h` across from the middle of the measured points.
            let q0 = p0 + n * (h - (p0 - m).dot(n));
            let q1 = p1 + n * (h - (p1 - m).dot(n));
            for ((p, q), r) in [(p0, q0), (p1, q1)].into_iter().zip(reach) {
                let p = r.map_or(p, |r| nearest_on(r, q));
                let v = q - p;
                if let Some(u) = v.normalized() {
                    out.lines.push(vec![p + u * gap.min(v.len()), q + u * over]);
                }
            }
            out.text = m + n * h + dir * s;
            let len = (q1 - q0).dot(dir);
            let (lo, hi) = if len >= 0.0 { (q0, q1) } else { (q1, q0) };
            let span = len.abs();
            // The text is drawn level on screen: it fits between the arrows when its whole box
            // does (`along`, its farthest corner), and the line stops where it meets the box.
            let along = (half.x * dir.x.abs() + half.y * dir.y.abs()) + gap;
            let meet = box_exit(half, dir) + gap;
            let inside = s.abs() + along <= span * 0.5;
            if span > 2.0 * arrow + 2.0 * along && inside {
                // Arrows inside, pointing out, the line broken around the text.
                out.arrows.push((lo, -dir));
                out.arrows.push((hi, dir));
                let tc = m + n * h + dir * s;
                out.lines.push(vec![lo, tc - dir * meet]);
                out.lines.push(vec![tc + dir * meet, hi]);
            } else {
                // Too tight (or the text pulled out): arrows outside pointing in, short tails.
                out.arrows.push((lo, dir));
                out.arrows.push((hi, -dir));
                let tail = arrow * 1.8;
                out.lines.push(vec![lo - dir * tail, hi + dir * tail]);
                // A text outside the extension lines gets a leader along the line.
                let tc = m + n * h + dir * s;
                let reach = (tc - m - n * h).dot(dir);
                if reach.abs() > span * 0.5 + tail {
                    let end = if reach > 0.0 { hi + dir * tail } else { lo - dir * tail };
                    out.lines.push(vec![end, tc - dir * (meet * reach.signum())]);
                }
            }
        }
        DimFrame::Radial { center, radius, diameter, arc } => {
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Radial { center, radius, diameter, arc }, px, half));
            let u = t.normalized().unwrap_or(Vec2::new(1.0, 1.0) * std::f64::consts::FRAC_1_SQRT_2);
            let d = t.len();
            let at = center + u * radius;
            out.text = center + t;
            // An arrow past the ends of an arc points at the arc's extension (#3393).
            if let Some((start, sweep)) = arc {
                let tips: &[Vec2] = if diameter { &[u, -u] } else { &[u] };
                for tip in tips {
                    if let Some(ext) = arc_extension(center, radius, start, sweep, tip.angle(), over) {
                        out.lines.push(ext);
                    }
                }
            }
            // The leader stops where it meets the (level) text box.
            let rim = box_exit(half, u) + gap;
            let from = if diameter { center - u * radius } else { center };
            if d > radius {
                // Leader from the far side (or centre) out to the text, arrows on the rim.
                out.lines.push(vec![from, center + u * (d - rim).max(radius)]);
                out.arrows.push((at, u));
                if diameter {
                    out.arrows.push((center - u * radius, -u));
                }
            } else {
                // Text inside: the line runs rim to rim (or centre to rim) around it.
                let end = center + u * radius;
                let t0 = (d - rim).max(0.0);
                let start_d = if diameter { -radius } else { 0.0 };
                if t0 > start_d {
                    out.lines.push(vec![from, center + u * t0]);
                }
                if d + rim < radius {
                    out.lines.push(vec![center + u * (d + rim), end]);
                }
                out.arrows.push((at, u));
                if diameter {
                    out.arrows.push((center - u * radius, -u));
                }
            }
        }
        DimFrame::Angular { vertex, d0, sweep } => {
            let a0 = d0.angle();
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Angular { vertex, d0, sweep }, px, half));
            let r = t.len().max(arrow);
            out.text = vertex + t;
            // Extension rays from the vertex out past the arc, reaching the lines they measure
            // (#15610): two lines that do not meet have their vertex where their extensions
            // cross, far from them, and the rays are what keeps the dimension attached.
            for (a, seg) in [a0, a0 + sweep].into_iter().zip(reach) {
                let u = Vec2::from_angle(a);
                let along = seg.map_or(0.0, |s| ray_reach(vertex, u, s));
                let from = if along < 0.0 { along } else { (r * 0.15).min(gap * 3.0) };
                let to = if along > r + over { along } else { r + over };
                out.lines.push(vec![vertex + u * from, vertex + u * to]);
            }
            let arc_len = r * sweep;
            if arc_len > 2.5 * arrow {
                out.lines.push(arc_pts(vertex, r, a0, sweep));
                out.arrows.push((vertex + Vec2::from_angle(a0) * r, -Vec2::from_angle(a0).perp()));
                out.arrows.push((vertex + Vec2::from_angle(a0 + sweep) * r, Vec2::from_angle(a0 + sweep).perp()));
            } else {
                // A narrow angle: arrows outside the sector pointing in.
                let tail = 2.0 * arrow / r;
                out.lines.push(arc_pts(vertex, r, a0 - tail, sweep + 2.0 * tail));
                out.arrows.push((vertex + Vec2::from_angle(a0) * r, Vec2::from_angle(a0).perp()));
                out.arrows.push((vertex + Vec2::from_angle(a0 + sweep) * r, -Vec2::from_angle(a0 + sweep).perp()));
            }
            // Text beyond the sector: the arc reaches round to it.
            let ta = (t.angle() - a0).rem_euclid(std::f64::consts::TAU);
            if ta > sweep {
                let (from, by) =
                    if ta - sweep < std::f64::consts::TAU - ta { (a0 + sweep, ta - sweep) } else { (t.angle(), std::f64::consts::TAU - ta) };
                out.lines.push(arc_pts(vertex, r, from, by));
            }
        }
        DimFrame::ArcLength { center, radius, start, sweep } => {
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::ArcLength { center, radius, start, sweep }, px, half));
            let r = t.len().max(arrow);
            out.text = center + t;
            for a in [start, start + sweep] {
                let u = Vec2::from_angle(a);
                let (from, to) = if r >= radius { (radius + gap, r + over) } else { (radius - gap, r - over) };
                out.lines.push(vec![center + u * from, center + u * to]);
            }
            out.lines.push(arc_pts(center, r, start, sweep));
            out.arrows.push((center + Vec2::from_angle(start) * r, -Vec2::from_angle(start).perp()));
            out.arrows.push((center + Vec2::from_angle(start + sweep) * r, Vec2::from_angle(start + sweep).perp()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn linear_dimension_has_extension_lines_and_inward_arrows() {
        let f = DimFrame::Linear { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(100.0, 0.0), dir: Vec2::X };
        let l = dim_layout(&f, Some(Vec2::new(0.0, 10.0)), 0.1, Vec2::new(2.0, 1.0));
        assert!(close(l.text, Vec2::new(50.0, 10.0)));
        assert_eq!(l.arrows.len(), 2);
        // Arrows at the dimension line ends, pointing outwards.
        assert!(close(l.arrows[0].0, Vec2::new(0.0, 10.0)) && close(l.arrows[0].1, -Vec2::X));
        assert!(close(l.arrows[1].0, Vec2::new(100.0, 10.0)) && close(l.arrows[1].1, Vec2::X));
        // Two extension lines going up from the points, then the line broken around the text.
        assert_eq!(l.lines.len(), 4);
        assert!(l.lines[0][0].y > 0.0 && l.lines[0][1].y > 10.0);
        assert!(l.lines[2][1].x < 48.0 && l.lines[3][0].x > 52.0);
    }

    #[test]
    fn short_linear_dimension_flips_arrows_outside() {
        let f = DimFrame::Linear { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(2.0, 0.0), dir: Vec2::X };
        let l = dim_layout(&f, None, 0.1, Vec2::new(2.0, 1.0));
        assert!(close(l.arrows[0].1, Vec2::X) && close(l.arrows[1].1, -Vec2::X));
    }

    #[test]
    fn unplaced_linear_text_sits_beside_the_line_it_measures() {
        // #12482: the value is drawn level on screen, so on a vertical line a wide value reached
        // back over the line from the fixed 22 px default place. One pixel is 0.1 here and the
        // text box is 60 x 16 px.
        let (px, half) = (0.1, Vec2::new(3.0, 0.8));
        // How far the text box stays from the measured line, along `n`.
        let clear = |f: &DimFrame, text: Option<Vec2>, n: Vec2| {
            let DimFrame::Linear { p0, .. } = *f else { return f64::NAN };
            let at = dim_layout(f, text, px, half).text;
            let across = half.x * n.x.abs() + half.y * n.y.abs();
            (at - p0).dot(n).abs() - across
        };
        let tall = DimFrame::Linear { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(0.0, 10.0), dir: Vec2::Y };
        // The shape is to the right of the line: the text goes left, clear of the line.
        let t = default_text(&tall, Vec2::new(5.0, 5.0), px, half);
        assert!(dim_layout(&tall, t, px, half).text.x < 0.0, "{t:?}");
        assert!(clear(&tall, t, Vec2::X) >= 8.0 * px - 1e-9, "{}", clear(&tall, t, Vec2::X));
        assert!(clear(&tall, None, Vec2::X) >= 8.0 * px - 1e-9);
        // A slanted line too.
        let d = Vec2::new(1.0, 2.0).normalized().unwrap_or(Vec2::X);
        let slant = DimFrame::Linear { p0: Vec2::ZERO, p1: d * 10.0, dir: d };
        let t = default_text(&slant, Vec2::new(5.0, 0.0), px, half);
        assert!(clear(&slant, t, d.perp()) >= 8.0 * px - 1e-9);
        // A short value over a level line keeps the 22 px place it always had.
        let wide = DimFrame::Linear { p0: Vec2::ZERO, p1: Vec2::new(10.0, 0.0), dir: Vec2::X };
        let t = default_text(&wide, Vec2::new(5.0, 5.0), px, half);
        assert!(t.is_some_and(|t| close(t, Vec2::new(0.0, -2.2))), "{t:?}");
        // A box of no finite size still gets a finite place.
        assert!(default_text(&tall, Vec2::ZERO, px, Vec2::new(f64::INFINITY, f64::NAN)).is_some_and(|t| t.is_finite()));
    }

    #[test]
    fn text_position_round_trips_through_the_frame() {
        let f = DimFrame::Linear { p0: Vec2::new(1.0, 2.0), p1: Vec2::new(31.0, 42.0), dir: Vec2::new(0.6, 0.8) };
        let at = Vec2::new(7.0, 30.0);
        let enc = encode_text(&f, at);
        assert!(close(dim_layout(&f, Some(enc), 0.1, Vec2::ZERO).text, at));
        let r = DimFrame::Radial { center: Vec2::new(5.0, 5.0), radius: 3.0, diameter: true, arc: None };
        assert!(close(dim_layout(&r, Some(encode_text(&r, at)), 0.1, Vec2::ZERO).text, at));
    }

    #[test]
    fn an_ellipse_s_minor_radius_is_drawn_as_the_distance_to_its_axis_end() {
        // A turned ellipse whose minor axis line was deleted: its minor radius is drawn as the
        // distance from the centre to the minor axis end would be, on whichever side the text is.
        let mut sk = Sketch::new();
        let major = Vec2::new(1.0, 1.0) * (20.0 * std::f64::consts::FRAC_1_SQRT_2);
        let c = sk.add_point(Vec2::ZERO, None).unwrap_or(0);
        let m = sk.add_point(major, None).unwrap_or(0);
        let e = sk.add_curve(CurveKind::Ellipse { c, m, r: 8.0 }, None).unwrap_or(0);
        let f =
            dim_frame(&sk, &ConstraintKind::Radius { c: e, value: 8.0 }).unwrap_or(DimFrame::Linear { p0: Vec2::ZERO, p1: Vec2::ZERO, dir: Vec2::X });
        assert!(matches!(f, DimFrame::HalfAxis { .. }), "{f:?}");
        // Its text is stored from the centre, as it was while it was drawn as a radius, so a
        // design saved then keeps the text where it was placed.
        let at = Vec2::new(3.0, 17.0);
        let radial = DimFrame::Radial { center: Vec2::ZERO, radius: 8.0, diameter: false, arc: None };
        assert!(close(encode_text(&f, at), encode_text(&radial, at)));
        assert!(close(dim_layout(&f, Some(encode_text(&radial, at)), 0.1, Vec2::ZERO).text, at));
        let minor = Vec2::new(-1.0, 1.0) * (8.0 * std::f64::consts::FRAC_1_SQRT_2);
        for end in [minor, minor * -1.0] {
            let to_end = DimFrame::Linear { p0: Vec2::ZERO, p1: end, dir: end * (1.0 / 8.0) };
            for at in [end * 1.6 + major * 0.2, end * 0.5 - major * 0.9] {
                let half = dim_layout(&f, Some(encode_text(&f, at)), 0.1, Vec2::new(2.0, 1.0));
                let axis = dim_layout(&to_end, Some(encode_text(&to_end, at)), 0.1, Vec2::new(2.0, 1.0));
                let same = |a: &[Vec2], b: &[Vec2]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| close(*p, *q));
                assert!(close(half.text, at) && close(half.text, axis.text), "{at:?}");
                assert!(
                    half.lines.len() == axis.lines.len() && half.lines.iter().zip(&axis.lines).all(|(a, b)| same(a, b)),
                    "{at:?}: {half:?} {axis:?}"
                );
                let tips = |l: &DimLayout| l.arrows.iter().map(|a| a.0).collect::<Vec<_>>();
                assert!(same(&tips(&half), &tips(&axis)), "{at:?}");
            }
        }
    }

    #[test]
    fn diameter_has_arrows_on_both_sides() {
        let r = DimFrame::Radial { center: Vec2::ZERO, radius: 10.0, diameter: true, arc: None };
        let l = dim_layout(&r, Some(Vec2::new(20.0, 0.0)), 0.1, Vec2::new(2.0, 1.0));
        assert_eq!(l.arrows.len(), 2);
        assert!(close(l.arrows[0].0, Vec2::new(10.0, 0.0)) && close(l.arrows[1].0, Vec2::new(-10.0, 0.0)));
        let r = DimFrame::Radial { center: Vec2::ZERO, radius: 10.0, diameter: false, arc: None };
        assert_eq!(dim_layout(&r, None, 0.1, Vec2::ZERO).arrows.len(), 1);
    }

    #[test]
    fn an_arrow_past_the_end_of_an_arc_points_at_its_extension() {
        // #3393: a quarter arc from 0° to 90°; a radius dimension dragged to 135° gets the arc
        // continued from its 90° end round to the arrow, and a little past it.
        let mut sk = Sketch::new();
        let c = sk.add_arc(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), [None, None, None], None).unwrap_or(0);
        let f =
            dim_frame(&sk, &ConstraintKind::Radius { c, value: 10.0 }).unwrap_or(DimFrame::Linear { p0: Vec2::ZERO, p1: Vec2::ZERO, dir: Vec2::X });
        let DimFrame::Radial { arc: Some((start, sweep)), .. } = f else { panic!("an arc's frame keeps its span: {f:?}") };
        assert!(start.abs() < 1e-9 && (sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let on_circle = |line: &Vec<Vec2>| line.len() > 2 && line.iter().all(|p| (p.len() - 10.0).abs() < 1e-9);
        let lay = dim_layout(&f, Some(Vec2::from_angle(0.75 * std::f64::consts::PI) * 20.0), 0.1, Vec2::new(2.0, 1.0));
        let ext: Vec<_> = lay.lines.iter().filter(|l| on_circle(l)).collect();
        assert_eq!(ext.len(), 1, "{:?}", lay.lines);
        let (first, last) = (ext[0][0], ext[0][ext[0].len() - 1]);
        assert!(first.dist(Vec2::new(0.0, 10.0)) < 1e-9, "starts at the arc's end: {first:?}");
        assert!((last.angle() - (0.75 * std::f64::consts::PI + 0.4 / 10.0)).abs() < 1e-9, "ends just past the arrow: {last:?}");
        // Dragged before the start (at -45°) the extension runs from the arrow to the start.
        let lay = dim_layout(&f, Some(Vec2::from_angle(-0.25 * std::f64::consts::PI) * 20.0), 0.1, Vec2::new(2.0, 1.0));
        let ext: Vec<_> = lay.lines.iter().filter(|l| on_circle(l)).collect();
        assert_eq!(ext.len(), 1);
        assert!(ext[0][ext[0].len() - 1].dist(Vec2::new(10.0, 0.0)) < 1e-9);
        // On the arc, or on a full circle, nothing is added.
        let on_arc = dim_layout(&f, Some(Vec2::from_angle(0.25 * std::f64::consts::PI) * 20.0), 0.1, Vec2::new(2.0, 1.0));
        assert!(!on_arc.lines.iter().any(on_circle));
        let circle = DimFrame::Radial { center: Vec2::ZERO, radius: 10.0, diameter: false, arc: None };
        assert!(!dim_layout(&circle, Some(Vec2::new(-20.0, 0.0)), 0.1, Vec2::ZERO).lines.iter().any(on_circle));
        // A diameter of the arc extends towards both arrows.
        let DimFrame::Radial { center, radius, arc, .. } = f else { return };
        let dia = DimFrame::Radial { center, radius, diameter: true, arc };
        let lay = dim_layout(&dia, Some(Vec2::from_angle(0.75 * std::f64::consts::PI) * 20.0), 0.1, Vec2::new(2.0, 1.0));
        assert_eq!(lay.lines.iter().filter(|l| on_circle(l)).count(), 2);
    }

    #[test]
    fn angle_frame_uses_the_rays_toward_the_lines() {
        let mut sk = Sketch::new();
        let a = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), None, None, None).unwrap_or(0);
        let b = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Some(1), None, None).unwrap_or(0);
        let k = ConstraintKind::Angle { a, b, value: std::f64::consts::FRAC_PI_2, flip: false };
        let Some(DimFrame::Angular { vertex, d0, sweep }) = dim_frame(&sk, &k) else { panic!("no frame") };
        assert!(close(vertex, Vec2::ZERO) && close(d0, Vec2::X));
        assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let l = dim_layout(&DimFrame::Angular { vertex, d0, sweep }, None, 0.1, Vec2::ZERO);
        assert_eq!(l.arrows.len(), 2);
        assert!(l.text.x > 0.0 && l.text.y > 0.0);
    }

    #[test]
    fn an_angle_dimension_reaches_the_lines_it_measures() {
        // #15610: two lines joined by an arc do not meet, so the angle's vertex is where their
        // extensions cross, far from the geometry; its rays have to reach the lines it measures.
        let mut sk = Sketch::new();
        let a = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), None, None, None).unwrap_or(0);
        let b = sk.add_line(Vec2::new(0.0, 10.0), Vec2::new(100.0, 20.0), None, None, None).unwrap_or(0);
        let k = ConstraintKind::Angle { a, b, value: 0.1, flip: false };
        let f = dim_frame(&sk, &k).unwrap_or(DimFrame::Radial { center: Vec2::ZERO, radius: 1.0, diameter: false, arc: None });
        let DimFrame::Angular { vertex, .. } = f else { panic!("an angle is drawn as an angle: {f:?}") };
        assert!(vertex.dist(Vec2::new(-100.0, 0.0)) < 1e-6, "the vertex is where the extensions cross: {vertex:?}");
        let reach = dim_reach(&sk, &k);
        assert!(reach[0].is_some() && reach[1].is_some(), "{reach:?}");
        let ray_ends = |lay: &DimLayout| -> Vec<Vec2> { lay.lines.iter().filter(|l| l.len() == 2).map(|l| l[l.len() - 1]).collect() };
        let lay = dim_layout_reaching(&f, None, 0.1, Vec2::new(2.0, 1.0), reach);
        let ends = ray_ends(&lay);
        assert!(ends.iter().any(|p| p.dist(Vec2::new(0.0, 0.0)) < 1e-6), "the first ray reaches the lower line: {ends:?}");
        assert!(ends.iter().any(|p| p.dist(Vec2::new(0.0, 10.0)) < 1e-6), "the second ray reaches the upper line: {ends:?}");
        // Without the lines the rays stop at the arc, short of the lines.
        let plain = ray_ends(&dim_layout(&f, None, 0.1, Vec2::new(2.0, 1.0)));
        assert!(plain.iter().all(|p| p.dist(vertex) < 20.0), "{plain:?}");
        // Two lines that meet keep their rays at the arc: there is nothing to reach.
        let mut sk = Sketch::new();
        let a = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), None, None, None).unwrap_or(0);
        let b = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Some(1), None, None).unwrap_or(0);
        let k = ConstraintKind::Angle { a, b, value: std::f64::consts::FRAC_PI_2, flip: false };
        let f = dim_frame(&sk, &k).unwrap_or(DimFrame::Radial { center: Vec2::ZERO, radius: 1.0, diameter: false, arc: None });
        let ends = ray_ends(&dim_layout_reaching(&f, None, 0.1, Vec2::new(2.0, 1.0), dim_reach(&sk, &k)));
        assert!(ends.iter().all(|p| p.dist(Vec2::ZERO) < 20.0), "{ends:?}");
    }

    #[test]
    fn angle_frame_follows_its_text_into_the_opposite_sector() {
        // #11800: a long base running left to the corner, a short line up and right from it.
        // The 45° between the base's direction and the slanted one is drawn below left, past
        // the slanted line, unless the text sits above right, past the base.
        let mut sk = Sketch::new();
        let a = sk.add_line(Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), None, None, None).unwrap_or(0);
        let b = sk.add_line(Vec2::new(0.0, 0.0), Vec2::new(30.0, 30.0), Some(2), None, None).unwrap_or(0);
        let k = ConstraintKind::Angle { a, b, value: std::f64::consts::FRAC_PI_4, flip: false };
        let d0 = |text: Option<Vec2>| match dim_frame_at(&sk, &k, text) {
            Some(DimFrame::Angular { d0, sweep, .. }) if (sweep - std::f64::consts::FRAC_PI_4).abs() < 1e-12 => d0,
            other => panic!("{other:?}"),
        };
        assert!(close(d0(None), -Vec2::X));
        assert!(close(d0(Some(Vec2::new(20.0, 5.0))), Vec2::X));
        assert!(close(d0(Some(Vec2::new(-20.0, -5.0))), -Vec2::X));
        // Text in a sector of the other angle, at the vertex or not finite: as without text.
        for t in [Vec2::new(-5.0, 20.0), Vec2::ZERO, Vec2::new(f64::NAN, 1.0)] {
            assert!(close(d0(Some(t)), -Vec2::X), "{t:?}");
        }
        // Other dimensions do not change with their text.
        let len = ConstraintKind::Length { l: a, value: 100.0 };
        assert_eq!(dim_frame_at(&sk, &len, Some(Vec2::new(0.0, -30.0))), dim_frame(&sk, &len));
    }

    #[test]
    fn extension_lines_reach_the_nearest_part_of_the_lines_measured() {
        // #1361: two parallel lines dimensioned from the low end of one; the dimension placed
        // across their middles needs no extension lines, one placed above them reaches their
        // top ends, not the low end.
        let mut sk = Sketch::new();
        let l = sk.add_line(Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0), None, None, None).unwrap_or(0);
        let m = sk.add_line(Vec2::new(30.0, -80.0), Vec2::new(30.0, 90.0), None, None, None).unwrap_or(0);
        let p = sk.curves[m].kind.point_ids()[0];
        let k = ConstraintKind::PointLineDistance { p, l, value: 30.0 };
        let f = dim_frame(&sk, &k).unwrap_or(DimFrame::Radial { center: Vec2::ZERO, radius: 1.0, diameter: false, arc: None });
        let reach = dim_reach(&sk, &k);
        assert!(reach[0].is_some() && reach[1].is_some());
        let extension_ys = |at: Vec2| -> Vec<(f64, f64)> {
            let lay = dim_layout_reaching(&f, Some(encode_text(&f, at)), 0.1, Vec2::new(2.0, 1.0), reach);
            lay.lines.iter().filter(|line| line.len() == 2 && (line[0].x - line[1].x).abs() < 1e-9).map(|line| (line[0].y, line[1].y)).collect()
        };
        assert!(extension_ys(Vec2::new(15.0, 50.0)).is_empty(), "{:?}", extension_ys(Vec2::new(15.0, 50.0)));
        let above = extension_ys(Vec2::new(15.0, 120.0));
        assert_eq!(above.len(), 2);
        assert!((above[0].0 - 100.3).abs() < 1e-9 && (above[1].0 - 90.3).abs() < 1e-9, "{above:?}");
        // Without the lines' reach the extension lines start at the measured point's height.
        let lay = dim_layout(&f, Some(encode_text(&f, Vec2::new(15.0, 120.0))), 0.1, Vec2::new(2.0, 1.0));
        assert!(lay.lines.iter().any(|line| (line[0].y + 79.7).abs() < 1e-9));
        // A point that is not the end of a parallel line keeps its own extension line.
        let mut sk = Sketch::new();
        let l = sk.add_line(Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0), None, None, None).unwrap_or(0);
        let q = sk.add_point(Vec2::new(30.0, -80.0), None).unwrap_or(0);
        assert!(dim_reach(&sk, &ConstraintKind::PointLineDistance { p: q, l, value: 30.0 })[1].is_none());
    }

    /// Where a line from the centre of an upright text box of half size `half` leaves it, along `u`.
    fn exit(half: Vec2, u: Vec2) -> f64 {
        (half.x / u.x.abs()).min(half.y / u.y.abs())
    }

    #[test]
    fn a_line_through_its_text_stops_at_the_text_box() {
        // A leader or a dimension line at an angle stops the gap past the text box's edge, not
        // where the box's farthest corner reaches along it.
        let (px, half) = (0.1, Vec2::new(2.5, 0.8));
        let gap = 3.0 * px;
        let deg = std::f64::consts::PI / 180.0;
        for a in [0.0, 1e-9, 30.0 * deg, 45.0 * deg, 90.0 * deg - 1e-9, 90.0 * deg, 135.0 * deg, 270.0 * deg] {
            let u = Vec2::from_angle(a);
            let want = exit(half, u) + gap;
            for diameter in [false, true] {
                let circle = DimFrame::Radial { center: Vec2::new(5.0, -3.0), radius: 10.0, diameter, arc: None };
                // Text outside: the leader ends `want` short of the text.
                let lay = dim_layout(&circle, Some(u * 30.0), px, half);
                let end = lay.lines[0][lay.lines[0].len() - 1];
                assert!((end.dist(lay.text) - want).abs() < 1e-9, "{a} {diameter}: {} vs {want}", end.dist(lay.text));
                // Text inside: the line breaks `want` either side of it.
                let lay = dim_layout(&circle, Some(u * 4.0), px, half);
                assert_eq!(lay.lines.len(), 2, "{a} {diameter}");
                assert!((lay.lines[0][1].dist(lay.text) - want).abs() < 1e-9 && (lay.lines[1][0].dist(lay.text) - want).abs() < 1e-9);
            }
        }
        // An aligned linear dimension at 0°, 45° and 90°: the line breaks `want` either side of its text.
        for a in [0.0, 45.0 * deg, 90.0 * deg] {
            let dir = Vec2::from_angle(a);
            let f = DimFrame::Linear { p0: Vec2::ZERO, p1: dir * 100.0, dir };
            let lay = dim_layout(&f, Some(Vec2::new(0.0, 10.0)), px, half);
            let want = exit(half, dir) + gap;
            assert_eq!(lay.lines.len(), 4, "{a}");
            assert!((lay.lines[2][1].dist(lay.text) - want).abs() < 1e-9, "{a}: {} vs {want}", lay.lines[2][1].dist(lay.text));
            assert!((lay.lines[3][0].dist(lay.text) - want).abs() < 1e-9);
            // Pulled out past the extension lines: the leader along the line reaches it too.
            let lay = dim_layout(&f, Some(Vec2::new(80.0, 10.0)), px, half);
            assert_eq!(lay.lines.len(), 4, "{a}");
            assert!((lay.lines[3][1].dist(lay.text) - want).abs() < 1e-9, "{a}: {} vs {want}", lay.lines[3][1].dist(lay.text));
        }
        // Whether the text fits between the arrows still goes by the whole box: at 45° a 6 long
        // dimension fits the line's crossing of the box (2.86 with the gaps) but not its corners.
        let dir = Vec2::from_angle(45.0 * deg);
        let l = dim_layout(&DimFrame::Linear { p0: Vec2::ZERO, p1: dir * 6.0, dir }, Some(Vec2::new(0.0, 10.0)), px, half);
        assert!(close(l.arrows[0].1, dir) && close(l.arrows[1].1, -dir), "arrows outside: {:?}", l.arrows);
    }

    #[test]
    fn hostile_text_boxes_lay_out_finite() {
        let frames = [
            DimFrame::Radial { center: Vec2::ZERO, radius: 10.0, diameter: false, arc: None },
            DimFrame::Radial { center: Vec2::ZERO, radius: 10.0, diameter: true, arc: Some((0.0, 1.0)) },
            DimFrame::Linear { p0: Vec2::ZERO, p1: Vec2::new(70.0, 70.0), dir: Vec2::new(1.0, 1.0) * std::f64::consts::FRAC_1_SQRT_2 },
            DimFrame::Linear { p0: Vec2::ZERO, p1: Vec2::new(100.0, 0.0), dir: Vec2::X },
        ];
        let sizes = [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300, 1e-300, f64::MAX];
        let texts = [Vec2::new(20.0, 20.0), Vec2::new(3.0, 0.0), Vec2::new(0.0, 1e-9), Vec2::new(300.0, 10.0), Vec2::new(-300.0, -10.0)];
        for f in &frames {
            for (hx, hy) in sizes.iter().flat_map(|x| sizes.iter().map(move |y| (*x, *y))) {
                for text in texts {
                    let lay = dim_layout(f, Some(text), 0.1, Vec2::new(hx, hy));
                    let mut points = lay.lines.iter().flatten().chain(lay.arrows.iter().map(|(p, _)| p));
                    assert!(lay.text.is_finite() && points.all(|p| p.is_finite()), "{f:?} {hx} {hy} {text:?}: {:?}", lay.lines);
                }
            }
        }
    }
}
