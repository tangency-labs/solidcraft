//! The surfaces of a contour flange's profile: the drawn profile of lines and circular arcs
//! moved a depth through the sheet, end to end again; and a closed profile moved the same way,
//! which also joins the corner where it closes (a lip's band edge).
//!
//! Not upstream (SolidCraft's contour flange orientation, #6790, moved into the kernel,
//! docs/layers.md §1).
//!
//! References: W. Tiller, E. G. Hanson, "Offsets of Two-Dimensional Profiles", IEEE Computer
//! Graphics and Applications 4(9), 1984, as given in Patrikalakis, Maekawa and Cho, *Shape
//! Interrogation for Computer Aided Design and Manufacturing*, §11.2.4: each piece offset,
//! consecutive offsets met where they cross, loops removed as a later step (here the caller's).
//! How an arc's ends move: `plan/findings/develop.contour--long-arc-cut-short-at-its-corners.md`.
//!
//! Deviations: none (no upstream counterpart). No recursion.
use crate::math::det;
use std::collections::BTreeSet;
use std::f64::consts::TAU;

use super::plane::Planar;
use super::seg::SegMeasure;
use crate::math::Vec2;
use crate::prism::profile::Seg2;

/// Why a contour profile has no surfaces at the depth asked: an application words it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourRefusal {
    /// A piece of the profile has no length, so no direction to shift it across.
    ZeroLength,
    /// Two lines meet turning back on each other, so their shifted lines never meet.
    TurnsBack,
    /// At a sharp corner beside an arc, the shifted line and circle (or two circles) miss.
    CornersMiss,
    /// A line is shorter than the shifted corners at its ends leave it.
    LineTooShort,
    /// An arc's shifted radius is no larger than the sheet is thick where the material lies
    /// inside it (no larger than zero otherwise).
    RadiusTooSmall,
    /// An arc is shorter than the shifted corners at its ends leave it.
    ArcTooShort,
}

/// The surfaces of a contour flange's profile `segs` drawn `depth` into the material from the
/// face the surfaces are: each line shifted, each arc's radius changed, end to end again. The
/// material lies on the right of the travel, or on the left when `flip`; a surface lies on the
/// other side of the profile, and `thickness` is the sheet's. Where two pieces meet turning less
/// than `smooth` radians the shifted pieces meet on the common normal; at a sharper corner they
/// meet where the shifted curves cross (two lines: `corner + depth·(n1+n2)/(1+n1·n2)`; a line or
/// an arc beside an arc: the crossing of the shifted line and circle, or of the two circles,
/// nearest the corner), and an arc there ends at that crossing. A depth of zero gives the profile
/// back. A free-form piece is given back as it is: a contour flange refuses one before this.
///
/// # Errors
/// A [`ContourRefusal`] naming why the shifted profile does not hold together.
pub fn offset_contour(
    segs: &[Seg2],
    depth: f64,
    thickness: f64,
    flip: bool,
    smooth: f64,
) -> Result<Vec<Seg2>, ContourRefusal> {
    offset_profile(segs, depth, thickness, flip, smooth, false)
}

/// A closed profile `segs` moved `depth` across its plane, as [`offset_contour`] moves an open
/// one, where the last piece ends where the first starts and meets it there at a corner like any
/// other: at a sharp one beside an arc, both moved pieces end where the shifted curves cross
/// nearest the corner. A lip's band edge is its face's loop moved this way. Only neighbouring
/// pieces are met: moved past a neck, the loop can cross itself, so a caller checks the moved loop
/// ([`crate::prism::profile::loop_crosses`]) before it builds on it.
///
/// # Errors
/// A [`ContourRefusal`] naming why the shifted profile does not hold together.
pub fn offset_closed_contour(
    segs: &[Seg2],
    depth: f64,
    thickness: f64,
    flip: bool,
    smooth: f64,
) -> Result<Vec<Seg2>, ContourRefusal> {
    offset_profile(segs, depth, thickness, flip, smooth, true)
}

/// [`offset_contour`] for an open profile, [`offset_closed_contour`] for a `closed` one.
fn offset_profile(
    segs: &[Seg2],
    depth: f64,
    thickness: f64,
    flip: bool,
    smooth: f64,
    closed: bool,
) -> Result<Vec<Seg2>, ContourRefusal> {
    if depth == 0.0 {
        return Ok(segs.to_vec());
    }
    let side = if flip { -1.0 } else { 1.0 };
    // The way a surface lies from the profile at a point of travel `t`: away from the material.
    let away = |t: Vec2| t.normalized().map(|d| d.perp() * side);
    let short = || ContourRefusal::ZeroLength;
    // Where the shifted pieces meet at the corner where `a` ends and `b` starts, and whether an
    // arc beside it is cut anew there (a sharp corner that is not two lines').
    let joint = |a: &Seg2, b: &Seg2| -> Result<(Vec2, bool), ContourRefusal> {
        let (na, nb) = (
            away(a.end_tangent()).ok_or_else(short)?,
            away(b.start_tangent()).ok_or_else(short)?,
        );
        let (da, db) = (
            a.end_tangent().normalized().ok_or_else(short)?,
            b.start_tangent().normalized().ok_or_else(short)?,
        );
        let turn = det::atan2(da.cross(db).abs(), da.dot(db));
        let corner = a.end();
        Ok(if turn <= smooth {
            (corner + na * depth, false)
        } else if matches!((a, b), (Seg2::Line { .. }, Seg2::Line { .. })) {
            let meet = 1.0 + na.dot(nb);
            if !(meet > 1e-9) {
                return Err(ContourRefusal::TurnsBack);
            }
            (corner + (na + nb) * (depth / meet), false)
        } else {
            let (oa, ob) = (
                shifted(a, na, corner, depth, flip),
                shifted(b, nb, corner, depth, flip),
            );
            let x = shifted_crossing(oa, ob, corner).ok_or(ContourRefusal::CornersMiss)?;
            (x, true)
        })
    };
    // The joints (indices into `at`) at sharp corners, where an arc beside them is cut anew.
    let mut sharp = BTreeSet::new();
    let (first, last) = (
        segs.first().ok_or_else(short)?,
        segs.last().ok_or_else(short)?,
    );
    // An open profile's ends move square to it; a closed one's last piece meets its first at a
    // corner, the start of the first piece and the end of the last (joints 0 and `segs.len()`).
    let (begin, end) = if closed {
        let (x, cut) = joint(last, first)?;
        if cut {
            sharp.extend([0, segs.len()]);
        }
        (x, x)
    } else {
        (
            first.start() + away(first.start_tangent()).unwrap_or(Vec2::ZERO) * depth,
            last.end() + away(last.end_tangent()).unwrap_or(Vec2::ZERO) * depth,
        )
    };
    let mut at = vec![begin];
    for pair in segs.windows(2) {
        let [a, b] = pair else { continue };
        let (x, cut) = joint(a, b)?;
        if cut {
            sharp.insert(at.len());
        }
        at.push(x);
    }
    at.push(end);
    let mut out = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        let (Some(&from), Some(&to)) = (at.get(i), at.get(i + 1)) else {
            return Err(short());
        };
        out.push(match *seg {
            Seg2::Line { a, b } => {
                let d = (b - a).normalized().ok_or_else(short)?;
                if !((to - from).dot(d) > 1e-9) {
                    return Err(ContourRefusal::LineTooShort);
                }
                Seg2::Line { a: from, b: to }
            }
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (inward, radius) = shifted_radius(radius, sweep, depth, flip);
                if !(radius.is_finite() && radius - if inward { thickness } else { 0.0 } > 1e-9) {
                    return Err(ContourRefusal::RadiusTooSmall);
                }
                // An arc beside a sharp corner now ends where the shifted curves cross there.
                let angle = |p: Vec2| (p - center).angle();
                let begin = if sharp.contains(&i) {
                    angle(from)
                } else {
                    start
                };
                let end = if sharp.contains(&(i + 1)) {
                    angle(to)
                } else {
                    start + sweep
                };
                // The drawn sweep, changed by how far each end moved along the circle (less than
                // half a turn: the crossing nearest its corner). Wrapping the two moves together
                // instead would take a long arc cut much shorter for one a whole turn longer.
                let moved = |x: f64| x - TAU * (x / TAU).round();
                let cut = sweep + moved(end - (start + sweep)) - moved(begin - start);
                if !(cut.signum() == sweep.signum() && cut.abs() > 1e-9) {
                    return Err(ContourRefusal::ArcTooShort);
                }
                Seg2::Arc {
                    center,
                    radius,
                    start: begin,
                    sweep: cut,
                }
            }
            _ => *seg,
        });
    }
    Ok(out)
}

/// A drawn arc's radius once its surface lies `depth` from it, away from the material, and
/// whether the material lies toward its center (the drawn arc is then the roll's outside and the
/// surface moves outward).
fn shifted_radius(radius: f64, sweep: f64, depth: f64, flip: bool) -> (bool, f64) {
    let inward = (sweep < 0.0) != flip;
    (
        inward,
        if inward {
            radius + depth
        } else {
            radius - depth
        },
    )
}

/// A profile piece's surface near a corner, extended past its ends.
#[derive(Clone, Copy)]
enum Shifted {
    /// Through `p` along `d` (unit).
    Line { p: Vec2, d: Vec2 },
    /// About `c` at radius `r`.
    Circle { c: Vec2, r: f64 },
}

/// The surface of the piece `seg` that ends or starts at `corner`, where its normal away from
/// the material is `n`, lying `depth` from it.
fn shifted(seg: &Seg2, n: Vec2, corner: Vec2, depth: f64, flip: bool) -> Shifted {
    match *seg {
        Seg2::Arc {
            center,
            radius,
            sweep,
            ..
        } => Shifted::Circle {
            c: center,
            r: shifted_radius(radius, sweep, depth, flip).1,
        },
        _ => Shifted::Line {
            p: corner + n * depth,
            d: Vec2::new(n.y, -n.x),
        },
    }
}

/// Where two shifted surfaces cross, the crossing nearest `corner` when there are two; `None`
/// where they miss (or are parallel lines).
fn shifted_crossing(a: Shifted, b: Shifted, corner: Vec2) -> Option<Vec2> {
    let nearest = |[p0, p1]: [Vec2; 2]| {
        if p1.dist(corner) < p0.dist(corner) {
            p1
        } else {
            p0
        }
    };
    let line_circle = |p: Vec2, d: Vec2, c: Vec2, r: f64| {
        // |p + t d - c| = r with |d| = 1: t² + 2 t (d·(p-c)) + |p-c|² - r² = 0.
        let w = p - c;
        let (half_b, q) = (d.dot(w), w.dot(w) - r * r);
        let disc = half_b * half_b - q;
        (disc >= 0.0).then(|| {
            let root = disc.sqrt();
            nearest([p + d * (-half_b - root), p + d * (-half_b + root)])
        })
    };
    match (a, b) {
        (Shifted::Line { p, d }, Shifted::Circle { c, r })
        | (Shifted::Circle { c, r }, Shifted::Line { p, d }) => line_circle(p, d, c, r),
        (Shifted::Circle { c: c0, r: r0 }, Shifted::Circle { c: c1, r: r1 }) => {
            let between = c1 - c0;
            let dd = between.dot(between);
            if !(dd > 0.0) {
                return None;
            }
            let dist = dd.sqrt();
            let along = (dd + r0 * r0 - r1 * r1) / (2.0 * dist);
            let h2 = r0 * r0 - along * along;
            (h2 >= 0.0).then(|| {
                let u = between.over(dist);
                let (mid, across) = (c0 + u * along, u.perp() * h2.sqrt());
                nearest([mid + across, mid - across])
            })
        }
        (Shifted::Line { p: p0, d: d0 }, Shifted::Line { p: p1, d: d1 }) => {
            let det = d0.cross(d1);
            (det.abs() > 1e-12).then(|| p0 + d0 * ((p1 - p0).cross(d1) / det))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    const SMOOTH: f64 = 1e-6;

    /// A contour profile lying 1 into a sheet 2 thick, with the material on the right of the
    /// travel. A line along x to (10, 0) meets, at a right angle, a quarter turn of radius 10
    /// about the origin up to (0, 10). The line's surface is y = 1 and the arc's the circle of
    /// radius 9 (the material is outside it), so they cross at (√80, 1): the line ends there and
    /// the arc starts at atan2(1, √80) and still ends at (0, 9).
    #[test]
    fn a_sharp_corner_at_an_arc_meets_where_the_shifted_line_and_circle_cross() {
        let segs = [
            Seg2::Line {
                a: Vec2::ZERO,
                b: Vec2::new(10.0, 0.0),
            },
            Seg2::Arc {
                center: Vec2::ZERO,
                radius: 10.0,
                start: 0.0,
                sweep: PI / 2.0,
            },
        ];
        let out = offset_contour(&segs, 1.0, 2.0, false, SMOOTH).unwrap();
        let x = 80f64.sqrt();
        let Seg2::Line { a, b } = out[0] else {
            panic!("{out:?}")
        };
        assert!(a.dist(Vec2::new(0.0, 1.0)) < 1e-12, "{a:?}");
        assert!(
            (b.x - x).abs() < 1e-12 && (b.y - 1.0).abs() < 1e-12,
            "{b:?}"
        );
        let Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } = out[1]
        else {
            panic!("{out:?}")
        };
        let begin = det::atan2(1f64, x);
        assert_eq!((center, radius), (Vec2::ZERO, 9.0));
        assert!(
            (start - begin).abs() < 1e-12 && (sweep - (PI / 2.0 - begin)).abs() < 1e-12,
            "{start} {sweep}"
        );
        // The other way round (material on the left) the surfaces swap sides: y = −1 and
        // radius 11.
        let out = offset_contour(&segs, 1.0, 2.0, true, SMOOTH).unwrap();
        let Seg2::Arc { radius, start, .. } = out[1] else {
            panic!("{out:?}")
        };
        assert_eq!(radius, 11.0);
        assert!(
            (start - det::atan2(-1f64, 120f64.sqrt())).abs() < 1e-12,
            "{start}"
        );
    }

    /// Two quarter turns of radius 10 meeting at a right angle at (10, 0): up round the origin
    /// from (0, −10), then on round (10, 10) to (20, 10). With the material on the right both
    /// surfaces are circles of radius 9, whose centers are 10√2 apart: they cross at
    /// (5 ± k, 5 ∓ k) with k = √15.5, and the one by the corner is (5 + k, 5 − k).
    #[test]
    fn a_sharp_corner_between_two_arcs_meets_where_the_shifted_circles_cross() {
        let segs = [
            Seg2::Arc {
                center: Vec2::ZERO,
                radius: 10.0,
                start: -PI / 2.0,
                sweep: PI / 2.0,
            },
            Seg2::Arc {
                center: Vec2::new(10.0, 10.0),
                radius: 10.0,
                start: -PI / 2.0,
                sweep: PI / 2.0,
            },
        ];
        let out = offset_contour(&segs, 1.0, 2.0, false, SMOOTH).unwrap();
        let k = 15.5f64.sqrt();
        let meet = Vec2::new(5.0 + k, 5.0 - k);
        let Seg2::Arc {
            radius,
            start,
            sweep,
            ..
        } = out[0]
        else {
            panic!("{out:?}")
        };
        assert_eq!((radius, start), (9.0, -PI / 2.0));
        assert!(
            (sweep - (det::atan2(meet.y, meet.x) + PI / 2.0)).abs() < 1e-12,
            "{sweep}"
        );
        let Seg2::Arc {
            radius,
            start,
            sweep,
            ..
        } = out[1]
        else {
            panic!("{out:?}")
        };
        let begin = det::atan2(meet.y - 10.0, meet.x - 10.0);
        assert_eq!(radius, 9.0);
        assert!(
            (start - begin).abs() < 1e-12 && (start + sweep).abs() < 1e-12,
            "{start} {sweep}"
        );
    }

    /// Two lines at a right angle shift to meet at `corner + depth·(n1+n2)`; lines that turn
    /// back on each other, an arc thinner than the sheet and an empty profile are refused; a
    /// depth of zero gives the profile back.
    #[test]
    fn lines_meet_on_the_bisector_and_bad_profiles_are_refused() {
        let ell = [
            Seg2::Line {
                a: Vec2::ZERO,
                b: Vec2::new(10.0, 0.0),
            },
            Seg2::Line {
                a: Vec2::new(10.0, 0.0),
                b: Vec2::new(10.0, 10.0),
            },
        ];
        let out = offset_contour(&ell, 1.0, 2.0, false, SMOOTH).unwrap();
        let Seg2::Line { b, .. } = out[0] else {
            panic!("{out:?}")
        };
        assert!(b.dist(Vec2::new(9.0, 1.0)) < 1e-12, "{b:?}");
        assert_eq!(
            offset_contour(&ell, 0.0, 2.0, false, SMOOTH),
            Ok(ell.to_vec())
        );
        let back = [
            Seg2::Line {
                a: Vec2::ZERO,
                b: Vec2::new(10.0, 0.0),
            },
            Seg2::Line {
                a: Vec2::new(10.0, 0.0),
                b: Vec2::new(0.0, 0.0),
            },
        ];
        assert_eq!(
            offset_contour(&back, 1.0, 2.0, false, SMOOTH),
            Err(ContourRefusal::TurnsBack)
        );
        // Material inside a radius 1.5 arc, the sheet 2 thick: the inner face would pass the axis.
        let thin = [Seg2::Arc {
            center: Vec2::ZERO,
            radius: 1.5,
            start: 0.0,
            sweep: -PI / 2.0,
        }];
        assert_eq!(
            offset_contour(&thin, 0.4, 2.0, false, SMOOTH),
            Err(ContourRefusal::RadiusTooSmall)
        );
        assert_eq!(
            offset_contour(&[], 1.0, 2.0, false, SMOOTH),
            Err(ContourRefusal::ZeroLength)
        );
    }

    /// A quarter disc of radius 10 run counter-clockwise, its arc first: from (10, 0) round the
    /// origin to (0, 10), down the y axis, and along the x axis back to (10, 0), where the loop
    /// closes at a sharp corner beside the arc. Moved 1 to the left of the travel (inside), the
    /// lines are x = 1 and y = 1 and the arc's circle has radius 9; with s = √80 they cross at
    /// (1, s), (1, 1) and, where the loop closes, (s, 1). So the arc runs from atan2(1, s) to
    /// atan2(s, 1), and both lines end at those crossings. Moved 1 to the right (outside), the
    /// lines are x = −1 and y = −1, the circle has radius 11, and the loop closes at (√120, −1).
    #[test]
    fn a_closed_profile_meets_where_its_last_and_first_pieces_cross_beside_an_arc() {
        let quarter = [
            Seg2::Arc {
                center: Vec2::ZERO,
                radius: 10.0,
                start: 0.0,
                sweep: PI / 2.0,
            },
            Seg2::Line {
                a: Vec2::new(0.0, 10.0),
                b: Vec2::ZERO,
            },
            Seg2::Line {
                a: Vec2::ZERO,
                b: Vec2::new(10.0, 0.0),
            },
        ];
        let near = |p: Vec2, x: f64, y: f64| p.dist(Vec2::new(x, y)) < 1e-12;
        let out = offset_closed_contour(&quarter, 1.0, 0.0, false, SMOOTH).unwrap();
        let s = 80f64.sqrt();
        let [Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        }, Seg2::Line { a: a1, b: b1 }, Seg2::Line { a: a2, b: b2 }] = out[..]
        else {
            panic!("{out:?}")
        };
        assert_eq!((center, radius), (Vec2::ZERO, 9.0));
        let begin = det::atan2(1f64, s);
        assert!(
            (start - begin).abs() < 1e-12 && (sweep - (det::atan2(s, 1.0) - begin)).abs() < 1e-12,
            "{start} {sweep}"
        );
        assert!(near(a1, 1.0, s) && near(b1, 1.0, 1.0), "{a1:?} {b1:?}");
        assert!(near(a2, 1.0, 1.0) && near(b2, s, 1.0), "{a2:?} {b2:?}");
        let out = offset_closed_contour(&quarter, 1.0, 0.0, true, SMOOTH).unwrap();
        let s = 120f64.sqrt();
        let [Seg2::Arc {
            radius,
            start,
            sweep,
            ..
        }, Seg2::Line { a: a1, b: b1 }, Seg2::Line { a: a2, b: b2 }] = out[..]
        else {
            panic!("{out:?}")
        };
        let begin = det::atan2(-1f64, s);
        assert_eq!(radius, 11.0);
        assert!(
            (start - begin).abs() < 1e-12 && (sweep - (det::atan2(s, -1.0) - begin)).abs() < 1e-12,
            "{start} {sweep}"
        );
        assert!(near(a1, -1.0, s) && near(b1, -1.0, -1.0), "{a1:?} {b1:?}");
        assert!(near(a2, -1.0, -1.0) && near(b2, s, -1.0), "{a2:?} {b2:?}");
        // Moved 6 inside, the line y = 6 misses the circle of radius 4.
        assert_eq!(
            offset_closed_contour(&quarter, 6.0, 0.0, false, SMOOTH),
            Err(ContourRefusal::CornersMiss)
        );
        // A depth that is not a number moves the line nowhere, so it meets no circle either.
        assert_eq!(
            offset_closed_contour(&quarter, f64::NAN, 0.0, false, SMOOTH),
            Err(ContourRefusal::CornersMiss)
        );
    }

    /// A circle of radius 10 cut by the chord y = −6, run counter-clockwise: the long arc from
    /// (8, −6) round to (−8, −6), sweeping π + 2·atan2(6, 8), then the chord back. Moved 7.875
    /// inside, the circle has radius 2.125 and the chord is y = 1.875, crossing at (±1, 1.875):
    /// the arc is now the short one from atan2(1.875, 1) to atan2(1.875, −1), not a whole turn
    /// longer.
    #[test]
    fn a_long_arc_cut_short_by_its_corners_keeps_less_than_a_turn() {
        let k = det::atan2(6f64, 8.0);
        let cut = [
            Seg2::Arc {
                center: Vec2::ZERO,
                radius: 10.0,
                start: -k,
                sweep: PI + 2.0 * k,
            },
            Seg2::Line {
                a: Vec2::new(-8.0, -6.0),
                b: Vec2::new(8.0, -6.0),
            },
        ];
        let out = offset_closed_contour(&cut, 7.875, 0.0, false, SMOOTH).unwrap();
        let [Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        }, Seg2::Line { a, b }] = out[..]
        else {
            panic!("{out:?}")
        };
        let begin = det::atan2(1.875f64, 1.0);
        assert_eq!((center, radius), (Vec2::ZERO, 2.125));
        assert!(
            (start - begin).abs() < 1e-12
                && (sweep - (det::atan2(1.875f64, -1.0) - begin)).abs() < 1e-12,
            "{start} {sweep}"
        );
        assert!(
            a.dist(Vec2::new(-1.0, 1.875)) < 1e-12 && b.dist(Vec2::new(1.0, 1.875)) < 1e-12,
            "{a:?} {b:?}"
        );
    }

    /// A lens of two arcs of radius 5 run counter-clockwise: about (0, 3) from (−4, 0) to (4, 0),
    /// then about (0, −3) back, where the loop closes at a sharp corner. Moved 1 inside, both
    /// circles have radius 4 and cross at (±√7, 0): each arc runs between those, the lower from
    /// atan2(−3, −√7) to atan2(−3, √7) and the upper from atan2(3, √7) to atan2(3, −√7). A whole
    /// circle closes smoothly on itself and stays a whole circle of radius 9.
    #[test]
    fn a_closed_lens_of_two_arcs_meets_where_the_shifted_circles_cross() {
        let k = det::atan2(3f64, 4.0);
        let lens = [
            Seg2::Arc {
                center: Vec2::new(0.0, 3.0),
                radius: 5.0,
                start: -PI + k,
                sweep: PI - 2.0 * k,
            },
            Seg2::Arc {
                center: Vec2::new(0.0, -3.0),
                radius: 5.0,
                start: k,
                sweep: PI - 2.0 * k,
            },
        ];
        let out = offset_closed_contour(&lens, 1.0, 0.0, false, SMOOTH).unwrap();
        let r7 = 7f64.sqrt();
        let [Seg2::Arc {
            center: c0,
            radius: r0,
            start: s0,
            sweep: w0,
        }, Seg2::Arc {
            center: c1,
            radius: r1,
            start: s1,
            sweep: w1,
        }] = out[..]
        else {
            panic!("{out:?}")
        };
        assert_eq!(
            (c0, r0, c1, r1),
            (Vec2::new(0.0, 3.0), 4.0, Vec2::new(0.0, -3.0), 4.0)
        );
        let (b0, b1) = (det::atan2(-3f64, -r7), det::atan2(3f64, r7));
        assert!(
            (s0 - b0).abs() < 1e-12 && (w0 - (det::atan2(-3f64, r7) - b0)).abs() < 1e-12,
            "{s0} {w0}"
        );
        assert!(
            (s1 - b1).abs() < 1e-12 && (w1 - (det::atan2(3f64, -r7) - b1)).abs() < 1e-12,
            "{s1} {w1}"
        );
        let circle = [Seg2::Arc {
            center: Vec2::ZERO,
            radius: 10.0,
            start: 0.0,
            sweep: TAU,
        }];
        assert_eq!(
            offset_closed_contour(&circle, 1.0, 0.0, false, SMOOTH),
            Ok(vec![Seg2::Arc {
                center: Vec2::ZERO,
                radius: 9.0,
                start: 0.0,
                sweep: TAU,
            }])
        );
    }
}
