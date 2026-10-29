//! Where a sketch dimension is drawn: extension lines from the measured geometry, the dimension
//! line (or arc) with its arrows, and the value text. Shared by the UI (drawing and hit testing)
//! and the engine (storing a dragged text position), all in sketch coordinates.
//!
//! A dimension's text position is kept relative to its own frame ([`Constraint::text`]), so it
//! follows the geometry: for a linear dimension it is (along, across) from the middle of the
//! measured points, for the others an offset from the centre or the vertex.
//!
//! [`Constraint::text`]: crate::Constraint::text

use solidcraft_geom::Vec2;

use crate::model::{ConstraintKind, CurveKind, Sketch, TwoCircles};
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
    /// to that end is when the ellipse has its axes. The text is stored as for the linear
    /// frame along `dir`.
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
            solidcraft_geom::Seg2::Arc { center, start, sweep, .. } => {
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
        Radius { c, .. } if matches!(sk.curves.get(c)?.kind, CurveKind::Ellipse { .. }) => {
            let CurveKind::Ellipse { c: centre, m, r } = sk.curves.get(c)?.kind else { return None };
            let center = sk.point(centre)?;
            DimFrame::HalfAxis { center, dir: unit(sk.point(m)? - center, Vec2::X).perp(), len: r }
        }
        Radius { c, .. } | Diameter { c, .. } => {
            let (radius, arc) = match sk.curves.get(c)?.kind {
                CurveKind::Ellipse { r, .. } => (r, None),
                CurveKind::Arc { .. } => match sk.segs(c).first()? {
                    solidcraft_geom::Seg2::Arc { radius, start, sweep, .. } => (*radius, Some((*start, *sweep))),
                    _ => return None,
                },
                _ => (sk.radius(c)?, None),
            };
            DimFrame::Radial { center: sk.center(c)?, radius, diameter: matches!(k, Diameter { .. }), arc }
        }
        ArcLength { c, .. } => match sk.segs(c).first()? {
            solidcraft_geom::Seg2::Arc { center, radius, start, sweep } => {
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

/// The stored text position for text centred at `at`.
pub fn encode_text(f: &DimFrame, at: Vec2) -> Vec2 {
    match *f {
        DimFrame::Linear { p0, p1, dir } => {
            let m = (p0 + p1) * 0.5;
            Vec2::new((at - m).dot(dir), (at - m).dot(dir.perp()))
        }
        DimFrame::Radial { center, .. } | DimFrame::ArcLength { center, .. } => at - center,
        DimFrame::Angular { vertex, .. } => at - vertex,
        DimFrame::HalfAxis { center, dir, len } => encode_text(&DimFrame::Linear { p0: center, p1: center + dir * len, dir }, at),
    }
}

/// The linear frame a [`DimFrame::HalfAxis`] is drawn as for the stored text `t`: to the end
/// on the text's side, with `t` re-expressed for that frame.
fn half_axis_side(center: Vec2, dir: Vec2, len: f64, t: Vec2) -> (DimFrame, Vec2) {
    let along = DimFrame::Linear { p0: center, p1: center + dir * len, dir };
    let at = center + dir * (len * 0.5 + t.x) + dir.perp() * t.y;
    if (at - center).dot(dir) >= 0.0 {
        return (along, t);
    }
    let against = DimFrame::Linear { p0: center, p1: center - dir * len, dir: -dir };
    (against, encode_text(&against, at))
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
pub fn default_text(f: &DimFrame, away: Vec2, px: f64) -> Option<Vec2> {
    match *f {
        DimFrame::Linear { p0, p1, dir } => {
            let side = if ((p0 + p1) * 0.5 - away).dot(dir.perp()) < -1e-9 { -1.0 } else { 1.0 };
            Some(Vec2::new(0.0, side * 22.0 * px))
        }
        _ => None,
    }
}

/// The text offset [`dim_layout_reaching`] uses when a dimension's text was never placed:
/// frame-relative, as in [`encode_text`].
pub fn default_text_offset(f: &DimFrame, px: f64) -> Vec2 {
    match *f {
        DimFrame::Linear { .. } | DimFrame::HalfAxis { .. } => Vec2::new(0.0, 22.0 * px),
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

/// The lines a linear dimension's two extension lines may start from (#1361), for its `p0` and
/// `p1` sides: a dimension to a line reaches the nearest part of that line, not the foot of the
/// measured point, and a dimension between two parallel lines reaches the nearest part of the
/// line whose end it measures from. `None` for a side that measures a point.
pub fn dim_reach(sk: &Sketch, k: &ConstraintKind) -> [Option<(Vec2, Vec2)>; 2] {
    use ConstraintKind::*;
    let (p, l, to_line) = match *k {
        PointLineDistance { p, l, .. } => (p, l, true),
        LinearDiameter { p, l, .. } => (p, l, false),
        LineCircleDistance { l, .. } => return [line_ends(sk, l), None],
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

/// Lay a dimension out. `text` is the stored text position (None: the default place), `px`
/// the sketch length of one screen pixel and `half` the half size of the text box (sketch
/// units), which the dimension line leaves a gap for.
pub fn dim_layout(f: &DimFrame, text: Option<Vec2>, px: f64, half: Vec2) -> DimLayout {
    dim_layout_reaching(f, text, px, half, [None, None])
}

/// [`dim_layout`] with the extension lines of a linear dimension starting from the nearest
/// point of the geometry in `reach` ([`dim_reach`]) instead of the measured points.
pub fn dim_layout_reaching(f: &DimFrame, text: Option<Vec2>, px: f64, half: Vec2, reach: [Option<(Vec2, Vec2)>; 2]) -> DimLayout {
    let px = if px.is_finite() && px > 0.0 { px } else { 1.0 };
    let arrow = 9.0 * px;
    let gap = 3.0 * px;
    let over = 4.0 * px;
    let mut out = DimLayout::default();
    match *f {
        DimFrame::HalfAxis { center, dir, len } => {
            let t = text.unwrap_or_else(|| default_text_offset(f, px));
            let (linear, t) = half_axis_side(center, dir, len, t);
            return dim_layout_reaching(&linear, Some(t), px, half, reach);
        }
        DimFrame::Linear { p0, p1, dir } => {
            let n = dir.perp();
            let m = (p0 + p1) * 0.5;
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Linear { p0, p1, dir }, px));
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
            // Half the text along the line (it is drawn level on screen).
            let along = (half.x * dir.x.abs() + half.y * dir.y.abs()) + gap;
            let inside = s.abs() + along <= span * 0.5;
            if span > 2.0 * arrow + 2.0 * along && inside {
                // Arrows inside, pointing out, the line broken around the text.
                out.arrows.push((lo, -dir));
                out.arrows.push((hi, dir));
                let tc = m + n * h + dir * s;
                out.lines.push(vec![lo, tc - dir * along]);
                out.lines.push(vec![tc + dir * along, hi]);
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
                    out.lines.push(vec![end, tc - dir * (along * reach.signum())]);
                }
            }
        }
        DimFrame::Radial { center, radius, diameter, arc } => {
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Radial { center, radius, diameter, arc }, px));
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
            let rim = half.x * u.x.abs() + half.y * u.y.abs() + gap;
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
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::Angular { vertex, d0, sweep }, px));
            let r = t.len().max(arrow);
            out.text = vertex + t;
            // Extension rays from the vertex out past the arc.
            for a in [a0, a0 + sweep] {
                let u = Vec2::from_angle(a);
                out.lines.push(vec![vertex + u * (r * 0.15).min(gap * 3.0), vertex + u * (r + over)]);
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
            let t = text.unwrap_or_else(|| default_text_offset(&DimFrame::ArcLength { center, radius, start, sweep }, px));
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
}
