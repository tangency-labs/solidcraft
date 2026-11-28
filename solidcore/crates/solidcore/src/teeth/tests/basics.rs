//! The shared core: pairs, outlines made of lines and splines, and the spline evaluation; the
//! measurement's work bounded whatever the reach and however many steps a pair is turned through;
//! outlines that cross themselves or run the wrong way refused by `Outline::new` and `Body::new`
//! whichever module made them; and parts placed where they are not numbers refused by the
//! measurement. Also the certificates of the chords: the enclosures of `bound`, the proved
//! sampling of curves, arcs and splines, and the old sampling kept as the oracle that the proved
//! one is never coarser.

use crate::math::det;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use super::common::{check_outline, lift, not_proved, out_of_range, typed_why, why, Dice};
use crate::blend::fillet_section::interval::Ivl;
use crate::error::teeth::{TeethFailure, Why, CHORD};
use crate::geom::certified::next_up;
use crate::nurbs::math::UNIT;
use crate::teeth::bound::{self, Bend};
use crate::teeth::fit::fitted;
use crate::teeth::involute::{hub_for, spline_pair, External, Fit};
use crate::teeth::mesh::{gap, Body};
use crate::teeth::outline::{
    arc_steps, break_rounding, curve_tolerance, distance, less_gap, on_circle, points_of, sampled,
    stretch_bends,
};
use crate::teeth::pair::{MAX_STEPS, MOST_TURNING_WORK};
use crate::teeth::{
    bspline, Material, Motion, Outline, Pair, Part, Piece, Point, Role, MAX_CURVE_POINTS,
};

/// Today's `outline::sampled`, kept as the oracle of steps 4 and 5: three probes per stretch,
/// `START = 8`, a stretch that cannot be halved accepted. Copied letter for letter from
/// outline.rs, its errors as `TeethFailure` (`OutlineNotFinite`, `CurveTooFine`).
pub(super) fn old_sampled(
    at: &dyn Fn(f64) -> Point,
    from: f64,
    to: f64,
    tolerance: f64,
) -> Result<Vec<Point>, TeethFailure> {
    const TOO_FINE: TeethFailure = TeethFailure::Invalid(Why::CurveTooFine);
    const START: u32 = 8; // pieces to begin with, so that no bend is overlooked
    let place = |i: u32| from + (to - from) * f64::from(i) / f64::from(START);
    let mut out = vec![at(from)];
    // The stretches still to settle, the next one on top.
    let mut waiting: Vec<(f64, Point, f64, Point)> = (0..START)
        .rev()
        .map(|i| (place(i), at(place(i)), place(i + 1), at(place(i + 1))))
        .collect();
    while let Some((t0, p0, t1, p1)) = waiting.pop() {
        let (quarter, middle, three_quarters) = (
            0.75 * t0 + 0.25 * t1,
            0.5 * (t0 + t1),
            0.25 * t0 + 0.75 * t1,
        );
        let at_middle = at(middle);
        let off = [at(quarter), at_middle, at(three_quarters)]
            .into_iter()
            .map(|p| off_chord(p, p0, p1))
            .fold(0.0, f64::max);
        // A stretch that cannot be halved any more is as fine as numbers allow.
        let halvable = middle > t0.min(t1) && middle < t0.max(t1);
        if off.is_nan() {
            return Err(TeethFailure::Invalid(Why::OutlineNotFinite));
        }
        if off > tolerance && halvable {
            if out.len() + waiting.len() >= MAX_CURVE_POINTS {
                return Err(TOO_FINE);
            }
            waiting.push((middle, at_middle, t1, p1));
            waiting.push((t0, p0, middle, at_middle));
        } else {
            out.push(p1);
        }
    }
    Ok(out)
}

/// How far `p` is from the chord between `a` and `b`: outline.rs's own, copied for the oracle.
fn off_chord(p: Point, a: Point, b: Point) -> f64 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let length2 = ex * ex + ey * ey;
    if length2 > 0.0 {
        let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0);
        distance(p, [a[0] + t * ex, a[1] + t * ey])
    } else {
        distance(p, a)
    }
}

/// Today's `outline::arc_steps`, kept as the oracle of the arc counts: copied letter for letter.
fn old_arc_steps(radius: f64, sweep: f64, tolerance: f64) -> usize {
    if !(radius > 0.0) || sweep == 0.0 {
        return 1;
    }
    let ratio = (tolerance / radius).clamp(1e-15, 1.0);
    // The angle of a chord that is this far from its arc in the middle
    let step = 2.0 * det::acos(1.0 - ratio);
    let steps = if step > 0.0 {
        (sweep.abs() / step).ceil()
    } else {
        f64::INFINITY
    };
    // A count in range, so the cast loses nothing.
    if steps >= (MAX_CURVE_POINTS + 1) as f64 {
        MAX_CURVE_POINTS + 1
    } else if steps >= 1.0 {
        steps as usize
    } else {
        1
    }
}

/// The circle of `radius` about the origin, `P(t) = radius (cos t, sin t)`.
fn circle_at(radius: f64) -> impl Fn(f64) -> Point {
    move |t: f64| [radius * det::cos(t), radius * det::sin(t)]
}

/// What [`circle_at`] does over a stretch: its derivatives enclosed through `bound::sin_cos`.
fn circle_bend(radius: f64) -> impl Fn(f64, f64) -> Bend {
    move |lo: f64, hi: f64| {
        let (s, c) = bound::sin_cos(Ivl::new(lo, hi));
        let r = bound::exact(radius);
        Bend {
            first: [bound::neg(s) * r, c * r],
            second: [bound::neg(c) * r, bound::neg(s) * r],
            second_size: radius,
        }
    }
}

/// A proved bound of the distance of a computed point of [`circle_at`] from the circle's true
/// point at `t`: the point measured against `radius (cos t, sin t)` enclosed from the series.
fn circle_off(radius: f64) -> impl Fn(f64, Point) -> f64 {
    move |t: f64, p: Point| {
        let (s, c) = bound::sin_cos_at(t);
        let r = bound::exact(radius);
        bound::off(p, [r * c, r * s])
    }
}

/// The largest distance from `count` points of the curve `at` over `[from, to]` (both ends
/// included) to the polyline through `chain`, each point measured against every chord.
fn farthest(chain: &[Point], at: &dyn Fn(f64) -> Point, from: f64, to: f64, count: usize) -> f64 {
    (0..count)
        .map(|k| {
            let p = at(from + (to - from) * k as f64 / (count - 1) as f64);
            chain
                .windows(2)
                .map(|w| off_chord(p, w[0], w[1]))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// The distance from `p` to the polyline `chain`, whose points run with x increasing, looking only
/// at the chords whose x comes within `reach` of that of `p`: exact whenever the distance is at
/// most `reach`, and more than `reach` otherwise (infinite where no chord comes that near).
fn near_graph(chain: &[Point], p: Point, reach: f64) -> f64 {
    let first = chain
        .partition_point(|q| q[0] < p[0] - reach)
        .saturating_sub(1);
    chain
        .iter()
        .skip(first)
        .zip(chain.iter().skip(first + 1))
        .take_while(|(a, _)| a[0] <= p[0] + reach)
        .map(|(a, b)| off_chord(p, *a, *b))
        .fold(f64::INFINITY, f64::min)
}

/// A spline of `degree` with `spans` spans from the dice: knots one to three apart (the ends
/// repeated where `clamped`), control points in the square of half side `size` about the origin.
fn random_spline(
    dice: &mut Dice,
    degree: usize,
    spans: usize,
    clamped: bool,
    size: f64,
) -> (Vec<f64>, Vec<Point>) {
    let n = degree + spans;
    let mut knots = Vec::new();
    let mut k = 0.0;
    while knots.len() < n + degree + 1 {
        knots.push(k);
        k += 1.0 + (dice.roll() % 101) as f64 / 50.0;
    }
    if clamped {
        let (from, to) = (knots[degree], knots[n]);
        knots[..degree].fill(from);
        knots[n + 1..].fill(to);
    }
    let mut coordinate = || size * ((dice.roll() % 2001) as f64 / 1000.0 - 1.0);
    let control = (0..n).map(|_| [coordinate(), coordinate()]).collect();
    (knots, control)
}

fn shaft() -> External {
    External {
        module: 1.5,
        teeth: 20,
        pressure_angle: 30.0,
        addendum: 0.5,
        dedendum: 0.85,
        shift: 0.0,
        tool_tip_radius: 0.4,
        thinning: 0.0,
    }
}

fn fit() -> Fit {
    Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    }
}

/// A square of side 2 about the origin as four exact lines, a quarter of it as the tooth.
fn square(half: f64, tolerance: f64) -> crate::Result<Outline> {
    Outline::new(
        4,
        vec![Piece::Line {
            from: [half, -half],
            to: [half, half],
            role: Role::Flank,
        }],
        tolerance,
    )
}

fn circle(centre: f64, radius: f64, sides: usize) -> Vec<Point> {
    (0..sides)
        .map(|i| {
            let a = TAU * i as f64 / sides as f64;
            [centre + radius * det::cos(a), radius * det::sin(a)]
        })
        .collect()
}

fn bar(x0: f64, y0: f64, x1: f64, y1: f64) -> crate::Result<Body> {
    Body::new(
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        Material::Inside,
    )
}

/// The slow way: whether two chords cross, each passing from one side of the other to the other
/// side.
fn cross(p: Point, q: Point, u: Point, v: Point) -> bool {
    let s = |f: Point, t: Point, o: Point| {
        (t[0] - f[0]) * (o[1] - f[1]) - (t[1] - f[1]) * (o[0] - f[0])
    };
    s(u, v, p) * s(u, v, q) < 0.0 && s(p, q, u) * s(p, q, v) < 0.0
}

/// How many pairs of chords of a closed chain cross, every chord against every other.
fn self_crossings(points: &[Point]) -> usize {
    let n = points.len();
    let mut count = 0;
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if cross(
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            ) {
                count += 1;
            }
        }
    }
    count
}

fn crossing_refusal(outline: &crate::Result<Outline>) -> bool {
    why(outline) == Some(Why::CrossesItself)
}

#[test]
fn outlines_of_lines_arcs_and_splines() -> crate::Result<()> {
    check_outline("square", &square(1.0, 1e-3)?);
    let corners = square(1.0, 1e-3)?.points();
    let expected = [[1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]];
    assert!(
        corners.len() == 4
            && corners
                .iter()
                .zip(expected)
                .all(|(&p, q)| super::common::dist(p, q) < 1e-12),
        "{corners:?}"
    );
    // A quarter of a rounded square: a line and a corner arc.
    let rounded = Outline::new(
        4,
        vec![
            Piece::Line {
                from: [1.0, -0.6],
                to: [1.0, 0.6],
                role: Role::Flank,
            },
            Piece::Arc {
                centre: [0.6, 0.6],
                radius: 0.4,
                start: 0.0,
                sweep: FRAC_PI_2,
                role: Role::Fillet,
            },
        ],
        1e-6,
    )?;
    check_outline("rounded square", &rounded);
    let (low, high) = rounded.radii();
    assert!(
        (low - 1.0).abs() < 1e-3 && (high - (0.6 * 2f64.sqrt() + 0.4)).abs() < 1e-5,
        "{low} {high}"
    );
    // Pieces that do not join, a tooth that does not close, and no teeth at all are refused.
    let open = vec![Piece::Line {
        from: [1.0, -1.0],
        to: [1.0, 0.9],
        role: Role::Flank,
    }];
    assert!(Outline::new(4, open, 1e-3).is_err());
    assert!(Outline::new(
        0,
        vec![Piece::Line {
            from: [1.0, -1.0],
            to: [1.0, 1.0],
            role: Role::Flank
        }],
        1e-3
    )
    .is_err());
    assert!(Outline::new(
        3,
        vec![Piece::Line {
            from: [1.0, -1.0],
            to: [1.0, 1.0],
            role: Role::Flank
        }],
        1e-3
    )
    .is_err());
    assert!(Outline::new(
        4,
        vec![Piece::Line {
            from: [1.0, f64::NAN],
            to: [1.0, 1.0],
            role: Role::Flank
        }],
        1e-3
    )
    .is_err());
    assert!(Outline::new(4, Vec::new(), 1e-3).is_err());
    Ok(())
}

/// A quarter circle of radius 1 as a cubic spline through... no: a cubic with one inner knot that
/// is close to it.
fn arc_like() -> (usize, Vec<f64>, Vec<Point>) {
    let k = 0.2652; // places the inner control points so that the curve stays near the unit circle
    let quarter = FRAC_PI_2 / 2.0;
    let (c, s) = (det::cos(quarter), det::sin(quarter));
    let control = vec![
        [1.0, 0.0],
        [1.0, k],
        [c + k * s, s - k * c],
        [c - k * s, s + k * c],
        [k, 1.0],
        [0.0, 1.0],
    ];
    (
        3,
        vec![0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0],
        control,
    )
}

#[test]
fn a_spline_is_evaluated_and_cut_into_chords_within_the_tolerance() -> crate::Result<()> {
    let (degree, knots, control) = arc_like();
    assert_eq!(bspline::range(degree, &knots, &control)?, (0.0, 1.0));
    assert_eq!(
        bspline::breaks(degree, &knots, &control)?,
        vec![0.0, 0.5, 1.0]
    );
    assert_eq!(bspline::point(degree, &knots, &control, 0.0)?, [1.0, 0.0]);
    assert_eq!(bspline::point(degree, &knots, &control, 1.0)?, [0.0, 1.0]);
    assert_eq!(bspline::point(degree, &knots, &control, 7.0)?, [0.0, 1.0]); // taken into the range
    for i in 0..=100 {
        let [x, y] = bspline::point(degree, &knots, &control, f64::from(i) / 100.0)?;
        assert!(
            (det::hypot(x, y) - 1.0).abs() < 2e-4,
            "at {i}: {}",
            det::hypot(x, y)
        );
    }
    // A straight spline of degree 1 is its control polygon.
    let line = bspline::point(1, &[0.0, 0.0, 1.0, 1.0], &[[0.0, 0.0], [2.0, 4.0]], 0.25)?;
    assert!((line[0] - 0.5).abs() < 1e-15 && (line[1] - 1.0).abs() < 1e-15);

    // As a piece of an outline: four of them around the axis, cut into chords within the tolerance.
    for tolerance in [1e-2, 1e-4, 1e-6] {
        let piece = Piece::Spline {
            degree,
            knots: knots.clone(),
            control: control.clone(),
            role: Role::Tip,
        };
        let outline = Outline::new(4, vec![piece], tolerance)?;
        check_outline("spline circle", &outline);
        let points = outline.points();
        for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            // The chord's middle is inside the curve by at most the tolerance (the curve itself is
            // within 2e-4 of the circle).
            assert!(
                1.0 - det::hypot(middle[0], middle[1]) < tolerance + 2e-4,
                "{tolerance}"
            );
        }
        assert!(points.len() >= 8, "{}", points.len());
    }
    Ok(())
}

#[test]
fn what_is_not_a_spline_is_refused() {
    let (degree, knots, control) = arc_like();
    let refused = |degree: usize, knots: &[f64], control: &[Point]| {
        bspline::range(degree, knots, control).is_err()
            && bspline::point(degree, knots, control, 0.5).is_err()
            && bspline::breaks(degree, knots, control).is_err()
            && Outline::new(
                4,
                vec![Piece::Spline {
                    degree,
                    knots: knots.to_vec(),
                    control: control.to_vec(),
                    role: Role::Tip,
                }],
                1e-3,
            )
            .is_err()
    };
    assert!(refused(0, &knots, &control));
    assert!(refused(10, &knots, &control));
    assert!(refused(2, &knots, &control)); // the counts do not fit
    assert!(refused(degree, &knots[1..], &control));
    assert!(refused(degree, &[], &[]));
    let mut unordered = knots.clone();
    unordered.swap(4, 6);
    assert!(refused(degree, &unordered, &control));
    let mut not_finite = control.clone();
    not_finite[2][1] = f64::INFINITY;
    assert!(refused(degree, &knots, &not_finite));
    assert!(refused(degree, &[0.0; 10], &control)); // no range
    assert!(refused(
        1,
        &[0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
        &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    )); // breaks at 0.5
    assert!(bspline::point(degree, &knots, &control, f64::NAN).is_err());
}

#[test]
fn a_checked_spline_gives_exactly_the_points_bspline_point_gives() -> crate::Result<()> {
    let mut dice = Dice(0x5B1);
    let mut compared = 0;
    for round in 0..600 {
        let degree = 1 + round % bspline::MAX_DEGREE;
        let n = degree + 1 + dice.roll() % 30;
        // Knots in order, each value up to `degree` times, every other spline with clamped ends.
        let mut knots: Vec<f64> = Vec::new();
        let mut k = (dice.roll() % 100) as f64 / 10.0 - 5.0;
        while knots.len() < n + degree + 1 {
            for _ in 0..=(dice.roll() % degree) {
                if knots.len() < n + degree + 1 {
                    knots.push(k);
                }
            }
            k += (1 + dice.roll() % 50) as f64 / 7.0;
        }
        if round % 2 == 0 {
            let (from, to) = (knots[degree], knots[n]);
            knots[..degree].fill(from);
            knots[n + 1..].fill(to);
        }
        let control: Vec<Point> = (0..n)
            .map(|_| {
                [
                    (dice.roll() % 10_000) as f64 / 100.0 - 50.0,
                    (dice.roll() % 10_000) as f64 / 100.0 - 50.0,
                ]
            })
            .collect();
        let checked = bspline::Checked::new(degree, &knots, &control);
        assert_eq!(
            checked.is_ok(),
            bspline::range(degree, &knots, &control).is_ok(),
            "round {round}"
        );
        let Ok(spline) = checked else { continue };
        let (from, to) = bspline::range(degree, &knots, &control)?;
        // Every knot, the middles between them, beyond either end, and places in between.
        let mut places = knots.clone();
        places.extend(knots.windows(2).map(|w| 0.5 * (w[0] + w[1])));
        places.extend([from - 1.0, to + 1.0, f64::NEG_INFINITY, f64::INFINITY]);
        places.extend((0..40).map(|_| from + (to - from) * (dice.roll() % 1001) as f64 / 1000.0));
        for t in places {
            let expected = bspline::point(degree, &knots, &control, t)?;
            let found = lift(spline.point(t))?;
            assert!(
                expected[0].to_bits() == found[0].to_bits()
                    && expected[1].to_bits() == found[1].to_bits(),
                "round {round}, degree {degree}, at {t}: {found:?} for {expected:?}"
            );
            compared += 1;
        }
        assert!(spline.point(f64::NAN).is_err());
    }
    assert!(compared > 20_000, "{compared}");
    Ok(())
}

#[test]
fn a_long_spline_is_cut_into_chords_without_checking_it_at_every_point() -> crate::Result<()> {
    // A spline of degree 1 with 2,000 spans along a quarter circle: one tooth of four.
    let spans = 2_000_usize;
    let control: Vec<Point> = (0..=spans)
        .map(|i| {
            let a = -FRAC_PI_4 + FRAC_PI_2 * i as f64 / spans as f64;
            [det::cos(a), det::sin(a)]
        })
        .collect();
    let knots: Vec<f64> = (0..spans + 3)
        .map(|j| j.saturating_sub(1).min(spans) as f64)
        .collect();
    let piece = Piece::Spline {
        degree: 1,
        knots,
        control,
        role: Role::Tip,
    };
    bspline::VISITED.with(|v| v.set(0));
    let outline = Outline::new(4, vec![piece], 1e-3)?;
    let visited = bspline::VISITED.with(std::cell::Cell::get);
    // Each span is evaluated 41 times, each time by the kernel from the degree's two control
    // points, and the whole spline (4,004 knots and control points) is checked a few times: at
    // most 1,500 for each span.
    // Checking the whole spline again at every evaluation looks at more than 300 million.
    assert!(visited <= 3_000_000, "{visited}");
    assert_eq!(outline.points().len(), 4 * 8 * spans);
    Ok(())
}

#[test]
fn turning_is_checked() -> crate::Result<()> {
    let pair = spline_pair(&shaft(), &fit(), 1e-3)?;
    assert!(pair.through(0.0, 1.0, 0, 0.5).is_err());
    assert!(pair.through(0.0, 1.0, 1_000_000, 0.5).is_err());
    assert!(pair.through(0.0, f64::NAN, 3, 0.5).is_err());
    assert!(pair.gap_at(f64::INFINITY, 0.5).is_err());
    assert!(pair.gap_at(0.0, -1.0).is_err());
    let lost = Pair {
        second_start: f64::NAN,
        ..pair.clone()
    };
    assert!(lost.gap_at(0.0, 0.5).is_err());
    // Parts too far apart to be within reach: nothing is found, which is not an error.
    let apart = Pair {
        second: Part {
            axis: [500.0, 0.0],
            ..pair.second.clone()
        },
        ..pair
    };
    let gaps = apart.through_a_cycle(3, 0.5)?;
    assert!(gaps.least.is_none() && gaps.most.is_none());
    Ok(())
}

/// A turn whose steps times the points each measurement looks from would be more than
/// `MOST_TURNING_WORK` is refused before its first step, saying so, though each step on its own
/// could be measured; fewer steps of the same pair are measured.
#[test]
fn a_turn_of_too_many_steps_for_its_outlines_is_refused_before_it_starts() -> crate::Result<()> {
    let pair = spline_pair(&shaft(), &fit(), 1e-3)?;
    let points = pair.first.outline.points().len() + pair.second.outline.points().len();
    // Every point of both outlines is looked from at every step, so the most steps are too many for
    // these.
    assert!(
        points as f64 * f64::from(MAX_STEPS) > MOST_TURNING_WORK as f64,
        "{points} points"
    );
    let refused = pair.through(0.0, 1.0, MAX_STEPS, 0.5);
    assert!(why(&refused) == Some(Why::TooMuchTurning), "{refused:?}");
    assert!(pair.gap_at(0.0, 0.5).is_ok());
    assert!(pair.through_a_cycle(3, 0.5).is_ok());
    Ok(())
}

#[test]
fn a_tiny_reach_is_measured_whatever_it_is() -> crate::Result<()> {
    // Two 40-sided circles 0.5 apart; a reach of 1e-300 must not take seconds per call.
    let (a, b) = (
        Body::new(circle(0.0, 5.0, 40), Material::Inside)?,
        Body::new(circle(10.5, 5.0, 40), Material::Inside)?,
    );
    let (big_a, big_b) = (
        Body::new(circle(0.0, 5.0, 20_000), Material::Inside)?,
        Body::new(circle(10.5, 5.0, 20_000), Material::Inside)?,
    );
    for reach in [
        1e-300,
        f64::MIN_POSITIVE,
        1e-200,
        1e-30,
        1e-12,
        1e-9,
        2e-8,
        1e-7,
        1e-5,
        1e-3,
        0.4,
        0.6,
        5.0,
        1e5,
    ] {
        for (x, y) in [(&a, &b), (&big_a, &big_b)] {
            let found = gap(x, y, reach);
            match (out_of_range(&found), why(&found), &found) {
                // A reach finer than the numbers can tell apart at this size is refused, saying so.
                (Some("reach"), _, _) => {
                    assert!(reach < 2e-8, "reach {reach:e} refused");
                }
                // Fine outlines looked at from far: more comparisons than one measurement may make.
                (_, Some(Why::MeasureTooIntricate), _) => {
                    assert!(
                        x.points().len() > 1000 && reach >= 5.0,
                        "reach {reach:e}: {}",
                        Why::MeasureTooIntricate
                    );
                }
                (_, _, Err(e)) => panic!("reach {reach:e}: {e}"),
                (_, _, Ok(None)) => assert!(reach < 0.5 - 1e-3, "reach {reach:e}: nothing found"),
                (_, _, Ok(Some(g))) => assert!(
                    reach >= 0.49 && (g.distance - 0.5).abs() < 0.002,
                    "reach {reach:e}: {g:?}"
                ),
            }
        }
    }
    Ok(())
}

#[test]
fn a_tiny_reach_still_finds_overlaps_and_contact() -> crate::Result<()> {
    // With a reach far below the spacing of the points looked from along a chord, crossings are
    // still found.
    let across = bar(-50.0, -1.0, 50.0, 1.0)?;
    let upright = bar(-1.0, -50.0, 1.0, 50.0)?;
    for reach in [1e-7, 1e-6, 1e-4] {
        let found = gap(&across, &upright, reach)?.map_or(f64::NAN, |g| g.distance);
        assert!(
            found < 0.0 && found >= -reach - 1e-12,
            "reach {reach}: {found}"
        );
        // Touching side on side: nothing between them, found from the corners.
        let beside = bar(50.0, -1.0, 60.0, 1.0)?;
        let touching = gap(&across, &beside, reach)?.map_or(f64::NAN, |g| g.distance);
        assert!(touching.abs() < 1e-12, "reach {reach}: {touching}");
        // A hair apart, within the reach: the hair.
        let near = bar(50.0 + reach / 2.0, -1.0, 60.0, 1.0)?;
        let hair = gap(&across, &near, reach)?.map_or(f64::NAN, |g| g.distance);
        assert!((hair - reach / 2.0).abs() < 1e-12, "reach {reach}: {hair}");
    }
    Ok(())
}

#[test]
fn parts_that_are_not_at_a_finite_place_are_refused_by_the_measurement() -> crate::Result<()> {
    let square = Body::new(
        vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]],
        Material::Inside,
    )?;
    for (angle, axis) in [
        (f64::NAN, [0.0, 0.0]),
        (f64::INFINITY, [0.0, 0.0]),
        (0.0, [f64::NAN, 0.0]),
        (0.0, [0.0, f64::INFINITY]),
        (0.0, [f64::NEG_INFINITY, 0.0]),
        (0.3, [f64::MAX, f64::MAX]),
    ] {
        let lost = square.placed(angle, axis);
        for reach in [0.5, 1e5] {
            assert!(gap(&square, &lost, reach).is_err(), "{angle} {axis:?}");
            assert!(
                gap(&lost, &square, reach).is_err(),
                "{angle} {axis:?} the other way round"
            );
        }
    }
    // Through a pair: a first part's axis that is not at a finite place is refused too.
    let round = Outline::new(
        1,
        vec![Piece::Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start: 0.0,
            sweep: TAU,
            role: Role::Tip,
        }],
        1e-4,
    )?;
    let pair = Pair {
        first: Part {
            outline: round.clone(),
            material: Material::Inside,
            axis: [0.0, 0.0],
        },
        second: Part {
            outline: round,
            material: Material::Inside,
            axis: [10.5, 0.0],
        },
        motion: Motion::Turning { ratio: -1.0 },
        second_start: 0.0,
        cycle: 1.0,
    };
    assert!(pair
        .gap_at(0.0, 1.0)?
        .is_some_and(|g| (g.distance - 0.5).abs() < 1e-3));
    for v in [f64::NAN, f64::INFINITY, 1e300] {
        let lost = Pair {
            first: Part {
                axis: [v, 0.0],
                ..pair.first.clone()
            },
            ..pair.clone()
        };
        assert!(
            lost.gap_at(0.0, 1.0).is_err() && lost.through_a_cycle(2, 1.0).is_err(),
            "{v}"
        );
    }
    Ok(())
}

#[test]
fn a_pentagram_a_bow_tie_and_a_clockwise_square_are_refused() {
    let star: Vec<Point> = (0..5)
        .map(|k| {
            [
                10.0 * det::cos(TAU * f64::from(2 * k) / 5.0),
                10.0 * det::sin(TAU * f64::from(2 * k) / 5.0),
            ]
        })
        .collect();
    let lines = |p: &[Point]| -> Vec<Piece> {
        (0..p.len())
            .map(|k| Piece::Line {
                from: p[k],
                to: p[(k + 1) % p.len()],
                role: Role::Flank,
            })
            .collect()
    };
    assert!(crossing_refusal(&Outline::new(1, lines(&star), 0.01)));
    assert!(Body::new(star, Material::Inside).is_err());
    let bow = [[1.0, 1.0], [-1.0, -1.0], [-1.0, 1.0], [1.0, -1.0]];
    assert!(Outline::new(1, lines(&bow), 1e-3).is_err());
    let clockwise = [[1.0, 0.0], [0.0, -1.0], [-1.0, 0.0], [0.0, 1.0]];
    assert!(Outline::new(1, lines(&clockwise), 1e-3).is_err());
    // A chain that spirals in and crosses itself on the way out is refused as a body.
    let spiral = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [1.0, 4.0],
        [1.0, -1.0],
        [0.5, -1.0],
        [0.5, 3.0],
        [3.0, 3.0],
        [3.0, 1.0],
        [2.0, 1.0],
        [2.0, 5.0],
        [0.0, 5.0],
    ];
    assert!(Body::new(spiral.to_vec(), Material::Inside).is_err());
}

/// Every outline an involute shaft of plausible numbers is accepted with does not cross itself, and
/// every one refused as crossing itself does, the slow way. Shafts of few teeth, heavily undercut,
/// among them.
#[test]
fn involute_outlines_are_accepted_exactly_when_they_do_not_cross_themselves() {
    let mut dice = Dice(99);
    let (mut accepted, mut crossing) = (0, 0);
    let mut check = |g: External, tolerance: f64| {
        let outline = g.outline(tolerance);
        if let Ok(o) = &outline {
            accepted += 1;
            assert_eq!(
                self_crossings(&o.points()),
                0,
                "accepted but crossing itself: {g:?}"
            );
        }
        if crossing_refusal(&outline) {
            crossing += 1;
        }
    };
    // Shafts of few teeth and ordinary proportions, heavily undercut.
    for pressure_angle in [14.5, 17.5, 20.0] {
        for teeth in [6, 8, 10, 12] {
            for shift in [-0.6, -0.4, -0.2, 0.0] {
                for tool_tip_radius in [0.0, 0.3] {
                    for thinning in [0.0, 0.1] {
                        let g = External {
                            module: 2.0,
                            teeth,
                            pressure_angle,
                            addendum: 1.0,
                            dedendum: 1.25,
                            shift,
                            tool_tip_radius,
                            thinning,
                        };
                        check(g, 0.002);
                    }
                }
            }
        }
    }
    let x12 = External {
        module: 2.0,
        teeth: 6,
        pressure_angle: 14.5,
        addendum: 1.0,
        dedendum: 1.25,
        shift: -0.6,
        tool_tip_radius: 0.0,
        thinning: 0.0,
    };
    assert!(
        x12.outline(0.002).is_err(),
        "a shaft whose outline would cross itself in twelve places"
    );
    // Random sweeps over plausible ranges.
    let f = |d: &mut Dice, lo: f64, hi: f64| lo + (hi - lo) * (d.roll() % 10000) as f64 / 10000.0;
    for round in 0..3000 {
        let wide = round % 2 == 0;
        let module = f(&mut dice, 0.5, 4.0);
        let g = External {
            module,
            teeth: if wide {
                3 + (dice.roll() % 40) as u32
            } else {
                8 + (dice.roll() % 40) as u32
            },
            pressure_angle: if wide {
                f(&mut dice, 8.0, 40.0)
            } else {
                f(&mut dice, 14.0, 25.0)
            },
            addendum: f(&mut dice, 0.3, 1.8),
            dedendum: f(&mut dice, 0.5, 2.0),
            shift: if wide {
                f(&mut dice, -1.5, 2.0)
            } else {
                f(&mut dice, -0.5, 1.0)
            },
            tool_tip_radius: f(&mut dice, 0.0, 0.8),
            thinning: if wide {
                f(&mut dice, 0.0, 0.4)
            } else {
                f(&mut dice, 0.0, 0.15)
            },
        };
        check(g, 0.01 * module);
    }
    assert!(accepted > 1000, "{accepted} accepted");
    assert!(crossing > 0 || accepted > 0, "{crossing}");
}

// -------------------------------------------------------------------------------------------------
// The certificates of the chords (steps 4 and 5)
// -------------------------------------------------------------------------------------------------

/// Whether the interval holds `x`.
/// Whether `x` lies in `range` or within a few units in the last place of it: where a value of
/// the platform's functions is looked for beside a proved enclosure of the true one.
fn beside(range: Ivl, x: f64) -> bool {
    let slack = 8.0 * UNIT * (1.0 + x.abs());
    range.lo - slack <= x && x <= range.hi + slack
}

fn holds(range: Ivl, x: f64) -> bool {
    (range.lo..=range.hi).contains(&x)
}

/// The enclosures of `bound` hold what they enclose: sine and cosine over intervals of every width
/// at angles far from 0, the series of one angle, pi, squares, atan, the Taylor forms of a
/// quadratic and the chord bound of a circle's arc; and a curve sampled against the direction of
/// its chords keeps the bound across them.
#[test]
fn bound_enclosures_hold_dense_values() -> crate::Result<()> {
    let mut dice = Dice(0xB0D);
    let mut unit = || (dice.roll() % 1_000_001) as f64 / 1_000_000.0;
    let mut looked = 0_usize;
    for _ in 0..2000 {
        let centre = -1e4 + 2e4 * unit();
        let width = det::powf(10f64, -12.0 + 12.3 * unit());
        let (lo, hi) = (centre - 0.5 * width, centre + 0.5 * width);
        let (s, c) = bound::sin_cos(Ivl::new(lo, hi));
        for k in 0..100 {
            let t = (lo + (hi - lo) * f64::from(k) / 99.0).max(lo).min(hi);
            // The enclosure is proved; the platform's value is only good in practice, so it is
            // looked for within a few units in the last place of the enclosure.
            let (sine, cosine) = det::sin_cos(t);
            assert!(
                beside(s, sine) && beside(c, cosine),
                "at {t} in [{lo}, {hi}]: sin {sine} in {s:?}, cos {cosine} in {c:?}"
            );
            looked += 1;
        }
    }
    assert_eq!(looked, 200_000);
    // The sine, cosine and arc tangent of one number, from their series: the platform's values lie
    // beside them, they are close, and what has no enclosure is the whole range.
    for x in [
        0.0,
        -0.0,
        1e-300,
        f64::MIN_POSITIVE,
        -3.7e-5,
        0.5,
        -1.0,
        PI,
        1e300,
    ] {
        let (s, c) = bound::sin_cos_at(x);
        if x.abs() <= 1e6 {
            let (sine, cosine) = det::sin_cos(x);
            assert!(
                beside(s, sine) && beside(c, cosine),
                "{x:e}: sin {sine:e} in {s:?}, cos {cosine:e} in {c:?}"
            );
            assert!(
                s.hi - s.lo < 1e-9 && c.hi - c.lo < 1e-9,
                "{x:e}: {s:?} {c:?}"
            );
        } else {
            assert_eq!((s.lo, s.hi, c.lo, c.hi), (-1.0, 1.0, -1.0, 1.0), "{x:e}");
        }
        let a = bound::atan_at(x);
        assert!(
            beside(a, det::atan(x)),
            "atan {x:e}: {} in {a:?}",
            det::atan(x)
        );
        assert!(a.hi - a.lo < 1e-14, "atan {x:e}: {a:?}");
    }
    // Exact cases: the sine and cosine of 0, the arc tangent of 1 (a quarter of pi) and of what
    // has no end.
    let (s, c) = bound::sin_cos_at(0.0);
    assert!(holds(s, 0.0) && holds(c, 1.0), "{s:?} {c:?}");
    let quarter = bound::atan_at(1.0);
    assert!(
        quarter.lo <= next_up(PI) / 4.0 && quarter.hi >= PI / 4.0,
        "{quarter:?}"
    );
    let far = bound::atan_at(f64::INFINITY);
    assert!(far.lo <= FRAC_PI_2 && far.hi > FRAC_PI_2, "{far:?}");
    assert_eq!(bound::atan_at(f64::NAN).lo, f64::NEG_INFINITY);
    let (s, c) = bound::sin_cos_at(f64::NAN);
    assert_eq!((s.lo, s.hi, c.lo, c.hi), (-1.0, 1.0, -1.0, 1.0));
    // A point is measured against the box that holds the true one, and a distance is an upper
    // bound of it.
    let held = [Ivl::new(1.0, 2.0), Ivl::new(-1.0, 1.0)];
    let farthest_corner = bound::off([0.0, 0.0], held);
    assert!(
        farthest_corner >= 5f64.sqrt() && farthest_corner < 5f64.sqrt() * (1.0 + 1e-14),
        "{farthest_corner}"
    );
    assert_eq!(bound::off([f64::NAN, 0.0], held), f64::INFINITY);
    let length = bound::apart([3.0, 0.0], [0.0, 4.0]);
    assert!((5.0..5.0 * (1.0 + 1e-14)).contains(&length), "{length}");
    let pi = bound::pi();
    assert!(pi.lo == PI && pi.hi > PI, "{pi:?}");
    // Squares: never negative, and exactly 0 below where the interval holds 0.
    for (lo, hi) in [
        (-1.0, 2.0),
        (0.0, 0.0),
        (-3.0, 0.0),
        (0.0, 1e-200),
        (-1e-170, 1e-170),
    ] {
        let square = bound::sqr(Ivl::new(lo, hi));
        assert!(square.lo == 0.0, "[{lo}, {hi}]: {square:?}");
        for k in 0..=10 {
            let x = lo + (hi - lo) * f64::from(k) / 10.0;
            assert!(holds(square, x * x), "[{lo}, {hi}] at {x}: {square:?}");
        }
    }
    let square = bound::sqr(Ivl::new(2.0, 3.0));
    assert!(square.lo > 0.0, "{square:?}");
    assert!(holds(square, 4.0) && holds(square, 9.0), "{square:?}");
    assert_eq!(bound::sqr(Ivl::new(f64::NAN, 1.0)).hi, f64::INFINITY);
    // atan over an interval holds atan of every number in it.
    for (lo, hi) in [(-1e6, -3.0), (-0.5, 0.25), (2.0, 1e300)] {
        let a = bound::atan_over(Ivl::new(lo, hi));
        for k in 0..=50 {
            let x = lo + (hi - lo) * f64::from(k) / 50.0;
            assert!(holds(a, det::atan(x)), "{x}: {a:?}");
        }
    }
    let ray = bound::atan_over(Ivl::new(f64::NEG_INFINITY, 0.0));
    assert!(ray.lo < -FRAC_PI_2 && ray.hi > 0.0, "{ray:?}");
    assert_eq!(bound::atan_over(Ivl::new(f64::NAN, 0.0)).hi, f64::INFINITY);
    // The Taylor forms of a quadratic hold its values; every number here is exact in doubles.
    for (a, b, c) in [
        (0.75, 1.25, 2.0),
        (-4.0, -2.5, -3.0),
        (0.75, 0.0, 0.5),
        (-4.0, 1.25, -1.0),
    ] {
        let f = |x: f64| a + b * x + c * x * x;
        let value = |x: f64| {
            let x = bound::exact(x);
            bound::exact(a) + bound::exact(b) * x + bound::exact(c) * x * x
        };
        let slope = |x: f64| bound::exact(b) + bound::exact(2.0 * c) * bound::exact(x);
        let curvature = bound::exact(2.0 * c);
        for m in [-1.5, 0.0, 2.25] {
            for r in [0.125, 0.5, 1.0] {
                let centred = bound::taylor(value(m), slope(m), curvature, r);
                let (start, end) = (m - r, m + r);
                let forward =
                    bound::taylor_from_end(value(start), slope(start), curvature, 2.0 * r, true);
                let backward =
                    bound::taylor_from_end(value(end), slope(end), curvature, 2.0 * r, false);
                for k in -32..=32 {
                    let x = m + r * f64::from(k) / 32.0;
                    let y = f(x);
                    assert!(
                        holds(centred, y) && holds(forward, y) && holds(backward, y),
                        "{a} + {b} x + {c} x^2 at {x}: {y} in {centred:?} {forward:?} {backward:?}"
                    );
                }
            }
        }
    }
    // The chord bound of an arc: at least its sagitta and at most about twice it, never above
    // what the size of the second derivative alone gives, and the same when the stretch's ends
    // are swapped (a curve run against its chord).
    for radius in [0.01, 1.0, 250.0] {
        let (at, bend_of, off_of) = (circle_at(radius), circle_bend(radius), circle_off(radius));
        for (t0, h) in [(0.3_f64, 1e-6_f64), (-2.0, 1e-3), (1.0, 0.1), (4.0, 0.5)] {
            let t1 = t0 + h;
            let width = next_up((t1 - t0).abs());
            let (p0, p1, bend) = (at(t0), at(t1), bend_of(t0, t1));
            let e = off_of(t0, p0).max(off_of(t1, p1));
            let sagitta = 2.0 * radius * det::powi(det::sin(0.25 * (t1 - t0)), 2);
            let by_size = width * width / 8.0 * bend.second_size + e;
            let proved = bound::chord_bound(width, p0, p1, e, &bend);
            let swapped = bound::chord_bound(width, p1, p0, e, &bend);
            let label = format!("radius {radius}, from {t0} by {h}");
            assert!(
                proved >= sagitta && proved <= by_size * (1.0 + 1e-12),
                "{label}: by size {by_size:e}, proved {proved:e}, sagitta {sagitta:e}"
            );
            assert!(
                proved <= 2.0 * (sagitta + e),
                "{label}: proved {proved:e}, sagitta {sagitta:e}"
            );
            assert_eq!(swapped.to_bits(), proved.to_bits(), "{label}");
        }
    }
    // A curve sampled from the larger parameter to the smaller runs against its chords and is
    // bounded across them all the same: as many points either way, every chord within the
    // tolerance.
    let (radius, tolerance) = (3.0, 1e-4);
    let (at, bend, off) = (circle_at(radius), circle_bend(radius), circle_off(radius));
    let forward = lift(sampled(&at, &off, &bend, 0.3, 1.7, tolerance))?;
    let backward = lift(sampled(&at, &off, &bend, 1.7, 0.3, tolerance))?;
    assert_eq!(forward.len(), backward.len());
    for chain in [&forward, &backward] {
        let far = farthest(chain, &at, 0.3, 1.7, 10_000);
        assert!(
            far <= tolerance + 1e-12,
            "{far} with {} points",
            chain.len()
        );
    }
    // A parabola run along a straight line: its second derivative lies along every chord, so the
    // part of it across a chord is nothing and the first eight chords are proved either way, where
    // the size of the second derivative alone would not prove one of them.
    let line = |t: f64| [t * t, 0.5 * (t * t)];
    let line_bend = |lo: f64, hi: f64| {
        let t = Ivl::new(lo, hi);
        Bend {
            first: [bound::exact(2.0) * t, t],
            second: [bound::exact(2.0), bound::exact(1.0)],
            second_size: 5f64.sqrt() * (1.0 + 1e-15),
        }
    };
    // A product and a halving: correctly rounded, so the point's error is counted.
    let line_off = |t: f64, _: Point| 8.0 * UNIT * t * t;
    let forward = lift(sampled(&line, &line_off, &line_bend, 1.0, 2.0, 1e-6))?;
    let backward = lift(sampled(&line, &line_off, &line_bend, 2.0, 1.0, 1e-6))?;
    assert_eq!((forward.len(), backward.len()), (9, 9));
    let by_size = 0.125 * 0.125 / 8.0 * line_bend(1.0, 1.125).second_size;
    assert!(by_size > 1e3 * 1e-6, "{by_size:e}");
    Ok(())
}

/// Step 5: on every span of random splines of every degree, the hodograph boxes of `span_bend`
/// hold the first and second derivatives, measured by central differences (Richardson's, so that
/// the differences' own error stays below the slack), and the rounding is positive.
#[test]
fn span_bends_hold_the_spline() -> crate::Result<()> {
    let mut dice = Dice(0x5BE);
    let mut looked = 0_usize;
    for round in 0..180 {
        let degree = 1 + round % bspline::MAX_DEGREE;
        let spans = 1 + dice.roll() % 4;
        let (knots, control) = random_spline(&mut dice, degree, spans, round % 2 == 0, 1.0);
        let point = |t: f64| bspline::point(degree, &knots, &control, t);
        let (h1, h2) = (
            det::powi(2f64, -12),
            if degree <= 5 {
                det::powi(2f64, -7)
            } else {
                det::powi(2f64, -9)
            },
        );
        for span in degree..control.len() {
            let (a, b) = (knots[span], knots[span + 1]);
            if !(a < b) {
                continue;
            }
            let bend = bspline::span_bend(degree, &knots, &control, span);
            let rounding = bspline::span_rounding(degree, &control, span);
            assert!(
                rounding > 0.0 && rounding.is_finite(),
                "degree {degree}, span {span}: {rounding}"
            );
            // The first and second derivative at `t`, by Richardson's central differences.
            let derivatives = |t: f64| -> crate::Result<(Point, Point)> {
                let p = point(t)?;
                let first_at = |h: f64| -> crate::Result<Point> {
                    let (u, v) = (point(t + h)?, point(t - h)?);
                    Ok([(u[0] - v[0]) / (2.0 * h), (u[1] - v[1]) / (2.0 * h)])
                };
                let second_at = |h: f64| -> crate::Result<Point> {
                    let (u, v) = (point(t + h)?, point(t - h)?);
                    Ok([
                        (u[0] - 2.0 * p[0] + v[0]) / (h * h),
                        (u[1] - 2.0 * p[1] + v[1]) / (h * h),
                    ])
                };
                let (d1, d2) = (first_at(h1)?, first_at(h1 / 2.0)?);
                let (s1, s2) = (second_at(h2)?, second_at(h2 / 2.0)?);
                Ok((
                    [0, 1].map(|i| (4.0 * d2[i] - d1[i]) / 3.0),
                    [0, 1].map(|i| (4.0 * s2[i] - s1[i]) / 3.0),
                ))
            };
            // The whole span, then parts of it (`stretch_bend`): the whole again from its own
            // Bezier points, a quarter of it, and a thousandth about its middle. A part's boxes
            // lie inside the span's and hold the derivatives on the part.
            let width = b - a;
            let parts = [
                (a, b, bend),
                (
                    a,
                    b,
                    bspline::stretch_bend(degree, &knots, &control, span, a, b),
                ),
                (
                    a + 0.1 * width,
                    a + 0.35 * width,
                    bspline::stretch_bend(
                        degree,
                        &knots,
                        &control,
                        span,
                        a + 0.1 * width,
                        a + 0.35 * width,
                    ),
                ),
                (
                    a + 0.4995 * width,
                    a + 0.5005 * width,
                    bspline::stretch_bend(
                        degree,
                        &knots,
                        &control,
                        span,
                        a + 0.4995 * width,
                        a + 0.5005 * width,
                    ),
                ),
            ];
            for (lo, hi, part) in parts {
                assert!(
                    part.second_size <= bend.second_size,
                    "degree {degree}, span {span}, [{lo}, {hi}]: {part:?} against {bend:?}"
                );
                for (inner, outer) in part
                    .first
                    .into_iter()
                    .chain(part.second)
                    .zip(bend.first.into_iter().chain(bend.second))
                {
                    assert!(
                        inner.lo >= outer.lo && inner.hi <= outer.hi,
                        "degree {degree}, span {span}, [{lo}, {hi}]: {inner:?} in {outer:?}"
                    );
                }
                for k in 0..50 {
                    let t = lo + (hi - lo) * (f64::from(k) + 0.5) / 50.0;
                    // The differences need room on both sides inside the span.
                    if t < a + width / 16.0 || t > b - width / 16.0 {
                        continue;
                    }
                    let (first, second) = derivatives(t)?;
                    for (values, boxes, name) in [
                        (first, part.first, "first"),
                        (second, part.second, "second"),
                    ] {
                        for (v, iv) in values.into_iter().zip(boxes) {
                            let slack = 1e-7 * iv.mag() + 1e-9;
                            assert!(
                                (iv.lo - slack..=iv.hi + slack).contains(&v),
                                "degree {degree}, span {span}, [{lo}, {hi}] at {t}: {name} {v} \
                                 outside {iv:?}"
                            );
                            looked += 1;
                        }
                    }
                    let size = det::hypot(second[0], second[1]);
                    assert!(
                        size <= part.second_size * (1.0 + 1e-7) + 1e-9,
                        "degree {degree}, span {span}, [{lo}, {hi}] at {t}: {size} above {}",
                        part.second_size
                    );
                }
            }
        }
    }
    assert!(looked > 100_000, "{looked}");
    // What is not a part of its span proves nothing.
    let (degree, knots, control) = arc_like();
    let (a, b) = (knots[degree], knots[degree + 1]);
    for (lo, hi) in [
        (a, a),
        (b, a),
        (a - 1e-9, b),
        (a, b + 1e-9),
        (f64::NAN, b),
        (a, f64::INFINITY),
    ] {
        let none = bspline::stretch_bend(degree, &knots, &control, degree, lo, hi);
        assert_eq!(none.second_size, f64::INFINITY, "[{lo}, {hi}]: {none:?}");
    }
    Ok(())
}

/// Step 5, the counterexample to probing alone: a curve that is zero at every place the old
/// sampling looks at in its first eighth, with a bump of ten tolerances between them. The old
/// sampling takes one chord there and misses the bump; the proved chain does not. (A bump of ten
/// tolerances, not a hundred: with the span's whole hodograph as the bound, this degree 5 curve,
/// which grows like `t^5` past its first eighth, would need more than `MAX_CURVE_POINTS` chords
/// for a bump of a hundred.)
#[test]
fn a_bump_between_probes_is_caught() -> crate::Result<()> {
    const BUMP: f64 = 10.0;
    let tolerance = 1e-4;
    let a = 1.0 / 32.0;
    let g = |t: f64| t * (t - a) * (t - 2.0 * a) * (t - 3.0 * a) * (t - 4.0 * a);
    let peak = (0..=80_000)
        .map(|i| g(f64::from(i) / 640_000.0).abs())
        .fold(0.0, f64::max);
    let c = BUMP * tolerance / peak;
    // g in powers of t, from t^0 to t^5, then in Bernstein form on [0, 1]; x(t) = t.
    let power = [
        0.0,
        24.0 * det::powi(a, 4),
        -50.0 * det::powi(a, 3),
        35.0 * a * a,
        -10.0 * a,
        1.0,
    ];
    let choose = |n: usize, k: usize| {
        (0..k).fold(1.0, |product, i| product * (n - i) as f64 / (i + 1) as f64)
    };
    let control: Vec<Point> = (0..=5)
        .map(|j| {
            let y: f64 = (0..=j)
                .map(|k| choose(j, k) / choose(5, k) * power[k])
                .sum();
            [j as f64 / 5.0, c * y]
        })
        .collect();
    let knots = [[0.0; 6], [1.0; 6]].concat();
    let spline = lift(bspline::Checked::new(5, &knots, &control))?;
    let at = |t: f64| spline.point(t).unwrap_or([f64::NAN, f64::NAN]);
    for i in 0..=64 {
        let t = f64::from(i) / 64.0;
        let [x, y] = at(t);
        assert!(
            (x - t).abs() < 1e-12 && (y - c * g(t)).abs() <= 1e-9 * (1.0 + y.abs()),
            "at {t}: {x}, {y} for {}",
            c * g(t)
        );
    }
    let old = lift(old_sampled(&at, 0.0, 1.0, tolerance))?;
    let missed = (0..20_000)
        .map(|k| at(0.125 * f64::from(k) / 19_999.0))
        .map(|p| near_graph(&old, p, 2.0 * tolerance))
        .fold(0.0, f64::max);
    assert!(
        missed > tolerance,
        "the old sampling came within {missed:e}"
    );
    let piece = Piece::Spline {
        degree: 5,
        knots: knots.clone(),
        control: control.clone(),
        role: Role::Flank,
    };
    let mut new = lift(piece.chain(tolerance))?;
    new.extend(piece.end());
    let worst = (0..20_000)
        .map(|k| at(f64::from(k) / 19_999.0))
        .map(|p| near_graph(&new, p, 2.0 * tolerance))
        .fold(0.0, f64::max);
    assert!(
        worst <= tolerance + 1e-9,
        "{worst:e} with {} points, against {} the old way",
        new.len(),
        old.len()
    );
    Ok(())
}

/// The counts of the step 5 oracle: per span, the proved chain against the old sampling.
#[derive(Default)]
struct Growth {
    /// Spans compared.
    spans: usize,
    /// Spans where the proved chain has more points.
    grew: usize,
    /// Spans the old sampling refused (too many points), left out.
    skipped: usize,
    /// The largest growth, new over old, and its degree.
    largest: (f64, usize),
    /// Spans the proved chain refused.
    refused: Vec<String>,
    /// Spans where the proved chain has fewer points than the old one.
    coarser: Vec<String>,
    /// Spans where the proved chain has more than four times the old points and sixteen.
    too_many: Vec<String>,
}

impl Growth {
    /// Every span of a spline at a tolerance: the old sampling and the proved chain.
    fn spans_of(
        &mut self,
        name: &str,
        degree: usize,
        knots: &[f64],
        control: &[Point],
        tolerance: f64,
    ) -> crate::Result<()> {
        let spline = lift(bspline::Checked::new(degree, knots, control))?;
        let at = |t: f64| spline.point(t).unwrap_or([f64::NAN, f64::NAN]);
        for stretch in bspline::breaks(degree, knots, control)?.windows(2) {
            let (from, to) = (stretch[0], stretch[1]);
            let Ok(old) = old_sampled(&at, from, to, tolerance) else {
                self.skipped += 1;
                continue;
            };
            let near = stretch_bends(degree, knots, control, from);
            let rounding = break_rounding(degree, knots, control, from, to);
            let new = sampled(&at, &|_, _| rounding, &near, from, to, tolerance);
            self.spans += 1;
            let label = format!("{name}, degree {degree}, at {tolerance:e} over [{from}, {to}]");
            match new {
                Err(e) => self.refused.push(format!("{label}: {e}")),
                Ok(new) => {
                    let (n, o) = (new.len(), old.len());
                    if n < o {
                        self.coarser.push(format!("{label}: {n} against {o}"));
                    }
                    if n > 4 * o + 16 {
                        self.too_many.push(format!("{label}: {n} against {o}"));
                    }
                    if n != o {
                        self.grew += 1;
                    }
                    let growth = n as f64 / o as f64;
                    if growth > self.largest.0 {
                        self.largest = (growth, degree);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Step 5 oracle: on every span of an arc-like spline, random splines of degree 2 to 9 and the
/// fitted splines of three involute outlines, the proved chain is never coarser than the old
/// sampling (the probes still nominate, so every old split is made), at most four times as fine
/// and sixteen, and never refused.
#[test]
fn spline_chains_are_never_coarser() -> crate::Result<()> {
    const TOLERANCES: [f64; 3] = [1e-2, 1e-4, 1e-6];
    let mut growth = Growth::default();
    let (degree, knots, control) = arc_like();
    for tolerance in TOLERANCES {
        growth.spans_of("arc-like", degree, &knots, &control, tolerance)?;
    }
    let mut dice = Dice(0x5C5);
    for round in 0..300 {
        let degree = 2 + round % 8;
        let spans = 1 + dice.roll() % 4;
        let (knots, control) = random_spline(&mut dice, degree, spans, round % 2 == 0, 0.1);
        for tolerance in TOLERANCES {
            growth.spans_of("random", degree, &knots, &control, tolerance)?;
        }
    }
    let shaft_of = |module: f64, teeth: u32| External {
        module,
        teeth,
        pressure_angle: 20.0,
        addendum: 1.0,
        dedendum: 1.25,
        shift: 0.0,
        tool_tip_radius: 0.3,
        thinning: 0.0,
    };
    let sources = [
        shaft_of(2.0, 20).outline(1e-5)?,
        shaft_of(3.0, 7).outline(1e-5)?,
        hub_for(&shaft(), &fit())?.outline(1e-5)?,
    ];
    let mut pieces = 0;
    for source in &sources {
        let splines = fitted(source, 1e-3)?;
        for piece in splines.tooth() {
            if let Piece::Spline {
                degree,
                knots,
                control,
                ..
            } = piece
            {
                pieces += 1;
                for tolerance in TOLERANCES {
                    growth.spans_of("fitted", *degree, knots, control, tolerance)?;
                }
            }
        }
    }
    assert!(
        pieces > 0 && growth.spans > 1000,
        "{pieces} {}",
        growth.spans
    );
    assert!(
        growth.refused.is_empty() && growth.coarser.is_empty() && growth.too_many.is_empty(),
        "refused: {:?}; coarser: {:?}; more than 4 old + 16: {:?}; largest growth {:.2} at degree \
         {}; {} of {} spans grew ({} the old way could not sample)",
        growth.refused,
        growth.coarser,
        growth.too_many,
        growth.largest.0,
        growth.largest.1,
        growth.grew,
        growth.spans,
        growth.skipped
    );
    Ok(())
}

/// The arcs: the proved count against today's rule. Never more than one chord fewer; never fewer
/// where the tolerance is at least a millionth of the radius (but where today's quotient lies
/// within 1e-9 of a whole number); no arc that builds today refused where its own points can be
/// placed within their allowance (`tolerance >= 1e-13 radius`; below that nothing about
/// the arc can be proved, and the refusal is `NotProved`); and every chord within the tolerance
/// of 10,000 points of the circle.
#[test]
fn arc_counts_against_the_old_rule() {
    let (mut fewer, mut refused, mut far) = (Vec::new(), Vec::new(), Vec::new());
    let mut proved = 0_usize;
    for radius in [1e-3, 1.0, 10.0, 1e3, 1e5] {
        for sweep in [1e-6, 0.1, 1.0, PI, TAU] {
            for tolerance in [1e-12, 1e-9, 1e-6, 1e-3, 1.0, 10.0] {
                let old = old_arc_steps(radius, sweep, tolerance);
                let label = format!("radius {radius:e}, sweep {sweep}, tolerance {tolerance:e}");
                let n = match arc_steps([0.0, 0.0], radius, 0.0, sweep, tolerance) {
                    Ok(n) => n,
                    Err(e) => {
                        assert!(
                            matches!(e, TeethFailure::NotProved { what, .. } if what == CHORD),
                            "{label}: {e:?}"
                        );
                        if old <= MAX_CURVE_POINTS && tolerance >= 1e-13 * radius {
                            refused.push(label);
                        }
                        continue;
                    }
                };
                proved += 1;
                assert!(n + 1 >= old, "{label}: {n} against {old}");
                let ratio = tolerance / radius;
                let step = 2.0 * det::acos(1.0 - ratio.clamp(1e-15, 1.0));
                let quotient = sweep / step;
                if ratio >= 1e-6 && (quotient - quotient.round()).abs() >= 1e-9 && n < old {
                    fewer.push(format!("{label}: {n} against {old}"));
                }
                if n > MAX_CURVE_POINTS {
                    continue;
                }
                // The chain as `Piece::chain` makes it, with the arc's end.
                let mut chain: Vec<Point> = (0..n)
                    .map(|i| on_circle([0.0, 0.0], radius, sweep * (i as f64) / (n as f64)))
                    .collect();
                chain.push(on_circle([0.0, 0.0], radius, sweep));
                let worst = (0..10_000)
                    .map(|k| {
                        let share = f64::from(k) / 9_999.0;
                        let p = on_circle([0.0, 0.0], radius, sweep * share);
                        let i = ((share * n as f64) as usize).min(n - 1);
                        (i.saturating_sub(1)..(i + 2).min(n))
                            .map(|j| off_chord(p, chain[j], chain[j + 1]))
                            .fold(f64::INFINITY, f64::min)
                    })
                    .fold(0.0, f64::max);
                if !(worst <= tolerance + 16.0 * UNIT * radius) {
                    far.push(format!("{label}: {worst:e} with {n} chords"));
                }
            }
        }
    }
    assert!(
        fewer.is_empty() && refused.is_empty() && far.is_empty(),
        "fewer chords than today: {fewer:?}; refused where today builds: {refused:?}; chords too \
         far: {far:?}"
    );
    assert!(proved > 100, "{proved}");
}

/// Hostile bends and numbers: the proved sampling, the arc count and the span bounds refuse or
/// prove nothing, saying why, and never panic.
#[test]
fn hostile_bends_refuse() {
    fn chord_unproved(r: &Result<Vec<Point>, TeethFailure>) -> bool {
        matches!(r, Err(TeethFailure::NotProved { what, .. }) if *what == CHORD)
    }
    let unit = circle_at(1.0);
    let bend = circle_bend(1.0);
    let off = circle_off(1.0);
    let knows_nothing = |_: f64, _: f64| Bend::unknown();
    let refused = sampled(&unit, &off, &knows_nothing, 0.0, 1.0, 1e-3);
    assert!(chord_unproved(&refused), "{refused:?}");
    // A point whose distance from the curve is not known proves nothing either.
    for unknown in [f64::NAN, f64::INFINITY] {
        let refused = sampled(&unit, &|_, _| unknown, &bend, 0.0, 1.0, 1e-3);
        assert!(chord_unproved(&refused), "{unknown}: {refused:?}");
    }
    // Nor does a platform whose sine is wrong by more than the tolerance pass for right: its
    // points are measured, and the curve is refused.
    let wrong = |t: f64| [det::cos(t), det::sin(t) + 2e-3];
    let refused = sampled(&wrong, &off, &bend, 0.0, 1.0, 1e-3);
    assert!(chord_unproved(&refused), "{refused:?}");
    let lost = |t: f64| if t > 0.5 { [f64::NAN, 0.0] } else { unit(t) };
    let refused = sampled(&lost, &off, &bend, 0.0, 1.0, 1e-3);
    assert_eq!(
        typed_why(&refused),
        Some(Why::OutlineNotFinite),
        "{refused:?}"
    );
    let refused = sampled(&unit, &off, &bend, 0.0, 1.0, 1e-300);
    assert!(
        matches!(refused, Err(TeethFailure::NotProved { what, reached, .. })
            if what == CHORD && reached > 1e-300),
        "{refused:?}"
    );
    for (from, to, tolerance) in [
        (f64::NAN, 1.0, 1e-3),
        (0.0, f64::INFINITY, 1e-3),
        (0.0, 1.0, f64::NAN),
        (0.0, 1.0, -1.0),
        (0.0, 1.0, 0.0),
    ] {
        let refused = sampled(&unit, &off, &bend, from, to, tolerance);
        assert!(refused.is_err(), "{from} {to} {tolerance}: {refused:?}");
    }
    for (centre, radius, start, sweep, tolerance) in [
        ([f64::NAN, 0.0], 1.0, 0.0, 1.0, 1e-3),
        ([0.0, 0.0], f64::INFINITY, 0.0, 1.0, 1e-3),
        ([0.0, 0.0], f64::NAN, 0.0, 1.0, 1e-3),
        ([0.0, 0.0], -1.0, 0.0, 1.0, 1e-3),
        ([0.0, 0.0], 1.0, f64::INFINITY, 1.0, 1e-3),
        ([0.0, 0.0], 1.0, 0.0, f64::NAN, 1e-3),
        ([0.0, 0.0], 1.0, 0.0, 1.0, f64::NAN),
        ([0.0, 0.0], 1.0, 0.0, 1.0, 1e-300),
    ] {
        let refused = arc_steps(centre, radius, start, sweep, tolerance);
        assert!(
            matches!(refused, Err(TeethFailure::NotProved { what, .. }) if what == CHORD),
            "{centre:?} {radius} {start} {sweep} {tolerance}: {refused:?}"
        );
    }
    let (degree, knots, control) = arc_like();
    for (degree, span) in [
        (degree, 0),
        (degree, 2),
        (degree, control.len()),
        (degree, usize::MAX),
        (0, 3),
        (bspline::MAX_DEGREE + 1, 3),
    ] {
        let unknown = bspline::span_bend(degree, &knots, &control, span);
        let rounding = bspline::span_rounding(degree, &control, span);
        assert!(
            rounding == f64::INFINITY && unknown.second_size == f64::INFINITY,
            "degree {degree}, span {span}: {rounding} {unknown:?}"
        );
    }
}

/// What a reading of the proved sampling found, each kept from coming back. An end point that is
/// not a number proves nothing, in either form of the chord bound. The last point of a sampled
/// curve is the curve at `to` itself, not at a rounded sum that only comes close to it. And the
/// gap that a piece's closing chord bridges to the next piece's start is taken off the piece's
/// tolerance, so a tolerance that the gap alone uses up is refused, not claimed.
#[test]
fn ends_gaps_and_what_is_not_a_number_are_counted() -> crate::Result<()> {
    let (unit, bend, off) = (circle_at(1.0), circle_bend(1.0), circle_off(1.0));
    let known = bend(0.0, 0.1);
    for (p0, p1) in [
        ([f64::NAN, 0.0], [1.0, 0.0]),
        ([1.0, 0.0], [0.0, f64::INFINITY]),
    ] {
        let proved = bound::chord_bound(0.1, p0, p1, 1e-15, &known);
        assert_eq!(proved, f64::INFINITY, "{p0:?} {p1:?}");
    }
    // Nor does an end whose distance from the curve is not a number, or is below nothing.
    for e in [f64::NAN, -1e-15] {
        let proved = bound::chord_bound(0.1, unit(0.0), unit(0.1), e, &known);
        assert_eq!(proved, f64::INFINITY, "{e}");
    }
    // 0.2 + (0.9 - 0.2) * 8 / 8 is not 0.9 in doubles, so eight even pieces end beside `to`.
    let (from, to) = (0.2_f64, 0.9_f64);
    assert_ne!(from + (to - from) * 8.0 / 8.0, to);
    let points = lift(sampled(&unit, &off, &bend, from, to, 1e-6))?;
    assert_eq!(points.first(), Some(&unit(from)));
    assert_eq!(points.last(), Some(&unit(to)));
    // No gap leaves the tolerance as it is; a gap takes itself off, rounded down.
    assert_eq!(less_gap(1e-3, 0.0), Ok(1e-3));
    let left = less_gap(1e-3, 1e-4);
    assert!(
        matches!(left, Ok(x) if x <= 9e-4 && x > 9e-4 * (1.0 - 1e-12)),
        "{left:?}"
    );
    for (tolerance, gap) in [
        (1e-9, 1e-9),
        (1e-9, 2e-9),
        (1e-3, f64::NAN),
        (1e-3, f64::INFINITY),
        (1e-3, -1e-4),
    ] {
        let refused = less_gap(tolerance, gap);
        assert!(
            matches!(refused, Err(TeethFailure::NotProved { what, .. }) if what == CHORD),
            "{tolerance} {gap}: {refused:?}"
        );
    }
    // A square of four sides, each ending 4e-10 short of the next (within the join allowance of
    // 2e-9): accepted at a tolerance above that gap and refused at one below it, by the outline
    // and by its points alike.
    let side = || {
        vec![Piece::Line {
            from: [1.0, -1.0],
            to: [1.0, 1.0 - 4e-10],
            role: Role::Tip,
        }]
    };
    assert!(Outline::new(4, side(), 1e-6).is_ok());
    assert!(points_of(4, &side(), 1e-6, 0.0).is_ok());
    let refused = Outline::new(4, side(), 1e-10);
    assert_eq!(not_proved(&refused), Some(CHORD), "{refused:?}");
    let refused = points_of(4, &side(), 1e-10, 0.0);
    assert_eq!(not_proved(&refused), Some(CHORD), "{refused:?}");
    Ok(())
}

/// A curve given as points keeps a share of the tolerance for the gap its closing chord bridges,
/// and an outline holds it to that. A sampled arc that stops a little short of where the next
/// tooth starts is accepted while the gap fits in what the curve's own chords leave of the
/// tolerance (a sixty-fourth), and refused, as not proved, for a gap that is below the tolerance
/// but above that share: its closing chord could lie farther from the curve than the tolerance.
/// The same for the outline's points; and the accepted outline's closing chord is measured against
/// the arc.
#[test]
fn a_curve_keeps_room_for_the_gap_its_closing_chord_bridges() -> crate::Result<()> {
    let (radius, teeth, tolerance) = (0.1_f64, 64_u32, 4e-9_f64);
    let pitch = TAU / f64::from(teeth);
    let (at, off, bend) = (circle_at(radius), circle_off(radius), circle_bend(radius));
    // One tooth: the arc of a pitch, stopping `gap` short of the next tooth's first point, its
    // chords proved to the share of the tolerance a curve given as points has.
    let tooth = |gap: f64| -> crate::Result<(Vec<Piece>, f64)> {
        let to = pitch - gap / radius;
        let points = lift(sampled(
            &at,
            &off,
            &bend,
            0.0,
            to,
            curve_tolerance(tolerance),
        ))?;
        let piece = Piece::Curve {
            points,
            role: Role::Tip,
        };
        Ok((vec![piece], to))
    };
    // Both gaps are of pieces that join (the join allowance here is 1e-9 (1 + 0.1)), and both are
    // below the tolerance; one fits in the sixty-fourth the curve leaves, the other does not.
    let share = tolerance / 64.0;
    let (fits, too_wide) = (0.5 * share, 8.0 * share);
    assert!(too_wide < tolerance && too_wide < 1e-9, "{too_wide:e}");
    let (pieces, to) = tooth(fits)?;
    assert!(points_of(teeth, &pieces, tolerance, 0.0).is_ok());
    let outline = Outline::new(teeth, pieces, tolerance)?;
    let (pieces, _) = tooth(too_wide)?;
    let refused = points_of(teeth, &pieces, tolerance, 0.0);
    assert_eq!(not_proved(&refused), Some(CHORD), "{refused:?}");
    let refused = Outline::new(teeth, pieces, tolerance);
    assert_eq!(not_proved(&refused), Some(CHORD), "{refused:?}");
    // The accepted outline's closing chord runs from the arc's last point but one to the next
    // tooth's first point: every point of it is within the tolerance of the arc, whose nearest
    // point is at the same angle, or its end for a point past the end.
    let points = outline.points();
    let per_tooth = points.len() / teeth as usize;
    let (a, b) = (points[per_tooth - 1], points[per_tooth]);
    for k in 0..=200 {
        let s = f64::from(k) / 200.0;
        let c = [a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1])];
        let angle = det::atan2(c[1], c[0]).min(to);
        let far = distance(c, at(angle));
        assert!(far <= tolerance, "{far:e} at {s} of the closing chord");
    }
    Ok(())
}
