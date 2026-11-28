//! The planar pieces of a projected outline, tidied: a body's outline on a plane comes from many
//! sources (each stretch of an edge, each curved face's silhouette), and where they meet or
//! coincide it is drawn once and in its plainest exact form.
//!
//! - A rational quadratic spline whose inner knots are all double (the projection of a stretch
//!   of a circle at a slant) is its conic spans ([`conic_spans`]).
//! - A conic whose control lies on its chord's line (an arc seen edge-on) is the line it covers
//!   ([`flat_conic`]).
//! - A conic that is an arc of a circle is that arc, and arcs of one circle that follow each other
//!   are joined, a whole turn into the circle ([`round_conics`]).
//! - Ends within a gap of each other become one point ([`meet_at_shared_ends`]).
//! - Lines on one line become the stretches they cover together, and any other piece given twice
//!   is kept once ([`drop_coincident`]); [`near`] tells whether a point lies on a piece.
//!
//! Not upstream: SolidCraft's whole-body projection (`crates/doc/src/project.rs`, MIT OR
//! Apache-2.0, tangency-labs/solidcraft#19386 and #19413), moved into the kernel so that the
//! application computes no geometry (docs/layers.md §1). The arithmetic is SolidCraft's, step for
//! step: a vector's length is `hypot`, and a division divides each coordinate.
//!
//! Deviations: none (no upstream counterpart).

// A comparison that is not true takes the refusing branch, so a NaN is refused too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::math::det;
use crate::math::Vec2;

/// Below this length a vector has no direction.
const EPS: f64 = 1e-9;

/// One piece of a planar outline.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A point.
    Point(Vec2),
    /// A line from the first point to the second.
    Line(Vec2, Vec2),
    /// A circle: its centre and radius.
    Circle(Vec2, f64),
    /// A counter-clockwise arc about `c` from `a` to `b`.
    Arc {
        /// Its centre.
        c: Vec2,
        /// Its start.
        a: Vec2,
        /// Its end.
        b: Vec2,
    },
    /// A rational quadratic conic from `a` to `b` with control `apex` and shape `rho` (the middle
    /// weight `w` over `1 + w`, end weights one).
    Conic {
        /// Its start.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its end.
        b: Vec2,
        /// Its shape.
        rho: f64,
    },
    /// A full ellipse: its centre, the end of its major axis and its minor radius.
    Ellipse {
        /// Its centre.
        c: Vec2,
        /// The end of its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
    /// A spline through (`control` false) or over (`control`) its points, with its own knots and
    /// weights where it has them.
    Spline {
        /// Its points.
        pts: Vec<Vec2>,
        /// The points are control points.
        control: bool,
        /// Its degree.
        degree: u8,
        /// It closes on itself.
        periodic: bool,
        /// Its knots and weights, where they are not uniform.
        nurbs: Option<(Vec<f64>, Vec<f64>)>,
    },
}

fn dot(a: Vec2, b: Vec2) -> f64 {
    a.x * b.x + a.y * b.y
}
fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn len(a: Vec2) -> f64 {
    det::hypot(a.x, a.y)
}
fn len2(a: Vec2) -> f64 {
    dot(a, a)
}
fn dist(a: Vec2, b: Vec2) -> f64 {
    len(a - b)
}
fn div(a: Vec2, d: f64) -> Vec2 {
    Vec2::new(a.x / d, a.y / d)
}
fn normalized(a: Vec2) -> Option<Vec2> {
    let l = len(a);
    (l > EPS && l.is_finite()).then(|| div(a, l))
}
fn angle(a: Vec2) -> f64 {
    det::atan2(a.y, a.x)
}

/// The conic from `a` to `b` with control `apex` and shape `rho`, at `t` in [0, 1].
#[must_use]
pub fn conic_point(a: Vec2, apex: Vec2, b: Vec2, rho: f64, t: f64) -> Vec2 {
    let w = rho / (1.0 - rho).max(1e-9);
    let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
    let den = b0 + b1 + b2;
    div(a * b0 + apex * b1 + b * b2, den.max(1e-12))
}

/// The ellipse about `c` with its major axis ending at `m` and minor radius `r`, at angle `t`.
#[must_use]
pub fn ellipse_point(c: Vec2, m: Vec2, r: f64, t: f64) -> Vec2 {
    let u = m - c;
    let a = len(u).max(1e-12);
    let v = div(Vec2::new(-u.y, u.x), a) * r;
    c + u * det::cos(t) + v * det::sin(t)
}

/// A rational quadratic spline whose inner knots are all double, as the projection of a stretch
/// of a circle at a slant comes out: its conic spans, one per span, the same curve (each span's
/// middle weight over its ends' geometric mean). Any other piece as it is.
#[must_use]
pub fn conic_spans(g: Piece) -> Vec<Piece> {
    let Piece::Spline {
        pts,
        control: true,
        degree: 2,
        periodic: false,
        nurbs: Some((k, w)),
    } = &g
    else {
        return vec![g];
    };
    let spans = pts.len().saturating_sub(1) / 2;
    if spans == 0 || pts.len() != 2 * spans + 1 || w.len() != pts.len() || k.len() != pts.len() + 3
    {
        return vec![g];
    }
    // Knots t t t, then each inner knot twice, then u u u.
    let double = (0..spans.saturating_sub(1)).all(|i| k.get(3 + 2 * i) == k.get(4 + 2 * i));
    let ends = k.first() == k.get(1)
        && k.get(1) == k.get(2)
        && k.last() == k.get(k.len() - 2)
        && k.get(k.len() - 2) == k.get(k.len() - 3);
    if !double || !ends {
        return vec![g];
    }
    let mut out = Vec::new();
    for i in 0..spans {
        let (Some(a), Some(apex), Some(b), Some(w0), Some(w1), Some(w2)) = (
            pts.get(2 * i),
            pts.get(2 * i + 1),
            pts.get(2 * i + 2),
            w.get(2 * i),
            w.get(2 * i + 1),
            w.get(2 * i + 2),
        ) else {
            return vec![g];
        };
        let wm = w1 / (w0 * w2).sqrt();
        if !(wm.is_finite() && wm > 0.0 && w0 > &0.0 && w2 > &0.0) {
            return vec![g];
        }
        let rho = wm / (1.0 + wm);
        // Past this a conic's middle weight no longer survives its shape (`conic_point` caps
        // it): the spline stays as it is.
        if !(rho < 1.0 - 1e-6) {
            return vec![g];
        }
        out.push(Piece::Conic {
            a: *a,
            apex: *apex,
            b: *b,
            rho,
        });
    }
    out
}

/// A conic whose control lies on its chord's line, within 1e-7 of its size, is a stretch of that
/// line (an arc seen edge-on): the line it covers, from end to end of the points it passes (65
/// along it). Any other piece as it is.
#[must_use]
pub fn flat_conic(g: Piece) -> Piece {
    let Piece::Conic { a, apex, b, rho } = g else {
        return g;
    };
    let Some(u) = normalized(b - a) else {
        return g;
    };
    if cross(apex - a, u).abs() > 1e-7 * (1.0 + dist(a, b).max(dist(a, apex))) {
        return g;
    }
    let along: Vec<f64> = (0..=64)
        .map(|k| dot(conic_point(a, apex, b, rho, f64::from(k) / 64.0) - a, u))
        .collect();
    let (lo, hi) = along
        .iter()
        .fold((0.0_f64, 0.0_f64), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    Piece::Line(a + u * lo, a + u * hi)
}

/// `geom` with every conic that is an arc of a circle (its control as far from both ends, and its
/// weight the cosine of the angle between its chord and its tangents, each within 1e-9) as that
/// arc, and arcs of one circle that follow each other joined, a whole turn into the circle.
#[must_use]
pub fn round_conics(geom: Vec<Piece>) -> Vec<Piece> {
    let as_arc = |g: Piece| -> Piece {
        let Piece::Conic { a, apex, b, rho } = g else {
            return g;
        };
        let w = rho / (1.0 - rho);
        let (ta, tb) = (apex - a, apex - b);
        let chord = b - a;
        let (la, lb, lc) = (len(ta), len(tb), len(chord));
        let scale = la.max(lb).max(lc);
        if !(w > 0.0 && w < 1.0 && lc > 0.0 && la > 0.0)
            || (la - lb).abs() > 1e-9 * scale
            || (dot(ta, chord) / (la * lc) - w).abs() > 1e-9
        {
            return g;
        }
        let m = (a + b) * 0.5;
        let c = m + (m - apex) * (w * w / (1.0 - w * w));
        if cross(ta, chord) > 0.0 {
            Piece::Arc { c, a, b }
        } else {
            Piece::Arc { c, a: b, b: a }
        }
    };
    // Each arc's counter-clockwise sweep, kept beside it: two ends that meet close a whole turn
    // only when the arcs joined there sweep more than half of one (a tiny arc's ends meet too).
    let sweep_of = |g: &Piece| match *g {
        Piece::Conic { rho, .. } => {
            let w = rho / (1.0 - rho);
            2.0 * det::acos(w.clamp(-1.0, 1.0))
        }
        Piece::Arc { c, a, b } => (angle(b - c) - angle(a - c)).rem_euclid(std::f64::consts::TAU),
        _ => 0.0,
    };
    let mut out: Vec<(Piece, f64)> = Vec::new();
    for g in geom {
        let sweep = sweep_of(&g);
        let g = as_arc(g);
        if let (
            Some((Piece::Arc { c, a, b }, swept)),
            Piece::Arc {
                c: c2,
                a: a2,
                b: b2,
            },
        ) = (out.last_mut(), &g)
        {
            let tol = 1e-9 * (1.0 + dist(*c, *a));
            if dist(*c, *c2) <= tol {
                if dist(*b, *a2) <= tol {
                    *b = *b2;
                    *swept += sweep;
                    continue;
                }
                if dist(*a, *b2) <= tol {
                    *a = *a2;
                    *swept += sweep;
                    continue;
                }
            }
        }
        out.push((g, sweep));
    }
    out.into_iter()
        .map(|(g, swept)| match g {
            Piece::Arc { c, a, b }
                if swept > std::f64::consts::PI && dist(a, b) <= 1e-9 * (1.0 + dist(c, a)) =>
            {
                Piece::Circle(c, dist(c, a))
            }
            g => g,
        })
        .collect()
}

/// Whether `p` lies within `tol` of the piece `g`: exactly for a line, an arc or a circle; within
/// the sag of 256 chords for a conic (under 1e-5 of its size for at most a quarter turn) and of
/// 1024 for an ellipse. A point or a spline never holds it.
#[must_use]
pub fn near(g: &Piece, p: Vec2, tol: f64) -> bool {
    let to_segment = |a: Vec2, b: Vec2| {
        let d = b - a;
        let t = if len2(d) > 0.0 {
            (dot(p - a, d) / len2(d)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        dist(p, a + d * t)
    };
    let on_chords = |pts: &[Vec2], slack: f64| {
        pts.windows(2)
            .any(|w| matches!(w, [a, b] if to_segment(*a, *b) <= tol + slack))
    };
    let tau = std::f64::consts::TAU;
    match *g {
        Piece::Line(a, b) => to_segment(a, b) <= tol,
        Piece::Circle(c, r) => (dist(p, c) - r).abs() <= tol,
        Piece::Arc { c, a, b } => {
            let r = dist(c, a);
            let (t0, t) = (angle(a - c), angle(p - c));
            let sweep = (angle(b - c) - t0).rem_euclid(tau);
            ((dist(p, c) - r).abs() <= tol && (t - t0).rem_euclid(tau) <= sweep)
                || dist(p, a) <= tol
                || dist(p, b) <= tol
        }
        Piece::Conic { a, apex, b, rho } => {
            let pts: Vec<Vec2> = (0..=256)
                .map(|k| conic_point(a, apex, b, rho, f64::from(k) / 256.0))
                .collect();
            on_chords(&pts, 1e-5 * dist(a, b).max(dist(a, apex)))
        }
        Piece::Ellipse { c, major, minor } => {
            let pts: Vec<Vec2> = (0..=1024)
                .map(|k| ellipse_point(c, major, minor, tau * f64::from(k) / 1024.0))
                .collect();
            on_chords(&pts, 1e-5 * dist(c, major))
        }
        Piece::Point(_) | Piece::Spline { .. } => false,
    }
}

/// Where pieces' ends lie within `gap` of an end met before, they move onto it: the ends of lines,
/// arcs, conics and open splines, in order.
pub fn meet_at_shared_ends(geom: &mut [Piece], gap: f64) {
    let mut seen: Vec<Vec2> = Vec::new();
    let mut snap = |p: &mut Vec2| match seen.iter().find(|q| dist(**q, *p) <= gap) {
        Some(q) => *p = *q,
        None => seen.push(*p),
    };
    for g in geom {
        match g {
            Piece::Line(a, b) | Piece::Arc { a, b, .. } | Piece::Conic { a, b, .. } => {
                snap(a);
                snap(b);
            }
            Piece::Spline {
                pts,
                periodic: false,
                ..
            } => {
                if let Some(first) = pts.first_mut() {
                    snap(first);
                }
                if let Some(last) = pts.last_mut() {
                    snap(last);
                }
            }
            _ => {}
        }
    }
}

/// `geom` with pieces that coincide (within `tol`) drawn once: lines on one line become the
/// stretches they cover together, in the order the line's first piece runs; any other piece given
/// twice (a spline only with the same kind, degree, knots and weights; a uniform one also the
/// other way round) is kept once. Lines come first, then the rest in order. An outline seen square
/// to a face meets several edges on one line or circle: a hole's rim seen edge-on along a plate's
/// edge, a boss's rim on its foot.
#[must_use]
pub fn drop_coincident(geom: Vec<Piece>, tol: f64) -> Vec<Piece> {
    let same = |p: Vec2, q: Vec2| dist(p, q) <= tol;
    let duplicate = |a: &Piece, b: &Piece| match (a, b) {
        (Piece::Circle(c, r), Piece::Circle(c2, r2)) => same(*c, *c2) && (r - r2).abs() <= tol,
        (
            Piece::Arc { c, a, b },
            Piece::Arc {
                c: c2,
                a: a2,
                b: b2,
            },
        ) => {
            // Counter-clockwise from the same start to the same end: the same arc, unless the
            // ends are so close that one sweeps a hair and the other nearly a whole turn.
            let sweep = |c: Vec2, a: Vec2, b: Vec2| {
                (angle(b - c) - angle(a - c)).rem_euclid(std::f64::consts::TAU)
            };
            same(*c, *c2)
                && same(*a, *a2)
                && same(*b, *b2)
                && (sweep(*c, *a, *b) - sweep(*c2, *a2, *b2)).abs() < std::f64::consts::PI
        }
        (
            Piece::Conic { a, apex, b, rho },
            Piece::Conic {
                a: a2,
                apex: x2,
                b: b2,
                rho: r2,
            },
        ) => {
            same(*apex, *x2)
                && (rho - r2).abs() <= 1e-9
                && ((same(*a, *a2) && same(*b, *b2)) || (same(*a, *b2) && same(*b, *a2)))
        }
        (
            Piece::Ellipse { c, major, minor },
            Piece::Ellipse {
                c: c2,
                major: m2,
                minor: n2,
            },
        ) => {
            same(*c, *c2)
                && (minor - n2).abs() <= tol
                && (same(*major, *m2) || same(*major, *c2 * 2.0 - *m2))
        }
        (
            Piece::Spline {
                pts,
                control,
                degree,
                periodic,
                nurbs,
            },
            Piece::Spline {
                pts: p2,
                control: c2,
                degree: d2,
                periodic: o2,
                nurbs: n2,
            },
        ) => {
            pts.len() == p2.len()
                && control == c2
                && degree == d2
                && periodic == o2
                && nurbs == n2
                && (pts.iter().zip(p2).all(|(p, q)| same(*p, *q))
                    || (nurbs.is_none()
                        && pts.iter().zip(p2.iter().rev()).all(|(p, q)| same(*p, *q))))
        }
        _ => false,
    };
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    let mut others: Vec<Piece> = Vec::new();
    for g in geom {
        match g {
            Piece::Line(a, b) => lines.push((a, b)),
            g if others.iter().any(|o| duplicate(o, &g)) => {}
            g => others.push(g),
        }
    }
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    for i in 0..lines.len() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(&(a, b)) = lines.get(i) else {
            continue;
        };
        let Some(u) = normalized(b - a) else {
            continue;
        };
        let on = |p: Vec2| cross(p - a, u).abs() <= tol;
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for (j, (p, q)) in lines.iter().enumerate().skip(i) {
            if !used.get(j).copied().unwrap_or(true) && on(*p) && on(*q) {
                if let Some(slot) = used.get_mut(j) {
                    *slot = true;
                }
                let (s, t) = (dot(*p - a, u), dot(*q - a, u));
                spans.push((s.min(t), s.max(t)));
            }
        }
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (s, t) in spans {
            match merged.last_mut() {
                Some(last) if s <= last.1 + tol => last.1 = last.1.max(t),
                _ => merged.push((s, t)),
            }
        }
        out.extend(
            merged
                .into_iter()
                .map(|(s, t)| Piece::Line(a + u * s, a + u * t)),
        );
    }
    out.extend(others);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    /// A unit circle's quarter from angle `a` as a conic: control at the corner of the tangents,
    /// weight cos 45 degrees.
    fn quarter(a: f64) -> Piece {
        let h = std::f64::consts::FRAC_PI_4;
        let w = det::cos(h);
        let at = |t: f64| v(det::cos(t), det::sin(t));
        Piece::Conic {
            a: at(a),
            apex: at(a + h) * (1.0 / w),
            b: at(a + 2.0 * h),
            rho: w / (1.0 + w),
        }
    }

    #[test]
    fn four_quarter_conics_of_a_circle_are_the_circle() {
        let h = std::f64::consts::FRAC_PI_2;
        let out = round_conics((0..4).map(|k| quarter(f64::from(k) * h)).collect());
        let [Piece::Circle(c, r)] = out.as_slice() else {
            panic!("{out:?}")
        };
        assert!(len(*c) < 1e-12 && (r - 1.0).abs() < 1e-12, "{out:?}");
    }

    #[test]
    fn a_tiny_arc_whose_ends_meet_stays_an_arc() {
        let at = |t: f64| v(det::cos(t), det::sin(t));
        let tiny = Piece::Arc {
            c: v(0.0, 0.0),
            a: at(0.0),
            b: at(1e-10),
        };
        assert_eq!(round_conics(vec![tiny.clone()]), vec![tiny]);
    }

    #[test]
    fn arcs_over_opposite_parts_of_a_circle_are_both_kept() {
        let at = |t: f64| v(det::cos(t), det::sin(t));
        let short = Piece::Arc {
            c: v(0.0, 0.0),
            a: at(0.0),
            b: at(5e-8),
        };
        let long = Piece::Arc {
            c: v(0.0, 0.0),
            a: at(5e-8),
            b: at(0.0),
        };
        assert_eq!(
            drop_coincident(vec![short.clone(), long.clone()], 1e-7).len(),
            2
        );
        assert_eq!(drop_coincident(vec![short.clone(), short], 1e-7).len(), 1);
    }

    #[test]
    fn a_quarter_conic_is_a_counter_clockwise_arc_and_lies_on_it() {
        let out = round_conics(vec![quarter(0.0)]);
        let [arc @ Piece::Arc { c, a, b }] = out.as_slice() else {
            panic!("{out:?}")
        };
        assert!(len(*c) < 1e-12 && dist(*a, v(1.0, 0.0)) < 1e-12 && dist(*b, v(0.0, 1.0)) < 1e-12);
        let m = std::f64::consts::FRAC_PI_4;
        assert!(near(arc, v(det::cos(m), det::sin(m)), 1e-12));
        assert!(
            !near(arc, v(-1.0, 0.0), 1e-6),
            "the other side of the circle"
        );
    }

    #[test]
    fn a_rational_spline_with_double_knots_is_its_conic_spans() {
        let (a, b) = (quarter(0.0), quarter(std::f64::consts::FRAC_PI_2));
        let (
            Piece::Conic {
                a: p0,
                apex: p1,
                b: p2,
                rho,
            },
            Piece::Conic {
                apex: p3, b: p4, ..
            },
        ) = (&a, &b)
        else {
            unreachable!()
        };
        let w = rho / (1.0 - rho);
        // Two quarter conics as one spline with a double knot between them.
        let spline = Piece::Spline {
            pts: vec![*p0, *p1, *p2, *p3, *p4],
            control: true,
            degree: 2,
            periodic: false,
            nurbs: Some((
                vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
                vec![1.0, w, 1.0, w, 1.0],
            )),
        };
        let spans = conic_spans(spline);
        assert_eq!(spans.len(), 2, "{spans:?}");
        for (got, want) in spans.iter().zip([a, b]) {
            let (Piece::Conic { rho: r1, .. }, Piece::Conic { rho: r2, .. }) = (got, &want) else {
                panic!("{got:?}")
            };
            assert!((r1 - r2).abs() < 1e-15);
        }
        // Single inner knots are not spans of conics.
        let smooth = Piece::Spline {
            pts: vec![v(0.0, 0.0), v(1.0, 1.0), v(2.0, 0.0), v(3.0, 1.0)],
            control: true,
            degree: 2,
            periodic: false,
            nurbs: Some((vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], vec![1.0; 4])),
        };
        assert_eq!(conic_spans(smooth.clone()), vec![smooth]);
    }

    #[test]
    fn a_conic_seen_edge_on_is_the_line_it_covers() {
        // Its control past its end: the curve runs out beyond b and back.
        let g = flat_conic(Piece::Conic {
            a: v(0.0, 0.0),
            apex: v(4.0, 0.0),
            b: v(2.0, 0.0),
            rho: 0.4,
        });
        let Piece::Line(p, q) = g else {
            panic!("{g:?}")
        };
        assert!(
            dist(p, v(0.0, 0.0)) < 1e-12 && q.x > 2.0 && q.y.abs() < 1e-12,
            "{g:?}"
        );
    }

    #[test]
    fn lines_on_one_line_and_repeated_pieces_are_drawn_once() {
        let out = drop_coincident(
            vec![
                Piece::Line(v(0.0, 0.0), v(2.0, 0.0)),
                Piece::Circle(v(5.0, 5.0), 1.0),
                Piece::Line(v(3.0, 0.0), v(1.0, 0.0)),
                Piece::Circle(v(5.0, 5.0), 1.0),
                Piece::Line(v(0.0, 1.0), v(1.0, 1.0)),
            ],
            1e-9,
        );
        assert_eq!(
            out,
            vec![
                Piece::Line(v(0.0, 0.0), v(3.0, 0.0)),
                Piece::Line(v(0.0, 1.0), v(1.0, 1.0)),
                Piece::Circle(v(5.0, 5.0), 1.0),
            ]
        );
    }

    #[test]
    fn ends_within_the_gap_meet_at_the_first() {
        let mut g = vec![
            Piece::Line(v(0.0, 0.0), v(1.0, 0.0)),
            Piece::Line(v(1.0 + 1e-6, 0.0), v(1.0, 1.0)),
        ];
        meet_at_shared_ends(&mut g, 1e-5);
        assert_eq!(g[1], Piece::Line(v(1.0, 0.0), v(1.0, 1.0)));
    }
}
