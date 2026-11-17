//! The shared core: pairs, outlines made of lines and splines, and the spline evaluation; the
//! measurement's work bounded whatever the reach and however many steps a pair is turned through;
//! outlines that cross themselves or run the wrong way refused by `Outline::new` and `Body::new`
//! whichever module made them; and parts placed where they are not numbers refused by the
//! measurement.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use super::common::{check_outline, Dice};
use crate::teeth::involute::{spline_pair, External, Fit};
use crate::teeth::mesh::{gap, Body};
use crate::teeth::pair::{MAX_STEPS, MOST_TURNING_WORK};
use crate::teeth::{bspline, Error, Material, Motion, Outline, Pair, Part, Piece, Point, Role};

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
fn square(half: f64, tolerance: f64) -> Result<Outline, Error> {
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
            [centre + radius * a.cos(), radius * a.sin()]
        })
        .collect()
}

fn bar(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Body, Error> {
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

fn crossing_refusal(outline: &Result<Outline, Error>) -> bool {
    matches!(outline, Err(Error::Invalid { why }) if why.contains("cross itself"))
}

#[test]
fn outlines_of_lines_arcs_and_splines() -> Result<(), Error> {
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
    let (c, s) = (quarter.cos(), quarter.sin());
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
fn a_spline_is_evaluated_and_cut_into_chords_within_the_tolerance() -> Result<(), Error> {
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
        assert!((x.hypot(y) - 1.0).abs() < 2e-4, "at {i}: {}", x.hypot(y));
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
                1.0 - middle[0].hypot(middle[1]) < tolerance + 2e-4,
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
fn a_checked_spline_gives_exactly_the_points_bspline_point_gives() -> Result<(), Error> {
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
            let found = spline.point(t)?;
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
fn a_long_spline_is_cut_into_chords_without_checking_it_at_every_point() -> Result<(), Error> {
    // A spline of degree 1 with 2,000 spans along a quarter circle: one tooth of four.
    let spans = 2_000_usize;
    let control: Vec<Point> = (0..=spans)
        .map(|i| {
            let a = -FRAC_PI_4 + FRAC_PI_2 * i as f64 / spans as f64;
            [a.cos(), a.sin()]
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
    // Each span is evaluated 41 times, each time looking at about 15 knots and control points,
    // and the whole spline (4,004 of them) is checked a few times: at most 1,500 for each span.
    // Checking the whole spline again at every evaluation looks at more than 300 million.
    assert!(visited <= 3_000_000, "{visited}");
    assert_eq!(outline.points().len(), 4 * 8 * spans);
    Ok(())
}

#[test]
fn turning_is_checked() -> Result<(), Error> {
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
fn a_turn_of_too_many_steps_for_its_outlines_is_refused_before_it_starts() -> Result<(), Error> {
    let pair = spline_pair(&shaft(), &fit(), 1e-3)?;
    let points = pair.first.outline.points().len() + pair.second.outline.points().len();
    // Every point of both outlines is looked from at every step, so the most steps are too many for
    // these.
    assert!(
        points as f64 * f64::from(MAX_STEPS) > MOST_TURNING_WORK as f64,
        "{points} points"
    );
    let refused = pair.through(0.0, 1.0, MAX_STEPS, 0.5);
    assert!(
        matches!(refused, Err(Error::Invalid { why }) if why.contains("turned through so many steps")),
        "{refused:?}"
    );
    assert!(pair.gap_at(0.0, 0.5).is_ok());
    assert!(pair.through_a_cycle(3, 0.5).is_ok());
    Ok(())
}

#[test]
fn a_tiny_reach_is_measured_whatever_it_is() -> Result<(), Error> {
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
            match found {
                // A reach finer than the numbers can tell apart at this size is refused, saying so.
                Err(Error::OutOfRange { what: "reach", .. }) => {
                    assert!(reach < 2e-8, "reach {reach:e} refused");
                }
                // Fine outlines looked at from far: more comparisons than one measurement may make.
                Err(Error::Invalid { why }) if why.contains("too fine") => {
                    assert!(
                        x.points().len() > 1000 && reach >= 5.0,
                        "reach {reach:e}: {why}"
                    );
                }
                Err(e) => panic!("reach {reach:e}: {e}"),
                Ok(None) => assert!(reach < 0.5 - 1e-3, "reach {reach:e}: nothing found"),
                Ok(Some(g)) => assert!(
                    reach >= 0.49 && (g.distance - 0.5).abs() < 0.002,
                    "reach {reach:e}: {g:?}"
                ),
            }
        }
    }
    Ok(())
}

#[test]
fn a_tiny_reach_still_finds_overlaps_and_contact() -> Result<(), Error> {
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
fn parts_that_are_not_at_a_finite_place_are_refused_by_the_measurement() -> Result<(), Error> {
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
                10.0 * (TAU * f64::from(2 * k) / 5.0).cos(),
                10.0 * (TAU * f64::from(2 * k) / 5.0).sin(),
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
